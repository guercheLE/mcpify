use anyhow::{Context, Result, bail};
use openapiv3::OpenAPI;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct OpenApiDocument {
    raw: Value,
}

impl OpenApiDocument {
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    pub fn openapi_version(&self) -> &str {
        self.raw["openapi"].as_str().unwrap_or_default()
    }

    pub fn title(&self) -> &str {
        self.raw["info"]["title"].as_str().unwrap_or_default()
    }

    pub fn is_31(&self) -> bool {
        self.openapi_version().starts_with("3.1.")
    }
}

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

fn parse_value_as(raw: &str, format: Format) -> Result<Value> {
    match format {
        Format::Json => serde_json::from_str(raw).map_err(anyhow::Error::from),
        Format::Yaml => serde_yaml::from_str(raw).map_err(anyhow::Error::from),
    }
}

fn validate_response_map(responses: &serde_json::Map<String, Value>, location: &str) -> Result<()> {
    for (status, response) in responses {
        let valid_status = status == "default"
            || (status.len() == 3
                && status.bytes().all(|byte| byte.is_ascii_digit())
                && status
                    .parse::<u16>()
                    .is_ok_and(|code| (100..=599).contains(&code)))
            || (status.len() == 3 && matches!(status.as_bytes(), [b'1'..=b'5', b'X', b'X']));
        if !valid_status {
            bail!(
                "invalid OpenAPI 3.1 document: response key {status:?} at {location} must be 'default', an HTTP status code, or an uppercase status range"
            );
        }

        let response = response.as_object().with_context(|| {
            format!(
                "invalid OpenAPI 3.1 document: response {status:?} at {location} must be an object"
            )
        })?;
        if response.contains_key("$ref") {
            continue;
        }
        if response
            .get("description")
            .and_then(Value::as_str)
            .is_none()
        {
            bail!(
                "invalid OpenAPI 3.1 document: inline response {status:?} at {location} must declare a string description"
            );
        }
    }
    Ok(())
}

fn validate_openapi_31_shape(raw: &Value, version: &str) -> Result<()> {
    let parsed_version = semver::Version::parse(version)
        .with_context(|| format!("invalid OpenAPI 3.1 document: invalid version '{version}'"))?;
    if parsed_version.major != 3 || parsed_version.minor != 1 {
        bail!("invalid OpenAPI 3.1 document: invalid version '{version}'");
    }

    // `oas3` models `responses` as optional even though the OpenAPI 3.1
    // specification requires it on every Operation Object. Enforce that
    // required-field invariant explicitly rather than silently generating an
    // operation with an accept-anything output schema.
    let paths = raw["paths"]
        .as_object()
        .expect("paths was validated as an object");
    for (path, path_item) in paths {
        let Some(path_item) = path_item.as_object() else {
            continue;
        };
        for method in [
            "get", "put", "post", "delete", "options", "head", "patch", "trace",
        ] {
            let Some(operation) = path_item.get(method) else {
                continue;
            };
            let responses = operation
                .as_object()
                .and_then(|operation| operation.get("responses"))
                .and_then(Value::as_object);
            let Some(responses) = responses.filter(|responses| !responses.is_empty()) else {
                bail!(
                    "invalid OpenAPI 3.1 document: operation {method:?} at path {path:?} must declare responses"
                );
            };
            validate_response_map(responses, &format!("{method:?} operation at path {path:?}"))?;
        }
    }

    if let Some(component_responses) = raw
        .pointer("/components/responses")
        .and_then(Value::as_object)
    {
        // Component response names are not status codes, but every inline
        // reusable Response Object still has the required description field.
        for (name, response) in component_responses {
            let response = response.as_object().with_context(|| {
                format!(
                    "invalid OpenAPI 3.1 document: component response {name:?} must be an object"
                )
            })?;
            if !response.contains_key("$ref")
                && response
                    .get("description")
                    .and_then(Value::as_str)
                    .is_none()
            {
                bail!(
                    "invalid OpenAPI 3.1 document: inline component response {name:?} must declare a string description"
                );
            }
        }
    }
    Ok(())
}

