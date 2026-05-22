# Phase M6 — Codebase Indexer

**Phase:** M6 (production polish, deeper agent capability)
**Spec:** new — closes one of the largest visible gaps vs Claude Code
**Depends:** D (search tool), G (session store)
**Estimated PR size:** ~1500 LoC

---

## Why

Today `search` shells out to ripgrep — fast, but text-only. Claude
Code, Cursor, OpenCode, and Codex all index the workspace ahead of
time so the agent can do semantic queries ("find the function that
parses tool calls") without the model burning tokens on speculative
greps. artui currently re-greps from scratch every turn.

## Scope

### In scope

- Index two layers:
  1. **Symbol index** via tree-sitter — function/class names, file
     and line; per-language (start with Rust, TypeScript, Python).
  2. **BM25 text index** — sections of code/docs as searchable chunks
     (FTS5 in the existing rusqlite store from Phase G).
- `src/index/mod.rs` builds both at startup using `ignore::WalkBuilder`
  to respect `.gitignore`.
- File watcher (`notify` crate) updates indexes on save; debounced.
- New `search` tool modes:
  - `mode: "text"` — current ripgrep behaviour (default).
  - `mode: "semantic"` — BM25 over the chunk index.
  - `mode: "symbol"` — tree-sitter symbol lookup.
- `glob` tool gains a `mode: "indexed"` shortcut that reuses the
  walker without re-walking.
- Index size budget: hard cap at `[index] max_size_mb = 200`; LRU evict.

### Out of scope

- Vector embeddings (would require a model and API budget; defer to a
  cloud-only optional phase).
- Cross-repo / monorepo joins (workspace-only for now).
- Semantic graph (call graph, type graph). Tree-sitter symbols are
  enough for v1.

## Acceptance criteria

- "where is `run_turn` defined" → `search mode: "symbol"` returns
  `src/agent/loop.rs:42` without ripgrep needing to scan everything.
- File save → index updates within 1 s.
- 100k-file repo indexes in <30 s on cold start; <5 s on warm.
- Index lives at `~/.local/share/artui/index/<workspace_hash>/` with
  `0o600` perms.
- `cargo test` covers symbol parse, FTS5 query, watcher debounce.

## Files touched

| File | Change |
|---|---|
| `src/index/mod.rs` (new) | Public API |
| `src/index/symbols.rs` (new) | tree-sitter parsers per language |
| `src/index/text.rs` (new) | FTS5 chunker + writer |
| `src/index/watcher.rs` (new) | notify-based file watcher |
| `src/tools/search.rs` | New `mode` parameter |
| `src/tools/glob.rs` | New `mode = "indexed"` |
| `Cargo.toml` | tree-sitter, tree-sitter-{rust,typescript,python}, notify |
| Tests | Per-mode round trips |

## Risks

- **Tree-sitter binary bloat**: each grammar adds ~300 KB. Gate
  unused grammars behind cargo features.
- **Watcher reliability**: notify quirks on macOS (rename → delete +
  create). Fall back to a 5-min sweep.
- **FTS5 stop-words**: code identifiers like `i` and `it` get
  stripped by default. Use a custom tokenizer.
- **Index drift**: if artui is killed mid-write, FTS5 may corrupt.
  WAL mode + atomic temp-rename swap.

## References

- Cursor's local symbol index
- claude-code's semantic search hint
- tree-sitter-rust parser
