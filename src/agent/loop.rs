//! Agent loop — drives multi-step tool-call conversations.
//!
//! `run_turn` streams a model response, collects any tool calls, dispatches them,
//! feeds results back, and iterates until the model emits `Done { end_turn: true }`
//! or the step limit is reached.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::compaction;
use crate::app::{AppEvent, AuthEvent, Message, Role};
use crate::hooks::{fire_hooks, HookConfig, HookEvent};
use crate::permissions::{ApprovalAnswer, ApprovalPrompt, PermissionDecision, PermissionEngine};
use crate::providers::{LlmProvider, ModelEvent, ModelRequest, ToolCall};
use crate::tools::registry::ToolRegistry;
use crate::tools::{ToolContext, ToolResult};

/// Configuration for the agent loop.
pub struct AgentLoopConfig {
    pub max_steps_per_turn: usize,
    pub max_read_file_chars: usize,
    pub workspace_root: PathBuf,
    /// Active model context window in tokens. `None` = unknown → use compaction default.
    pub context_window: Option<u32>,
    /// Disable auto-compaction entirely (matches opencode `compaction.auto = false`).
    pub compaction_auto: bool,
    /// Reserved output budget for compaction trigger calculation.
    pub compaction_reserve_tokens: u32,
    /// Recent-message budget preserved verbatim during compaction.
    pub compaction_keep_recent_tokens: u32,
    /// User-defined lifecycle hooks (empty = no-op).
    pub hooks: HookConfig,
    /// Permission engine — classifies tool calls. When `None`, all tools
    /// auto-allow (legacy behaviour for tests). Wrapped in `Arc<Mutex<…>>`
    /// because the loop needs to flip session-allow flags after the user
    /// answers an approval modal.
    pub permissions: Option<std::sync::Arc<tokio::sync::Mutex<PermissionEngine>>>,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_steps_per_turn: 25,
            max_read_file_chars: 32_000,
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            context_window: None,
            compaction_auto: true,
            compaction_reserve_tokens: 20_000,
            compaction_keep_recent_tokens: 8_000,
            hooks: HookConfig::default(),
            permissions: None,
        }
    }
}

