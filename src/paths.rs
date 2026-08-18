use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use rusqlite::{Connection, Error as SqliteError, ErrorCode, OpenFlags};

pub fn package_root() -> PathBuf {
    env::var_os("ZCLI_PACKAGE_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

pub(crate) fn sqlite_readonly_uri(path: &Path) -> String {
    sqlite_uri(path, false)
}

fn sqlite_immutable_uri(path: &Path) -> String {
    sqlite_uri(path, true)
}

fn sqlite_uri(path: &Path, immutable: bool) -> String {
    let raw = path.to_string_lossy();
    let mut escaped = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b' ' => escaped.push_str("%20"),
            b'#' => escaped.push_str("%23"),
            b'?' => escaped.push_str("%3F"),
            b'%' => escaped.push_str("%25"),
            _ => escaped.push(byte as char),
        }
    }
    format!(
        "file:{escaped}?mode=ro{}",
        if immutable { "&immutable=1" } else { "" }
    )
}

pub(crate) fn open_sqlite_readonly(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    if let Ok(conn) = Connection::open_with_flags(sqlite_readonly_uri(path), flags) {
        conn.busy_timeout(Duration::from_millis(250))?;
        match conn.query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0)) {
            Ok(_) => return Ok(conn),
            Err(error) if sqlite_is_busy(&error) => {}
            Err(error) => return Err(error.into()),
        }
    }

    // Zotero can hold an exclusive SQLite lock while it is running. Preserve
    // the historical always-readable snapshot path in that case; mutation
    // execution verifies fresh state through the Local API itself.
    Connection::open_with_flags(sqlite_immutable_uri(path), flags)
        .or_else(|_| Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY))
        .with_context(|| format!("failed to open SQLite database {}", path.display()))
}

fn sqlite_is_busy(error: &SqliteError) -> bool {
    matches!(
        error,
        SqliteError::SqliteFailure(details, _)
            if matches!(details.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_sqlite_uri_keeps_wal_visibility() {
        assert_eq!(
            sqlite_readonly_uri(Path::new("/tmp/Zotero data/#main?.sqlite")),
            "file:/tmp/Zotero%20data/%23main%3F.sqlite?mode=ro"
        );
    }

    #[test]
    fn readonly_connection_observes_committed_wal_rows() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("zotero.sqlite");
        let writer = Connection::open(&path).unwrap();
        writer.pragma_update(None, "journal_mode", "WAL").unwrap();
        writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        writer
            .execute_batch("CREATE TABLE items(value TEXT); PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        writer
            .execute("INSERT INTO items(value) VALUES ('current')", [])
            .unwrap();

        let reader = open_sqlite_readonly(&path).unwrap();
        assert_eq!(
            reader
                .query_row("SELECT value FROM items", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "current"
        );
    }
}
