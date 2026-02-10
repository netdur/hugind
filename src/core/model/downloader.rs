use anyhow::{Result, anyhow, Context};
use std::path::PathBuf;
use std::fs::{self, File};
use std::io::Write;
use reqwest::header::AUTHORIZATION;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use crate::core::config::settings::GlobalSettings;
use crate::core::model::registry::RepoManager;

pub struct Downloader;

pub trait ProgressSink: Send + Sync {
    fn on_start(&self, repo: &str, filename: &str, total_bytes: Option<u64>);
    fn on_progress(&self, repo: &str, filename: &str, downloaded: u64, total_bytes: Option<u64>);
    fn on_finish(&self, repo: &str, filename: &str, final_path: &PathBuf);
}

impl Downloader {
    pub async fn download_file(repo: &str, filename: &str) -> Result<PathBuf> {
        Self::download_file_with_sink(repo, filename, None).await
    }

    pub async fn download_file_with_sink(
        repo: &str,
        filename: &str,
        sink: Option<&dyn ProgressSink>,
    ) -> Result<PathBuf> {
        let url = format!("https://huggingface.co/{}/resolve/main/{}", repo, filename);
        let repo_dir = RepoManager::get_repo_dir(repo);
        
        if !repo_dir.exists() {
            fs::create_dir_all(&repo_dir)?;
        }

        let final_path = repo_dir.join(filename);
        let part_path = repo_dir.join(format!("{}.part", filename));

        
        if part_path.exists() {
            fs::remove_file(&part_path)?;
        }

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
             return Err(anyhow!("Failed to download file: Status {}", response.status()));
        }

        let total_size = response.content_length();
        let mut pb = if sink.is_none() {
            let total = total_size.unwrap_or(0);
            let pb = ProgressBar::new(total);
            pb.set_style(ProgressStyle::default_bar()
                .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"));
            pb.set_message(format!("Downloading {}", filename));
            Some(pb)
        } else {
            None
        };

        if let Some(sink) = sink {
            sink.on_start(repo, filename, total_size);
        }

        let mut file = File::create(&part_path).context("Failed to create .part file")?;
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        while let Some(item) = stream.next().await {
            let chunk = item.map_err(|e| anyhow!("Chunk error: {}", e))?;
            file.write_all(&chunk).context("Error writing chunk")?;
            downloaded += chunk.len() as u64;
            if let Some(sink) = sink {
                sink.on_progress(repo, filename, downloaded, total_size);
            } else if let Some(pb) = pb.as_ref() {
                pb.set_position(downloaded);
            }
        }

        if let Some(sink) = sink {
            sink.on_finish(repo, filename, &final_path);
        } else if let Some(pb) = pb.take() {
            pb.finish_with_message(format!("Downloaded {}", filename));
        }
        
        
        if final_path.exists() {
            fs::remove_file(&final_path)?;
        }
        fs::rename(&part_path, &final_path)?;

        Ok(final_path)
    }
}
