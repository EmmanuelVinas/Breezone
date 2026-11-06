## What and why

<!-- What does this change, and why? Link the issue if any. -->

## Checklist

- [ ] `cd server && cargo clippy --all-targets -- -D warnings && cargo test`
- [ ] `cd web && npm run lint && npm run build`
- [ ] `cd mobile && ./gradlew :androidApp:assembleDebug` (if the app changed)
- [ ] New texts of the mobile app are in `composeResources` (French and English)
- [ ] `docs/API.md` updated if the API changed
