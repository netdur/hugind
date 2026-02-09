use anyhow::{Context, Result};
use inquire::Confirm;
use reqwest::Url;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use crate::core::config::agent::{AgentConfig, FileSystemPermission, NetPermissions, Permissions, ShellPermission};
use crate::shared::paths;

pub async fn run(path: String, args_vec: Vec<String>) -> Result<()> {
    let resolved = resolve_agent_path(&path)?;
    let resolved_args = resolve_args_paths(args_vec)?;
    crate::core::orchestrator::execute(resolved, resolved_args).await
}

pub async fn install(path: String) -> Result<()> {
    let (source_root, config, _temp_guard) = if is_url(&path) {
        download_agent(&path).await?
    } else {
        let root = resolve_local_agent_root(&path)?;
        let config = AgentConfig::load_from_dir(&root)?;
        (root, config, None)
    };

    print_permissions(&config.permissions)?;
    let confirm = Confirm::new("Grant these permissions and install this agent?")
        .with_default(false)
        .prompt()?;
    if !confirm {
        println!("Installation cancelled.");
        return Ok(());
    }

    let dest_dir = paths::agents_dir().join(sanitize_agent_name(&config.name));
    if dest_dir.exists() {
        let overwrite = Confirm::new(&format!(
            "Agent already exists at {}. Overwrite?",
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

    println!("✅ Installed agent '{}' to {}", config.name, dest_dir.display());
    Ok(())
}

pub fn remove() -> Result<()> {
    println!("Agent remove not implemented yet");
    Ok(())
}

pub fn list() -> Result<()> {
    let dir = paths::agents_dir();
    if !dir.exists() {
        println!("No installed agents.");
        return Ok(());
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }

    if names.is_empty() {
        println!("No installed agents.");
        return Ok(());
    }

    names.sort();
    println!("{:<24} {}", "NAME", "PATH");
    for name in names {
        let path = dir.join(&name);
        println!("{:<24} {}", name, path.display());
    }
    Ok(())
}

fn is_url(input: &str) -> bool {
    Url::parse(input)
        .map(|u| u.scheme() == "http" || u.scheme() == "https")
        .unwrap_or(false)
}

fn resolve_local_agent_root(path: &str) -> Result<PathBuf> {
    let target = PathBuf::from(path)
        .canonicalize()
        .with_context(|| format!("Failed to resolve path {}", path))?;
    if target.is_file() {
        let name = target.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "agent.yaml" {
            return Ok(target
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Invalid agent.yaml path"))?
                .to_path_buf());
        }
        return Err(anyhow::anyhow!("Expected a folder containing agent.yaml or a direct agent.yaml path"));
    }
    Ok(target)
}

async fn download_agent(path: &str) -> Result<(PathBuf, AgentConfig, Option<TempDir>)> {
    let base_url = if path.ends_with("agent.yaml") {
        Url::parse(path)?
            .join(".")?
    } else {
        let mut url = Url::parse(path)?;
        if !path.ends_with('/') {
            url = Url::parse(&(path.to_string() + "/"))?;
        }
        url
    };

    let agent_url = base_url.join("agent.yaml")?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();

    let agent_yaml = fetch_text(&agent_url).await
        .with_context(|| format!("Failed to download {}", agent_url))?;
    fs::write(root.join("agent.yaml"), agent_yaml)?;

    let config = AgentConfig::load_from_dir(&root)?;
    let entry_url = base_url.join(&config.entry_point)?;
    let entry_path = root.join(&config.entry_point);
    if let Some(parent) = entry_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let entry_bytes = fetch_bytes(&entry_url).await
        .with_context(|| format!("Failed to download {}", entry_url))?;
    fs::write(&entry_path, &entry_bytes)?;

    Ok((root, config, Some(temp)))
}

async fn fetch_text(url: &Url) -> Result<String> {
    let response = reqwest::get(url.clone()).await?.error_for_status()?;
    Ok(response.text().await?)
}

async fn fetch_bytes(url: &Url) -> Result<Vec<u8>> {
    let response = reqwest::get(url.clone()).await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

fn sanitize_agent_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "agent".to_string();
    }
    trimmed
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn print_permissions(perms: &Option<Permissions>) -> Result<()> {
    println!("\nRequested permissions:");
    if perms.is_none() {
        println!("- No special permissions requested");
        return Ok(());
    }

    let perms = perms.as_ref().unwrap();
    if let Some(net) = &perms.network {
        print_net_permissions(net);
    } else {
        println!("- Network access: No");
    }

    if let Some(fs_perm) = &perms.filesystem {
        print_fs_permissions(fs_perm);
    } else {
        println!("- File access: No");
    }

    if let Some(shell) = &perms.shell {
        print_shell_permissions(shell);
    } else {
        println!("- Run system commands: No");
    }

    Ok(())
}

fn print_net_permissions(net: &NetPermissions) {
    if !net.allow {
        println!("- Network access: No");
        return;
    }

    let mut details = Vec::new();
    if !net.allowed_domains.is_empty() {
        details.push(format!("domains: {}", net.allowed_domains.join(", ")));
    }
    if !net.allowed_ips.is_empty() {
        details.push(format!("ips: {}", net.allowed_ips.join(", ")));
    }
    if net.block_private_networks {
        details.push("blocks private networks".to_string());
    }
    if let Some(v) = &net.max_response_bytes {
        details.push(format!("max response: {}", v));
    }
    if let Some(v) = &net.timeout {
        details.push(format!("timeout: {}", v));
    }

    if details.is_empty() {
        println!("- Network access: Yes");
    } else {
        println!("- Network access: Yes ({})", details.join("; "));
    }
}

fn print_fs_permissions(fs_perm: &FileSystemPermission) {
    if !fs_perm.allow {
        println!("- File access: No");
        return;
    }

    let mut actions = Vec::new();
    if fs_perm.read { actions.push("read"); }
    if fs_perm.write { actions.push("write"); }
    if fs_perm.create { actions.push("create"); }
    if fs_perm.delete { actions.push("delete"); }

    let mut details = Vec::new();
    if !actions.is_empty() {
        details.push(format!("actions: {}", actions.join(", ")));
    }
    if !fs_perm.allowed_paths.is_empty() {
        details.push(format!("paths: {}", fs_perm.allowed_paths.join(", ")));
    }
    if !fs_perm.denied_paths.is_empty() {
        details.push(format!("blocked: {}", fs_perm.denied_paths.join(", ")));
    }
    if fs_perm.allow_outside_agent_root {
        details.push("can access outside agent folder".to_string());
    }
    if fs_perm.follow_symlinks {
        details.push("follows symlinks".to_string());
    }

    if details.is_empty() {
        println!("- File access: Yes");
    } else {
        println!("- File access: Yes ({})", details.join("; "));
    }
}

fn print_shell_permissions(shell: &ShellPermission) {
    if !shell.allow {
        println!("- Run system commands: No");
        return;
    }

    let mut details = Vec::new();
    if let Some(list) = &shell.whitelist {
        if !list.is_empty() {
            details.push(format!("allowed: {}", list.join(", ")));
        }
    }
    if let Some(list) = &shell.blacklist {
        if !list.is_empty() {
            details.push(format!("blocked: {}", list.join(", ")));
        }
    }
    if let Some(v) = &shell.timeout {
        details.push(format!("timeout: {}", v));
    }
    if let Some(v) = &shell.max_output {
        details.push(format!("max output: {}", v));
    }
    if shell.env_clear {
        details.push("clears env".to_string());
    }
    if let Some(v) = &shell.working_dir {
        details.push(format!("working dir: {}", v));
    }

    if details.is_empty() {
        println!("- Run system commands: Yes");
    } else {
        println!("- Run system commands: Yes ({})", details.join("; "));
    }
}

fn resolve_agent_path(path: &str) -> Result<String> {
    let input = PathBuf::from(path);
    if input.exists() {
        return Ok(path.to_string());
    }

    let installed = paths::agents_dir().join(path);
    if installed.exists() {
        return Ok(installed.to_string_lossy().to_string());
    }

    Err(anyhow::anyhow!(
        "Error resolving path {}: No such file or directory",
        path
    ))
}

fn resolve_args_paths(args: Vec<String>) -> Result<Vec<String>> {
    let mut resolved = Vec::with_capacity(args.len());
    for arg in args {
        if arg.starts_with('-') {
            resolved.push(arg);
            continue;
        }
        if arg.contains("://") {
            resolved.push(arg);
            continue;
        }
        let path = PathBuf::from(&arg);
        if path.is_absolute() {
            resolved.push(arg);
            continue;
        }
        if path.exists() {
            let abs = path.canonicalize()
                .with_context(|| format!("Failed to resolve path {}", arg))?;
            resolved.push(abs.to_string_lossy().to_string());
        } else {
            resolved.push(arg);
        }
    }
    Ok(resolved)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src)
        .with_context(|| format!("Failed to read {}", src.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_file() {
            fs::copy(&src_path, &dst_path)
                .with_context(|| format!("Failed to copy {} to {}", src_path.display(), dst_path.display()))?;
        }
    }
    Ok(())
}
