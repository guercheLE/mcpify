use std::path::Path;

use anyhow::{Context, Result};

/// Every version-aware generated file has exactly one region delimited by
/// these two marker lines (in that target language's comment syntax, e.g.
/// `// mcpify:versions:begin` for TypeScript/Rust/Go/C#, `# mcpify:versions:begin`
/// for Python) surrounding a small, purely-data code literal (a label -> file
/// name map, or a list of choices) — never logic. `add-version` regenerates
/// just that literal in each of a target's version-aware files and splices
/// it back in via [`patch_marked_region`], without needing to reconstruct
/// the full Tera rendering context (auth schemes, project name, etc.) that
/// produced the rest of the file at `generate` time.
pub const BEGIN_MARKER: &str = "mcpify:versions:begin";
pub const END_MARKER: &str = "mcpify:versions:end";

/// Replaces the text strictly between a `{comment} mcpify:versions:begin`
/// line and a `{comment} mcpify:versions:end` line with `new_body`, leaving
/// both marker lines and everything outside them untouched. The marker
/// syntax is matched as a plain substring, so it works the same regardless
/// of which language's comment syntax wraps it.
pub async fn patch_marked_region(file_path: &Path, new_body: &str) -> Result<()> {
    let original = tokio::fs::read_to_string(file_path)
        .await
        .with_context(|| {
            format!(
                "failed to read '{}' while syncing versions",
                file_path.display()
            )
        })?;

    let patched = replace_marked_region(&original, new_body).with_context(|| {
        format!(
            "'{}' is missing mcpify's version markers",
            file_path.display()
        )
    })?;

    tokio::fs::write(file_path, patched).await.with_context(|| {
        format!(
            "failed to write '{}' after syncing versions",
            file_path.display()
        )
    })
}

fn replace_marked_region(original: &str, new_body: &str) -> Result<String> {
    let begin_idx = original
        .find(BEGIN_MARKER)
        .with_context(|| format!("no '{BEGIN_MARKER}' marker found"))?;
    let begin_line_start = original[..begin_idx]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let begin_line_end = original[begin_idx..]
        .find('\n')
        .map(|i| begin_idx + i + 1)
        .with_context(|| format!("'{BEGIN_MARKER}' marker's line was never terminated"))?;

    let end_idx = original[begin_line_end..]
        .find(END_MARKER)
        .map(|i| begin_line_end + i)
        .with_context(|| format!("no matching '{END_MARKER}' marker found"))?;
    let end_line_start = original[..end_idx].rfind('\n').map(|i| i + 1).unwrap_or(0);

    // Whitespace-sensitive languages (Python) can place the marker inside
    // an indented block (e.g. a class body); re-applying the begin
    // marker's own leading indentation to every line of `new_body` lets
    // body-renderer functions always generate flush-left text and still
    // land at the correct indentation wherever the marker actually sits,
    // matching what the original Tera template rendered. Only the
    // *whitespace* prefix counts as indentation — the line up to the
    // marker text may also contain comment syntax (`// `, `# `), which
    // must not be duplicated onto every body line.
    let line_prefix = &original[begin_line_start..begin_idx];
    let indent: String = line_prefix
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let indented_body = if indent.is_empty() {
        new_body.to_string()
    } else {
        new_body
            .lines()
            .map(|line| {
                if line.is_empty() {
                    line.to_string()
                } else {
                    format!("{indent}{line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + if new_body.ends_with('\n') { "\n" } else { "" }
    };

    let mut patched = String::with_capacity(original.len() + indented_body.len());
    patched.push_str(&original[..begin_line_end]);
    patched.push_str(&indented_body);
    if !indented_body.is_empty() && !indented_body.ends_with('\n') {
        patched.push('\n');
    }
    patched.push_str(&original[end_line_start..]);
    Ok(patched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replaces_only_the_marked_region() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.ts");
        tokio::fs::write(
            &path,
            "before\n// mcpify:versions:begin\nold body\n// mcpify:versions:end\nafter\n",
        )
        .await
        .unwrap();

        patch_marked_region(&path, "new body\n").await.unwrap();

        let contents = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(
            contents,
            "before\n// mcpify:versions:begin\nnew body\n// mcpify:versions:end\nafter\n"
        );
    }

    #[tokio::test]
    async fn adds_a_trailing_newline_when_the_body_lacks_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.py");
        tokio::fs::write(
            &path,
            "# mcpify:versions:begin\nold\n# mcpify:versions:end\n",
        )
        .await
        .unwrap();

        patch_marked_region(&path, "new").await.unwrap();

        let contents = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(
            contents,
            "# mcpify:versions:begin\nnew\n# mcpify:versions:end\n"
        );
    }

    #[tokio::test]
    async fn is_idempotent_across_repeated_patches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.go");
        tokio::fs::write(
            &path,
            "// mcpify:versions:begin\noriginal\n// mcpify:versions:end\n",
        )
        .await
        .unwrap();

        patch_marked_region(&path, "first\n").await.unwrap();
        patch_marked_region(&path, "second\n").await.unwrap();

        let contents = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(
            contents,
            "// mcpify:versions:begin\nsecond\n// mcpify:versions:end\n"
        );
    }

    #[tokio::test]
    async fn reindents_a_flush_left_body_to_match_an_indented_marker() {
        // Mirrors Python's `config.py`, where the marker sits inside an
        // indented class body — the patcher must not dedent the class
        // field it's replacing, or the file becomes invalid Python.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.py");
        tokio::fs::write(
            &path,
            "class Config:\n    url: str\n    # mcpify:versions:begin\n    api_version: str = \"old\"\n    # mcpify:versions:end\n    log_level: str = \"info\"\n",
        )
        .await
        .unwrap();

        patch_marked_region(&path, "api_version: str = \"new\"\n")
            .await
            .unwrap();

        let contents = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(
            contents,
            "class Config:\n    url: str\n    # mcpify:versions:begin\n    api_version: str = \"new\"\n    # mcpify:versions:end\n    log_level: str = \"info\"\n"
        );
    }

    #[tokio::test]
    async fn reindents_every_line_of_a_multiline_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.py");
        tokio::fs::write(
            &path,
            "class Config:\n    # mcpify:versions:begin\n    old_line\n    # mcpify:versions:end\n",
        )
        .await
        .unwrap();

        patch_marked_region(&path, "first\nsecond\n").await.unwrap();

        let contents = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(
            contents,
            "class Config:\n    # mcpify:versions:begin\n    first\n    second\n    # mcpify:versions:end\n"
        );
    }

    #[tokio::test]
    async fn errors_clearly_when_markers_are_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hand-edited.ts");
        tokio::fs::write(&path, "no markers here\n").await.unwrap();

        let err = patch_marked_region(&path, "new body\n").await.unwrap_err();
        assert!(err.to_string().contains("missing mcpify's version markers"));
    }
}
