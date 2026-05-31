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
    /// LSP manager handed through to tools that need it (currently only the
    /// `lsp` tool). `None` disables LSP entirely — matches `[lsp] enabled =
    /// false` in the global config.
    pub lsp_manager: Option<std::sync::Arc<crate::lsp::LspManager>>,
    /// Phase N3 — when true, `apply_patch` runs a writethrough pass after
    /// every successful patch.
    pub lsp_writethrough: bool,
    /// Phase N3 — wall-clock budget for the post-apply_patch
    /// publishDiagnostics poll.
    pub lsp_diagnostics_timeout_ms: u32,
    /// Maximum number of "please continue" nudges the loop will inject
    /// per turn when the model stalls (emits a preamble like "I'll
    /// inspect X" then `Done` without any tool calls). Capped to stop
    /// confused models from looping forever; the counter resets when a
    /// tool call actually lands. Default 2 — enough to recover from
    /// the most common smaller-model misfire without burning tokens
    /// on a model that's genuinely confused.
    pub max_nudges_per_turn: u32,
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
            lsp_manager: None,
            lsp_writethrough: true,
            lsp_diagnostics_timeout_ms: 750,
            max_nudges_per_turn: 2,
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
    // Number of times we've nudged the model after it said "I'll do X" but
    // emitted no tool calls. Capped per turn so a confused model can't
    // loop forever; resets when a tool call lands.
    let mut nudges_used: u32 = 0;

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

        // No tool calls — model either finished or stalled.
        //
        // Stall detection: small/cheap models (and some big ones, depending
        // on the prompt) emit a "preamble" like `I'll quickly inspect X`
        // and then `Done { end_turn: true }` without firing any tool. The
        // user sees a half-answer with no follow-through. This is the
        // exact failure mode flagged by every coding-agent post-mortem
        // (Claude Code's "Token Budget Continuation", LangChain's
        // tool_choice="required", oh-my-pi's nudge-and-retry).
        //
        // Recovery: if the assistant text reads like a promise to act
        // (heuristic match against intent verbs) and we're under the
        // per-turn nudge cap, append the assistant's preamble to the
        // request, then a synthetic user nudge that asks the model to
        // either follow through with a tool call or wrap up. Loop again.
        if tool_calls.is_empty() {
            let promised = looks_like_unfulfilled_promise(&assistant_text);
            let cap = config.max_nudges_per_turn;
            if promised && nudges_used < cap && !end_turn_explicit_finish(&assistant_text) {
                if !assistant_text.is_empty() {
                    request
                        .messages
                        .push(Message::new(Role::Assistant, assistant_text.clone()));
                    extra_messages.push(Message::new(Role::Assistant, assistant_text));
                }
                request
                    .messages
                    .push(Message::new(Role::User, NUDGE_BODY.to_owned()));
                nudges_used = nudges_used.saturating_add(1);
                tracing::debug!(
                    target: "agent",
                    nudge = nudges_used,
                    cap = cap,
                    "no tool calls but model promised action; nudging to continue",
                );
                continue;
            }
            // Either no promise was made (model genuinely finished) or
            // we hit the nudge cap. Record the assistant text and exit.
            if !assistant_text.is_empty() {
                extra_messages.push(Message::new(Role::Assistant, assistant_text));
            }
            break;
        }

        // A tool call landed; reset the nudge counter so a later stall
        // gets its own fresh budget.
        nudges_used = 0;

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
                                lsp_manager: config.lsp_manager.clone(),
                                lsp_writethrough: config.lsp_writethrough,
                                lsp_diagnostics_timeout_ms: config.lsp_diagnostics_timeout_ms,
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
                                lsp_manager: config.lsp_manager.clone(),
                                lsp_writethrough: config.lsp_writethrough,
                                lsp_diagnostics_timeout_ms: config.lsp_diagnostics_timeout_ms,
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
                        lsp_manager: config.lsp_manager.clone(),
                        lsp_writethrough: config.lsp_writethrough,
                        lsp_diagnostics_timeout_ms: config.lsp_diagnostics_timeout_ms,
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

