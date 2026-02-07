use std::path::PathBuf;
use std::fs;
use anyhow::Result;
use crate::shared::paths;

#[derive(Debug, Clone)]
pub struct Repo {
    pub user: String,
    pub name: String,
    pub path: PathBuf,
}

impl Repo {
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.user, self.name)
    }
}

#[derive(Debug, Clone)]
pub struct ModelFile {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

impl ModelFile {
    pub fn size_mb(&self) -> f64 {
        self.size_bytes as f64 / 1_048_576.0
    }
    
    pub fn size_gb(&self) -> f64 {
        self.size_bytes as f64 / 1_073_741_824.0
    }
}

pub struct RepoManager;

impl RepoManager {
    const RESERVED_DIRS: &'static [&'static str] = &[
        "configs", "cache", "temp", "sessions", "agents", "settings", ".DS_Store"
    ];

    pub fn list_repos() -> Result<Vec<Repo>> {
        let root = paths::data_home();
        if !root.exists() {
            return Ok(vec![]);
        }

        let mut repos = Vec::new();
        for user_entry in fs::read_dir(&root)? {
            let user_entry = user_entry?;
            if !user_entry.path().is_dir() { continue; }
            
            let user_name = user_entry.file_name().to_string_lossy().to_string();
            if user_name.starts_with('.') || Self::RESERVED_DIRS.contains(&user_name.as_str()) {
                continue;
            }

            for repo_entry in fs::read_dir(user_entry.path())? {
                let repo_entry = repo_entry?;
                if !repo_entry.path().is_dir() { continue; }
                
                let repo_name = repo_entry.file_name().to_string_lossy().to_string();
                repos.push(Repo {
                    user: user_name.clone(),
                    name: repo_name,
                    path: repo_entry.path(),
                });
            }
        }
        Ok(repos)
    }

    pub fn list_repo_files(repo: &Repo) -> Result<Vec<ModelFile>> {
        let mut files = Vec::new();
        if !repo.path.exists() {
            return Ok(files);
        }

        for entry in fs::read_dir(&repo.path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !name.starts_with('.') && !name.ends_with(".part") {
                        let metadata = entry.metadata()?;
                        files.push(ModelFile {
                            name: name.to_string(),
                            path,
                            size_bytes: metadata.len(),
                        });
                    }
                }
            }
        }
        
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(files)
    }
    pub fn get_repo_dir(repo: &str) -> PathBuf {
        let parts: Vec<&str> = repo.split('/').collect();
        if parts.len() == 2 {
            paths::data_home().join(parts[0]).join(parts[1])
        } else {
             paths::data_home().join(repo)
        }
    }

    pub fn repo_exists(repo: &str) -> bool {
        Self::get_repo_dir(repo).exists()
    }

    pub fn delete_repo(repo: &str) -> Result<()> {
        let dir = Self::get_repo_dir(repo);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        
        if let Some(parent) = dir.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                if entries.count() == 0 {
                    let _ = fs::remove_dir(parent);
                }
            }
        }
        Ok(())
    }

    pub fn delete_file(repo: &str, filename: &str) -> Result<()> {
        let path = Self::get_repo_dir(repo).join(filename);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}
