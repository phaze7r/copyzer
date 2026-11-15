use crate::scanner::{Bucket, ScanIndex};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FALLBACK_MSG: &str = "PhotoData/Photos.sqlite not accessible; using Year/Month & DCIM views";

#[derive(Debug, Clone, Serialize)]
pub struct AlbumScanResult {
    pub albums: Vec<Bucket>,
    pub warning: Option<String>,
}

pub fn load_albums(mount_root: &Path, index: &ScanIndex) -> AlbumScanResult {
    load_albums_with_udid(mount_root, index, None)
}

pub fn load_albums_with_udid(
    mount_root: &Path,
    index: &ScanIndex,
    udid: Option<&str>,
) -> AlbumScanResult {
    let staged_dir = tempfile::Builder::new().prefix("copyzer-db-").tempdir().ok();
    let db_path = match resolve_or_stage_sqlite(mount_root, udid, staged_dir.as_ref().map(|d| d.path())) {
        Some(path) => path,
        None => {
            return AlbumScanResult {
                albums: Vec::new(),
                warning: Some(FALLBACK_MSG.to_string()),
            };
        }
    };

    let base_flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let uri = format!("file:{}?immutable=1", db_path.display());
    let conn = match Connection::open_with_flags(&uri, base_flags | OpenFlags::SQLITE_OPEN_URI)
        .or_else(|_| Connection::open_with_flags(&db_path, base_flags))
    {
        Ok(c) => c,
        Err(err) => {
            return AlbumScanResult {
                albums: Vec::new(),
                warning: Some(format!(
                    "PhotoData/Photos.sqlite not accessible ({err}); using Year/Month & DCIM views"
                )),
            };
        }
    };
    let _ = conn.execute_batch("PRAGMA query_only = ON; PRAGMA cache_size = -16000;");

    match query_albums(&conn, index) {
        Ok(albums) => AlbumScanResult {
            albums,
            warning: None,
        },
        Err(reason) => AlbumScanResult {
            albums: Vec::new(),
            warning: Some(format!(
                "PhotoData/Photos.sqlite skipped ({reason}); using Year/Month & DCIM views"
            )),
        },
    }
}

/// Locates `Photos.sqlite` on the mount (case-insensitively) and copies it (+ `-wal`/`-shm`)
/// into a local staging directory so SQLite never fails on FUSE random-seek / lock `EIO`.
/// Falls back to `afcclient get` when direct FUSE access fails.
fn resolve_or_stage_sqlite(
    mount_root: &Path,
    udid: Option<&str>,
    stage_dir: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(src_db) = find_sqlite_on_mount(mount_root) {
        if let Some(dir) = stage_dir {
            let staged_db = dir.join("Photos.sqlite");
            if fs::copy(&src_db, &staged_db).is_ok() {
                for suffix in ["-wal", "-shm"] {
                    let mut src_companion = src_db.as_os_str().to_os_string();
                    src_companion.push(suffix);
                    let src_companion = PathBuf::from(src_companion);
                    if src_companion.is_file() {
                        let _ = fs::copy(&src_companion, dir.join(format!("Photos.sqlite{suffix}")));
                    }
                }
                return Some(staged_db);
            }
        }
        // If local staging failed (e.g. temp dir full) but source file is readable, try direct path.
        if fs::metadata(&src_db).is_ok_and(|m| m.len() > 0) {
            return Some(src_db);
        }
    }

    // Fallback: pull Photos.sqlite directly over AFC via `afcclient` when FUSE hides or blocks PhotoData.
    let stage = stage_dir?;
    stage_via_afcclient(udid, stage)
}

