//! Permission engine — classifies tool calls as Allow/Ask/Deny.
//!
//! Phase D scaffold: all read-only tools are auto-allowed.
//! Phase E will add Ask flow for write tools.

use crate::providers::ToolCall;

/// Permission decision for a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Tool may execute without user confirmation.
    Allow,
    /// Tool requires user confirmation before executing (Phase E).
    Ask,
    /// Tool is denied for this agent/context.
    Deny,
}

/// Classifies tool calls into permission decisions.
pub struct PermissionEngine {
    /// Tools that are always allowed without asking.
    read_only_tools: Vec<String>,
}

impl PermissionEngine {
    pub fn new() -> Self {
        Self {
            read_only_tools: vec![
                "read_file".to_owned(),
                "glob".to_owned(),
                "search".to_owned(),
            ],
        }
    }

    /// Classify a tool call. Read-only tools → Allow. Others → Ask (Phase E).
    pub fn classify(&self, call: &ToolCall) -> PermissionDecision {
        if self.read_only_tools.contains(&call.name) {
            PermissionDecision::Allow
        } else {
            // Phase E will implement Ask flow; for now deny unknown tools
            PermissionDecision::Ask
        }
    }
}

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn read_only_tools_allowed() {
        let engine = PermissionEngine::new();
        for name in &["read_file", "glob", "search"] {
            let call = ToolCall {
                id: "c1".to_owned(),
                name: name.to_string(),
                arguments: json!({}),
            };
            assert_eq!(engine.classify(&call), PermissionDecision::Allow);
        }
    }

    #[test]
    fn unknown_tool_asks() {
        let engine = PermissionEngine::new();
        let call = ToolCall {
            id: "c2".to_owned(),
            name: "apply_patch".to_owned(),
            arguments: json!({}),
        };
        assert_eq!(engine.classify(&call), PermissionDecision::Ask);
    }
}
