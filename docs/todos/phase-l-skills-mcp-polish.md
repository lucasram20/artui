# Phase L — Skills, MCP, and Polish

Next batch of features for artui.

## L1: Skills Integration

- Define a skill manifest format (YAML/TOML) for reusable prompt templates
- Skills can be loaded from `~/.config/artui/skills/` or workspace `.artui/skills/`
- `/skill` slash command to list and invoke skills
- Skills can define: system prompt overrides, tool restrictions, output format hints
- Research how Claude Code skills and opencode plugins work for reference

## L2: MCP (Model Context Protocol) Integration

- Implement MCP client that connects to local MCP servers
- Support `stdio` and `sse` transport types
- Auto-discover MCP servers from `.artui/mcp.json` or config
- Register MCP tools alongside built-in tools in the ToolRegistry
- Forward tool calls to MCP servers and return results
- `/mcp` slash command to list connected servers and available tools

## L3: Auto-Compact Conversation

- Trigger compaction automatically when context usage reaches 80% of model's window
- Currently `needs_compaction` exists but is not wired into the main loop
- Wire `compact_if_needed` into the agent loop before each model request
- Show a brief "Compacting context..." status when triggered
- Preserve tool call history summaries during compaction

## L4: Powerful System Prompt

- Research Claude Code and Codex system prompts for structure/patterns
- Include: workspace awareness, file tree summary, git state, active tools list
- Add project-level context (detect language, framework, build system)
- Include coding conventions and safety guidelines
- Make system prompt composable (base + agent-specific + skill overrides)
- Add `/system` slash command to inspect the active system prompt

## L5: Installation Commands (curl/powershell)

- Set up GitHub Releases CI workflow for cross-platform binaries
- Linux/macOS: `curl -fsSL https://artui.dev/install.sh | sh`
- Windows: `irm https://artui.dev/install.ps1 | iex`
- Install script should detect arch (x86_64/aarch64), download binary, place in PATH
- Add `cargo install artui` as alternative
- Update README with installation section
