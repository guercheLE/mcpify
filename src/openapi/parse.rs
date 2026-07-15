use anyhow::{Context, Result, bail};
use openapiv3::OpenAPI;
use serde_json::{Map, Value, json};

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

fn validate_document(mut raw: Value) -> Result<OpenApiDocument> {
    if raw.get("swagger").and_then(Value::as_str) == Some("2.0") {
        raw = convert_swagger_2(raw)?;
    }
    repair_missing_parameter_schemas(&mut raw);
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

/// Converts the Swagger 2 surface mcpify consumes into OpenAPI 3.0.3 before
/// the normal validator/normalizer sees it. It covers reusable definitions,
/// security definitions, body/form parameters, response schemas and server
/// URL fields while retaining unknown vendor extensions.
fn convert_swagger_2(mut swagger: Value) -> Result<Value> {
    let root = swagger
        .as_object_mut()
        .context("Swagger 2 document root must be an object")?;
    root.remove("swagger");
    root.insert("openapi".to_string(), Value::String("3.0.3".to_string()));

    let definitions = root.remove("definitions");
    let security_definitions = root.remove("securityDefinitions");
    let parameters = root.remove("parameters");
    let responses = root.remove("responses");
    let mut components = root
        .remove("components")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if let Some(value) = definitions {
        components.insert("schemas".to_string(), value);
    }
    if let Some(Value::Object(schemes)) = security_definitions {
        let converted = schemes
            .into_iter()
            .map(|(name, mut value)| {
                if let Some(scheme) = value.as_object_mut() {
                    match scheme.get("type").and_then(Value::as_str) {
                        Some("basic") => {
                            scheme.insert("type".to_string(), Value::String("http".to_string()));
                            scheme.insert("scheme".to_string(), Value::String("basic".to_string()));
                        }
                        Some("oauth2") => convert_swagger_oauth2(scheme),
                        _ => {}
                    }
                }
                (name, value)
            })
            .collect();
        components.insert("securitySchemes".to_string(), Value::Object(converted));
    }
    if let Some(value) = parameters {
        components.insert("parameters".to_string(), value);
    }
    if let Some(mut value) = responses {
        if let Some(responses) = value.as_object_mut() {
            for response in responses.values_mut().filter_map(Value::as_object_mut) {
                convert_swagger_response(response);
            }
        }
        components.insert("responses".to_string(), value);
    }
    if !components.is_empty() {
        root.insert("components".to_string(), Value::Object(components));
    }

    let base_path = root
        .remove("basePath")
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "/".to_string());
    let host = root
        .remove("host")
        .and_then(|value| value.as_str().map(ToOwned::to_owned));
    let scheme = root
        .remove("schemes")
        .and_then(|value| value.as_array().and_then(|items| items.first()).cloned())
        .and_then(|value| value.as_str().map(ToOwned::to_owned));
    let server_url = match host {
        Some(host) => format!(
            "{}://{}{}",
            scheme.as_deref().unwrap_or("https"),
            host,
            base_path
        ),
        None => base_path,
    };
    root.insert("servers".to_string(), json!([{ "url": server_url }]));
    root.remove("consumes");
    root.remove("produces");

    if let Some(paths) = root.get_mut("paths").and_then(Value::as_object_mut) {
        for path_item in paths.values_mut().filter_map(Value::as_object_mut) {
            for method in ["get", "put", "post", "delete", "options", "head", "patch"] {
                if let Some(operation) = path_item.get_mut(method).and_then(Value::as_object_mut) {
                    convert_swagger_operation(operation);
                }
            }
        }
    }
    rewrite_swagger_refs(&mut swagger);
    Ok(swagger)
}

fn convert_swagger_oauth2(scheme: &mut Map<String, Value>) {
    let flow = scheme
        .remove("flow")
        .and_then(|value| value.as_str().map(ToOwned::to_owned));
    let authorization_url = scheme.remove("authorizationUrl");
    let token_url = scheme.remove("tokenUrl");
    let scopes = scheme.remove("scopes").unwrap_or_else(|| json!({}));
    let (flow_name, mut flow_value) = match flow.as_deref() {
        Some("implicit") => ("implicit", json!({ "scopes": scopes })),
        Some("password") => ("password", json!({ "scopes": scopes })),
        Some("application") => ("clientCredentials", json!({ "scopes": scopes })),
        _ => ("authorizationCode", json!({ "scopes": scopes })),
    };
    if let Some(url) = authorization_url {
        flow_value["authorizationUrl"] = url;
    }
    if let Some(url) = token_url {
        flow_value["tokenUrl"] = url;
    }
    let mut flows = Map::new();
    flows.insert(flow_name.to_string(), flow_value);
    scheme.insert("flows".to_string(), Value::Object(flows));
}

