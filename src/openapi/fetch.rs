use anyhow::{Context, Result};

use super::source::InputSource;

/// Loads the raw spec text, non-blocking either way (architecture.md §1, step 1).
pub async fn load_raw(source: &InputSource) -> Result<String> {
    match source {
        InputSource::LocalFile(path) => tokio::fs::read_to_string(path).await.with_context(|| {
            format!(
                "failed to read OpenAPI spec from local file '{}'",
                path.display()
            )
        }),
        InputSource::Url(url) => {
            let response = reqwest::get(url.clone())
                .await
                .with_context(|| format!("failed to fetch OpenAPI spec from '{url}'"))?
                .error_for_status()
                .with_context(|| format!("OpenAPI spec URL '{url}' returned an error response"))?;
            response
                .text()
                .await
                .with_context(|| format!("failed to read response body from '{url}'"))
        }
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn loads_local_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("spec.yaml");
        tokio::fs::write(&file_path, "openapi: 3.0.0")
            .await
            .unwrap();

        let raw = load_raw(&InputSource::LocalFile(file_path)).await.unwrap();

        assert_eq!(raw, "openapi: 3.0.0");
    }

    #[tokio::test]
    async fn errors_on_missing_local_file() {
        let missing = InputSource::LocalFile("/does/not/exist/spec.yaml".into());
        assert!(load_raw(&missing).await.is_err());
    }

    #[tokio::test]
    async fn fetches_remote_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/spec.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"openapi\":\"3.0.0\"}"))
            .mount(&server)
            .await;

        let url = url::Url::parse(&format!("{}/spec.json", server.uri())).unwrap();
        let raw = load_raw(&InputSource::Url(url)).await.unwrap();

        assert_eq!(raw, "{\"openapi\":\"3.0.0\"}");
    }

    #[tokio::test]
    async fn fetches_remote_yaml() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/spec.yaml"))
            .respond_with(ResponseTemplate::new(200).set_body_string("openapi: 3.0.0"))
            .mount(&server)
            .await;

        let url = url::Url::parse(&format!("{}/spec.yaml", server.uri())).unwrap();
        let raw = load_raw(&InputSource::Url(url)).await.unwrap();

        assert_eq!(raw, "openapi: 3.0.0");
    }

    #[tokio::test]
    async fn errors_on_remote_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing.yaml"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let url = url::Url::parse(&format!("{}/missing.yaml", server.uri())).unwrap();
        assert!(load_raw(&InputSource::Url(url)).await.is_err());
    }

    #[tokio::test]
    async fn errors_on_remote_500() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/broken.yaml"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let url = url::Url::parse(&format!("{}/broken.yaml", server.uri())).unwrap();
        assert!(load_raw(&InputSource::Url(url)).await.is_err());
    }

    #[tokio::test]
    async fn follows_redirects() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/redirected.yaml"))
            .respond_with(ResponseTemplate::new(200).set_body_string("openapi: 3.0.0"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/spec-redirect.yaml"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", format!("{}/redirected.yaml", server.uri())),
            )
            .mount(&server)
            .await;

        let url = url::Url::parse(&format!("{}/spec-redirect.yaml", server.uri())).unwrap();
        let raw = load_raw(&InputSource::Url(url)).await.unwrap();

        assert_eq!(raw, "openapi: 3.0.0");
    }
}
