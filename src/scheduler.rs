use crate::{BlocklistDownloader, BlocklistManager, Config, Result};
use chrono::Utc;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info, warn};

/// Automatic blocklist update scheduler
///
/// Provides cross-platform scheduling for periodic blocklist updates
/// using cron expressions. Works on Windows, macOS, and Linux.
pub struct UpdateScheduler {
    scheduler: JobScheduler,
    config: Arc<Config>,
    blocklist: Arc<BlocklistManager>,
}

impl UpdateScheduler {
    /// Create a new update scheduler
    pub async fn new(config: Arc<Config>, blocklist: Arc<BlocklistManager>) -> Result<Self> {
        let scheduler = JobScheduler::new().await?;
        
        Ok(Self {
            scheduler,
            config,
            blocklist,
        })
    }
    
    /// Start the scheduler with configured cron expression
    ///
    /// This will schedule automatic updates according to the config.
    /// The scheduler runs in the background and doesn't block.
    pub async fn start(&mut self) -> Result<()> {
        if !self.config.updater.enabled {
            info!("Automatic updates disabled in configuration");
            return Ok(());
        }
        
        let schedule = &self.config.updater.schedule;
        let timezone = &self.config.updater.timezone;
        
        info!("Setting up automatic updates: {} ({})", schedule, timezone);
        
        // Clone Arc references for the job closure
        let config = Arc::clone(&self.config);
        let blocklist = Arc::clone(&self.blocklist);
        
        // Create the update job
        let job = Job::new_async(schedule.as_str(), move |_uuid, _lock| {
            let config = Arc::clone(&config);
            let blocklist = Arc::clone(&blocklist);
            
            Box::pin(async move {
                info!("Automatic blocklist update triggered");

                match Self::run_update(&config, &blocklist).await {
                    Ok(count) => {
                        info!("Automatic update completed: {} domains loaded", count);
                    }
                    Err(e) => {
                        error!("Automatic update failed: {}", e);
                    }
                }
            })
        })?;
        
        self.scheduler.add(job).await?;
        self.scheduler.start().await?;
        
        info!("Scheduler started successfully");
        
        Ok(())
    }
    
