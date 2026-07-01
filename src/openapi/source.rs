use std::path::PathBuf;

use url::Url;

/// Where the raw OpenAPI spec text comes from — a local filesystem path or a
/// remote `http`/`https` URL (architecture.md §1, step 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSource {
    LocalFile(PathBuf),
    Url(Url),
}

/// Classifies `-i/--input` as a remote URL only when it parses as one with an
/// `http`/`https` scheme; everything else (including relative/absolute paths,
/// which never parse as absolute URLs) is treated as a local file path.
pub fn classify(input: &str) -> InputSource {
    match Url::parse(input) {
        Ok(url) if url.scheme() == "http" || url.scheme() == "https" => InputSource::Url(url),
        _ => InputSource::LocalFile(PathBuf::from(input)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_https_url() {
        assert_eq!(
            classify("https://example.com/spec.yaml"),
            InputSource::Url(Url::parse("https://example.com/spec.yaml").unwrap())
        );
    }

    #[test]
    fn classifies_http_url() {
        assert!(matches!(
            classify("http://localhost:8080/spec.json"),
            InputSource::Url(_)
        ));
    }

    #[test]
    fn classifies_relative_path_as_local_file() {
        assert_eq!(
            classify("./specs/api.yaml"),
            InputSource::LocalFile(PathBuf::from("./specs/api.yaml"))
        );
    }

    #[test]
    fn classifies_absolute_path_as_local_file() {
        assert_eq!(
            classify("/tmp/api.json"),
            InputSource::LocalFile(PathBuf::from("/tmp/api.json"))
        );
    }

    #[test]
    fn classifies_non_http_scheme_as_local_file() {
        // Url::parse succeeds for "file:///..." and other schemes we don't
        // treat as remote fetches, so they fall back to local-file handling.
        assert!(matches!(
            classify("ftp://example.com/spec.yaml"),
            InputSource::LocalFile(_)
        ));
    }
}
