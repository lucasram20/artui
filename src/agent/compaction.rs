//! Token budget compaction — summarizes old messages when context fills up.
//!
//! Trigger model (opencode-compatible): `usable = context - reserved`,
//! compact when `estimate(messages) >= usable`.
//! Strategy: summarize oldest messages, preserve recent N tokens verbatim.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::app::{AppEvent, Message, Role};
use crate::providers::{LlmProvider, ModelEvent, ModelRequest, ToolChoice};

/// Default context window when the active provider/model does not advertise one.
const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;

/// Default reserved output buffer (tokens). Matches opencode `COMPACTION_BUFFER`.
const DEFAULT_RESERVE_TOKENS: u32 = 20_000;

/// Default recent-message budget preserved verbatim during compaction.
const DEFAULT_KEEP_RECENT_TOKENS: u32 = 8_000;

/// Estimate token count from messages (chars / 4).
pub fn estimate_tokens(messages: &[Message]) -> u32 {
    let chars: usize = messages.iter().map(|m| m.content.len()).sum();
    (chars / 4) as u32
}

/// Usable token budget = context - reserved.
pub fn usable_tokens(context_window: Option<u32>, reserve: u32) -> u32 {
    let window = context_window.unwrap_or(DEFAULT_CONTEXT_WINDOW);
    window.saturating_sub(reserve)
}

/// Check if compaction is needed (opencode-style: count >= usable).
pub fn needs_compaction(messages: &[Message], context_window: Option<u32>) -> bool {
    needs_compaction_with(messages, context_window, DEFAULT_RESERVE_TOKENS)
}

/// Check if compaction is needed using a configurable reserve buffer.
pub fn needs_compaction_with(
    messages: &[Message],
    context_window: Option<u32>,
    reserve: u32,
) -> bool {
    let usable = usable_tokens(context_window, reserve);
    if usable == 0 {
        return false;
    }
    estimate_tokens(messages) >= usable
}

/// Get context window for a model (default 128k if unknown).
pub fn context_window_for_model(_model: &str) -> u32 {
    DEFAULT_CONTEXT_WINDOW
}

/// Run compaction: summarize oldest messages and replace them with a summary.
///
/// `keep_recent_tokens` is the budget walked back from the newest message — those
/// stay verbatim. Older messages are summarized into one assistant message that
/// replaces them.
pub async fn compact_messages(
    messages: &[Message],
    provider: Arc<dyn LlmProvider>,
    _context_window: u32,
) -> Vec<Message> {
    compact_messages_with(messages, provider, DEFAULT_KEEP_RECENT_TOKENS).await
}

/// Run compaction with a configurable recent-token preservation budget.
pub async fn compact_messages_with(
    messages: &[Message],
    provider: Arc<dyn LlmProvider>,
    keep_recent_tokens: u32,
) -> Vec<Message> {
    if messages.len() <= 2 {
        return messages.to_vec();
    }

    // Walk backwards from newest, accumulating tokens until keep_recent_tokens reached.
    // split_at = first index that stays verbatim.
    let mut accumulated: u32 = 0;
    let mut split_at = messages.len();
    for (i, msg) in messages.iter().enumerate().rev() {
        let cost = (msg.content.len() / 4) as u32;
        accumulated = accumulated.saturating_add(cost);
        split_at = i;
        if accumulated >= keep_recent_tokens {
            break;
        }
    }

    if split_at == 0 {
        // Everything is "recent" — fall back to oldest 60% to make room.
        let total_tokens = estimate_tokens(messages);
        let target_remove = ((total_tokens as f64) * 0.6) as u32;
        let mut acc = 0u32;
        for (i, msg) in messages.iter().enumerate() {
            acc += (msg.content.len() / 4) as u32;
            if acc >= target_remove {
                split_at = i + 1;
                break;
            }
        }
        if split_at == 0 || split_at >= messages.len() {
            return messages.to_vec();
        }
    }

    let to_compact = &messages[..split_at];
    let to_keep = &messages[split_at..];

    if to_compact.is_empty() {
        return messages.to_vec();
    }

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
        messages: vec![Message::new(
            Role::User,
            format!(
                "Summarize the following conversation history into a concise context summary. \
                 Preserve all key decisions, file paths mentioned, task progress, and technical details. \
                 Keep it under 2000 characters.\n\n---\n{compact_content}"
            ),
        )],
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

    if summary.trim().is_empty() {
        return messages.to_vec();
    }

    // Build new message list: summary + kept messages
    let mut result = Vec::with_capacity(to_keep.len() + 1);
    result.push(Message::new(
        Role::Assistant,
        format!(
            "[{} messages compacted]\n\nContext summary:\n{}",
            split_at,
            summary.trim()
        ),
    ));
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
            Message::new(Role::User, "a".repeat(400)),
            Message::new(Role::Assistant, "b".repeat(600)),
        ];
        assert_eq!(estimate_tokens(&messages), 250); // 1000 chars / 4
    }

    #[test]
    fn usable_subtracts_reserve() {
        assert_eq!(usable_tokens(Some(128_000), 20_000), 108_000);
        assert_eq!(usable_tokens(None, 20_000), DEFAULT_CONTEXT_WINDOW - 20_000);
        // Reserve larger than window saturates to 0 (no compaction).
        assert_eq!(usable_tokens(Some(10_000), 20_000), 0);
    }

    #[test]
    fn needs_compaction_below_threshold() {
        let messages = vec![Message::new(Role::User, "hello")];
        assert!(!needs_compaction(&messages, Some(128_000)));
    }

    #[test]
    fn needs_compaction_above_threshold() {
        // usable = 128k - 20k = 108k tokens = ~432k chars
        let messages = vec![Message::new(Role::User, "x".repeat(500_000))];
        assert!(needs_compaction(&messages, Some(128_000)));
    }

    #[test]
    fn needs_compaction_with_custom_reserve() {
        // usable = 128k - 100k = 28k tokens = 112k chars
        let messages = vec![Message::new(Role::User, "x".repeat(120_000))];
        assert!(needs_compaction_with(&messages, Some(128_000), 100_000));
        // Same messages but tiny reserve → not yet compacting.
        let small = vec![Message::new(Role::User, "x".repeat(120_000))];
        assert!(!needs_compaction_with(&small, Some(128_000), 1_000));
    }
}
