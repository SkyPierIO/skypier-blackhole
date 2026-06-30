# Quick Start Guide

**Skypier Blackhole** is a cross-platform DNS-based domain blocker. This guide will get you up and running in minutes.

## Supported Platforms

- ✅ Linux (x86_64, ARM64)
- ✅ macOS (Intel, Apple Silicon)
- ✅ Windows (x86_64)

## Installation

### Option 1: Quick Setup (Linux/macOS - Recommended)

```bash
# Clone the repository
git clone https://github.com/skypier/skypier-blackhole.git
cd skypier-blackhole

# Build the binary
cargo build --release

# Run setup script (installs to platform-appropriate locations)
sudo ./scripts/setup.sh
```

**What this does**:
- Linux: Installs to `/etc/skypier/` and `/usr/bin/`
- macOS: Installs to `/usr/local/etc/skypier/` and `/usr/local/bin/`

### Option 2: Manual Setup (All Platforms)

**Linux/macOS**:
```bash
# Build the binary
cargo build --release

# Create directories
sudo mkdir -p /etc/skypier /var/log/skypier      # Linux
# or
sudo mkdir -p /usr/local/etc/skypier /usr/local/var/log/skypier  # macOS

# Copy config
sudo cp config/blackhole.toml.example /etc/skypier/blackhole.toml

# Install binary
sudo cp target/release/skypier-blackhole /usr/bin/           # Linux
# or
sudo cp target/release/skypier-blackhole /usr/local/bin/     # macOS

# Create custom blocklist
sudo touch /etc/skypier/custom-blocklist.txt
```

**Windows**:
```powershell
# Build the binary
cargo build --release

# Create directories
mkdir C:\ProgramData\Skypier
mkdir C:\ProgramData\Skypier\Logs

# Copy config
copy config\blackhole.toml.example C:\ProgramData\Skypier\blackhole.toml

# Add binary to PATH or run from target\release\
```

## Usage

Once installed, the binary automatically uses platform-appropriate default paths.

### Basic Commands (No Config Flag Needed!)

```bash
# Check status
skypier-blackhole status

# Test if a domain is blocked
skypier-blackhole test ads.example.com

# Download remote blocklists
skypier-blackhole update

# Add a domain to blocklist
skypier-blackhole add malware.com

# Remove a domain
skypier-blackhole remove malware.com

# List blocklist statistics
skypier-blackhole list

# Start the DNS server (requires port 53, needs admin privileges)
sudo skypier-blackhole start          # Linux/macOS
# Run as Administrator on Windows

# Stop the server
skypier-blackhole stop

# Hot-reload blocklists (Unix signals on Linux/macOS, CLI on Windows)
skypier-blackhole reload
```

### Using Custom Config

If you want to use a different config file:

```bash
skypier-blackhole status --config /path/to/custom-config.toml
```

### Platform-Specific Notes

**Linux**:
- Default config: `/etc/skypier/blackhole.toml`
- Use `systemctl` for service management
- Signal handling fully supported (SIGHUP for reload)

**macOS**:
- Default config: `/usr/local/etc/skypier/blackhole.toml`
- Can use `launchd` for background service
- Signal handling fully supported

**Windows**:
- Default config: `C:\ProgramData\Skypier\blackhole.toml`
- Run as Administrator for port 53 access
- Use CLI commands instead of signals

## Configuration

Edit your platform's config file to customize:

```toml
[server]
listen_addr = "127.0.0.1"  # Change to "0.0.0.0" for all interfaces
listen_port = 53           # DNS port
upstream_dns = ["1.1.1.1:53", "8.8.8.8:53"]

[blocklist]
custom_list = "/etc/skypier/custom-blocklist.txt"
remote_lists = [
    "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts"
]

[logging]
log_level = "info"
log_path = "/var/log/skypier/blackhole.log"

[updater]
enabled = true
schedule = "0 0 0 * * *"  # Cron expression, 6-field with seconds (daily at midnight)
timezone = "EST"        # Your timezone (UTC, EST, PST, etc.)
```

### Automatic Updates

The scheduler runs in the background and automatically updates blocklists:

**How it works**:
1. When you run `skypier-blackhole start`, the scheduler starts automatically
2. It downloads blocklists at the configured time (e.g., daily at midnight)
3. Updates are applied with zero downtime (hot-reload)
4. Logs show update results

**Configure schedule**:
```toml
[updater]
enabled = true
schedule = "0 0 0 * * *"   # Daily at midnight (sec min hour dom month dow)
# schedule = "0 0 */6 * * *"  # Every 6 hours
# schedule = "0 0 0 */2 * *"  # Every 2 days
timezone = "EST"
```

**Disable automatic updates**:
```toml
[updater]
enabled = false
```

You can still manually update with `skypier-blackhole update`.

## First Run

```bash
# 1. Download blocklists (recommended first step)
skypier-blackhole update

# This will download ~86K domains from configured sources
# Output:
# 🔄 Updating Blocklists
#   ⬇ Downloading blocklists...
#   ✓ Downloaded 86332 unique domains
#   💾 Saving to cache: /etc/skypier/remote-blocklist-cache.txt
#   ✓ Cache saved successfully

# 2. Check status
skypier-blackhole status

# Output:
# 📊 Skypier Blackhole Status
# ══════════════════════════════════════════════════
#   ○ Server Status: STOPPED
#   📋 Blocklist Statistics:
#     • Total domains blocked: 86335
#     • Custom list: /etc/skypier/custom-blocklist.txt

# 3. Test some domains
skypier-blackhole test ads.example.com
# 🔍 Testing domain: ads.example.com
#   🚫 Status: BLOCKED

skypier-blackhole test google.com
# 🔍 Testing domain: google.com
#   ✓ Status: ALLOWED

# 4. Start the server (requires sudo for port 53)
sudo skypier-blackhole start
```

## Production Deployment

For production use with systemd:

```bash
# Copy systemd service file
sudo cp systemd/skypier-blackhole.service /etc/systemd/system/

# Reload systemd
sudo systemctl daemon-reload

# Enable on boot
sudo systemctl enable skypier-blackhole

# Start service
sudo systemctl start skypier-blackhole

# Check status
sudo systemctl status skypier-blackhole
```

## Troubleshooting

### Port 53 Already in Use

```bash
# Check what's using port 53
sudo lsof -i :53

# If systemd-resolved is running, you can:
# 1. Use a different port (edit config: listen_port = 5353)
# 2. Or disable systemd-resolved DNS stub
sudo systemctl disable systemd-resolved
```

### Permission Denied

```bash
# The start command needs sudo for port 53
sudo skypier-blackhole start

# Other commands work without sudo
skypier-blackhole status
skypier-blackhole test domain.com
```

### Config Not Found

```bash
# If you get "config not found", create it:
sudo mkdir -p /etc/skypier
sudo cp config/blackhole.toml.example /etc/skypier/blackhole.toml

# Or use --config flag:
skypier-blackhole status --config ./test-config.toml
```
