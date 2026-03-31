use anyhow::{Context, Result};
use futures_util::stream::{self, StreamExt};
use inquire::{Confirm, MultiSelect, Select, Text};

use crate::core::model::downloader::Downloader;
use crate::core::model::registry::RepoManager;
use crate::core::model::remote::{RemoteClient, RemoteFile};

pub fn list() -> Result<()> {
    let repos = RepoManager::list_repos()?;
    if repos.is_empty() {
        println!("No models found. Run \"hugind model add\" to download one.");
        return Ok(());
    }

    println!("\nDownloaded Repositories:");
    println!("{}", "-".repeat(40));
    for repo in repos {
        println!("{}", repo.full_name());
    }
    println!();
    Ok(())
}

pub fn show(repo: String) -> Result<()> {
    let repo_obj = RepoManager::get_repo(&repo)?;

    let Some(r) = repo_obj else {
        println!("Repository \"{}\" not found locally.", repo);
        return Ok(());
    };

    let files = RepoManager::list_repo_files(&r)?;
    if files.is_empty() {
        println!("Repository is empty.");
    } else {
        println!("\nFiles in {}:", repo);
        println!("{}", "-".repeat(40));
        for file in files {
            println!("{}  ({:.2} MB)", file.name, file.size_mb());
        }
        println!();
    }
    Ok(())
}

pub async fn add(repo_arg: Option<String>, yes: bool) -> Result<()> {
    let repo = if let Some(r) = repo_arg {
        r
    } else {
        Text::new("Enter Hugging Face repository (user/repo):")
            .with_placeholder("TheBloke/Llama-2-7B-Chat-GGUF")
            .prompt()?
    };

    println!("Scanning {} for GGUF files...", repo);
    let remote_files = RemoteClient::fetch_repo_files(&repo).await?;

    if remote_files.is_empty() {
        return Err(anyhow::anyhow!("No GGUF files found in {}.", repo));
    }

    // Build display list and let user select
    let display_names: Vec<String> = remote_files
        .iter()
        .map(|f| {
            if let Some(size) = f.size {
                format!("{} ({:.2} GB)", f.filename, size as f64 / 1_073_741_824.0)
            } else {
                f.filename.clone()
            }
        })
        .collect();

    let selected_indices: Vec<usize> = if yes {
        // In non-interactive mode, download all files
        (0..remote_files.len()).collect()
    } else {
        let selection = MultiSelect::new("Select files to download:", display_names).prompt()?;
        if selection.is_empty() {
            println!("No files selected.");
            return Ok(());
        }
        // Map selected display names back to indices
        selection
            .iter()
            .filter_map(|s| remote_files.iter().position(|f| {
                let display = if let Some(size) = f.size {
                    format!("{} ({:.2} GB)", f.filename, size as f64 / 1_073_741_824.0)
                } else {
                    f.filename.clone()
                };
                &display == s
            }))
            .collect()
    };

    let selected: Vec<&RemoteFile> = selected_indices.iter().map(|&i| &remote_files[i]).collect();

    // Filter out files that already exist locally with correct size
    let repo_dir = RepoManager::get_repo_dir(&repo)?;
    let to_download: Vec<&RemoteFile> = selected
        .into_iter()
        .filter(|f| {
            let local_path = repo_dir.join(&f.filename);
            if local_path.exists() {
                if let (Some(expected_size), Ok(meta)) = (f.size, std::fs::metadata(&local_path)) {
                    if meta.len() == expected_size {
                        println!("Skipping {} (already downloaded)", f.filename);
                        return false;
                    }
                }
            }
            true
        })
        .collect();

    if to_download.is_empty() {
        println!("All selected files already downloaded.");
        return Ok(());
    }

    println!("\nDownloading {} file(s)...", to_download.len());

    // Download up to 2 files concurrently
    let results: Vec<Result<std::path::PathBuf>> = stream::iter(to_download.iter().map(|f| {
        let repo = repo.clone();
        let filename = f.filename.clone();
        let sha = f.sha256.clone();
        async move { Downloader::download_file(&repo, &filename, sha.as_deref()).await }
    }))
    .buffer_unordered(2)
    .collect()
    .await;

    let mut had_error = false;
    for result in results {
        if let Err(e) = result {
            eprintln!("Error: {}", e);
            had_error = true;
        }
    }

    if had_error {
        return Err(anyhow::anyhow!("Some downloads failed"));
    }

    println!("\nDone.");
    Ok(())
}

pub fn remove(repo_arg: Option<String>, yes: bool) -> Result<()> {
    let repo = if let Some(r) = repo_arg {
        r
    } else {
        let repos = RepoManager::list_repos()?;
        if repos.is_empty() {
            println!("No repositories to remove.");
            return Ok(());
        }
        let options: Vec<String> = repos.iter().map(|r| r.full_name()).collect();
        Select::new("Select repository to remove/clean:", options).prompt()?
    };

    let repo_obj = RepoManager::get_repo(&repo)?
        .context("Repository does not exist locally")?;

    let files = RepoManager::list_repo_files(&repo_obj)?;

    if files.is_empty() {
        if yes
            || Confirm::new("Repository is empty. Delete folder?")
                .with_default(true)
                .prompt()?
        {
            RepoManager::delete_repo(&repo)?;
            println!("Deleted {}", repo);
        }
        return Ok(());
    }

    if yes {
        RepoManager::delete_repo(&repo)?;
        println!("Deleted repository {}", repo);
        return Ok(());
    }

    if Confirm::new(&format!(
        "Delete entire repository \"{}\" ({} files)?",
        repo,
        files.len()
    ))
    .with_default(true)
    .prompt()?
    {
        RepoManager::delete_repo(&repo)?;
        println!("Deleted repository {}", repo);
        return Ok(());
    }

    let file_names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    let selection = MultiSelect::new("Select specific files to delete:", file_names).prompt()?;

    if selection.is_empty() {
        return Ok(());
    }

    for filename in &selection {
        RepoManager::delete_file(&repo, filename)?;
        println!("Deleted {}", filename);
    }

    // Refresh to check if repo is now empty
    if let Some(updated) = RepoManager::get_repo(&repo)? {
        let remaining = RepoManager::list_repo_files(&updated)?;
        if remaining.is_empty() {
            if Confirm::new("Repository is now empty. Delete folder?")
                .with_default(true)
                .prompt()?
            {
                RepoManager::delete_repo(&repo)?;
                println!("Cleaned up empty folder.");
            }
        }
    }

    Ok(())
}

pub fn migrate() -> Result<()> {
    println!("Migrating models to ~/.hugind/models/ ...");
    let count = crate::core::model::migrate::migrate_models_to_models_dir()?;
    if count == 0 {
        println!("Nothing to migrate. All models are already in the new location.");
    } else {
        println!("Migrated {} repository(ies).", count);
    }
    Ok(())
}
