use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Table, TableState, Tabs, Wrap,
};
use ratatui::Frame;

use crate::device::DeviceStatus;
use crate::scanner;
use crate::tui::{AppState, Tab};

pub fn render(frame: &mut Frame<'_>, app: &AppState) {
    let area = frame.area();

    // Paint solid theme background.
    let bg_block = Block::default().style(
        Style::default()
            .bg(app.theme.background)
            .fg(app.theme.foreground),
    );
    frame.render_widget(bg_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(frame, chunks[0], app);
    render_tabs(frame, chunks[1], app);
    render_main(frame, chunks[2], app);
    render_footer(frame, chunks[3], app);

    if app.show_help {
        render_help_overlay(frame, area, app);
    }
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let (badge_text, badge_color) = match &app.device_status {
        DeviceStatus::Connected(info) => {
            let display_name = info.display_label();
            let label = if info.ios_version == "-" {
                format!(" {display_name} ")
            } else {
                format!(" {display_name} (iOS {}) ", info.ios_version)
            };
            (label, app.theme.success)
        }
        DeviceStatus::Untrusted { .. } => (" Untrusted — Tap Trust ".to_string(), app.theme.warning),
        DeviceStatus::NoDevice => (" No iPhone ".to_string(), app.theme.muted),
        DeviceStatus::MissingTools(_) => (" Missing Tools ".to_string(), app.theme.error),
    };

    let dest_span = if app.editing_dest {
        Span::styled(
            format!("Dest: {}_", app.dest_input),
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!("Dest: {}", app.display_dest()),
            Style::default().fg(app.theme.foreground),
        )
    };

    let line = Line::from(vec![
        Span::styled(
            concat!(" COPYZER v", env!("CARGO_PKG_VERSION"), " "),
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            badge_text,
            Style::default()
                .bg(badge_color)
                .fg(app.theme.background)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        dest_span,
    ]);

    let header = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.muted)),
    );
    frame.render_widget(header, area);
}