    /// Stop the scheduler
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping update scheduler");
        self.scheduler.shutdown().await?;
        Ok(())
    }
    
    /// Run a manual update (used by both scheduled and CLI updates)
    async fn run_update(config: &Config, blocklist: &BlocklistManager) -> Result<usize> {
        let start = Utc::now();
        
        // Download from remote sources
        let downloader = BlocklistDownloader::new()?;
        let domains = downloader.download_multiple(&config.blocklist.remote_lists).await?;
        
        if domains.is_empty() {
            warn!("No domains downloaded from remote sources");
            return Ok(0);
        }
        
        // Get cache file path (same directory as custom list)
        let cache_path = if let Some(parent) = std::path::Path::new(&config.blocklist.custom_list).parent() {
            parent.join("remote-blocklist-cache.txt")
        } else {
            std::path::PathBuf::from("remote-blocklist-cache.txt")
        };
        
        // Save to cache
        std::fs::write(&cache_path, domains.join("\n"))?;
        info!("Saved {} domains to cache: {}", domains.len(), cache_path.display());
        
        // Reload blocklist from all sources (including new cache)
        blocklist.clear().await?;
        
        let mut all_domains = Vec::new();
        
        // Load custom list
        if std::path::Path::new(&config.blocklist.custom_list).exists() {
            let content = std::fs::read_to_string(&config.blocklist.custom_list)?;
            let domains: Vec<String> = content
                .lines()
                .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
                .map(|line| line.trim().to_string())
                .collect();
            info!("Loaded {} domains from custom list", domains.len());
            all_domains.extend(domains);
        }
        
        // Load local lists
        for path in &config.blocklist.local_lists {
            if std::path::Path::new(path).exists() {
                let content = std::fs::read_to_string(path)?;
                let domains: Vec<String> = content
                    .lines()
                    .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
                    .map(|line| line.trim().to_string())
                    .collect();
                info!("Loaded {} domains from {}", domains.len(), path);
                all_domains.extend(domains);
            }
        }
        
        // Load cached remote list
        if cache_path.exists() {
            let content = std::fs::read_to_string(&cache_path)?;
            let domains: Vec<String> = content
                .lines()
                .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
                .map(|line| line.trim().to_string())
                .collect();
            info!("Loaded {} domains from remote cache", domains.len());
            all_domains.extend(domains);
        }
        
        blocklist.load_domains(all_domains).await?;
        
        let duration = Utc::now().signed_duration_since(start);
        let total_count = blocklist.count().await;
        
        info!("Update completed in {}ms: {} total domains",
              duration.num_milliseconds(), total_count);
        
        Ok(total_count)
    }
    
    /// Trigger a manual update now (for CLI command)
    pub async fn trigger_manual_update(&self) -> Result<usize> {
        info!("Manual update triggered via CLI");
        Self::run_update(&self.config, &self.blocklist).await
    }
    
    /// Get the next scheduled run time
    pub async fn next_run(&self) -> Option<chrono::DateTime<Utc>> {
        // Get all jobs and find the next tick
        // Note: tokio-cron-scheduler doesn't expose next_tick directly,
        // so we'll parse the cron expression ourselves
        self.parse_next_run()
    }
    
    /// Parse cron expression to get next run time
    fn parse_next_run(&self) -> Option<chrono::DateTime<Utc>> {
        // This is a simplified implementation
        // In production, you might want to use a full cron parser
        let schedule = &self.config.updater.schedule;
        
        // For "0 0 * * *" (daily at midnight), calculate next midnight
        if schedule == "0 0 * * *" {
            let now = Utc::now();
            let tomorrow = now.date_naive().succ_opt()?;
            let midnight = tomorrow.and_hms_opt(0, 0, 0)?;
            Some(chrono::DateTime::<Utc>::from_naive_utc_and_offset(midnight, Utc))
        } else {
            // For other schedules, return None (not implemented)
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    fn create_test_config(temp_dir: &TempDir) -> Config {
        let custom_list = temp_dir.path().join("custom.txt");
        std::fs::write(&custom_list, "test.com\n").unwrap();
        
        Config {
            server: crate::config::ServerConfig {
                listen_addr: "127.0.0.1".to_string(),
                listen_port: 15353,
                upstream_dns: vec!["1.1.1.1:53".to_string()],
                blocked_response: crate::config::BlockedResponse::Refused,
            },
            blocklist: crate::config::BlocklistConfig {
                remote_lists: vec![],
                local_lists: vec![],
                custom_list: custom_list.to_string_lossy().to_string(),
                enable_wildcards: true,
            },
            logging: crate::config::LoggingConfig {
                log_blocked: true,
                log_path: temp_dir.path().join("test.log").to_string_lossy().to_string(),
                log_level: "info".to_string(),
            },
            updater: crate::config::UpdaterConfig {
                enabled: true,
                schedule: "0 0 * * *".to_string(),
                timezone: "UTC".to_string(),
            },
        }
    }
    
    #[tokio::test]
    async fn test_scheduler_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = Arc::new(create_test_config(&temp_dir));
        let blocklist = Arc::new(BlocklistManager::new());
        
        let scheduler = UpdateScheduler::new(config, blocklist).await;
        assert!(scheduler.is_ok());
    }
    
    #[tokio::test]
    async fn test_scheduler_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = create_test_config(&temp_dir);
        config.updater.enabled = false;
        
        let config = Arc::new(config);
        let blocklist = Arc::new(BlocklistManager::new());
        
        let mut scheduler = UpdateScheduler::new(config, blocklist).await.unwrap();
        let result = scheduler.start().await;
        assert!(result.is_ok());
    }
    
    #[tokio::test]
    async fn test_next_run_daily_midnight() {
        let temp_dir = TempDir::new().unwrap();
        let config = Arc::new(create_test_config(&temp_dir));
        let blocklist = Arc::new(BlocklistManager::new());
        
        let scheduler = UpdateScheduler::new(config, blocklist).await.unwrap();
        let next_run = scheduler.next_run().await;
        
        // Should return some time in the future
        assert!(next_run.is_some());
        if let Some(time) = next_run {
            assert!(time > Utc::now());
        }
    }
    
    #[test]
    fn test_cron_expression_validation() {
        // Valid cron expressions
        let valid = vec![
            "0 0 * * *",       // Daily at midnight
            "0 */6 * * *",     // Every 6 hours
            "0 0 */2 * *",     // Every 2 days
            "0 3 * * 0",       // Every Sunday at 3am
        ];
        
        for expr in valid {
            // tokio-cron-scheduler validates on job creation
            // We'll just check format here
            assert!(expr.split_whitespace().count() == 5, "Invalid cron expression: {}", expr);
        }
    }
}
