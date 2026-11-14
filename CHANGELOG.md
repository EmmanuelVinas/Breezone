# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.11.0-beta.10] - 2026-11-14

### Added

- Settings › Rooms: the rooms of the house (create, rename, order, delete; the objects of a deleted
  room stay, without a room), and every object of every module (Airzone zones by installation, Hue
  lights, HomeKit shutters…) with its room, « sans pièce seulement » to see what is left to place.
  For Hue, its own rooms are taken from there too.
- A room is created from the room choice itself (« Nouvelle pièce… »), in Settings › Modules and on
  the Shutters page: until now no screen could create one, so only « Sans pièce » was offered.

## [0.11.0-beta.9] - 2026-11-14

### Added

- Rooms of the devices of modules: each device gets its room in Settings › Modules (and a shutter in
  its detail on the Shutters page). « Reprendre les pièces Hue » takes the rooms of the Hue app:
  the missing ones are created by name, the lights not in a room yet go in theirs.
- Hue scenes on the Lights page: those of a room under it, those over several rooms above; one click
  turns one on (`GET /api/scenes`, `POST /api/scenes/{source}/{scene}`).
- Shutters page (web), as in the mockups: shutters by room with « Tout ouvrir » and « Tout fermer »,
  a card per shutter showing how far it is open with ▲ ■ ▼, and the detail of the one chosen
  (position, open, stop, close, its room). In the menu when a module provides shutters.

### Changed

- HomeKit: a device takes its accessory's name (« Rolling shutter switch 1 ») rather than its
  service's (« Window covering »), and its kind says what it is: shutter, roof window or door
  (shown in Settings › Modules).

## [0.11.0-beta.7] - 2026-11-14

### Fixed

- HomeKit pairing of Legrand accessories (Home + Control gateway, Ecometer): they refused Breezone's
  controller identifier, which was not a UUID, and the pairing ended in « Code incorrect » although
  the code was right. Breezone now gives a UUID, as Apple's controllers do. A refusal after the code
  is accepted no longer says the code is wrong.

## [0.11.0-beta.6] - 2026-11-14

### Added

- Export and import of the configuration (Settings › Breezone), to restore a server or move it to
  another machine: installations, modules, devices and rooms, routines and timers, users and their
  rights, a few settings. The file holds no secret (passwords, Airzone account password, HomeKit
  pairings, Hue key, federation keys) and no history. An import replaces that configuration; on the
  same server, the secrets and history of what comes back are kept; elsewhere, a list says what to
  type again.

### Fixed

- A HomeKit pairing started and never finished no longer keeps the accessory busy: it is given up
  after 2 minutes, or as soon as the add dialog closes. An accessory busy with another pairing says
  so instead of « Réponse inattendue ». The `Host` header no longer carries the port, as the
  reference controllers send it.

## [0.11.0-beta.5] - 2026-11-14

### Changed

- Settings: the Nodes and Shares tabs become a single Servers tab, in three sections: « Ce serveur »,
  « À qui ce serveur est partagé » and « Quels serveurs partagent avec celui-ci ». Messages that
  pointed to Settings › Shares or Settings › Nodes now point to Settings › Servers (the old
  `?tab=nodes` and `?tab=shares` links still open it).
- Switches waiting for the server (lamps, relays, charger lock, Airzone schedules, beta channel)
  show a spinner in their thumb instead of turning grey with a “forbidden” cursor.

### Fixed

- HomeKit pairing of an accessory with a screen (Legrand Ecometer): such an accessory shows a new
  code each time a pairing starts, so the code typed beforehand was always refused. « Appairer » now
  starts the pairing first, then asks for the code shown; a wrong code starts a new pairing.
- The scanner no longer offers to pair a Hue bridge through HomeKit: it also answers as a HomeKit
  accessory, and its own driver is found at the same address.

## [0.11.0-beta.4] - 2026-11-14

### Added

- Philips Hue: the bridge is found by the scanner and added with its button (its local API, no Hue
  account), each lamp becomes a `light` device (new trait: on, brightness, white temperature, color,
  only what the lamp has). Lights page (web), as in the mockups: lamps by room with « Tout allumer » and
  « Tout éteindre », a card per lamp tinted with its light, and the detail of the lamp chosen
  (brightness, white temperature, colors).
- HomeKit controller: Breezone pairs with a HomeKit accessory or bridge (a Legrand Home + Control
  gateway, a VELUX gateway…) with its 8-digit code, then reads and drives it on the local network
  through encrypted sessions, without Apple Home or any account. The scanner finds the accessories
  (mDNS, asked to the group and to each address), « Ajouter un module » pairs them. Shutters, windows
  and doors are `cover.position` devices (new trait: opening, target, movement; open to, stop),
  outlets, switches and lights `switch.relay`. Pairing changes nothing else on the accessory;
  removing the module removes Breezone's own pairing, and only it.
