//! Permission engine — classifies tool calls as Allow/Ask/Deny.
//!
//! Read-only tools are auto-allowed. Write tools default to Ask in Build mode
//! and Deny in Plan mode. Per-tool overrides (`allow` / `ask` / `deny`) come
//! from the `[permissions]` config table.

use std::collections::HashMap;

use tokio::sync::oneshot;

use crate::agent::PrimaryAgent;
use crate::providers::ToolCall;

/// Permission decision for a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Tool may execute without user confirmation.
    Allow,
    /// Tool requires user confirmation before executing.
    Ask,
    /// Tool is denied for this agent/context.
    Deny,
}

impl PermissionDecision {
    fn from_label(label: &str) -> Option<Self> {
        match label.trim().to_ascii_lowercase().as_str() {
            "allow" => Some(Self::Allow),
            "ask" => Some(Self::Ask),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// Tools that are always read-only (allowed without asking).
const READ_ONLY_TOOLS: &[&str] = &["read_file", "glob", "search"];

/// Tools that write to the workspace.
const WRITE_TOOLS: &[&str] = &["apply_patch", "shell"];

/// `lsp` actions that mutate the workspace and therefore must route
/// through the same approval flow as `apply_patch`. Phase N4.
const LSP_MUTATING_ACTIONS: &[&str] = &["rename", "code_actions"];

/// `lsp` actions that are pure reads — same safety class as `read_file`.
const LSP_READONLY_ACTIONS: &[&str] = &[
    "definition",
    "hover",
    "status",
    "references",
    "implementation",
    "type_definition",
    "document_symbols",
    "workspace_symbols",
    "diagnostics",
];

/// Classifies tool calls into permission decisions.
#[derive(Debug, Clone)]
pub struct PermissionEngine {
    agent: PrimaryAgent,
    /// Per-tool override map (`tool_name` → label). Looked up before the
    /// default agent/tool matrix. Populated from `[permissions]` config.
    overrides: HashMap<String, PermissionDecision>,
    /// Session-scope allowlist filled by "allow once / always allow" UI
    /// answers — keyed by `tool_name` so subsequent calls of the same
    /// tool short-circuit to Allow.
    session_allow: HashMap<String, bool>,
}

impl PermissionEngine {
    pub fn new(agent: PrimaryAgent) -> Self {
        Self {
            agent,
            overrides: HashMap::new(),
            session_allow: HashMap::new(),
        }
    }

    /// Build with per-tool overrides parsed from `[permissions]`.
    pub fn with_overrides(agent: PrimaryAgent, raw: &HashMap<String, String>) -> Self {
        let mut overrides = HashMap::new();
        for (k, v) in raw {
            if let Some(decision) = PermissionDecision::from_label(v) {
                overrides.insert(k.clone(), decision);
            }
        }
        Self {
            agent,
            overrides,
            session_allow: HashMap::new(),
        }
    }

    /// Classify a tool call based on overrides + agent mode + tool type.
    pub fn classify(&self, call: &ToolCall) -> PermissionDecision {
        // Session allowlist: user already approved this tool this session.
        if self.session_allow.get(&call.name).copied().unwrap_or(false) {
            return PermissionDecision::Allow;
        }
        // Explicit per-tool override always wins.
        if let Some(decision) = self.overrides.get(&call.name).copied() {
            return decision;
        }

        // The `lsp` tool is multi-action. The same dispatcher serves
        // read-only ops (definition/hover/...) and mutating ops
        // (rename/code_actions with apply). Classify by the action arg
        // so read-only calls don't trigger the approval modal but
        // mutating calls go through the same flow as apply_patch.
        if call.name == "lsp" {
            return self.classify_lsp_call(call);
        }

        // Read-only tools always allowed
        if READ_ONLY_TOOLS.contains(&call.name.as_str()) {
            return PermissionDecision::Allow;
        }
        // Write tools: pi-coding-agent-style default — Build mode
        // auto-allows, Plan still denies. Users who want a Claude
        // Code-style "ask before each write" experience opt in via
        // `[permissions.tools]` overrides:
        //
        //     [permissions.tools]
        //     apply_patch = "ask"
        //     shell       = "ask"
        //
        // Or pass `--strict-permissions` on the CLI.
        if WRITE_TOOLS.contains(&call.name.as_str()) {
            return match self.agent {
                PrimaryAgent::Plan => PermissionDecision::Deny,
                PrimaryAgent::Build => PermissionDecision::Allow,
            };
        }
        // Unknown tools: Ask
        PermissionDecision::Ask
    }

    /// Sub-classifier for `lsp` calls: dispatch on the `action` arg so the
    /// approval flow sees rename/code_actions as Ask-class mutations and
    /// every other action as a read-only allow.
    fn classify_lsp_call(&self, call: &ToolCall) -> PermissionDecision {
        let action = call
            .arguments
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if LSP_READONLY_ACTIONS.contains(&action) {
            return PermissionDecision::Allow;
        }
        if !LSP_MUTATING_ACTIONS.contains(&action) {
            // Unknown action — let the tool surface the friendly error
            // string by Allow-ing the call. The tool's own validator
            // returns "unknown action `X`" which the agent can route
            // around without an approval round-trip.
            return PermissionDecision::Allow;
        }
        // Mutating: code_actions without `apply` is the listing variant
        // (read-only); only the apply variant is a write.
        if action == "code_actions"
            && call
                .arguments
                .get("apply")
                .and_then(|v| v.as_str())
                .map(str::is_empty)
                .unwrap_or(true)
        {
            return PermissionDecision::Allow;
        }
        // True mutation: same gate as apply_patch.
        match self.agent {
            PrimaryAgent::Plan => PermissionDecision::Deny,
            PrimaryAgent::Build => PermissionDecision::Ask,
        }
    }

    /// Update the active agent.
    pub fn set_agent(&mut self, agent: PrimaryAgent) {
        self.agent = agent;
    }

    /// Mark a tool as approved for the rest of the session.
    pub fn approve_for_session(&mut self, tool_name: &str) {
        self.session_allow.insert(tool_name.to_owned(), true);
    }
}

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new(PrimaryAgent::Build)
    }
}

/// Sent to the UI when a tool call needs interactive approval. UI fills
/// in `reply` with a yes/no/always answer; the agent loop blocks on it.
#[derive(Debug)]
pub struct ApprovalPrompt {
    pub call_id: String,
    pub tool_name: String,
    /// One-line summary rendered as the modal title.
    pub title: String,
    /// Optional pre-formatted body (a unified diff for `apply_patch`,
    /// the command line for `shell`, JSON args for unknown tools).
    pub body: String,
    /// Channel the UI uses to send the answer back. Replaced with `None`
    /// once consumed.
    pub reply: oneshot::Sender<ApprovalAnswer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalAnswer {
    /// Allow this single call only.
    Once,
    /// Allow this tool for the rest of the session.
    Session,
    /// Deny this call. Tool sees a `denied_by_user` result.
    Deny,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn read_only_tools_allowed_in_both_modes() {
        for agent in PrimaryAgent::ALL {
            let engine = PermissionEngine::new(agent);
            for name in READ_ONLY_TOOLS {
                let call = ToolCall {
                    id: "c1".to_owned(),
                    name: name.to_string(),
                    arguments: json!({}),
                };
                assert_eq!(engine.classify(&call), PermissionDecision::Allow);
            }
        }
    }

    #[test]
    fn write_tools_allow_in_build_default() {
        // Pi-style default — agent has full write access in Build mode.
        // Users opt into strict prompts via [permissions.tools] overrides.
        let engine = PermissionEngine::new(PrimaryAgent::Build);
        for name in WRITE_TOOLS {
            let call = ToolCall {
                id: "c2".to_owned(),
                name: name.to_string(),
                arguments: json!({}),
            };
            assert_eq!(engine.classify(&call), PermissionDecision::Allow);
        }
    }

    #[test]
    fn ask_override_promotes_write_tool() {
        let mut raw = HashMap::new();
        raw.insert("apply_patch".to_owned(), "ask".to_owned());
        let engine = PermissionEngine::with_overrides(PrimaryAgent::Build, &raw);
        let call = ToolCall {
            id: "c2b".to_owned(),
            name: "apply_patch".to_owned(),
            arguments: json!({}),
        };
        assert_eq!(engine.classify(&call), PermissionDecision::Ask);
    }

    #[test]
    fn write_tools_denied_in_plan() {
        let engine = PermissionEngine::new(PrimaryAgent::Plan);
        for name in WRITE_TOOLS {
            let call = ToolCall {
                id: "c3".to_owned(),
                name: name.to_string(),
                arguments: json!({}),
            };
            assert_eq!(engine.classify(&call), PermissionDecision::Deny);
        }
    }

    #[test]
    fn unknown_tool_asks() {
        let engine = PermissionEngine::new(PrimaryAgent::Build);
        let call = ToolCall {
            id: "c4".to_owned(),
            name: "unknown_tool".to_owned(),
            arguments: json!({}),
        };
        assert_eq!(engine.classify(&call), PermissionDecision::Ask);
    }

    #[test]
    fn override_short_circuits_default() {
        let mut raw = HashMap::new();
        raw.insert("apply_patch".to_owned(), "allow".to_owned());
        let engine = PermissionEngine::with_overrides(PrimaryAgent::Build, &raw);
        let call = ToolCall {
            id: "c5".to_owned(),
            name: "apply_patch".to_owned(),
            arguments: json!({}),
        };
        assert_eq!(engine.classify(&call), PermissionDecision::Allow);
    }

    #[test]
    fn session_allow_persists_until_set_agent() {
        // Default Build allows write tools; override via "ask" to test the
        // path where session-allow short-circuits the engine result.
        let mut raw = HashMap::new();
        raw.insert("apply_patch".to_owned(), "ask".to_owned());
        let mut engine = PermissionEngine::with_overrides(PrimaryAgent::Build, &raw);
        let call = ToolCall {
            id: "c6".to_owned(),
            name: "apply_patch".to_owned(),
            arguments: json!({}),
        };
        assert_eq!(engine.classify(&call), PermissionDecision::Ask);
        engine.approve_for_session("apply_patch");
        assert_eq!(engine.classify(&call), PermissionDecision::Allow);
    }

    #[test]
    fn deny_override_works_for_unknown_tool() {
        let mut raw = HashMap::new();
        raw.insert("dangerous_tool".to_owned(), "deny".to_owned());
        let engine = PermissionEngine::with_overrides(PrimaryAgent::Build, &raw);
        let call = ToolCall {
            id: "c7".to_owned(),
            name: "dangerous_tool".to_owned(),
            arguments: json!({}),
        };
        assert_eq!(engine.classify(&call), PermissionDecision::Deny);
    }

    #[test]
    fn lsp_readonly_actions_auto_allow() {
        let engine = PermissionEngine::new(PrimaryAgent::Build);
        for action in LSP_READONLY_ACTIONS {
            let call = ToolCall {
                id: "c-lsp-ro".to_owned(),
                name: "lsp".to_owned(),
                arguments: json!({"action": *action}),
            };
            assert_eq!(
                engine.classify(&call),
                PermissionDecision::Allow,
                "lsp action `{action}` should be auto-allowed"
            );
        }
    }

    #[test]
    fn lsp_rename_asks_in_build() {
        let engine = PermissionEngine::new(PrimaryAgent::Build);
        let call = ToolCall {
            id: "c-rename".to_owned(),
            name: "lsp".to_owned(),
            arguments: json!({"action": "rename", "path": "src/lib.rs", "line": 1, "column": 4, "new_name": "Foo"}),
        };
        assert_eq!(engine.classify(&call), PermissionDecision::Ask);
    }

    #[test]
    fn lsp_rename_denied_in_plan() {
        let engine = PermissionEngine::new(PrimaryAgent::Plan);
        let call = ToolCall {
            id: "c-rename".to_owned(),
            name: "lsp".to_owned(),
            arguments: json!({"action": "rename", "path": "src/lib.rs", "line": 1, "column": 4, "new_name": "Foo"}),
        };
        assert_eq!(engine.classify(&call), PermissionDecision::Deny);
    }

    #[test]
    fn lsp_code_actions_listing_is_readonly() {
        let engine = PermissionEngine::new(PrimaryAgent::Build);
        // No `apply` arg → listing variant, read-only.
        let listing = ToolCall {
            id: "c-ca-list".to_owned(),
            name: "lsp".to_owned(),
            arguments: json!({"action": "code_actions", "path": "src/lib.rs", "line": 1, "column": 1}),
        };
        assert_eq!(engine.classify(&listing), PermissionDecision::Allow);
    }

    #[test]
    fn lsp_code_actions_apply_asks_in_build() {
        let engine = PermissionEngine::new(PrimaryAgent::Build);
        let apply = ToolCall {
            id: "c-ca-apply".to_owned(),
            name: "lsp".to_owned(),
            arguments: json!({
                "action": "code_actions",
                "path": "src/lib.rs", "line": 1, "column": 1,
                "apply": "Add missing import"
            }),
        };
        assert_eq!(engine.classify(&apply), PermissionDecision::Ask);
    }

    #[test]
    fn lsp_unknown_action_defaults_to_allow_so_tool_can_emit_friendly_error() {
        let engine = PermissionEngine::new(PrimaryAgent::Build);
        let call = ToolCall {
            id: "c-unknown".to_owned(),
            name: "lsp".to_owned(),
            arguments: json!({"action": "telepathy"}),
        };
        // The tool's own validator returns "unknown action `telepathy`",
        // which is more useful than an approval round-trip for what is
        // really a malformed request.
        assert_eq!(engine.classify(&call), PermissionDecision::Allow);
    }

    #[test]
    fn lsp_rename_override_respected() {
        let mut raw = HashMap::new();
        raw.insert("lsp".to_owned(), "deny".to_owned());
        let engine = PermissionEngine::with_overrides(PrimaryAgent::Build, &raw);
        let call = ToolCall {
            id: "c-rename-deny".to_owned(),
            name: "lsp".to_owned(),
            arguments: json!({"action": "rename", "path": "src/lib.rs", "line": 1, "column": 4, "new_name": "Foo"}),
        };
        // Override on the tool name short-circuits the lsp sub-classifier.
        assert_eq!(engine.classify(&call), PermissionDecision::Deny);
    }
}
