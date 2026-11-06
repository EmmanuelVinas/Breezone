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
./scripts/dev.sh --real          # with your real WebServers (settings from .env)
./scripts/run-local.sh --demo    # the Docker image built from the sources, with a fake Airzone
```

The fake WebServer (`cargo run --bin mock-airzone`) implements the Airzone local API
(`POST/PUT /api/v1/hvac`, `/api/v1/webserver`); `MOCK_CLOUD_PORT=3900` adds a fake Airzone Cloud.

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
