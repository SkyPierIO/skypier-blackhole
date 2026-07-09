use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// In-memory runtime metrics for the DNS daemon.
///
/// Everything lives in RAM and is lost on restart; this exists to feed
/// live dashboards (the TUI), not long-term reporting.
#[derive(Debug)]
pub struct RuntimeMetrics {
    start_time: Instant,
    total_queries: AtomicU64,
    blocked_queries: AtomicU64,
    allowed_queries: AtomicU64,
    /// Per-domain hit counts for blocked queries since startup
    domain_hits: Mutex<HashMap<String, u64>>,
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeMetrics {
    pub fn new() -> Self {
        RuntimeMetrics {
            start_time: Instant::now(),
            total_queries: AtomicU64::new(0),
            blocked_queries: AtomicU64::new(0),
            allowed_queries: AtomicU64::new(0),
            domain_hits: Mutex::new(HashMap::new()),
        }
    }

    pub fn record_allowed(&self) {
        self.total_queries.fetch_add(1, Ordering::Relaxed);
        self.allowed_queries.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_blocked(&self, domain: &str) {
        self.total_queries.fetch_add(1, Ordering::Relaxed);
        self.blocked_queries.fetch_add(1, Ordering::Relaxed);

        let normalized = domain.trim_end_matches('.').to_lowercase();
        let mut hits = self.domain_hits.lock().unwrap();
        *hits.entry(normalized).or_insert(0) += 1;
    }

    pub fn uptime(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    pub fn total_queries(&self) -> u64 {
        self.total_queries.load(Ordering::Relaxed)
    }

    pub fn blocked_queries(&self) -> u64 {
        self.blocked_queries.load(Ordering::Relaxed)
    }

    pub fn allowed_queries(&self) -> u64 {
        self.allowed_queries.load(Ordering::Relaxed)
    }

    /// Number of distinct domains blocked since startup
    pub fn distinct_blocked(&self) -> usize {
        self.domain_hits.lock().unwrap().len()
    }

    /// Top `n` blocked domains by hit count, descending
    pub fn top_blocked(&self, n: usize) -> Vec<(String, u64)> {
        let hits = self.domain_hits.lock().unwrap();
        let mut entries: Vec<(String, u64)> = hits.iter().map(|(d, c)| (d.clone(), *c)).collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        entries.truncate(n);
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counters() {
        let m = RuntimeMetrics::new();
        m.record_allowed();
        m.record_blocked("ads.example.com.");
        m.record_blocked("ads.example.com");
        m.record_blocked("tracker.com");

        assert_eq!(m.total_queries(), 4);
        assert_eq!(m.allowed_queries(), 1);
        assert_eq!(m.blocked_queries(), 3);
        assert_eq!(m.distinct_blocked(), 2);

        let top = m.top_blocked(10);
        assert_eq!(top[0], ("ads.example.com".to_string(), 2));
        assert_eq!(top[1], ("tracker.com".to_string(), 1));
    }
}
