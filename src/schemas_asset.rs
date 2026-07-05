use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::openapi::NormalizedOperation;

/// zstd compression level for the schemas asset. Chosen for compression
/// ratio over speed: this runs once per `generate`/`add-version` call, not
/// on any hot path, so the multi-second cost at this level is a non-issue.
const ZSTD_LEVEL: i32 = 19;

/// zstd window log (2^27 = 128 MiB) enabling long-distance matching. Every
/// operation's `inputSchema`/`outputSchema` embeds a full copy of the same
/// `$defs` library (`openapi::schema_resolve`'s deliberate
/// simpler-than-cross-schema-$ref-resolution tradeoff) — standard zstd's
/// default window is far smaller than the distance between those repeated
/// copies, so without long-distance matching this duplication survives
/// compression almost untouched. With it, a hundreds-of-operations spec's
/// ~190 MB of duplicated JSON compresses down to tens of KB.
const ZSTD_WINDOW_LOG: u32 = 27;

fn compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder =
        zstd::Encoder::new(Vec::new(), ZSTD_LEVEL).context("failed to construct zstd encoder")?;
    encoder
        .long_distance_matching(true)
        .context("failed to enable zstd long-distance matching")?;
    encoder
        .window_log(ZSTD_WINDOW_LOG)
        .context("failed to set zstd window log")?;
    // Without this, the frame header omits the decompressed content size
    // (it's only included when a size is pledged up front) — Rust's own
    // `zstd::decode_all` doesn't care, but other targets' zstd bindings
    // (e.g. Python's `zstandard.decompress`) need it for one-shot
    // decompression rather than a streaming reader.
    encoder
        .set_pledged_src_size(Some(data.len() as u64))
        .context("failed to set zstd pledged source size")?;
    encoder
        .write_all(data)
        .context("failed to zstd-compress schemas JSON")?;
    encoder.finish().context("failed to finalize zstd stream")
}

/// Builds the "generated schemas" asset every target's validator reads
/// (Ajv/jsonschema/etc.) at runtime, keyed by `operation_id`, and writes it
/// to `out_path` as zstd-compressed bytes (see `compress`) rather than
/// plain JSON text — every operation embeds a full copy of the spec's
/// `$defs` library, and a long-distance-matching compressor collapses that
/// duplication (measured ~190 MB -> tens of KB on a real spec) without
/// requiring any change to the JSON Schema shape itself. Identical shape
/// across all 5 targets — only the *destination path* and how the runtime
/// decompresses/loads it differ — so this is shared rather than duplicated
/// in each `targets::<lang>::steps::tools` module and re-used as-is by v8's
/// `add-version` command (which writes this same shape at a version-suffixed
/// path, without needing any target-specific knowledge).
///
/// Built directly with `serde_json` rather than through a Tera loop: the
/// per-operation JSON Schema documents (from `openapi::schema_resolve`,
/// already `$ref`-resolved) are genuine data, not boilerplate text, and a
/// hundreds-of-operations spec would make a loop-heavy `.tera` template
/// slow to render and unreadable to maintain.
pub async fn write_schemas_json_at(
    operations: &[NormalizedOperation],
    out_path: &Path,
) -> Result<()> {
    let mut schemas = Map::new();
    for operation in operations {
        schemas.insert(
            operation.operation_id.clone(),
            serde_json::json!({
                "inputSchema": operation.validation_input_schema,
                "outputSchema": operation.validation_output_schema,
            }),
        );
    }

    let json_bytes = serde_json::to_vec(&Value::Object(schemas))
        .context("failed to serialize generated schemas JSON")?;
    let compressed = compress(&json_bytes)?;

    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create directory '{}'", parent.display()))?;
    }
    tokio::fs::write(out_path, compressed)
        .await
        .with_context(|| format!("failed to write '{}'", out_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_operation() -> NormalizedOperation {
        NormalizedOperation {
            operation_id: "listWidgets".to_string(),
            path: "/widgets".to_string(),
            method: "GET".to_string(),
            summary: Some("List widgets".to_string()),
            description: None,
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
            auth_scheme_ref: None,
            validation_input_schema: serde_json::json!({"type": "object", "properties": {}}),
            validation_output_schema: serde_json::json!({"type": "array"}),
        }
    }

    fn decompress(bytes: &[u8]) -> Value {
        let decoded = zstd::decode_all(bytes).expect("valid zstd stream");
        serde_json::from_slice(&decoded).expect("valid JSON")
    }

    #[tokio::test]
    async fn round_trips_operation_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("generated-schemas.json.zst");

        write_schemas_json_at(&[sample_operation()], &out_path)
            .await
            .unwrap();

        let contents = tokio::fs::read(&out_path).await.unwrap();
        let parsed = decompress(&contents);
        assert_eq!(parsed["listWidgets"]["inputSchema"]["type"], "object");
        assert_eq!(parsed["listWidgets"]["outputSchema"]["type"], "array");
    }

    #[tokio::test]
    async fn is_an_empty_object_with_no_operations() {
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("generated-schemas.json.zst");

        write_schemas_json_at(&[], &out_path).await.unwrap();

        let contents = tokio::fs::read(&out_path).await.unwrap();
        let parsed = decompress(&contents);
        assert_eq!(parsed, Value::Object(Map::new()));
    }

    #[tokio::test]
    async fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir
            .path()
            .join("nested")
            .join("dir")
            .join("generated-schemas.json.zst");

        write_schemas_json_at(&[sample_operation()], &out_path)
            .await
            .unwrap();

        assert!(out_path.is_file());
    }

    #[test]
    fn compressed_frame_embeds_the_content_size() {
        // Other targets' zstd bindings (e.g. Python's `zstandard.decompress`)
        // need the decompressed size in the frame header for one-shot
        // decompression, unlike Rust's own streaming `zstd::decode_all`.
        let data = b"some schemas JSON";
        let compressed = compress(data).unwrap();
        assert_eq!(
            zstd::zstd_safe::get_frame_content_size(&compressed).unwrap(),
            Some(data.len() as u64)
        );
    }

    #[test]
    fn long_distance_matching_collapses_duplicated_defs() {
        // Mirrors the real bug: every operation's schema embeds a full
        // ~140 KB copy of the same `$defs` blob. Without long-distance
        // matching, standard zstd's window can't see back far enough to
        // recognize the repeat.
        let defs = "x".repeat(140_000);
        let mut json = String::from("[");
        for i in 0..800 {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(r#"{{"op": {i}, "defs": "{defs}"}}"#));
        }
        json.push(']');

        let compressed = compress(json.as_bytes()).unwrap();
        assert!(
            compressed.len() < json.len() / 100,
            "expected long-distance matching to collapse duplicated defs, got {} bytes from {} input bytes",
            compressed.len(),
            json.len()
        );

        let decompressed = zstd::decode_all(compressed.as_slice()).unwrap();
        assert_eq!(decompressed, json.as_bytes());
    }
}
