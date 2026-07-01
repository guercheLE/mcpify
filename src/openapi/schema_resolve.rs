use indexmap::IndexMap;
use openapiv3::{Components, MediaType, Operation, Parameter, ReferenceOr, Schema, StatusCode};
use serde_json::{Map, Value};

/// Rewrites every `"$ref": "#/components/schemas/X"` found anywhere in
/// `value` to `"$ref": "#/$defs/X"`, so a standalone JSON Schema document
/// (with `$defs` populated from `components.schemas`, see `build_defs`) can
/// resolve it locally without needing multi-schema registration in Ajv.
fn rewrite_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && let Some(name) = reference.strip_prefix("#/components/schemas/")
            {
                let rewritten = format!("#/$defs/{name}");
                map.insert("$ref".to_string(), Value::String(rewritten));
            }
            for v in map.values_mut() {
                rewrite_refs(v);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                rewrite_refs(item);
            }
        }
        _ => {}
    }
}

fn to_ref_rewritten_value<T: serde::Serialize>(value: &T) -> Value {
    let mut json = serde_json::to_value(value).unwrap_or(Value::Null);
    rewrite_refs(&mut json);
    json
}

/// All of `components.schemas`, keyed by name, with internal `$ref`s
/// rewritten to `#/$defs/...` — embedded as `$defs` in every generated
/// operation schema so each one is a fully standalone, Ajv-compilable
/// document. This duplicates shared definitions across operations rather
/// than registering them once in Ajv; simpler and more robust than
/// cross-schema `$ref` resolution, at the cost of some file size.
fn build_defs(components: Option<&Components>) -> Value {
    let mut defs = Map::new();
    if let Some(components) = components {
        for (name, schema) in &components.schemas {
            defs.insert(name.clone(), to_ref_rewritten_value(schema));
        }
    }
    Value::Object(defs)
}

fn insert_defs_if_any(schema: &mut Value, components: Option<&Components>) {
    let defs = build_defs(components);
    if defs.as_object().is_some_and(|map| !map.is_empty())
        && let Value::Object(map) = schema
    {
        map.insert("$defs".to_string(), defs);
    }
}

fn parameter_data(parameter: &Parameter) -> &openapiv3::ParameterData {
    match parameter {
        Parameter::Query { parameter_data, .. }
        | Parameter::Header { parameter_data, .. }
        | Parameter::Path { parameter_data, .. }
        | Parameter::Cookie { parameter_data, .. } => parameter_data,
    }
}

fn parameter_schema(parameter: &Parameter) -> Option<&ReferenceOr<Schema>> {
    match &parameter_data(parameter).format {
        openapiv3::ParameterSchemaOrContent::Schema(schema) => Some(schema),
        openapiv3::ParameterSchemaOrContent::Content(_) => None,
    }
}

fn json_media_schema(content: &IndexMap<String, MediaType>) -> Option<&ReferenceOr<Schema>> {
    content
        .get("application/json")
        .and_then(|media| media.schema.as_ref())
}

/// Builds the JSON Schema Ajv validates `call` arguments against: an object
/// whose properties are the operation's parameters (by name) plus a `body`
/// property for the request body's `application/json` schema, if any.
/// `$ref`'d shared parameters (rather than inline ones) aren't resolved —
/// a v1 limitation matching `normalize_operations`' treatment of `$ref`'d
/// path items.
pub fn resolve_input_schema(operation: &Operation, components: Option<&Components>) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();

    for parameter_ref in &operation.parameters {
        let ReferenceOr::Item(parameter) = parameter_ref else {
            continue;
        };
        let data = parameter_data(parameter);
        let schema = parameter_schema(parameter)
            .map(to_ref_rewritten_value)
            .unwrap_or_else(|| Value::Object(Map::new()));
        properties.insert(data.name.clone(), schema);
        if data.required {
            required.push(Value::String(data.name.clone()));
        }
    }

    if let Some(ReferenceOr::Item(request_body)) = &operation.request_body {
        let body_schema = json_media_schema(&request_body.content)
            .map(to_ref_rewritten_value)
            .unwrap_or_else(|| Value::Object(Map::new()));
        properties.insert("body".to_string(), body_schema);
        if request_body.required {
            required.push(Value::String("body".to_string()));
        }
    }

    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".to_string(), Value::Array(required));
    }

    let mut schema = Value::Object(schema);
    insert_defs_if_any(&mut schema, components);
    schema
}

fn is_success_status(code: &StatusCode) -> bool {
    matches!(code, StatusCode::Code(200..=299) | StatusCode::Range(2))
}

