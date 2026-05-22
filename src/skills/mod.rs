//! Skill manifest loader and registry.
//!
//! A skill is a reusable prompt overlay loaded from `.artui/skills/<id>.toml`
//! (project) or `~/.config/artui/skills/<id>.toml` (user). Format compatible
//! with Claude Code's skill format and Vercel AI SDK agent skills:
//!
//! ```toml
//! name = "rust-tdd"
//! description = "Test-driven Rust development"
//! version = "0.1.0"
//! author = "you"
//! triggers = ["test", "tdd", "spec"]
//! prompt = """
//! When writing Rust, always add a #[cfg(test)] mod tests {} block first.
//! Run `cargo test` after every change.
//! """
//! tools = ["read_file", "search", "shell"]   # optional allow-list
//! ```
//!
//! Loaded skills can be activated via `/skill use <id>` — the active skill's
//! `prompt` is appended to the system prompt build in `agent::prompts`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Owned, in-memory skill manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct Skill {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub prompt: String,
    /// Optional tool allow-list. Empty = inherit registry default.
    /// skills.sh calls this `allowed-tools`; both names accepted on load.
    #[serde(default, alias = "allowed-tools")]
    pub tools: Vec<String>,
    /// SPDX-style license id (skills.sh frontmatter).
    #[serde(default)]
    pub license: String,
    /// Free-form compatibility notes (skills.sh frontmatter, ≤500 chars).
    #[serde(default)]
    pub compatibility: String,
    /// Arbitrary skills.sh metadata blob.
    #[serde(default)]
    pub metadata: std::collections::BTreeMap<String, String>,
    /// Origin path for diagnostic messages.
    #[serde(skip)]
    pub source: Option<PathBuf>,
}

/// In-memory skill collection. Built once at startup.
#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: BTreeMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover skills from project + user directories.
    /// Project skills override user skills with the same name.
    pub fn load(workspace_root: &Path) -> Self {
        let mut out = Self::new();
        if let Some(user_dir) = user_skill_dir() {
            out.load_dir(&user_dir);
        }
        out.load_dir(&workspace_root.join(".artui").join("skills"));
        out
    }

    fn load_dir(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // Mattpocock / Claude Code format: skills/<name>/SKILL.md.
            if path.is_dir() {
                let manifest = path.join("SKILL.md");
                if let Some(skill) = read_markdown_skill(&manifest) {
                    self.skills.insert(skill.name.clone(), skill);
                }
                continue;
            }
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase());
            match extension.as_deref() {
                Some("toml") => {
                    if let Some(skill) = read_toml_skill(&path) {
                        self.skills.insert(skill.name.clone(), skill);
                    }
                }
                Some("md") => {
                    if let Some(skill) = read_markdown_skill(&path) {
                        self.skills.insert(skill.name.clone(), skill);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

fn user_skill_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return Some(PathBuf::from(xdg).join("artui").join("skills"));
        }
    }
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".config").join("artui").join("skills"))
}

fn read_toml_skill(path: &Path) -> Option<Skill> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut skill: Skill = toml::from_str(&raw).ok()?;
    skill.source = Some(path.to_path_buf());
    if skill.name.trim().is_empty() {
        return None;
    }
    Some(skill)
}

/// Parse a Mattpocock/Claude-Code-style SKILL.md with YAML frontmatter.
///
/// Format:
/// ```markdown
/// ---
/// name: rust-tdd
/// description: Test-driven Rust development
/// ---
///
/// # Body becomes the prompt overlay…
/// ```
fn read_markdown_skill(path: &Path) -> Option<Skill> {
    let raw = std::fs::read_to_string(path).ok()?;
    let (frontmatter, body) = split_frontmatter(&raw)?;

    let mut skill = Skill {
        name: String::new(),
        description: String::new(),
        version: String::new(),
        author: String::new(),
        triggers: Vec::new(),
        prompt: body.trim().to_owned(),
        tools: Vec::new(),
        license: String::new(),
        compatibility: String::new(),
        metadata: std::collections::BTreeMap::new(),
        source: Some(path.to_path_buf()),
    };

    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().trim_matches('"').trim_matches('\'').to_owned();
        match key.as_str() {
            "name" => skill.name = value,
            "description" => skill.description = value,
            "version" => skill.version = value,
            "author" => skill.author = value,
            "license" => skill.license = value,
            "compatibility" => skill.compatibility = value,
            "triggers" => {
                // Lightweight: accept inline `[a, b]` or comma list `a, b`.
                let cleaned = value.trim_matches(['[', ']'].as_ref());
                skill.triggers = cleaned
                    .split(',')
                    .map(|piece| piece.trim().trim_matches('"').to_owned())
                    .filter(|piece| !piece.is_empty())
                    .collect();
            }
            "tools" | "allowed-tools" => {
                let cleaned = value.trim_matches(['[', ']'].as_ref());
                skill.tools = cleaned
                    .split(',')
                    .map(|piece| piece.trim().trim_matches('"').to_owned())
                    .filter(|piece| !piece.is_empty())
                    .collect();
            }
            other => {
                // Stash unknown frontmatter keys as skills.sh-style metadata
                // so callers can introspect them later.
                skill.metadata.insert(other.to_owned(), value);
            }
        }
    }

    if skill.name.trim().is_empty() {
        // For directory-style skills (path/to/<name>/SKILL.md), prefer the
        // parent directory name; for loose `.md` files, use the file stem.
        let is_skill_md = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case("SKILL.md"))
            .unwrap_or(false);
        let fallback = if is_skill_md {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
        } else {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
        };
        let fallback = fallback.unwrap_or_default();
        if fallback.is_empty() {
            return None;
        }
        skill.name = fallback;
    }
    Some(skill)
}