fn find_sqlite_on_mount(mount_root: &Path) -> Option<PathBuf> {
    let candidates = [
        "PhotoData/Photos.sqlite",
        "PhotoData/Photos.sqlite_aside",
        "Media/PhotoData/Photos.sqlite",
    ];
    for rel in candidates {
        let p = mount_root.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }

    let entries = fs::read_dir(mount_root).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().eq_ignore_ascii_case("photodata") {
            let dir_path = entry.path();
            let sub_entries = fs::read_dir(&dir_path).ok()?;
            for sub in sub_entries.flatten() {
                if sub
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("photos.sqlite")
                {
                    let p = sub.path();
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

fn stage_via_afcclient(udid: Option<&str>, stage_dir: &Path) -> Option<PathBuf> {
    let staged_db = stage_dir.join("Photos.sqlite");
    for remote in [
        "PhotoData/Photos.sqlite",
        "PhotoData/Photos.sqlite-wal",
        "PhotoData/Photos.sqlite-shm",
    ] {
        let local_name = Path::new(remote).file_name()?.to_str()?;
        let local_target = stage_dir.join(local_name);
        let mut cmd = Command::new("afcclient");
        if let Some(u) = udid.filter(|u| !u.is_empty() && *u != "local") {
            cmd.args(["-u", u]);
        }
        let _ = cmd.arg("get").arg(remote).arg(&local_target).output();
    }

    if staged_db.is_file() && fs::metadata(&staged_db).is_ok_and(|m| m.len() > 0) {
        Some(staged_db)
    } else {
        None
    }
}

fn query_albums(conn: &Connection, index: &ScanIndex) -> Result<Vec<Bucket>, String> {
    if !table_exists(conn, "ZGENERICALBUM")? {
        return Err("missing ZGENERICALBUM".to_string());
    }
    let asset_table = if table_exists(conn, "ZASSET")? {
        "ZASSET"
    } else if table_exists(conn, "ZGENERICASSET")? {
        "ZGENERICASSET"
    } else {
        return Err("missing ZASSET or ZGENERICASSET".to_string());
    };

    let has_zkind = column_exists(conn, "ZGENERICALBUM", "ZKIND")?;
    // ZKIND = 2 is a user-created album on iOS; exclude system/smart/syndication albums.
    let kind_filter = if has_zkind { "AND al.ZKIND = 2" } else { "" };

    let trashed_filter = if column_exists(conn, "ZGENERICALBUM", "ZTRASHEDSTATE")? {
        "AND COALESCE(al.ZTRASHEDSTATE, 0) = 0"
    } else {
        ""
    };

    let asset_trashed_filter = if column_exists(conn, asset_table, "ZTRASHEDSTATE")? {
        "AND COALESCE(ast.ZTRASHEDSTATE, 0) = 0"
    } else {
        ""
    };

    let (join_table, album_col, asset_col) =
        find_album_join_table(conn, asset_table, kind_filter, trashed_filter)?;

    let sql = format!(
        "SELECT al.ZTITLE, ast.ZDIRECTORY, ast.ZFILENAME
         FROM ZGENERICALBUM al
         JOIN \"{join_table}\" j ON j.\"{album_col}\" = al.Z_PK
         JOIN \"{asset_table}\" ast ON ast.Z_PK = j.\"{asset_col}\"
         WHERE al.ZTITLE IS NOT NULL AND TRIM(al.ZTITLE) != ''
         {kind_filter} {trashed_filter} {asset_trashed_filter}"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let title: String = row.get(0)?;
            let dir: Option<String> = row.get(1)?;
            let filename: Option<String> = row.get(2)?;
            Ok((title, dir, filename))
        })
        .map_err(|e| e.to_string())?;

    let lookup = build_item_lookup(index);
    let mut grouped: BTreeMap<String, (Vec<usize>, HashSet<usize>, u64)> = BTreeMap::new();

    for row in rows {
        let (title, dir, filename) = row.map_err(|e| e.to_string())?;
        let Some(fname) = filename else {
            continue;
        };
        let Some(item_idx) = match_item(&lookup, dir.as_deref(), &fname) else {
            continue;
        };

        let key = title.trim().to_string();
        let entry = grouped
            .entry(key)
            .or_insert_with(|| (Vec::new(), HashSet::new(), 0));
        if entry.1.insert(item_idx) {
            entry.0.push(item_idx);
            entry.2 = entry.2.saturating_add(index.items[item_idx].size);
        }
    }

    let mut albums: Vec<Bucket> = grouped
        .into_iter()
        .map(|(title, (item_indices, _, total_bytes))| Bucket {
            key: title.clone(),
            label: title,
            item_indices,
            total_bytes,
        })
        .collect();
    albums.sort_by_key(|a| a.label.to_ascii_lowercase());
    Ok(albums)
}

struct ItemLookup {
    by_rel_path: HashMap<String, usize>,
    by_filename: HashMap<String, usize>,
}

fn build_item_lookup(index: &ScanIndex) -> ItemLookup {
    let mut by_rel_path = HashMap::with_capacity(index.items.len());
    let mut by_filename = HashMap::with_capacity(index.items.len());
    for (idx, item) in index.items.iter().enumerate() {
        by_rel_path.insert(item.rel_path.to_ascii_lowercase(), idx);
        by_filename
            .entry(item.filename.to_ascii_lowercase())
            .or_insert(idx);
    }
    ItemLookup {
        by_rel_path,
        by_filename,
    }
}

fn match_item(lookup: &ItemLookup, dir: Option<&str>, filename: &str) -> Option<usize> {
    let fname_clean = filename.trim_matches('/');
    if fname_clean.is_empty() {
        return None;
    }

    let dir = dir.unwrap_or("").trim_matches('/');
    if !dir.is_empty() {
        let direct = format!("{dir}/{fname_clean}").to_ascii_lowercase();
        if let Some(&idx) = lookup.by_rel_path.get(&direct) {
            return Some(idx);
        }
        let with_dcim = format!("dcim/{dir}/{fname_clean}").to_ascii_lowercase();
        if let Some(&idx) = lookup.by_rel_path.get(&with_dcim) {
            return Some(idx);
        }
        if let Some(last_seg) = dir.rsplit('/').next() {
            let short_dcim = format!("dcim/{last_seg}/{fname_clean}").to_ascii_lowercase();
            if let Some(&idx) = lookup.by_rel_path.get(&short_dcim) {
                return Some(idx);
            }
        }
    }

    lookup
        .by_filename
        .get(&fname_clean.to_ascii_lowercase())
        .copied()
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        [name],
        |_| Ok(()),
    )
    .map(|()| true)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(false),
        other => Err(other.to_string()),
    })
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    if !is_safe_ident(table) {
        return Err(format!("invalid table identifier: {table}"));
    }
    let pragma = format!("PRAGMA table_info(\"{table}\")");
    let mut stmt = conn.prepare(&pragma).map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let name: String = row.get(1).map_err(|e| e.to_string())?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn find_album_join_table(
    conn: &Connection,
    asset_table: &str,
    kind_filter: &str,
    trashed_filter: &str,
) -> Result<(String, String, String), String> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name LIKE 'Z\\_%ASSETS' ESCAPE '\\'
             ORDER BY name",
        )
        .map_err(|e| e.to_string())?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    let mut any_valid_join = None;
    let mut fallback = None;

    for table in names {
        if !is_safe_ident(&table) {
            continue;
        }
        let pragma = format!("PRAGMA table_info(\"{table}\")");
        let mut col_stmt = conn.prepare(&pragma).map_err(|e| e.to_string())?;
        let cols: Vec<String> = col_stmt
            .query_map([], |row| row.get(1))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;

        let album_col = cols
            .iter()
            .find(|c| c.starts_with("Z_") && c.ends_with("ALBUMS") && is_safe_ident(c))
            .cloned();
        let asset_col = cols
            .iter()
            .find(|c| c.starts_with("Z_") && c.ends_with("ASSETS") && is_safe_ident(c))
            .cloned();

        if let (Some(al), Some(ast)) = (album_col, asset_col) {
            let probe_user_albums = format!(
                "SELECT 1 FROM \"{table}\" j
                 JOIN ZGENERICALBUM al ON al.Z_PK = j.\"{al}\"
                 JOIN \"{asset_table}\" ast ON ast.Z_PK = j.\"{ast}\"
                 WHERE al.ZTITLE IS NOT NULL AND TRIM(al.ZTITLE) != ''
                 {kind_filter} {trashed_filter}
                 LIMIT 1"
            );
            if conn.query_row(&probe_user_albums, [], |_| Ok(())).is_ok() {
                return Ok((table, al, ast));
            }

            if any_valid_join.is_none() {
                let has_rows = conn
                    .query_row(&format!("SELECT 1 FROM \"{table}\" LIMIT 1"), [], |_| Ok(()))
                    .is_ok();
                if has_rows {
                    any_valid_join = Some((table.clone(), al.clone(), ast.clone()));
                }
            }
            if fallback.is_none() {
                fallback = Some((table, al, ast));
            }
        }
    }

    any_valid_join
        .or(fallback)
        .ok_or_else(|| "no album-to-asset join table found".to_string())
}

