use clap::Parser;
use skypier_blackhole::{Cli, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    skypier_blackhole::setup_logging()?;
    
    // Parse CLI arguments
    let cli = Cli::parse();
    
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Starting Skypier Blackhole DNS resolver");
    
    // Execute CLI command (each command loads its own config)
    cli.execute().await?;
    
    Ok(())
}