- A module whose address changed is found again: each source keeps the identity of its hardware
  (Shelly id, Trydan id, Hoymiles serial number), learnt at its first reading when it was added by its
  address. Unreachable for 3 minutes, it is looked for on the network by this identity (at most every
  15 minutes), and its address follows; the move is in the activity. « Rechercher » in Settings ›
  Modules does it at once (`/api/sources/{id}/locate`).

## [0.11.0-beta.3] - 2026-11-14

### Added

- Solar page (web), for the detail: the production now and today, self-consumption and what is
  returned, each inverter with its temperature, voltage and frequency, its panels with their power,
  voltage, current and production, the power of each panel along the day (a curve per panel), today's
  production beside the consumption, and the last 30 days. The energy page keeps the solar production
  (figures, flow, curve, days) and leads to it.
- Device history over a period (`/api/devices/{id}/history?from=&until=&bucket=`), e.g. a local day.
- The menu in sections, as in the mockups: Home (energy, solar, charging), Automations, Server
  (modules, settings).

### Fixed

- A solar inverter is the solar production of the home without being told; a meter on its line
  (a Shelly channel) can say which inverter it measures, and both count once. A measure linked to an
  inverter counts as a production, whatever role it was given; a production that does not say what it
  measures leaves the inverters out unless they are told to count (another installation).
- A source that misses a reading (a Hoymiles DTU serves one client at a time) stays online with its
  last values for a minute, or three readings, instead of showing as unreachable.

## [0.11.0-beta.2] - 2026-11-14

### Changed

- Settings › Modules (web), as in the mockups: the list of modules on the left, Airzone first, and
  the detail of the chosen one on the right: its connection (address, search, test, state, firmware,
  last reading), who sets the charging current for a charger, its devices, and what the module reads
  and writes. Airzone is a module like the others: its installations and the Airzone account are in
  its detail; the « Compte Airzone » and « Installations » tabs are gone (old links still lead there).
- Energy page (web): the products measuring in the header, a Year period (bars per month), CSV export,
  and with a solar production, the bars show it used on the spot or returned, as in the mockups.
- Charging page (web): the charger and its maximum power in the header with when it was read, goal
  and estimated end, the solar surplus beside the charging power, charger settings with a link to the
  module, and the states shared by every charger.

## [0.11.0-beta.1] - 2026-11-14

### Added

- Hoymiles micro-inverters through their Wi-Fi DTU (DTU-W100, DTU-Lite-S, DTU-WLite, DTU-Pro), read
  locally every 30 seconds, found by the network scanner. Each inverter gives its AC power, today's and
  total production, temperature and grid frequency, and can be the solar production of the energy page;
  each panel gives its power, voltage, current and production, shown on the energy page. Read only.
  New traits: `solar.inverter`, `solar.panel`.
- V2C Trydan charger (local API), found by the network scanner and added in one click. New trait
  `ev.charger`: state, whether a vehicle is plugged in, session energy and duration, current; actions `start` (with a maximum current
  and an energy to add, after which Breezone stops the charge), `stop`, `set_limit`, `lock` and
  `set_auto_limit` (the charger sets its current itself, V2C dynamic mode). The charger also measures
  its power, so it can be a consumption on the energy page.
- Charging page (web): the state of each charger, start with a maximum current and an energy to add,
  stop, lock, who sets the current (the charger or Breezone), and its power over the last 24 hours.
- Drivers (ADR 0013): a common interface for every source of devices, with Shelly as the first one
  (Plus, Pro, Gen3, Gen4, local RPC API): a Pro 3EM gives a three-phase meter, plugs and relays give
  switchable relays that measure their power. Sources are configured by administrators
  (`/api/sources`), read every 5 seconds, and their devices are listed with the zones in
  `/api/devices`. A single route applies an action of a device's trait whatever its source
  (`/api/devices/{id}/actions`): it drives an Airzone zone as well as a Shelly relay. New traits:
  `energy.power`, `energy.meter`, `switch.relay`.
- Network scanner for every supported brand (`/api/discovery/products`): each address is asked at once
  whether it is an Airzone WebServer or a Shelly, and products are identified down to the model
  (« Shelly Pro 3EM, monophasé : 3 voies indépendantes »), with those already added marked.

