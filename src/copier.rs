use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::Context;
use filetime::FileTime;
use serde::Serialize;

use crate::scanner::MediaItem;

const COPY_BUF_SIZE: usize = 1024 * 1024; // 1 MiB reusable buffer for USB-C / ifuse throughput

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestLayout {
    YearMonth,
    FlatBucket(String),
}

#[derive(Debug, Clone)]
pub struct CopyProgress {
    pub current_file: String,
    pub copied_files: usize,
    pub skipped_files: usize,
    pub total_files: usize,
    pub copied_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_sec: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CopySummary {
    pub copied_files: usize,
    pub skipped_files: usize,
    pub failed_files: Vec<(String, String)>,
    pub copied_bytes: u64,
    pub cancelled: bool,
}

enum SingleCopyOutcome {
    Copied(u64),
    Cancelled,
}

pub fn copy_items<F>(
    items: &[&MediaItem],
    dest_root: &Path,
    layout: &DestLayout,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> anyhow::Result<CopySummary>
where
    F: FnMut(CopyProgress),
{
    let total_files = items.len();
    let total_bytes: u64 = items.iter().map(|item| item.size).sum();
    let mut summary = CopySummary::default();
    let mut buf = vec![0u8; COPY_BUF_SIZE];
    let mut created_dirs: HashSet<PathBuf> = HashSet::new();
    let started_at = Instant::now();

    let calc_speed = |copied_bytes: u64, start: &Instant| -> u64 {
        let secs = start.elapsed().as_secs_f64();
        if secs > 0.001 {
            (copied_bytes as f64 / secs) as u64
        } else {
            copied_bytes
        }
    };

    for item in items {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }

        let base_dest = dest_path_for(dest_root, layout, item);
        let dest_path = match resolve_safe_dest_path(&base_dest, item) {
            DestResolution::SkipExisting => {
                summary.skipped_files += 1;
                on_progress(CopyProgress {
                    current_file: item.filename.clone(),
                    copied_files: summary.copied_files,
                    skipped_files: summary.skipped_files,
                    total_files,
                    copied_bytes: summary.copied_bytes,
                    total_bytes,
                    bytes_per_sec: calc_speed(summary.copied_bytes, &started_at),
                });
                continue;
            }
            DestResolution::WriteTo(path) => path,
        };

        let base_copied = summary.copied_bytes;
        let copied_files_so_far = summary.copied_files;
        let skipped_files_so_far = summary.skipped_files;

        let res = copy_single_file(
            item,
            &dest_path,
            &mut buf,
            &mut created_dirs,
            cancel,
            |in_flight_bytes| {
                let current_total = base_copied.saturating_add(in_flight_bytes);
                on_progress(CopyProgress {
                    current_file: item.filename.clone(),
                    copied_files: copied_files_so_far,
                    skipped_files: skipped_files_so_far,
                    total_files,
                    copied_bytes: current_total,
                    total_bytes,
                    bytes_per_sec: calc_speed(current_total, &started_at),
                });
            },
        );

        match res {
            Ok(SingleCopyOutcome::Copied(bytes)) => {
                summary.copied_files += 1;
                summary.copied_bytes += bytes;
            }
            Ok(SingleCopyOutcome::Cancelled) => {
                summary.cancelled = true;
                break;
            }
            Err(err) => {
                summary
                    .failed_files
                    .push((item.filename.clone(), err.to_string()));
            }
        }

        on_progress(CopyProgress {
            current_file: item.filename.clone(),
            copied_files: summary.copied_files,
            skipped_files: summary.skipped_files,
            total_files,
            copied_bytes: summary.copied_bytes,
            total_bytes,
            bytes_per_sec: calc_speed(summary.copied_bytes, &started_at),
        });
    }

    Ok(summary)
}

enum DestResolution {
    SkipExisting,
    WriteTo(PathBuf),
}

/// Ensures existing user files are never overwritten.
/// - Same filename + same byte size => skip (idempotent resume).
/// - Same filename + different byte size (e.g. IMG_9999 rollover across DCIM folders) =>
///   disambiguate with `<stem>_<dcim_folder>.<ext>` (or numeric suffix).
fn resolve_safe_dest_path(base_dest: &Path, item: &MediaItem) -> DestResolution {
    match fs::metadata(base_dest) {
        Ok(meta) if meta.is_file() => {
            if meta.len() == item.size {
                return DestResolution::SkipExisting;
            }
        }
        Err(_) => return DestResolution::WriteTo(base_dest.to_path_buf()),
        Ok(_) => {}
    }

    let parent = base_dest.parent().unwrap_or_else(|| Path::new("."));
    let safe_name = sanitize_name(&item.filename);
    let (stem, ext) = match safe_name.rfind('.') {
        Some(idx) if idx > 0 => (&safe_name[..idx], &safe_name[idx..]),
        _ => (safe_name.as_str(), ""),
    };
    let folder_tag = sanitize_name(&item.dcim_folder);

    let candidate = parent.join(format!("{stem}_{folder_tag}{ext}"));
    match fs::metadata(&candidate) {
        Ok(meta) if meta.is_file() && meta.len() == item.size => {
            return DestResolution::SkipExisting;
        }
        Err(_) => return DestResolution::WriteTo(candidate),
        Ok(_) => {}
    }

    for n in 2..=1000u32 {
        let numbered = parent.join(format!("{stem}_{folder_tag}_{n}{ext}"));
        match fs::metadata(&numbered) {
            Ok(meta) if meta.is_file() && meta.len() == item.size => {
                return DestResolution::SkipExisting;
            }
            Err(_) => return DestResolution::WriteTo(numbered),
            Ok(_) => {}
        }
    }

    DestResolution::WriteTo(candidate)
}

fn dest_path_for(dest_root: &Path, layout: &DestLayout, item: &MediaItem) -> PathBuf {
    let safe_filename = sanitize_name(&item.filename);
    match layout {
        DestLayout::YearMonth => dest_root
            .join(format!("{:04}", item.year))
            .join(format!("{:02}", item.month))
            .join(safe_filename),
        DestLayout::FlatBucket(name) => dest_root
            .join(sanitize_name(name))
            .join(safe_filename),
    }
}

fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "_".to_string()
    } else {
        cleaned
    }
}

