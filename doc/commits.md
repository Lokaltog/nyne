# Commits

Commit policy and workflow. Load this when committing changes.

## Policy

- **Commit without asking.** After every meaningful, self-contained change — commit immediately. Never ask permission.
- **Atomic commits.** Each commit is one logical unit. If it can be split, split it.
- **No mega-commits.** Commit as you go, not at the end.
- **Stage specific files.** Use `git add <file>`. Never `git add -A` or `git add .` — these risk including unintended files.

## Conventional Commits

Format: `type(scope): description`

### Types

| Type       | Use for…                                   |
| ---------- | ------------------------------------------ |
| `feat`     | New user-facing functionality              |
| `fix`      | Bug fixes                                  |
| `refactor` | Code restructuring without behavior change |
| `chore`    | Maintenance (deps, config, CI tweaks)      |
| `docs`     | Documentation changes                      |
| `test`     | Adding or modifying tests                  |
| `perf`     | Performance improvements                   |
| `ci`       | CI/CD pipeline changes                     |

### Scopes

Scope matches the affected module: `cli`, `config`, `fs`, `syntax`, `handler`. For cross-cutting changes, use the primary affected module. For truly cross-cutting work (deps, CI, tooling), omit the scope.

### Examples

```
feat(fs): add @deps cross-reference virtual file
fix(syntax): use floor_char_boundary to prevent UTF-8 slice panic
refactor(handler): extract do_* domain methods from FUSE trait impls
test(config): add garde validation edge case tests
docs: update architecture section in CLAUDE.md
chore: update clap to 4.x
```

### What Constitutes One Logical Unit?

- A refactoring + updating all its consumers → **one commit** (it's one logical change)
- A refactoring + an unrelated bug fix → **two commits** (separate concerns)
- Adding a test + the fix it validates → **one commit** (the test proves the fix)
- Adding a test for existing untested behavior → **its own commit**
- When a commit spans multiple modules (e.g., adding an agent type and wiring it into a command), scope to the primary domain — usually the module where the new logic lives.

## Documentation Sync

**Mandatory.** When a commit changes any of the following, update the affected prose documentation in the same commit.

| Change                                        | Update                                                |
| --------------------------------------------- | ----------------------------------------------------- |
| Add/remove/rename a CLI command               | Architecture table + command list in root `CLAUDE.md` |
| Add/remove a module                           | Architecture table in root `CLAUDE.md`                |
| Change domain patterns (e.g., new convention) | `doc/codebase.md` or `doc/conventions.md`             |
| Change tooling/build config                   | Root `CLAUDE.md` Environment section                  |
