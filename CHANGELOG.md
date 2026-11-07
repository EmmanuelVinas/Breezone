# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-11-07

### Added

- Secure pairing between Breezone servers: single-use invitation valid 15 minutes, access request
  to accept in Settings › Shares, every request signed with the server's Ed25519 key (time window
  and replay protection).
- Visual acknowledgement of commands (pending, applied, failed) in the web UI and the app.
- Synology NAS installation guide.

### Changed

- Invitations from 0.1.0 (`bz1.`) are no longer accepted: existing links between servers are
  removed on upgrade and must be paired again.

### Fixed

- Setpoint briefly jumping back to its previous value after a change.
- Zone names and settings kept from another WebServer when an installation is pointed at a new one.
- Android 17: request the local network permission needed to reach the WebServer.

## [0.1.0] - 2026-11-06

### Added

- Rust server driving Airzone WebServers through their local API: zones, groups, modes, history,
  activity feed, users and devices.
- Web UI (React) and Android / iOS app (Compose Multiplatform).
- Schedules of the Airzone app, synchronised hourly and editable from Breezone (Airzone Cloud).
- Routines: scenes with delayed steps and timers, run by the server.
- Federation between Breezone servers (sharing installations across sites without VPN).
- Optional read-only import of the Airzone account (names, city, groups).
- Docker image for `linux/amd64` and `linux/arm64` on GitHub Container Registry.

### Changed

- Project renamed from FreeZone to Breezone (former `FREEZONE_*` variables and `freezone.db` are
  still recognised).

[Unreleased]: https://github.com/EmmanuelVinas/breezone/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/EmmanuelVinas/breezone/releases/tag/v0.1.0
