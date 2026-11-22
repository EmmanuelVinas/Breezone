# Contributing to Breezone

Thanks for your interest! Issues and pull requests are welcome, in English or French.

## Before you start

- For a bug, open an issue with the version, your Airzone hardware and the logs
  (`docker logs breezone`, without personal information).
- For a larger change, open an issue first to discuss it.
- By contributing, you agree that your contribution is licensed under the
  [PolyForm Noncommercial 1.0.0](LICENSE.md) license of the project, and that the author may also
  license it under other terms (to grant commercial licenses).

## Development

Requirements: Rust (stable), Node 24, Docker; for the app, JDK 21 and Android SDK (Xcode for iOS).

```bash
./scripts/dev.sh                 # fake Airzone + server (cargo) + web UI with hot reload: http://localhost:5173
./scripts/run-local.sh --demo    # the Docker image built from the sources, with a fake Airzone
```

`scripts/dev.sh` options (they combine):

| Option | What it does |
|---|---|
| *(none)* | Fake Airzone WebServer (:3000) and Airzone Cloud (:3900, `demo@breezone.local` / `demo-password`), server on :8427, web UI on :5173; the network scan stays on `127.0.0.0/30` |
| `--real` | No fake Airzone: the real WebServers and the real network, with the settings of `.env` (`BREEZONE_*`) |
| `--reset` | Starts from an empty database (deletes the data folder first) |
| `--modules` | With the fake Airzone: one fake device of each kind (Shelly, Hue, Trydan, Renson, Tasmota, ESPHome, Forecast.Solar on :3100-3106) with 30 days of history, added for the admin; turns the weather on |
| `-h`, `--help` | These options |

The modules shipped with Breezone (WebAssembly, the list `SHIPPED_MODULES` of the `Dockerfile`)
drive the hardware: `dev.sh` always builds them, packs them into `data/dev-modules` and installs
them at start (`BREEZONE_MODULES_DEV`), as in the image (`--shipped`, from before, is still
accepted).

| Variable | What it does |
|---|---|
| `BREEZONE_DEV_PORT` | Port of the server (8427 by default), e.g. 8091 for the mobile E2E test |
| `BREEZONE_DEV_DATA` | Another data folder (`data/dev` by default), e.g. `data/screenshots` for screenshots on a fresh home |
| `BREEZONE_ADMIN_USER`, `BREEZONE_ADMIN_PASSWORD` | The admin created by `--modules` on a new database (`admin` / `motdepasse1` by default) |
| `BREEZONE_MODULES_DEV` | A folder of `.bzm` being written, installed at each start without a signature (development builds only: a release ignores it); set by `dev.sh` |
| `BREEZONE_MODULES_LOOPBACK` | `1`: the modules may reach this machine, where the fake devices answer (development builds only); set by `dev.sh` with the fake Airzone |
| `BREEZONE_WEATHER` | `1` to read the weather (off by default in development) |

Examples: `./scripts/dev.sh --real` tries the shipped modules with your own hardware;
`BREEZONE_DEV_DATA=data/screenshots ./scripts/dev.sh --reset --modules` gives a fresh
demonstration home.

The fake WebServer (`cargo run --bin mock-airzone`) implements the Airzone local API
(`POST/PUT /api/v1/hvac`, `/api/v1/webserver`); `MOCK_CLOUD_PORT=3900` adds a fake Airzone Cloud.

Modules in WebAssembly: see [`modules/README.md`](modules/README.md).

## Checks (also run by CI)

```bash
cd server && cargo clippy --all-targets -- -D warnings && cargo test
cd web && npm run lint && npm run build
cd mobile && ./gradlew :androidApp:assembleDebug :shared:iosSimulatorArm64Test
```

## Conventions

- Commits: [Conventional Commits](https://www.conventionalcommits.org/), one line, at most 100
  characters (`feat(server): …`, `fix(web): …`, `docs: …`).
- Code, identifiers and comments in English; user-facing texts in French (mobile: French and English
  resources in `composeResources`, never hard-coded).
- Never commit secrets, `.env`, keystores or personal data (real MAC addresses, addresses, names).
- Never modify an existing SQL migration (`server/migrations/`): existing databases would refuse to
  start. Add a new one and append its hash in `server/tests/migrations.rs`.
- API changes: update `docs/API.md`, the web types (`web/src/lib/api.ts`) and the mobile models
  (`mobile/shared/.../data/Models.kt`) together.
- Dependencies: Breezone ships the licences of the software it includes. After adding or updating
  one, regenerate them: `node scripts/third-party-licenses.mjs` (server and web, served at
  `/third-party-licenses.txt`) and `cd mobile && node scripts/third-party-licenses.mjs` (the app's
  Settings › Licences). Only permissive licences (MIT, Apache, BSD, ISC…): nothing under GPL, LGPL or
  AGPL, whose terms do not allow the noncommercial restriction of Breezone's licence.

## Android release signing (maintainers)

Create `mobile/keystore.properties` (ignored by git):

```properties
storeFile=/absolute/path/to/upload.jks
storePassword=…
keyAlias=upload
keyPassword=…
```

then `./gradlew :androidApp:bundleRelease`. Bump `breezone.versionCode` / `breezone.versionName` in
`mobile/gradle.properties` for every Play Store upload.