fn render_tabs(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let titles = vec![
        "[1] Years & Months",
        "[2] Albums",
        "[3] DCIM Folders",
    ];
    let selected = match app.tab {
        Tab::YearsMonths => 0,
        Tab::Albums => 1,
        Tab::DcimFolders => 2,
    };

    let tabs = Tabs::new(titles)
        .select(selected)
        .style(Style::default().fg(app.theme.muted))
        .highlight_style(
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled("  │  ", Style::default().fg(app.theme.muted)))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.muted)),
        );
    frame.render_widget(tabs, area);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let buckets = app.current_buckets();

    let title = match app.tab {
        Tab::YearsMonths => " Years & Months ".to_string(),
        Tab::Albums => match &app.album_warning {
            Some(warn) => format!(" Albums ({warn}) "),
            None => " Albums ".to_string(),
        },
        Tab::DcimFolders => " DCIM Folders ".to_string(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.muted));

    if buckets.is_empty() {
        let (msg, color) = empty_state_message(app);
        let paragraph = Paragraph::new(msg)
            .style(Style::default().fg(color))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    let rows: Vec<Row<'_>> = buckets
        .iter()
        .map(|row| {
            let checked = app.is_selected(app.tab, &row.bucket.key);
            let checkbox = if checked { "[x]" } else { "[ ]" };
            let label = if row.indented {
                format!("  {}", row.bucket.label)
            } else if app.tab == Tab::YearsMonths {
                format!("{} (All)", row.bucket.label)
            } else {
                row.bucket.label.clone()
            };
            let count = format!("{} files", row.bucket.item_indices.len());
            let size = scanner::format_bytes(row.bucket.total_bytes);

            let check_style = if checked {
                Style::default()
                    .fg(app.theme.success)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.muted)
            };

            Row::new(vec![
                Cell::from(checkbox).style(check_style),
                Cell::from(label).style(Style::default().fg(app.theme.foreground)),
                Cell::from(count).style(Style::default().fg(app.theme.muted)),
                Cell::from(size).style(Style::default().fg(app.theme.foreground)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Min(20),
            Constraint::Length(14),
            Constraint::Length(12),
        ],
    )
    .block(block)
    .row_highlight_style(
        Style::default()
            .bg(app.theme.accent)
            .fg(app.theme.background)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("▶ ");

    let mut state = TableState::default();
    state.select(Some(app.cursor));
    frame.render_stateful_widget(table, area, &mut state);
}

fn empty_state_message(app: &AppState) -> (String, ratatui::style::Color) {
    if app.is_refreshing() {
        return (format!("\n{}", app.status_msg), app.theme.accent);
    }
    match &app.device_status {
        DeviceStatus::MissingTools(tools) => (
            format!(
                "\nMissing tools: {} — install libimobiledevice & ifuse",
                tools.join(", ")
            ),
            app.theme.error,
        ),
        DeviceStatus::Untrusted { message, .. } => (
            format!("\n{message}\n\nUnlock iPhone & tap Trust, then press r to refresh."),
            app.theme.warning,
        ),
        DeviceStatus::NoDevice => {
            let hint = if crate::device::is_apple_fastcharge_loaded() {
                "\nPlug in iPhone, then press r to refresh.\n\n(Using a USB-A to USB-C cable? Run `sudo modprobe -r apple_mfi_fastcharge` if the port resets.)"
            } else {
                "\nPlug in iPhone via USB-C or USB-A, then press r to refresh."
            };
            (hint.to_string(), app.theme.muted)
        }
        DeviceStatus::Connected(_) => {
            if app.tab == Tab::Albums {
                if let Some(warn) = &app.album_warning {
                    return (format!("\n{warn}"), app.theme.warning);
                }
            }
            ("\nNo media items found in DCIM/.".to_string(), app.theme.muted)
        }
    }
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.muted));

    if let Some(progress) = &app.copy_progress {
        let done = progress.copied_files + progress.skipped_files;
        let ratio = if progress.total_bytes > 0 && progress.copied_bytes > 0 {
            (progress.copied_bytes as f64 / progress.total_bytes as f64).clamp(0.0, 1.0)
        } else if progress.total_files == 0 {
            1.0
        } else {
            (done as f64 / progress.total_files as f64).clamp(0.0, 1.0)
        };
        let speed_str = if progress.bytes_per_sec > 0 {
            format!(" @ {}/s", scanner::format_bytes(progress.bytes_per_sec))
        } else {
            String::new()
        };
        let label = format!(
            "{done}/{}{speed_str} ({}) — Esc/q Cancel",
            progress.total_files, progress.current_file
        );
        let gauge = Gauge::default()
            .block(block)
            .gauge_style(
                Style::default()
                    .fg(app.theme.accent)
                    .bg(app.theme.background),
            )
            .ratio(ratio)
            .label(label);
        frame.render_widget(gauge, area);
        return;
    }

    let text = Line::from(vec![
        Span::styled(
            format!(" {} ", app.status_msg),
            Style::default().fg(app.theme.foreground),
        ),
        Span::styled(" │ ", Style::default().fg(app.theme.muted)),
        Span::styled(
            "? Help  Space Select  Enter Copy  d Dest  r Refresh  q Quit",
            Style::default().fg(app.theme.muted),
        ),
    ]);
    let footer = Paragraph::new(text).block(block);
    frame.render_widget(footer, area);
}

fn render_help_overlay(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let popup = centered_rect(64, 15, area);
    frame.render_widget(Clear, popup);

    let help_lines = vec![
        Line::from(""),
        Line::from("  Tab / 1 / 2 / 3   Switch view (Years/Months, Albums, DCIM)"),
        Line::from("  Up/k, Down/j      Move cursor"),
        Line::from("  Space             Toggle selection on highlighted bucket"),
        Line::from("  a                 Select all / none in current tab"),
        Line::from("  Enter / c         Copy selected (or highlighted) bucket"),
        Line::from("  d                 Edit destination path (Enter save, Esc cancel)"),
        Line::from("  r                 Refresh device & rescan"),
        Line::from("  ?                 Toggle this help overlay"),
        Line::from("  q / Esc           Cancel active copy or quit"),
        Line::from(""),
        Line::from(Span::styled(
            "  Press ? or Esc to close",
            Style::default().fg(app.theme.muted),
        )),
    ];

    let help = Paragraph::new(help_lines)
        .style(
            Style::default()
                .bg(app.theme.background)
                .fg(app.theme.foreground),
        )
        .block(
            Block::default()
                .title(" Keyboard Shortcuts ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.accent)),
        );
    frame.render_widget(help, popup);
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height.min(area.height)),
            Constraint::Min(0),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
