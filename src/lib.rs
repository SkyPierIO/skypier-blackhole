mod cli;
mod config;
mod dns;
mod blocklist;
mod downloader;
mod scheduler;
mod logger;

pub use cli::Cli;
pub use config::{Config, get_default_config_path};
pub use dns::DnsServer;
pub use blocklist::BlocklistManager;
pub use downloader::BlocklistDownloader;
pub use scheduler::UpdateScheduler;
pub use logger::setup_logging;

pub type Result<T> = std::result::Result<T, anyhow::Error>;
