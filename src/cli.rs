use crate::config::Upstream;
use crate::{BlocklistDownloader, BlocklistManager, Config, DnsServer, Result, UpdateScheduler};
use clap::{Parser, Subcommand};
use colored::*;
use futures::stream::StreamExt;
use signal_hook::consts::signal::*;
use signal_hook_tokio::Signals;
use std::fs;
use std::sync::Arc;

// Platform-specific default config path
#[cfg(target_os = "linux")]
const DEFAULT_CONFIG_PATH: &str = "/etc/skypier/blackhole.toml";

#[cfg(target_os = "macos")]
const DEFAULT_CONFIG_PATH: &str = "/usr/local/etc/skypier/blackhole.toml";

#[cfg(target_os = "windows")]
const DEFAULT_CONFIG_PATH: &str = "C:\\ProgramData\\Skypier\\blackhole.toml";

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
const DEFAULT_CONFIG_PATH: &str = "blackhole.toml";

/// ASCII art logo, shared by the console banner and the TUI dashboard
pub(crate) const BANNER: &str = r#"
       ____  __           __    __          __
      / __ )/ /___ ______/ /__ / /_  ____  / /__
     / __  / / __ `/ ___/ //_// __ \/ __ \/ / _ \
 ___/ /_/ / / /_/ / /__/ ,<  / / / / /_/ / /  __/__
/________/_/\__,_/\___/_/|_|/_/ /_/\____/_/\______/

"#;

/// Print the startup ASCII art banner
fn print_banner() {
    println!("{}", BANNER.bright_cyan());
    println!(
        "  {} {}",
        "Skypier Blackhole".bright_white().bold(),
        format!("v{}", env!("CARGO_PKG_VERSION")).bright_black()
    );
    println!(
        "  {}",
        "A fast, blocklist-driven DNS sinkhole".bright_black()
    );
    println!();
}

/// Render the configured upstreams for display; queries are forwarded to a
/// randomly picked one per query rather than always the first in the list
fn format_upstream_list(upstreams: &[Upstream]) -> String {
    if upstreams.is_empty() {
        return "1.1.1.1:53".to_string();
    }
    let list = upstreams
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if upstreams.len() > 1 {
        format!("{list} (random per query)")
    } else {
        list
    }
}

/// Find the PID of the running skypier-blackhole server
fn find_server_pid() -> Result<Option<u32>> {
    let output = std::process::Command::new("pgrep")
        .arg("-f")
        .arg("skypier-blackhole.*start")
        .output()?;

    if output.status.success() && !output.stdout.is_empty() {
        let pid_str = String::from_utf8_lossy(&output.stdout);
        let pids: Vec<&str> = pid_str.trim().lines().collect();

        // Return the first PID (there should only be one server)
        if let Some(pid) = pids.first() {
            if let Ok(pid_num) = pid.parse::<u32>() {
                return Ok(Some(pid_num));
            }
        }
    }

    Ok(None)
}

/// Send a signal to the running server
fn send_signal(pid: u32, signal: i32) -> Result<()> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    let sig = match signal {
        SIGTERM => Signal::SIGTERM,
        SIGHUP => Signal::SIGHUP,
        _ => anyhow::bail!("Unsupported signal"),
    };

    kill(Pid::from_raw(pid as i32), sig)?;

    Ok(())
}

#[derive(Parser)]
#[command(name = "skypier-blackhole")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
    pub config: String,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the DNS server
    Start {
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },

    /// Stop the DNS server
    Stop {
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },

    /// Reload blocklists without restarting
    Reload {
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },

    /// Show server status and statistics
    Status {
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },

    /// Add a domain to the blocklist
    Add {
        /// Domain to add (e.g., ads.example.com or *.tracker.com)
        domain: String,
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },

    /// Remove a domain from the blocklist
    Remove {
        /// Domain to remove
        domain: String,
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },

    /// List blocklist statistics
    List {
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },

    /// Force update blocklists from remote sources
    Update {
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },

    /// Test if a domain is blocked
    Test {
        /// Domain to test
        domain: String,
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },

    /// Start the DNS server with an interactive terminal dashboard
    Tui {
        /// Path to configuration file
        #[arg(short, long, default_value_t = DEFAULT_CONFIG_PATH.to_string())]
        config: String,
    },
}

impl Cli {
    /// Whether the TUI dashboard was requested (it owns the terminal, so the
    /// console logger must not be installed)
    pub fn is_tui(&self) -> bool {
        matches!(self.command, Some(Commands::Tui { .. }))
    }

    pub async fn execute(&self) -> Result<()> {
        match &self.command {
            Some(Commands::Tui {
                config: config_path,
            }) => crate::tui::run(config_path).await,
            Some(Commands::Start {
                config: config_path,
            }) => {
                print_banner();

                let config = Config::load_or_prompt_default(config_path)?;
                tracing::info!("Starting DNS server...");

                // Create blocklist manager
                let blocklist = Arc::new(BlocklistManager::new());

                // Load initial blocklist
                crate::loader::load_blocklist(&config, &blocklist).await?;
                tracing::info!("Blocklist manager initialized");

                // Create and start update scheduler
                let config_arc = Arc::new(config.clone());
                let mut scheduler =
                    UpdateScheduler::new(Arc::clone(&config_arc), Arc::clone(&blocklist)).await?;

                if let Err(e) = scheduler.start().await {
                    tracing::warn!("Failed to start update scheduler: {}", e);
                } else {
                    tracing::info!("Update scheduler started");
                }

                // Create DNS server
                let server = DnsServer::new(config.clone(), Arc::clone(&blocklist))?;

                // Setup signal handling for graceful shutdown and reload
                let mut signals = Signals::new([SIGTERM, SIGINT, SIGHUP])?;
                let signals_handle = signals.handle();

                let config_clone = config.clone();
                let blocklist_clone = Arc::clone(&blocklist);

                // Spawn signal handler task
                let signal_task = tokio::spawn(async move {
                    while let Some(signal) = signals.next().await {
                        match signal {
                            SIGTERM | SIGINT => {
                                tracing::info!("Received shutdown signal, stopping server...");
                                // Server will stop when main task exits
                                break;
                            }
                            SIGHUP => {
                                tracing::info!("Received SIGHUP, reloading blocklists...");
                                match crate::loader::load_blocklist(&config_clone, &blocklist_clone)
                                    .await
                                {
                                    Ok(_) => {
                                        let count = blocklist_clone.count().await;
                                        tracing::info!(
                                            "Blocklist reloaded successfully with {} domains",
                                            count
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to reload blocklist: {}", e);
                                    }
                                }
                            }
                            _ => unreachable!(),
                        }
                    }
                });

                // Start DNS server (blocks until error or signal)
                let server_task = tokio::spawn(async move { server.start().await });

                // Kick off a one-shot remote blocklist refresh in the background
                // so the server is already serving while the download runs.
                scheduler.spawn_startup_refresh();

                // Wait for either server error or signal
                tokio::select! {
                    result = server_task => {
                        match result {
                            Ok(Ok(())) => tracing::info!("DNS server stopped normally"),
                            Ok(Err(e)) => tracing::error!("DNS server error: {}", e),
                            Err(e) => tracing::error!("Server task panicked: {}", e),
                        }
                    }
                    _ = signal_task => {
                        tracing::info!("Signal handler stopped");
                    }
                }

                // Cleanup
                if let Err(e) = scheduler.stop().await {
                    tracing::warn!("Failed to stop scheduler: {}", e);
                }
                signals_handle.close();
                tracing::info!("Server shutdown complete");

                Ok(())
            }
            Some(Commands::Stop {
                config: config_path,
            }) => {
                let _config = Config::load(config_path)?;
                println!(
                    "{}",
                    "Stopping Skypier Blackhole DNS Server"
                        .bright_yellow()
                        .bold()
                );
                println!();

                match find_server_pid()? {
                    Some(pid) => {
                        println!(
                            "  {} Server PID: {}",
                            "[*]".bright_blue(),
                            pid.to_string().bright_cyan()
                        );
                        println!("  {} Sending SIGTERM...", "[*]".bright_yellow());

                        send_signal(pid, SIGTERM)?;

                        // Wait a bit for graceful shutdown
                        std::thread::sleep(std::time::Duration::from_millis(500));

                        // Check if still running
                        match find_server_pid()? {
                            Some(_) => {
                                println!(
                                    "  {} Server is taking longer to stop (this is normal)",
                                    "[..]".bright_yellow()
                                );
                            }
                            None => {
                                println!(
                                    "  {} Server stopped successfully",
                                    "[ok]".bright_green().bold()
                                );
                            }
                        }
                    }
                    None => {
                        println!("  {} No running server found", "[i]".bright_blue());
                    }
                }

                println!();
                Ok(())
            }
            Some(Commands::Reload {
                config: config_path,
            }) => {
                let _config = Config::load(config_path)?;
                println!("{}", "Reloading Blocklists".bright_cyan().bold());
                println!();

                match find_server_pid()? {
                    Some(pid) => {
                        println!(
                            "  {} Server PID: {}",
                            "[*]".bright_blue(),
                            pid.to_string().bright_cyan()
                        );
                        println!("  {} Sending SIGHUP (hot-reload)...", "[*]".bright_yellow());

                        send_signal(pid, SIGHUP)?;

                        std::thread::sleep(std::time::Duration::from_millis(300));

                        println!(
                            "  {} Reload signal sent successfully",
                            "[ok]".bright_green().bold()
                        );
                        println!(
                            "  {} Blocklists are being reloaded with zero downtime",
                            "[ok]".bright_green()
                        );
                    }
                    None => {
                        println!("  {} No running server found", "[x]".bright_red().bold());
                        println!(
                            "  {} Start the server first with: {} skypier-blackhole start",
                            "[i]".bright_blue(),
                            "->".bright_white()
                        );
                    }
                }

                println!();
                Ok(())
            }
            Some(Commands::Status {
                config: config_path,
            }) => {
                let config = Config::load(config_path)?;
                println!("{}", "Skypier Blackhole Status".bright_magenta().bold());
                println!("{}", "=".repeat(50).bright_black());
                println!();

                // Check if server is running
                match find_server_pid()? {
                    Some(pid) => {
                        println!(
                            "  {} Server Status: {}",
                            "[+]".bright_green(),
                            "RUNNING".bright_green().bold()
                        );
                        println!(
                            "  {} Process ID: {}",
                            "[*]".bright_blue(),
                            pid.to_string().bright_cyan()
                        );
                    }
                    None => {
                        println!(
                            "  {} Server Status: {}",
                            "[-]".bright_red(),
                            "STOPPED".bright_red().bold()
                        );
                    }
                }

                println!();

                // Load config and show blocklist stats
                if std::path::Path::new(&config.blocklist.custom_list).exists() {
                    let blocklist = BlocklistManager::new();
                    crate::loader::load_blocklist(&config, &blocklist).await?;
                    let count = blocklist.count().await;

                    println!("  {} Blocklist Statistics:", "[*]".bright_cyan());
                    println!(
                        "    {} Total domains blocked: {}",
                        "-".bright_white(),
                        count.to_string().bright_yellow().bold()
                    );
                    println!(
                        "    {} Custom list: {}",
                        "-".bright_white(),
                        config.blocklist.custom_list.bright_blue()
                    );

                    if !config.blocklist.local_lists.is_empty() {
                        println!(
                            "    {} Local lists: {}",
                            "-".bright_white(),
                            config
                                .blocklist
                                .local_lists
                                .len()
                                .to_string()
                                .bright_yellow()
                        );
                    }
                } else {
                    println!(
                        "  {} No blocklist found at: {}",
                        "[!]".bright_yellow(),
                        config.blocklist.custom_list.bright_blue()
                    );
                }

                println!();
                println!("  {} Configuration:", "[*]".bright_cyan());
                println!(
                    "    {} Listen: {}",
                    "-".bright_white(),
                    format!(
                        "{}:{}",
                        config.server.listen_addr, config.server.listen_port
                    )
                    .bright_green()
                );
                println!(
                    "    {} Upstream DNS: {}",
                    "-".bright_white(),
                    format_upstream_list(&config.server.upstream_dns).bright_green()
                );

                println!();
                println!("{}", "=".repeat(50).bright_black());
                println!();

                Ok(())
            }
            Some(Commands::Add {
                domain,
                config: config_path,
            }) => {
                let config = Config::load(config_path)?;
                println!(
                    "{} {}",
                    "Adding domain:".bright_green().bold(),
                    domain.bright_cyan()
                );
                println!();

                // Add to custom blocklist file
                crate::loader::append_custom_domain(&config, domain)?;

                println!(
                    "  {} Domain added to: {}",
                    "[ok]".bright_green(),
                    config.blocklist.custom_list.bright_blue()
                );

                // Trigger reload if server is running
                match find_server_pid()? {
                    Some(pid) => {
                        println!("  {} Reloading server...", "[*]".bright_cyan());
                        send_signal(pid, SIGHUP)?;
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        println!(
                            "  {} Server reloaded, domain is now blocked",
                            "[ok]".bright_green().bold()
                        );
                    }
                    None => {
                        println!(
                            "  {} Server not running - changes will apply on next start",
                            "[i]".bright_yellow()
                        );
                    }
                }

                println!();
                Ok(())
            }
            Some(Commands::Remove {
                domain,
                config: config_path,
            }) => {
                let config = Config::load(config_path)?;
                println!(
                    "{} {}",
                    "Removing domain:".bright_red().bold(),
                    domain.bright_cyan()
                );
                println!();

                if crate::loader::remove_custom_domain(&config, domain)?.is_none() {
                    println!("  {} Domain not found in blocklist", "[i]".bright_yellow());
                } else {
                    println!(
                        "  {} Domain removed from: {}",
                        "[ok]".bright_green(),
                        config.blocklist.custom_list.bright_blue()
                    );

                    // Trigger reload if server is running
                    match find_server_pid()? {
                        Some(pid) => {
                            println!("  {} Reloading server...", "[*]".bright_cyan());
                            send_signal(pid, SIGHUP)?;
                            std::thread::sleep(std::time::Duration::from_millis(300));
                            println!(
                                "  {} Server reloaded, domain is now allowed",
                                "[ok]".bright_green().bold()
                            );
                        }
                        None => {
                            println!(
                                "  {} Server not running - changes will apply on next start",
                                "[i]".bright_yellow()
                            );
                        }
                    }
                }

                println!();
                Ok(())
            }
            Some(Commands::List {
                config: config_path,
            }) => {
                let config = Config::load(config_path)?;
                println!("{}", "Blocklist Statistics".bright_cyan().bold());
                println!("{}", "=".repeat(50).bright_black());
                println!();

                let blocklist = BlocklistManager::new();
                crate::loader::load_blocklist(&config, &blocklist).await?;
                let total = blocklist.count().await;

                println!(
                    "  {} Total Blocked Domains: {}",
                    "[*]".bright_red(),
                    total.to_string().bright_yellow().bold()
                );
                println!();

                // Count by source
                println!("  {} Sources:", "[*]".bright_cyan());

                if std::path::Path::new(&config.blocklist.custom_list).exists() {
                    let content = fs::read_to_string(&config.blocklist.custom_list)?;
                    let count = content
                        .lines()
                        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
                        .count();
                    println!(
                        "    {} Custom list: {} domains",
                        "-".bright_white(),
                        count.to_string().bright_green()
                    );
                    println!(
                        "      {} {}",
                        ">".bright_black(),
                        config.blocklist.custom_list.bright_blue()
                    );
                }

                for (idx, local_list) in config.blocklist.local_lists.iter().enumerate() {
                    if std::path::Path::new(local_list).exists() {
                        let content = fs::read_to_string(local_list)?;
                        let count = content
                            .lines()
                            .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
                            .count();
                        println!(
                            "    {} Local list {}: {} domains",
                            "-".bright_white(),
                            idx + 1,
                            count.to_string().bright_green()
                        );
                        println!("      {} {}", ">".bright_black(), local_list.bright_blue());
                    }
                }

                if !config.blocklist.remote_lists.is_empty() {
                    println!();
                    println!(
                        "  {} Remote Sources (not yet downloaded):",
                        "[*]".bright_cyan()
                    );
                    for url in &config.blocklist.remote_lists {
                        println!("    {} {}", "-".bright_white(), url.bright_blue());
                    }
                }

                println!();
                println!("{}", "=".repeat(50).bright_black());
                println!();

                Ok(())
            }
            Some(Commands::Update {
                config: config_path,
            }) => {
                let config = Config::load(config_path)?;
                println!("{}", "Updating Blocklists".bright_cyan().bold());
                println!();

                if config.blocklist.remote_lists.is_empty() {
                    println!("  {} No remote sources configured", "[!]".bright_yellow());
                    println!();
                    println!(
                        "  {} Add remote sources to your config:",
                        "[i]".bright_yellow()
                    );
                    println!("    {}", "[blocklist]".bright_blue());
                    println!("    {}", "remote_lists = [".bright_blue());
                    println!(
                        "    {}",
                        "  \"https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts\""
                            .bright_green()
                    );
                    println!("    {}", "]".bright_blue());
                    println!();
                    return Ok(());
                }

                println!("  {} Remote sources:", "[*]".bright_cyan());
                for url in &config.blocklist.remote_lists {
                    println!("    {} {}", "-".bright_white(), url.bright_blue());
                }
                println!();

                // Download blocklists
                println!("  {} Downloading blocklists...", "[*]".bright_yellow());
                let downloader = BlocklistDownloader::new()?;

                match downloader
                    .download_multiple(&config.blocklist.remote_lists)
                    .await
                {
                    Ok(domains) => {
                        println!(
                            "  {} Downloaded {} unique domains",
                            "[ok]".bright_green(),
                            domains.len().to_string().bright_yellow().bold()
                        );

                        // Save to a cache file
                        let cache_dir = std::path::Path::new(&config.blocklist.custom_list)
                            .parent()
                            .unwrap_or(std::path::Path::new("/tmp"));
                        let cache_file = cache_dir.join("remote-blocklist-cache.txt");

                        println!(
                            "  {} Saving to cache: {}",
                            "[*]".bright_cyan(),
                            cache_file.display().to_string().bright_blue()
                        );

                        let content = domains.join("\n") + "\n";
                        std::fs::write(&cache_file, content)?;

                        println!("  {} Cache saved successfully", "[ok]".bright_green());

                        // Trigger reload if server is running
                        match find_server_pid()? {
                            Some(pid) => {
                                println!();
                                println!(
                                    "  {} Reloading server with new blocklists...",
                                    "[*]".bright_cyan()
                                );
                                send_signal(pid, SIGHUP)?;
                                std::thread::sleep(std::time::Duration::from_millis(500));
                                println!(
                                    "  {} Server reloaded successfully",
                                    "[ok]".bright_green().bold()
                                );
                                println!(
                                    "  {} {} domains now active",
                                    "[*]".bright_red(),
                                    domains.len().to_string().bright_yellow().bold()
                                );
                            }
                            None => {
                                println!();
                                println!("  {} Server not running", "[i]".bright_yellow());
                                println!(
                                    "  {} Start the server to apply new blocklists:",
                                    "->".bright_white()
                                );
                                println!("    {}", "skypier-blackhole start".bright_green());
                            }
                        }
                    }
                    Err(e) => {
                        println!(
                            "  {} Failed to download blocklists: {}",
                            "[x]".bright_red(),
                            e
                        );
                        println!(
                            "  {} Check your internet connection and URLs",
                            "[i]".bright_yellow()
                        );
                    }
                }

                println!();
                Ok(())
            }
            Some(Commands::Test {
                domain,
                config: config_path,
            }) => {
                let config = Config::load(config_path)?;
                println!(
                    "{} {}",
                    "Testing domain:".bright_cyan().bold(),
                    domain.bright_yellow()
                );
                println!();

                let blocklist = BlocklistManager::new();
                crate::loader::load_blocklist(&config, &blocklist).await?;

                let is_blocked = blocklist.is_blocked(domain).await;

                if is_blocked {
                    println!(
                        "  {} Status: {}",
                        "[x]".bright_red(),
                        "BLOCKED".bright_red().bold()
                    );
                    println!(
                        "  {} This domain will be blocked by the DNS server",
                        "[i]".bright_blue()
                    );
                    println!(
                        "  {} DNS queries will receive: {}",
                        "->".bright_white(),
                        "REFUSED".bright_yellow()
                    );
                } else {
                    println!(
                        "  {} Status: {}",
                        "[ok]".bright_green(),
                        "ALLOWED".bright_green().bold()
                    );
                    println!(
                        "  {} This domain will be resolved normally",
                        "[i]".bright_blue()
                    );
                    println!(
                        "  {} DNS queries will be forwarded to upstream: {}",
                        "->".bright_white(),
                        format_upstream_list(&config.server.upstream_dns).bright_cyan()
                    );
                }

                println!();
                Ok(())
            }
            None => {
                // Default action: show banner and help
                print_banner();
                println!(
                    "{}",
                    "No command specified. Use --help to see available commands.".bright_yellow()
                );
                println!();
                println!("{}", "Quick start:".bright_cyan().bold());
                println!("  {} Start the DNS server:", "-".bright_white());
                println!("    {}", "skypier-blackhole start".bright_green());
                println!("  {} Check server status:", "-".bright_white());
                println!("    {}", "skypier-blackhole status".bright_green());
                println!("  {} Test a domain:", "-".bright_white());
                println!(
                    "    {}",
                    "skypier-blackhole test ads.example.com".bright_green()
                );
                println!();

                Ok(())
            }
        }
    }
}
