use std::path::PathBuf;

use clap::Parser;

/// Output targets `mcpify` knows how to generate. "typescript" shipped in
/// v1 (PRD REQ-1.1.4); "rust" joined in v2, "python" in v3, "csharp" in
/// v4, "go" in v5. ("python" was registered in `targets::build_registry()`
/// back in v3 Story P8 but missed being added here — a pre-existing gap
/// fixed alongside "csharp" since both needed this list updated to
/// actually be reachable from the CLI.)
pub const SUPPORTED_LANGUAGES: &[&str] = &["typescript", "rust", "python", "csharp", "go"];

#[derive(Debug, Parser)]
#[command(
    name = "mcpify",
    version,
    about = "Generate a deployment-ready, enterprise-grade MCP server project from an OpenAPI spec."
)]
pub struct Cli {
    /// Path or remote URL to the source OpenAPI specification (JSON/YAML)
    #[arg(short = 'i', long = "input")]
    pub input: String,

    /// Destination directory where the project will be generated
    #[arg(short = 'o', long = "output")]
    pub output: PathBuf,

    /// Target stack ("typescript", "rust", "python", "csharp", or "go")
    #[arg(short = 'l', long = "language", default_value = "typescript")]
    pub language: String,

    /// Overwrite the destination folder if it already contains files
    #[arg(short = 'f', long = "force")]
    pub force: bool,
}

impl Cli {
    /// REQ-1.1.4: reject unimplemented targets with a clear message pointing
    /// at the roadmap, rather than silently falling through.
    pub fn validate_language(&self) -> anyhow::Result<()> {
        if SUPPORTED_LANGUAGES.contains(&self.language.as_str()) {
            Ok(())
        } else {
            anyhow::bail!(
                "'{}' is not yet supported. Supported targets: {}. See the roadmap in architecture.md § \"Target Language Roadmap\".",
                self.language,
                SUPPORTED_LANGUAGES.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("mcpify").chain(args.iter().copied()))
    }

    #[test]
    fn requires_input() {
        assert!(parse(&["-o", "./out"]).is_err());
    }

    #[test]
    fn requires_output() {
        assert!(parse(&["-i", "spec.yaml"]).is_err());
    }

    #[test]
    fn defaults_language_to_typescript_and_force_to_false() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out"]).unwrap();
        assert_eq!(cli.language, "typescript");
        assert!(!cli.force);
    }

    #[test]
    fn parses_force_flag() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out", "--force"]).unwrap();
        assert!(cli.force);
    }

    #[test]
    fn accepts_typescript_language() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out", "-l", "typescript"]).unwrap();
        assert!(cli.validate_language().is_ok());
    }

    #[test]
    fn accepts_rust_language() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out", "-l", "rust"]).unwrap();
        assert!(cli.validate_language().is_ok());
    }

    #[test]
    fn accepts_python_language() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out", "-l", "python"]).unwrap();
        assert!(cli.validate_language().is_ok());
    }

    #[test]
    fn accepts_csharp_language() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out", "-l", "csharp"]).unwrap();
        assert!(cli.validate_language().is_ok());
    }

    #[test]
    fn accepts_go_language() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out", "-l", "go"]).unwrap();
        assert!(cli.validate_language().is_ok());
    }

    #[test]
    fn rejects_unsupported_language() {
        for lang in ["ruby", "java"] {
            let cli = parse(&["-i", "spec.yaml", "-o", "./out", "-l", lang]).unwrap();
            let err = cli.validate_language().unwrap_err();
            assert!(err.to_string().contains("not yet supported"));
        }
    }
}
