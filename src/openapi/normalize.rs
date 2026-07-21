use std::collections::HashSet;

use serde_json::Value;

use super::parse::OpenApiDocument;
use super::schema_resolve::{
    embed_literal_schema_defs, resolve_input_schema_value, resolve_output_schema_value,
};

/// One HTTP operation flattened out of `doc.paths`, ready for both
/// `mcp_store.db` assembly (Story 5) and, later, target template contexts
/// (Story 12). `input_schema`/`output_schema` here are a JSON snapshot of
/// the OpenAPI-level parameter/requestBody/responses objects — the shape
/// `get` and api-client's parameter-location lookups need (a `parameters`
/// array with `in`/`name`, a per-status `responses` map) — rather than the
/// normalized-to-a-single-object-schema shape of `validation_*_schema`
/// below. Any `$ref` to a `components.schemas` entry found within it is
/// rewritten to `#/$defs/...` with the referenced component embedded
/// alongside in a top-level `$defs` map (`embed_literal_schema_defs`), so
/// the snapshot stays self-contained for a reader who only ever sees this
/// one operation — never the full spec.
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

const METHODS: &[(&str, &str)] = &[
    ("GET", "get"),
    ("PUT", "put"),
    ("POST", "post"),
    ("DELETE", "delete"),
    ("OPTIONS", "options"),
    ("HEAD", "head"),
    ("PATCH", "patch"),
    ("TRACE", "trace"),
];

