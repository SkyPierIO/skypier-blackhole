mod blocklist;
mod cli;
mod config;
mod dns;
mod downloader;
mod loader;
mod logger;
mod metrics;
mod scheduler;
pub mod tui;

pub use blocklist::BlocklistManager;
pub use cli::Cli;
pub use config::{get_default_config_path, Config};
pub use dns::DnsServer;
pub use downloader::BlocklistDownloader;
pub use logger::setup_logging;
pub use metrics::RuntimeMetrics;
pub use scheduler::UpdateScheduler;

pub type Result<T> = std::result::Result<T, anyhow::Error>;
