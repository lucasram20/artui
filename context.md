# Context percentage + model metadata findings

## Current `ctx N%` implementation

- Display path: `src/ui/layout.rs:699-704`
  - Prompt title renders `ctx {}% · {}` via `input_title_left(app)`, combining `context_percent(app)` and `app.provider_usage_label()`.
- Calculation: `src/ui/layout.rs:749-756`
  - `used` = sum of `message.content.chars().count()` for every transcript message.
  - `budget` = `app.config.agent.max_tool_output_chars.max(1)`.
  - Percent = `(used * 100 / budget).min(100)`.
- Problem: this is not model context usage. It compares transcript character count to an agent tool-output truncation limit (`30_000` by default), so it can be wildly wrong for large-context models and ignores system prompt/tool schema/current input/tokenization.

## Config/provider model context availability

- Config schema has agent output/read/search limits but no context-window/model-context fields: `src/config/schema.rs:92-113`.
  - `max_tool_output_chars` default is `30_000`; this is the field currently misused as context budget.
- Provider configs only carry default model/listing/connectivity fields, not context size:
  - Ollama: `host`, `default_model` (`src/config/schema.rs:125-138`)
  - OpenAI-compatible: `base_url`, `api_key_env`, `default_model` (`src/config/schema.rs:141-156`)
  - OpenAI account: `base_url`, `default_model` (`src/config/schema.rs:159-164`)
  - Copilot: endpoints, discovered `models`, timeout, `default_model` (`src/config/schema.rs:166-204`)
- Provider registry metadata is provider-level only: id, display name, auth requirement, model list strategy, streaming (`src/providers/registry.rs:37-75`). No per-model context metadata.
- Copilot model discovery parses/persists some per-model metadata, but not context window:
  - Parsed fields: `id`, `model_picker_enabled`, `supported_endpoints`, `policy`, `capabilities.supports.reasoning_effort` (`src/providers/copilot.rs:1176-1229`).
  - Persisted endpoint metadata: `id`, `api`, `supported_endpoints`, `reasoning_efforts` (`src/providers/copilot.rs:1235-1257`).
  - Stored into auth metadata as `models` + `model_endpoints` during provider discovery (`src/providers/copilot.rs:386-398`) and as `model_endpoints` during manual refresh (`src/providers/copilot.rs:442-456`).
  - App reads stored endpoint metadata only for `/model` hints and reasoning effort support (`src/app.rs:1727-1774`).
- `/model` UI displays only model name + hint (`src/ui/popups.rs:301-307`). Hints currently include route/reasoning/embeddings labels, not context size (`src/app.rs:1776-1785`).
- `Cargo.toml:13-25` has no tokenizer dependency; exact token counting is not currently available.

## Related status/usage display

- `provider_usage_label()` is separate from context percent (`src/app.rs:1351-1364`): Ollama local/cloud, Copilot cached quota label from auth metadata, OpenAI-compatible `api`, OpenAI account placeholder. This should remain separate from context usage.

## Likely fix plan

1. Stop using `max_tool_output_chars` as context budget.
2. Add an explicit model context-window source, with fallbacks:
   - Minimal configurable fix: add optional per-provider `context_window_tokens` (or `context_window_chars` if avoiding token claims) and maybe provider/model override map in `config/schema.rs`.
   - Better Copilot-aware fix: extend `CopilotModel` and `CopilotModelEndpointMetadata` to parse/store any context-window fields returned by the models endpoint if present; keep config overrides as fallback because other providers do not expose discovery metadata locally.
3. Replace `context_percent(app)` with a helper that uses:
   - estimated prompt usage = system prompt + transcript + current input (current code only uses transcript),
   - denominator = active model context window from metadata/config/known fallback,
   - label fallback like `ctx ?` or hide percent when no reliable context window exists.
4. If no tokenizer is added, label/implement it clearly as an approximation (e.g. chars/4 token estimate) rather than exact token percentage. For exactness, add a tokenizer dependency and provider-specific encodings.
5. Add tests around the helper: no budget => unknown/0?; configured budget; transcript+input/system included; no divide-by-zero; percent capped at 100; Copilot stored metadata/config override path.

## Main risks/constraints

- Existing config format should remain backward-compatible via `#[serde(default)]` option fields.
- Copilot model endpoint field names for context length are not currently known from local code; parser should be permissive/optional and not break if absent.
- Exact token counts vary by provider/model; a cheap char estimate is better than current tool-limit percent but should not be presented as exact.
- Avoid mixing quota/usage (`provider_usage_label`) with context-window usage; they are different concepts.