/// Flattens `doc.paths` into one `NormalizedOperation` per HTTP method
/// actually defined, synthesizing an `operation_id` (`"METHOD /path"`) for
/// operations that omit one. Path items behind a `$ref` are skipped — mcpify
/// does not resolve external path references.
///
/// Some real-world specs (e.g. Jira Data Center's) reuse the same
/// `operationId` across unrelated endpoints (multiple `getProperty`
/// operations across issue/project/application-properties resources). Since
/// `operation_id` is the `endpoints` table's primary key, and downstream
/// targets expose it as the MCP tool name, collisions are disambiguated by
/// appending the method and path — which is always unique per OpenAPI's own
/// rules — rather than failing the whole generation run.
pub fn normalize_operations(doc: &OpenApiDocument) -> Vec<NormalizedOperation> {
    let mut operations = Vec::new();
    let mut seen_operation_ids: HashSet<String> = HashSet::new();
    let Some(paths) = doc.raw().get("paths").and_then(Value::as_object) else {
        return operations;
    };
    let components = doc
        .raw()
        .pointer("/components/schemas")
        .and_then(Value::as_object);
    let document_security = doc.raw().get("security");

    for (path, item) in paths {
        let Some(item) = item.as_object() else {
            continue;
        };
        if item.contains_key("$ref") {
            continue;
        }

        for (method, method_key) in METHODS {
            let Some(operation) = item.get(*method_key).and_then(Value::as_object) else {
                continue;
            };

            let operation_id = operation
                .get("operationId")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{method} {path}"));
            let operation_id = if seen_operation_ids.insert(operation_id.clone()) {
                operation_id
            } else {
                format!("{operation_id} ({method} {path})")
            };
            seen_operation_ids.insert(operation_id.clone());

            let auth_scheme_ref = operation
                .get("security")
                .or(document_security)
                .and_then(Value::as_array)
                .and_then(|requirements| requirements.first())
                .and_then(Value::as_object)
                .and_then(|requirement| requirement.keys().next())
                .cloned();

            let mut input_schema = serde_json::json!({
                "parameters": operation.get("parameters").cloned().unwrap_or_else(|| serde_json::json!([])),
                "requestBody": operation.get("requestBody").cloned().unwrap_or(Value::Null),
            });
            embed_literal_schema_defs(&mut input_schema, components);

            let mut output_schema = operation
                .get("responses")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            embed_literal_schema_defs(&mut output_schema, components);

            operations.push(NormalizedOperation {
                operation_id,
                path: path.clone(),
                method: method.to_string(),
                summary: operation
                    .get("summary")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                description: operation
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                input_schema,
                output_schema,
                auth_scheme_ref,
                validation_input_schema: resolve_input_schema_value(
                    operation,
                    components,
                    doc.is_31(),
                ),
                validation_output_schema: resolve_output_schema_value(
                    operation,
                    components,
                    doc.is_31(),
                ),
            });
        }
    }

    operations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> OpenApiDocument {
        crate::openapi::parse::parse_document(yaml, Some(crate::openapi::parse::Format::Yaml))
            .expect("fixture must parse as OpenAPI")
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
    fn disambiguates_duplicate_operation_ids() {
        let doc = parse(
            r#"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths:
  /issue/{issueIdOrKey}/properties/{propertyKey}:
    get:
      operationId: getProperty
      responses:
        "200":
          description: OK
  /project/{projectIdOrKey}/properties/{propertyKey}:
    get:
      operationId: getProperty
      responses:
        "200":
          description: OK
"#,
        );

        let operations = normalize_operations(&doc);
        assert_eq!(operations.len(), 2);

        let ids: HashSet<_> = operations
            .iter()
            .map(|op| op.operation_id.clone())
            .collect();
        assert_eq!(ids.len(), 2, "operation ids must be unique: {ids:?}");
        assert!(ids.contains("getProperty"));
        assert!(ids.iter().any(|id| id.starts_with("getProperty (GET /")));
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

    #[test]
    fn preserves_openapi_31_schema_keywords_for_generated_validators() {
        let doc = parse(
            r#"
openapi: 3.1.0
info:
  title: Test
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
        );

        let operations = normalize_operations(&doc);
        let body = &operations[0].validation_input_schema["properties"]["body"];
        assert_eq!(
            body["properties"]["name"]["type"],
            serde_json::json!(["string", "null"])
        );
        assert_eq!(body["properties"]["score"]["exclusiveMinimum"], 0);
        assert_eq!(
            operations[0].validation_input_schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
    }

    #[test]
    fn literal_output_schema_embeds_defs_for_its_refs() {
        let doc = parse(
            r##"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths:
  /widgets:
    get:
      operationId: listWidgets
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: "#/components/schemas/Widget"
components:
  schemas:
    Widget:
      type: object
      properties:
        name:
          type: string
        parent:
          $ref: "#/components/schemas/Widget"
    Unused:
      type: object
      properties:
        ignored:
          type: string
"##,
        );

        let operations = normalize_operations(&doc);
        let output = &operations[0].output_schema;

        // The literal snapshot `get` hands back keeps its per-status-code
        // shape (unlike `validation_output_schema`), but the dangling
        // `$ref` inside it now resolves within the same document.
        let item_ref = &output["200"]["content"]["application/json"]["schema"]["items"]["$ref"];
        assert_eq!(item_ref, "#/$defs/Widget");
        assert_eq!(
            output["$defs"]["Widget"]["properties"]["name"]["type"],
            "string"
        );
        // Self-reference must point at the rewritten location too, not the
        // original OpenAPI-only pointer that has nothing to resolve
        // against inside this snapshot.
        assert_eq!(
            output["$defs"]["Widget"]["properties"]["parent"]["$ref"],
            "#/$defs/Widget"
        );
        assert!(output["$defs"].get("Unused").is_none());
    }

    #[test]
    fn literal_input_schema_embeds_defs_while_keeping_parameter_locations() {
        let doc = parse(
            r##"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths:
  /widgets:
    post:
      operationId: createWidget
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: "#/components/schemas/Widget"
      responses:
        "201":
          description: Created
components:
  schemas:
    Widget:
      type: object
      properties:
        name:
          type: string
"##,
        );

        let operations = normalize_operations(&doc);
        let input = &operations[0].input_schema;

        // api-client's parameter-location lookup (`in`/`name`) still works
        // against the literal snapshot's `parameters` array.
        assert_eq!(input["parameters"][0]["name"], "id");
        assert_eq!(input["parameters"][0]["in"], "path");

        let body_ref = &input["requestBody"]["content"]["application/json"]["schema"]["$ref"];
        assert_eq!(body_ref, "#/$defs/Widget");
        assert_eq!(
            input["$defs"]["Widget"]["properties"]["name"]["type"],
            "string"
        );
    }
}
