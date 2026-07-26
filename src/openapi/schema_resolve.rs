use std::collections::HashSet;

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
            for keyword in ["$ref", "$dynamicRef"] {
                if let Some(Value::String(reference)) = map.get(keyword)
                    && let Some(name) = reference.strip_prefix("#/components/schemas/")
                {
                    let rewritten = format!("#/$defs/{name}");
                    map.insert(keyword.to_string(), Value::String(rewritten));
                }
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

fn collect_component_refs(value: &Value, refs: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            for keyword in ["$ref", "$dynamicRef"] {
                if let Some(Value::String(reference)) = map.get(keyword)
                    && let Some(name) = reference.strip_prefix("#/$defs/")
                {
                    refs.insert(name.to_string());
                }
            }
            for v in map.values() {
                collect_component_refs(v, refs);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_component_refs(item, refs);
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

/// Only the component schemas transitively reachable from this operation
/// schema, keyed by name, with internal `$ref`s rewritten to `#/$defs/...`.
///
/// Endpoint rows stay self-contained for Ajv: every referenced component is
/// embedded in the operation's `$defs`. Unlike the older all-components
/// strategy, unrelated schemas are not duplicated into every operation.
fn build_defs(schema: &Value, components: Option<&Components>) -> Value {
    let mut defs = Map::new();
    let Some(components) = components else {
        return Value::Object(defs);
    };

    let mut pending = HashSet::new();
    let mut visited = HashSet::new();
    collect_component_refs(schema, &mut pending);

    while let Some(name) = pending.iter().next().cloned() {
        pending.remove(&name);
        if !visited.insert(name.clone()) {
            continue;
        }

        let Some(component_schema) = components.schemas.get(&name) else {
            continue;
        };

        let def = to_ref_rewritten_value(component_schema);
        collect_component_refs(&def, &mut pending);
        defs.insert(name, def);
    }

    Value::Object(defs)
}

fn insert_defs_if_any(schema: &mut Value, components: Option<&Components>) {
    let Value::Object(defs) = build_defs(schema, components) else {
        return;
    };
    if defs.is_empty() {
        return;
    }
    *schema = finalize_with_defs(std::mem::take(schema), defs);
}

/// Names of every `$defs` entry that participates in a reference cycle —
/// directly self-referential, or mutually recursive through one or more
/// other entries. These are the only entries `finalize_with_defs` cannot
/// fully inline: JSON has no finite representation for infinite expansion,
/// so a cyclic entry keeps its `$ref`/`$defs` form instead.
fn detect_cyclic_defs(defs: &Map<String, Value>) -> HashSet<String> {
    fn visit(
        name: &str,
        defs: &Map<String, Value>,
        stack: &mut Vec<String>,
        done: &mut HashSet<String>,
        cyclic: &mut HashSet<String>,
    ) {
        if done.contains(name) {
            return;
        }
        if let Some(pos) = stack.iter().position(|entry| entry == name) {
            for entry in &stack[pos..] {
                cyclic.insert(entry.clone());
            }
            return;
        }
        let Some(body) = defs.get(name) else {
            return;
        };
        stack.push(name.to_string());
        let mut refs = HashSet::new();
        collect_component_refs(body, &mut refs);
        for reference in refs {
            visit(&reference, defs, stack, done, cyclic);
        }
        stack.pop();
        done.insert(name.to_string());
    }

    let mut cyclic = HashSet::new();
    let mut done = HashSet::new();
    let mut stack = Vec::new();
    for name in defs.keys() {
        visit(name, defs, &mut stack, &mut done, &mut cyclic);
    }
    cyclic
}

/// Replaces every `$ref` to a non-cyclic `#/$defs/<name>` entry with a deep
/// copy of that entry's own (already similarly inlined) content. A `$ref`
/// naming an entry in `keep` (only ever a cyclic entry — see
/// `detect_cyclic_defs`) is left exactly as-is, and `$dynamicRef` is never
/// touched: it's OpenAPI 3.1's mechanism for describing an intentionally
/// recursive schema, which this function cannot flatten any more than a
/// cyclic plain `$ref` can be.
fn inline_except(value: &Value, defs: &Map<String, Value>, keep: &HashSet<String>) -> Value {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && let Some(name) = reference.strip_prefix("#/$defs/")
                && !keep.contains(name)
                && let Some(target) = defs.get(name)
            {
                let inlined = inline_except(target, defs, keep);
                if map.len() == 1 {
                    return inlined;
                }
                let mut merged = inlined.as_object().cloned().unwrap_or_default();
                for (key, v) in map {
                    if key == "$ref" {
                        continue;
                    }
                    merged.insert(key.clone(), inline_except(v, defs, keep));
                }
                return Value::Object(merged);
            }
            let mut out = Map::new();
            for (key, v) in map {
                out.insert(key.clone(), inline_except(v, defs, keep));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| inline_except(item, defs, keep))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Finishes a schema that already has every `$ref`/`$dynamicRef` rewritten
/// to `#/$defs/<name>`, given the full map of reachable definitions:
/// inlines every `$ref` whose target isn't part of a reference cycle, so
/// the only `$ref`/`$dynamicRef`/`$defs` left in the result (if any) are
/// the ones a genuinely recursive component schema requires — there's no
/// finite JSON representation for those. In the common case (no recursive
/// components in the source spec — e.g. every PostgreSQL catalog operation
/// today), this removes `$ref`/`$defs` entirely.
fn finalize_with_defs(schema: Value, defs: Map<String, Value>) -> Value {
    let cyclic = detect_cyclic_defs(&defs);
    let result = inline_except(&schema, &defs, &cyclic);

    let mut pending = HashSet::new();
    collect_component_refs(&result, &mut pending);
    if pending.is_empty() {
        return result;
    }

    let mut kept = Map::new();
    while let Some(name) = pending.iter().next().cloned() {
        pending.remove(&name);
        if kept.contains_key(&name) {
            continue;
        }
        let Some(body) = defs.get(&name) else {
            continue;
        };
        let inlined_body = inline_except(body, &defs, &cyclic);
        collect_component_refs(&inlined_body, &mut pending);
        kept.insert(name, inlined_body);
    }

    let mut result = result;
    if let Value::Object(map) = &mut result {
        map.insert("$defs".to_string(), Value::Object(kept));
    }
    result
}

fn build_defs_from_value(schema: &Value, components: Option<&Map<String, Value>>) -> Value {
    let mut defs = Map::new();
    let Some(components) = components else {
        return Value::Object(defs);
    };

    let mut pending = HashSet::new();
    let mut visited = HashSet::new();
    collect_component_refs(schema, &mut pending);
    while let Some(name) = pending.iter().next().cloned() {
        pending.remove(&name);
        if !visited.insert(name.clone()) {
            continue;
        }
        let Some(component) = components.get(&name) else {
            continue;
        };
        let mut definition = component.clone();
        rewrite_refs(&mut definition);
        collect_component_refs(&definition, &mut pending);
        defs.insert(name, definition);
    }
    Value::Object(defs)
}

fn decorate_value_schema(
    mut schema: Value,
    components: Option<&Map<String, Value>>,
    openapi_31: bool,
) -> Value {
    rewrite_refs(&mut schema);
    let defs = build_defs_from_value(&schema, components);
    if !schema.is_object() {
        schema = serde_json::json!({ "allOf": [schema] });
    }
    if let Value::Object(defs_map) = defs
        && !defs_map.is_empty()
    {
        schema = finalize_with_defs(schema, defs_map);
    }
    let object = schema
        .as_object_mut()
        .expect("schema was wrapped as an object");
    if openapi_31 {
        object.insert(
            "$schema".to_string(),
            Value::String("https://json-schema.org/draft/2020-12/schema".to_string()),
        );
    }
    schema
}

/// Rewrites every `$ref`/`$dynamicRef` inside `schema` to point at a
/// `$defs` map embedded directly in `schema` itself, populated with every
/// component transitively reachable from it — the same self-containment
/// `decorate_value_schema` gives the Ajv-facing `validation_*_schema`
/// fields, applied instead to the *literal* OpenAPI-shape snapshot
/// (`NormalizedOperation::input_schema`/`output_schema`) that the `get`
/// tool hands back verbatim. Without this, a `get` response can contain a
/// `$ref` naming a component schema that appears nowhere else in the
/// response, leaving the caller (human or LLM) nothing to resolve it
/// against.
pub fn embed_literal_schema_defs(schema: &mut Value, components: Option<&Map<String, Value>>) {
    rewrite_refs(schema);
    let Value::Object(defs) = build_defs_from_value(schema, components) else {
        return;
    };
    if defs.is_empty() {
        return;
    }
    *schema = finalize_with_defs(std::mem::take(schema), defs);
}

pub fn resolve_input_schema_value(
    operation: &Map<String, Value>,
    components: Option<&Map<String, Value>>,
    openapi_31: bool,
) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();

    if let Some(parameters) = operation.get("parameters").and_then(Value::as_array) {
        for parameter in parameters {
            let Some(parameter) = parameter.as_object() else {
                continue;
            };
            if parameter.contains_key("$ref") {
                continue;
            }
            let Some(name) = parameter.get("name").and_then(Value::as_str) else {
                continue;
            };
            properties.insert(
                name.to_string(),
                parameter
                    .get("schema")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
            );
            if parameter.get("required").and_then(Value::as_bool) == Some(true) {
                required.push(Value::String(name.to_string()));
            }
        }
    }

    if let Some(request_body) = operation.get("requestBody").and_then(Value::as_object)
        && !request_body.contains_key("$ref")
    {
        let body_schema = request_body
            .get("content")
            .and_then(Value::as_object)
            .and_then(|content| content.get("application/json"))
            .and_then(Value::as_object)
            .and_then(|media| media.get("schema"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        properties.insert("body".to_string(), body_schema);
        if request_body.get("required").and_then(Value::as_bool) == Some(true) {
            required.push(Value::String("body".to_string()));
        }
    }

    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".to_string(), Value::Array(required));
    }
    decorate_value_schema(Value::Object(schema), components, openapi_31)
}

pub fn resolve_output_schema_value(
    operation: &Map<String, Value>,
    components: Option<&Map<String, Value>>,
    openapi_31: bool,
) -> Value {
    let responses = operation.get("responses").and_then(Value::as_object);
    let response = responses
        .and_then(|responses| {
            responses
                .iter()
                .find_map(|(status, response)| {
                    if status.eq_ignore_ascii_case("2XX") {
                        Some(response)
                    } else {
                        status
                            .parse::<u16>()
                            .ok()
                            .filter(|status| (200..300).contains(status))
                            .map(|_| response)
                    }
                })
                .or_else(|| responses.get("default"))
        })
        .and_then(Value::as_object);
    let schema = response
        .filter(|response| !response.contains_key("$ref"))
        .and_then(|response| response.get("content"))
        .and_then(Value::as_object)
        .and_then(|content| content.get("application/json"))
        .and_then(Value::as_object)
        .and_then(|media| media.get("schema"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    decorate_value_schema(schema, components, openapi_31)
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
    fn a_self_referential_component_keeps_ref_and_defs_since_it_cannot_be_inlined() {
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
    Unused:
      type: object
      properties:
        ignored:
          type: string
"##,
        );

        let operation = first_operation(&doc);
        let output = resolve_output_schema(operation, doc.components.as_ref());

        // Widget references itself, so full inlining would never terminate
        // — this is the one case `finalize_with_defs` leaves as $ref/$defs.
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
        assert!(output["$defs"].get("Unused").is_none());
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
        child:
          $ref: "#/components/schemas/Child"
    Child:
      type: object
      properties:
        id:
          type: string
    Unused:
      type: object
      properties:
        ignored:
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
        // Base and Child are acyclic, so both are fully inlined — no $ref
        // and no $defs left anywhere in the schema.
        assert_eq!(
            all_of[0]["properties"]["child"]["properties"]["id"]["type"],
            "string"
        );
        assert_eq!(all_of[1]["properties"]["extra"]["type"], "string");
        assert!(input.get("$defs").is_none());
    }

    #[test]
    fn a_component_referenced_from_multiple_places_is_inlined_at_each_one() {
        // Mirrors the real-world PostgresError shape: one shared, non-
        // recursive component reused across several response codes for the
        // same operation. Every occurrence must be independently inlined,
        // and $defs must not survive once nothing references it anymore.
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
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/Widget"
        "404":
          description: Not found
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/Error"
components:
  schemas:
    Widget:
      type: object
      properties:
        name:
          type: string
    Error:
      type: object
      properties:
        message:
          type: string
"##,
        );

        let operation = first_operation(&doc);
        let output = resolve_output_schema(operation, doc.components.as_ref());
        assert_eq!(output["properties"]["name"]["type"], "string");
        assert!(output.get("$ref").is_none());
        assert!(output.get("$defs").is_none());
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

    #[test]
    fn raw_resolver_accepts_success_response_ranges() {
        let operation = serde_json::json!({
            "responses": {
                "2XX": {
                    "description": "Success",
                    "content": {
                        "application/json": {
                            "schema": { "type": "string" }
                        }
                    }
                }
            }
        });

        let output = resolve_output_schema_value(operation.as_object().unwrap(), None, true);
        assert_eq!(output["type"], "string");
    }

    #[test]
    fn raw_resolver_rebases_dynamic_refs_and_embeds_dependencies() {
        let operation = serde_json::json!({
            "responses": {
                "200": {
                    "description": "Success",
                    "content": {
                        "application/json": {
                            "schema": { "$dynamicRef": "#/components/schemas/Node" }
                        }
                    }
                }
            }
        });
        let components = serde_json::json!({
            "Node": {
                "$dynamicAnchor": "node",
                "type": "object",
                "properties": {
                    "child": { "$dynamicRef": "#/components/schemas/Node" }
                }
            }
        });

        let output = resolve_output_schema_value(
            operation.as_object().unwrap(),
            components.as_object(),
            true,
        );
        assert_eq!(output["$dynamicRef"], "#/$defs/Node");
        assert_eq!(
            output["$defs"]["Node"]["properties"]["child"]["$dynamicRef"],
            "#/$defs/Node"
        );
    }
}
