# Writing a module for Breezone

A module drives hardware (or reaches a service) for Breezone. It is a WebAssembly component
(`wasm32-wasip2`) with a manifest, packed in a `.bzm` and signed. Breezone loads it only when a
source uses it, in a sandbox: no files, no environment, and only the network its manifest allows.

## The pieces

- `server/contracts/wit/breezone-module.wit`: the interface. The host gives `log`, `now`, `http`,
  `subscribe` / `send` / `unsubscribe` (WebSocket or TCP kept open by the host, each message
  handed to `on-message`), `store-get` / `store-set` (small values of the source), `account`,
  `pressed`, `changed`. The module gives a `source` resource (`open`, `describe`, `read`, `act`,
  `on-message`, `diagnostic`, and the optional calls) and `probe`.
- `server/contracts/module.schema.json`: the manifest (`manifest.json`), with
  `server/contracts/module.example.json` as an example. The traits a module provides are those
  of `server/contracts/traits/`.
- `modules/tasmota/`: a complete module, to start from.

## Build, pack, try

```
cd modules
cargo build --release --target wasm32-wasip2 -p breezone-module-<id>
cargo run --manifest-path ../server/Cargo.toml --bin module-publish -- \
  pack <id>/manifest.json target/wasm32-wasip2/release/breezone_module_<id>.wasm /tmp/out
```

**In development**, without signing: start a development build of the server with
`BREEZONE_MODULES_DEV=/tmp/out` (for instance `BREEZONE_MODULES_DEV=/tmp/out ./scripts/dev.sh`):
the packages of that folder are installed at each start, marked « in development, not signed ».
A release build never reads that folder.

## Publish your own source of modules

1. A key, kept secret: `openssl genpkey -algorithm ed25519 -out my-modules.pem`; its public half in
   base64: `openssl pkey -in my-modules.pem -pubout -outform DER | tail -c 32 | base64`.
2. The index: `module-publish index <current index.json, or -> <out> <out>/*.bzm` (its number
   grows at each publication: Breezone refuses an older one).
3. Sign each `.message` written by `module-publish`:
   `openssl pkeyutl -sign -inkey my-modules.pem -rawin -in X.message -out sig && base64 -w0 sig > X.sig`.
4. Serve the folder (`index.json`, `index.json.sig`, the `.bzm` and their `.bzm.sig`) over HTTPS, or
   over HTTP on your local network.
5. In Breezone: Settings › Modules › Library › Sources of modules › Add: its address and the public
   key. Its modules show as « unofficial ». A module of another source never replaces a module of
   Breezone, nor a module installed from another source.

Breezone's own modules are published by `.github/workflows/modules.yml` (tag
`module-<id>-v<version>`), signed with the key of Breezone's releases.
