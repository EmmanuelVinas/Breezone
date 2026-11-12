use breezone_server::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Used by the self-update to check that a downloaded binary runs here.
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("Breezone {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tower_http=warn,sqlx=warn")),
        )
        .init();
    breezone_server::run(Config::from_env()).await
}
