use std::path::Path;
use std::sync::Once;

use anyhow::{Context, Result};
use rusqlite::Connection;

static REGISTER_VEC_EXTENSION: Once = Once::new();

/// Registers the `sqlite-vec` extension once per process — via
/// `sqlite3_auto_extension`, which applies to every `Connection` opened
/// afterwards, matching the pattern the `sqlite-vec` crate itself documents
/// for `rusqlite` — then opens (creating if needed) the database at `path`.
pub fn open_store(path: &Path) -> Result<Connection> {
    REGISTER_VEC_EXTENSION.call_once(|| unsafe {
        #[allow(clippy::missing_transmute_annotations)]
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });

    Connection::open(path).with_context(|| format!("failed to open '{}'", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_a_fresh_database_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_store.db");

        open_store(&path).unwrap();

        assert!(path.exists());
    }

    #[test]
    fn sqlite_vec_extension_is_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_store(&dir.path().join("mcp_store.db")).unwrap();

        let version: String = conn
            .query_row("SELECT vec_version()", [], |row| row.get(0))
            .unwrap();

        assert!(version.starts_with('v'));
    }
}
