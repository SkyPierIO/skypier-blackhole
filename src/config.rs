use crate::Result;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::str::FromStr;

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
    pub upstream_dns: Vec<Upstream>,

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

/// An upstream resolver, either plain UDP (`1.1.1.1:53`) or DNS over HTTPS
/// (`https://dns.quad9.net/dns-query@9.9.9.9:443`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub enum Upstream {
    /// Plain DNS over UDP
    Udp(SocketAddr),
    /// DNS over HTTPS: socket address to connect to and TLS server name
    DoH { addr: SocketAddr, dns_name: String },
}

/// The only endpoint path supported by hickory 0.24 (hardcoded upstream).
const DOH_QUERY_PATH: &str = "/dns-query";
const DOH_DEFAULT_PORT: u16 = 443;

impl FromStr for Upstream {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let Some(rest) = s.strip_prefix("https://") else {
            let addr = s.parse::<SocketAddr>().with_context(|| {
                format!("Invalid upstream '{s}': expected 'ip:port' or 'https://...'")
            })?;
            return Ok(Upstream::Udp(addr));
        };

        // Grammar: https://<host>[:port][/path][@bootstrap_ip[:port]]
        let (url_part, bootstrap) = match rest.rsplit_once('@') {
            Some((url, bootstrap)) => (url, Some(bootstrap)),
            None => (rest, None),
        };

        let (authority, path) = match url_part.find('/') {
            Some(idx) => (&url_part[..idx], &url_part[idx..]),
            None => (url_part, ""),
        };
        if !path.is_empty() && path != DOH_QUERY_PATH {
            anyhow::bail!(
                "Invalid DoH upstream '{s}': only the '{DOH_QUERY_PATH}' endpoint path is supported"
            );
        }

        let (host, host_port) =
            split_host_port(authority).with_context(|| format!("Invalid DoH upstream '{s}'"))?;

        let addr = match bootstrap {
            Some(bootstrap) => {
                let (ip_str, port) = split_host_port(bootstrap)
                    .with_context(|| format!("Invalid DoH upstream '{s}'"))?;
                let ip = ip_str.parse::<IpAddr>().with_context(|| {
                    format!(
                        "Invalid DoH upstream '{s}': bootstrap '{bootstrap}' is not an IP address"
                    )
                })?;
                SocketAddr::new(ip, port.or(host_port).unwrap_or(DOH_DEFAULT_PORT))
            }
            None => match host.parse::<IpAddr>() {
                Ok(ip) => SocketAddr::new(ip, host_port.unwrap_or(DOH_DEFAULT_PORT)),
                Err(_) => anyhow::bail!(
                    "Invalid DoH upstream '{s}': hostname requires a bootstrap address, \
                     e.g. 'https://{host}{DOH_QUERY_PATH}@9.9.9.9:443'"
                ),
            },
        };

        Ok(Upstream::DoH {
            addr,
            dns_name: host,
        })
    }
}

/// Split `host[:port]`, requiring brackets for IPv6 (`[2620:fe::fe]:443`)
fn split_host_port(s: &str) -> Result<(String, Option<u16>)> {
    if let Some(rest) = s.strip_prefix('[') {
        let (host, after) = rest
            .split_once(']')
            .ok_or_else(|| anyhow::anyhow!("unclosed '[' in '{s}'"))?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(
                p.parse::<u16>()
                    .with_context(|| format!("bad port '{p}'"))?,
            ),
            None if after.is_empty() => None,
            None => anyhow::bail!("unexpected '{after}' after ']' in '{s}'"),
        };
        Ok((host.to_string(), port))
    } else if let Some((host, port)) = s.rsplit_once(':') {
        if host.contains(':') {
            anyhow::bail!("IPv6 addresses must be bracketed, e.g. '[{s}]'");
        }
        let port = port
            .parse::<u16>()
            .with_context(|| format!("bad port '{port}'"))?;
        Ok((host.to_string(), Some(port)))
    } else {
        Ok((s.to_string(), None))
    }
}

impl TryFrom<String> for Upstream {
    type Error = anyhow::Error;

    fn try_from(s: String) -> std::result::Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Upstream> for String {
    fn from(upstream: Upstream) -> Self {
        upstream.to_string()
    }
}

impl fmt::Display for Upstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Upstream::Udp(addr) => write!(f, "{addr}"),
            Upstream::DoH { addr, dns_name } => {
                let host_is_addr_ip = dns_name
                    .parse::<IpAddr>()
                    .map(|ip| ip == addr.ip())
                    .unwrap_or(false);
                if host_is_addr_ip {
                    let host = if addr.is_ipv6() {
                        format!("[{dns_name}]")
                    } else {
                        dns_name.clone()
                    };
                    if addr.port() == DOH_DEFAULT_PORT {
                        write!(f, "https://{host}{DOH_QUERY_PATH}")
                    } else {
                        write!(f, "https://{host}:{}{DOH_QUERY_PATH}", addr.port())
                    }
                } else {
                    write!(f, "https://{dns_name}{DOH_QUERY_PATH}@{addr}")
                }
            }
        }
    }
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

    /// Refresh remote blocklists once at daemon startup (in the background)
    #[serde(default = "default_true")]
    pub update_on_start: bool,
}

// Default value functions
fn default_listen_addr() -> String {
    "127.0.0.1".to_string()
}