fn validate_document(raw: Value) -> Result<OpenApiDocument> {
    let version = raw
        .get("openapi")
        .and_then(Value::as_str)
        .context("OpenAPI document is missing string field 'openapi'")?;
    if !version.starts_with("3.0.") && !version.starts_with("3.1.") {
        bail!("unsupported OpenAPI version '{version}'; expected 3.0.x or 3.1.x");
    }
    raw.get("info")
        .and_then(Value::as_object)
        .context("OpenAPI document is missing object field 'info'")?;
    raw.pointer("/info/title")
        .and_then(Value::as_str)
        .context("OpenAPI document is missing string field 'info.title'")?;
    raw.pointer("/info/version")
        .and_then(Value::as_str)
        .context("OpenAPI document is missing string field 'info.version'")?;
    raw.get("paths")
        .and_then(Value::as_object)
        .context("OpenAPI document is missing object field 'paths'")?;

    if version.starts_with("3.0.") {
        serde_json::from_value::<OpenAPI>(raw.clone()).context("invalid OpenAPI 3.0 document")?;
    } else {
        serde_json::from_value::<oas3::Spec>(raw.clone())
            .context("invalid OpenAPI 3.1 document")?;
        validate_openapi_31_shape(&raw, version)?;
    }

    Ok(OpenApiDocument { raw })
}

/// Parses `raw` as JSON or YAML into a normalized `OpenAPI` document,
/// trying `hint`'s format first and falling back to the other rather than
/// rejecting a spec whose extension lies about its content.
pub fn parse_document(raw: &str, hint: Option<Format>) -> Result<OpenApiDocument> {
    let (first, second) = match hint {
        Some(Format::Yaml) => (Format::Yaml, Format::Json),
        _ => (Format::Json, Format::Yaml),
    };

    let first_err = match parse_value_as(raw, first).and_then(validate_document) {
        Ok(doc) => return Ok(doc),
        Err(err) => err,
    };

    match parse_value_as(raw, second).and_then(validate_document) {
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
        assert_eq!(doc.title(), "Minimal API");
    }

    #[test]
    fn parses_json_without_hint() {
        let doc = parse_document(MINIMAL_JSON, None).unwrap();
        assert_eq!(doc.title(), "Minimal API");
    }

    #[test]
    fn parses_yaml_with_json_hint_via_fallback() {
        // A wrong hint must not prevent parsing — both formats are tried.
        let doc = parse_document(MINIMAL_YAML, Some(Format::Json)).unwrap();
        assert_eq!(doc.title(), "Minimal API");
    }

    #[test]
    fn rejects_malformed_spec() {
        let err = parse_document("not: [valid, openapi", None).unwrap_err();
        assert!(err.to_string().contains("failed to parse OpenAPI spec"));
    }

    #[test]
    fn parses_openapi_31_json_schema_2020_12_constructs() {
        let doc = parse_document(
            r#"
openapi: 3.1.0
info:
  title: Modern API
  version: "1.0.0"
paths:
  /widgets:
    post:
      operationId: createWidget
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                name:
                  type: [string, "null"]
                score:
                  type: number
                  exclusiveMinimum: 0
      responses:
        "204":
          description: Created
"#,
            Some(Format::Yaml),
        )
        .expect("OpenAPI 3.1 must parse");

        assert_eq!(doc.openapi_version(), "3.1.0");
        assert_eq!(doc.title(), "Modern API");
    }

    #[test]
    fn rejects_invalid_openapi_31_version() {
        let err = parse_document(
            r#"
openapi: 3.1.invalid
info:
  title: Invalid API
  version: "1.0.0"
paths: {}
"#,
            Some(Format::Yaml),
        )
        .unwrap_err();

        assert!(err.to_string().contains("invalid OpenAPI 3.1 document"));
    }

    #[test]
    fn rejects_openapi_31_operation_without_responses() {
        let err = parse_document(
            r#"
openapi: 3.1.0
info:
  title: Invalid API
  version: "1.0.0"
paths:
  /widgets:
    get:
      operationId: listWidgets
"#,
            Some(Format::Yaml),
        )
        .unwrap_err();

        assert!(err.to_string().contains("invalid OpenAPI 3.1 document"));
    }

    #[test]
    fn rejects_openapi_31_invalid_response_key() {
        let err = parse_document(
            r#"
openapi: 3.1.0
info:
  title: Invalid API
  version: "1.0.0"
paths:
  /widgets:
    get:
      responses:
        banana:
          description: Not a status
"#,
            Some(Format::Yaml),
        )
        .unwrap_err();

        assert!(err.to_string().contains("response key"));
    }

    #[test]
    fn rejects_openapi_31_response_without_description() {
        let err = parse_document(
            r#"
openapi: 3.1.0
info:
  title: Invalid API
  version: "1.0.0"
paths:
  /widgets:
    get:
      responses:
        "200": {}
"#,
            Some(Format::Yaml),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("must declare a string description")
        );
    }
}
