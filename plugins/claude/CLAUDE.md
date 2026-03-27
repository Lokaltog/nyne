# nyne-plugin-claude

Claude Code integration — hooks, settings, skills, and system prompt injection. Module structure discoverable via VFS.

## Dependencies

- **nyne** (core): Provider trait, dispatch types, templates, script system
- **nyne-plugin-source**: `Services` for decomposition in hooks
- **nyne-plugin-lsp**: `Manager` for diagnostics in post_tool_use (via TypeMap)
- **nyne-plugin-analysis** (optional, feature-gated): `Engine` for code smell hints in post_tool_use
- **nyne-plugin-git**: `Repo` for branch display in session-start and statusline hooks

## Config

Plugin config: `[plugin.claude]` in `~/.config/nyne/config.toml` or project-level `.nyne/config.toml` / `.nyne.toml` / `nyne.toml`.

- `config/mod.rs`: `Config` — deserialized via `Config::from_plugin_config()`
- `enabled` (bool, default true) — master toggle for `.claude/` directory and all hooks
- `hooks`: per-hook toggles — `session_start`, `pre_tool_use`, `post_tool_use`, `stop`, `statusline` (all default true)
- `hook_config.pre_tool`: `PreToolHookConfig` with per-filetype policy overrides
- `hook_config.stop`: `StopHookConfig` with `min_files` and `ignore_extensions`

## Hook Scripts

Scripts are registered via `script_entries()` in `provider/mod.rs` (driven by `HOOK_REGISTRY` in `settings/mod.rs`) and execute as `Script` trait objects. Each hook accesses `Services` from the TypeMap for decomposition, `Manager` for diagnostics, and `Engine` for code smell hints — all with graceful degradation when absent.

- `pre_tool_use` — intercepts Read/Edit/Write/Bash/Grep, provides VFS hints or denies broad reads
- `post_tool_use` — runs analysis + fetches LSP diagnostics scoped to the changed region
- `session_start` — surfaces mount status and project context
- `stop` — SSOT/DRY review when files were changed
- `statusline` — renders ANSI status bar from Claude Code JSON payload
