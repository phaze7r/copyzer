use chrono::Datelike;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

const MEDIA_EXTENSIONS: &[&str] = &[
    "heic", "heif", "jpg", "jpeg", "png", "dng", "raw", "mov", "mp4", "m4v", "aae", "gif",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MediaItem {
    pub path: PathBuf,
    pub rel_path: String,
    pub filename: String,
    pub size: u64,
    pub mtime: i64,
    pub year: i32,
    pub month: u32,
    pub dcim_folder: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Bucket {
    pub key: String,
    pub label: String,
    pub item_indices: Vec<usize>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanIndex {
    pub items: Vec<MediaItem>,
    pub years: Vec<Bucket>,
    pub months: Vec<Bucket>,
    pub dcim_folders: Vec<Bucket>,
    pub total_bytes: u64,
}

impl ScanIndex {
    fn empty() -> Self {
        Self {
            items: Vec::new(),
            years: Vec::new(),
            months: Vec::new(),
            dcim_folders: Vec::new(),
            total_bytes: 0,
        }
    }
}

pub fn scan_dcim(mount_root: &Path) -> anyhow::Result<ScanIndex> {
    scan_dcim_with_progress(mount_root, |_| {})
}

pub fn scan_dcim_with_progress<F>(mount_root: &Path, mut on_progress: F) -> anyhow::Result<ScanIndex>
where
    F: FnMut(usize),
{
    if !mount_root.exists() {
        anyhow::bail!("Mount root does not exist: {}", mount_root.display());
    }

    let dcim_dir = mount_root.join("DCIM");
    if !dcim_dir.is_dir() {
        return Ok(ScanIndex::empty());
    }

    let mut items = Vec::new();
    let mut total_bytes = 0u64;

    let walker = WalkDir::new(&dcim_dir)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with('.'))
        });

    for entry in walker {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if !is_media_file(path) {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let size = metadata.len();
        let mtime = filetime::FileTime::from_last_modification_time(&metadata).unix_seconds();
        let (year, month) = mtime_year_month(&metadata);

        let rel_path = path
            .strip_prefix(mount_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let filename = entry.file_name().to_string_lossy().into_owned();
        let dcim_folder = extract_dcim_folder(&dcim_dir, path);

        total_bytes = total_bytes.saturating_add(size);
        items.push(MediaItem {
            path: path.to_path_buf(),
            rel_path,
            filename,
            size,
            mtime,
            year,
            month,
            dcim_folder,
        });

        if items.len() % 50 == 0 {
            on_progress(items.len());
        }
    }

    let years = build_buckets(&items, true, |item| format!("{:04}", item.year));
    let months = build_buckets(&items, true, |item| {
        format!("{:04}-{:02}", item.year, item.month)
    });
    let dcim_folders = build_buckets(&items, false, |item| item.dcim_folder.clone());

    Ok(ScanIndex {
        items,
        years,
        months,
        dcim_folders,
        total_bytes,
    })
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if b < MB {
        format!("{:.1} KB", b / KB)
    } else if b < GB {
        format!("{:.1} MB", b / MB)
    } else {
        format!("{:.2} GB", b / GB)
    }
}

fn is_media_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| MEDIA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn extract_dcim_folder(dcim_dir: &Path, file_path: &Path) -> String {
    let Ok(rel) = file_path.strip_prefix(dcim_dir) else {
        return "DCIM".to_string();
    };
    let mut components = rel.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(folder)), Some(_)) => folder.to_string_lossy().into_owned(),
        _ => "DCIM".to_string(),
    }
}

fn mtime_year_month(metadata: &std::fs::Metadata) -> (i32, u32) {
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let dt: chrono::DateTime<chrono::Local> = chrono::DateTime::from(mtime);
    (dt.year(), dt.month())
}

fn build_buckets<F>(items: &[MediaItem], descending: bool, key_fn: F) -> Vec<Bucket>
where
    F: Fn(&MediaItem) -> String,
{
    let mut grouped: BTreeMap<String, Bucket> = BTreeMap::new();
    for (idx, item) in items.iter().enumerate() {
        let key = key_fn(item);
        let bucket = grouped.entry(key.clone()).or_insert_with(|| Bucket {
            key: key.clone(),
            label: key,
            item_indices: Vec::new(),
            total_bytes: 0,
        });
        bucket.item_indices.push(idx);
        bucket.total_bytes = bucket.total_bytes.saturating_add(item.size);
    }

    let mut buckets: Vec<Bucket> = grouped.into_values().collect();
    if descending {
        buckets.reverse();
    }
    buckets
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use filetime::{set_file_mtime, FileTime};
    use std::fs;
    use tempfile::TempDir;

    fn write_file_with_mtime(path: &Path, bytes: &[u8], year: i32, month: u32, day: u32) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
        let dt = chrono::Local
            .with_ymd_and_hms(year, month, day, 12, 0, 0)
            .single()
            .unwrap();
        set_file_mtime(path, FileTime::from_unix_time(dt.timestamp(), 0)).unwrap();
    }

    #[test]
    fn filters_non_media_and_groups_by_mtime_and_dcim_folder() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        write_file_with_mtime(&root.join("DCIM/100APPLE/IMG_0001.HEIC"), b"12345", 2025, 11, 5);
        write_file_with_mtime(&root.join("DCIM/100APPLE/IMG_0002.jpg"), b"12345678", 2025, 11, 6);
        write_file_with_mtime(&root.join("DCIM/101APPLE/VID_0003.MOV"), b"123", 2026, 1, 10);
        write_file_with_mtime(&root.join("DCIM/100APPLE/notes.txt"), b"ignore", 2026, 1, 10);
        write_file_with_mtime(&root.join("DCIM/100APPLE/.DS_Store"), b"ignore", 2026, 1, 10);

        let idx = scan_dcim(root).unwrap();
        assert_eq!(idx.items.len(), 3);
        assert_eq!(idx.total_bytes, 16);

        assert_eq!(idx.years.len(), 2);
        assert_eq!(idx.years[0].key, "2026");
        assert_eq!(idx.years[0].item_indices.len(), 1);
        assert_eq!(idx.years[1].key, "2025");
        assert_eq!(idx.years[1].item_indices.len(), 2);

        assert_eq!(idx.months.len(), 2);
        assert_eq!(idx.months[0].key, "2026-01");
        assert_eq!(idx.months[1].key, "2025-11");

        assert_eq!(idx.dcim_folders.len(), 2);
        assert_eq!(idx.dcim_folders[0].key, "100APPLE");
        assert_eq!(idx.dcim_folders[0].total_bytes, 13);
        assert_eq!(idx.dcim_folders[1].key, "101APPLE");
        assert_eq!(idx.dcim_folders[1].total_bytes, 3);
    }

    #[test]
    fn missing_dcim_returns_empty_index_and_missing_root_errors() {
        let tmp = TempDir::new().unwrap();
        let empty = scan_dcim(tmp.path()).unwrap();
        assert!(empty.items.is_empty());
        assert_eq!(empty.total_bytes, 0);

        let missing = tmp.path().join("does-not-exist");
        assert!(scan_dcim(&missing).is_err());
    }

    #[test]
    fn formats_bytes_human_readably() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }
}