/// Builds the JSON Schema Ajv validates the live response against: the
/// `application/json` schema of the first documented 2xx response (falling
/// back to `default`, then to an accept-anything `{}` schema for endpoints
/// without a documented success body, e.g. 204 No Content) — protecting
/// callers from upstream API drift (architecture.md's `call` pipeline).
pub fn resolve_output_schema(operation: &Operation, components: Option<&Components>) -> Value {
    let success_response = operation
        .responses
        .responses
        .iter()
        .find(|(code, _)| is_success_status(code))
        .map(|(_, response)| response)
        .or(operation.responses.default.as_ref());

    let Some(ReferenceOr::Item(response)) = success_response else {
        return Value::Object(Map::new());
    };

    let Some(schema) = json_media_schema(&response.content) else {
        return Value::Object(Map::new());
    };

    let mut schema = to_ref_rewritten_value(schema);
    insert_defs_if_any(&mut schema, components);
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> openapiv3::OpenAPI {
        serde_yaml::from_str(yaml).expect("fixture must parse as OpenAPI")
    }

    fn first_operation(doc: &openapiv3::OpenAPI) -> &Operation {
        doc.paths
            .paths
            .values()
            .find_map(|item| match item {
                ReferenceOr::Item(item) => item.get.as_ref(),
                ReferenceOr::Reference { .. } => None,
            })
            .expect("fixture must declare a GET operation")
    }

    #[test]
    fn resolves_inline_parameter_and_body_schemas() {
        let doc = parse(
            r##"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths:
  /widgets/{id}:
    get:
      operationId: getWidget
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema:
                type: object
                properties:
                  name:
                    type: string
"##,
        );

        let operation = first_operation(&doc);
        let input = resolve_input_schema(operation, doc.components.as_ref());
        assert_eq!(input["type"], "object");
        assert_eq!(input["properties"]["id"]["type"], "string");
        assert_eq!(input["required"][0], "id");

        let output = resolve_output_schema(operation, doc.components.as_ref());
        assert_eq!(output["properties"]["name"]["type"], "string");
    }

    #[test]
    fn rewrites_component_schema_refs_into_defs() {
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
"##,
        );

        let operation = first_operation(&doc);
        let output = resolve_output_schema(operation, doc.components.as_ref());

        assert_eq!(output["$ref"], "#/$defs/Widget");
        assert_eq!(
            output["$defs"]["Widget"]["properties"]["name"]["type"],
            "string"
        );
        // The self-referential "parent" field must point at the rewritten
        // $defs location too, not the original OpenAPI-only pointer.
        assert_eq!(
            output["$defs"]["Widget"]["properties"]["parent"]["$ref"],
            "#/$defs/Widget"
        );
    }

    #[test]
    fn resolves_allof_schemas_with_nested_refs() {
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
      requestBody:
        required: true
        content:
          application/json:
            schema:
              allOf:
                - $ref: "#/components/schemas/Base"
                - type: object
                  properties:
                    extra:
                      type: string
      responses:
        "201":
          description: Created
components:
  schemas:
    Base:
      type: object
      properties:
        id:
          type: string
"##,
        );

        let operation = doc
            .paths
            .paths
            .values()
            .find_map(|item| match item {
                ReferenceOr::Item(item) => item.post.as_ref(),
                ReferenceOr::Reference { .. } => None,
            })
            .unwrap();

        let input = resolve_input_schema(operation, doc.components.as_ref());
        let all_of = input["properties"]["body"]["allOf"].as_array().unwrap();
        assert_eq!(all_of[0]["$ref"], "#/$defs/Base");
        assert_eq!(all_of[1]["properties"]["extra"]["type"], "string");
        assert_eq!(input["$defs"]["Base"]["properties"]["id"]["type"], "string");
    }

    #[test]
    fn missing_success_response_yields_accept_anything_schema() {
        let doc = parse(
            r##"
openapi: 3.0.0
info:
  title: Test
  version: "1.0.0"
paths:
  /widgets/{id}:
    delete:
      operationId: deleteWidget
      responses:
        "204":
          description: No Content
"##,
        );

        let operation = doc
            .paths
            .paths
            .values()
            .find_map(|item| match item {
                ReferenceOr::Item(item) => item.delete.as_ref(),
                ReferenceOr::Reference { .. } => None,
            })
            .unwrap();

        let output = resolve_output_schema(operation, doc.components.as_ref());
        assert_eq!(output, Value::Object(Map::new()));
    }
}
