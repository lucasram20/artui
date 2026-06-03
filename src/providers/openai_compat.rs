//! OpenAI-compatible Chat Completions provider with streaming tool-call support.

use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{app::AppEvent, config::OpenAiCompatConfig};

use super::{
    tool_serialization::{to_openai_chat, tool_choice_to_openai},
    LlmProvider, ModelEvent, ModelRequest,
};

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    config: OpenAiCompatConfig,
}

impl OpenAiCompatProvider {
    pub fn new(config: OpenAiCompatConfig) -> Self {
        // Stable User-Agent for gateways that gate on client identity.
        let client = reqwest::Client::builder()
            .user_agent(format!("artui/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client, config }
    }

    async fn send_event(tx: &mpsc::Sender<AppEvent>, event: ModelEvent) {
        let _ = tx.send(AppEvent::Model(event)).await;
    }

    async fn stream_chat(&self, request: ModelRequest, tx: mpsc::Sender<AppEvent>) -> Result<()> {
        let mut messages = Vec::new();

        if let Some(system) = &request.system_prompt {
            if !system.trim().is_empty() {
                messages.push(json!({"role": "system", "content": system}));
            }
        }

        for msg in &request.messages {
            if msg.content.is_empty() {
                continue;
            }
            let role = match msg.role {
                crate::app::Role::User => "user",
                crate::app::Role::Assistant => "assistant",
            };
            if msg.images.is_empty() {
                messages.push(json!({"role": role, "content": msg.content}));
            } else {
                let content = super::copilot::build_openai_content(&msg.content, &msg.images);
                messages.push(json!({"role": role, "content": content}));
            }
        }

        let mut body = json!({
            "model": self.config.default_model,
            "messages": messages,
            "stream": true,
        });

        // Include tools if any are registered
        if !request.tools.is_empty() {
            body["tools"] = to_openai_chat(&request.tools);
            body["tool_choice"] = tool_choice_to_openai(&request.tool_choice);
        }

        if let Some(max_tokens) = request.max_output_tokens {
            body["max_tokens"] = json!(max_tokens);
        }

        if let Some(effort) = &request.reasoning_effort {
            if !effort.is_empty() {
                body["reasoning_effort"] = json!(effort);
            }
        }

        let base = self.config.base_url.trim();
        if base.is_empty() {
            bail!(
                "no API base URL configured for provider '{}'. \
                 Set providers.openai_compat.base_url in ~/.config/artui/config.toml",
                self.config.credential_provider_id
            );
        }
        let mut url = base.trim_end_matches('/').to_owned();
        url.push_str("/chat/completions");

        let mut req = self.client.post(&url).json(&body);

        let credential_provider = if self.config.credential_provider_id.trim().is_empty() {
            "openai_compat"
        } else {
            self.config.credential_provider_id.as_str()
        };
        if let Some(api_key) = crate::auth::resolve_credential(credential_provider, None)
            .or_else(|| std::env::var(&self.config.api_key_env).ok())
            .filter(|key| !key.trim().is_empty())
        {
            req = req.header("Authorization", format!("Bearer {api_key}"));
        }

        let response = req
            .send()
            .await
            .context("failed to connect to OpenAI-compatible endpoint")?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .context("failed to read error response")?;
            bail!("{}", format_openai_compat_http_error(status, &body));
        }

        // Parse SSE stream
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut tool_calls: HashMap<u32, ToolCallAccumulator> = HashMap::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed to read response stream")?;
            buffer.extend_from_slice(&chunk);

            while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                let line = buffer.drain(..=newline).collect::<Vec<_>>();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if handle_sse_line(&tx, line, &mut tool_calls).await? {
                    return Ok(());
                }
            }
        }

        // Handle remaining buffer
        let line = String::from_utf8_lossy(&buffer);
        let line = line.trim();
        if !line.is_empty() {
            handle_sse_line(&tx, line, &mut tool_calls).await?;
        }

        // Emit any pending tool call ends
        emit_pending_tool_ends(&tx, &mut tool_calls).await;

        Self::send_event(&tx, ModelEvent::Done { end_turn: true }).await;
        Ok(())
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    async fn stream_turn(&self, request: ModelRequest, tx: mpsc::Sender<AppEvent>) {
        if let Err(error) = self.stream_chat(request, tx.clone()).await {
            Self::send_event(&tx, ModelEvent::Error(error.to_string())).await;
        }
    }
}

// ---------------------------------------------------------------------------
// SSE parsing internals
// ---------------------------------------------------------------------------

/// Accumulates streaming tool call arguments.
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

