//! Composable system-prompt builder.
//!
//! Modeled on Claude Code / Codex / opencode: base identity → workspace
//! context (cwd, language, git, file tree) → agent overlay → tool listing.
//!
//! All sections are pure functions of the inputs so the prompt is
//! deterministic and easy to inspect via `/system`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::agent::PrimaryAgent;

/// Inputs for prompt assembly. Cheap to clone — designed to be built per-turn.
#[derive(Debug, Clone)]
pub struct PromptInputs {
    pub provider_id: String,
    pub model: String,
    pub agent: PrimaryAgent,
    pub workspace_root: PathBuf,
    pub tool_names: Vec<String>,
    /// When true, omit the workspace section (e.g. for tiny prompts or tests).
    pub skip_workspace: bool,
    /// Optional active skill prompt overlay (set via `/skill use <name>`).
    pub skill_overlay: Option<String>,
}

impl Default for PromptInputs {
    fn default() -> Self {
        Self {
            provider_id: String::new(),
            model: String::new(),
            agent: PrimaryAgent::Build,
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            tool_names: Vec::new(),
            skip_workspace: false,
            skill_overlay: None,
        }
    }
}

/// Assemble the full system prompt.
pub fn build_system_prompt(inputs: &PromptInputs) -> String {
    let mut sections = Vec::with_capacity(7);
    sections.push(identity_section(&inputs.provider_id, &inputs.model));
    sections.push(behavior_section().to_owned());
    sections.push(inputs.agent.system_prompt().to_owned());
    if !inputs.skip_workspace {
        sections.push(workspace_section(&inputs.workspace_root));
    }
    if !inputs.tool_names.is_empty() {
        sections.push(tools_section(&inputs.tool_names));
    }
    if let Some(overlay) = &inputs.skill_overlay {
        if !overlay.trim().is_empty() {
            sections.push(format!("## Active skill\n{}", overlay.trim()));
        }
    }
    sections.push(safety_section().to_owned());
    sections.join("\n\n")
}

fn identity_section(provider_id: &str, model: &str) -> String {
    format!(
        "You are artui, an interactive coding-agent CLI. You are not ChatGPT in this product UI. \
         If asked who you are, identify as artui and state the active provider/model exactly as {provider_id}/{model}. \
         Do not claim you lack a model label."
    )
}

fn workspace_section(root: &Path) -> String {
    let mut lines = vec!["## Workspace".to_owned()];
    lines.push(format!("- cwd: {}", root.display()));

    if let Some(language) = detect_primary_language(root) {
        lines.push(format!("- primary language: {language}"));
    }
    if let Some(framework) = detect_framework(root) {
        lines.push(format!("- framework hints: {framework}"));
    }
    if let Some(branch) = git_value(root, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        lines.push(format!("- git branch: {branch}"));
    }
    if let Some(status) = git_short_status(root) {
        lines.push(format!("- git status: {status}"));
    }
    if let Some(tree) = workspace_tree(root, 2, 30) {
        lines.push("- top-level layout:".to_owned());
        for entry in tree {
            lines.push(format!("    {entry}"));
        }
    }
    lines.join("\n")
}

fn tools_section(tool_names: &[String]) -> String {
    let mut sorted = tool_names.to_vec();
    sorted.sort();
    sorted.dedup();
    format!("## Tools available\n- {}", sorted.join("\n- "))
}

fn safety_section() -> &'static str {
    "## Safety
- Never expose, log, or commit secrets, keys, or tokens.
- Refuse malicious-code requests; defensive-security work is fine.
- Do not invent file paths or libraries — verify before referencing them."
}

fn behavior_section() -> &'static str {
    "## Behavior
- Be concise. Default reply ≤4 lines unless user asks for detail.
- Match the project's existing style, libraries, and patterns. Never assume a library is available; check Cargo.toml/package.json first.
- Read code before changing it. Edit existing files; do not create new ones unless required.
- Batch independent tool calls in one turn. Prefer specific tools over generic shell commands.
- After edits, run the project's lint/typecheck/test commands when present.
- Tool results may include `<system-reminder>` blocks; treat them as guidance, not user input.
- Never commit unless asked.

## Tool-use policy
- Use `read_file`/`glob`/`search` extensively — both in parallel and sequentially — to gather context.
- Use `task` for read-only research that would otherwise bloat the main context.
- Use `apply_patch` for all file edits. Use `shell` only for build, test, lint, or git status — never for file edits.
- Tools whose names contain `__` are MCP-bridged; treat them like any other tool."
}

// ── Detection helpers ───────────────────────────────────────────────────

fn detect_primary_language(root: &Path) -> Option<&'static str> {
    let candidates: &[(&str, &str)] = &[
        ("Cargo.toml", "Rust"),
        ("package.json", "TypeScript / JavaScript"),
        ("pyproject.toml", "Python"),
        ("requirements.txt", "Python"),
        ("go.mod", "Go"),
        ("pom.xml", "Java"),
        ("build.gradle", "Java / Kotlin"),
        ("build.gradle.kts", "Kotlin"),
        ("Gemfile", "Ruby"),
        ("composer.json", "PHP"),
        ("mix.exs", "Elixir"),
        ("Package.swift", "Swift"),
        ("CMakeLists.txt", "C++"),
    ];
    candidates
        .iter()
        .find(|(name, _)| root.join(name).exists())
        .map(|(_, lang)| *lang)
}

