//! Loads a module and asks it about an address (`cargo run --release --features wasm --example
//! wasm_probe -- module.wasm 192.168.1.40`): to try a module by hand, and to weigh the WebAssembly
//! host in a release build.

#[cfg(feature = "wasm")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use breezone_server::drivers::Driver;
    use breezone_server::modules::wasm::{Permissions, WasmModule};
    let mut args = std::env::args().skip(1);
    let (path, host) = (args.next().unwrap_or_default(), args.next().unwrap_or_default());
    let module =
        std::sync::Arc::new(WasmModule::load("module", &std::fs::read(&path)?, Permissions { lan: true, hosts: vec![], loopback: true, memory_mb: 16, accounts: vec![] }, None)?);
    println!("probe: {:?}", module.probe(&host).await);
    let source = module.open(None, &serde_json::json!({ "host": host }), None).await?;
    println!("{:#?}", source.describe().await?);
    Ok(())
}

#[cfg(not(feature = "wasm"))]
fn main() {
    eprintln!("build with --features wasm");
}
