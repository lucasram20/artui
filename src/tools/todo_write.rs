//! `todo_write` tool — Claude-Code-style task list for multi-step work.
//!
//! When the agent is doing a non-trivial multi-step task (refactor across
//! N files, implement a feature with planning + tests + docs), it calls
//! `todo_write` with a list of `{title, status}` entries. Subsequent
//! calls REPLACE the list entirely so the agent can mark items
//! `in_progress` / `completed` as it works.
//!
//! The TUI renders the list as a checklist in the chat pane, and the
//! header shows `[N/M tasks done]` so the user can track progress.
//!
//! Mirrors Claude Code's TodoWrite tool. Read-only as far as filesystem
//! goes — only mutates app state — so it bypasses the approval engine.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::app::AppEvent;
use crate::providers::ToolSpec;

use super::{Tool, ToolContext, ToolResult};

const MAX_TODOS: usize = 50;
const MAX_TITLE_CHARS: usize = 120;

pub struct TodoWriteTool;

/// One todo item. Mirrors Claude Code's three-state model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub subject: String,
    #[serde(default = "default_status")]
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

fn default_status() -> TodoStatus {
    TodoStatus::Pending
}

impl TodoStatus {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Pending => "☐",
            Self::InProgress => "▶",
            Self::Completed => "☑",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "todo_write".to_owned(),
            description: "Maintain a structured todo list for multi-step work. \
                Pass `todos` as a full replacement list — the previous list is \
                discarded each call. Use it when a task takes 3+ distinct steps \
                so the user can see progress; mark exactly one item \
                `in_progress` at a time, flip to `completed` when done. \
                Statuses: `pending` (default), `in_progress`, `completed`. \
                Don't use this for trivial single-step tasks."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "required": ["todos"],
                "properties": {
                    "todos": {
                        "type": "array",
                        "description": "Full replacement list of todos.",
                        "maxItems": MAX_TODOS,
                        "items": {
                            "type": "object",
                            "required": ["subject"],
                            "properties": {
                                "subject": {
                                    "type": "string",
                                    "description": "Short imperative title (e.g. \"Add LSP smoke test\")."
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"],
                                    "description": "Default: pending."
                                }
                            }
                        }
                    }
                }
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: ToolContext) -> ToolResult {
        let Some(raw) = args.get("todos").and_then(|v| v.as_array()) else {
            return ToolResult::error(
                ctx.call_id,
                "missing required parameter: todos (array of {subject, status})".to_owned(),
            );
        };

        if raw.len() > MAX_TODOS {
            return ToolResult::error(
                ctx.call_id,
                format!(
                    "too many todos ({} > max {MAX_TODOS}); split the work or remove completed items",
                    raw.len()
                ),
            );
        }

        let mut todos: Vec<Todo> = Vec::with_capacity(raw.len());
        for (i, item) in raw.iter().enumerate() {
            match serde_json::from_value::<Todo>(item.clone()) {
                Ok(mut t) => {
                    t.subject = t.subject.trim().to_owned();
                    if t.subject.is_empty() {
                        return ToolResult::error(
                            ctx.call_id,
                            format!("todo at index {i} has empty `subject`"),
                        );
                    }
                    if t.subject.chars().count() > MAX_TITLE_CHARS {
                        return ToolResult::error(
                            ctx.call_id,
                            format!(
                                "todo at index {i} `subject` is over {MAX_TITLE_CHARS} chars; keep it short"
                            ),
                        );
                    }
                    todos.push(t);
                }
                Err(error) => {
                    return ToolResult::error(
                        ctx.call_id,
                        format!("todo at index {i} failed to parse: {error}"),
                    );
                }
            }
        }

        // Enforce: at most one `in_progress` at a time. Anything more is a
        // sign the model lost track of which task it's actively working on.
        let in_progress_count = todos
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        if in_progress_count > 1 {
            return ToolResult::error(
                ctx.call_id,
                format!(
                    "{in_progress_count} todos are marked `in_progress`; only one task should be active at a time"
                ),
            );
        }

        // Send the new list to the App so the TUI can render it. The
        // event is fire-and-forget; if the channel is closed (shutdown),
        // the tool still succeeds because the disk wasn't touched.
        let _ = ctx.events.send(AppEvent::TodoUpdate(todos.clone())).await;

        // Format a compact summary back to the model so it sees its
        // own list reflected (helps with "what was step 3 again?").
        let mut summary = format!("Updated todo list ({} items)\n", todos.len());
        for (i, t) in todos.iter().enumerate() {
            summary.push_str(&format!("{}. {} {}\n", i + 1, t.status.glyph(), t.subject));
        }
        ToolResult::ok(ctx.call_id, summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tokio::sync::mpsc;

    fn ctx(workspace: &Path) -> (ToolContext, mpsc::Receiver<AppEvent>) {
        let (tx, rx) = mpsc::channel(8);
        (
            ToolContext {
                call_id: "todo-test".to_owned(),
                workspace_root: workspace.to_path_buf(),
                cwd: workspace.to_path_buf(),
                events: tx,
                max_read_file_chars: 10_000,
                lsp_manager: None,
                lsp_writethrough: false,
                lsp_diagnostics_timeout_ms: 750,
                sandbox: crate::sandbox::SandboxSettings::default(),
                workspace_index: None,
                agent_depth: 0,
            },
            rx,
        )
    }

    #[tokio::test]
    async fn replaces_list_and_emits_event() {
        let dir = tempfile::TempDir::new().unwrap();
        let (c, mut rx) = ctx(dir.path());
        let result = TodoWriteTool
            .execute(
                json!({
                    "todos": [
                        {"subject": "Read spec", "status": "completed"},
                        {"subject": "Implement N3", "status": "in_progress"},
                        {"subject": "Write tests"},
                    ]
                }),
                c,
            )
            .await;
        assert!(!result.is_error(), "got error: {:?}", result.error);
        assert!(result.content.contains("3 items"));
        assert!(result.content.contains("Implement N3"));
        // The event should have fired.
        let event = rx.recv().await.expect("expected TodoUpdate event");
        match event {
            AppEvent::TodoUpdate(todos) => {
                assert_eq!(todos.len(), 3);
                assert_eq!(todos[0].status, TodoStatus::Completed);
                assert_eq!(todos[1].status, TodoStatus::InProgress);
                assert_eq!(todos[2].status, TodoStatus::Pending);
            }
            other => panic!("unexpected event {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejects_multiple_in_progress() {
        let dir = tempfile::TempDir::new().unwrap();
        let (c, _rx) = ctx(dir.path());
        let result = TodoWriteTool
            .execute(
                json!({
                    "todos": [
                        {"subject": "A", "status": "in_progress"},
                        {"subject": "B", "status": "in_progress"},
                    ]
                }),
                c,
            )
            .await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("only one task"));
    }

    #[tokio::test]
    async fn rejects_empty_subject() {
        let dir = tempfile::TempDir::new().unwrap();
        let (c, _rx) = ctx(dir.path());
        let result = TodoWriteTool
            .execute(
                json!({
                    "todos": [{"subject": "   "}]
                }),
                c,
            )
            .await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("empty `subject`"));
    }

    #[tokio::test]
    async fn rejects_overlong_subject() {
        let dir = tempfile::TempDir::new().unwrap();
        let (c, _rx) = ctx(dir.path());
        let huge = "x".repeat(MAX_TITLE_CHARS + 1);
        let result = TodoWriteTool
            .execute(json!({"todos": [{"subject": huge}]}), c)
            .await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("short"));
    }

    #[tokio::test]
    async fn requires_todos_array() {
        let dir = tempfile::TempDir::new().unwrap();
        let (c, _rx) = ctx(dir.path());
        let result = TodoWriteTool.execute(json!({}), c).await;
        assert!(result.is_error());
        assert!(result.error.unwrap().contains("missing required parameter"));
    }

    #[tokio::test]
    async fn empty_list_is_valid_meaning_clear_todos() {
        let dir = tempfile::TempDir::new().unwrap();
        let (c, mut rx) = ctx(dir.path());
        let result = TodoWriteTool.execute(json!({"todos": []}), c).await;
        assert!(!result.is_error());
        let event = rx.recv().await.expect("event");
        match event {
            AppEvent::TodoUpdate(todos) => assert!(todos.is_empty()),
            _ => panic!(),
        }
    }
}