- Settings › Modules (web): the sources of devices with their state, « Ajouter un module » that
  scans the network and adds a Shelly in one click (or by its address), and each energy measure
  named and given a role: grid (whole home), solar production, battery, or the consumption of a
  device or a room. The energy page will rely on these roles.
- Device history: every number of the state of a device (power, energy, relay…) is recorded every
  5 minutes, as the mean of the readings since the previous point, and served per device
  (`/api/devices/{id}/history`). Kept as long as the zone history.
- Energy page (web): the home right now (solar, battery, grid, home and its main consumers, with
  flows that move the way the energy goes), the curve of a day every 15 minutes, the energy per day
  over 7 or 30 days, where the energy goes, and the measured devices with their relay. Production,
  self-consumption and self-sufficiency appear with a solar channel and a main meter. Served by
  `/api/energy/live`, `/api/energy/day` and `/api/energy/days`; the page shows in the menu once a
  measure has a role.

### Fixed

- Network scan: devices were missed when many addresses were asked at once (a real device can take
  several hundred milliseconds to accept a connection then); it waits longer per address and asks
  more of them in parallel, about 3 seconds for a /24. The Airzone scan no longer reads every zone
  of each WebServer found before answering.

### Added

- Home model (ADR 0013), with nothing changed in the interface yet: Airzone zones are now also
  devices of the trait `climate.zone@1`, with a stable id, and can be named and placed in rooms
  (`/api/devices`, `/api/rooms`). The catalog of traits (`server/contracts/traits`, served at
  `/api/traits`) is the single contract from which the web and mobile types are generated.

### Changed

- Zone page (web and app): the air quality section only shows when the thermostat has measured the
  air quality now or in the last 24 hours (it was 30 days), so a sensor that stopped measuring does
  not leave an empty section for a month.

## [0.10.1] - 2026-11-14

### Added

- Servers of different versions now tell each other what they can do (ADR 0012), when their link
  opens and again after an update, a rollback or any change. Settings › Nodes and Shares show the
  other server's version (with a « bêta » badge) and whether they understand each other fully,
  partly (with what is missing and which one to update) or no longer. `GET /api/capabilities` gives
  the same for clients; its fingerprint is in `/api/version`.

### Changed

- A zone command with a field the server does not know is refused (`422`, `code: "unsupported"`)
  instead of being applied in part, so that the sender, an app or another server, knows this server
  must be updated. Commands to another server are checked against what it announced before they are
  sent.

## [0.10.0] - 2026-11-13

### Added

- Beta channel: Settings › Breezone › « Recevoir les versions bêta ». A server on it is offered the
  pre-releases (`v1.0.0-beta.1`…) as well, installed from the web UI like any version (signed, undone
  if it does not start); a « bêta » badge marks them. Leaving the channel never goes back to an older
  version.
- Releases: a pre-release tag (any branch, e.g. `beta`) publishes a GitHub pre-release and the
  `:beta` Docker image only; `latest` and stable versions only come from main, and a stable tag
  outside main is refused.

### Fixed

- Web UI: after an update from Settings › Breezone, the page did not reload once the server was back,
  and kept the previous interface. It now waits for the new version, and any open page reloads by
  itself when the server's version changes (checked every minute), after an update from the web UI
  or a new Docker image alike.

## [0.9.1] - 2026-11-13

### Fixed

- Settings › Breezone: a release older than the installed version (an image installed before its
  GitHub release was published) was shown as the latest version; « Breezone est à jour » now shows
  after any check that finds nothing newer.
- Zone page (web and app): the air quality section only shows when the thermostat has measured the
  air quality, now or in the last 30 days.

## [0.9.0] - 2026-11-12

### Added

