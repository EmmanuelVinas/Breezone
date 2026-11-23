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
answers (`answer`, `answer_json`), answers computed from the request (`respond`, for a device
that proves something, as a handshake), TCP exchanges (`answer_exchange`, `queue_exchanges`), the time asked to wait (`waited_ms`), the
requests made (`requests`), and `check(&source, manifest)`,
which reads the source and checks it against the catalog of traits and your manifest (traits
provided, types, bounds, required fields). `modules/tasmota/src/lib.rs` has examples;
`cargo test -p breezone-module-<id>`.

## The pieces

- `server/contracts/wit/breezone-module.wit`: the interface. The host gives `log`, `now`, `wait`
  (time a device wants between two requests, 10 s at most), `http` (with `insecure` to accept a
  camera's own certificate, on the local network only: `host::request_insecure`), `exchange` (a
  question and its answer on TCP, read until a length the answer announces, a close, a number of
  bytes or a silence: binary protocols), `subscribe` / `send` / `receive` / `unsubscribe` (WebSocket or TCP
  kept open by the host, each message
  handed to `on-message`), `store-get` / `store-set` (small values of the source), `account`,
  `pressed`, `changed`. The module gives a `source` resource (`open`, `describe`, `read`, `act`,
  `on-message`, `diagnostic`, and the optional calls), `probe`, and `pair` (the steps of the
  appairage when the manifest's `pairing` is `button` or `code`: `Driver::pair` in the SDK,
  `modules/hue/` as an example). Subscriptions take `ws://`, `wss://` (a device of the local
  network may have its own certificate) and `tcp://`.
- An account that is an OAuth 2 one declares `"oauth": {"token_url": "https://…"}` in its
  manifest, with `client_id`, `client_secret` and `refresh_token` in its form: `host::access_token()`
  gives a valid access, which Breezone renews itself (one renewal at a time) and whose new refresh
  token it keeps. The module never writes its account. `modules/netatmo/` is an example; a module
  whose devices come with its account (roles `account` and `driver`) gets its source once the
  account is connected.
- An account that is not OAuth but keeps a session (EZVIZ) stores it with
  `host::account_update(&json!({...}))`: during `check`, it may also change what was typed (a
  password kept as its MD5, a one-time code removed); once kept, never the fields of its form.
  `permissions.network.hosts` takes `*.example.com` for a service whose regions and pictures live
  on subdomains. `modules/ezviz/` is an example.
- A protocol of several questions and answers on a connection kept open (a handshake, then a
  listing) reads the answers during the call with `host::receive(subscription, timeout)`; what
  comes between calls still reaches `on_message`, in order. `modules/esphome/` is an example
  (Noise encryption with `snow` without its default features: they bring `ring`, which does not
  build for WebAssembly).
- A pairing in several steps (`pairing: "code"`: `start`, then `finish` with the code the
  hardware shows) keeps its state in the module's memory (a `thread_local!`): while a step answers
  `Paired::Next`, Breezone keeps the same instance for the next step on the same `host`, two
  minutes at most. Hardware that remembers its controllers lists `forget` in `optional_calls`:
  `Driver::forget(config, secret)` is called when the source is removed (in an instance of its
  own), so that it can be paired again.
  `modules/homekit/` is an example (HAP: SRP, Ed25519, X25519, ChaCha20-Poly1305 in pure Rust).
- Hardware that keeps schedules of its own lists `schedules` in `optional_calls` and implements
  `Driver::schedules`, `save_schedule` and `delete_schedule` (days, time, devices, an action of a
  trait with its parameters, an optional end; `editable: false` with a `note` for those only its
  app can change). Breezone checks the devices and the action against the catalog first.
- A module of the `service` role implements `Service` (`check` of its account, `query` of the
  contracts of its `provides.services`, as `server/contracts/services/` describes them) and ends
  with `module!(service: MyService)` (or `module!(driver: MyDriver, service: MyService)`). Its
  account's fields (the `config` of its manifest) come from `host::account("<its id>")`; Breezone
  checks each answer against the contract and keeps the caches. `modules/rte/` is an example.
- Conditions for the automations: a service module lists in its manifest `conditions`, fields
  like those of a trait's state (`bool`, `enum` with its `values` and `value_labels`, `integer`,
  `number` with its `unit`), adds `conditions@1.0` to `provides.services`, and answers
  `query("conditions@1", "read")` with an object of their values (a field it cannot tell now is
  left out). Breezone reads it every 5 minutes (`read_every_s` to change it) and offers its fields
  to the rules: the yes/no and the choices as conditions and triggers, the numbers as measures
  with a threshold. `modules/rte/` gives today's and tomorrow's Tempo colour and the days left.
- The network scan: a module lists `identify` in `optional_calls` and implements
  `Driver::identify(host, announced)`, which gives a `Found` (model, note, device id, firmware,
  MAC, flags for its appairage, whether it is supported). It is asked for an address where one of
  the ports of `discovery.tcp` is open (`announced` is `None`: it may ask the hardware), or with
  what the hardware announced by mDNS (`discovery.mdns`: service, name, port, TXT; nothing to ask
  it). `modules/homekit/` (mDNS only) and `modules/esphome/` (both) are examples.
- An account may show what it reaches beside its fields: during `check`, the module keeps
  `{"shown": {...}}` with `host::account_update` (the homes of Netatmo, the devices of EZVIZ);
  it is shown with the account, never as part of its secret. A field of its form may also be kept
  as written (a meter number without the spaces typed: `modules/linky/`).
- A service the core uses by itself sets `"internal": true` in its manifest: never switched off,
  not listed in Settings › Accounts and services. `modules/airzone_local/` is one: the core keeps
  the installations and zones of Airzone, the module speaks to the WebServers
  (`airzone.webserver@1`, a document passed as it is: the type `json` of a contract).
- The configuration of a source gets what its manifest's `fills` ask (`server.timezone`) at each
  start, also for a source that came with its account.
- A call computes for 10 s at most; the time it waits in the host (the network, `host::wait`)
  is not counted, so that a slow device (a key checked in 12 s) is not cut.
- Versions of the interface: `driver` and `service` stay as in 1.0, so that a module built
  before still loads on a newer server; a call added later goes in `extras` (each one optional,
  looked up by the host only if the module exports it). A module that uses a function of `host`
  added later requires its version: `"requires": {"api": "^1.1"}` for `host::receive` (1.1),
  and an older server does not offer it. `modules/testing/contract-1-0` is built against the
  1.0 interface, kept as it was, and must keep loading.
- `server/contracts/module.schema.json`: the manifest (`manifest.json`), with
  `server/contracts/module.example.json` as an example. The traits a module provides are those
  of `server/contracts/traits/`.
- The tests of the server run the modules shipped with Breezone (`server/tests/support`): built
  once, packed as in the image, their requests to a service of the Internet sent to a local fake
  (`breezone_server::modules::wasm::redirect_for_tests("api.netatmo.com", "127.0.0.1:port")`,
  development builds only).
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

Breezone's own modules are published by `.github/workflows/modules.yml`, signed with the key of
Breezone's releases:

- **raise the `version` of a module's `manifest.json` and push it on `main`**: that module is built,
  tested, published and tagged (`module-<id>-v<version>`); a change of code without a new version
  is not published (installations would not see it as an update);
- or push a tag `module-<id>-v<version>` (one at a time: GitHub starts no workflow for more than
  three tags pushed at once);
- or run the workflow by hand (`gh workflow run modules.yml -f modules="shelly trydan"`), for
  several modules in one run.

A version already in the library is left as it is.
