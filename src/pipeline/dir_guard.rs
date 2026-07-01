use std::path::Path;

use anyhow::{Result, bail};

/// REQ-1.1.3: abort if the output directory exists and is non-empty, unless
/// `--force` is set. Returns whether `output_dir` already existed with
/// content before this call (`output_dir_preexisted`), so a failed run later
/// (architecture.md §1's rollback in `execute()`) knows whether it's safe to
/// delete the directory.
pub async fn check_output_dir(output_dir: &Path, force: bool) -> Result<bool> {
    let metadata = match tokio::fs::metadata(output_dir).await {
        Ok(metadata) => metadata,
        Err(_) => {
            tokio::fs::create_dir_all(output_dir).await?;
            return Ok(false);
        }
    };

    if !metadata.is_dir() {
        bail!(
            "output path '{}' exists and is not a directory",
            output_dir.display()
        );
    }

    let mut entries = tokio::fs::read_dir(output_dir).await?;
    let is_empty = entries.next_entry().await?.is_none();

    if is_empty || force {
        Ok(true)
    } else {
        bail!(
            "output directory '{}' is not empty; pass --force to overwrite",
            output_dir.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn creates_missing_dir_and_reports_not_preexisted() {
        let parent = tempfile::tempdir().unwrap();
        let target = parent.path().join("generated");

        let preexisted = check_output_dir(&target, false).await.unwrap();

        assert!(!preexisted);
        assert!(target.is_dir());
    }

    #[tokio::test]
    async fn empty_existing_dir_reports_preexisted() {
        let dir = tempfile::tempdir().unwrap();

        let preexisted = check_output_dir(dir.path(), false).await.unwrap();

        assert!(preexisted);
    }

    #[tokio::test]
    async fn non_empty_dir_without_force_errors() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("existing.txt"), b"content")
            .await
            .unwrap();

        let err = check_output_dir(dir.path(), false).await.unwrap_err();

        assert!(err.to_string().contains("pass --force"));
    }

    #[tokio::test]
    async fn non_empty_dir_with_force_succeeds_and_reports_preexisted() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("existing.txt"), b"content")
            .await
            .unwrap();

        let preexisted = check_output_dir(dir.path(), true).await.unwrap();

        assert!(preexisted);
        // --force must not touch existing content itself; that's the
        // target's job during generation, not the guard's.
        assert!(dir.path().join("existing.txt").exists());
    }
}
