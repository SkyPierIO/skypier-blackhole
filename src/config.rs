use serde::{Deserialize, Serialize};
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use crate::Result;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    pub blocklist: BlocklistConfig,
    pub logging: LoggingConfig,
    pub updater: UpdaterConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    /// Listen address for DNS server
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    
    /// Listen port for DNS server
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    
    /// Upstream DNS servers to forward non-blocked queries
    #[serde(default = "default_upstream_dns")]
    pub upstream_dns: Vec<String>,
    
    /// Response to return for blocked domains
    #[serde(default = "default_blocked_response")]
    pub blocked_response: BlockedResponse,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockedResponse {
    /// Return REFUSED DNS response
    Refused,
    /// Return NXDOMAIN (domain doesn't exist)
    NxDomain,
    /// Return a specific IP address (e.g., 0.0.0.0)
    Ip(IpAddr),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlocklistConfig {
    /// Remote URLs to download blocklists from
    #[serde(default)]
    pub remote_lists: Vec<String>,
    
    /// Local blocklist file paths
    #[serde(default)]
    pub local_lists: Vec<String>,
    
    /// Path to custom blocklist file
    #[serde(default = "default_custom_list")]
    pub custom_list: String,
    
    /// Enable wildcard domain matching
    #[serde(default = "default_true")]
    pub enable_wildcards: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    /// Enable logging of blocked queries
    #[serde(default = "default_true")]
    pub log_blocked: bool,
    
    /// Log file path
    #[serde(default = "default_log_path")]
    pub log_path: String,
    
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdaterConfig {
    /// Enable automatic updates
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Update schedule (cron format)
    #[serde(default = "default_update_schedule")]
    pub schedule: String,
    
    /// Timezone for schedule (e.g., "EST", "UTC")
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

// Default value functions
fn default_listen_addr() -> String {
    "127.0.0.1".to_string()
}

fn default_listen_port() -> u16 {
    53
}

fn default_upstream_dns() -> Vec<String> {
    vec!["1.1.1.1:53".to_string()]
}

fn default_blocked_response() -> BlockedResponse {
    BlockedResponse::Refused
}

fn default_custom_list() -> String {
    get_default_custom_list_path()
}

fn default_true() -> bool {
    true
}

fn default_log_path() -> String {
    get_default_log_path()
}

// Platform-specific default paths

#[cfg(target_os = "linux")]
fn get_default_custom_list_path() -> String {
    "/etc/skypier/custom-blocklist.txt".to_string()
}

#[cfg(target_os = "macos")]
fn get_default_custom_list_path() -> String {
    "/usr/local/etc/skypier/custom-blocklist.txt".to_string()
}

#[cfg(target_os = "windows")]
fn get_default_custom_list_path() -> String {
    format!("{}\\Skypier\\custom-blocklist.txt",
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn get_default_custom_list_path() -> String {
    "custom-blocklist.txt".to_string()
}

#[cfg(target_os = "linux")]
fn get_default_log_path() -> String {
    "/var/log/skypier/blackhole.log".to_string()
}

#[cfg(target_os = "macos")]
fn get_default_log_path() -> String {
    "/usr/local/var/log/skypier/blackhole.log".to_string()
}

#[cfg(target_os = "windows")]
fn get_default_log_path() -> String {
    format!("{}\\Skypier\\Logs\\blackhole.log",
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn get_default_log_path() -> String {
    "blackhole.log".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_update_schedule() -> String {
    "0 0 0 * * *".to_string() // Daily at midnight (sec min hour dom month dow)
}

fn default_timezone() -> String {
    "EST".to_string()
}

/// Get the default configuration file path for the current platform
#[cfg(target_os = "linux")]
pub fn get_default_config_path() -> String {
    "/etc/skypier/blackhole.toml".to_string()
}

#[cfg(target_os = "macos")]
pub fn get_default_config_path() -> String {
    "/usr/local/etc/skypier/blackhole.toml".to_string()
}

#[cfg(target_os = "windows")]
pub fn get_default_config_path() -> String {
    format!("{}\\Skypier\\blackhole.toml",
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn get_default_config_path() -> String {
    "blackhole.toml".to_string()
}

impl Config {
    /// Load configuration from file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
    
    /// Create default configuration
    pub fn default() -> Self {
        Config {
            server: ServerConfig {
                listen_addr: default_listen_addr(),
                listen_port: default_listen_port(),
                upstream_dns: default_upstream_dns(),
                blocked_response: default_blocked_response(),
            },
            blocklist: BlocklistConfig {
                remote_lists: vec![],
                local_lists: vec![],
                custom_list: default_custom_list(),
                enable_wildcards: true,
            },
            logging: LoggingConfig {
                log_blocked: true,
                log_path: default_log_path(),
                log_level: default_log_level(),
            },
            updater: UpdaterConfig {
                enabled: true,
                schedule: default_update_schedule(),
                timezone: default_timezone(),
            },
        }
    }
    
    /// Save configuration to file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.listen_addr, "127.0.0.1");
        assert_eq!(config.server.listen_port, 53);
        assert!(config.blocklist.enable_wildcards);
    }
}
