use anyhow::{Result, anyhow};
use serde::Deserialize;
use reqwest::header::AUTHORIZATION;
use crate::core::config::settings::GlobalSettings;

#[derive(Debug, Deserialize)]
struct Sibling {
    rfilename: String,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    siblings: Vec<Sibling>,
}

pub struct RemoteClient;

impl RemoteClient {
    pub async fn fetch_repo_files(repo: &str) -> Result<Vec<String>> {
        let url = format!("https://huggingface.co/api/models/{}", repo);
        let client = reqwest::Client::new();
        
        let mut request = client.get(&url);
        
        if let Ok(settings) = GlobalSettings::load() {
            if let Some(token) = settings.get("hf_token") {
                if !token.is_empty() {
                    request = request.header(AUTHORIZATION, format!("Bearer {}", token));
                }
            }
        }

        let response = request.send().await?;
        
        if !response.status().is_success() {
             return Err(anyhow!("Failed to fetch repo info: Status {}", response.status()));
        }

        let info: ModelInfo = response.json().await?;
        
        
        let files: Vec<String> = info.siblings
            .into_iter()
            .map(|s| s.rfilename)
            .filter(|f| f.ends_with(".gguf"))
            .collect();
            
        Ok(files)
    }
}
