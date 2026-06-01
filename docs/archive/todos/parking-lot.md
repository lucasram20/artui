# Parking lot

Items that are out-of-scope for the M-series production-polish
phases but worth noting so we don't lose track.

| Item | Why deferred |
|---|---|
| Cross-machine session sync | Full distributed-state design needed; defer until single-machine UX is solid. |
| Vector embedding index | Requires API budget + a model contract; phase M6 ships BM25 only. |
| AppContainer / Hyper-V Windows isolation | Job Object + restricted token (Phase M5) is enough for v1; AppContainer needs UWP packaging. |
| Public plugin marketplace | M-series only ships the loader spec (in Phase L). Community + curation work later. |
| iOS / Android TUI port | ratatui is desktop-shaped; mobile would need a fresh rendering layer. |
| GUI front-end | Out of scope for "TUI coding agent" identity. |
| Provider gateway / proxy | Solved by tools like LiteLLM; artui stays a client. |
| Built-in code-execution sandbox (run-in-WASM) | Needs WASM tooling that doesn't exist yet for the languages we target. |
| Session timeline scrubber UI | Cool, but not a Tier-S blocker. Phase Z if ever. |
| Self-hosted update server (replaces R2) | Not until users explicitly request air-gapped mode. |

If any of these become important, promote them to a real `phase-*.md`
ticket and fold into the next milestone.
