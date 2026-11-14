pub mod ui;

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::{albums, copier, device, scanner, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    YearsMonths,
    Albums,
    DcimFolders,
}

impl Tab {
    pub fn next(self) -> Self {
        match self {
            Self::YearsMonths => Self::Albums,
            Self::Albums => Self::DcimFolders,
            Self::DcimFolders => Self::YearsMonths,
        }
    }
}

pub enum CopyEvent {
    Progress(copier::CopyProgress),
    Done(anyhow::Result<copier::CopySummary>),
}

pub struct RefreshOutcome {
    pub device_status: device::DeviceStatus,
    pub mount_guard: Option<device::MountGuard>,
    pub scan_index: Option<scanner::ScanIndex>,
    pub albums: Vec<scanner::Bucket>,
    pub album_warning: Option<String>,
    pub status_msg: String,
}

pub enum RefreshEvent {
    Status(String),
    DeviceStatus(device::DeviceStatus),
    ScanReady(Box<scanner::ScanIndex>),
    Done(Box<RefreshOutcome>),
}

/// A bucket row displayed in the TUI list, with optional visual indentation.
#[derive(Debug, Clone)]
pub struct DisplayBucket<'a> {
    pub bucket: &'a scanner::Bucket,
    pub indented: bool,
}

pub struct AppState {
    pub theme: theme::Theme,
    pub device_status: device::DeviceStatus,
    pub mount_guard: Option<device::MountGuard>,
    pub mount_dir_override: Option<PathBuf>,
    pub scan_index: Option<scanner::ScanIndex>,
    pub albums: Vec<scanner::Bucket>,
    pub album_warning: Option<String>,
    pub tab: Tab,
    pub cursor: usize,
    pub selected_keys: HashSet<(Tab, String)>,
    pub dest_dir: PathBuf,
    pub editing_dest: bool,
    pub dest_input: String,
    pub show_help: bool,
    pub copy_rx: Option<mpsc::Receiver<CopyEvent>>,
    pub refresh_rx: Option<mpsc::Receiver<RefreshEvent>>,
    pub cancel_flag: Arc<AtomicBool>,
    pub copy_progress: Option<copier::CopyProgress>,
    pub copy_summary: Option<copier::CopySummary>,
    pub status_msg: String,
}

impl AppState {
    pub fn new(mount_dir: Option<PathBuf>, dest: Option<PathBuf>) -> Self {
        let dest_dir = dest.unwrap_or_else(default_dest_dir);
        Self {
            theme: theme::Theme::load_default(),
            device_status: device::DeviceStatus::NoDevice,
            mount_guard: None,
            mount_dir_override: mount_dir,
            scan_index: None,
            albums: Vec::new(),
            album_warning: None,
            tab: Tab::YearsMonths,
            cursor: 0,
            selected_keys: HashSet::new(),
            dest_dir,
            editing_dest: false,
            dest_input: String::new(),
            show_help: false,
            copy_rx: None,
            refresh_rx: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            copy_progress: None,
            copy_summary: None,
            status_msg: "Initializing...".to_string(),
        }
    }

    pub fn is_copying(&self) -> bool {
        self.copy_rx.is_some()
    }

    pub fn is_refreshing(&self) -> bool {
        self.refresh_rx.is_some()
    }

    /// Formats `dest_dir` with `~` in place of the user's home directory so local usernames aren't shown on screen.
    pub fn display_dest(&self) -> String {
        if let Some(home) = dirs::home_dir() {
            if let Ok(rel) = self.dest_dir.strip_prefix(&home) {
                if rel.as_os_str().is_empty() {
                    return "~".to_string();
                }
                return format!("~/{}", rel.display());
            }
        }
        self.dest_dir.display().to_string()
    }

    pub fn refresh(&mut self) {
        self.scan_index = None;
        self.albums.clear();
        self.album_warning = None;
        let prev_guard = self.mount_guard.take();
        let outcome = execute_refresh(
            self.mount_dir_override.clone(),
            prev_guard,
            |_| {},
            |_| {},
            |_| {},
        );
        self.apply_refresh_outcome(outcome);
    }

