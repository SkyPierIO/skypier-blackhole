use crate::Result;
use reqwest::Client;
use std::time::Duration;

/// Downloader for remote blocklists
pub struct BlocklistDownloader {
    client: Client,
}

impl BlocklistDownloader {
    /// Create a new downloader with default settings
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("Skypier-Blackhole/", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(BlocklistDownloader { client })
    }

    /// Download a blocklist from a URL
    /// Returns a vector of domain strings
    pub async fn download(&self, url: &str) -> Result<Vec<String>> {
        tracing::info!("Downloading blocklist from: {}", url);

        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to download blocklist: HTTP {}", response.status());
        }

        let content = response.text().await?;
        let domains = Self::parse_blocklist(&content);

        tracing::info!("Downloaded {} domains from {}", domains.len(), url);

        Ok(domains)
    }

    /// Parse a blocklist file content
    /// Supports multiple formats:
    /// - Plain domain list (one per line)
    /// - Hosts file format (0.0.0.0 domain.com)
    /// - Hosts file format (127.0.0.1 domain.com)
    /// - Comments starting with #
    fn parse_blocklist(content: &str) -> Vec<String> {
        let mut domains = Vec::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse different formats
            let domain = if line.starts_with("0.0.0.0 ") {
                // Hosts format: 0.0.0.0 domain.com
                line.trim_start_matches("0.0.0.0 ").trim()
            } else if line.starts_with("127.0.0.1 ") {
                // Hosts format: 127.0.0.1 domain.com
                line.trim_start_matches("127.0.0.1 ").trim()
            } else if line.contains(' ') {
                // Generic hosts format: IP domain.com
                // Take the second token (domain)
                match line.split_whitespace().nth(1) {
                    Some(d) => d,
                    None => line,
                }
            } else {
                // Plain domain
                line
            };

            // Validate domain and add
            if !domain.is_empty() && Self::is_valid_domain(domain) {
                domains.push(domain.to_lowercase());
            }
        }

        domains
    }

    /// Basic domain validation
    fn is_valid_domain(domain: &str) -> bool {
        // Skip localhost and special domains
        if domain == "localhost"
            || domain.starts_with("localhost.")
            || domain == "broadcasthost"
            || domain.starts_with("local")
        {
            return false;
        }

        // Skip IP addresses (simple check)
        if domain.starts_with("0.0.0.0") || domain.parse::<std::net::IpAddr>().is_ok() {
            return false;
        }

        // Must contain at least one dot (unless it's a wildcard)
        if !domain.contains('.') && !domain.starts_with("*.") {
            return false;
        }

        // Skip if starts with a digit (likely IP or invalid)
        if domain.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return false;
        }

        // Basic character validation
        domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '*')
    }

    /// Download multiple blocklists and merge them
    pub async fn download_multiple(&self, urls: &[String]) -> Result<Vec<String>> {
        let mut all_domains = Vec::new();

        for url in urls {
            match self.download(url).await {
                Ok(mut domains) => {
                    all_domains.append(&mut domains);
                }
                Err(e) => {
                    tracing::error!("Failed to download from {}: {}", url, e);
                    // Continue with other URLs
                }
            }
        }

        // Deduplicate
        all_domains.sort();
        all_domains.dedup();

        tracing::info!("Total unique domains downloaded: {}", all_domains.len());

        Ok(all_domains)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plain_domains() {
        let content = r#"
ads.example.com
tracker.example.com
malware.example.com
"#;

        let domains = BlocklistDownloader::parse_blocklist(content);
        assert_eq!(domains.len(), 3);
        assert!(domains.contains(&"ads.example.com".to_string()));
    }

    #[test]
    fn test_parse_hosts_format() {
        let content = r#"
# Comment line
0.0.0.0 ads.example.com
127.0.0.1 tracker.example.com
0.0.0.0 malware.example.com
"#;

        let domains = BlocklistDownloader::parse_blocklist(content);
        assert_eq!(domains.len(), 3);
        assert!(domains.contains(&"ads.example.com".to_string()));
        assert!(domains.contains(&"tracker.example.com".to_string()));
    }

    #[test]
    fn test_parse_mixed_format() {
        let content = r#"
# Blocklist with mixed formats
plain.example.com
0.0.0.0 hosts1.example.com
127.0.0.1 hosts2.example.com

# Another section
*.wildcard.example.com
"#;

        let domains = BlocklistDownloader::parse_blocklist(content);
        assert_eq!(domains.len(), 4);
        assert!(domains.contains(&"*.wildcard.example.com".to_string()));
    }

    #[test]
    fn test_skip_localhost() {
        let content = r#"
0.0.0.0 localhost
127.0.0.1 localhost.localdomain
good.example.com
"#;

        let domains = BlocklistDownloader::parse_blocklist(content);
        assert_eq!(domains.len(), 1);
        assert!(domains.contains(&"good.example.com".to_string()));
    }

    #[test]
    fn test_domain_validation() {
        assert!(BlocklistDownloader::is_valid_domain("example.com"));
        assert!(BlocklistDownloader::is_valid_domain("sub.example.com"));
        assert!(BlocklistDownloader::is_valid_domain("*.example.com"));
        assert!(BlocklistDownloader::is_valid_domain(
            "sub-domain.example.com"
        ));

        assert!(!BlocklistDownloader::is_valid_domain("localhost"));
        assert!(!BlocklistDownloader::is_valid_domain("invalid"));
        assert!(!BlocklistDownloader::is_valid_domain(""));
    }
}
