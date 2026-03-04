use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::core::config::agent::FileSystemPermission;

#[derive(Debug, Clone, Copy)]
pub enum FsOp {
    Read,
    Write,
    Create,
    Delete,
}

#[derive(Debug, Serialize)]
pub struct FsStat {
    pub path: String,
    pub is_file: bool,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub readonly: bool,
    pub created_ms: Option<u128>,
    pub modified_ms: Option<u128>,
}

#[derive(Debug, Clone)]
pub struct FsAccess {
    agent_root: PathBuf,
    perm: FileSystemPermission,
}

impl FsAccess {
    pub fn new(agent_root: PathBuf, perm: Option<FileSystemPermission>) -> Self {
        Self {
            agent_root,
            perm: perm.unwrap_or_default(),
        }
    }

    pub fn cwd(&self) -> PathBuf {
        self.agent_root.clone()
    }

    pub fn exists(&self, path: &str) -> Result<bool> {
        self.check_perm(FsOp::Read)?;
        let resolved = self.resolve_path(path)?;
        Ok(resolved.exists())
    }

    pub fn is_file(&self, path: &str) -> Result<bool> {
        self.check_perm(FsOp::Read)?;
        let resolved = self.resolve_path(path)?;
        Ok(resolved.is_file())
    }

    pub fn is_dir(&self, path: &str) -> Result<bool> {
        self.check_perm(FsOp::Read)?;
        let resolved = self.resolve_path(path)?;
        Ok(resolved.is_dir())
    }

    pub fn realpath(&self, path: &str) -> Result<PathBuf> {
        self.check_perm(FsOp::Read)?;
        let resolved = self.resolve_path(path)?;
        let real = if self.perm.follow_symlinks {
            self.canonicalize_soft(&resolved)?
        } else {
            resolved
        };
        Ok(real)
    }

    pub fn read_text(&self, path: &str) -> Result<String> {
        self.check_perm(FsOp::Read)?;
        let resolved = self.resolve_existing_path(path)?;
        let content = fs::read_to_string(&resolved)
            .with_context(|| format!("failed to read text from {}", resolved.display()))?;
        Ok(content)
    }

    pub fn read_bytes(&self, path: &str) -> Result<Vec<u8>> {
        self.check_perm(FsOp::Read)?;
        let resolved = self.resolve_existing_path(path)?;
        let content = fs::read(&resolved)
            .with_context(|| format!("failed to read bytes from {}", resolved.display()))?;
        Ok(content)
    }

    pub fn write_text(&self, path: &str, data: &str, append: bool) -> Result<()> {
        self.write_bytes(path, data.as_bytes(), append)
    }

