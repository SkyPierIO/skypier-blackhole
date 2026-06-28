use radix_trie::Trie;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::Result;

pub struct BlocklistManager {
    // Domain blocklist using radix trie for efficient lookups
    domains: Arc<RwLock<Trie<String, ()>>>,
    
    // Exact domain matches
    exact_matches: Arc<RwLock<HashSet<String>>>,
    
    // Wildcard domains (e.g., *.example.com)
    // Stored as reversed domains for efficient subdomain matching
    // *.example.com -> com.example
    wildcards: Arc<RwLock<HashSet<String>>>,
}

impl BlocklistManager {
    pub fn new() -> Self {
        BlocklistManager {
            domains: Arc::new(RwLock::new(Trie::new())),
            exact_matches: Arc::new(RwLock::new(HashSet::new())),
            wildcards: Arc::new(RwLock::new(HashSet::new())),
        }
    }
    
    /// Parse a domain entry and determine if it's a wildcard
    /// Returns (is_wildcard, normalized_domain)
    fn parse_domain(domain: &str) -> (bool, String) {
        let normalized = domain.trim().to_lowercase();
        
        if normalized.starts_with("*.") {
            // Wildcard domain: *.example.com -> example.com (reversed later)
            let base = normalized.trim_start_matches("*.").to_string();
            (true, base)
        } else {
            (false, normalized)
        }
    }
    
    /// Check if a domain matches any wildcard patterns
    fn matches_wildcard(domain: &str, wildcards: &HashSet<String>) -> bool {
        // For domain "sub.example.com", check if any wildcard base matches
        // Need to check: example.com, sub.example.com against wildcard bases
        
        let parts: Vec<&str> = domain.split('.').collect();
        
        // Try each suffix of the domain
        // For "a.b.example.com", try: example.com, b.example.com, a.b.example.com
        for i in 0..parts.len() {
            let suffix = parts[i..].join(".");
            if wildcards.contains(&suffix) {
                // Found a matching wildcard base
                // Only match if this is actually a subdomain (not the exact domain)
                if i > 0 {
                    return true;
                }
            }
        }
        
        false
    }
    
    /// Check if a domain is blocked
    pub async fn is_blocked(&self, domain: &str) -> bool {
        // Normalize domain: remove trailing dot if present
        let normalized = domain.trim_end_matches('.').to_lowercase();
        
        // Check exact match first (fastest)
        let exact = self.exact_matches.read().await;
        if exact.contains(&normalized) {
            return true;
        }
        
        // Check wildcard matches
        let wildcards = self.wildcards.read().await;
        if Self::matches_wildcard(&normalized, &wildcards) {
            return true;
        }
        
        // Check in trie (for future use or backward compatibility)
        let trie = self.domains.read().await;
        trie.get(&normalized).is_some()
    }
    
    /// Add a domain to the blocklist
    /// Supports both exact domains and wildcards (*.example.com)
    pub async fn add_domain(&self, domain: String) -> Result<()> {
        let (is_wildcard, normalized) = Self::parse_domain(&domain);
        
        if is_wildcard {
            // Add to wildcard collection
            let mut wildcards = self.wildcards.write().await;
            wildcards.insert(normalized);
        } else {
            // Add to exact matches
            let mut exact = self.exact_matches.write().await;
            exact.insert(normalized.clone());
            
            let mut trie = self.domains.write().await;
            trie.insert(normalized, ());
        }
        
        Ok(())
    }
    
    /// Remove a domain from the blocklist
    pub async fn remove_domain(&self, domain: &str) -> Result<()> {
        let (is_wildcard, normalized) = Self::parse_domain(domain);
        
        if is_wildcard {
            let mut wildcards = self.wildcards.write().await;
            wildcards.remove(&normalized);
        } else {
            let mut exact = self.exact_matches.write().await;
            exact.remove(&normalized);
            
            let mut trie = self.domains.write().await;
            trie.remove(&normalized);
        }
        
        Ok(())
    }
    
    /// Load domains from a list
    /// Supports both exact domains and wildcards (*.example.com)
    pub async fn load_domains(&self, domains: Vec<String>) -> Result<()> {
        let mut exact = self.exact_matches.write().await;
        let mut trie = self.domains.write().await;
        let mut wildcards = self.wildcards.write().await;
        
        for domain in domains {
            let (is_wildcard, normalized) = Self::parse_domain(&domain);
            
            if is_wildcard {
                wildcards.insert(normalized);
            } else {
                exact.insert(normalized.clone());
                trie.insert(normalized, ());
            }
        }
        
        Ok(())
    }
    
    /// Get the number of blocked domains (exact + wildcards)
    pub async fn count(&self) -> usize {
        let exact = self.exact_matches.read().await;
        let wildcards = self.wildcards.read().await;
        exact.len() + wildcards.len()
    }
    
    /// Clear all domains from the blocklist
    pub async fn clear(&self) -> Result<()> {
        let mut exact = self.exact_matches.write().await;
        let mut trie = self.domains.write().await;
        let mut wildcards = self.wildcards.write().await;
        
        exact.clear();
        wildcards.clear();
        *trie = Trie::new();
        
        Ok(())
    }
    
