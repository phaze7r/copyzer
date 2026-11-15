use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::scanner::{Bucket, ScanIndex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCache {
    pub udid: String,
    pub index: ScanIndex,
    pub albums: Vec<Bucket>,
    pub album_warning: Option<String>,
}

fn cache_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        let trimmed = xdg.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed).join("copyzer"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed).join(".cache").join("copyzer"));
        }
    }
    if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
        let trimmed = appdata.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed).join("copyzer").join("cache"));
        }
    }
    None
}

fn sanitize_udid(udid: &str) -> String {
    udid.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

fn cache_file_path(dir: &Path, udid: &str) -> PathBuf {
    let safe = sanitize_udid(udid);
    dir.join(format!("device-{safe}.json"))
}

pub fn load_device_cache(udid: &str) -> Option<DeviceCache> {
    let dir = cache_dir()?;
    load_device_cache_from_dir(&dir, udid)
}

pub fn load_device_cache_from_dir(dir: &Path, udid: &str) -> Option<DeviceCache> {
    let path = cache_file_path(dir, udid);
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save_device_cache(cache: &DeviceCache) {
    if let Some(dir) = cache_dir() {
        save_device_cache_to_dir(&dir, cache);
    }
}

pub fn save_device_cache_to_dir(dir: &Path, cache: &DeviceCache) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }

    let final_path = cache_file_path(dir, &cache.udid);
    let tmp_path = final_path.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(cache) {
        if fs::write(&tmp_path, bytes).is_ok() {
            let _ = fs::rename(&tmp_path, &final_path);
        }
    }
}

/// Updates all `MediaItem.path` entries inside a cached `ScanIndex` to point to a newly
/// mounted FUSE directory (`new_mount_root`), preserving `rel_path`.
pub fn rebase_scan_index(index: &mut ScanIndex, new_mount_root: &Path) {
    for item in &mut index.items {
        item.path = new_mount_root.join(&item.rel_path);
    }
}

/// Fast check to see if the mounted iPhone's `DCIM/` subfolders and media entry count
/// still match the cached `ScanIndex`, avoiding a full per-file USB `stat()` walk.
pub fn is_dcim_unchanged(mount_root: &Path, cached: &ScanIndex) -> bool {
    let dcim = mount_root.join("DCIM");
    let Ok(subfolders) = fs::read_dir(&dcim) else {
        return false;
    };

    let mut folder_names = Vec::new();
    let mut media_count = 0usize;

    for entry in subfolders.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        folder_names.push(name);

        let Ok(files) = fs::read_dir(&path) else {
            return false;
        };
        for file_entry in files.flatten() {
            let fname = file_entry.file_name();
            let fname_str = fname.to_string_lossy();
            if has_media_ext(&fname_str) {
                media_count += 1;
            }
        }
    }

    folder_names.sort();
    let cached_folders: Vec<String> = cached.dcim_folders.iter().map(|b| b.key.clone()).collect();
    if folder_names != cached_folders || media_count != cached.items.len() {
        return false;
    }

    // Verify the newest cached item still exists with matching size/mtime
    if let Some(newest) = cached.items.iter().max_by_key(|i| i.mtime) {
        let probe = mount_root.join(&newest.rel_path);
        let Ok(meta) = fs::metadata(&probe) else {
            return false;
        };
        if meta.len() != newest.size {
            return false;
        }
        if let Ok(modified) = meta.modified() {
            if let Ok(dur) = modified.duration_since(UNIX_EPOCH) {
                if dur.as_secs() as i64 != newest.mtime {
                    return false;
                }
            }
        }
    }

    true
}

fn has_media_ext(filename: &str) -> bool {
    const EXTS: &[&str] = &[
        "heic", "heif", "jpg", "jpeg", "png", "dng", "raw", "mov", "mp4", "m4v", "aae", "gif",
    ];
    Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| EXTS.iter().any(|allowed| ext.eq_ignore_ascii_case(allowed)))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner;
    use tempfile::TempDir;

    #[test]
    fn saves_loads_rebases_and_validates_dcim_cache() {
        let mount1 = TempDir::new().unwrap();
        let dcim1 = mount1.path().join("DCIM/100APPLE");
        fs::create_dir_all(&dcim1).unwrap();
        fs::write(dcim1.join("IMG_0001.JPG"), b"photo-one").unwrap();

        let index = scanner::scan_dcim(mount1.path()).unwrap();
        let cache_dir = TempDir::new().unwrap();
        let cache = DeviceCache {
            udid: "00008140-TEST".to_string(),
            index,
            albums: vec![Bucket {
                key: "پښتو".to_string(),
                label: "پښتو".to_string(),
                item_indices: vec![0],
                total_bytes: 9,
            }],
            album_warning: None,
        };

        save_device_cache_to_dir(cache_dir.path(), &cache);
        let mut loaded = load_device_cache_from_dir(cache_dir.path(), "00008140-TEST").unwrap();
        assert_eq!(loaded.albums.len(), 1);
        assert_eq!(loaded.albums[0].label, "پښتو");
        assert!(is_dcim_unchanged(mount1.path(), &loaded.index));

        // Rebase onto a new mount path
        let mount2 = TempDir::new().unwrap();
        rebase_scan_index(&mut loaded.index, mount2.path());
        assert_eq!(
            loaded.index.items[0].path,
            mount2.path().join("DCIM/100APPLE/IMG_0001.JPG")
        );
    }
}
