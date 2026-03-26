# nyne-plugin-claude

Claude Code integration — hooks, settings, skills, and system prompt injection. Module structure discoverable via VFS.

## Dependencies

- **nyne** (core): Provider trait, dispatch types, templates, script system
- **nyne-plugin-coding**: `CodingServices` for decomposition, LSP, and analysis in hooks (optional — graceful degradation)
- **nyne-plugin-git**: `GitRepo` for branch display in session-start and statusline hooks

## Config

Plugin config: `[plugin.claude]` in `~/.config/nyne/config.toml` or project-level `.nyne/config.toml` / `.nyne.toml` / `nyne.toml`.

- `config/mod.rs`: `ClaudePluginConfig` — deserialized via `ClaudePluginConfig::from_plugin_config()`
- `enabled` (bool, default true) — master toggle for `.claude/` directory and all hooks
- `hooks`: per-hook toggles — `session_start`, `pre_tool_use`, `post_tool_use`, `stop`, `statusline` (all default true)
- `hook_config.pre_tool`: `PreToolHookConfig` with per-filetype policy overrides
- `hook_config.stop`: `StopHookConfig` with `min_files` and `ignore_extensions`

## Hook Scripts

Scripts are registered via `script_entries()` in `provider/mod.rs` and execute as `Script` trait objects. Each hook accesses `CodingServices` from the `TypeMap` for decomposition/LSP/analysis when available.

- `pre_tool_use` — intercepts Read/Edit/Write/Bash/Grep, provides VFS hints or denies broad reads
- `post_tool_use` — runs analysis + fetches LSP diagnostics scoped to the changed region
- `session_start` — surfaces mount status and project context
- `stop` — SSOT/DRY review when files were changed
- `statusline` — renders ANSI status bar from Claude Code JSON payload
