use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Output targets `mcpify` knows how to generate. "typescript" shipped in
/// v1 (PRD REQ-1.1.4); "rust" joined in v2, "python" in v3, "csharp" in
/// v4, "go" in v5. ("python" was registered in `targets::build_registry()`
/// back in v3 Story P8 but missed being added here — a pre-existing gap
/// fixed alongside "csharp" since both needed this list updated to
/// actually be reachable from the CLI.)
pub const SUPPORTED_LANGUAGES: &[&str] = &["typescript", "rust", "python", "csharp", "go"];

/// Sentinel label recorded in a project's version ledger when `generate` is
/// invoked without an explicit `--version` (v8's multi-version support).
/// Kept as an ordinary string, never specially parsed — a project that will
/// only ever have one version (e.g. Bamboo) never needs to think about this.
pub const DEFAULT_VERSION_LABEL: &str = "default";

#[derive(Debug, Parser)]
#[command(
    name = "mcpify",
    version,
    about = "Generate a deployment-ready, enterprise-grade MCP server project from an OpenAPI spec."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path or remote URL to the source OpenAPI specification (JSON/YAML).
    /// Required unless a subcommand is given.
    #[arg(short = 'i', long = "input")]
    pub input: Option<String>,

    /// Destination directory where the project will be generated.
    /// Required unless a subcommand is given.
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    /// Target stack ("typescript", "rust", "python", "csharp", or "go")
    #[arg(short = 'l', long = "language", default_value = "typescript")]
    pub language: String,

    /// Overwrite the destination folder if it already contains files
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Emit a registry-publish step (cargo publish / uv publish / dotnet
    /// nuget push) in the generated release workflow, for Rust/Python/C#.
    /// Off by default — these are generated applications tied to one API,
    /// not reusable libraries, so publishing them to a public registry is a
    /// deliberate choice, not a default. No effect for typescript (which
    /// always publishes) or go (which has no registry-publish step).
    #[arg(long = "publish-registry")]
    pub publish_registry: bool,

    /// v8: label recorded for the spec ingested by this `generate` run, so a
    /// later `add-version` call can extend this project with more versions.
    /// Defaults to `DEFAULT_VERSION_LABEL` ("default") for projects that
    /// only ever have one version (e.g. Bamboo, which publishes no other
    /// API version to add later) — those users never need this flag.
    /// Named `--api-version` (not `--version`) to avoid colliding with
    /// clap's auto-generated `-V/--version` flag for mcpify's own tool
    /// version.
    #[arg(long = "api-version", default_value = DEFAULT_VERSION_LABEL)]
    pub api_version: String,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Add another OpenAPI spec version to an already-generated project as
    /// an extra, independently-queryable store, without regenerating the
    /// whole project (v8 multi-version support).
    AddVersion {
        /// Path to the previously-generated project directory
        #[arg(long = "project")]
        project: PathBuf,

        /// Label for this version (e.g. "11.2") — an opaque string, never
        /// parsed or compared as a version number.
        #[arg(long = "version")]
        version: String,

        /// Path or remote URL to this version's OpenAPI specification
        #[arg(short = 'i', long = "input")]
        input: String,

        /// Promote this version to be the project's new default/latest.
        /// The version it replaces is preserved (demoted to its own store
        /// file), never silently overwritten.
        #[arg(long = "set-default")]
        set_default: bool,

        /// Overwrite this version's existing store if `--version` names a
        /// version that was already added
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
}

/// Generate-mode arguments, validated out of `Cli` once no subcommand is
/// present — kept separate from `Cli` itself so `input`/`output` can stay
/// `Option` at the clap-parsing level (required only when `command` is
/// `None`) while every other part of the generate pipeline still gets a
/// plain, non-optional `String`/`PathBuf` to work with.
#[derive(Debug, Clone)]
pub struct GenerateArgs {
    pub input: String,
    pub output: PathBuf,
    pub language: String,
    pub force: bool,
    pub publish_registry: bool,
    pub version: String,
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

    /// Validates and unpacks the flat `generate` invocation (`self.command`
    /// is `None`) into `GenerateArgs`. `-i/--input` and `-o/--output` are
    /// optional at the clap level (so `add-version` can be a valid parse
    /// without them), so this is the one place that still enforces "both
    /// are required for `generate`" — the same contract `clap`'s
    /// `required = true` used to enforce directly on those fields.
    pub fn into_generate_args(self) -> anyhow::Result<GenerateArgs> {
        let input = self.input.ok_or_else(|| {
            anyhow::anyhow!("the following required arguments were not provided: --input <INPUT>")
        })?;
        let output = self.output.ok_or_else(|| {
            anyhow::anyhow!("the following required arguments were not provided: --output <OUTPUT>")
        })?;
        Ok(GenerateArgs {
            input,
            output,
            language: self.language,
            force: self.force,
            publish_registry: self.publish_registry,
            version: self.api_version,
        })
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
        let cli = parse(&["-o", "./out"]).unwrap();
        assert!(cli.into_generate_args().is_err());
    }

    #[test]
    fn requires_output() {
        let cli = parse(&["-i", "spec.yaml"]).unwrap();
        assert!(cli.into_generate_args().is_err());
    }

    #[test]
    fn defaults_language_to_typescript_and_force_to_false() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out"]).unwrap();
        assert_eq!(cli.language, "typescript");
        assert!(!cli.force);
    }

    #[test]
    fn defaults_version_to_sentinel() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out"]).unwrap();
        assert_eq!(cli.api_version, DEFAULT_VERSION_LABEL);
        let args = cli.into_generate_args().unwrap();
        assert_eq!(args.version, "default");
    }

    #[test]
    fn parses_explicit_version_label() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out", "--api-version", "11.3"]).unwrap();
        assert_eq!(cli.into_generate_args().unwrap().version, "11.3");
    }

    #[test]
    fn into_generate_args_round_trips_all_fields() {
        let cli = parse(&[
            "-i",
            "spec.yaml",
            "-o",
            "./out",
            "-l",
            "go",
            "--force",
            "--publish-registry",
            "--api-version",
            "11.3",
        ])
        .unwrap();
        let args = cli.into_generate_args().unwrap();
        assert_eq!(args.input, "spec.yaml");
        assert_eq!(args.output, PathBuf::from("./out"));
        assert_eq!(args.language, "go");
        assert!(args.force);
        assert!(args.publish_registry);
        assert_eq!(args.version, "11.3");
    }

    #[test]
    fn no_subcommand_parses_as_generate() {
        let cli = parse(&["-i", "spec.yaml", "-o", "./out"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_add_version_subcommand() {
        let cli = parse(&[
            "add-version",
            "--project",
            "./out",
            "--version",
            "11.2",
            "-i",
            "spec-v11.2.yaml",
            "--set-default",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::AddVersion {
                project,
                version,
                input,
                set_default,
                force,
            }) => {
                assert_eq!(project, PathBuf::from("./out"));
                assert_eq!(version, "11.2");
                assert_eq!(input, "spec-v11.2.yaml");
                assert!(set_default);
                assert!(!force);
            }
            None => panic!("expected the add-version subcommand to parse"),
        }
    }

    #[test]
    fn add_version_subcommand_requires_project_version_and_input() {
        assert!(parse(&["add-version", "--version", "11.2", "-i", "spec.yaml"]).is_err());
        assert!(parse(&["add-version", "--project", "./out", "-i", "spec.yaml"]).is_err());
        assert!(parse(&["add-version", "--project", "./out", "--version", "11.2"]).is_err());
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
