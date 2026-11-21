# Writing a module for Breezone

A module drives hardware (or reaches a service) for Breezone. It is a WebAssembly component
(`wasm32-wasip2`) with a manifest, packed in a `.bzm` and signed. Breezone loads it only when a
source uses it, in a sandbox: no files, no environment, and only the network its manifest allows.

## Start from the SDK

`modules/sdk` (`breezone-module-sdk`) does the plumbing: implement its `Driver` trait for your
source, in plain Rust and `serde_json`, and declare it with `module!`. The calls you do not need
answer « unsupported » by themselves; what you ask of Breezone goes through `sdk::host`
(`get_json`, `request`, `subscribe`, `store_get`, `account`, `pressed`, `changed`…).

```rust
use breezone_module_sdk::{device, host, module, Described, Driver, Error, Result, State, Value};

pub struct Plug { base: String }

impl Driver for Plug {
    fn open(config: Value, _secret: Option<Value>) -> Result<Self> {
        let host = config["host"].as_str().ok_or_else(|| Error::Invalid("the address is required".into()))?;
        Ok(Plug { base: format!("http://{host}") })
    }
    fn describe(&self) -> Result<Described> {
        Ok(Described { model: "Plug".into(), firmware: None, identity: None,
                       devices: vec![device("relay", "switch.relay", &["switch.relay@1"], "Plug")] })
    }
    fn read(&self) -> Result<Vec<(String, State)>> {
        let (_, v) = host::get_json(&format!("{}/status", self.base), 5000)?;
        Ok(vec![("relay".into(), serde_json::json!({"on": v["on"]}).as_object().unwrap().clone())])
    }
    fn act(&self, _reference: &str, _trait_name: &str, _action: &str, params: &State) -> Result<()> {
        let on = params["on"].as_bool().unwrap_or(false);
        host::request("POST", &format!("{}/relay?on={on}", self.base), &[], None, 5000).map(|_| ())
    }
}

module!(Plug);
```

`Cargo.toml`: `crate-type = ["cdylib", "rlib"]`, and `breezone-module-sdk` as a dependency.

**Test it natively** with the bench of the SDK (`breezone_module_sdk::testing`): recorded HTTP
answers (`answer`, `answer_json`), the requests made (`requests`), and `check(&source, manifest)`,
which reads the source and checks it against the catalog of traits and your manifest (traits
provided, types, bounds, required fields). `modules/tasmota/src/lib.rs` has examples;
`cargo test -p breezone-module-<id>`.

## The pieces

- `server/contracts/wit/breezone-module.wit`: the interface. The host gives `log`, `now`, `http`,
  `subscribe` / `send` / `unsubscribe` (WebSocket or TCP kept open by the host, each message
  handed to `on-message`), `store-get` / `store-set` (small values of the source), `account`,
  `pressed`, `changed`. The module gives a `source` resource (`open`, `describe`, `read`, `act`,
  `on-message`, `diagnostic`, and the optional calls) and `probe`.
- `server/contracts/module.schema.json`: the manifest (`manifest.json`), with
  `server/contracts/module.example.json` as an example. The traits a module provides are those
  of `server/contracts/traits/`.
- `modules/tasmota/`: a complete module on the SDK, with its tests, to start from.
- `modules/testing/misbehave`: a module written on the raw interface (no SDK), which misbehaves
  on purpose for the tests of the host.

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
