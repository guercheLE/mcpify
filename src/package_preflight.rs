use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizedPath {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSizeReport {
    pub total_bytes: u64,
    pub largest: Vec<SizedPath>,
}

impl PackageSizeReport {
    pub fn enforce(&self, limit_bytes: u64) -> Result<()> {
        if self.total_bytes <= limit_bytes {
            return Ok(());
        }
        let contributors = self
            .largest
            .iter()
            .take(10)
            .map(|item| format!("{} ({} B)", item.path.display(), item.bytes))
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "package preflight size {} B exceeds configured limit {} B; largest contributors: {}",
            self.total_bytes,
            limit_bytes,
            contributors
        )
    }
}

pub fn analyze_tree(root: &Path) -> Result<PackageSizeReport> {
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    let total_bytes = files.iter().map(|file| file.bytes).sum();
    Ok(PackageSizeReport {
        total_bytes,
        largest: files,
    })
}

pub fn enforce_project_limit(ctx: &crate::context::GeneratorContext) -> Result<()> {
    let settings = crate::project_config::read_settings(&ctx.output_dir);
    if let Some(limit) = settings.package_size_limit_bytes {
        analyze_tree(&ctx.output_dir)?.enforce(limit)?;
    }
    Ok(())
}

pub fn enforce_artifact_limit(
    ctx: &crate::context::GeneratorContext,
    extensions: &[&str],
) -> Result<()> {
    let settings = crate::project_config::read_settings(&ctx.output_dir);
    let Some(limit) = settings.package_size_limit_bytes else {
        return Ok(());
    };
    let mut artifacts = Vec::new();
    collect_matching(&ctx.output_dir, &ctx.output_dir, extensions, &mut artifacts)?;
    let artifact = artifacts
        .into_iter()
        .max_by_key(|item| item.bytes)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "package preflight produced no {} artifact",
                extensions.join("/")
            )
        })?;
    if artifact.bytes > limit {
        let tree = analyze_tree(&ctx.output_dir)?;
        let contributors = tree
            .largest
            .iter()
            .take(10)
            .map(|item| format!("{} ({} B)", item.path.display(), item.bytes))
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "package artifact {} is {} B, exceeding configured limit {} B; largest source contributors: {}",
            artifact.path.display(),
            artifact.bytes,
            limit,
            contributors
        );
    }
    Ok(())
}

fn collect(root: &Path, current: &Path, files: &mut Vec<SizedPath>) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if entry.file_type()?.is_dir() {
            let always_skip = matches!(
                name.to_str(),
                Some(".git" | ".fastembed_cache" | "target" | "node_modules" | ".venv")
            );
            let root_build_dir = current == root && matches!(name.to_str(), Some("bin" | "obj"));
            if always_skip || root_build_dir {
                continue;
            }
            collect(root, &path, files)?;
        } else if entry.file_type()?.is_file() {
            files.push(SizedPath {
                path: path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                bytes: entry.metadata()?.len(),
            });
        }
    }
    Ok(())
}

fn collect_matching(
    root: &Path,
    current: &Path,
    extensions: &[&str],
    files: &mut Vec<SizedPath>,
) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            if entry.file_name() != ".git" {
                collect_matching(root, &path, extensions, files)?;
            }
        } else if entry.file_type()?.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extensions.contains(&extension))
        {
            files.push(SizedPath {
                path: path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                bytes: entry.metadata()?.len(),
            });
        }
    }
    Ok(())
}
