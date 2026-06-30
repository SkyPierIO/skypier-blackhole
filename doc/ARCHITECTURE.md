# Skypier Blackhole - Architecture Documentation

## Table of Contents
1. [Overview](#overview)
2. [System Architecture](#system-architecture)
3. [Core Components](#core-components)
4. [Data Flow](#data-flow)
5. [Performance Optimization](#performance-optimization)
6. [Security Considerations](#security-considerations)
7. [Deployment Architecture](#deployment-architecture)

## Overview

Skypier Blackhole is a high-performance DNS-based domain blocking system designed to run alongside Skypier VPN nodes. It acts as a local DNS resolver that intercepts DNS queries and blocks requests to domains in configured blocklists, returning immediate responses for blocked domains without forwarding to upstream DNS servers.

### Design Goals
- **Performance**: Sub-100μs latency per DNS lookup
- **Reliability**: Zero-downtime updates and hot-reload capability
- **Compatibility**: Support Pi-hole and standard blocklist formats
- **Simplicity**: Single binary deployment with minimal configuration
- **Security**: Memory-safe implementation in Rust

## System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        VPN Node Network Stack                   │
└────────────────────────────┬────────────────────────────────────┘
                             │ DNS Query (port 53)
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Skypier Blackhole DNS Resolver               │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              CLI Interface (clap)                        │  │
│  │  Commands: start, stop, reload, status, add, remove,    │  │
│  │           list, update, test                            │  │
│  └──────────────────────────────────────────────────────────┘  │
│                             │                                   │
│  ┌──────────────────────────┴───────────────────────────────┐  │
│  │         Configuration Manager (config.rs)                │  │
│  │  • TOML-based configuration                             │  │
│  │  • Runtime validation                                   │  │
│  │  • Default values                                       │  │
│  └──────────────────────────────────────────────────────────┘  │
│                             │                                   │
│  ┌──────────────────────────┴───────────────────────────────┐  │
│  │         DNS Server (hickory-dns)                         │  │
│  │  • Async UDP/TCP listener                               │  │
│  │  • Query parser                                         │  │
│  │  • Response builder                                     │  │
│  └────────────┬─────────────────────────────┬───────────────┘  │
│               │                             │                   │
│               ▼                             ▼                   │
│  ┌────────────────────────┐   ┌────────────────────────────┐  │
│  │  Blocklist Manager     │   │  Upstream Forwarder        │  │
│  │  (blocklist.rs)        │   │  (dns.rs)                  │  │
│  │                        │   │                            │  │
│  │  ┌──────────────────┐  │   │  Forward to 1.1.1.1:53    │  │
│  │  │  Radix Trie      │  │   │  if not blocked           │  │
│  │  │  (Wildcard)      │  │   │                            │  │
│  │  └──────────────────┘  │   └────────────────────────────┘  │
│  │  ┌──────────────────┐  │                                    │
│  │  │  HashSet         │  │                                    │
│  │  │  (Exact Match)   │  │                                    │
│  │  └──────────────────┘  │                                    │
│  │  ┌──────────────────┐  │                                    │
│  │  │  Bloom Filter    │  │                                    │
│  │  │  (Fast negative) │  │                                    │
│  │  └──────────────────┘  │                                    │
│  └────────────────────────┘                                    │
│               │                                                 │
│  ┌────────────┴─────────────────────────────────────────────┐  │
│  │      Update Scheduler (tokio-cron-scheduler)             │  │
│  │  • Cron-based scheduling (configurable)                 │  │
│  │  • HTTP downloads via BlocklistDownloader               │  │
│  │  • Automatic hot-reload after download                  │  │
│  │  • Background async task                                │  │
│  │  • Cross-platform (Linux/macOS/Windows)                 │  │
│  └──────────────────────────────────────────────────────────┘  │
│               │                                                 │
│  ┌────────────┴─────────────────────────────────────────────┐  │
│  │      Blocklist Downloader (reqwest)                      │  │
│  │  • HTTP client with 30s timeout                         │  │
│  │  • Multi-format parsing (hosts, plain, wildcards)       │  │
│  │  • Domain validation (filters IPs, invalid entries)     │  │
│  │  • Cache to local file                                  │  │
│  └──────────────────────────────────────────────────────────┘  │
│               │                                                 │
│  ┌────────────┴─────────────────────────────────────────────┐  │
│  │         Logger (tracing)                                 │  │
│  │  • Structured logging                                   │  │
│  │  • Platform-aware log paths                            │  │
│  │  • Blocked query logging with source IP                │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                             │
                             ▼
                    Upstream DNS (1.1.1.1)
```

## Core Components

### 1. DNS Server (`dns.rs`)
**Responsibility**: Handle incoming DNS queries and route them appropriately

**Key Features**:
- Async UDP/TCP listeners using hickory-dns
- Support for standard DNS query types (A, AAAA, CNAME, etc.)
- Configurable listen address (default: 127.0.0.1:53)
- Thread-safe query handling with Tokio

**Data Structures**:
```rust
pub struct DnsServer {
    config: Arc<Config>,
    blocklist: Arc<BlocklistManager>,
    upstream_client: DnsClient,
    stats: Arc<RwLock<Statistics>>,
}
```

**Query Flow**:
1. Receive DNS query
2. Parse domain name
3. Check blocklist (fast path)
4. If blocked: return immediate REFUSED/NXDOMAIN response
5. If allowed: forward to upstream DNS (1.1.1.1)
6. Return upstream response

### 2. Blocklist Manager (`blocklist.rs`)
**Responsibility**: Efficient domain lookup with wildcard support

**Data Structures**:
- **Radix Trie**: O(k) lookup where k = domain length, supports wildcard prefixes
- **HashSet**: O(1) exact match for common domains
- **Bloom Filter**: O(1) probabilistic negative check (avoid trie lookup)

**Lookup Algorithm**:
```
1. Check Bloom Filter → if negative, domain NOT blocked (fast path)
2. Check HashSet → if found, domain IS blocked
3. Check Radix Trie → wildcard match (*.example.com)
4. Return result
```

**Wildcard Support**:
- `*.example.com` blocks `subdomain.example.com` but not `example.com`
- Reverse domain storage for efficient prefix matching
- Example: `com.example.*` in trie

**Thread Safety**:
- `Arc<RwLock<T>>` for concurrent read access
- Write lock only for updates (rare)
- Atomic swap for hot-reload

### 3. Configuration Manager (`config.rs`)
**Responsibility**: Load, validate, and provide configuration

**Configuration File**: `/etc/skypier/blackhole.toml`

**Structure**:
```toml
[server]
listen_addr = "127.0.0.1"
listen_port = 53
upstream_dns = ["1.1.1.1:53"]
blocked_response = "refused"  # or "nxdomain" or "0.0.0.0"

[blocklist]
remote_lists = [
    "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts"
]
local_lists = ["/etc/skypier/custom-blocklist.txt"]
custom_list = "/etc/skypier/custom-blocklist.txt"
enable_wildcards = true

[logging]
log_blocked = true
log_path = "/var/log/skypier/blackhole.log"
log_level = "info"

[updater]
enabled = true
schedule = "0 0 0 * * *"  # Daily at midnight (6-field cron: sec min hour dom month dow)
timezone = "EST"
```

**Platform-Specific Paths**:
- Linux: `/etc/skypier/blackhole.toml`
- macOS: `/usr/local/etc/skypier/blackhole.toml`
- Windows: `C:\ProgramData\Skypier\blackhole.toml`

### 4. Update Scheduler (`scheduler.rs`)
**Responsibility**: Automatically download and update blocklists on schedule

**Features**:
- Cron-based scheduling using tokio-cron-scheduler
- Cross-platform (Linux/macOS/Windows)
- Timezone support (UTC, EST, PST, etc.)
- Background async task execution
- Automatic hot-reload after updates
- Manual trigger support via CLI

**Implementation**:
```rust
pub struct UpdateScheduler {
    scheduler: JobScheduler,
    config: Arc<Config>,
    blocklist: Arc<BlocklistManager>,
}
```

**Update Process**:
```
1. Cron schedule triggers (e.g., "0 0 0 * * *" for daily at midnight)
2. Download from remote URLs via BlocklistDownloader
3. Parse and validate domains (filter IPs, invalid entries)
4. Save to cache file (remote-blocklist-cache.txt)
5. Clear current blocklist
6. Reload all sources:
   - Custom list
   - Local lists
   - Remote cache
7. Hot-reload blocklist (zero downtime)
8. Log statistics and results
```

**Cron Schedule Examples**:
- `0 0 0 * * *` - Daily at midnight
- `0 0 */6 * * *` - Every 6 hours
- `0 0 0 */2 * *` - Every 2 days
- `*/30 * * * *` - Every 30 minutes

### 5. Blocklist Downloader (`downloader.rs`)
**Responsibility**: HTTP download and parsing of remote blocklists

**Features**:
- HTTP client with reqwest (30s timeout)
- Multi-format parsing:
  - Plain domains (one per line)
  - Hosts file format (`0.0.0.0 domain.com`, `127.0.0.1 domain.com`)
  - Wildcard entries (`*.domain.com`)
- Domain validation (filters out IPs, localhost, invalid entries)
- Batch processing (multiple URLs)
- Deduplication

**Performance**:
- 86,332 domains downloaded in ~270ms
- Cache file: 1.7MB
- Load time: ~125ms

### 6. CLI Interface (`cli.rs`)
**Responsibility**: User interaction and control

**Commands**:
- `start`: Launch DNS server with integrated scheduler
- `stop`: Gracefully shutdown server and scheduler
- `reload`: Hot-reload blocklists (SIGHUP on Unix, CLI on Windows)
- `status`: Show statistics (domains blocked, queries/sec, uptime)
- `add <domain>`: Add domain to custom blocklist
- `remove <domain>`: Remove domain from custom blocklist
- `list`: Show blocklist statistics
- `update`: Force immediate blocklist update (bypasses schedule)
- `test <domain>`: Test if domain would be blocked

**Cross-Platform Support**:
- Platform-aware default config paths (Linux/macOS/Windows)
- Signal handling on Unix (SIGHUP, SIGTERM, SIGINT)
- CLI commands work on all platforms

### 7. Logger (`logger.rs`)
**Responsibility**: Structured logging

**Features**:
- Dual output: stdout + file
- Blocked query logging:
  ```
  [2025-12-23T12:34:56Z BLOCKED] domain=ad.example.com source_ip=10.0.0.5
  ```
- Log rotation (external: logrotate)
- Configurable levels: trace, debug, info, warn, error

## Data Flow

### Blocked Query Flow
```
Client → DNS Query (ad.example.com)
  ↓
DNS Server receives query
  ↓
BlocklistManager.is_blocked("ad.example.com")
  ↓ (true)
Return DNS REFUSED response (instant, <100μs)
  ↓
Log: [BLOCKED] domain=ad.example.com source=10.0.0.5
  ↓
Client receives immediate response
```

### Allowed Query Flow
```
Client → DNS Query (google.com)
  ↓
DNS Server receives query
  ↓
BlocklistManager.is_blocked("google.com")
  ↓ (false)
Forward to upstream DNS (1.1.1.1:53)
  ↓
Receive upstream response (IP: 142.250.80.46)
  ↓
Return response to client
  ↓
Client receives valid IP
```

### Hot Reload Flow
```
CLI: skypier-blackhole reload
  ↓
Send SIGHUP signal to daemon
  ↓
BlocklistManager.reload()
  ↓
Load new domains from files
  ↓
Build new Trie + HashSet + Bloom
  ↓
Atomic swap: Arc::new(new_data)
  ↓
Old data dropped when last ref released
  ↓
DNS server continues without interruption
```

## Performance Optimization

### 1. Lookup Optimization
- **Bloom Filter**: 1% false positive rate, saves 99% of trie lookups for non-blocked domains
- **HashSet**: O(1) for top 10k most common blocked domains
- **Radix Trie**: O(k) where k = domain length, memory-efficient

### 2. Memory Efficiency
- Radix trie shares common prefixes (e.g., all `.com` domains)
- Domain interning for duplicate strings
- Estimated memory: ~50MB for 1M domains

### 3. Concurrency
- Lock-free reads with `Arc<T>`
- Read-write lock only for updates
- Tokio async runtime: handle 10k+ concurrent queries

### 4. Network Optimization
- UDP for fast responses
- TCP fallback for large responses
- Connection pooling for upstream DNS
- DNS response caching (optional future feature)

### 5. Compilation Optimization
```toml
[profile.release]
opt-level = 3          # Maximum optimization
lto = true             # Link-time optimization
codegen-units = 1      # Single codegen unit (slower build, faster binary)
strip = true           # Strip debug symbols
```

**Expected Performance**:
- Blocked query: <100μs (0.0001s)
- Allowed query: <5ms (upstream latency)
- Memory: <100MB for 1M domains
- Throughput: >50k queries/sec on modern hardware

## Security Considerations

### 1. Memory Safety
- Rust guarantees: no buffer overflows, no use-after-free
- No unsafe code in core logic
- Dependencies audited with `cargo audit`

### 2. Privilege Separation
- Run as non-root user (systemd User=skypier)
- CAP_NET_BIND_SERVICE for port 53 binding
- Drop privileges after binding

### 3. Input Validation
- Domain name validation (RFC 1035)
- URL validation for remote lists
- File path sanitization

### 4. DoS Protection
- Rate limiting per source IP (future)
- Maximum query size limits
- Connection limits

### 5. Configuration Security
- File permissions: 640 for config, 600 for custom lists
- No secrets in config (future: encrypted blocklists)

## Deployment Architecture

### Systemd Service
**File**: `/etc/systemd/system/skypier-blackhole.service`

```ini
[Unit]
Description=Skypier Blackhole DNS Resolver
After=network.target
Documentation=https://github.com/skypier/skypier-blackhole

[Service]
Type=simple
User=skypier
Group=skypier
ExecStart=/usr/bin/skypier-blackhole start
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=5s

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/log/skypier

# Network capabilities
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
```

### Integration with VPN Node
1. **System Resolver**: Set in `/etc/resolv.conf`:
   ```
   nameserver 127.0.0.1
   ```

2. **VPN DHCP**: Push DNS server to clients:
   ```
   push "dhcp-option DNS 10.8.0.1"  # VPN node IP running blackhole
   ```

3. **Health Check**: VPN node can query special domain:
   ```bash
   dig @127.0.0.1 _health.skypier-blackhole.local
   ```

### Monitoring
- Systemd journal: `journalctl -u skypier-blackhole -f`
- Log file: `/var/log/skypier/blackhole.log`
- Metrics endpoint (future): Prometheus exporter

### Package Distribution
- **DEB package**: For Debian/Ubuntu VPN nodes
- **Binary**: Static binary for other distros
- **Docker image**: Containerized deployment (future)

## Future Enhancements

1. **DNS-over-HTTPS (DoH)**: Support for encrypted DNS
2. **Response Caching**: Cache upstream responses
3. **Rate Limiting**: Per-IP rate limits
4. **Metrics**: Prometheus metrics endpoint
5. **Web UI**: Simple web dashboard
6. **Whitelist**: Override blocklist entries
7. **Regex Support**: Advanced pattern matching
8. **GeoIP Blocking**: Block domains by country
9. **DNSSEC**: Validate upstream responses

## References

- [Hickory DNS Documentation](https://docs.rs/hickory-server/)
- [Pi-hole Blocklist Format](https://github.com/pi-hole/pi-hole)
- [DNS RFC 1035](https://tools.ietf.org/html/rfc1035)
- [Tokio Async Runtime](https://tokio.rs/)
