use anyhow::{Result, bail};
use openapiv3::OpenAPI;

/// Which serialization the raw spec text is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Yaml,
}

/// Fast-path format hint from a file extension or URL path, used to try the
/// more likely parser first (both are always attempted regardless).
pub fn detect_format_from_name(name: &str) -> Option<Format> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".json") {
        Some(Format::Json)
    } else if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        Some(Format::Yaml)
    } else {
        None
    }
}

fn parse_as(raw: &str, format: Format) -> Result<OpenAPI> {
    match format {
        Format::Json => serde_json::from_str(raw).map_err(anyhow::Error::from),
        Format::Yaml => serde_yaml::from_str(raw).map_err(anyhow::Error::from),
    }
}

/// Parses `raw` as JSON or YAML into a normalized `OpenAPI` document,
/// trying `hint`'s format first and falling back to the other rather than
/// rejecting a spec whose extension lies about its content.
pub fn parse_document(raw: &str, hint: Option<Format>) -> Result<OpenAPI> {
    let (first, second) = match hint {
        Some(Format::Yaml) => (Format::Yaml, Format::Json),
        _ => (Format::Json, Format::Yaml),
    };

    let first_err = match parse_as(raw, first) {
        Ok(doc) => return Ok(doc),
        Err(err) => err,
    };

    match parse_as(raw, second) {
        Ok(doc) => Ok(doc),
        Err(_) => bail!("failed to parse OpenAPI spec as JSON or YAML: {first_err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_YAML: &str = r#"
openapi: 3.0.0
info:
  title: Minimal API
  version: "1.0.0"
paths: {}
"#;

    const MINIMAL_JSON: &str = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Minimal API", "version": "1.0.0" },
        "paths": {}
    }"#;

    #[test]
    fn detects_format_from_extension() {
        assert_eq!(detect_format_from_name("spec.json"), Some(Format::Json));
        assert_eq!(detect_format_from_name("spec.yaml"), Some(Format::Yaml));
        assert_eq!(detect_format_from_name("spec.yml"), Some(Format::Yaml));
        assert_eq!(detect_format_from_name("spec"), None);
    }

    #[test]
    fn parses_yaml_without_hint() {
        let doc = parse_document(MINIMAL_YAML, None).unwrap();
        assert_eq!(doc.info.title, "Minimal API");
    }

    #[test]
    fn parses_json_without_hint() {
        let doc = parse_document(MINIMAL_JSON, None).unwrap();
        assert_eq!(doc.info.title, "Minimal API");
    }

    #[test]
    fn parses_yaml_with_json_hint_via_fallback() {
        // A wrong hint must not prevent parsing — both formats are tried.
        let doc = parse_document(MINIMAL_YAML, Some(Format::Json)).unwrap();
        assert_eq!(doc.info.title, "Minimal API");
    }

    #[test]
    fn rejects_malformed_spec() {
        let err = parse_document("not: [valid, openapi", None).unwrap_err();
        assert!(err.to_string().contains("failed to parse OpenAPI spec"));
    }
}