/// Run a single user turn through the agent loop.
///
/// This function:
/// 1. Sends the request to the provider
/// 2. Collects streaming events, forwarding text deltas to the UI
/// 3. If tool calls are emitted, dispatches them and feeds results back
/// 4. Repeats until `Done { end_turn: true }` or step limit reached
pub async fn run_turn(
    provider: Arc<dyn LlmProvider>,
    registry: Arc<ToolRegistry>,
    mut request: ModelRequest,
    event_tx: mpsc::Sender<AppEvent>,
    cancel: CancellationToken,
    config: &AgentLoopConfig,
) -> Vec<Message> {
    let mut extra_messages: Vec<Message> = Vec::new();
    let mut steps = 0;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        steps += 1;
        if steps > config.max_steps_per_turn {
            let limit_msg = format!(
                "Step limit reached ({} steps). Stopping agent loop.",
                config.max_steps_per_turn
            );
            let _ = event_tx
                .send(AppEvent::Model(ModelEvent::TextDelta(limit_msg.clone())))
                .await;
            let _ = event_tx
                .send(AppEvent::Model(ModelEvent::Done { end_turn: true }))
                .await;
            extra_messages.push(Message::new(Role::Assistant, limit_msg));
            break;
        }

        // ── Auto-compaction pre-flight ─────────────────────────────────
        // Mirror opencode's SessionCompaction.isOverflow check before each
        // provider call. Skip if disabled, cancelled, or transcript too small.
        if config.compaction_auto
            && !cancel.is_cancelled()
            && compaction::needs_compaction_with(
                &request.messages,
                config.context_window,
                config.compaction_reserve_tokens,
            )
        {
            let before = request.messages.len();
            let _ = event_tx
                .send(AppEvent::Auth(AuthEvent::Status(
                    "Compacting context…".to_owned(),
                )))
                .await;

            let compacted = compaction::compact_messages_with(
                &request.messages,
                Arc::clone(&provider),
                config.compaction_keep_recent_tokens,
            )
            .await;

            // Only adopt result if it actually shrank — the provider may have
            // failed silently (empty summary) and returned the original list.
            if compacted.len() < before {
                request.messages = compacted;
                let _ = event_tx
                    .send(AppEvent::Auth(AuthEvent::Status(format!(
                        "Compacted {} → {} messages",
                        before,
                        request.messages.len()
                    ))))
                    .await;
            } else {
                let _ = event_tx
                    .send(AppEvent::Auth(AuthEvent::Status(
                        "Compaction skipped".to_owned(),
                    )))
                    .await;
            }
        }
        // ───────────────────────────────────────────────────────────────

        // Stream one model response
        let (stream_tx, mut stream_rx) = mpsc::channel::<AppEvent>(64);
        let provider_clone = Arc::clone(&provider);
        let req_clone = request.clone();
        let cancel_clone = cancel.clone();

        let stream_handle = tokio::spawn(async move {
            tokio::select! {
                _ = cancel_clone.cancelled() => {}
                _ = provider_clone.stream_turn(req_clone, stream_tx) => {}
            }
        });

        // Collect events from this stream
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut pending_calls: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new(); // id -> (name, args_buffer)
        let mut assistant_text = String::new();
        let mut end_turn = false;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    stream_handle.abort();
                    let _ = event_tx.send(AppEvent::Model(ModelEvent::Done { end_turn: true })).await;
                    return extra_messages;
                }
                event = stream_rx.recv() => {
                    let Some(event) = event else { break; };
                    match &event {
                        AppEvent::Model(ModelEvent::TextDelta(text)) => {
                            assistant_text.push_str(text);
                            let _ = event_tx.send(event).await;
                        }
                        AppEvent::Model(ModelEvent::ToolCallStart { id, name }) => {
                            pending_calls.insert(id.clone(), (name.clone(), String::new()));
                            // Don't forward to UI — handled internally
                        }
                        AppEvent::Model(ModelEvent::ToolCallArgsDelta { id, json_chunk }) => {
                            if let Some((_, buf)) = pending_calls.get_mut(id) {
                                buf.push_str(json_chunk);
                            }
                        }
                        AppEvent::Model(ModelEvent::ToolCallEnd { id, arguments }) => {
                            if let Some((name, _)) = pending_calls.remove(id) {
                                tool_calls.push(ToolCall {
                                    id: id.clone(),
                                    name,
                                    arguments: arguments.clone(),
                                });
                            }
                        }
                        AppEvent::Model(ModelEvent::Done { end_turn: et }) => {
                            end_turn = *et;
                            if tool_calls.is_empty() {
                                // No tool calls — forward Done to UI
                                let _ = event_tx.send(event).await;
                            }
                            break;
                        }
                        AppEvent::Model(ModelEvent::Error(_)) => {
                            let _ = event_tx.send(event).await;
                            return extra_messages;
                        }
                        // Usage, ReasoningDelta — forward to UI
                        _ => {
                            let _ = event_tx.send(event).await;
                        }
                    }
                }
            }
        }

        // Wait for stream task to finish
        let _ = stream_handle.await;

        // If no tool calls, we're done
        if tool_calls.is_empty() {
            if !assistant_text.is_empty() {
                extra_messages.push(Message::new(Role::Assistant, assistant_text));
            }
            break;
        }

        // Record assistant message with tool calls in transcript
        if !assistant_text.is_empty() {
            extra_messages.push(Message::new(Role::Assistant, assistant_text.clone()));
            request
                .messages
                .push(Message::new(Role::Assistant, assistant_text));
        }

        // Dispatch tool calls and collect results
        for call in &tool_calls {
            if cancel.is_cancelled() {
                let _ = event_tx
                    .send(AppEvent::Model(ModelEvent::Done { end_turn: true }))
                    .await;
                return extra_messages;
            }

            fire_hooks(
                &config.hooks,
                HookEvent::PreToolUse,
                &call.name,
                &config.workspace_root,
            )
            .await;

            // ── Permission gate ─────────────────────────────────────
            // Classify the tool call, render an Approval modal when the
            // engine says Ask, deny outright when it says Deny. Allow
            // falls through to dispatch as before.
            let decision = match &config.permissions {
                Some(engine) => engine.lock().await.classify(call),
                None => PermissionDecision::Allow,
            };
            let result = match decision {
                PermissionDecision::Deny => {
                    let msg = format!(
                        "denied_by_policy: tool '{}' is not allowed in the current agent mode",
                        call.name
                    );
                    let _ = event_tx
                        .send(AppEvent::Model(ModelEvent::TextDelta(format!(
                            "\n⛔ {msg}\n"
                        ))))
                        .await;
                    ToolResult::error(call.id.clone(), msg)
                }
                PermissionDecision::Ask => {
                    let (tx_answer, rx_answer) = tokio::sync::oneshot::channel();
                    let prompt = ApprovalPrompt {
                        call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        title: format!("Allow {} to run? (a once / s session / d deny)", call.name),
                        body: render_approval_body(&call.name, &call.arguments),
                        reply: tx_answer,
                    };
                    let _ = event_tx
                        .send(AppEvent::ApprovalRequest(Box::new(prompt)))
                        .await;
                    let answer = tokio::select! {
                        ans = rx_answer => ans.unwrap_or(ApprovalAnswer::Deny),
                        _ = cancel.cancelled() => ApprovalAnswer::Deny,
                    };
                    match answer {
                        ApprovalAnswer::Deny => {
                            ToolResult::error(call.id.clone(), "denied_by_user".to_owned())
                        }
                        ApprovalAnswer::Once => {
                            let ctx = ToolContext {
                                call_id: call.id.clone(),
                                workspace_root: config.workspace_root.clone(),
                                cwd: config.workspace_root.clone(),
                                events: event_tx.clone(),
                                max_read_file_chars: config.max_read_file_chars,
                            };
                            registry.dispatch(call, ctx).await
                        }
                        ApprovalAnswer::Session => {
                            if let Some(engine) = &config.permissions {
                                engine.lock().await.approve_for_session(&call.name);
                            }
                            let ctx = ToolContext {
                                call_id: call.id.clone(),
                                workspace_root: config.workspace_root.clone(),
                                cwd: config.workspace_root.clone(),
                                events: event_tx.clone(),
                                max_read_file_chars: config.max_read_file_chars,
                            };
                            registry.dispatch(call, ctx).await
                        }
                    }
                }
                PermissionDecision::Allow => {
                    let ctx = ToolContext {
                        call_id: call.id.clone(),
                        workspace_root: config.workspace_root.clone(),
                        cwd: config.workspace_root.clone(),
                        events: event_tx.clone(),
                        max_read_file_chars: config.max_read_file_chars,
                    };
                    registry.dispatch(call, ctx).await
                }
            };

            fire_hooks(
                &config.hooks,
                HookEvent::PostToolUse,
                &call.name,
                &config.workspace_root,
            )
            .await;

            // Show tool result in UI as a text delta
            let display = format_tool_result(&call.name, &result);
            let _ = event_tx
                .send(AppEvent::Model(ModelEvent::TextDelta(display.clone())))
                .await;

            // Add tool result to transcript for next iteration
            let tool_content = if let Some(err) = &result.error {
                format!("Error: {err}")
            } else {
                result.content.clone()
            };

            // Push as a user message with tool result context
            // (OpenAI format: role=tool, but we use role=User with prefix for simplicity until Phase G)
            let tool_msg = Message::new(
                Role::User,
                format!(
                    "[tool_result call_id={} name={}]\n{}",
                    call.id, call.name, tool_content
                ),
            );
            extra_messages.push(tool_msg.clone());
            request.messages.push(tool_msg);
        }

        // If model said end_turn even with tool calls, respect it
        if end_turn {
            let _ = event_tx
                .send(AppEvent::Model(ModelEvent::Done { end_turn: true }))
                .await;
            break;
        }

        // Continue loop — model will see tool results and respond
    }

    extra_messages
}

