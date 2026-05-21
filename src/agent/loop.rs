//! Agent loop — drives multi-step tool-call conversations.
//!
//! `run_turn` streams a model response, collects any tool calls, dispatches them,
//! feeds results back, and iterates until the model emits `Done { end_turn: true }`
//! or the step limit is reached.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::{AppEvent, Message, Role};
use crate::providers::{LlmProvider, ModelEvent, ModelRequest, ToolCall};
use crate::tools::registry::ToolRegistry;
use crate::tools::{ToolContext, ToolResult};

/// Configuration for the agent loop.
pub struct AgentLoopConfig {
    pub max_steps_per_turn: usize,
    pub max_read_file_chars: usize,
    pub workspace_root: PathBuf,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_steps_per_turn: 25,
            max_read_file_chars: 32_000,
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
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
            extra_messages.push(Message {
                role: Role::Assistant,
                content: limit_msg,
            });
            break;
        }

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
                extra_messages.push(Message {
                    role: Role::Assistant,
                    content: assistant_text,
                });
            }
            break;
        }

        // Record assistant message with tool calls in transcript
        if !assistant_text.is_empty() {
            extra_messages.push(Message {
                role: Role::Assistant,
                content: assistant_text.clone(),
            });
            request.messages.push(Message {
                role: Role::Assistant,
                content: assistant_text,
            });
        }

        // Dispatch tool calls and collect results
        for call in &tool_calls {
            if cancel.is_cancelled() {
                let _ = event_tx
                    .send(AppEvent::Model(ModelEvent::Done { end_turn: true }))
                    .await;
                return extra_messages;
            }

            let ctx = ToolContext {
                call_id: call.id.clone(),
                workspace_root: config.workspace_root.clone(),
                cwd: config.workspace_root.clone(),
                events: event_tx.clone(),
                max_read_file_chars: config.max_read_file_chars,
            };

            let result = registry.dispatch(call, ctx).await;

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
            let tool_msg = Message {
                role: Role::User,
                content: format!(
                    "[tool_result call_id={} name={}]\n{}",
                    call.id, call.name, tool_content
                ),
            };
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
    if let Some(err) = &result.error {
        format!("\n⚠ {tool_name}: {err}\n")
    } else {
        let content = &result.content;
        let preview = if content.len() > 200 {
            format!("{}...", &content[..200])
        } else {
            content.clone()
        };
        format!("\n📄 {tool_name}:\n{preview}\n")
    }
}