    pub fn start_refresh(&mut self) {
        if self.is_refreshing() || self.is_copying() {
            return;
        }
        self.scan_index = None;
        self.albums.clear();
        self.album_warning = None;
        self.status_msg = "Detecting iPhone...".to_string();
        let prev_guard = self.mount_guard.take();
        let mount_dir_override = self.mount_dir_override.clone();

        let (tx, rx) = mpsc::channel();
        self.refresh_rx = Some(rx);

        std::thread::spawn(move || {
            let outcome = execute_refresh(
                mount_dir_override,
                prev_guard,
                |msg| {
                    let _ = tx.send(RefreshEvent::Status(msg));
                },
                |status| {
                    let _ = tx.send(RefreshEvent::DeviceStatus(status));
                },
                |index| {
                    let _ = tx.send(RefreshEvent::ScanReady(Box::new(index)));
                },
            );
            let _ = tx.send(RefreshEvent::Done(Box::new(outcome)));
        });
    }

    pub fn poll_refresh_events(&mut self) {
        let Some(rx) = &self.refresh_rx else {
            return;
        };

        let mut finished = None;
        let mut scan_updated = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                RefreshEvent::Status(msg) => {
                    self.status_msg = msg;
                }
                RefreshEvent::DeviceStatus(status) => {
                    self.device_status = status;
                }
                RefreshEvent::ScanReady(index) => {
                    self.scan_index = Some(*index);
                    scan_updated = true;
                }
                RefreshEvent::Done(outcome) => {
                    finished = Some(*outcome);
                }
            }
        }

        if let Some(done) = finished {
            self.refresh_rx = None;
            self.apply_refresh_outcome(done);
        } else if scan_updated {
            self.clamp_cursor();
        }
    }

    fn apply_refresh_outcome(&mut self, outcome: RefreshOutcome) {
        self.device_status = outcome.device_status;
        self.mount_guard = outcome.mount_guard;
        self.scan_index = outcome.scan_index;
        self.albums = outcome.albums;
        self.album_warning = outcome.album_warning;
        self.status_msg = outcome.status_msg;
        self.clamp_cursor();
    }

    pub fn current_buckets(&self) -> Vec<DisplayBucket<'_>> {
        match self.tab {
            Tab::YearsMonths => {
                let Some(index) = &self.scan_index else {
                    return Vec::new();
                };
                let mut rows = Vec::with_capacity(index.years.len() + index.months.len());
                for year in &index.years {
                    rows.push(DisplayBucket {
                        bucket: year,
                        indented: false,
                    });
                    let prefix = format!("{}-", year.key);
                    for month in &index.months {
                        if month.key.starts_with(&prefix) {
                            rows.push(DisplayBucket {
                                bucket: month,
                                indented: true,
                            });
                        }
                    }
                }
                rows
            }
            Tab::Albums => self
                .albums
                .iter()
                .map(|b| DisplayBucket {
                    bucket: b,
                    indented: false,
                })
                .collect(),
            Tab::DcimFolders => self
                .scan_index
                .as_ref()
                .map(|idx| {
                    idx.dcim_folders
                        .iter()
                        .map(|b| DisplayBucket {
                            bucket: b,
                            indented: false,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn is_selected(&self, tab: Tab, key: &str) -> bool {
        self.selected_keys.contains(&(tab, key.to_string()))
    }

    fn clamp_cursor(&mut self) {
        let len = self.current_buckets().len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
    }

    fn toggle_current_selection(&mut self) {
        let key = self
            .current_buckets()
            .get(self.cursor)
            .map(|row| row.bucket.key.clone());
        let Some(key) = key else {
            return;
        };
        let entry = (self.tab, key);
        if !self.selected_keys.remove(&entry) {
            self.selected_keys.insert(entry);
        }
    }

    fn toggle_select_all(&mut self) {
        let keys: Vec<String> = self
            .current_buckets()
            .into_iter()
            .map(|row| row.bucket.key.clone())
            .collect();
        if keys.is_empty() {
            return;
        }

        let all_selected = keys
            .iter()
            .all(|k| self.selected_keys.contains(&(self.tab, k.clone())));
        if all_selected {
            for k in keys {
                self.selected_keys.remove(&(self.tab, k));
            }
        } else {
            for k in keys {
                self.selected_keys.insert((self.tab, k));
            }
        }
    }

    fn collect_selected_items(&self) -> Vec<scanner::MediaItem> {
        let Some(index) = &self.scan_index else {
            return Vec::new();
        };
        let buckets = self.current_buckets();
        if buckets.is_empty() {
            return Vec::new();
        }

        let mut chosen: Vec<&scanner::Bucket> = buckets
            .iter()
            .filter(|row| self.is_selected(self.tab, &row.bucket.key))
            .map(|row| row.bucket)
            .collect();

        if chosen.is_empty() {
            if let Some(row) = buckets.get(self.cursor) {
                chosen.push(row.bucket);
            }
        }

        let mut seen = HashSet::new();
        let mut items = Vec::new();
        for bucket in chosen {
            for &idx in &bucket.item_indices {
                if seen.insert(idx) {
                    if let Some(item) = index.items.get(idx) {
                        items.push(item.clone());
                    }
                }
            }
        }
        items
    }

    fn start_copy(&mut self) {
        if self.is_copying() {
            return;
        }
        let items = self.collect_selected_items();
        if items.is_empty() {
            self.status_msg = "No media items selected to copy".to_string();
            return;
        }

        let total_files = items.len();
        let total_bytes: u64 = items.iter().map(|i| i.size).sum();
        let dest_dir = self.dest_dir.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_thread = Arc::clone(&cancel);
        let (tx, rx) = mpsc::channel();

        self.cancel_flag = cancel;
        self.copy_summary = None;
        self.copy_progress = Some(copier::CopyProgress {
            current_file: String::new(),
            copied_files: 0,
            skipped_files: 0,
            total_files,
            copied_bytes: 0,
            total_bytes,
            bytes_per_sec: 0,
        });
        self.copy_rx = Some(rx);
        self.status_msg = format!(
            "Copying {total_files} files ({}) to {}...",
            scanner::format_bytes(total_bytes),
            dest_dir.display()
        );

        std::thread::spawn(move || {
            let refs: Vec<&scanner::MediaItem> = items.iter().collect();
            let res = copier::copy_items(
                &refs,
                &dest_dir,
                &copier::DestLayout::YearMonth,
                &cancel_thread,
                |progress| {
                    let _ = tx.send(CopyEvent::Progress(progress));
                },
            );
            let _ = tx.send(CopyEvent::Done(res));
        });
    }

    fn poll_copy_events(&mut self) {
        let mut done_result = None;
        if let Some(rx) = &self.copy_rx {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    CopyEvent::Progress(progress) => {
                        self.copy_progress = Some(progress);
                    }
                    CopyEvent::Done(res) => {
                        done_result = Some(res);
                        break;
                    }
                }
            }
        }

        if let Some(res) = done_result {
            self.copy_rx = None;
            self.copy_progress = None;
            match res {
                Ok(summary) => {
                    self.status_msg = format_copy_summary(&summary);
                    self.copy_summary = Some(summary);
                }
                Err(err) => {
                    self.status_msg = format!("Copy failed: {err}");
                }
            }
        }
    }

    /// Processes a key event. Returns `true` when the application should exit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }

        if self.editing_dest {
            match key.code {
                KeyCode::Enter => {
                    let trimmed = self.dest_input.trim();
                    if !trimmed.is_empty() {
                        self.dest_dir = expand_tilde(trimmed);
                        self.status_msg = format!("Destination set to {}", self.display_dest());
                    }
                    self.editing_dest = false;
                }
                KeyCode::Esc => {
                    self.editing_dest = false;
                    self.dest_input.clear();
                }
                KeyCode::Backspace => {
                    self.dest_input.pop();
                }
                KeyCode::Char(c) => {
                    self.dest_input.push(c);
                }
                _ => {}
            }
            return false;
        }

        if self.show_help {
            if matches!(
                key.code,
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
            return false;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.is_copying() {
                    self.cancel_flag.store(true, Ordering::Relaxed);
                    self.status_msg = "Cancelling copy...".to_string();
                    return false;
                }
                return true;
            }
            KeyCode::Char('?') => {
                self.show_help = true;
            }
            KeyCode::Tab => {
                self.tab = self.tab.next();
                self.clamp_cursor();
            }
            KeyCode::Char('1') => {
                self.tab = Tab::YearsMonths;
                self.clamp_cursor();
            }
            KeyCode::Char('2') => {
                self.tab = Tab::Albums;
                self.clamp_cursor();
            }
            KeyCode::Char('3') => {
                self.tab = Tab::DcimFolders;
                self.clamp_cursor();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.current_buckets().len();
                if len > 0 && self.cursor + 1 < len {
                    self.cursor += 1;
                }
            }
            KeyCode::Char(' ') => {
                self.toggle_current_selection();
            }
            KeyCode::Char('a') => {
                self.toggle_select_all();
            }
            KeyCode::Char('d') => {
                if !self.is_copying() {
                    self.dest_input = self.display_dest();
                    self.editing_dest = true;
                }
            }
            KeyCode::Char('r') => {
                if !self.is_copying() && !self.is_refreshing() {
                    self.start_refresh();
                }
            }
            KeyCode::Enter | KeyCode::Char('c') => {
                self.start_copy();
            }
            _ => {}
        }

        false
    }
}

fn execute_refresh(
    mount_dir_override: Option<PathBuf>,
    prev_guard: Option<device::MountGuard>,
    on_status: impl Fn(String),
    on_device: impl Fn(device::DeviceStatus),
    on_scan: impl Fn(scanner::ScanIndex),
) -> RefreshOutcome {
    drop(prev_guard);

    let (device_status, guard) = if let Some(dir) = mount_dir_override {
        let status = device::DeviceStatus::Connected(device::DeviceInfo {
            udid: "local".to_string(),
            name: dir.display().to_string(),
            product_type: "MountDir".to_string(),
            ios_version: "-".to_string(),
        });
        on_device(status.clone());
        (status, device::MountGuard::from_existing(dir))
    } else {
        on_status("Detecting iPhone...".to_string());
        let status = device::detect_device();
        on_device(status.clone());
        match &status {
            device::DeviceStatus::Connected(info) => {
                on_status(format!("Mounting {}...", info.display_label()));
                match device::mount_iphone(&info.udid) {
                    Ok(guard) => (status, guard),
                    Err(err) => {
                        return RefreshOutcome {
                            device_status: status,
                            mount_guard: None,
                            scan_index: None,
                            albums: Vec::new(),
                            album_warning: None,
                            status_msg: format!("Mount failed: {err}"),
                        };
                    }
                }
            }
            device::DeviceStatus::Untrusted { message, .. } => {
                let msg = format!("{message} — press r to retry");
                return RefreshOutcome {
                    device_status: status,
                    mount_guard: None,
                    scan_index: None,
                    albums: Vec::new(),
                    album_warning: None,
                    status_msg: msg,
                };
            }
            device::DeviceStatus::NoDevice => {
                return RefreshOutcome {
                    device_status: status,
                    mount_guard: None,
                    scan_index: None,
                    albums: Vec::new(),
                    album_warning: None,
                    status_msg: "Plug in iPhone via USB-C — press r to retry".to_string(),
                };
            }
            device::DeviceStatus::MissingTools(tools) => {
                let msg = format!("Missing tools: {} — install via pacman", tools.join(", "));
                return RefreshOutcome {
                    device_status: status,
                    mount_guard: None,
                    scan_index: None,
                    albums: Vec::new(),
                    album_warning: None,
                    status_msg: msg,
                };
            }
        }
    };

    on_status("Scanning DCIM/...".to_string());
    match scanner::scan_dcim_with_progress(guard.path(), |count| {
        on_status(format!("Scanning DCIM/ ({count} files)..."));
    }) {
        Ok(index) => {
            on_scan(index.clone());
            on_status(format!("Loading albums ({} files)...", index.items.len()));
            let album_res = albums::load_albums(guard.path(), &index);
            let status_msg = format!(
                "Ready: {} items ({})",
                index.items.len(),
                scanner::format_bytes(index.total_bytes)
            );
            RefreshOutcome {
                device_status,
                mount_guard: Some(guard),
                scan_index: Some(index),
                albums: album_res.albums,
                album_warning: album_res.warning,
                status_msg,
            }
        }
        Err(err) => RefreshOutcome {
            device_status,
            mount_guard: Some(guard),
            scan_index: None,
            albums: Vec::new(),
            album_warning: None,
            status_msg: format!("Scan failed: {err}"),
        },
    }
}

fn expand_tilde(input: &str) -> PathBuf {
    if input == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(input)
}

fn default_dest_dir() -> PathBuf {
    dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Copyzer")
}

fn format_copy_summary(summary: &copier::CopySummary) -> String {
    let prefix = if summary.cancelled {
        "Cancelled"
    } else {
        "Done"
    };
    let mut msg = format!(
        "{prefix}: {} copied ({}), {} skipped",
        summary.copied_files,
        scanner::format_bytes(summary.copied_bytes),
        summary.skipped_files
    );
    if !summary.failed_files.is_empty() {
        msg.push_str(&format!(", {} failed", summary.failed_files.len()));
    }
    msg
}

/// RAII terminal wrapper ensuring raw mode and alternate screen are always restored.
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(err) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(err.into());
        }
        let backend = CrosstermBackend::new(stdout);
        match Terminal::new(backend) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(err) => {
                let _ = disable_raw_mode();
                let mut out = io::stdout();
                let _ = execute!(out, LeaveAlternateScreen);
                Err(err.into())
            }
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

