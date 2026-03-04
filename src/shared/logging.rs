use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use parking_lot::Mutex;

use crate::shared::paths;

#[derive(Clone)]
pub struct RunLogger {
    path: PathBuf,
    file: Arc<Mutex<std::fs::File>>,
}

impl RunLogger {
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn log_line(&self, line: impl AsRef<str>) {
        let ts = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        if let Some(mut file) = self.file.try_lock() {
            let _ = writeln!(file, "[{}] {}", ts, line.as_ref());
        }
    }
}

pub fn create_agent_logger(agent_name: &str) -> Result<RunLogger> {
    let safe = sanitize_agent_name(agent_name);
    let dir = paths::logs_dir().join("agents").join(&safe);
    std::fs::create_dir_all(&dir)?;

    let ts = Utc::now().format("%Y%m%d_%H%M%S%.3f");
    let path = dir.join(format!("{}.txt", ts));
    create_logger_at_path(path)
}

pub fn create_agent_logger_at(path: impl AsRef<Path>) -> Result<RunLogger> {
    create_logger_at_path(path.as_ref().to_path_buf())
}

fn create_logger_at_path(path: PathBuf) -> Result<RunLogger> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(&path)?;

    Ok(RunLogger {
        path,
        file: Arc::new(Mutex::new(file)),
    })
}

fn sanitize_agent_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "agent".to_string();
    }

    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }

    let cleaned = out.trim_matches('_').to_string();
    if cleaned.is_empty() {
        "agent".to_string()
    } else {
        cleaned
    }
}
