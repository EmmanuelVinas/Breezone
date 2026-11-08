# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- App: the section bar of an installation stays visible (it no longer hides while scrolling), the
  lists end right above it.
- App: the setpoint in the routine and schedule editors follows the limits of the chosen zones; the
  Airzone password can be shown while typing it.

## [0.6.0] - 2026-11-08

### Added

- Federation: a server follows the installations another server shares with it **live** (long
  polling over the signed API): a change there shows here within a second, instead of at the next
  read. At rest, a few hundred bytes every 45 s; the full state only travels when something changes.
  The link reopens by itself when either server comes back (retries after 10 s, then 1, 2 and 4 min, then every 5 min; and at start);
  servers up to 0.5.x keep being read periodically.
- Web: the version of the Breezone server, at the bottom of the side bar and in the account menu.

### Changed

- App: the Zones / Routines / Schedules sections of an installation are a bottom bar that hides while
  scrolling down; the latency is no longer shown (a green dot when connected, « Offline » otherwise);
  the selected tab label is readable again.

## [0.5.0] - 2026-11-08

### Added

- API versions: `/health` announces `api_version` and `min_client_api`; the app sends its own in the
  `Breezone-Api` header and asks to update the app or the server when they are too far apart (clients
  without the header are still served).
- App: each installation has **Zones**, **Routines** and **Schedules** tabs; the Airzone schedules of
  the installation can be viewed, switched on or off, created, edited and deleted from the app. The
  separate « Routines » tab is gone.
- Web and app: the mode selector of a system sits on the row of its group when all its zones belong to
  that group (in the installation header otherwise).

- App: built-in demo (« Try the demo » on the first screen): a simulated home and workshop, with
  live updates, history and routines, without any server nor Airzone hardware.
  The demo account `https://breezone.cloud` / `demo` / `demo` opens it too (store reviews); no request
  is sent to that address.

- App: Spanish translation (French, English and Spanish, following the phone's language; per-app
  language in Android 13+ and iOS settings).
- App: a server whose HTTPS certificate fails the usual checks can be trusted after reviewing it
  (names, issuer, dates, SHA-256 fingerprint); only that exact certificate is then accepted.

### Changed

- App: the version shown in Settings comes from the build (it showed 0.1.0).
- App: explicit connection errors (invalid certificate, unknown address, refused connection, no
  answer, no network) instead of the raw system message.
- When live updates (WebSocket) are blocked, typically by a reverse proxy without the WebSocket
  headers, the server says so in the activity feed and the app shows « Live updates unavailable »
  with the reason instead of « Connecting… ».

## [0.4.1] - 2026-11-07

### Added

- Choose which of the installations another server shares are shown on this one (Settings ›
  Nodes); installations it shares later are shown until excluded.

## [0.4.0] - 2026-11-07

### Added

- Per-user access: a user only sees the installations an administrator gives them, read-only or
  with control (web, app and live updates); new installations are given to nobody. Existing users
  keep access to every current installation.
- "Add an installation outside Airzone" also offers an installation of another Breezone server.

### Changed

- Read-only installations show their controls disabled in the web UI.
- A reciprocal share tells whether the other server shares back, and how to fix it if not.
- Linking an installation through another server shows when that server still has to accept.

### Fixed

- An installation shared by another server is no longer hidden by a local copy of the same
  WebServer that this server cannot reach (e.g. Airzone account connected on both servers); the
  local copy still wins when it answers.

## [0.3.0] - 2026-11-07

### Added

- Reciprocal sharing: an invitation can ask the other server to share its installations too; the
  invited server chooses which ones, and both directions are linked when the request is accepted.
- Servers behind an HTTPS certificate that fails the usual checks: the certificate is shown (names,
  issuer, dates, SHA-256 fingerprint) and can be trusted; only that exact certificate is then
  accepted, in both directions.

### Fixed

- Clearer errors when another server cannot be reached (certificate not covering the name, unknown
  domain, refused connection).
- A server waiting for acceptance no longer shows a wrong permission and a raw English message.
- The fake WebServers of the demo have distinct MAC addresses.

### Documentation

- Screenshots, a guide to link two sites, the older Synology Docker package, the reverse proxy
  certificate, and a minimal `docker-compose.yml` using the published image.

## [0.2.1] - 2026-11-07

### Changed

- Default port is now 8427 instead of 8080, which is often taken by other applications. Set
  `BREEZONE_PORT` (and `BREEZONE_HTTP_PORT` with compose) to `8080` to keep the former address.

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

[Unreleased]: https://github.com/EmmanuelVinas/breezone/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/EmmanuelVinas/breezone/releases/tag/v0.1.0