fn convert_swagger_operation(operation: &mut Map<String, Value>) {
    if let Some(parameters) = operation
        .get_mut("parameters")
        .and_then(Value::as_array_mut)
    {
        let mut request_body = None;
        let mut form_properties = Map::new();
        let mut required_form_fields = Vec::new();
        parameters.retain_mut(|parameter| {
            let Some(object) = parameter.as_object_mut() else {
                return true;
            };
            let location = object.get("in").and_then(Value::as_str);
            if location == Some("formData") {
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("value")
                    .to_string();
                let schema_type = object
                    .remove("type")
                    .unwrap_or_else(|| Value::String("string".to_string()));
                form_properties.insert(name.clone(), json!({ "type": schema_type }));
                if object.get("required").and_then(Value::as_bool) == Some(true) {
                    required_form_fields.push(Value::String(name));
                }
                return false;
            }
            if location != Some("body") {
                return true;
            }
            let schema = object
                .remove("schema")
                .unwrap_or_else(|| json!({ "type": "object" }));
            request_body = Some(json!({
                "required": object.get("required").and_then(Value::as_bool).unwrap_or(false),
                "content": { "application/json": { "schema": schema } }
            }));
            false
        });
        if let Some(body) = request_body {
            operation.insert("requestBody".to_string(), body);
        } else if !form_properties.is_empty() {
            let mut schema = json!({
                "type": "object",
                "properties": Value::Object(form_properties)
            });
            if !required_form_fields.is_empty() {
                schema["required"] = Value::Array(required_form_fields);
            }
            operation.insert(
                "requestBody".to_string(),
                json!({ "content": { "application/x-www-form-urlencoded": { "schema": schema } } }),
            );
        }
    }
    if let Some(responses) = operation
        .get_mut("responses")
        .and_then(Value::as_object_mut)
    {
        for response in responses.values_mut().filter_map(Value::as_object_mut) {
            convert_swagger_response(response);
        }
    }
}

fn convert_swagger_response(response: &mut Map<String, Value>) {
    if let Some(schema) = response.remove("schema") {
        response.insert(
            "content".to_string(),
            json!({ "application/json": { "schema": schema } }),
        );
    }
    if let Some(headers) = response.get_mut("headers").and_then(Value::as_object_mut) {
        for header in headers.values_mut().filter_map(Value::as_object_mut) {
            repair_parameter_schema(header);
        }
    }
}

fn rewrite_swagger_refs(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref") {
                if let Some(rest) = reference.strip_prefix("#/definitions/") {
                    *reference = format!("#/components/schemas/{rest}");
                } else if let Some(rest) = reference.strip_prefix("#/parameters/") {
                    *reference = format!("#/components/parameters/{rest}");
                } else if let Some(rest) = reference.strip_prefix("#/responses/") {
                    *reference = format!("#/components/responses/{rest}");
                }
            }
            for child in object.values_mut() {
                rewrite_swagger_refs(child);
            }
        }
        Value::Array(array) => {
            for child in array {
                rewrite_swagger_refs(child);
            }
        }
        _ => {}
    }
}

fn repair_missing_parameter_schemas(raw: &mut Value) {
    let Some(paths) = raw.get_mut("paths").and_then(Value::as_object_mut) else {
        return;
    };
    for path_item in paths.values_mut().filter_map(Value::as_object_mut) {
        repair_parameter_array(path_item.get_mut("parameters"));
        for method in [
            "get", "put", "post", "delete", "options", "head", "patch", "trace",
        ] {
            if let Some(operation) = path_item.get_mut(method).and_then(Value::as_object_mut) {
                repair_parameter_array(operation.get_mut("parameters"));
            }
        }
    }
    if let Some(parameters) = raw
        .pointer_mut("/components/parameters")
        .and_then(Value::as_object_mut)
    {
        for parameter in parameters.values_mut().filter_map(Value::as_object_mut) {
            repair_parameter_schema(parameter);
        }
    }
}

