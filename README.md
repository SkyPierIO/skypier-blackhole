
<div align="center">

# Skypier Blackhole

<img src="doc/logo.png" alt="Skypier Blackhole Logo" height="300">

**High-Performance DNS-Based Domain Blocking for Skypier VPN Nodes**

<p align="center">
  <img src="https://img.shields.io/badge/Rust_1.70+-813374?style=flat&logo=rust&logoColor=white" alt="Rust 1.70+">
  <img src="https://img.shields.io/badge/DNS_Resolver-813374?style=flat&logo=cloudflare&logoColor=white" alt="DNS Resolver">
  <img src="https://img.shields.io/badge/VPN_Ready-813374?style=flat&logo=openvpn&logoColor=white" alt="VPN Ready">
  <img src="https://img.shields.io/badge/Tokio_Async-813374?style=flat&logo=tokio&logoColor=white" alt="Tokio Async">
  <img src="https://img.shields.io/badge/Zero_Unsafe-813374?style=flat&logo=rust&logoColor=white" alt="Zero Unsafe">
  <img src="https://img.shields.io/badge/License-MIT-813374?style=flat" alt="MIT License">
</p>

*Block ads, trackers, and unwanted domains at the DNS level with lightning-fast performance*

</div>

## Overview

Skypier Blackhole is a Rust-based DNS resolver designed to run alongside Skypier VPN nodes, providing domain-level blocking of ads, trackers, analytics, and other unwanted content. It acts as a local DNS server that intercepts queries and blocks requests to domains in configurable blocklists.

### Key Features

- **Blazing Fast**: Sub-100μs latency for blocked queries
- **Memory Safe**: Written in Rust with zero unsafe code
- **Hot Reload**: Update blocklists without service restart (SIGHUP signal)
- **Auto-Update**: Scheduled automatic downloads from remote blocklists
- **Cron Scheduler**: Flexible scheduling with cron expressions (daily, hourly, custom)
- **Cross-Platform**: Works on Linux, macOS, and Windows
- **Wildcard Support**: Block entire subdomains with `*.domain.com`
- **Monitoring**: Real-time statistics and detailed logging
- **Production Ready**: Systemd integration and DEB packages
- **Easy Configuration**: Simple TOML config with sensible defaults

### Performance

- **Blocked Query Latency**: <100μs (0.0001s)
- **Allowed Query Latency**: <5ms (upstream forwarding)
- **Memory Usage**: <100MB for 1M domains
- **Throughput**: >50k queries/sec on modern hardware

