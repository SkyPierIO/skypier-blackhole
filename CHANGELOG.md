# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Interactive terminal dashboard (`skypier-blackhole tui`) built with
  [ratatui](https://ratatui.rs): runs the DNS server with a live activity log
  (blocked queries highlighted), upstream resolver overview, in-RAM session
  stats (query counts, block rate, top blocked domains since startup), and a
  blocklist summary panel with per-source counts and last/next remote update.
  Domains can be added/removed and remote lists updated/reloaded directly from
  the dashboard (`a`/`d`/`u`/`r`).

## [0.2.0] - 2026-07-09

### Added

- DNS over HTTPS (DoH) upstream support ([#27](https://github.com/SkyPierIO/skypier-blackhole/issues/27)):
  `upstream_dns` entries can now be DoH URLs alongside plain `ip:port`, e.g.
  `https://dns.quad9.net/dns-query@9.9.9.9:443` (hostname + bootstrap IP) or
  `https://1.1.1.1/dns-query` (IP-literal host). Invalid entries fail at
  startup with descriptive errors.
- `CHANGELOG.md` (this file).

### Changed

- The upstream connection is now cached and reused across queries instead of
  being re-established per query, with an automatic reconnect-and-retry on
  stale connections. This also speeds up plain UDP forwarding.
- The blocklist downloader user agent now derives its version from
  `Cargo.toml` instead of a hardcoded string.

## [0.1.3] - 2026-07-08

### Changed

- README overhaul: asciinema demo, blocklist lookup diagram, updated blocked
  domains section.
- Better error messages for config file read/parse failures.
- Dependency updates (clap, tracing-subscriber, windows-sys).

## [0.1.2] - 2026-07-03

### Fixed

- `create-release` CI job: add missing checkout step.

## [0.1.1] - 2026-07-03

### Changed

- Logging formatter: deduplication and terminal detection.
- Release workflow: create the GitHub release before uploading binaries.
- Dependency updates via Renovate (futures, mockito, chrono, anyhow).

## [0.1.0] - 2026-07-01

### Added

- Initial release: blocklist-driven DNS sinkhole with UDP listener, upstream
  forwarding, remote/local/custom blocklists, wildcard matching, scheduled
  blocklist updates, systemd unit, and Debian packaging.

[Unreleased]: https://github.com/SkyPierIO/skypier-blackhole/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/SkyPierIO/skypier-blackhole/compare/v0.1.3...v0.2.0
[0.1.3]: https://github.com/SkyPierIO/skypier-blackhole/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/SkyPierIO/skypier-blackhole/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/SkyPierIO/skypier-blackhole/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/SkyPierIO/skypier-blackhole/releases/tag/v0.1.0
