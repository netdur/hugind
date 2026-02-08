use anyhow::{bail, Result};
use serde::Deserialize;
use std::fs;

use crate::core::config::agent::{AgentConfig, RuntimeSession, SessionMode};
use crate::shared::paths;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default, Clone)]
struct AgentBackend {
    url: Option<String>,
    config: Option<String>,
    session: Option<AgentBackendSession>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct AgentBackendSession {
    mode: Option<SessionMode>,
    id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedBackend {
    pub base_url: String,
    pub health_url: String,
    pub model: Option<String>,
    pub session: Option<RuntimeSession>,
}

pub fn resolve_backend(config: &AgentConfig) -> Result<ResolvedBackend> {
    resolve_backend_internal(config)
}

pub fn prepare_backend(config: &mut AgentConfig) -> Result<ResolvedBackend> {
    let resolved = resolve_backend_internal(config)?;
    config.runtime_session = resolved.session.clone();
    Ok(resolved)
}

fn resolve_backend_internal(config: &AgentConfig) -> Result<ResolvedBackend> {
    let parsed = if let Some(backend) = &config.backend {
        serde_yaml::from_value::<AgentBackend>(backend.clone())
            .map_err(|e| anyhow::anyhow!("Invalid backend config: {}", e))?
    } else {
        AgentBackend::default()
    };

    if config.backend.is_some() && parsed.url.is_none() && parsed.config.is_none() {
        bail!("backend must include either 'url' or 'config'");
    }

    let mut base_url = parsed.url;
    let mut model = parsed.config;

    if base_url.is_none() {
        if let Some(cfg_name) = model.as_deref() {
            base_url = resolve_base_url_from_config(cfg_name);
        }
    }

    if base_url.is_none() {
        base_url = Some("http://127.0.0.1:8080/v1".to_string());
    }

    if model.is_none() {
        model = Some("default".to_string());
    }

    let base_url = base_url.unwrap();
    let trimmed = base_url.trim_end_matches('/');
    let health_base = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
    let health_url = format!("{}/v1/monitor", health_base);

    let session = if let Some(session) = config.runtime_session.clone() {
        Some(session)
    } else {
        resolve_session(parsed.session)?
    };

    Ok(ResolvedBackend {
        base_url,
        health_url,
        model,
        session,
    })
}

fn resolve_session(session: Option<AgentBackendSession>) -> Result<Option<RuntimeSession>> {
    let session = match session {
        Some(s) => s,
        None => return Ok(None),
    };

    let mode = session.mode.unwrap_or_default();
    match mode {
        SessionMode::Stateless => Ok(None),
        SessionMode::Fresh => Ok(Some(RuntimeSession {
            mode,
            id: Some(Uuid::new_v4().to_string()),
        })),
        SessionMode::Resume => {
            let id = session.id.and_then(|s| {
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            });
            if id.is_none() {
                bail!("backend.session.id is required for resume");
            }
            Ok(Some(RuntimeSession { mode, id }))
        }
    }
}

fn resolve_base_url_from_config(config_name: &str) -> Option<String> {
    if config_name.is_empty() {
        return None;
    }

    let config_dir = paths::configs_dir();
    let yml_path = config_dir.join(format!("{}.yml", config_name));
    let yaml_path = config_dir.join(format!("{}.yaml", config_name));

    let path = if yml_path.exists() {
        yml_path
    } else if yaml_path.exists() {
        yaml_path
    } else {
        return None;
    };

    let content = fs::read_to_string(&path).ok()?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    let server = yaml.get("server")?;

    let host = server
        .get("host")
        .and_then(|h| h.as_str())
        .unwrap_or("127.0.0.1");
    let port = server
        .get("port")
        .and_then(|p| p.as_u64())
        .unwrap_or(8080);

    Some(format!("http://{}:{}/v1", host, port))
}
