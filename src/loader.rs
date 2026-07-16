use crate::{BlocklistManager, Config, Result};
use std::path::{Path, PathBuf};

/// Kind of blocklist source
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Custom,
    Local,
    RemoteCache,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Custom => "custom",
            SourceKind::Local => "local",
            SourceKind::RemoteCache => "remote cache",
        }
    }
}

/// A blocklist source that was inspected on disk
#[derive(Debug, Clone)]
pub struct SourceSummary {
    pub kind: SourceKind,
    pub path: PathBuf,
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

/// All configured sources, in load order
fn source_paths(config: &Config) -> Vec<(SourceKind, PathBuf)> {
    let mut paths = vec![(
        SourceKind::Custom,
        PathBuf::from(&config.blocklist.custom_list),
    )];
    for local in &config.blocklist.local_lists {
        paths.push((SourceKind::Local, PathBuf::from(local)));
    }
    paths.push((SourceKind::RemoteCache, remote_cache_path(config)));
    paths
}

fn is_entry(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty() && !line.starts_with('#')
}

fn read_domains(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)?;
    Ok(content
        .lines()
        .filter(|line| is_entry(line))
        .map(|line| line.trim().to_string())
        .collect())
}

/// Count the entries in one source file; None if it is missing or unreadable.
/// For display only — load errors are surfaced by `load_blocklist`.
pub fn count_domains(path: &Path) -> Option<usize> {
    read_domains(path).ok().map(|domains| domains.len())
}

/// Load all configured blocklist sources into the manager, reading each file
/// once, and return per-source summaries.
///
/// A missing file is skipped (with a warning, except for the remote cache);
/// an existing file that cannot be read is an error. Does not clear the
/// manager first; call `blocklist.clear()` beforehand for a full reload.
pub async fn load_blocklist(
    config: &Config,
    blocklist: &BlocklistManager,
) -> Result<Vec<SourceSummary>> {
    let mut sources = Vec::new();
    let mut all_domains = Vec::new();

    for (kind, path) in source_paths(config) {
        let domains = if path.exists() {
            tracing::info!("Loading {} blocklist from {}", kind.label(), path.display());
            let domains = read_domains(&path)?;
            let count = domains.len();
            all_domains.extend(domains);
            Some(count)
        } else {
            if kind != SourceKind::RemoteCache {
                tracing::warn!("{} blocklist not found: {}", kind.label(), path.display());
            }
            None
        };
        sources.push(SourceSummary {
            kind,
            path,
            domains,
        });
    }

    blocklist.load_domains(all_domains).await?;
    let count = blocklist.count().await;
    tracing::info!("Loaded {} total domains into blocklist", count);

    Ok(sources)
}

/// Append a domain to the custom list, creating the file if needed and
/// repairing a missing trailing newline. Returns the new entry count.
pub fn append_custom_domain(config: &Config, domain: &str) -> Result<usize> {
    let path = Path::new(&config.blocklist.custom_list);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(domain);
    content.push('\n');
    std::fs::write(path, &content)?;
    Ok(content.lines().filter(|line| is_entry(line)).count())
}

/// Remove a domain from the custom list. Returns the new entry count, or
/// None if the domain was not present.
pub fn remove_custom_domain(config: &Config, domain: &str) -> Result<Option<usize>> {
    let path = &config.blocklist.custom_list;
    let content = std::fs::read_to_string(path)?;
    let kept: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != domain)
        .collect();
    if kept.len() == content.lines().filter(|l| !l.trim().is_empty()).count() {
        return Ok(None);
    }
    let content = kept.join("\n") + "\n";
    std::fs::write(path, &content)?;
    Ok(Some(content.lines().filter(|line| is_entry(line)).count()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_for(dir: &Path) -> Config {
        let mut config = Config::default();
        config.blocklist.custom_list = dir.join("custom.txt").display().to_string();
        config
    }

    #[test]
    fn append_repairs_missing_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let config = config_for(dir.path());
        std::fs::write(&config.blocklist.custom_list, "foo.com").unwrap();

        let count = append_custom_domain(&config, "bar.com").unwrap();

        assert_eq!(count, 2);
        let content = std::fs::read_to_string(&config.blocklist.custom_list).unwrap();
        assert_eq!(content, "foo.com\nbar.com\n");
    }

    #[test]
    fn append_creates_file_and_parents() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.blocklist.custom_list = dir.path().join("sub/custom.txt").display().to_string();

        let count = append_custom_domain(&config, "foo.com").unwrap();

        assert_eq!(count, 1);
        let content = std::fs::read_to_string(&config.blocklist.custom_list).unwrap();
        assert_eq!(content, "foo.com\n");
    }

    #[test]
    fn remove_reports_missing_domain() {
        let dir = tempfile::tempdir().unwrap();
        let config = config_for(dir.path());
        std::fs::write(&config.blocklist.custom_list, "foo.com\n\nbar.com\n").unwrap();

        assert_eq!(remove_custom_domain(&config, "baz.com").unwrap(), None);
        assert_eq!(remove_custom_domain(&config, "bar.com").unwrap(), Some(1));
        let content = std::fs::read_to_string(&config.blocklist.custom_list).unwrap();
        assert_eq!(content, "foo.com\n");
    }

    #[tokio::test]
    async fn load_blocklist_errors_on_unreadable_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let config = config_for(dir.path());
        std::fs::write(&config.blocklist.custom_list, "foo.com\n").unwrap();
        std::fs::set_permissions(
            &config.blocklist.custom_list,
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();

        let blocklist = BlocklistManager::new();
        let result = load_blocklist(&config, &blocklist).await;
        assert!(result.is_err(), "unreadable existing file must be an error");
    }
}
