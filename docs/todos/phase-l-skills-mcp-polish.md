# Phase L — Skills, MCP, and Polish

Next batch of features for artui.

## L1: Skills Integration

- Define a skill manifest format compatible with Vercel AI SDK agent skills
- Support community skill packages (npm-style registry or git URLs)
- Skills can be loaded from `~/.config/artui/skills/` or workspace `.artui/skills/`
- `/skill` slash command to list, install, and invoke skills
- Skills can define: system prompt overrides, tool restrictions, output format hints
- Skill manifest fields: name, description, version, author, triggers, prompt, tools
- Support skill composition (skills can depend on other skills)
- Research Vercel agent skills format, Claude Code skills, and opencode plugins for compatibility

## L1b: Plugin System

- Define a plugin API for extending artui with custom functionality
- Plugins can register: new tools, slash commands, UI panels, providers
- Plugin discovery from `~/.config/artui/plugins/` or workspace `.artui/plugins/`
- Support WASM-based plugins for sandboxed execution
- Support native Rust plugins (dynamic loading via cdylib)
- `/plugin` slash command to list, enable, disable plugins
- Plugin lifecycle hooks: on_init, on_message, on_tool_call, on_response
- Community plugin registry support (install from URL or registry)

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

## L5: Installation Commands (curl/powershell/npm)

- Set up GitHub Releases CI workflow for cross-platform binaries
- Linux/macOS: `curl -fsSL https://artui.dev/install.sh | sh`
- Windows: `irm https://artui.dev/install.ps1 | iex`
- npm: `npm install -g artui` (publish as npm package wrapping native binary)
- Install script should detect arch (x86_64/aarch64), download binary, place in PATH
- npm package uses postinstall script to download platform-specific binary (like turbo, esbuild pattern)
- Add `cargo install artui` as alternative
- npx support: `npx artui` for one-off usage without global install
- Update README with installation section covering all methods