fn format_tool_result(tool_name: &str, result: &ToolResult) -> String {
    /// Tools whose output should not be truncated (e.g. diffs).
    const FULL_OUTPUT_TOOLS: &[&str] = &["apply_patch"];
    /// Default preview length for other tool results.
    const DEFAULT_PREVIEW_LEN: usize = 200;

    if let Some(err) = &result.error {
        format!("\n⚠ {tool_name}: {err}\n")
    } else {
        let content = &result.content;
        let preview =
            if FULL_OUTPUT_TOOLS.contains(&tool_name) || content.len() <= DEFAULT_PREVIEW_LEN {
                content.clone()
            } else {
                format!("{}...", &content[..DEFAULT_PREVIEW_LEN])
            };
        format!("\n📄 {tool_name}:\n{preview}\n")
    }
}

/// Render a tool call as a human-readable body for the Approval modal.
///
/// `apply_patch` gets the raw V4A patch (already the user-facing diff).
/// `shell` gets the command + cwd. Everything else falls back to a
/// pretty-printed JSON dump of arguments.
fn render_approval_body(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "apply_patch" => args
            .get("patch")
            .and_then(|v| v.as_str())
            .unwrap_or("(no patch payload)")
            .to_owned(),
        "shell" => {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let cwd = args
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_owned();
            let reason = args
                .get("reason")
                .and_then(|v| v.as_str())
                .map(|r| format!("\nreason: {r}"))
                .unwrap_or_default();
            format!("$ {cmd}\ncwd: {cwd}{reason}")
        }
        _ => serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string()),
    }
}
