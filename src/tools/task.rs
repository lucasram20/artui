//! `task` tool — spawn a subagent to handle a focused task in isolated context.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::r#loop::{run_turn, AgentLoopConfig};
use crate::app::{AppEvent, Message, Role};
use crate::providers::{LlmProvider, ModelRequest, ToolChoice, ToolSpec};
use crate::tools::registry::ToolRegistry;

use super::{Tool, ToolContext, ToolResult};

pub struct TaskTool {
    provider: Arc<dyn LlmProvider>,
}

impl TaskTool {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

/// Subagent types determine which tools are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubagentType {
    /// Read-only: read_file, glob, search
    Explore,
    /// Full minus task (no recursion)
    General,
}

impl SubagentType {
    fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "explore" => Self::Explore,
            _ => Self::General,
        }
    }

    fn system_prompt(self) -> &'static str {
        match self {
            Self::Explore => {
                "You are a focused exploration subagent. Read files, search, and glob to answer the question. Do not modify any files. Be concise in your final answer."
            }
            Self::General => {
                "You are a focused subagent handling a specific task. Complete the task efficiently and report what you did in a concise summary."
            }
        }
    }
}

#[async_trait]
impl Tool for TaskTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task".to_owned(),
            description: "Spawn a subagent to handle a focused task in an isolated context. Returns the subagent's summary.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "description": {
                        "type": "string",
                        "description": "3-5 word summary of the task"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "The full task prompt for the subagent"
                    },
                    "subagent_type": {
                        "type": "string",
                        "enum": ["explore", "general"],
                        "description": "Type of subagent: 'explore' (read-only) or 'general' (full minus task)"
                    }
                },
                "required": ["description", "prompt", "subagent_type"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: ToolContext) -> ToolResult {
        let Some(prompt) = args.get("prompt").and_then(|v| v.as_str()) else {
            return ToolResult::error(ctx.call_id, "missing required parameter: prompt".to_owned());
        };

        let subagent_type = args
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general");
        let agent_type = SubagentType::from_str(subagent_type);

        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("subagent task");

        // Build a limited registry for the subagent (no `task` tool to prevent recursion)
        let registry = Arc::new(ToolRegistry::for_subagent(
            agent_type == SubagentType::Explore,
        ));

        let request = ModelRequest {
            messages: vec![Message {
                role: Role::User,
                content: prompt.to_owned(),
            }],
            system_prompt: Some(agent_type.system_prompt().to_owned()),
            reasoning_effort: None,
            tools: registry.specs(),
            tool_choice: ToolChoice::Auto,
            max_output_tokens: None,
        };

        // Run subagent with reduced step limit
        let config = AgentLoopConfig {
            max_steps_per_turn: 10,
            max_read_file_chars: 32_000,
            workspace_root: ctx.workspace_root.clone(),
        };

        let cancel = CancellationToken::new();
        let (sink_tx, mut sink_rx) = mpsc::channel::<AppEvent>(64);
        tokio::spawn(async move { while sink_rx.recv().await.is_some() {} });

        let extra_messages = run_turn(
            Arc::clone(&self.provider),
            registry,
            request,
            sink_tx,
            cancel,
            &config,
        )
        .await;

        // Extract final assistant response as the subagent's summary
        let summary = extra_messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "Subagent completed without producing a summary.".to_owned());

        let max_summary = 8000;
        let content = if summary.len() > max_summary {
            format!("{}...\n(truncated)", &summary[..max_summary])
        } else {
            summary
        };

        ToolResult::ok(
            ctx.call_id,
            format!("[subagent:{subagent_type} \"{description}\"]\n{content}"),
        )
    }
}
