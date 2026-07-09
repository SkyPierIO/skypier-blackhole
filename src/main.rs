use clap::Parser;
use skypier_blackhole::{Cli, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // The TUI owns the terminal and captures logs into its own panel,
    // so only install the console logger for regular commands.
    if !cli.is_tui() {
        skypier_blackhole::setup_logging()?;

        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            "Starting Skypier Blackhole DNS resolver"
        );
    }

    // Execute CLI command (each command loads its own config)
    cli.execute().await?;

    Ok(())
}