/// Split `---\n…\n---\nbody` into (frontmatter, body). Returns None when no
/// frontmatter delimiter is present.
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    let trimmed = trimmed.strip_prefix("---")?;
    let trimmed = trimmed.strip_prefix('\n').unwrap_or(trimmed);
    let close = trimmed.find("\n---")?;
    let frontmatter = &trimmed[..close];
    let body = &trimmed[close + 4..];
    let body = body.strip_prefix('\n').unwrap_or(body);
    Some((frontmatter, body))
}

/// Active skill identifier. None = no overlay applied.
#[derive(Debug, Clone, Default)]
pub struct ActiveSkill(pub Option<String>);

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_skill(dir: &Path, file: &str, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(file), body).unwrap();
    }

    #[test]
    fn loads_skill_from_project_dir() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            &tmp.path().join(".artui").join("skills"),
            "rust-tdd.toml",
            r#"
name = "rust-tdd"
description = "TDD discipline"
version = "0.1.0"
author = "you"
triggers = ["tdd"]
prompt = "Always write tests first."
"#,
        );
        let registry = SkillRegistry::load(tmp.path());
        let skill = registry.get("rust-tdd").expect("skill loaded");
        assert_eq!(skill.description, "TDD discipline");
        assert_eq!(skill.triggers, vec!["tdd"]);
        assert_eq!(skill.prompt, "Always write tests first.");
        assert!(skill.source.is_some());
    }

    #[test]
    fn empty_when_no_skills_dir() {
        let tmp = TempDir::new().unwrap();
        let registry = SkillRegistry::load(tmp.path());
        assert!(registry.is_empty());
    }

    #[test]
    fn skips_files_without_name() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            &tmp.path().join(".artui").join("skills"),
            "broken.toml",
            r#"description = "no name""#,
        );
        let registry = SkillRegistry::load(tmp.path());
        assert!(registry.is_empty());
    }

    #[test]
    fn skips_non_toml_files() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            &tmp.path().join(".artui").join("skills"),
            "ignored.md",
            "# not a skill",
        );
        let registry = SkillRegistry::load(tmp.path());
        assert!(registry.is_empty());
    }

    #[test]
    fn iter_returns_loaded_skills() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".artui").join("skills");
        write_skill(&dir, "a.toml", "name = \"a\"\nprompt = \"x\"\n");
        write_skill(&dir, "b.toml", "name = \"b\"\nprompt = \"y\"\n");
        let registry = SkillRegistry::load(tmp.path());
        assert_eq!(registry.len(), 2);
        let names: Vec<_> = registry.iter().map(|s| s.name.clone()).collect();
        assert!(names.contains(&"a".to_owned()));
        assert!(names.contains(&"b".to_owned()));
    }

    #[test]
    fn loads_mattpocock_directory_skill_md() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join(".artui").join("skills").join("diagnose");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: diagnose\ndescription: Disciplined debugging loop\ntriggers: [debug, bug, broken]\n---\n\n# Diagnose\n\nReproduce → minimise → hypothesise → fix.\n",
        )
        .unwrap();

        let registry = SkillRegistry::load(tmp.path());
        let skill = registry.get("diagnose").expect("md skill loaded");
        assert_eq!(skill.description, "Disciplined debugging loop");
        assert_eq!(skill.triggers, vec!["debug", "bug", "broken"]);
        assert!(skill.prompt.contains("Reproduce"));
    }

    #[test]
    fn frontmatter_split_returns_none_without_delimiter() {
        assert!(split_frontmatter("# just markdown").is_none());
    }

    #[test]
    fn loose_md_file_in_skills_dir_uses_filename_when_name_missing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".artui").join("skills");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("anchor.md"),
            "---\ndescription: anchor skill\n---\nbody",
        )
        .unwrap();
        let registry = SkillRegistry::load(tmp.path());
        assert!(registry.get("anchor").is_some(), "filename stem fallback");
    }
}
