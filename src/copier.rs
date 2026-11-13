use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use filetime::FileTime;
use serde::Serialize;

use crate::scanner::MediaItem;

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
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CopySummary {
    pub copied_files: usize,
    pub skipped_files: usize,
    pub failed_files: Vec<(String, String)>,
    pub copied_bytes: u64,
    pub cancelled: bool,
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

    for item in items {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }

        let dest_path = dest_path_for(dest_root, layout, item);

        if let Ok(meta) = fs::metadata(&dest_path) {
            if meta.is_file() && meta.len() == item.size {
                summary.skipped_files += 1;
                on_progress(CopyProgress {
                    current_file: item.filename.clone(),
                    copied_files: summary.copied_files,
                    skipped_files: summary.skipped_files,
                    total_files,
                    copied_bytes: summary.copied_bytes,
                    total_bytes,
                });
                continue;
            }
        }

        match copy_single_file(&item.path, &dest_path) {
            Ok(bytes) => {
                summary.copied_files += 1;
                summary.copied_bytes += bytes;
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
        });
    }

    Ok(summary)
}

fn dest_path_for(dest_root: &Path, layout: &DestLayout, item: &MediaItem) -> PathBuf {
    match layout {
        DestLayout::YearMonth => dest_root
            .join(format!("{:04}", item.year))
            .join(format!("{:02}", item.month))
            .join(&item.filename),
        DestLayout::FlatBucket(name) => dest_root
            .join(sanitize_bucket_name(name))
            .join(&item.filename),
    }
}

fn sanitize_bucket_name(name: &str) -> String {
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

fn copy_single_file(src: &Path, dest: &Path) -> anyhow::Result<u64> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create destination directory {}", parent.display())
        })?;
    }

    let src_meta =
        fs::metadata(src).with_context(|| format!("failed to stat source {}", src.display()))?;
    let mtime = FileTime::from_last_modification_time(&src_meta);
    let part_path = part_path_for(dest);

    let result = (|| -> anyhow::Result<u64> {
        let mut src_file = File::open(src)
            .with_context(|| format!("failed to open source {}", src.display()))?;
        let mut part_file = File::create(&part_path)
            .with_context(|| format!("failed to create temp file {}", part_path.display()))?;
        let bytes = io::copy(&mut src_file, &mut part_file).with_context(|| {
            format!("failed to copy {} to {}", src.display(), part_path.display())
        })?;
        part_file
            .sync_all()
            .with_context(|| format!("failed to sync {}", part_path.display()))?;
        drop(part_file);
        filetime::set_file_mtime(&part_path, mtime)
            .with_context(|| format!("failed to set mtime on {}", part_path.display()))?;
        fs::rename(&part_path, dest).with_context(|| {
            format!("failed to rename {} to {}", part_path.display(), dest.display())
        })?;
        Ok(bytes)
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
