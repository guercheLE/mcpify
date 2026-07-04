use std::io::IsTerminal;
use std::path::PathBuf;

use clap::Parser;

use mcpify::cli::Cli;
use mcpify::pipeline::run_shared_pipeline;
use mcpify::targets;

#[tokio::main]
async fn main() {
    #[cfg(feature = "profiling")]
    console_subscriber::init();

    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    cli.validate_language()?;

    // Only fall back to an interactive auth-scheme prompt (REQ-1.2.4) when
    // there's a human on the other end of stdin; a scripted/CI invocation
    // with an unclassifiable spec should fail loudly instead of hanging.
    let interactive = std::io::stdin().is_terminal();

    let ctx = run_shared_pipeline(
        &cli.input,
        PathBuf::from(&cli.output),
        cli.force,
        interactive,
        cli.publish_registry,
    )
    .await?;

    let registry = targets::build_registry();
    let target = registry
        .get(cli.language.as_str())
        .ok_or_else(|| anyhow::anyhow!("no generator registered for target '{}'", cli.language))?;

    target.execute(&ctx).await
}