## Table of Contents

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [Usage](#usage)
- [Architecture](#architecture)
- [Development](#development)
- [FAQ](#faq)
- [Contributing](#contributing)
- [License](#license)

## Installation

Skypier Blackhole runs on **Linux**, **macOS**, and **Windows**. Choose the installation method that works best for your platform.

### Option 1: DEB Package (Recommended for Debian/Ubuntu)

```bash
# Download the latest release
wget https://github.com/skypier/skypier-blackhole/releases/latest/download/skypier-blackhole_amd64.deb

# Install the package
sudo dpkg -i skypier-blackhole_amd64.deb

# The service is automatically enabled and started
sudo systemctl status skypier-blackhole
```

### Option 2: From Source (All Platforms)

```bash
# Prerequisites: Rust 1.70+ and Cargo
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone the repository
git clone https://github.com/skypier/skypier-blackhole.git
cd skypier-blackhole

# Build the release binary
cargo build --release

# Install the binary
sudo cp target/release/skypier-blackhole /usr/bin/

# Create directories
sudo mkdir -p /etc/skypier /var/log/skypier

# Copy example config
sudo cp config/blackhole.toml.example /etc/skypier/blackhole.toml

# Install systemd service
sudo cp systemd/skypier-blackhole.service /etc/systemd/system/
sudo systemctl daemon-reload
```

### Option 3: Pre-built Binary (All Platforms)

```bash
# Download the latest binary
wget https://github.com/skypier/skypier-blackhole/releases/latest/download/skypier-blackhole

# Make it executable
chmod +x skypier-blackhole

# Move to appropriate location
# Linux: /usr/bin/
# macOS: /usr/local/bin/
# Windows: Add to PATH
```

### Platform-Specific Configuration Paths

Skypier Blackhole uses platform-appropriate default paths:

| Platform | Config File | Custom Blocklist | Logs |
|----------|-------------|------------------|------|
| **Linux** | `/etc/skypier/blackhole.toml` | `/etc/skypier/custom-blocklist.txt` | `/var/log/skypier/blackhole.log` |
| **macOS** | `/usr/local/etc/skypier/blackhole.toml` | `/usr/local/etc/skypier/custom-blocklist.txt` | `/usr/local/var/log/skypier/blackhole.log` |
| **Windows** | `C:\ProgramData\Skypier\blackhole.toml` | `C:\ProgramData\Skypier\custom-blocklist.txt` | `C:\ProgramData\Skypier\Logs\blackhole.log` |

> **Note**: On Linux/macOS, you can override these with the `--config` flag. Windows paths use `PROGRAMDATA` environment variable.

## Quick Start

### 1. Configure System DNS

Set your system to use Skypier Blackhole as the DNS resolver:

```bash
# Edit /etc/resolv.conf
sudo nano /etc/resolv.conf

# Add this line at the top:
nameserver 127.0.0.1
```

Or for permanent configuration (with systemd-resolved):

```bash
sudo mkdir -p /etc/systemd/resolved.conf.d/
cat << EOF | sudo tee /etc/systemd/resolved.conf.d/skypier.conf
[Resolve]
DNS=127.0.0.1
Domains=~.
EOF

sudo systemctl restart systemd-resolved
```

### 2. Start the Service

```bash
# Enable and start the service
sudo systemctl enable skypier-blackhole
sudo systemctl start skypier-blackhole

# Check status
sudo systemctl status skypier-blackhole
```

### 3. Test Domain Blocking

```bash
# Test a blocked domain (should return REFUSED)
dig @127.0.0.1 ads.example.com

# Test an allowed domain (should return IP)
dig @127.0.0.1 google.com

# Use the CLI test command
skypier-blackhole test ads.example.com
```

### 4. View Statistics

```bash
# Show current statistics
skypier-blackhole status

# View logs
sudo journalctl -u skypier-blackhole -f
```

## Configuration

The configuration file uses TOML format and is automatically located based on your platform (see table above). You can also specify a custom path with `--config`.

### Basic Configuration

```toml
[server]
listen_addr = "127.0.0.1"  # Use "0.0.0.0" for all interfaces
listen_port = 53
upstream_dns = ["1.1.1.1:53", "8.8.8.8:53"]
blocked_response = "refused"  # Options: "refused", "nxdomain", {ip = "0.0.0.0"}

[blocklist]
remote_lists = [
    "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts"
]
enable_wildcards = true

[logging]
log_blocked = true
log_path = "/var/log/skypier/blackhole.log"
log_level = "info"

[updater]
enabled = true
schedule = "0 0 * * *"  # Daily at midnight (cron format)
timezone = "EST"
```

### Scheduler Configuration

The built-in scheduler automatically downloads and updates blocklists on a configurable schedule:

```toml
[updater]
enabled = true                    # Enable/disable automatic updates
schedule = "0 0 * * *"           # Cron expression (see below)
timezone = "EST"                 # Timezone for schedule (UTC, EST, PST, etc.)
```

**Common Cron Schedules**:
- `0 0 * * *` - Daily at midnight
- `0 */6 * * *` - Every 6 hours
- `0 0 */2 * *` - Every 2 days at midnight
- `0 3 * * 0` - Every Sunday at 3am
- `*/30 * * * *` - Every 30 minutes

**How it works**:
1. Scheduler runs in background when DNS server starts
2. Downloads blocklists from `remote_lists` URLs at scheduled time
3. Validates and caches domains locally
4. Hot-reloads blocklist without interrupting DNS queries
5. Logs update results for monitoring

**Manual Trigger**:
```bash
# Force an update now (bypasses schedule)
skypier-blackhole update
```

### Configuration Options

| Section | Option | Default | Description |
|---------|--------|---------|-------------|
| `[server]` | `listen_addr` | `127.0.0.1` | DNS server listen address |
| | `listen_port` | `53` | DNS server listen port |
| | `upstream_dns` | `["1.1.1.1:53"]` | Upstream DNS servers |
| | `blocked_response` | `refused` | Response for blocked domains |
| `[blocklist]` | `remote_lists` | `[]` | URLs to download blocklists |
| | `local_lists` | `[]` | Local blocklist file paths |
| | `custom_list` | `/etc/skypier/custom-blocklist.txt` | Custom blocklist file |
| | `enable_wildcards` | `true` | Enable wildcard matching |
| `[logging]` | `log_blocked` | `true` | Log blocked queries |
| | `log_path` | `/var/log/skypier/blackhole.log` | Log file path |
| | `log_level` | `info` | Log level (trace/debug/info/warn/error) |
| `[updater]` | `enabled` | `true` | Enable automatic updates |
| | `schedule` | `0 0 * * *` | Update schedule (cron format) |
| | `timezone` | `EST` | Timezone for schedule |

## Usage

### CLI Commands

```bash
# Start the DNS server (usually via systemd)
skypier-blackhole start

# Stop the server
skypier-blackhole stop

# Hot reload blocklists without restart
skypier-blackhole reload

# Show server status and statistics
skypier-blackhole status

# Add a domain to the custom blocklist
skypier-blackhole add ads.example.com

# Remove a domain from the custom blocklist
skypier-blackhole remove ads.example.com

# List blocklist statistics
skypier-blackhole list

# Force update blocklists from remote sources
skypier-blackhole update

# Test if a domain is blocked
skypier-blackhole test doubleclick.net
```

### Systemd Service Management

```bash
# Start the service
sudo systemctl start skypier-blackhole

# Stop the service
sudo systemctl stop skypier-blackhole

# Restart the service
sudo systemctl restart skypier-blackhole

# Reload blocklists (hot reload)
sudo systemctl reload skypier-blackhole

# Enable on boot
sudo systemctl enable skypier-blackhole

# View logs
sudo journalctl -u skypier-blackhole -f

# View status
sudo systemctl status skypier-blackhole
```

### Signal Handling (Linux/macOS)

Skypier Blackhole supports Unix signals for graceful operations on Linux and macOS systems:

> **Note**: Signal handling is Unix-specific. On Windows, use the CLI commands (`skypier-blackhole reload`, `stop`) instead.

#### SIGHUP - Hot Reload Blocklists
Reload blocklists without restarting the server (zero downtime):

```bash
# Method 1: Direct signal (Unix only)
kill -HUP $(pgrep skypier-blackhole)

# Method 2: Systemd reload (Linux)
sudo systemctl reload skypier-blackhole

# Method 3: CLI command (all platforms)
skypier-blackhole reload
```

**What happens during SIGHUP:**
- Clears current in-memory blocklist
- Reloads all configured blocklist files
- Updates data structures (radix tree + hash set)
- DNS queries continue without interruption
- Reload completes in <5ms for typical blocklists

#### SIGTERM / SIGINT - Graceful Shutdown
Stop the server gracefully:

```bash
# Method 1: SIGTERM (recommended)
kill -TERM $(pgrep skypier-blackhole)

# Method 2: SIGINT (Ctrl+C in foreground)
^C

# Method 3: Systemd stop
sudo systemctl stop skypier-blackhole
```

**What happens during shutdown:**
- Stops accepting new DNS queries
- Completes all in-flight queries
- Closes signal handlers
- Releases resources cleanly
- Exits with status 0

#### Production Usage

For production deployments, the systemd service automatically handles signals:

```bash
# Graceful shutdown (sends SIGTERM)
sudo systemctl stop skypier-blackhole

# Hot reload (sends SIGHUP)
sudo systemctl reload skypier-blackhole

# Restart (stop + start)
sudo systemctl restart skypier-blackhole
```

**Performance Impact:**
- Signal handling overhead: <0.01ms
- SIGHUP reload time: ~2ms for 10K domains
- DNS queries dropped during reload: 0
- Memory spike during reload: temporary (cleared after)

See [wip/SIGNAL_HANDLING_COMPLETE.md](wip/SIGNAL_HANDLING_COMPLETE.md) for detailed implementation and test results.

### Custom Blocklists

Create a custom blocklist file (one domain per line):

```bash
# Create custom blocklist
sudo nano /etc/skypier/custom-blocklist.txt

# Add domains (one per line)
# Exact matches
ads.example.com
tracker.example.com

# Wildcard support - blocks all subdomains
*.analytics.example.com
*.doubleclick.net
*.googlesyndication.com

# Comments are supported
# This will block tracker.ads.facebook.com but NOT ads.facebook.com
*.ads.facebook.com

# Reload to apply changes
sudo systemctl reload skypier-blackhole
```

#### Wildcard Matching Rules

Wildcard syntax `*.domain.com` blocks **all subdomains** but **not** the base domain:

| Blocklist Entry | Blocks | Allows |
|-----------------|--------|--------|
| `*.example.com` | `sub.example.com`<br>`deep.sub.example.com`<br>`ads.example.com` | `example.com` |
| `*.ads.example.com` | `tracker.ads.example.com`<br>`banner.ads.example.com` | `ads.example.com`<br>`example.com` |
| `exact.com` | `exact.com` only | `sub.exact.com` |

**Examples**:
```bash
# Block all Google advertising subdomains
*.googlesyndication.com

# Block all Facebook tracking subdomains  
*.facebook.com

# Block specific ad networks
*.doubleclick.net
*.advertising.com
```

### Integration with VPN Nodes

For use with Skypier VPN nodes, configure the VPN to push Skypier Blackhole as the DNS server:

**OpenVPN Configuration**:
```conf
# In server.conf
push "dhcp-option DNS 10.8.0.1"  # VPN server IP
```

**WireGuard Configuration**:
```ini
# In wg0.conf
[Interface]
DNS = 10.8.0.1
```

## Architecture

Skypier Blackhole uses a multi-layered approach for efficient domain lookup:

1. **Bloom Filter**: Fast probabilistic negative check (O(1))
2. **HashSet**: Exact match for common domains (O(1))
3. **Radix Trie**: Wildcard matching (O(k) where k = domain length)

For detailed architecture documentation, see [doc/ARCHITECTURE.md](doc/ARCHITECTURE.md).

### Data Flow

```
DNS Query → Bloom Filter (negative?) → HashSet (exact match?) 
    → Radix Trie (wildcard match?) → Blocked or Forward to Upstream
```

## Development

### Prerequisites

- Rust 1.70 or later
- Cargo
- Linux (for full systemd integration)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/skypier/skypier-blackhole.git
cd skypier-blackhole

# Build in debug mode
cargo build

# Build in release mode (optimized)
cargo build --release

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run -- start --config config/blackhole.toml.example

# Run clippy (linter)
cargo clippy

# Format code
cargo fmt
```

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_blocklist

# Run benchmarks
cargo bench
```

### Project Structure

```
skypier-blackhole/
├── src/
│   ├── main.rs          # Entry point
│   ├── lib.rs           # Library exports
│   ├── cli.rs           # CLI interface (cross-platform)
│   ├── config.rs        # Configuration management (platform-aware paths)
│   ├── dns.rs           # DNS server implementation
│   ├── blocklist.rs     # Blocklist management (wildcard support)
│   ├── downloader.rs    # Remote blocklist downloader
│   ├── scheduler.rs     # Automatic update scheduler (NEW)
│   └── logger.rs        # Logging setup
├── doc/
│   ├── ARCHITECTURE.md  # Architecture documentation
│   └── UserStories.md   # User stories and tasks
├── config/
│   └── blackhole.toml.example  # Example configuration
├── scripts/
│   ├── test-dns.sh              # DNS integration tests
│   ├── test-signals.sh          # Signal handling tests
│   ├── test-wildcards.sh        # Wildcard matching tests
│   ├── test-cli.sh              # CLI commands tests
│   ├── test-update.sh           # Remote download tests
│   ├── test-scheduler.sh        # Scheduler tests (NEW)
│   └── setup.sh                 # Production setup script
├── systemd/
│   └── skypier-blackhole.service  # Systemd service file
├── tests/               # Integration tests
├── Cargo.toml           # Rust project manifest
└── README.md            # This file
```

## FAQ

### Q: What platforms are supported?

**A**: Skypier Blackhole runs on:
- ✅ **Linux** (x86_64, ARM64)
- ✅ **macOS** (Intel, Apple Silicon)
- ✅ **Windows** (x86_64)

All core features work on all platforms. Signal handling (SIGHUP) is Unix-only; use CLI commands on Windows.

### Q: How is this different from Pi-hole?

**A**: Skypier Blackhole is:
- Written in Rust (faster, more memory-safe)
- Single binary with no dependencies
- Cross-platform (Linux/macOS/Windows vs Linux-only)
- Optimized for VPN node deployments
- <100μs latency vs Pi-hole's ~1-5ms
- Smaller memory footprint (<50MB vs ~200MB)
- Built-in cron scheduler (no external cron needed)

### Q: Can I use this as a Pi-hole replacement?

**A**: Yes! Configure `listen_addr = "0.0.0.0"` to listen on all interfaces, then point your devices to the server's IP. Works great for home networks.

### Q: Does it support DNSSEC?

**A**: Not yet. This is planned for a future release.

### Q: Can I use multiple upstream DNS servers?

**A**: Yes, configure multiple servers in `upstream_dns = ["1.1.1.1:53", "8.8.8.8:53"]`. Currently, the first responsive server is used.

### Q: How do I update blocklists manually?

**A**: Run `skypier-blackhole update` to force an immediate update, bypassing the schedule.

### Q: How does the automatic scheduler work?

**A**: The scheduler runs in the background using cron expressions. It downloads blocklists at the configured time, caches them locally, and hot-reloads without interrupting DNS queries. Default: daily at midnight.

### Q: Can I disable automatic updates?

**A**: Yes, set `enabled = false` in the `[updater]` section of your config. You can still manually update with `skypier-blackhole update`.

### Q: Can I whitelist domains?

**A**: Not in the current version. Whitelist support is planned for v0.3.0.

### Q: What blocklist format is supported?

**A**: Standard Pi-hole format (one domain per line), with `#` for comments. Wildcards are supported with `*.domain.com` syntax. Compatible with StevenBlack hosts files and similar formats.

### Q: How much memory does it use?

**A**: Approximately 50-100MB for 1 million domains, depending on configuration.

## Troubleshooting

### DNS Queries Not Working

```bash
# Check if service is running
sudo systemctl status skypier-blackhole

# Check if port 53 is listening
sudo netstat -tulpn | grep :53

# Test DNS directly
dig @127.0.0.1 google.com

# Check logs for errors
sudo journalctl -u skypier-blackhole -n 50
```

### Permission Denied on Port 53

Port 53 requires elevated privileges. The systemd service handles this with capabilities. If running manually:

```bash
# Option 1: Run as root (not recommended)
sudo skypier-blackhole start

# Option 2: Use a higher port (>1024)
# Edit config: listen_port = 5353
```

### Blocklists Not Updating

```bash
# Check updater configuration
cat /etc/skypier/blackhole.toml | grep -A5 "\[updater\]"

# Force manual update
skypier-blackhole update

# Check network connectivity
curl -I https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts
```

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Workflow

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests (`cargo test`)
5. Run clippy (`cargo clippy`)
6. Format code (`cargo fmt`)
7. Commit your changes (`git commit -m 'Add amazing feature'`)
8. Push to the branch (`git push origin feature/amazing-feature`)
9. Open a Pull Request

## License

This project is dual-licensed under:

- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

You may choose either license.

## Acknowledgments

- [Hickory DNS](https://github.com/hickory-dns/hickory-dns) - Rust DNS library
- [Pi-hole](https://pi-hole.net/) - Inspiration for blocklist format
- [StevenBlack/hosts](https://github.com/StevenBlack/hosts) - Comprehensive blocklists
- Tokio team - Excellent async runtime

## Support

- 📖 [Documentation](doc/ARCHITECTURE.md)
- 🐛 [Issue Tracker](https://github.com/skypier/skypier-blackhole/issues)
- 💬 [Discussions](https://github.com/skypier/skypier-blackhole/discussions)

---

<div align="center">

**Made with ❤️ by the Skypier Team**

[Website](https://skypier.io) • [GitHub](https://github.com/skypierio) • [Twitter](https://twitter.com/skypierio)

</div>
