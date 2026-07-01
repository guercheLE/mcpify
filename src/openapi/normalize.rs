use openapiv3::{OpenAPI, Operation, PathItem, ReferenceOr};

use super::schema_resolve::{resolve_input_schema, resolve_output_schema};

/// One HTTP operation flattened out of `doc.paths`, ready for both
/// `mcp_store.db` assembly (Story 5) and, later, target template contexts
/// (Story 12). `input_schema`/`output_schema` here are a straightforward
/// JSON snapshot of the OpenAPI-level parameter/requestBody/responses
/// objects — not yet a fully `$ref`-resolved JSON Schema document. That
/// resolution is Story 12's concern when building the generated project's
/// Ajv validator; this struct only needs to survive a JSON round-trip.
#[derive(Debug, Clone)]
pub struct NormalizedOperation {
    pub operation_id: String,
    pub path: String,
    pub method: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    /// Name of the first security scheme this operation requires (falling
    /// back to the document-level default), or `None` for a public
    /// endpoint. Since v1 selects a single active auth strategy at runtime
    /// (REQ-1.2.3) this is informational rather than an enforced binding.
    pub auth_scheme_ref: Option<String>,
    /// A genuine, `$ref`-resolved JSON Schema (properties keyed by
    /// parameter name, plus a `body` property) that Ajv can compile
    /// directly to validate `call` arguments — distinct from
    /// `input_schema` above, which stays a literal OpenAPI-shape snapshot
    /// for the `get` tool and api-client's parameter-location lookups.
    pub validation_input_schema: serde_json::Value,
    /// A genuine, `$ref`-resolved JSON Schema for the first documented 2xx
    /// response, that Ajv can compile directly to validate a live response.
    pub validation_output_schema: serde_json::Value,
}

type MethodAccessor = fn(&PathItem) -> &Option<Operation>;

const METHODS: &[(&str, MethodAccessor)] = &[
    ("GET", |item| &item.get),
    ("PUT", |item| &item.put),
    ("POST", |item| &item.post),
    ("DELETE", |item| &item.delete),
    ("OPTIONS", |item| &item.options),
    ("HEAD", |item| &item.head),
    ("PATCH", |item| &item.patch),
    ("TRACE", |item| &item.trace),
];

/// Flattens `doc.paths` into one `NormalizedOperation` per HTTP method
/// actually defined, synthesizing an `operation_id` (`"METHOD /path"`) for
/// operations that omit one. Path items behind a `$ref` are skipped — mcpify
/// does not resolve external path references.
pub fn normalize_operations(doc: &OpenAPI) -> Vec<NormalizedOperation> {
    let mut operations = Vec::new();

    for (path, item) in doc.paths.paths.iter() {
        let ReferenceOr::Item(item) = item else {
            continue;
        };

        for (method, accessor) in METHODS {
            let Some(operation) = accessor(item) else {
                continue;
            };

            let operation_id = operation
                .operation_id
                .clone()
                .unwrap_or_else(|| format!("{method} {path}"));

            let auth_scheme_ref = operation
                .security
                .as_ref()
                .or(doc.security.as_ref())
                .and_then(|requirements| requirements.first())
                .and_then(|requirement| requirement.keys().next())
                .cloned();

            operations.push(NormalizedOperation {
                operation_id,
                path: path.clone(),
                method: method.to_string(),
                summary: operation.summary.clone(),
                description: operation.description.clone(),
                input_schema: serde_json::json!({
                    "parameters": operation.parameters,
                    "requestBody": operation.request_body,
                }),
                output_schema: serde_json::json!(operation.responses),
                auth_scheme_ref,
                validation_input_schema: resolve_input_schema(operation, doc.components.as_ref()),
                validation_output_schema: resolve_output_schema(operation, doc.components.as_ref()),
            });
        }
    }

    operations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> OpenAPI {
        serde_yaml::from_str(yaml).expect("fixture must parse as OpenAPI")
    }

    #[test]
    fn extracts_one_operation_per_method() {
        let doc = parse(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths:
  /widgets:
    get:
      operationId: listWidgets
      summary: List widgets
      responses:
        "200":
          description: OK
    post:
      operationId: createWidget
      responses:
        "201":
          description: Created
"#,
        );

        let operations = normalize_operations(&doc);
        assert_eq!(operations.len(), 2);
        assert!(
            operations
                .iter()
                .any(|op| op.operation_id == "listWidgets" && op.method == "GET")
        );
        assert!(
            operations
                .iter()
                .any(|op| op.operation_id == "createWidget" && op.method == "POST")
        );
    }

    #[test]
    fn synthesizes_operation_id_when_missing() {
        let doc = parse(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths:
  /widgets/{id}:
    delete:
      responses:
        "204":
          description: No Content
"#,
        );

        let operations = normalize_operations(&doc);
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].operation_id, "DELETE /widgets/{id}");
    }

    #[test]
    fn resolves_operation_level_auth_scheme_ref() {
        let doc = parse(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths:
  /widgets:
    get:
      operationId: listWidgets
      security:
        - basicAuth: []
      responses:
        "200":
          description: OK
components:
  securitySchemes:
    basicAuth:
      type: http
      scheme: basic
"#,
        );

        let operations = normalize_operations(&doc);
        assert_eq!(operations[0].auth_scheme_ref.as_deref(), Some("basicAuth"));
    }

    #[test]
    fn falls_back_to_document_level_security() {
        let doc = parse(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
security:
  - basicAuth: []
paths:
  /widgets:
    get:
      operationId: listWidgets
      responses:
        "200":
          description: OK
components:
  securitySchemes:
    basicAuth:
      type: http
      scheme: basic
"#,
        );

        let operations = normalize_operations(&doc);
        assert_eq!(operations[0].auth_scheme_ref.as_deref(), Some("basicAuth"));
    }

    #[test]
    fn public_endpoint_has_no_auth_scheme_ref() {
        let doc = parse(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths:
  /health:
    get:
      operationId: health
      responses:
        "200":
          description: OK
"#,
        );

        let operations = normalize_operations(&doc);
        assert_eq!(operations[0].auth_scheme_ref, None);
    }
}
