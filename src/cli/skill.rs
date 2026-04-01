use anyhow::{Context, Result, anyhow};
use inquire::Confirm;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::skill;
use crate::shared::paths;

pub async fn install(path: String) -> Result<()> {
    let source_root = if is_url(&path) {
        download_skill(&path).await?
    } else {
        resolve_local_skill_root(&path)?
    };

    let loaded = skill::parse_skill(&source_root)
        .map_err(|e| anyhow!("Invalid skill at {}: {}", source_root.display(), e))?;

    println!("Skill: {}", loaded.config.name);
    println!("Version: {}", loaded.config.version);
    println!("Description: {}", loaded.config.description);
    if !loaded.config.tags.is_empty() {
        println!("Tags: {}", loaded.config.tags.join(", "));
    }

    let dest_dir = paths::skills_dir().join(sanitize_name(&loaded.config.name));
    if dest_dir.exists() {
        let overwrite = Confirm::new(&format!(
            "Skill already exists at {}. Overwrite?",
            dest_dir.display()
        ))
        .with_default(false)
        .prompt()?;
        if !overwrite {
            println!("Installation cancelled.");
            return Ok(());
        }
        fs::remove_dir_all(&dest_dir)
            .with_context(|| format!("Failed to remove {}", dest_dir.display()))?;
    }

    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("Failed to create {}", dest_dir.display()))?;
    copy_dir_recursive(&source_root, &dest_dir)?;

    println!(
        "Installed skill '{}' to {}",
        loaded.config.name,
        dest_dir.display()
    );
    Ok(())
}

pub fn remove(name: String) -> Result<()> {
    let sanitized = sanitize_name(&name);
    let target = paths::skills_dir().join(&sanitized);

    if !target.exists() {
        return Err(anyhow!(
            "Skill '{}' not found at {}",
            name,
            target.display()
        ));
    }

    fs::remove_dir_all(&target)
        .with_context(|| format!("Failed to remove {}", target.display()))?;
    println!("Removed skill '{}'", sanitized);
    Ok(())
}

pub fn list() -> Result<()> {
    let skills = skill::load_all_skills()?;

    if skills.is_empty() {
        println!("No installed skills.");
        return Ok(());
    }

    println!("{:<20} {:<12} {}", "NAME", "VERSION", "DESCRIPTION");
    for s in &skills {
        println!(
            "{:<20} {:<12} {}",
            s.config.name, s.config.version, s.config.description
        );
    }
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn sanitize_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "skill".to_string();
    }
    trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn resolve_local_skill_root(path: &str) -> Result<PathBuf> {
    let target = PathBuf::from(path)
        .canonicalize()
        .with_context(|| format!("Failed to resolve path {}", path))?;
    if target.is_file() {
        let name = target.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "SKILL.md" {
            return Ok(target
                .parent()
                .ok_or_else(|| anyhow!("Invalid SKILL.md path"))?
                .to_path_buf());
        }
        return Err(anyhow!(
            "Expected a folder containing SKILL.md or a direct SKILL.md path"
        ));
    }
    Ok(target)
}

fn is_url(input: &str) -> bool {
    reqwest::Url::parse(input)
        .map(|u| u.scheme() == "http" || u.scheme() == "https")
        .unwrap_or(false)
}

async fn download_skill(url: &str) -> Result<PathBuf> {
    let base_url = resolve_skill_base_url(url)?;
    let skill_md_url = format!("{}/SKILL.md", base_url.as_str().trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .user_agent("Hugind/0.1")
        .build()?;

    let temp = tempfile::tempdir()?;
    let dest = temp.path().to_path_buf();

    let resp = client.get(&skill_md_url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "Failed to download SKILL.md from {}: {}",
            skill_md_url,
            resp.status()
        ));
    }
    let body = resp.text().await?;
    fs::write(dest.join("SKILL.md"), &body)?;

    // Leak the tempdir so it isn't deleted before we copy
    let path = dest.clone();
    std::mem::forget(temp);
    Ok(path)
}

fn resolve_skill_base_url(path: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(path)?;

    // Convert GitHub tree URLs to raw content URLs
    if let Some(host) = url.host_str() {
        if host == "github.com" {
            let segments: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
            if segments.len() >= 4 && segments[2] == "tree" {
                let user = segments[0];
                let repo = segments[1];
                let branch = segments[3];
                let rest = &segments[4..];
                let path_part = rest.join("/");
                let raw_url = format!(
                    "https://raw.githubusercontent.com/{}/{}/{}/{}",
                    user, repo, branch, path_part
                );
                return Ok(reqwest::Url::parse(&raw_url)?);
            }
        }
    }

    Ok(url)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("Failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("Failed to read {}", src.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_file() {
            fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}