pub fn run(mount_dir: Option<PathBuf>, dest: Option<PathBuf>) -> anyhow::Result<()> {
    let mut guard = TerminalGuard::new()?;
    let mut app = AppState::new(mount_dir, dest);
    app.start_refresh();

    loop {
        app.poll_refresh_events();
        app.poll_copy_events();
        guard.terminal.draw(|frame| ui::render(frame, &app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if app.handle_key(key) {
                    break;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::fs;
    use tempfile::TempDir;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn setup_mock_mount() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let dcim = tmp.path().join("DCIM/100APPLE");
        fs::create_dir_all(&dcim).unwrap();
        fs::write(dcim.join("IMG_0001.JPG"), b"hello").unwrap();
        fs::write(dcim.join("IMG_0002.MOV"), b"world!!").unwrap();
        tmp
    }

    #[test]
    fn refresh_with_mount_dir_populates_scan_index_and_buckets() {
        let mount = setup_mock_mount();
        let dest = TempDir::new().unwrap();
        let mut app = AppState::new(
            Some(mount.path().to_path_buf()),
            Some(dest.path().to_path_buf()),
        );
        app.refresh();

        assert!(app.scan_index.is_some());
        assert!(!app.current_buckets().is_empty());
        assert!(app.album_warning.is_some());

        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.tab, Tab::DcimFolders);
        assert_eq!(app.current_buckets().len(), 1);
        assert_eq!(app.current_buckets()[0].bucket.label, "100APPLE");
    }

    #[test]
    fn selection_and_deduplication_across_year_and_month() {
        let mount = setup_mock_mount();
        let dest = TempDir::new().unwrap();
        let mut app = AppState::new(
            Some(mount.path().to_path_buf()),
            Some(dest.path().to_path_buf()),
        );
        app.refresh();

        // Select all in YearsMonths (both the year and the month bucket).
        app.handle_key(key(KeyCode::Char('a')));
        let items = app.collect_selected_items();
        // Should deduplicate to the 2 unique files, not 4.
        assert_eq!(items.len(), 2);

        // Toggle select all again -> deselects all.
        app.handle_key(key(KeyCode::Char('a')));
        assert!(app.selected_keys.is_empty());
    }

    #[test]
    fn dest_editor_saves_and_cancels() {
        let mut app = AppState::new(None, Some(PathBuf::from("/tmp/orig")));
        app.handle_key(key(KeyCode::Char('d')));
        assert!(app.editing_dest);

        app.handle_key(key(KeyCode::Esc));
        assert!(!app.editing_dest);
        assert_eq!(app.dest_dir, PathBuf::from("/tmp/orig"));

        app.handle_key(key(KeyCode::Char('d')));
        app.dest_input = "/tmp/new_dest".to_string();
        app.handle_key(key(KeyCode::Enter));
        assert!(!app.editing_dest);
        assert_eq!(app.dest_dir, PathBuf::from("/tmp/new_dest"));
    }

    #[test]
    fn help_overlay_and_quit_behavior() {
        let mut app = AppState::new(None, None);
        assert!(!app.show_help);

        assert!(!app.handle_key(key(KeyCode::Char('?'))));
        assert!(app.show_help);

        // Esc closes help instead of quitting.
        assert!(!app.handle_key(key(KeyCode::Esc)));
        assert!(!app.show_help);

        // q quits when idle.
        assert!(app.handle_key(key(KeyCode::Char('q'))));
    }
}
