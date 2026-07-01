pub mod fetch;
pub mod parse;
pub mod source;

use anyhow::Result;
use openapiv3::OpenAPI;

use fetch::load_raw;
use parse::{detect_format_from_name, parse_document};
use source::{InputSource, classify};

/// Loads and parses an OpenAPI spec from a local path or remote URL, JSON or
/// YAML, into a normalized document (architecture.md §1, step 1).
pub async fn ingest(input: &str) -> Result<OpenAPI> {
    let source = classify(input);
    let raw = load_raw(&source).await?;
    let hint = match &source {
        InputSource::LocalFile(path) => path.to_str().and_then(detect_format_from_name),
        InputSource::Url(url) => detect_format_from_name(url.path()),
    };
    parse_document(&raw, hint)
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path as path_matcher};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn ingests_local_json_fixture() {
        let doc = ingest("tests/fixtures/openapi/minimal.json").await.unwrap();
        assert_eq!(doc.info.title, "Minimal API");
        assert!(doc.paths.paths.contains_key("/ping"));
    }

    #[tokio::test]
    async fn ingests_local_yaml_fixture() {
        let doc = ingest("tests/fixtures/openapi/minimal.yaml").await.unwrap();
        assert_eq!(doc.info.title, "Minimal API");
        assert!(doc.paths.paths.contains_key("/ping"));
    }

    #[tokio::test]
    async fn rejects_malformed_local_fixture() {
        let err = ingest("tests/fixtures/openapi/malformed.yaml")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("failed to parse OpenAPI spec"));
    }

    #[tokio::test]
    async fn errors_on_missing_local_file() {
        assert!(
            ingest("tests/fixtures/openapi/does-not-exist.yaml")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn ingests_remote_yaml() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_matcher("/spec.yaml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(include_str!("../../tests/fixtures/openapi/minimal.yaml")),
            )
            .mount(&server)
            .await;

        let doc = ingest(&format!("{}/spec.yaml", server.uri()))
            .await
            .unwrap();

        assert_eq!(doc.info.title, "Minimal API");
    }

    #[tokio::test]
    async fn ingests_remote_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_matcher("/spec.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(include_str!("../../tests/fixtures/openapi/minimal.json")),
            )
            .mount(&server)
            .await;

        let doc = ingest(&format!("{}/spec.json", server.uri()))
            .await
            .unwrap();

        assert_eq!(doc.info.title, "Minimal API");
    }
}
