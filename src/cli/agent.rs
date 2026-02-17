use anyhow::{Context, Result};
use inquire::Confirm;
use reqwest::Url;
use std::fs;
use std::io::{self};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use zip::read::ZipArchive;

use crate::core::config::agent::{AgentConfig, FileSystemPermission, NetPermissions, Permissions, ShellPermission};
use crate::shared::paths;

pub async fn run(path: String, cwd: Option<String>, args_vec: Vec<String>) -> Result<()> {
    let resolved = resolve_agent_path(&path)?;
    let resolved_args = resolve_args_paths(args_vec)?;
    crate::core::orchestrator::execute(resolved, resolved_args, cwd).await
}

pub async fn install(path: String) -> Result<()> {
    let (source_root, config, _temp_guard) = if is_url(&path) {
        if is_zip_path(&path) {
            download_zip_agent(&path).await?
        } else {
            download_agent(&path).await?
        }
    } else if is_zip_path(&path) {
        let root = extract_local_zip_agent(&path)?;
        let config = AgentConfig::load_from_dir(&root)?;
        (root, config, None)
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

pub fn remove(name: String) -> Result<()> {
    let dir = paths::agents_dir();
    let sanitized = sanitize_agent_name(&name);
    let target = dir.join(&sanitized);

    if !target.exists() {
        return Err(anyhow::anyhow!(
            "Agent '{}' not found at {}",
            name,
            target.display()
        ));
    }

    fs::remove_dir_all(&target)
        .with_context(|| format!("Failed to remove {}", target.display()))?;
    println!("Removed agent '{}' from {}", sanitized, dir.display());
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

fn is_zip_path(path: &str) -> bool {
    path.to_lowercase().ends_with(".zip")
}

fn extract_local_zip_agent(path: &str) -> Result<PathBuf> {
    let zip_path = PathBuf::from(path)
        .canonicalize()
        .with_context(|| format!("Failed to resolve zip path {}", path))?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();
    extract_zip(&zip_path, &root)?;
    let agent_root = find_agent_root(&root)?;
    Ok(agent_root)
}

async fn download_agent(path: &str) -> Result<(PathBuf, AgentConfig, Option<TempDir>)> {
    let base_url = resolve_agent_base_url(path)?;

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

async fn download_zip_agent(path: &str) -> Result<(PathBuf, AgentConfig, Option<TempDir>)> {
    let url = Url::parse(path)?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().to_path_buf();
    let zip_path = root.join("agent.zip");

    let bytes = fetch_bytes(&url).await
        .with_context(|| format!("Failed to download {}", url))?;
    fs::write(&zip_path, &bytes)?;

    extract_zip(&zip_path, &root)?;
    let agent_root = find_agent_root(&root)?;
    let config = AgentConfig::load_from_dir(&agent_root)?;
    Ok((agent_root, config, Some(temp)))
}

fn resolve_agent_base_url(path: &str) -> Result<Url> {
    let url = Url::parse(path)?;

    if let Some(raw_base) = github_raw_base(&url) {
        return Ok(raw_base);
    }

    if path.ends_with("agent.yaml") {
        return Ok(url.join(".")?);
    }

    if path.ends_with('/') {
        return Ok(url);
    }

    Url::parse(&(path.to_string() + "/")).map_err(Into::into)
}

fn github_raw_base(url: &Url) -> Option<Url> {
    if url.host_str() != Some("github.com") {
        return None;
    }

    let segments: Vec<_> = url.path_segments()?.collect();
    if segments.len() < 4 {
        return None;
    }

    let owner = segments[0];
    let repo = segments[1];
    let kind = segments[2];

    if kind != "tree" && kind != "blob" {
        return None;
    }

    let branch = segments[3];
    let path_parts = &segments[4..];
    let mut base = format!("https://raw.githubusercontent.com/{}/{}/{}/", owner, repo, branch);

    if !path_parts.is_empty() {
        let mut dir_parts = path_parts.to_vec();
        if kind == "blob" && !dir_parts.is_empty() {
            dir_parts.pop();
        }
        if !dir_parts.is_empty() {
            base.push_str(&dir_parts.join("/"));
            base.push('/');
        }
    }

    Url::parse(&base).ok()
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

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("Failed to open zip {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Invalid zip {}", zip_path.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut outfile = fs::File::create(&out_path)?;
        io::copy(&mut entry, &mut outfile)?;
    }
    Ok(())
}

fn find_agent_root(root: &Path) -> Result<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut found: Option<PathBuf> = None;

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name().and_then(|s| s.to_str()) == Some("agent.yaml") {
                let candidate = path.parent().unwrap_or(root).to_path_buf();
                if let Some(existing) = &found {
                    if existing != &candidate {
                        return Err(anyhow::anyhow!(
                            "Multiple agent.yaml files found in zip; please provide a zip with a single agent"
                        ));
                    }
                } else {
                    found = Some(candidate);
                }
            }
        }
    }

    found.ok_or_else(|| anyhow::anyhow!("agent.yaml not found in zip"))
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
