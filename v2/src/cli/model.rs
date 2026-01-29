use anyhow::{Result, Context};
use inquire::{Select, MultiSelect, Confirm, Text};

use crate::core::model::registry::RepoManager;
use crate::core::model::remote::RemoteClient;
use crate::core::model::downloader::Downloader;

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
    if !RepoManager::repo_exists(&repo) {
        println!("Repository \"{}\" not found locally.", repo);
        return Ok(());
    }
    
    // We construct a temporary Repo struct just to query files. 
    // In a cleaner refactor, repo_exists could return the Repo object or we look it up.
    // For now, re-use list_repos or manually construct path is fine, 
    // but list_repo_files expects a Repo reference.
    // Let's modify list_repo_files to take a path or be more flexible? 
    // Or just find it in the list.
    
    let repos = RepoManager::list_repos()?;
    let repo_obj = repos.iter().find(|r| r.full_name() == repo);
    
    if let Some(r) = repo_obj {
        let files = RepoManager::list_repo_files(r)?;
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
    } else {
        // Technically repo_exists check passed, so this shouldn't happen unless race condition
        // or partial state.
        println!("Error locating repository metadata.");
    }
    Ok(())
}

pub async fn add(repo_arg: Option<String>) -> Result<()> {
    let repo = if let Some(r) = repo_arg {
        r
    } else {
        Text::new("Enter Hugging Face repository (user/repo):")
            .with_placeholder("TheBloke/Llama-2-7B-Chat-GGUF")
            .prompt()?
    };

    println!("🔍 Scanning {} for GGUF files...", repo);
    let files = match RemoteClient::fetch_repo_files(&repo).await {
        Ok(f) => f,
        Err(e) => {
            println!("❌ Error fetching files: {}", e);
            return Ok(());
        }
    };

    if files.is_empty() {
        println!("No GGUF files found in {}.", repo);
        return Ok(());
    }

    let selection = MultiSelect::new("Select files to download:", files)
        .prompt()?;

    if selection.is_empty() {
        println!("No files selected.");
        return Ok(());
    }

    println!("\nStarting download for {} file(s)...", selection.len());
    
    for filename in selection {
        if let Err(e) = Downloader::download_file(&repo, &filename).await {
             println!("❌ Error downloading {}: {}", filename, e);
        }
    }
    println!("\nDone.");

    Ok(())
}

pub fn remove(repo_arg: Option<String>) -> Result<()> {
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

    if !RepoManager::repo_exists(&repo) {
        println!("Repository \"{}\" does not exist locally.", repo);
        return Ok(());
    }

    // Get files
    // Again need Repo obj
    let repos = RepoManager::list_repos()?;
    let repo_obj = repos.iter().find(|r| r.full_name() == repo)
        .context("Could not find repo object")?;
    
    let files = RepoManager::list_repo_files(repo_obj)?;

    if files.is_empty() {
         if Confirm::new("Repository is empty. Delete folder?")
            .with_default(true)
            .prompt()? {
                RepoManager::delete_repo(&repo)?;
                println!("🗑️  Deleted {}", repo);
            }
         return Ok(());
    }

    if Confirm::new(&format!("Delete entire repository \"{}\" ({} files)?", repo, files.len()))
        .with_default(true)
        .prompt()? 
    {
        RepoManager::delete_repo(&repo)?;
        println!("🗑️  Deleted repository {}", repo);
        return Ok(());
    }

    // Granular delete
    let file_names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    let selection = MultiSelect::new("Select specific files to delete:", file_names)
        .prompt()?;

    if selection.is_empty() { return Ok(()); }

    for filename in selection {
        RepoManager::delete_file(&repo, &filename)?;
        println!("🗑️  Deleted {}", filename);
    }

    // Check empty
    // Re-query
    if let Ok(remaining) = RepoManager::list_repo_files(repo_obj) {
        if remaining.is_empty() {
            if Confirm::new("Repository is now empty. Delete folder?")
                .with_default(true)
                .prompt()? 
            {
                RepoManager::delete_repo(&repo)?;
                println!("🗑️  Cleaned up empty folder.");
            }
        }
    }

    Ok(())
}