fn repair_parameter_array(parameters: Option<&mut Value>) {
    let Some(parameters) = parameters.and_then(Value::as_array_mut) else {
        return;
    };
    for parameter in parameters.iter_mut().filter_map(Value::as_object_mut) {
        repair_parameter_schema(parameter);
    }
}

fn repair_parameter_schema(parameter: &mut Map<String, Value>) {
    if parameter.contains_key("$ref")
        || parameter.contains_key("schema")
        || parameter.contains_key("content")
        || parameter.get("in").and_then(Value::as_str) == Some("body")
    {
        return;
    }

    let mut schema = Map::new();
    for key in [
        "type",
        "format",
        "items",
        "default",
        "maximum",
        "exclusiveMaximum",
        "minimum",
        "exclusiveMinimum",
        "maxLength",
        "minLength",
        "pattern",
        "maxItems",
        "minItems",
        "uniqueItems",
        "enum",
        "multipleOf",
    ] {
        if let Some(value) = parameter.remove(key) {
            schema.insert(key.to_string(), value);
        }
    }
    schema
        .entry("type".to_string())
        .or_insert_with(|| Value::String("string".to_string()));
    if schema.get("type").and_then(Value::as_str) == Some("array") {
        schema
            .entry("items".to_string())
            .or_insert_with(|| json!({ "type": "string" }));
    }
    parameter.insert("schema".to_string(), Value::Object(schema));

    let collection_format = parameter
        .remove("collectionFormat")
        .and_then(|value| value.as_str().map(ToOwned::to_owned));
    let location = parameter.get("in").and_then(Value::as_str);
    match (location, collection_format.as_deref()) {
        (Some("query"), Some("ssv")) => {
            parameter.insert(
                "style".to_string(),
                Value::String("spaceDelimited".to_string()),
            );
            parameter.insert("explode".to_string(), Value::Bool(false));
        }
        (Some("query"), Some("pipes")) => {
            parameter.insert(
                "style".to_string(),
                Value::String("pipeDelimited".to_string()),
            );
            parameter.insert("explode".to_string(), Value::Bool(false));
        }
        (Some("query"), Some("multi")) => {
            parameter.insert("style".to_string(), Value::String("form".to_string()));
            parameter.insert("explode".to_string(), Value::Bool(true));
        }
        (Some("query"), Some("csv")) => {
            parameter.insert("style".to_string(), Value::String("form".to_string()));
            parameter.insert("explode".to_string(), Value::Bool(false));
        }
        (Some("path" | "header"), Some(_)) => {
            parameter.insert("style".to_string(), Value::String("simple".to_string()));
            parameter.insert("explode".to_string(), Value::Bool(false));
        }
        _ => {}
    }
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
        Err(_) => bail!("failed to parse OpenAPI spec as JSON or YAML: {first_err:#}"),
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
    fn converts_swagger_array_parameter_schema_and_serialization() {
        let doc = parse_document(
            r#"
swagger: "2.0"
info: { title: Legacy arrays, version: "1.0.0" }
paths:
  /widgets:
    get:
      parameters:
        - name: ids
          in: query
          type: array
          items: { type: string, format: uuid }
          collectionFormat: multi
      responses:
        "200":
          description: ok
          headers:
            X-RateLimit-Remaining: { type: integer, format: int32 }
"#,
            Some(Format::Yaml),
        )
        .unwrap();

        let parameter = &doc.raw()["paths"]["/widgets"]["get"]["parameters"][0];
        assert_eq!(parameter["schema"]["type"], "array");
        assert_eq!(parameter["schema"]["items"]["format"], "uuid");
        assert_eq!(parameter["style"], "form");
        assert_eq!(parameter["explode"], true);
        assert!(parameter.get("collectionFormat").is_none());
        assert_eq!(
            doc.raw()["paths"]["/widgets"]["get"]["responses"]["200"]["headers"]["X-RateLimit-Remaining"]
                ["schema"]["format"],
            "int32"
        );
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