fn default_listen_port() -> u16 {
    53
}

fn default_upstream_dns() -> Vec<Upstream> {
    vec!["1.1.1.1:53".parse().expect("valid default upstream")]
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
    format!(
        "{}\\Skypier\\custom-blocklist.txt",
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string())
    )
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
    format!(
        "{}\\Skypier\\Logs\\blackhole.log",
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string())
    )
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
    format!(
        "{}\\Skypier\\blackhole.toml",
        std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string())
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn get_default_config_path() -> String {
    "blackhole.toml".to_string()
}

impl Config {
    /// Load configuration from file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        Ok(())
    }

    /// Load configuration from `path`, offering to create a default config
    /// there on first launch.
    ///
    /// If the file is missing and stdin is an interactive terminal, prompts
    /// the user to write out `Config::default()` at `path` (creating parent
    /// directories as needed) before loading. Non-interactive sessions and
    /// declined prompts fall through to the plain `load` error.
    pub fn load_or_prompt_default<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() && io::stdin().is_terminal() {
            print!(
                "No config file found at {}. Create a default one there now? [Y/n] ",
                path.display()
            );
            io::stdout().flush().ok();

            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            let answer = answer.trim().to_lowercase();

            if answer.is_empty() || answer == "y" || answer == "yes" {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create config directory: {}", parent.display())
                    })?;
                }
                Config::default().save(path)?;
                println!("Wrote default configuration to {}", path.display());
            }
        }

        Self::load(path)
    }
}

impl Default for Config {
    /// Create default configuration
    fn default() -> Self {
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
                update_on_start: true,
            },
        }
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
        assert_eq!(
            config.server.upstream_dns,
            vec![Upstream::Udp("1.1.1.1:53".parse().unwrap())]
        );
    }

    #[test]
    fn test_upstream_parse_udp() {
        let upstream: Upstream = "9.9.9.9:53".parse().unwrap();
        assert_eq!(upstream, Upstream::Udp("9.9.9.9:53".parse().unwrap()));
    }

    #[test]
    fn test_upstream_parse_doh_with_bootstrap() {
        let upstream: Upstream = "https://dns.quad9.net/dns-query@9.9.9.9:443"
            .parse()
            .unwrap();
        assert_eq!(
            upstream,
            Upstream::DoH {
                addr: "9.9.9.9:443".parse().unwrap(),
                dns_name: "dns.quad9.net".to_string(),
            }
        );
    }

    #[test]
    fn test_upstream_parse_doh_bootstrap_default_port() {
        let upstream: Upstream = "https://dns.quad9.net/dns-query@9.9.9.9".parse().unwrap();
        assert_eq!(
            upstream,
            Upstream::DoH {
                addr: "9.9.9.9:443".parse().unwrap(),
                dns_name: "dns.quad9.net".to_string(),
            }
        );
    }

    #[test]
    fn test_upstream_parse_doh_ip_host() {
        let upstream: Upstream = "https://1.1.1.1/dns-query".parse().unwrap();
        assert_eq!(
            upstream,
            Upstream::DoH {
                addr: "1.1.1.1:443".parse().unwrap(),
                dns_name: "1.1.1.1".to_string(),
            }
        );
    }

    #[test]
    fn test_upstream_parse_doh_ipv6_host() {
        let upstream: Upstream = "https://[2620:fe::fe]/dns-query".parse().unwrap();
        assert_eq!(
            upstream,
            Upstream::DoH {
                addr: "[2620:fe::fe]:443".parse().unwrap(),
                dns_name: "2620:fe::fe".to_string(),
            }
        );
    }

    #[test]
    fn test_upstream_parse_doh_no_path() {
        let upstream: Upstream = "https://1.1.1.1".parse().unwrap();
        assert_eq!(
            upstream,
            Upstream::DoH {
                addr: "1.1.1.1:443".parse().unwrap(),
                dns_name: "1.1.1.1".to_string(),
            }
        );
    }

    #[test]
    fn test_upstream_rejects_custom_path() {
        let err = "https://dns.quad9.net/other-path@9.9.9.9"
            .parse::<Upstream>()
            .unwrap_err();
        assert!(err.to_string().contains("/dns-query"));
    }

    #[test]
    fn test_upstream_rejects_hostname_without_bootstrap() {
        let err = "https://dns.quad9.net/dns-query"
            .parse::<Upstream>()
            .unwrap_err();
        assert!(err.to_string().contains("bootstrap"));
    }

    #[test]
    fn test_upstream_rejects_garbage() {
        assert!("not-an-address".parse::<Upstream>().is_err());
        assert!("1.1.1.1".parse::<Upstream>().is_err()); // missing port for UDP
    }

    #[test]
    fn test_upstream_toml_round_trip() {
        let toml_str = r#"
            listen_addr = "127.0.0.1"
            upstream_dns = [
                "1.1.1.1:53",
                "https://dns.quad9.net/dns-query@9.9.9.9:443",
                "https://1.1.1.1/dns-query",
            ]
        "#;
        let server: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(server.upstream_dns.len(), 3);

        let serialized = toml::to_string(&server).unwrap();
        let reparsed: ServerConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.upstream_dns, server.upstream_dns);
        assert!(serialized.contains("https://dns.quad9.net/dns-query@9.9.9.9:443"));
        assert!(serialized.contains("https://1.1.1.1/dns-query"));
    }
}