- Update Breezone from the web UI (#40): **Settings › Breezone** shows the installed version and the
  latest release (checked once a day), and administrators install it in one click. The server and the
  web UI are downloaded from the official repository only, their Ed25519 signature checked against
  the project key compiled in, the database backed up, then the server restarts on the new version in
  a few seconds, without Docker. A version that does not start is undone at the next start (database
  restored, previous version). `BREEZONE_SELF_UPDATE=false` removes the button. The app tells
  administrators when their server has an update. Each release now publishes signed
  `breezone-<version>-linux-<arch>.tar.gz` archives.
- Zone history: heating and cooling periods as bands under the curves (web and app), from the
  recorded demand time, else estimated from the gap to the setpoint.
- `breezone-server --version`.

## [0.8.0] - 2026-11-12

### Added

- Charts (#30): a 24-hour temperature curve on every zone card (web and app), and the comfort of a
  zone day by day on its page: minimum, maximum and mean temperature, share of the time on within
  ±0.5° of the setpoint, time on, over 7 or 30 days.
- Zone history: pinch or drag to zoom (app) or zoom bar (web), touch or hover for every value at that
  time, tap the legend to hide a curve; the air quality of the zone shows as a colored band under the
  curves, and a gap without readings breaks the lines instead of joining them. The comfort chart
  also shows the mean humidity of each day. Charts use libraries (Recharts on the web, Vico in the
  app) instead of hand drawing.
- Air quality (#30), for the thermostats that measure it (other zones show nothing): a leaf on the
  zone card colored by level (good, moderate, poor), a band of the level under the history curves,
  and on the zone page the level now, the purification mode and the share of each level day by day. Read from the zone data the WebServer already sends; its `iaq` endpoint, which
  stops some WebServers from answering, is never called.
- Demand time: an installation can record how long each zone asks for heat or cold, with the
  history (installation settings, off by default). It is set on the server that owns the
  installation; the servers it is shared with follow it. Shown on the comfort chart, it prepares
  the energy features.
- API: `GET /api/installations/{id}/trends`, `GET /api/installations/{id}/zones/{s}/{z}/daily`,
  `record_demand` on installations, `air_quality` on zones, `demand_secs` and `air_quality` in the
  history.

## [0.7.4] - 2026-11-09

### Fixed

- Installations reached through another Breezone server: the latency shown counted the wait of the
  live link (up to 45 s) when the content changed meanwhile, e.g. « 14986 ms » for a fast network.

## [0.7.3] - 2026-11-09

### Fixed

- Airzone schedules of an installation reached through another Breezone server: creating, changing,
  switching on or off and deleting them now goes through the server that owns the installation,
  which writes them with its own Airzone account. Before, this server's account was used, and
  Airzone refused it without a subscription (« An active subscription is required »).

## [0.7.2] - 2026-11-09

### Fixed

- Airzone schedules: switching a schedule on or off failed with « Bad input parameters » when it
  has a setpoint. Breezone sent the schedule as read from Airzone Cloud (setpoints as
  `{celsius, fahrenheit}`, read-only fields); it now sends what the official app sends.

## [0.7.1] - 2026-11-09

### Changed

- Users: rights are given per installation — no access, read-only, control (with or without the
  right to change zone names and groups, routines, Airzone schedules) or administrator of the
  installation (every right, plus its settings: address, Airzone link, deletion, from the
  installation's page). Rights given until now apply to each installation of the user. Server
  administrators keep every right.
- Web and site: link to the Android app on Google Play.

- Web and app: changing the mode of a system asks for a confirmation first (it applies to every
  zone, an accidental tap could switch the house from heating to cooling).
- Security: device sessions last 60 days instead of 365 (still extended at each use).
- Security: sign-in attempts are limited per account and address, per address and per account
  (an attacker elsewhere no longer locks the owner out), the counters cannot fill the memory, and
  at most two passwords are hashed at a time.
- Security: the server sends a Content-Security-Policy, `X-Frame-Options: DENY`,
  `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`.

## [0.7.0] - 2026-11-09

### Added

- Users: an administrator chooses what each user may change on the installations they drive:
  zone names and groups, routines, Airzone schedules (everything allowed by default, as before).
  Driving, running a routine and switching a schedule on or off stay possible; the web and the app
  hide what is not allowed. Server settings stay reserved to administrators.
- Languages: the web UI is in French, English and Spanish, chosen in the profile (My account ›
  Language; automatic follows the browser). The server writes its messages (errors, activity feed,
  connection and synchronisation states) in the language of each reader: the web and the app send
  theirs; the activity feed is stored in the three languages.

## [0.6.1] - 2026-11-09

### Changed

- App: the section bar of an installation stays visible (it no longer hides while scrolling), the
  lists end right above it.
- App: the setpoint in the routine and schedule editors follows the limits of the chosen zones; the
  Airzone password can be shown while typing it.
- App: going back from an installation no longer shows two bottom bars during the animation (the
  section bar switches in place, only the content slides); the home screen loader is the wavy
  Material 3 Expressive one.
- App (Android): the release build is shrunk, optimized and obfuscated by R8 (2 MB APK); exception
  names stay readable in error messages.

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

[Unreleased]: https://github.com/EmmanuelVinas/breezone/compare/v0.10.1...HEAD
[0.10.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.4...v0.8.0
[0.7.4]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.3...v0.7.4
[0.7.3]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.2...v0.7.3
[0.7.2]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/EmmanuelVinas/breezone/releases/tag/v0.1.0
