use std::path::Path;

use anyhow::{Context, Result};
use tera::Tera;

/// Renders `template_name` against `ctx` and writes the result to
/// `out_path`, creating parent directories as needed. Every generation step
/// (Stories R2-R7) funnels through this one helper, so template/IO error
/// context is centralized in one place instead of repeated at each call
/// site. Identical in shape to `targets::typescript::emit::render_and_write`
/// — deliberately duplicated rather than shared, since the two targets'
/// `Tera` instances hold disjoint template sets and sharing this one
/// generic function across them would be the only coupling between two
/// otherwise fully independent target implementations.
pub async fn render_and_write(
    tera: &Tera,
    template_name: &str,
    ctx: &tera::Context,
    out_path: &Path,
) -> Result<()> {
    let rendered = tera.render(template_name, ctx).with_context(|| {
        format!(
            "failed to render template '{template_name}' for '{}'",
            out_path.display()
        )
    })?;

    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create directory '{}'", parent.display()))?;
    }

    tokio::fs::write(out_path, rendered)
        .await
        .with_context(|| format!("failed to write '{}'", out_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Story R1 has no real `.tera` templates yet (those land in R2+), so
    // these tests exercise `render_and_write` against a synthetic template
    // registered directly, rather than one loaded through `render_engine()`.
    fn tera_with_greeting_template() -> Tera {
        let mut tera = Tera::default();
        tera.add_raw_template("greeting.tera", "Hello, {{ name }}!")
            .unwrap();
        tera
    }

    #[tokio::test]
    async fn renders_and_writes_a_template() {
        let tera = tera_with_greeting_template();
        let mut ctx = tera::Context::new();
        ctx.insert("name", "Widget API");

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("greeting.txt");

        render_and_write(&tera, "greeting.tera", &ctx, &out_path)
            .await
            .unwrap();

        let contents = tokio::fs::read_to_string(&out_path).await.unwrap();
        assert_eq!(contents, "Hello, Widget API!");
    }

    #[tokio::test]
    async fn creates_parent_directories() {
        let tera = tera_with_greeting_template();
        let mut ctx = tera::Context::new();
        ctx.insert("name", "X");

        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("nested").join("dir").join("greeting.txt");

        render_and_write(&tera, "greeting.tera", &ctx, &out_path)
            .await
            .unwrap();

        assert!(out_path.exists());
    }
}
