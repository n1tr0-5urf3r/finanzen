use finanzen::{AppState, Config, db};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "finanzen=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env()?;
    std::fs::create_dir_all(&config.data_dir)?;
    std::fs::create_dir_all(config.data_dir.join("receipts"))?;

    // Runs the migrations and refuses to start if the database role bypasses RLS,
    // because tenant isolation would then be silently inert.
    let db = db::connect(&config).await?;

    let bind = config.bind;
    let state = AppState::new(db, config);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "Finanzen gestartet");
    axum::serve(listener, finanzen::router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("ctrl-c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("Finanzen wird beendet");
}
