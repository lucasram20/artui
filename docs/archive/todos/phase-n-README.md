# Phase N — Language Server Protocol Support

Wire LSP into artui so the agent can ask "where's this defined", "what calls
this", "is the file I just wrote diagnostic-clean", and "rename this symbol
everywhere" — without re-implementing those queries with grep + Tree-sitter.

Reference design: **oh-my-pi** `packages/coding-agent/src/lsp/` — a Rust port
of their writethrough-on-edit pattern, scoped to artui's tool-loop and
approval engine. The full architecture spec lives at
[`docs/spec/lsp.md`](../../spec/lsp.md); these tickets break it into
shippable phases.

Current state (v0.4.0): no LSP. Goal of this phase set: every popular CLI
agent (Codex, Claude Code, OpenCode, oh-my-pi) ships LSP — artui catches up
and surpasses on the writethrough-feedback loop.

## Reading order

| Order | Phase | Ticket | Outcome | Size |
|---|---|---|---|---|
| 1 | N1 | [phase-n1-lsp-skeleton.md](phase-n1-lsp-skeleton.md) | `lsp/` module, async-lsp dep, vendored ~80-server registry from helix-editor, single `lsp` tool with `definition` / `hover` / `status` | ~1500 LoC |
| 2 | N2 | [phase-n2-lsp-readonly-ops.md](phase-n2-lsp-readonly-ops.md) | references, implementation, type_definition, document_symbols, workspace_symbols, cached diagnostics | ~500 LoC |
| 3 | N3 | [phase-n3-lsp-writethrough.md](phase-n3-lsp-writethrough.md) | didOpen/didChange tracking; post-apply_patch publishDiagnostics pull; diagnostics rendered back into the tool result | ~400 LoC |
| 4 | N4 | [phase-n4-lsp-mutating-ops.md](phase-n4-lsp-mutating-ops.md) | rename + code_actions through the approval engine; workspace/applyEdit handling | ~600 LoC |

## Conventions

- Phase N1 ships under v0.5.0 (first LSP release).
- Phase N2 ships under v0.5.x as a patch (rounds out read-only ops).
- Phase N3 ships under v0.6.0 (the feedback loop is the headline feature).
- Phase N4 ships under v0.7.0 (gated on the approval engine M2 work).
- Every phase ticket follows the same shape as Phases A–M: scope,
  acceptance criteria, files touched, test plan, risks.
- The crate choice is `async-lsp` — see N1 for rationale.
- Bundled defaults cover rust-analyzer / gopls / pyright /
  typescript-language-server / clangd. User-defined servers via
  `~/.config/artui/lsp.toml`.
- Out-of-scope items end up in [parking-lot.md](parking-lot.md).
