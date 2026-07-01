mod auth_profile;
mod cli;
mod context;
mod targets;

use std::path::PathBuf;

use clap::Parser;

use cli::Cli;
use context::GeneratorContext;

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

    // The shared ingestion pipeline (OpenAPI parse, directory guard, auth
    // profiling, mcp_store.db assembly — architecture.md §1 steps 1-4) fills
    // in output_dir_preexisted/auth_schemes and lands in Story 6. Until then
    // this constructs a stub context so CLI parsing and target dispatch are
    // already exercised end to end.
    let ctx = GeneratorContext {
        openapi_input: cli.input.clone(),
        output_dir: PathBuf::from(&cli.output),
        force: cli.force,
        output_dir_preexisted: false,
        auth_schemes: Vec::new(),
    };

    let registry = targets::build_registry();
    let target = registry
        .get(cli.language.as_str())
        .ok_or_else(|| anyhow::anyhow!("no generator registered for target '{}'", cli.language))?;

    target.execute(&ctx).await
}