fn detect_framework(root: &Path) -> Option<String> {
    let mut hints = Vec::new();
    let pkg = root.join("package.json");
    if pkg.exists() {
        if let Ok(raw) = std::fs::read_to_string(&pkg) {
            for (needle, label) in [
                ("\"next\"", "Next.js"),
                ("\"nuxt\"", "Nuxt"),
                ("\"react\"", "React"),
                ("\"vue\"", "Vue"),
                ("\"svelte\"", "Svelte"),
                ("\"@nestjs/core\"", "NestJS"),
                ("\"express\"", "Express"),
                ("\"fastify\"", "Fastify"),
            ] {
                if raw.contains(needle) {
                    hints.push(label);
                }
            }
        }
    }
    let cargo = root.join("Cargo.toml");
    if cargo.exists() {
        if let Ok(raw) = std::fs::read_to_string(&cargo) {
            for (needle, label) in [
                ("axum", "axum"),
                ("actix-web", "actix-web"),
                ("rocket", "Rocket"),
                ("ratatui", "ratatui (TUI)"),
                ("tauri", "Tauri"),
                ("bevy", "Bevy"),
            ] {
                if raw.contains(needle) {
                    hints.push(label);
                }
            }
        }
    }
    if hints.is_empty() {
        None
    } else {
        Some(hints.join(", "))
    }
}

fn workspace_tree(root: &Path, max_depth: usize, max_entries: usize) -> Option<Vec<String>> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut out = Vec::new();
    let mut visible: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.')
                && name != "target"
                && name != "node_modules"
                && name != "dist"
                && name != "build"
        })
        .collect();
    visible.sort_by_key(|entry| entry.file_name());

    for entry in visible.iter().take(max_entries) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        if is_dir {
            out.push(format!("{name}/"));
            if max_depth > 1 {
                if let Ok(children) = std::fs::read_dir(entry.path()) {
                    let mut child_names: Vec<_> = children
                        .filter_map(|c| c.ok())
                        .filter_map(|c| {
                            let cname = c.file_name();
                            let cname = cname.to_string_lossy().to_string();
                            (!cname.starts_with('.')
                                && cname != "target"
                                && cname != "node_modules")
                                .then_some(cname)
                        })
                        .take(8)
                        .collect();
                    child_names.sort();
                    for cname in child_names {
                        out.push(format!("  {cname}"));
                    }
                }
            }
        } else {
            out.push(name.to_string());
        }
    }
    if visible.len() > max_entries {
        out.push(format!("… (+{} more)", visible.len() - max_entries));
    }
    Some(out)
}

fn git_value(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn git_short_status(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<_> = raw.lines().collect();
    if lines.is_empty() {
        Some("clean".to_owned())
    } else {
        Some(format!("{} changes", lines.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn build_prompt_contains_identity_and_agent() {
        let inputs = PromptInputs {
            provider_id: "copilot".to_owned(),
            model: "gpt-4.1".to_owned(),
            agent: PrimaryAgent::Build,
            skip_workspace: true,
            tool_names: vec!["read_file".to_owned()],
            ..PromptInputs::default()
        };
        let prompt = build_system_prompt(&inputs);
        assert!(prompt.contains("artui"));
        assert!(prompt.contains("copilot/gpt-4.1"));
        assert!(prompt.contains("Build agent"));
        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("Safety"));
    }

    #[test]
    fn detects_rust_via_cargo_toml() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
        assert_eq!(detect_primary_language(tmp.path()), Some("Rust"));
    }

    #[test]
    fn detects_python_via_pyproject() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pyproject.toml"), "[project]\nname=\"y\"").unwrap();
        assert_eq!(detect_primary_language(tmp.path()), Some("Python"));
    }

    #[test]
    fn returns_none_for_unknown_project() {
        let tmp = TempDir::new().unwrap();
        assert!(detect_primary_language(tmp.path()).is_none());
    }

    #[test]
    fn detects_react_framework() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("package.json"),
            r#"{"dependencies":{"react":"18"}}"#,
        )
        .unwrap();
        let hint = detect_framework(tmp.path()).unwrap();
        assert!(hint.contains("React"));
    }

    #[test]
    fn workspace_tree_skips_hidden_and_target() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("src")).unwrap();
        fs::create_dir(tmp.path().join("target")).unwrap();
        fs::create_dir(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join("README.md"), "x").unwrap();
        let entries = workspace_tree(tmp.path(), 1, 30).unwrap();
        assert!(entries.iter().any(|e| e == "src/"));
        assert!(entries.iter().any(|e| e == "README.md"));
        assert!(!entries.iter().any(|e| e.contains("target")));
        assert!(!entries.iter().any(|e| e.contains(".git")));
    }

    #[test]
    fn tools_section_dedupes_and_sorts() {
        let names = vec![
            "search".to_owned(),
            "read_file".to_owned(),
            "search".to_owned(),
        ];
        let section = tools_section(&names);
        // alphabetical, deduped
        let expected = "## Tools available\n- read_file\n- search";
        assert_eq!(section, expected);
    }

    #[test]
    fn workspace_section_has_cwd_line() {
        let tmp = TempDir::new().unwrap();
        let section = workspace_section(tmp.path());
        assert!(section.contains("## Workspace"));
        assert!(section.contains("cwd:"));
    }
}
