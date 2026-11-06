# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
