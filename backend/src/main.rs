use std::sync::Arc;
use axum::Router;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use guibiao_backend::{AppState, ClickHouseStore, create_router};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "guibiao_backend=info,tower_http=info,axum=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let clickhouse_url = std::env::var("CLICKHOUSE_URL")
        .unwrap_or_else(|_| "http://localhost:8123".to_string());
    let clickhouse_db = std::env::var("CLICKHOUSE_DB")
        .unwrap_or_else(|_| "guibiao".to_string());
    let server_port = std::env::var("SERVER_PORT")
        .unwrap_or_else(|_| "3000".to_string());

    tracing::info!("Connecting to ClickHouse at {}", clickhouse_url);
    let store = Arc::new(ClickHouseStore::new(&clickhouse_url, &clickhouse_db));
    let app_state = AppState::new(store);

    let app = create_router(app_state);

    let addr = format!("0.0.0.0:{}", server_port);
    tracing::info!("Starting server on {}", addr);
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
