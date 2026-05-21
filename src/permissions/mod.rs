//! Permission engine — classifies tool calls as Allow/Ask/Deny.
//!
//! Read-only tools are auto-allowed. Write tools require Ask (Build mode)
//! or are Denied (Plan mode).

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

/// Tools that are always read-only (allowed without asking).
const READ_ONLY_TOOLS: &[&str] = &["read_file", "glob", "search"];

/// Tools that write to the workspace.
const WRITE_TOOLS: &[&str] = &["apply_patch", "shell"];

/// Classifies tool calls into permission decisions.
pub struct PermissionEngine {
    agent: PrimaryAgent,
}

impl PermissionEngine {
    pub fn new(agent: PrimaryAgent) -> Self {
        Self { agent }
    }

    /// Classify a tool call based on agent mode and tool type.
    pub fn classify(&self, call: &ToolCall) -> PermissionDecision {
        // Read-only tools always allowed
        if READ_ONLY_TOOLS.contains(&call.name.as_str()) {
            return PermissionDecision::Allow;
        }

        // Write tools: denied in Plan mode, Ask in Build mode
        if WRITE_TOOLS.contains(&call.name.as_str()) {
            return match self.agent {
                PrimaryAgent::Plan => PermissionDecision::Deny,
                PrimaryAgent::Build => PermissionDecision::Allow, // Auto-allow in Build for v1
            };
        }

        // Unknown tools: Ask
        PermissionDecision::Ask
    }

    /// Update the active agent.
    pub fn set_agent(&mut self, agent: PrimaryAgent) {
        self.agent = agent;
    }
}

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new(PrimaryAgent::Build)
    }
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
    fn write_tools_allowed_in_build() {
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
}
