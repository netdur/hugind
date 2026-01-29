use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;
use crate::shared::paths;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: serde_json::Value, // Can be string or array (multimodal)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Session {
    pub id: String,
    pub model: String,
    #[serde(default)]
    pub title: Option<String>,
    pub created: String,
    pub last_active: String,
    pub messages: Vec<Message>,
}

#[derive(Debug)]
pub struct SessionInfo {
    pub id: String,
    pub model: String,
    pub title: String,
    pub last_active: DateTime<Utc>,
}

pub struct SessionRepo;

impl SessionRepo {
    fn chats_dir() -> PathBuf {
        paths::data_home().join("chats")
    }

    fn session_file(id: &str) -> PathBuf {
        Self::chats_dir().join(format!("{}.json", id))
    }

    pub fn exists(id: &str) -> bool {
        Self::session_file(id).exists()
    }

    pub fn create(model: &str) -> Result<String> {
        let dir = Self::chats_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }

        let id = format!("session-{}-{}", Utc::now().timestamp_millis(), Uuid::new_v4().as_u128() % 1000);
        let session = Session {
            id: id.clone(),
            model: model.to_string(),
            title: None,
            created: Utc::now().to_rfc3339(),
            last_active: Utc::now().to_rfc3339(),
            messages: vec![],
        };

        let file_path = Self::session_file(&id);
        let json = serde_json::to_string_pretty(&session)?;
        fs::write(file_path, json)?;

        Ok(id)
    }

    pub fn load(id: &str) -> Result<Session> {
        let path = Self::session_file(id);
        if !path.exists() {
            return Err(anyhow!("Session not found: {}", id));
        }
        let content = fs::read_to_string(path)?;
        let session: Session = serde_json::from_str(&content)?;
        Ok(session)
    }

    pub fn save(id: &str, mut session: Session) -> Result<()> {
        session.last_active = Utc::now().to_rfc3339();
        let path = Self::session_file(id);
        let json = serde_json::to_string_pretty(&session)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn list() -> Result<Vec<SessionInfo>> {
        let dir = Self::chats_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut sessions = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                if let Ok(content) = fs::read_to_string(&path) {
                    match serde_json::from_str::<Session>(&content) {
                        Ok(session) => {
                            // Title Logic
                            let title = session.title.clone().unwrap_or_else(|| {
                                // Try to find first user message
                                session.messages.iter()
                                    .find(|m| m.role == "user")
                                    .map(|m| {
                                        match &m.content {
                                            serde_json::Value::String(s) => {
                                                let t = s.trim().replace('\n', " ");
                                                if t.len() > 30 { format!("{}...", &t[0..30]) } else { t }
                                            },
                                            _ => "Image/Multimodal".to_string()
                                        }
                                    })
                                    .unwrap_or_else(|| "New Chat".to_string())
                            });

                             // Date Logic (Handle RFC3339 or Naive)
                             let last_active = Self::parse_date(&session.last_active).unwrap_or_else(|_| Utc::now());

                            sessions.push(SessionInfo {
                                id: session.id,
                                model: session.model,
                                title,
                                last_active,
                            });
                        },
                        Err(_) => {}
                    }
                }
            }
        }
        // Sort newest first
        sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        Ok(sessions)
    }

    fn parse_date(s: &str) -> Result<DateTime<Utc>> {
        // Try RFC3339/ISO8601 with offset
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Ok(dt.with_timezone(&Utc));
        }
        // Try Naive (assume UTC or Local?)
        // Dart's naive format often: "2026-01-12T22:00:30.065888"
        // Try to append Z?
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
             return Ok(DateTime::from_naive_utc_and_offset(dt, Utc));
        }
        Err(anyhow!("Invalid date format"))
    }

    pub fn delete(id: &str) -> Result<()> {
        let path = Self::session_file(id);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}
