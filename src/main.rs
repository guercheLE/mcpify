use std::io::IsTerminal;
use std::path::PathBuf;

use clap::Parser;

use mcpify::cli::Cli;
use mcpify::context::GeneratorContext;
use mcpify::pipeline::dir_guard::check_output_dir;
use mcpify::{auth_profile, openapi, targets};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    cli.validate_language()?;

    let output_dir = PathBuf::from(&cli.output);
    let output_dir_preexisted = check_output_dir(&output_dir, cli.force).await?;
    let doc = openapi::ingest(&cli.input).await?;

    // Only fall back to an interactive prompt (REQ-1.2.4) when there's a
    // human on the other end of stdin; a scripted/CI invocation with an
    // unclassifiable spec should fail loudly instead of hanging.
    let interactive = std::io::stdin().is_terminal();
    let auth_schemes = auth_profile::profile_auth(&doc, interactive).await?;

    // mcp_store.db assembly (Story 5) joins this flow as the shared pipeline
    // (Story 6) is wired up.
    let ctx = GeneratorContext {
        openapi_input: cli.input.clone(),
        output_dir,
        force: cli.force,
        output_dir_preexisted,
        auth_schemes,
    };

    let registry = targets::build_registry();
    let target = registry
        .get(cli.language.as_str())
        .ok_or_else(|| anyhow::anyhow!("no generator registered for target '{}'", cli.language))?;

    target.execute(&ctx).await
}
