use std::process::Command;
use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;
use serde_json::json;
use sysinfo::System;

use crate::server::state::AppState;

pub async fn monitor(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let uptime_secs = state.started_at.elapsed().as_secs();
    let queue = state.queue_snapshot();

    let (ram_used_mb, ram_total_mb) = ram_usage_mb();
    let vram = vram_usage_mb();

    let metrics = state.manager.service().metrics_snapshot();

    Json(json!({
        "status": "ok",
        "model": state.model_name,
        "uptime_secs": uptime_secs,
        "queue_depth": queue.waiting,
        "active_slots": queue.active,
        "available_slots": queue.available,
        "max_slots": queue.max_slots,
        "ram": {
            "used_mb": ram_used_mb,
            "total_mb": ram_total_mb
        },
        "vram": {
            "used_mb": vram.map(|v| v.0),
            "total_mb": vram.map(|v| v.1)
        },
        "tps": metrics.tps,
        "emitted_tokens": metrics.emitted_tokens,
        "generation_uptime_secs": metrics.uptime_secs
    }))
}

fn ram_usage_mb() -> (u64, u64) {
    let mut sys = System::new();
    sys.refresh_memory();
    let total = sys.total_memory() / 1024;
    let used = (sys.total_memory().saturating_sub(sys.available_memory())) / 1024;
    (used, total)
}

fn vram_usage_mb() -> Option<(u64, u64)> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next()?.trim();
    let mut parts = line.split(',');
    let used = parts.next()?.trim().parse::<u64>().ok()?;
    let total = parts.next()?.trim().parse::<u64>().ok()?;
    Some((used, total))
}
