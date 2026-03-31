use crate::core::model::auth::authenticated_request;
use crate::core::model::registry::RepoManager;
use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub struct Downloader;

pub trait ProgressSink: Send + Sync {
    fn on_start(&self, repo: &str, filename: &str, total_bytes: Option<u64>);
    fn on_progress(&self, repo: &str, filename: &str, downloaded: u64, total_bytes: Option<u64>);
    fn on_finish(&self, repo: &str, filename: &str, final_path: &PathBuf);
}

impl Downloader {
    pub async fn download_file(repo: &str, filename: &str, expected_sha256: Option<&str>) -> Result<PathBuf> {
        Self::download_file_with_sink(repo, filename, expected_sha256, None).await
    }

    pub async fn download_file_with_sink(
        repo: &str,
        filename: &str,
        expected_sha256: Option<&str>,
        sink: Option<&dyn ProgressSink>,
    ) -> Result<PathBuf> {
        let url = format!("https://huggingface.co/{}/resolve/main/{}", repo, filename);
        let repo_dir = RepoManager::get_repo_dir(repo)?;

        if !repo_dir.exists() {
            fs::create_dir_all(&repo_dir)?;
        }

        let final_path = repo_dir.join(filename);
        let part_path = repo_dir.join(format!("{}.part", filename));

        // Resume: check how much of the .part file we already have
        let mut downloaded: u64 = 0;
        if part_path.exists() {
            downloaded = fs::metadata(&part_path)?.len();
        }

        let client = reqwest::Client::new();
        let mut request = authenticated_request(&client, &url);

        // Request remaining bytes if we have a partial download
        if downloaded > 0 {
            request = request.header("Range", format!("bytes={}-", downloaded));
        }

        let response = request.send().await?;

        // If server returns 416 Range Not Satisfiable, the file is already complete
        if response.status().as_u16() == 416 {
            // .part file is already the full size — treat as complete
        } else if !response.status().is_success()
            && response.status().as_u16() != 206 /* Partial Content */
        {
            return Err(anyhow!(
                "Failed to download file: Status {}",
                response.status()
            ));
        } else {
            // If server doesn't support range (returns 200 instead of 206), restart
            if downloaded > 0 && response.status().as_u16() == 200 {
                downloaded = 0;
                if part_path.exists() {
                    fs::remove_file(&part_path)?;
                }
            }

            let total_size = response.content_length().map(|cl| cl + downloaded);

            let pb = if sink.is_none() {
                let total = total_size.unwrap_or(0);
                let pb = ProgressBar::new(total);
                pb.set_style(ProgressStyle::default_bar()
                    .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                    .progress_chars("#>-"));
                pb.set_message(format!("Downloading {}", filename));
                pb.set_position(downloaded);
                Some(pb)
            } else {
                None
            };

            if let Some(sink) = sink {
                sink.on_start(repo, filename, total_size);
            }

            // Open in append mode for resume, create if new
            let mut file = if downloaded > 0 {
                OpenOptions::new().append(true).open(&part_path)
                    .context("Failed to open .part file for resume")?
            } else {
                File::create(&part_path).context("Failed to create .part file")?
            };

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
            } else if let Some(pb) = pb {
                pb.finish_with_message(format!("Downloaded {}", filename));
            }
        }

        // Verify integrity if SHA256 was provided
        if let Some(expected) = expected_sha256 {
            let actual = sha256_file(&part_path)?;
            if actual != expected {
                fs::remove_file(&part_path)?;
                return Err(anyhow!(
                    "SHA256 mismatch for {}: expected {}, got {}",
                    filename,
                    expected,
                    actual
                ));
            }
        }

        // Atomically move .part to final location
        if final_path.exists() {
            fs::remove_file(&final_path)?;
        }
        fs::rename(&part_path, &final_path)?;

        Ok(final_path)
    }
}

fn sha256_file(path: &PathBuf) -> Result<String> {
    use std::io::Read;
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