async fn handle_sse_line(
    tx: &mpsc::Sender<AppEvent>,
    line: &str,
    tool_calls: &mut HashMap<u32, ToolCallAccumulator>,
) -> Result<bool> {
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(false);
    };
    let data = data.trim();
    if data == "[DONE]" {
        emit_pending_tool_ends(tx, tool_calls).await;
        OpenAiCompatProvider::send_event(tx, ModelEvent::Done { end_turn: true }).await;
        return Ok(true);
    }

    let event: Value =
        serde_json::from_str(data).context("failed to parse OpenAI-compatible stream event")?;

    // Extract usage if present
    if let Some(usage) = event.get("usage") {
        if let (Some(input), Some(output)) = (
            usage.get("prompt_tokens").and_then(|v| v.as_u64()),
            usage.get("completion_tokens").and_then(|v| v.as_u64()),
        ) {
            OpenAiCompatProvider::send_event(
                tx,
                ModelEvent::Usage {
                    input_tokens: input as u32,
                    output_tokens: output as u32,
                },
            )
            .await;
        }
    }

    let Some(choices) = event.get("choices").and_then(|v| v.as_array()) else {
        return Ok(false);
    };

    for choice in choices {
        let Some(delta) = choice.get("delta") else {
            continue;
        };

        // Text content
        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
            if !content.is_empty() {
                OpenAiCompatProvider::send_event(tx, ModelEvent::TextDelta(content.to_owned()))
                    .await;
            }
        }

        // Tool calls
        if let Some(tc_array) = delta.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tc_array {
                let index = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                let acc = tool_calls
                    .entry(index)
                    .or_insert_with(|| ToolCallAccumulator {
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                        started: false,
                    });

                // First chunk carries id and function name
                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                    acc.id = id.to_owned();
                }
                if let Some(func) = tc.get("function") {
                    if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                        acc.name = name.to_owned();
                    }
                    if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                        acc.arguments.push_str(args);

                        // Emit start on first chunk with a name
                        if !acc.started && !acc.name.is_empty() {
                            acc.started = true;
                            OpenAiCompatProvider::send_event(
                                tx,
                                ModelEvent::ToolCallStart {
                                    id: acc.id.clone(),
                                    name: acc.name.clone(),
                                },
                            )
                            .await;
                        }

                        // Emit args delta
                        if acc.started {
                            OpenAiCompatProvider::send_event(
                                tx,
                                ModelEvent::ToolCallArgsDelta {
                                    id: acc.id.clone(),
                                    json_chunk: args.to_owned(),
                                },
                            )
                            .await;
                        }
                    }
                }
            }
        }

        // Check finish_reason for tool_calls
        if let Some(finish) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            if finish == "tool_calls" || finish == "stop" {
                emit_pending_tool_ends(tx, tool_calls).await;
            }
        }
    }

    Ok(false)
}

async fn emit_pending_tool_ends(
    tx: &mpsc::Sender<AppEvent>,
    tool_calls: &mut HashMap<u32, ToolCallAccumulator>,
) {
    for (_, acc) in tool_calls.drain() {
        if acc.started {
            let arguments = serde_json::from_str(&acc.arguments)
                .unwrap_or(Value::String(acc.arguments.clone()));
            OpenAiCompatProvider::send_event(
                tx,
                ModelEvent::ToolCallEnd {
                    id: acc.id,
                    arguments,
                },
            )
            .await;
        }
    }
}

fn format_openai_compat_http_error(status: reqwest::StatusCode, body: &str) -> String {
    format!("OpenAI-compatible endpoint returned HTTP {status}: {body}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn parses_tool_call_sse_chunks() {
        let (tx, mut rx) = mpsc::channel(32);
        let mut tool_calls = HashMap::new();

        // First chunk: tool call start with id and function name + first args
        let line1 = r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc123","type":"function","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#;
        handle_sse_line(&tx, line1, &mut tool_calls).await.unwrap();

        // Second chunk: more arguments
        let line2 = r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"/src"}}]}}]}"#;
        handle_sse_line(&tx, line2, &mut tool_calls).await.unwrap();

        // Third chunk: final arguments
        let line3 = r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"/main.rs\"}"}}]}}]}"#;
        handle_sse_line(&tx, line3, &mut tool_calls).await.unwrap();

        // Finish
        let line4 = r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#;
        handle_sse_line(&tx, line4, &mut tool_calls).await.unwrap();

        // Verify events
        let event1 = rx.recv().await.unwrap();
        assert!(
            matches!(&event1, AppEvent::Model(ModelEvent::ToolCallStart { id, name }) if id == "call_abc123" && name == "read_file")
        );

        let event2 = rx.recv().await.unwrap();
        assert!(
            matches!(&event2, AppEvent::Model(ModelEvent::ToolCallArgsDelta { id, json_chunk }) if id == "call_abc123" && json_chunk == r#"{"pa"#)
        );

        let event3 = rx.recv().await.unwrap();
        assert!(
            matches!(&event3, AppEvent::Model(ModelEvent::ToolCallArgsDelta { id, json_chunk }) if id == "call_abc123" && json_chunk == r#"th":"/src"#)
        );

        let event4 = rx.recv().await.unwrap();
        assert!(
            matches!(&event4, AppEvent::Model(ModelEvent::ToolCallArgsDelta { id, json_chunk }) if id == "call_abc123" && json_chunk == r#"/main.rs"}"#)
        );

        let event5 = rx.recv().await.unwrap();
        assert!(
            matches!(&event5, AppEvent::Model(ModelEvent::ToolCallEnd { id, arguments }) if id == "call_abc123" && arguments == &json!({"path": "/src/main.rs"}))
        );
    }

    #[tokio::test]
    async fn parses_text_delta() {
        let (tx, mut rx) = mpsc::channel(32);
        let mut tool_calls = HashMap::new();

        let line = r#"data: {"choices":[{"index":0,"delta":{"content":"Hello"}}]}"#;
        handle_sse_line(&tx, line, &mut tool_calls).await.unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event,
            AppEvent::Model(ModelEvent::TextDelta(ref t)) if t == "Hello"
        ));
    }

    #[tokio::test]
    async fn done_signal() {
        let (tx, mut rx) = mpsc::channel(32);
        let mut tool_calls = HashMap::new();

        let line = "data: [DONE]";
        let done = handle_sse_line(&tx, line, &mut tool_calls).await.unwrap();
        assert!(done);

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, AppEvent::Model(ModelEvent::Done { .. })));
    }
}
