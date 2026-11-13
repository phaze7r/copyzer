use crate::scanner::{Bucket, ScanIndex};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

const FALLBACK_MSG: &str = "PhotoData/Photos.sqlite not accessible; using Year/Month & DCIM views";

#[derive(Debug, Clone, Serialize)]
pub struct AlbumScanResult {
    pub albums: Vec<Bucket>,
    pub warning: Option<String>,
}

pub fn load_albums(mount_root: &Path, index: &ScanIndex) -> AlbumScanResult {
    let db_path = mount_root.join("PhotoData/Photos.sqlite");
    if !db_path.is_file() {
        return AlbumScanResult {
            albums: Vec::new(),
            warning: Some(FALLBACK_MSG.to_string()),
        };
    }

    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = match Connection::open_with_flags(&db_path, flags) {
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

fn query_albums(conn: &Connection, index: &ScanIndex) -> Result<Vec<Bucket>, String> {
    if !table_exists(conn, "ZGENERICALBUM")? || !table_exists(conn, "ZASSET")? {
        return Err("missing ZGENERICALBUM or ZASSET".to_string());
    }

    let (join_table, album_col, asset_col) = find_album_join_table(conn)?;
    let trashed_filter = if column_exists(conn, "ZGENERICALBUM", "ZTRASHEDSTATE")? {
        "AND COALESCE(al.ZTRASHEDSTATE, 0) = 0"
    } else {
        ""
    };

    let sql = format!(
        "SELECT al.ZTITLE, ast.ZDIRECTORY, ast.ZFILENAME
         FROM ZGENERICALBUM al
         JOIN \"{join_table}\" j ON j.\"{album_col}\" = al.Z_PK
         JOIN ZASSET ast ON ast.Z_PK = j.\"{asset_col}\"
         WHERE al.ZTITLE IS NOT NULL AND TRIM(al.ZTITLE) != '' {trashed_filter}"
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

fn build_item_lookup(index: &ScanIndex) -> HashMap<String, usize> {
    let mut map = HashMap::with_capacity(index.items.len());
    for (idx, item) in index.items.iter().enumerate() {
        map.insert(item.rel_path.to_ascii_lowercase(), idx);
    }
    map
}

fn match_item(
    lookup: &HashMap<String, usize>,
    dir: Option<&str>,
    filename: &str,
) -> Option<usize> {
    let dir = dir.unwrap_or("").trim_matches('/');
    if !dir.is_empty() {
        let direct = format!("{dir}/{filename}").to_ascii_lowercase();
        if let Some(&idx) = lookup.get(&direct) {
            return Some(idx);
        }
        let with_dcim = format!("dcim/{dir}/{filename}").to_ascii_lowercase();
        if let Some(&idx) = lookup.get(&with_dcim) {
            return Some(idx);
        }
    }
    None
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

fn find_album_join_table(conn: &Connection) -> Result<(String, String, String), String> {
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
            return Ok((table, al, ast));
        }
    }

    Err("no album-to-asset join table found".to_string())
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
}
