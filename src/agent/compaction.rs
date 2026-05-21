//! Token budget compaction — summarizes old messages when context fills up.
//!
//! Trigger: when estimated tokens >= 0.835 * context_window.
//! Strategy: replace oldest N messages (≈60% of used tokens) with a single summary.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::app::{AppEvent, Message, Role};
use crate::providers::{LlmProvider, ModelEvent, ModelRequest, ToolChoice};

/// Default context window (tokens).
const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;

/// Compaction threshold as fraction of context window.
const COMPACTION_THRESHOLD: f64 = 0.835;

/// Estimate token count from messages (chars / 4).
pub fn estimate_tokens(messages: &[Message]) -> u32 {
    let chars: usize = messages.iter().map(|m| m.content.len()).sum();
    (chars / 4) as u32
}

/// Check if compaction is needed.
pub fn needs_compaction(messages: &[Message], context_window: Option<u32>) -> bool {
    let window = context_window.unwrap_or(DEFAULT_CONTEXT_WINDOW);
    let tokens = estimate_tokens(messages);
    tokens as f64 >= COMPACTION_THRESHOLD * window as f64
}

/// Get context window for a model (default 128k if unknown).
pub fn context_window_for_model(_model: &str) -> u32 {
    DEFAULT_CONTEXT_WINDOW
}

/// Run compaction: summarize oldest messages and replace them with a summary.
pub async fn compact_messages(
    messages: &[Message],
    provider: Arc<dyn LlmProvider>,
    _context_window: u32,
) -> Vec<Message> {
    let total_tokens = estimate_tokens(messages);
    let target_remove = (total_tokens as f64 * 0.6) as u32;

    // Find how many messages from the start to remove
    let mut accumulated = 0u32;
    let mut split_at = 0;
    for (i, msg) in messages.iter().enumerate() {
        accumulated += (msg.content.len() / 4) as u32;
        if accumulated >= target_remove {
            split_at = i + 1;
            break;
        }
    }

    if split_at == 0 || split_at >= messages.len() {
        return messages.to_vec();
    }

    let to_compact = &messages[..split_at];
    let to_keep = &messages[split_at..];

    // Build compaction prompt
    let compact_content = to_compact
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            format!("[{role}]: {}", m.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let compaction_request = ModelRequest {
        messages: vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation history into a concise context summary. \
                 Preserve all key decisions, file paths mentioned, task progress, and technical details. \
                 Keep it under 2000 characters.\n\n---\n{compact_content}"
            ),
        }],
        system_prompt: Some(COMPACTION_SYSTEM_PROMPT.to_owned()),
        reasoning_effort: None,
        tools: vec![],
        tool_choice: ToolChoice::None,
        max_output_tokens: Some(1000),
    };

    // Run compaction through provider
    let (tx, mut rx) = mpsc::channel::<AppEvent>(64);
    let provider_clone = Arc::clone(&provider);
    tokio::spawn(async move {
        provider_clone.stream_turn(compaction_request, tx).await;
    });

    let mut summary = String::new();
    while let Some(event) = rx.recv().await {
        if let AppEvent::Model(ModelEvent::TextDelta(text)) = event {
            summary.push_str(&text);
        }
    }

    if summary.is_empty() {
        return messages.to_vec();
    }

    // Build new message list: summary + kept messages
    let mut result = Vec::with_capacity(to_keep.len() + 1);
    result.push(Message {
        role: Role::Assistant,
        content: format!("[{split_at} messages compacted]\n\nContext summary:\n{summary}"),
    });
    result.extend_from_slice(to_keep);
    result
}

const COMPACTION_SYSTEM_PROMPT: &str = "\
You are a context compaction assistant. Your job is to summarize conversation history \
into a concise context brief that preserves all actionable information. Include: \
key decisions made, files modified or discussed, current task state, any errors encountered, \
and next steps. Omit pleasantries, repeated information, and verbose tool outputs. \
Output only the summary, no preamble.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_basic() {
        let messages = vec![
            Message {
                role: Role::User,
                content: "a".repeat(400),
            },
            Message {
                role: Role::Assistant,
                content: "b".repeat(600),
            },
        ];
        assert_eq!(estimate_tokens(&messages), 250); // 1000 chars / 4
    }

    #[test]
    fn needs_compaction_below_threshold() {
        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_owned(),
        }];
        assert!(!needs_compaction(&messages, Some(128_000)));
    }

    #[test]
    fn needs_compaction_above_threshold() {
        // 128k * 0.835 = ~106k tokens = ~424k chars
        let messages = vec![Message {
            role: Role::User,
            content: "x".repeat(500_000),
        }];
        assert!(needs_compaction(&messages, Some(128_000)));
    }
}
