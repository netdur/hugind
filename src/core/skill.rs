use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::shared::paths;

#[derive(Debug, Clone, Deserialize)]
pub struct SkillConfig {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LoadedSkill {
    pub config: SkillConfig,
    pub instructions: String,
    pub root_path: PathBuf,
}

/// Parse a SKILL.md file into config + instructions body.
pub fn parse_skill(skill_dir: &Path) -> Result<LoadedSkill> {
    let skill_file = skill_dir.join("SKILL.md");
    if !skill_file.exists() {
        bail!("SKILL.md not found in {}", skill_dir.display());
    }

    let content = std::fs::read_to_string(&skill_file)
        .map_err(|e| anyhow!("Failed to read {}: {}", skill_file.display(), e))?;

    let (config, instructions) = parse_frontmatter(&content)?;

    Ok(LoadedSkill {
        config,
        instructions,
        root_path: skill_dir.to_path_buf(),
    })
}

/// Split YAML frontmatter from markdown body.
fn parse_frontmatter(content: &str) -> Result<(SkillConfig, String)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        bail!("SKILL.md must start with YAML frontmatter (---)");
    }

    let after_first = &trimmed[3..];
    let end = after_first
        .find("\n---")
        .ok_or_else(|| anyhow!("SKILL.md: missing closing --- for frontmatter"))?;

    let yaml_str = &after_first[..end];
    let body = &after_first[end + 4..]; // skip \n---

    let config: SkillConfig = serde_yaml::from_str(yaml_str)
        .map_err(|e| anyhow!("Failed to parse SKILL.md frontmatter: {}", e))?;

    Ok((config, body.trim().to_string()))
}

/// Resolve a skill name to its directory path.
pub fn resolve_skill(name: &str) -> Result<PathBuf> {
    let dir = paths::skills_dir().join(name);
    if dir.exists() && dir.join("SKILL.md").exists() {
        Ok(dir)
    } else {
        bail!("Skill '{}' not found in {}", name, paths::skills_dir().display())
    }
}

/// Load all installed skills from ~/.hugind/skills/.
pub fn load_all_skills() -> Result<Vec<LoadedSkill>> {
    let skills_dir = paths::skills_dir();
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    let entries = std::fs::read_dir(&skills_dir)
        .map_err(|e| anyhow!("Failed to read skills dir: {}", e))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.join("SKILL.md").exists() {
            match parse_skill(&path) {
                Ok(skill) => skills.push(skill),
                Err(e) => {
                    eprintln!(
                        "Warning: failed to load skill at {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }
    }

    skills.sort_by(|a, b| a.config.name.cmp(&b.config.name));
    Ok(skills)
}

/// Build the skill catalog for injection into the system prompt.
/// Contains only names and descriptions, not full instructions.
pub fn build_skill_catalog(skills: &[LoadedSkill]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut out = String::from(
        "\n\n## Available Skills\n\n\
         You have access to the following skills. To activate a skill that is \
         relevant to your current task, use the activate_skill tool.\n\n",
    );

    for skill in skills {
        out.push_str(&format!("- {}: {}\n", skill.config.name, skill.config.description));
    }

    out
}

/// Get the full instructions for a skill by name.
pub fn get_skill_instructions(name: &str) -> Result<String> {
    let dir = resolve_skill(name)?;
    let skill = parse_skill(&dir)?;
    Ok(skill.instructions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_valid() {
        let content = "---\nname: test\nversion: \"1.0\"\ndescription: A test skill\n---\n\nHello world.";
        let (config, body) = parse_frontmatter(content).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.version, "1.0");
        assert_eq!(config.description, "A test skill");
        assert!(config.tags.is_empty());
        assert_eq!(body, "Hello world.");
    }

    #[test]
    fn parse_frontmatter_with_tags() {
        let content = "---\nname: rust\nversion: \"1.0\"\ndescription: Rust dev\ntags: [rust, coding]\n---\n\nUse Result.";
        let (config, body) = parse_frontmatter(content).unwrap();
        assert_eq!(config.tags, vec!["rust", "coding"]);
        assert_eq!(body, "Use Result.");
    }

    #[test]
    fn parse_frontmatter_missing_start() {
        let content = "No frontmatter here.";
        assert!(parse_frontmatter(content).is_err());
    }

    #[test]
    fn parse_frontmatter_missing_end() {
        let content = "---\nname: test\n";
        assert!(parse_frontmatter(content).is_err());
    }

    #[test]
    fn build_catalog_empty() {
        assert_eq!(build_skill_catalog(&[]), "");
    }

    #[test]
    fn build_catalog_formats_skills() {
        let skills = vec![
            LoadedSkill {
                config: SkillConfig {
                    name: "rust".into(),
                    version: "1.0".into(),
                    description: "Rust patterns".into(),
                    tags: vec![],
                },
                instructions: "body".into(),
                root_path: PathBuf::from("/tmp"),
            },
        ];
        let catalog = build_skill_catalog(&skills);
        assert!(catalog.contains("- rust: Rust patterns"));
        assert!(catalog.contains("activate_skill"));
    }
}