fn part_path_for(dest: &Path) -> PathBuf {
    let mut os = dest.as_os_str().to_os_string();
    os.push(".copyzer.part");
    PathBuf::from(os)
}

fn copy_single_file<P>(
    item: &MediaItem,
    dest: &Path,
    buf: &mut [u8],
    created_dirs: &mut HashSet<PathBuf>,
    cancel: &AtomicBool,
    mut on_chunk: P,
) -> anyhow::Result<SingleCopyOutcome>
where
    P: FnMut(u64),
{
    let src = &item.path;
    if let Some(parent) = dest.parent() {
        if !created_dirs.contains(parent) {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create destination directory {}", parent.display())
            })?;
            created_dirs.insert(parent.to_path_buf());
        }
    }

    let mtime = FileTime::from_unix_time(item.mtime, 0);
    let part_path = part_path_for(dest);

    let result = (|| -> anyhow::Result<SingleCopyOutcome> {
        let mut src_file = File::open(src)
            .with_context(|| format!("failed to open source {}", src.display()))?;
        let mut part_file = File::create(&part_path)
            .with_context(|| format!("failed to create temp file {}", part_path.display()))?;

        let mut total_written = 0u64;
        let mut chunks_since_report = 0usize;

        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(part_file);
                let _ = fs::remove_file(&part_path);
                return Ok(SingleCopyOutcome::Cancelled);
            }

            let read_len = src_file
                .read(buf)
                .with_context(|| format!("failed reading from {}", src.display()))?;
            if read_len == 0 {
                break;
            }

            part_file
                .write_all(&buf[..read_len])
                .with_context(|| format!("failed writing to {}", part_path.display()))?;
            total_written += read_len as u64;
            chunks_since_report += 1;

            if chunks_since_report >= 4 {
                chunks_since_report = 0;
                on_chunk(total_written);
            }
        }

        part_file
            .sync_data()
            .with_context(|| format!("failed to sync {}", part_path.display()))?;
        drop(part_file);
        filetime::set_file_mtime(&part_path, mtime)
            .with_context(|| format!("failed to set mtime on {}", part_path.display()))?;
        fs::rename(&part_path, dest).with_context(|| {
            format!("failed to rename {} to {}", part_path.display(), dest.display())
        })?;
        Ok(SingleCopyOutcome::Copied(total_written))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&part_path);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use walkdir::WalkDir;

    fn make_source_file(
        dir: &Path,
        filename: &str,
        contents: &[u8],
        mtime_secs: i64,
        year: i32,
        month: u32,
    ) -> MediaItem {
        let path = dir.join(filename);
        let mut file = File::create(&path).unwrap();
        file.write_all(contents).unwrap();
        file.sync_all().unwrap();
        drop(file);

        filetime::set_file_mtime(&path, FileTime::from_unix_time(mtime_secs, 0)).unwrap();

        MediaItem {
            path,
            rel_path: format!("DCIM/100APPLE/{filename}"),
            filename: filename.to_string(),
            size: contents.len() as u64,
            mtime: mtime_secs,
            year,
            month,
            dcim_folder: "100APPLE".to_string(),
        }
    }

    fn has_part_files(root: &Path) -> bool {
        WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with(".copyzer.part")
            })
    }

    #[test]
    fn copies_into_year_month_preserves_mtime_and_leaves_no_part_files() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        let expected_mtime = 1_700_000_000;
        let item = make_source_file(
            src_dir.path(),
            "IMG_0001.HEIC",
            b"hello-heic-bytes",
            expected_mtime,
            2025,
            3,
        );

        let cancel = AtomicBool::new(false);
        let mut progress_events = Vec::new();
        let summary = copy_items(
            &[&item],
            dest_dir.path(),
            &DestLayout::YearMonth,
            &cancel,
            |p| progress_events.push(p),
        )
        .unwrap();

        assert_eq!(summary.copied_files, 1);
        assert_eq!(summary.skipped_files, 0);
        assert!(summary.failed_files.is_empty());
        assert_eq!(summary.copied_bytes, 16);
        assert!(!summary.cancelled);
        assert_eq!(progress_events.len(), 1);
        assert_eq!(progress_events[0].copied_files, 1);
        assert_eq!(progress_events[0].total_files, 1);

        let dest_file = dest_dir.path().join("2025/03/IMG_0001.HEIC");
        assert_eq!(fs::read(&dest_file).unwrap(), b"hello-heic-bytes");

        let dest_mtime = FileTime::from_last_modification_time(&fs::metadata(&dest_file).unwrap());
        assert_eq!(dest_mtime.unix_seconds(), expected_mtime);
        assert!(!has_part_files(dest_dir.path()));
    }

    #[test]
    fn copies_into_flat_bucket_with_sanitized_name() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        let item = make_source_file(
            src_dir.path(),
            "CLIP_01.MOV",
            b"mov-data",
            1_700_000_123,
            2026,
            11,
        );

        let cancel = AtomicBool::new(false);
        let summary = copy_items(
            &[&item],
            dest_dir.path(),
            &DestLayout::FlatBucket("Road/Trip:2026".to_string()),
            &cancel,
            |_| {},
        )
        .unwrap();

        assert_eq!(summary.copied_files, 1);
        let dest_file = dest_dir.path().join("Road_Trip_2026/CLIP_01.MOV");
        assert_eq!(fs::read(&dest_file).unwrap(), b"mov-data");
        assert!(!has_part_files(dest_dir.path()));
    }

    #[test]
    fn skips_already_copied_files_with_matching_size() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        let item = make_source_file(
            src_dir.path(),
            "IMG_0002.JPG",
            b"1234567890",
            1_700_000_000,
            2026,
            1,
        );

        let existing_dir = dest_dir.path().join("2026/01");
        fs::create_dir_all(&existing_dir).unwrap();
        let existing_file = existing_dir.join("IMG_0002.JPG");
        fs::write(&existing_file, b"abcdefghij").unwrap();

        let cancel = AtomicBool::new(false);
        let mut progress_events = Vec::new();
        let summary = copy_items(
            &[&item],
            dest_dir.path(),
            &DestLayout::YearMonth,
            &cancel,
            |p| progress_events.push(p),
        )
        .unwrap();

        assert_eq!(summary.copied_files, 0);
        assert_eq!(summary.skipped_files, 1);
        assert_eq!(summary.copied_bytes, 0);
        assert!(!summary.cancelled);
        assert_eq!(progress_events.len(), 1);
        assert_eq!(progress_events[0].skipped_files, 1);
        // Existing file was not overwritten
        assert_eq!(fs::read(&existing_file).unwrap(), b"abcdefghij");
    }

    #[test]
    fn disambiguates_filename_when_existing_file_has_different_size() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        let mut item = make_source_file(
            src_dir.path(),
            "IMG_0001.JPG",
            b"new-larger-photo-bytes",
            1_700_000_000,
            2026,
            1,
        );
        item.dcim_folder = "136APPLE".to_string();

        let existing_dir = dest_dir.path().join("2026/01");
        fs::create_dir_all(&existing_dir).unwrap();
        let existing_file = existing_dir.join("IMG_0001.JPG");
        fs::write(&existing_file, b"short").unwrap();

        let cancel = AtomicBool::new(false);
        let summary = copy_items(
            &[&item],
            dest_dir.path(),
            &DestLayout::YearMonth,
            &cancel,
            |_| {},
        )
        .unwrap();

        assert_eq!(summary.copied_files, 1);
        assert_eq!(fs::read(&existing_file).unwrap(), b"short");
        assert_eq!(
            fs::read(existing_dir.join("IMG_0001_136APPLE.JPG")).unwrap(),
            b"new-larger-photo-bytes"
        );
    }

    #[test]
    fn respects_cancel_flag() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        let item1 = make_source_file(
            src_dir.path(),
            "IMG_0001.JPG",
            b"first",
            1_700_000_000,
            2026,
            2,
        );
        let item2 = make_source_file(
            src_dir.path(),
            "IMG_0002.JPG",
            b"second",
            1_700_000_001,
            2026,
            2,
        );

        let cancel = AtomicBool::new(false);
        let summary = copy_items(
            &[&item1, &item2],
            dest_dir.path(),
            &DestLayout::YearMonth,
            &cancel,
            |_| {
                cancel.store(true, Ordering::Relaxed);
            },
        )
        .unwrap();

        assert!(summary.cancelled);
        assert_eq!(summary.copied_files, 1);
        assert!(dest_dir.path().join("2026/02/IMG_0001.JPG").exists());
        assert!(!dest_dir.path().join("2026/02/IMG_0002.JPG").exists());
        assert!(!has_part_files(dest_dir.path()));
    }
}