    /// Reload blocklist (clear and load new domains)
    pub async fn reload(&self, domains: Vec<String>) -> Result<()> {
        self.clear().await?;
        self.load_domains(domains).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_blocklist() {
        let manager = BlocklistManager::new();
        
        manager.add_domain("example.com".to_string()).await.unwrap();
        assert!(manager.is_blocked("example.com").await);
        assert!(!manager.is_blocked("other.com").await);
        
        manager.remove_domain("example.com").await.unwrap();
        assert!(!manager.is_blocked("example.com").await);
    }
    
    #[tokio::test]
    async fn test_reload() {
        let manager = BlocklistManager::new();
        
        manager.load_domains(vec!["domain1.com".to_string()]).await.unwrap();
        assert_eq!(manager.count().await, 1);
        
        manager.reload(vec!["domain2.com".to_string(), "domain3.com".to_string()]).await.unwrap();
        assert_eq!(manager.count().await, 2);
        assert!(!manager.is_blocked("domain1.com").await);
        assert!(manager.is_blocked("domain2.com").await);
    }
    
    #[tokio::test]
    async fn test_wildcard_basic() {
        let manager = BlocklistManager::new();
        
        // Add wildcard *.example.com
        manager.add_domain("*.example.com".to_string()).await.unwrap();
        
        // Should block subdomains
        assert!(manager.is_blocked("sub.example.com").await);
        assert!(manager.is_blocked("deep.sub.example.com").await);
        assert!(manager.is_blocked("ads.example.com").await);
        
        // Should NOT block the base domain itself
        assert!(!manager.is_blocked("example.com").await);
        
        // Should NOT block unrelated domains
        assert!(!manager.is_blocked("other.com").await);
        assert!(!manager.is_blocked("example.org").await);
    }
    
    #[tokio::test]
    async fn test_wildcard_multi_level() {
        let manager = BlocklistManager::new();
        
        // Add wildcard *.ads.example.com
        manager.add_domain("*.ads.example.com".to_string()).await.unwrap();
        
        // Should block subdomains of ads.example.com
        assert!(manager.is_blocked("tracker.ads.example.com").await);
        assert!(manager.is_blocked("banner.ads.example.com").await);
        
        // Should NOT block ads.example.com itself
        assert!(!manager.is_blocked("ads.example.com").await);
        
        // Should NOT block parent or sibling domains
        assert!(!manager.is_blocked("example.com").await);
        assert!(!manager.is_blocked("cdn.example.com").await);
    }
    
    #[tokio::test]
    async fn test_wildcard_and_exact() {
        let manager = BlocklistManager::new();
        
        // Add both wildcard and exact matches
        manager.add_domain("*.ads.com".to_string()).await.unwrap();
        manager.add_domain("exact.com".to_string()).await.unwrap();
        
        // Wildcard should work
        assert!(manager.is_blocked("tracker.ads.com").await);
        assert!(!manager.is_blocked("ads.com").await);
        
        // Exact should work
        assert!(manager.is_blocked("exact.com").await);
        assert!(!manager.is_blocked("sub.exact.com").await);
        
        // Count should include both
        assert_eq!(manager.count().await, 2);
    }
    
    #[tokio::test]
    async fn test_wildcard_removal() {
        let manager = BlocklistManager::new();
        
        manager.add_domain("*.example.com".to_string()).await.unwrap();
        assert!(manager.is_blocked("sub.example.com").await);
        
        manager.remove_domain("*.example.com").await.unwrap();
        assert!(!manager.is_blocked("sub.example.com").await);
    }
    
    #[tokio::test]
    async fn test_wildcard_case_insensitive() {
        let manager = BlocklistManager::new();
        
        manager.add_domain("*.Example.COM".to_string()).await.unwrap();
        
        // Should match regardless of case
        assert!(manager.is_blocked("sub.example.com").await);
        assert!(manager.is_blocked("SUB.EXAMPLE.COM").await);
        assert!(manager.is_blocked("Sub.Example.Com").await);
    }
    
    #[tokio::test]
    async fn test_wildcard_from_file() {
        let manager = BlocklistManager::new();
        
        // Simulate loading from file with mixed content
        let domains = vec![
            "exact1.com".to_string(),
            "*.wildcard.com".to_string(),
            "exact2.com".to_string(),
            "*.ads.example.com".to_string(),
        ];
        
        manager.load_domains(domains).await.unwrap();
        
        // Exact matches
        assert!(manager.is_blocked("exact1.com").await);
        assert!(manager.is_blocked("exact2.com").await);
        
        // Wildcard matches
        assert!(manager.is_blocked("sub.wildcard.com").await);
        assert!(manager.is_blocked("tracker.ads.example.com").await);
        
        // Should not match base domains
        assert!(!manager.is_blocked("wildcard.com").await);
        assert!(!manager.is_blocked("ads.example.com").await);
        
        assert_eq!(manager.count().await, 4);
    }
}
