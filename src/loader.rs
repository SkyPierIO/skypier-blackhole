use crate::{BlocklistManager, Config, Result};
use std::path::{Path, PathBuf};

/// A blocklist source that was inspected on disk
#[derive(Debug, Clone)]
pub struct SourceSummary {
    pub label: &'static str,
    pub path: String,
    /// Number of domain entries, or None if the file is missing
    pub domains: Option<usize>,
}

/// Path of the cache file where downloaded remote lists are stored
/// (same directory as the custom list)
pub fn remote_cache_path(config: &Config) -> PathBuf {
    Path::new(&config.blocklist.custom_list)
        .parent()
        .unwrap_or(Path::new("/tmp"))
        .join("remote-blocklist-cache.txt")
}

fn read_domains(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)?;
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
        .map(|line| line.trim().to_string())
        .collect())
}

/// Enumerate all configured blocklist sources with their entry counts,
/// without touching the blocklist manager.
pub fn summarize_sources(config: &Config) -> Vec<SourceSummary> {
    let mut sources = Vec::new();

    let mut push = |label: &'static str, path: &Path| {
        let domains = if path.exists() {
            read_domains(path).ok().map(|d| d.len())
        } else {
            None
        };
        sources.push(SourceSummary {
            label,
            path: path.display().to_string(),
            domains,
        });
    };

    push("custom", Path::new(&config.blocklist.custom_list));
    for local in &config.blocklist.local_lists {
        push("local", Path::new(local));
    }
    push("remote cache", &remote_cache_path(config));

    sources
}

/// Load all configured blocklist sources into the manager.
///
/// Returns per-source summaries. Does not clear the manager first; call
/// `blocklist.clear()` beforehand for a full reload.
pub async fn load_blocklist(
    config: &Config,
    blocklist: &BlocklistManager,
) -> Result<Vec<SourceSummary>> {
    let sources = summarize_sources(config);
    let mut all_domains = Vec::new();

    for source in &sources {
        let path = Path::new(&source.path);
        match source.domains {
            Some(_) => {
                tracing::info!("Loading blocklist from {}", source.path);
                all_domains.extend(read_domains(path)?);
            }
            None if source.label == "custom" => {
                tracing::warn!("Blocklist file not found: {}", source.path);
            }
            None if source.label == "local" => {
                tracing::warn!("Local blocklist file not found: {}", source.path);
            }
            None => {}
        }
    }

    blocklist.load_domains(all_domains).await?;
    let count = blocklist.count().await;
    tracing::info!("Loaded {} total domains into blocklist", count);

    Ok(sources)
}