fn is_safe_ident(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::scan_dcim;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn missing_sqlite_returns_empty_with_warning() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("DCIM/100APPLE")).unwrap();
        let index = scan_dcim(tmp.path()).unwrap();

        let res = load_albums(tmp.path(), &index);
        assert!(res.albums.is_empty());
        assert_eq!(res.warning.as_deref(), Some(FALLBACK_MSG));
    }

    #[test]
    fn maps_albums_from_synthetic_photos_sqlite() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("DCIM/100APPLE")).unwrap();
        fs::create_dir_all(root.join("PhotoData")).unwrap();
        fs::write(root.join("DCIM/100APPLE/IMG_0001.HEIC"), b"12345").unwrap();
        fs::write(root.join("DCIM/100APPLE/IMG_0002.JPG"), b"12345678").unwrap();

        let db_path = root.join("PhotoData/Photos.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE ZGENERICALBUM (Z_PK INTEGER PRIMARY KEY, ZTITLE TEXT, ZTRASHEDSTATE INTEGER);
             CREATE TABLE ZASSET (Z_PK INTEGER PRIMARY KEY, ZDIRECTORY TEXT, ZFILENAME TEXT);
             CREATE TABLE Z_28ASSETS (Z_28ALBUMS INTEGER, Z_28ASSETS INTEGER);
             INSERT INTO ZGENERICALBUM VALUES (1, 'Vacation', 0);
             INSERT INTO ZGENERICALBUM VALUES (2, 'Trashed', 1);
             INSERT INTO ZASSET VALUES (10, 'DCIM/100APPLE', 'IMG_0001.HEIC');
             INSERT INTO ZASSET VALUES (11, 'DCIM/100APPLE', 'IMG_0002.JPG');
             INSERT INTO Z_28ASSETS VALUES (1, 10);
             INSERT INTO Z_28ASSETS VALUES (1, 11);
             INSERT INTO Z_28ASSETS VALUES (2, 10);",
        )
        .unwrap();
        drop(conn);

        let index = scan_dcim(root).unwrap();
        let res = load_albums(root, &index);
        assert!(res.warning.is_none());
        assert_eq!(res.albums.len(), 1);
        assert_eq!(res.albums[0].label, "Vacation");
        assert_eq!(res.albums[0].item_indices, vec![0, 1]);
        assert_eq!(res.albums[0].total_bytes, 13);
    }

    #[test]
    fn unexpected_schema_returns_empty_with_warning() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("DCIM")).unwrap();
        fs::create_dir_all(root.join("PhotoData")).unwrap();

        let conn = Connection::open(root.join("PhotoData/Photos.sqlite")).unwrap();
        conn.execute_batch("CREATE TABLE unrelated (id INTEGER PRIMARY KEY);")
            .unwrap();
        drop(conn);

        let index = scan_dcim(root).unwrap();
        let res = load_albums(root, &index);
        assert!(res.albums.is_empty());
        assert!(res.warning.is_some());
    }

    #[test]
    fn filters_to_user_created_albums_and_selects_correct_join_table() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("DCIM/100APPLE")).unwrap();
        fs::create_dir_all(root.join("PhotoData")).unwrap();
        fs::write(root.join("DCIM/100APPLE/IMG_0001.HEIC"), b"12345").unwrap();
        fs::write(root.join("DCIM/100APPLE/IMG_0002.JPG"), b"12345678").unwrap();

        let db_path = root.join("PhotoData/Photos.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE ZGENERICALBUM (Z_PK INTEGER PRIMARY KEY, ZTITLE TEXT, ZKIND INTEGER, ZTRASHEDSTATE INTEGER);
             CREATE TABLE ZASSET (Z_PK INTEGER PRIMARY KEY, ZDIRECTORY TEXT, ZFILENAME TEXT);
             -- Decoy join table alphabetically before Z_28ASSETS with non-user album link
             CREATE TABLE Z_10ASSETS (Z_10ALBUMS INTEGER, Z_30ASSETS INTEGER);
             CREATE TABLE Z_28ASSETS (Z_28ALBUMS INTEGER, Z_34ASSETS INTEGER);
             INSERT INTO ZGENERICALBUM VALUES (1, 'My Trip', 2, 0);
             INSERT INTO ZGENERICALBUM VALUES (2, 'Favorites', 1600, 0);
             INSERT INTO ZGENERICALBUM VALUES (3, 'Shared Stream', 1505, 0);
             INSERT INTO ZGENERICALBUM VALUES (4, 'Recent Imports', 1506, 0);
             INSERT INTO ZASSET VALUES (10, '100APPLE', 'IMG_0001.HEIC');
             INSERT INTO ZASSET VALUES (11, NULL, 'IMG_0002.JPG');
             INSERT INTO Z_10ASSETS VALUES (999, 10);
             INSERT INTO Z_28ASSETS VALUES (1, 10);
             INSERT INTO Z_28ASSETS VALUES (1, 11);
             INSERT INTO Z_28ASSETS VALUES (2, 10);
             INSERT INTO Z_28ASSETS VALUES (3, 11);",
        )
        .unwrap();
        drop(conn);

        let index = scan_dcim(root).unwrap();
        let res = load_albums(root, &index);
        assert!(res.warning.is_none());
        assert_eq!(res.albums.len(), 1);
        assert_eq!(res.albums[0].label, "My Trip");
        assert_eq!(res.albums[0].item_indices, vec![0, 1]);
        assert_eq!(res.albums[0].total_bytes, 13);
    }

    #[test]
    fn supports_legacy_zgenericasset_table_and_case_insensitive_photodata() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("DCIM/100APPLE")).unwrap();
        fs::create_dir_all(root.join("photodata")).unwrap();
        fs::write(root.join("DCIM/100APPLE/IMG_0001.JPG"), b"1234567").unwrap();

        let db_path = root.join("photodata/Photos.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE ZGENERICALBUM (Z_PK INTEGER PRIMARY KEY, ZTITLE TEXT, ZKIND INTEGER);
             CREATE TABLE ZGENERICASSET (Z_PK INTEGER PRIMARY KEY, ZDIRECTORY TEXT, ZFILENAME TEXT);
             CREATE TABLE Z_26ASSETS (Z_26ALBUMS INTEGER, Z_34ASSETS INTEGER);
             INSERT INTO ZGENERICALBUM VALUES (1, 'Family', 2);
             INSERT INTO ZGENERICASSET VALUES (10, 'DCIM/100APPLE', 'IMG_0001.JPG');
             INSERT INTO Z_26ASSETS VALUES (1, 10);",
        )
        .unwrap();
        drop(conn);

        let index = scan_dcim(root).unwrap();
        let res = load_albums(root, &index);
        assert!(res.warning.is_none());
        assert_eq!(res.albums.len(), 1);
        assert_eq!(res.albums[0].label, "Family");
        assert_eq!(res.albums[0].total_bytes, 7);
    }
}