    pub fn write_bytes(&self, path: &str, data: &[u8], append: bool) -> Result<()> {
        let resolved = self.resolve_writable_path(path)?;
        if append {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&resolved)
                .with_context(|| format!("failed to open for append {}", resolved.display()))?;
            use std::io::Write;
            file.write_all(data)
                .with_context(|| format!("failed to append to {}", resolved.display()))?;
        } else {
            fs::write(&resolved, data)
                .with_context(|| format!("failed to write {}", resolved.display()))?;
        }
        Ok(())
    }

    pub fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        self.check_perm(FsOp::Read)?;
        let resolved = self.resolve_existing_path(path)?;
        if !resolved.is_dir() {
            bail!("not a directory: {}", resolved.display());
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(&resolved)
            .with_context(|| format!("failed to read dir {}", resolved.display()))?
        {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            entries.push(name);
        }
        Ok(entries)
    }

    pub fn mkdir(&self, path: &str, recursive: bool) -> Result<()> {
        self.check_perm(FsOp::Create)?;
        let resolved = self.resolve_path(path)?;
        if recursive {
            fs::create_dir_all(&resolved)
                .with_context(|| format!("failed to create dir {}", resolved.display()))?;
        } else {
            fs::create_dir(&resolved)
                .with_context(|| format!("failed to create dir {}", resolved.display()))?;
        }
        Ok(())
    }

    pub fn remove(&self, path: &str, recursive: bool) -> Result<()> {
        self.check_perm(FsOp::Delete)?;
        let resolved = self.resolve_existing_path(path)?;
        if resolved.is_dir() {
            if recursive {
                fs::remove_dir_all(&resolved)
                    .with_context(|| format!("failed to remove dir {}", resolved.display()))?;
            } else {
                fs::remove_dir(&resolved)
                    .with_context(|| format!("failed to remove dir {}", resolved.display()))?;
            }
        } else {
            fs::remove_file(&resolved)
                .with_context(|| format!("failed to remove file {}", resolved.display()))?;
        }
        Ok(())
    }

    pub fn rename(&self, src: &str, dst: &str) -> Result<()> {
        self.check_perm(FsOp::Write)?;
        if !self.perm.delete {
            bail!("filesystem delete permission is disabled");
        }
        let src_path = self.resolve_existing_path(src)?;
        let dst_path = self.resolve_path(dst)?;
        if !dst_path.exists() && !self.perm.create {
            bail!("filesystem create permission is disabled");
        }
        fs::rename(&src_path, &dst_path).with_context(|| {
            format!(
                "failed to rename {} -> {}",
                src_path.display(),
                dst_path.display()
            )
        })?;
        Ok(())
    }

    pub fn copy(&self, src: &str, dst: &str) -> Result<()> {
        self.check_perm(FsOp::Read)?;
        if !self.perm.write {
            bail!("filesystem write permission is disabled");
        }
        let src_path = self.resolve_existing_path(src)?;
        let dst_path = self.resolve_path(dst)?;
        if !dst_path.exists() && !self.perm.create {
            bail!("filesystem create permission is disabled");
        }
        fs::copy(&src_path, &dst_path).with_context(|| {
            format!(
                "failed to copy {} -> {}",
                src_path.display(),
                dst_path.display()
            )
        })?;
        Ok(())
    }

    pub fn stat(&self, path: &str) -> Result<FsStat> {
        self.check_perm(FsOp::Read)?;
        let resolved = self.resolve_existing_path(path)?;
        let meta = fs::metadata(&resolved)
            .with_context(|| format!("failed to stat {}", resolved.display()))?;
        let created_ms = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis());
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis());
        Ok(FsStat {
            path: resolved.to_string_lossy().into_owned(),
            is_file: meta.is_file(),
            is_dir: meta.is_dir(),
            size_bytes: meta.len(),
            readonly: meta.permissions().readonly(),
            created_ms,
            modified_ms,
        })
    }

    fn resolve_existing_path(&self, path: &str) -> Result<PathBuf> {
        let resolved = self.resolve_path(path)?;
        if !resolved.exists() {
            bail!("path does not exist: {}", resolved.display());
        }
        Ok(resolved)
    }

    fn resolve_writable_path(&self, path: &str) -> Result<PathBuf> {
        let resolved = self.resolve_path(path)?;
        if resolved.exists() {
            self.check_perm(FsOp::Write)?;
        } else {
            self.check_perm(FsOp::Write)?;
            self.check_perm(FsOp::Create)?;
            if let Some(parent) = resolved.parent() {
                if !parent.exists() {
                    bail!("parent directory does not exist: {}", parent.display());
                }
            }
        }
        Ok(resolved)
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        if path.trim().is_empty() {
            bail!("empty path");
        }

        let raw = Path::new(path);
        let absolute = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            self.agent_root.join(raw)
        };

        let normalized = self.normalize_absolute(&absolute)?;
        if !self.perm.follow_symlinks {
            self.reject_symlink_components(&normalized)?;
        }

        let compare_path = if self.perm.follow_symlinks {
            self.canonicalize_soft(&normalized)?
        } else {
            normalized.clone()
        };

        self.enforce_allowed(&compare_path)?;
        Ok(compare_path)
    }

    fn check_perm(&self, op: FsOp) -> Result<()> {
        if !self.perm.allow {
            bail!("filesystem access is disabled");
        }
        match op {
            FsOp::Read => {
                if !self.perm.read {
                    bail!("filesystem read permission is disabled");
                }
            }
            FsOp::Write => {
                if !self.perm.write {
                    bail!("filesystem write permission is disabled");
                }
            }
            FsOp::Create => {
                if !self.perm.create {
                    bail!("filesystem create permission is disabled");
                }
            }
            FsOp::Delete => {
                if !self.perm.delete {
                    bail!("filesystem delete permission is disabled");
                }
            }
        }
        Ok(())
    }

    fn enforce_allowed(&self, path: &Path) -> Result<()> {
        let denied = self.expand_config_paths(&self.perm.denied_paths)?;
        for deny in denied {
            if path.starts_with(&deny) {
                bail!("path is denied: {}", path.display());
            }
        }

        let mut allowed = self.expand_config_paths(&self.perm.allowed_paths)?;
        if allowed.is_empty() {
            if !self.perm.allow_outside_agent_root {
                allowed.push(self.agent_root.clone());
            }
        }

        if !allowed.is_empty() && !allowed.iter().any(|p| path.starts_with(p)) {
            bail!("path is not within allowed paths: {}", path.display());
        }

        Ok(())
    }

    fn expand_config_paths(&self, paths: &[String]) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for raw in paths {
            let expanded = self.expand_config_path(raw)?;
            let normalized = self.normalize_absolute(&expanded)?;
            let resolved = if self.perm.follow_symlinks && normalized.exists() {
                self.canonicalize_soft(&normalized)?
            } else {
                normalized
            };
            out.push(resolved);
        }
        Ok(out)
    }

    fn expand_config_path(&self, raw: &str) -> Result<PathBuf> {
        let s = raw.trim();
        let expanded = if s.starts_with("$HOME") {
            let home = dirs::home_dir().ok_or_else(|| anyhow!("$HOME is not available"))?;
            let rest = s.trim_start_matches("$HOME");
            home.join(rest.trim_start_matches('/'))
        } else if s.starts_with('~') {
            let home = dirs::home_dir().ok_or_else(|| anyhow!("home dir is not available"))?;
            let rest = s.trim_start_matches('~');
            home.join(rest.trim_start_matches('/'))
        } else {
            PathBuf::from(s)
        };

        let out = if expanded.is_absolute() {
            expanded
        } else {
            self.agent_root.join(expanded)
        };
        Ok(out)
    }

    fn normalize_absolute(&self, path: &Path) -> Result<PathBuf> {
        let mut out = PathBuf::new();
        for comp in path.components() {
            match comp {
                Component::Prefix(p) => out.push(p.as_os_str()),
                Component::RootDir => out.push(Path::new("/")),
                Component::CurDir => {}
                Component::ParentDir => bail!("parent directory '..' is not allowed"),
                Component::Normal(c) => out.push(c),
            }
        }
        Ok(out)
    }

    fn canonicalize_soft(&self, path: &Path) -> Result<PathBuf> {
        if path.exists() {
            return fs::canonicalize(path)
                .with_context(|| format!("failed to canonicalize {}", path.display()));
        }

        let mut cursor = path;
        let mut tail = Vec::new();
        while !cursor.exists() {
            if let Some(name) = cursor.file_name() {
                tail.push(name.to_os_string());
            }
            if let Some(parent) = cursor.parent() {
                cursor = parent;
            } else {
                break;
            }
        }

        let mut canon = fs::canonicalize(cursor)
            .with_context(|| format!("failed to canonicalize {}", cursor.display()))?;
        for part in tail.iter().rev() {
            canon.push(part);
        }
        Ok(canon)
    }

    fn reject_symlink_components(&self, path: &Path) -> Result<()> {
        let mut cur = PathBuf::new();
        for comp in path.components() {
            match comp {
                Component::Prefix(p) => cur.push(p.as_os_str()),
                Component::RootDir => cur.push(Path::new("/")),
                Component::CurDir => {}
                Component::ParentDir => bail!("parent directory '..' is not allowed"),
                Component::Normal(c) => {
                    cur.push(c);
                    if cur.exists() {
                        let meta = fs::symlink_metadata(&cur).with_context(|| {
                            format!("failed to read metadata {}", cur.display())
                        })?;
                        if meta.file_type().is_symlink() {
                            bail!("symlinks are not allowed in path: {}", cur.display());
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