/// Synthetic user message injected when the model stalls — emits a
/// preamble but no tool call. Wording mirrors the Claude Code / Aider /
/// oh-my-pi convention: short, unambiguous, gives the model two clear
/// paths (call the tool you intended, or wrap up cleanly).
const NUDGE_BODY: &str = "Please continue. If you intended to call a tool, call it now. \
If your previous answer was complete, briefly confirm and stop.";

/// Heuristic: did the assistant's latest message read like an intent to
/// act that the model never followed through on? Returns true when the
/// text contains common "I will / let me / I'll go ahead and" markers
/// — these are the phrases that precede a tool call in a healthy
/// trajectory and signal a stall when the tool call is missing.
///
/// Conservative: false negatives (real promises we miss and don't nudge)
/// are fine — the user can re-prompt. False positives (nudging when the
/// model already finished) waste a turn so the threshold is biased
/// toward only firing on clear promises.
pub(crate) fn looks_like_unfulfilled_promise(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.trim().is_empty() {
        return false;
    }
    // Common preamble phrasings, in order of frequency. Keeping the list
    // small + explicit so unit tests catch regressions; an ML classifier
    // would be overkill for what is essentially regex-tier intent
    // detection.
    const PROMISES: &[&str] = &[
        "i'll ",
        "i will ",
        "i'm going to ",
        "i am going to ",
        "let me ",
        "let's ",
        "going to ",
        "i'll quickly ",
        "first, i'll ",
        "first i'll ",
        "now i'll ",
        "next, i'll ",
        "next i'll ",
        "now let me ",
        "i'll start by ",
        "i'll begin by ",
        "i'll inspect ",
        "i'll check ",
        "i'll look ",
        "i'll read ",
        "i'll run ",
    ];
    PROMISES.iter().any(|needle| lower.contains(needle))
}

/// Heuristic: did the model explicitly say it's done / has nothing
/// further to do? When true, we skip the nudge even if a "promise"
/// pattern matched earlier in the same message. Belt-and-braces against
/// nudging a model that wrapped up properly with a final summary.
pub(crate) fn end_turn_explicit_finish(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const FINISH_PATTERNS: &[&str] = &[
        "all done",
        "task complete",
        "i'm done",
        "that's all",
        "nothing more",
        "no further action",
        "no other changes",
        "no additional changes",
    ];
    FINISH_PATTERNS.iter().any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod nudge_heuristic_tests {
    use super::*;

    #[test]
    fn empty_text_is_not_a_promise() {
        assert!(!looks_like_unfulfilled_promise(""));
        assert!(!looks_like_unfulfilled_promise("   \n  "));
    }

    #[test]
    fn detects_ill_quickly_inspect() {
        // The exact phrasing from the bug report screenshot.
        let text = "I'll quickly inspect `cloudflare/` (README, package config, worker) and summarize its purpose.";
        assert!(looks_like_unfulfilled_promise(text));
    }

    #[test]
    fn detects_let_me_check() {
        assert!(looks_like_unfulfilled_promise(
            "Let me check the existing layout."
        ));
    }

    #[test]
    fn detects_im_going_to() {
        assert!(looks_like_unfulfilled_promise(
            "I'm going to look at src/lib.rs first."
        ));
    }

    #[test]
    fn ignores_non_promise_text() {
        assert!(!looks_like_unfulfilled_promise("The function returns 42."));
        assert!(!looks_like_unfulfilled_promise(
            "Done. The fix is at src/lib.rs:12."
        ));
    }

    #[test]
    fn explicit_finish_overrides_promise() {
        // Edge case: the model said "I'll do X" but in the *same* message
        // also said "all done" — interpret as a final summary, don't
        // nudge.
        let text = "I'll explain — all done. The function lives at src/lib.rs:12.";
        assert!(looks_like_unfulfilled_promise(text));
        assert!(end_turn_explicit_finish(text));
    }

    #[test]
    fn case_insensitive() {
        assert!(looks_like_unfulfilled_promise("LET ME do that"));
        assert!(end_turn_explicit_finish("ALL DONE."));
    }
}
