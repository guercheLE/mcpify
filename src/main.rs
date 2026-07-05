use std::io::IsTerminal;

use clap::Parser;

use mcpify::add_version::{self, AddVersionRequest};
use mcpify::cli::{Cli, Commands};
use mcpify::pipeline::run_shared_pipeline;
use mcpify::targets;

#[tokio::main]
async fn main() {
    #[cfg(feature = "profiling")]
    console_subscriber::init();

    // Only the real CLI binary gets progress output — every unit test and
    // `tests/*.rs` integration test calls into the library directly and
    // never reaches this line, so `progress::enabled()` stays `false` there.
    mcpify::progress::init(true);

    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::AddVersion {
            project,
            version,
            input,
            set_default,
            force,
        }) => {
            add_version::run(AddVersionRequest {
                project_dir: project,
                version_label: version,
                input,
                set_default,
                force,
            })
            .await
        }
        None => run_generate(cli).await,
    }
}

async fn run_generate(cli: Cli) -> anyhow::Result<()> {
    cli.validate_language()?;
    let args = cli.into_generate_args()?;

    // Only fall back to an interactive auth-scheme prompt (REQ-1.2.4) when
    // there's a human on the other end of stdin; a scripted/CI invocation
    // with an unclassifiable spec should fail loudly instead of hanging.
    let interactive = std::io::stdin().is_terminal();

    let ctx = run_shared_pipeline(
        &args.input,
        args.output,
        args.force,
        interactive,
        args.publish_registry,
        &args.version,
    )
    .await?;

    let registry = targets::build_registry();
    let target = registry
        .get(args.language.as_str())
        .ok_or_else(|| anyhow::anyhow!("no generator registered for target '{}'", args.language))?;

    target.execute(&ctx).await?;

    add_version::seed::seed_ledger_after_generate(&ctx, &args.language, &args.version).await?;

    if mcpify::progress::enabled() {
        eprintln!(
            "==> Generated project ready at {}",
            ctx.output_dir.display()
        );
    }

    Ok(())
}
