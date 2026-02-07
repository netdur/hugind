use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::net::TcpListener;

use crate::core::config::server::ServerConfig;
use crate::server::manager::ServerManager;
use crate::server::routes;
use crate::server::state::AppState;

pub async fn serve(config: ServerConfig) -> Result<()> {
    let host = normalize_host(&config.host);
    let addr: SocketAddr = format!("{}:{}", host, config.port)
        .parse()
        .with_context(|| format!("Invalid server address {}:{}", host, config.port))?;

    let manager = Arc::new(ServerManager::new(config.clone())?);
    let model_name = manager.model_name().to_string();

    let max_slots = config.max_slots as usize;
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_slots));

    let state = Arc::new(AppState {
        model_name,
        api_key: config.api_key.clone(),
        started_at: Instant::now(),
        manager,
        semaphore,
        max_slots,
        waiting: std::sync::atomic::AtomicUsize::new(0),
        active: std::sync::atomic::AtomicUsize::new(0),
    });

    state.manager.start_heartbeat();

    let app = routes::router(state);

    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind {}", addr))?;

    let health_host = if config.host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        &config.host
    };

    println!("Health: http://{}:{}/health", health_host, config.port);
    println!("API base: http://{}:{}/v1", health_host, config.port);

    axum::serve(listener, app).await?;
    Ok(())
}

fn normalize_host(host: &str) -> &str {
    if host.is_empty() {
        "0.0.0.0"
    } else {
        host
    }
}
