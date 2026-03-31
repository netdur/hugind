use crate::core::model::auth::authenticated_request;
use anyhow::{Result, anyhow};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Sibling {
    rfilename: String,
    #[serde(default)]
    lfs: Option<LfsInfo>,
}

#[derive(Debug, Deserialize)]
struct LfsInfo {
    #[serde(rename = "sha256")]
    sha256: Option<String>,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    siblings: Vec<Sibling>,
}

/// Metadata about a remote GGUF file.
#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub filename: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
}

pub struct RemoteClient;

impl RemoteClient {
    /// Fetch GGUF files from a HuggingFace repo, including SHA256 and size when available.
    pub async fn fetch_repo_files(repo: &str) -> Result<Vec<RemoteFile>> {
        let url = format!("https://huggingface.co/api/models/{}", repo);
        let client = reqwest::Client::new();
        let request = authenticated_request(&client, &url);

        let response = request.send().await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch repo info: Status {}",
                response.status()
            ));
        }

        let info: ModelInfo = response.json().await?;

        let files: Vec<RemoteFile> = info
            .siblings
            .into_iter()
            .filter(|s| s.rfilename.ends_with(".gguf"))
            .map(|s| RemoteFile {
                filename: s.rfilename,
                sha256: s.lfs.as_ref().and_then(|l| l.sha256.clone()),
                size: s.lfs.as_ref().and_then(|l| l.size),
            })
            .collect();

        Ok(files)
    }
}
