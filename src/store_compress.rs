use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// zstd compression level for embedded store `.db` files — matches
/// `schemas_asset`'s level for the generated-schemas asset. Chosen for
/// compression ratio over speed: this runs once per `generate`/`sync`/
/// `add-version` call, not on any hot path.
const ZSTD_LEVEL: i32 = 19;

/// `path`'s zstd sibling, e.g. `mcp_store.db` -> `mcp_store.db.zst`.
pub fn zst_sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".zst");
    PathBuf::from(name)
}

/// Compresses the raw file at `path` into its `.zst` sibling and removes
/// `path` — only Rust's target embeds a store via `include_bytes!` (see
/// `targets::rust::templates::data::store.rs.tera`'s `VERSION_STORE_BYTES`),
/// so this is only ever called from Rust-specific code paths; every other
/// target reads `mcp_store*.db` straight off disk at runtime and must keep
/// the raw file. A no-op if `path` doesn't already exist: callers sweep a
/// whole ledger's worth of store paths unconditionally each time, and a
/// store that's already compressed (raw file removed by an earlier sweep)
/// is the normal steady state, not an exceptional one.
pub async fn compress_and_remove_raw(path: &Path) -> Result<()> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(());
    }
    let raw = tokio::fs::read(path)
        .await
        .with_context(|| format!("failed to read '{}'", path.display()))?;
    let compressed = zstd::stream::encode_all(raw.as_slice(), ZSTD_LEVEL)
        .with_context(|| format!("failed to zstd-compress '{}'", path.display()))?;
    let zst_path = zst_sibling(path);
    tokio::fs::write(&zst_path, compressed)
        .await
        .with_context(|| format!("failed to write '{}'", zst_path.display()))?;
    tokio::fs::remove_file(path).await.with_context(|| {
        format!(
            "failed to remove raw '{}' after compressing",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zst_sibling_appends_the_extension() {
        assert_eq!(
            zst_sibling(Path::new("mcp_store.db")),
            PathBuf::from("mcp_store.db.zst")
        );
        assert_eq!(
            zst_sibling(Path::new("mcp_store_v11.2.db")),
            PathBuf::from("mcp_store_v11.2.db.zst")
        );
    }

    #[tokio::test]
    async fn compresses_the_raw_file_and_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_store.db");
        tokio::fs::write(&path, b"some sqlite bytes").await.unwrap();

        compress_and_remove_raw(&path).await.unwrap();

        assert!(!path.exists());
        let zst_path = zst_sibling(&path);
        assert!(zst_path.is_file());
        let decompressed =
            zstd::decode_all(tokio::fs::read(&zst_path).await.unwrap().as_slice()).unwrap();
        assert_eq!(decompressed, b"some sqlite bytes");
    }

    #[tokio::test]
    async fn is_a_no_op_when_the_raw_file_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_store.db");

        compress_and_remove_raw(&path).await.unwrap();

        assert!(!zst_sibling(&path).exists());
    }
}
