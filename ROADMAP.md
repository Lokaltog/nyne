# nyne Roadmap

Feature roadmap and design notes. Items are grouped by domain and roughly ordered by priority within each group. Status markers: **planned** (committed direction), **exploring** (design not finalized), **speculative** (may not happen).

## Recursive VFS — planned

Virtual files fed back through the provider pipeline. Any virtual file with recognizable content becomes decomposable through the same router/providers as real files:

- Extracted markdown code blocks → symbol browsing: `doc.md@/symbols/20-getting-started/code/10.rs@/symbols/main.rs`
- Historical file versions → symbol browsing: `file.rs@/history/2025-01-15-abc1234.rs@/symbols/process.rs`
- Branch/tag file views → full decomposition: `@/git/branches/feature/src/lib.rs@/symbols/process.rs`
- Fetched web pages → section/code block traversal (see Web Integration)

Providers selectively expose features based on availability (tree-sitter support, git tracking, LSP coverage), so recursion is naturally bounded — no special casing needed. A code block without a recognized language is a passthrough leaf. A historical file version gets symbols but not LSP intelligence.

The splice chain propagates back through the recursion: editing a symbol inside a code block inside a markdown section splices back through each layer to the original source file.

## Test Extraction — planned

Individual test blocks extracted per-syntax into navigable, editable virtual files:

```
file.rs@/tests/10-it-validates-input.rs      # individual test: read, write (splices back)
file.rs@/tests/20-it-handles-timeout.rs      # numbered by source order, slugified from test name
file.rs@/tests/OVERVIEW.md                   # test listing with token estimates, pass/fail status
```

Same 10-increment numbering pattern as other ordered virtual entries. Test names are derived from the test function/method name (language-specific extraction rules). With recursive VFS, each extracted test file is itself decomposable — `file.rs@/tests/10-it-validates-input.rs@/symbols/...` for tests with complex setup.

### Test runner integration — exploring

```
file.rs@/tests/RESULTS.md                    # aggregated pass/fail results (read triggers test run, or cached)
file.rs@/tests/10-it-validates-input/RESULT.txt  # per-test result: pass/fail, stdout, stderr, duration
```

Trigger mechanisms under consideration:

- Read `RESULTS.md` to run all tests for the file
- Read individual `RESULT.txt` to run a single test
- `touch @/test/file.rs` to trigger file-level test run
- `touch @/test/file.rs@/symbols/Foo/` to run tests related to a specific symbol

## Task Management — planned

Agent task list in the root `@/` directory, backed by SQLite for persistence, transaction logging, and audit trails.

```
@/tasks/10-implement-validation.md            # task: description, context, acceptance criteria
@/tasks/20-fix-auth-bug.md
@/tasks/pending/                              # not yet started
@/tasks/active/                               # in progress
@/tasks/done/                                 # completed
@/tasks/deferred/                             # parked
```

Agent workflow: `ls @/tasks/pending/` → pick a task → `mv @/tasks/pending/10-implement-validation.md @/tasks/active/` → work → `mv @/tasks/active/10-implement-validation.md @/tasks/done/`.

Tasks are writable — agents append progress notes, blockers, decisions. All state transitions are logged to the SQLite audit trail.

### Task context links — exploring

Task directories include symlinks to relevant VFS entries:

```
@/tasks/active/10-implement-validation/context/
# → symlinks to relevant symbols, files, diagnostics
```

Enables jumping directly from a task description into the code that needs changing.

### Transaction/audit log — exploring

SQLite-backed log of all task state transitions, edits, and agent actions. Enables:

- Post-session review of what the agent did and why
- Analytics on agent efficiency (time per task, edit/retry ratios)
- Replay/undo of task state

## Web Integration — exploring

### Page fetching

URLs as paths, rendered to navigable markdown with recursive VFS decomposition:

```sh
# Fetch a page — trailing @ signals end-of-URL
ls @/url/docs.rs/tokio/latest/tokio/sync/struct.Mutex.html@/

# Sections as individual markdown files
cat @/url/.../struct.Mutex.html@/symbols/10-overview.md

# Code examples extracted and browsable
ls @/url/.../struct.Mutex.html@/symbols/20-methods/code/
# → 10.rs, 20.rs

# Recursive VFS: symbols inside fetched code examples
cat @/url/.../struct.Mutex.html@/symbols/20-methods/code/10.rs@/symbols/main/body.ext
```

### Web search

Readdir-interception pattern — query is the path, results materialize as symlinks:

```sh
ls @/web/"rust tokio mutex example"/
# → @/url/docs.rs/.../struct.Mutex.html@/
# → @/url/stackoverflow.com/questions/12345@/
```

Results are symlinks to `@/url/` entries, which are themselves navigable markdown trees.

## Diff-First Mutation Enhancements — planned

### File-level aggregated code actions

Per-file `actions/` directory that aggregates code actions across all symbols in the file:

```
file.rs@/actions/                            # aggregated across all symbols in the file
  10-organize-imports.diff
  20-add-missing-fields-Foo.diff
```

After any write that introduces diagnostics, code actions for the affected symbols are updated automatically. This turns "read error → reason about fix → write fix" into "read error → inspect action diff → apply."

### Cross-file move preview

Preview the full impact of moving a symbol between files:

```sh
cat file.rs@/symbols/helper@/move/other.rs.diff
```

The `move/` directory is lookup-only. The diff includes: body insertion in target file, import additions in target, import removals from source, and a summary of callers that need import path updates.

## Search — exploring

Filesystem-native search via readdir interception. Query encoded as path component, results materialize as symlinks:

```sh
# Symbol search
ls @/search/symbols/"process"/
# → file.rs@/symbols/process/
# → other.rs@/symbols/process_batch/

# Regex search — results are symlinks to line ranges
ls @/search/grep/"TODO.*fixme"/
# → file.rs@/lines:42
# → lib.rs@/lines:108-110

# Scoped search
ls src/handlers/@/search/symbols/"validate"/

# Type-filtered search
ls @/search/functions/"process"/
ls @/search/structs/"Config"/
```

## VFS Conventions — planned

### `@/events` — named pipe (FIFO)

Root-level and per-file event streams:

```
@/events                                     # all changes across the project
file.ext@/events                             # changes to this file only
```

Streams structured change events: `modified Foo/process`, `deleted old_helper`, `renamed Bar → Baz`. Enables real-time monitoring without polling.

### `@/format/<language>` — virtual formatter executables

Per-language formatter scripts, generated on-demand from Jinja2 templates. Each syntax provider contributes a language-specific template that includes a global shell boilerplate base template (shebang, error handling). The formatter accepts stdin and outputs formatted code to stdout:

```sh
cat messy.rs | @/format/rust >file.rs@/symbols/process@/body.ext
echo "def foo():pass" | @/format/python
```

Executable bit set to distinguish from other virtual files. Initial implementation redirects to standard formatters (`rustfmt`, `black`, etc.).

### Bidirectional imports directory

`symbols/@/imports/` as a directory of symlinks to imported files/symbols, in addition to the flat `imports.ext` file:

```
file.rs@/symbols/@/imports/Config            # → other.rs@/symbols/Config@/ (project symbol)
file.rs@/symbols/@/imports/HashMap           # regular file: "use std::collections::HashMap;" (external)
```

- `rm imports/Config` removes the import statement
- `cp other.rs@/symbols/NewType@/ file.rs@/symbols/@/imports/` adds the import (nyne infers syntax from target path)
- Symlinks for project symbols, regular files for external/stdlib imports

### PostToolUse — signature change hook

After writing to `signature.ext`, inject context about callers that may need updating. Surfaces `CALLERS.md` content so the agent can immediately see what call sites are affected.

## Blocking Write Semantics — planned

Writes to managed symbol files (`body.rs`, `signature.rs`, `imports.rs`, etc.) are **blocking**. The write does not return until the full validation pipeline completes:

1. **Tree-sitter parse** (~1ms) — reject malformed syntax immediately (EINVAL)
2. **Splice** content into the source file
3. **Format** via configured formatter (~10-50ms)
4. **LSP notification** — send `textDocument/didChange`
5. **Wait for LSP diagnostics** with configurable timeout (default 2s)
6. **Store diagnostics** — update `user.diagnostics_count` and `user.error` xattrs

This guarantees that PostToolUse hooks read current diagnostics after every edit. The hook can immediately inject errors/warnings into agent context without a separate diagnostic read.

### Per-provider constraint checking

Syntax providers register custom validators that run as part of the blocking write pipeline. These leverage LSP diagnostics queried immediately after splice:

- **Trait impl completeness:** writing an impl body missing required methods → surface missing methods in diagnostics
- **Unused imports:** write removes last usage of an imported type → surface in mutation_summary
- **Type mismatches:** return expression doesn't match signature → surface via LSP error diagnostics

The validation is "wait for LSP diagnostics and surface them synchronously" — the LSP does the actual checking, nyne just tightens the feedback loop.

### Timeout behavior

If LSP doesn't respond within the timeout, the write still succeeds but `user.diagnostics_count` is set to `pending`. PostToolUse hooks can detect this and either wait or skip injection. Timeout is configurable per-mount in `config.toml`.

## Editing Enhancements — planned

### Indentation-aware symbol splicing

Cross-scope symbol moves auto-indent to match the target's nesting level. Critical for moving functions between scopes (top-level → method, between different nesting depths). Triggered by `mv` reparenting: `mv symbols/func/ symbols/Class/func/`.

### Smart append heuristic

When appending to `body.ext`, detect whether content is a new sibling symbol (insert-after) vs. content that belongs inside the body (append-to-body). If the appended content parses as a standalone symbol at the parent's scope level, treat as insert-after; otherwise append inside.

### Stale-content detection

When writing to `body.ext`, reject if the source file's generation has changed since the last read. Prevents silently splicing against stale offsets.

### `mkdir` to create symbol scope

`mkdir file.rs@/symbols/new_func` creates a new empty symbol scope. Write to `body.ext` within it to populate. Complementary to the read-to-create scaffold pattern — mkdir is explicit creation, reading a non-existent path returns a scaffold.

### `mv` symbol reparenting

Re-scope a symbol with indent adjustment: `mv file.rs@/symbols/func/ file.rs@/symbols/Class/func/`.

## Impact Analysis — exploring

Expose the transitive blast radius of changing a symbol's interface. Goes beyond `callers.md` (direct callers) to show the full dependency chain.

```
file.rs@/symbols/Config@/IMPACT.md
# Impact analysis for Config
#
# Direct references (8 symbols in 4 files):
#   src/main.rs: init (L15), setup (L42)
#   src/server.rs: start (L8), configure (L30)
#   src/handler.rs: new (L12), process (L45), validate (L78), reset (L92)
#
# Transitive callers (depth 3, 23 symbols in 9 files):
#   L1: init, setup, start, configure, new, process, validate, reset [direct]
#   L2: main, run_server, handle_request [calls L1]
#   L3: integration_test_setup [calls L2]
#
# Estimated edit scope: 31 symbols, ~2800 tokens

file.rs@/symbols/Config@/impact/
  direct/                                    # symlinks to directly affected symbols
  transitive/                                # symlinks to transitively affected symbols
```

### Implementation approach

Combines two LSP capabilities:

**Direct impact** — LSP `textDocument/references`:

1. Query references for the symbol
2. For each reference location, identify the containing symbol (binary search the file's symbol table by line range)
3. Group by file → symbol

**Transitive impact** — LSP `callHierarchy/incomingCalls` (recursive):

1. Resolve the symbol as a call hierarchy item (`callHierarchy/prepare`)
2. Recursively call `callHierarchy/incomingCalls` up to depth N (default 3)
3. At each level, collect the calling symbols

**Combined:** call hierarchy for call-based impact (transitive), `textDocument/references` for type/usage-based impact (direct). Type references are not followed transitively (less meaningful than call chains).

**Caching:** cache per `(symbol_path, file_generation)`. Invalidate on any write to the file containing the symbol (generation bump).

## Contextual Scope — planned

Surface what types, traits, and methods are available at a given symbol's scope. Reduces "read imports → chase definitions → read type" to a single file read.

```
file.rs@/symbols/Foo~Impl@/new@/SCOPE.md
# Scope context for Foo::new
#
# Parameters:
#   config: Config          → src/config.rs@/symbols/Config@/
#   name: String
#
# Self type: Foo
#   Fields: inner (State), name (String), config (Config)
#   Methods: process(), validate(), shutdown()
#
# Traits in scope:
#   Default, Clone, Debug           (derived)
#   From<Config>                    (src/lib.rs L78)
#   Display                         (src/lib.rs L42)
#
# Available types (via imports):
#   Config, State, Handler, ProcessError
#   HashMap, Arc, Mutex             (std)
```

### Dual exposure

1. **As a file** — `SCOPE.md` in the symbol's `@/` namespace for explicit reads
2. **As a hook injection** — PreToolUse hook injects a condensed version when the agent is about to write to `body.rs` or `signature.rs`, providing up-to-date context to guide the edit

The hook injection is a condensed single-paragraph version (types + traits + Self methods). The file is there for when the agent wants the full picture.

### Implementation

- LSP `textDocument/completion` at the symbol's opening brace position → available completions
- LSP `textDocument/hover` on `Self` → type info and fields
- Tree-sitter extraction of parameters, local variables, field access patterns
- Combine into structured markdown

## Agent Memories — planned

Persistent, file-linked notes that survive across sessions. Agents write observations, gotchas, and decisions; nyne indexes them by file and tag for automatic retrieval in future sessions.

```
@/memories/
  2025-03-09-refactor-router.md              # session memory (free-form markdown)
  2025-03-08-fix-cache-bug.md

@/memories/tags/
  cache/                                     # symlinks to memories tagged 'cache'
  router/
  performance/

@/memories/files/
  src/dispatch/router.rs/                    # symlinks to memories linked to this file
  src/dispatch/cache.rs/
```

### Write flow (post-commit hook)

1. Agent commits code
2. PostToolUse hook fires on `git commit`
3. nyne computes touched files from the commit (`git diff HEAD~1 --name-only`)
4. Hook injects reminder: "Write session observations to `@/memories/`. Files touched: [list]"
5. Agent writes a memory file to `@/memories/`
6. nyne auto-links the memory to all files touched in the session (stored in SQLite: `memory_links(memory_path, file_path)`)
7. nyne extracts tags from `#tag` markers in the content

### Retrieval flow (pre-tool-use hook)

1. Agent is about to read or edit a file
2. PreToolUse hook checks `@/memories/files/{path}/`
3. If memories exist, inject into context: "Previous session notes about this file: [condensed content]"
4. Only inject the most recent N memories (configurable) to avoid context bloat

### Memory format

Free-form markdown. nyne recognizes optional structure:

```markdown
# Refactored router cache layer

## Gotchas

- DashMap guards must be dropped before acquiring other locks (deadlock)
- RefMut return values need binding to locals before &mut self calls

## Decisions

- Used pub(super) for CachedNode — only Router needs it
- Kept L1/L2 cache split despite complexity — latency matters

#router #cache #performance
```

### Storage

`~/.local/state/nyne/memories/{project-hash}/` — persists across mounts, scoped per project. SQLite for the link/tag index, markdown files for content.

## Cross-File Symbol Move — exploring

`mv file.rs@/symbols/helper/ other.rs@/symbols/` as a compound action that resolves dependencies automatically.

### Sequence

1. **Validate** — refuse with EINVAL if:
   - Symbol has `self`/`Self` references (bound to its impl block)
   - Target is a different scope level without explicit reparent
   - Symbol is private and has callers outside its module
2. **Compute imports** — identify types used by the symbol that the target file doesn't already import
3. **Move body** — insert into target file (after last symbol, or at specified position)
4. **Update imports** — add required imports to target, remove now-unused imports from source
5. **Surface callers** — mutation_summary lists all call sites that reference the old module path

### Diff-first preview

Following the diff-first pattern, preview before executing:

```sh
# Preview the full move as a unified diff
cat file.rs@/symbols/helper@/move/other.rs.diff

# Or execute directly
mv file.rs@/symbols/helper/ other.rs@/symbols/
```

The `move/` directory is lookup-only. The diff includes body insertion, import additions/removals, and a comment listing callers that need import path updates.

### Guardrails

- Refuse symbols with `self`/`Self` references out of their impl block
- Refuse cross-scope moves (top-level → method) without explicit reparent syntax
- Show import path changes in mutation_summary so the agent can verify
- For private symbols with callers: include caller update instructions in mutation_summary rather than silently breaking them

## Completions — exploring (low priority)

Expose method signatures as lightweight files for quick API reference without reading full bodies:

```
file.rs@/symbols/Router@/completions/
  new.md              # fn new(config: Config, providers: Vec<Box<dyn Provider>>) -> Self
  resolve.md          # fn resolve(&self, path: &VfsPath) -> Result<ResolvedInode>
  invalidate.md       # fn invalidate(&self, path: &VfsPath, reason: InvalidateReason)
```

Each file contains just the signature + doc comment — the minimum needed to write a correct call. Cheaper than reading the full body. This is what an IDE's autocomplete popup shows, but as files. Generated from LSP completion items.

## Snapshots — speculative (low priority)

Sub-git-granularity checkpoint/restore for exploratory coding:

```sh
mkdir @/snapshots/try-new-parser                  # checkpoint current state
ls @/snapshots/try-new-parser/                    # browse files as they were
mv @/snapshots/try-new-parser @/snapshots/restore # roll back
rm -r @/snapshots/try-new-parser                  # discard
```

Backed by copy-on-write (reflinks on btrfs/xfs, `cp --reflink=auto`). Lightweight, instant, no git noise. Enables agents to try multiple approaches and pick the best one.

## Rich Reference Files — planned

### CALLERS.md and REFERENCES.md with inline code context

Reference files must include **actual code at the reference site**, not just locations. This is the difference between "the agent needs N additional reads" and "the agent has full refactoring context in one read."

````markdown
## init — src/main.rs (L15-16)

```rust
let result = process(input, &config)?;
handle_result(result);
```
````

## start — src/server.rs (L42)

```rust
let output = process(request.body(), &server_config)?;
```

````

Each entry includes: containing symbol name, file path, line range, and the code at the call/reference site with 1-2 lines of surrounding context. Generated from LSP call hierarchy / references + source extraction.

### Writable USAGES.md

Aggregates all usage sites of a symbol as editable code blocks, mapped back to source locations. Enables bulk cross-file edits with a single Edit tool call.

```markdown
<!-- src/main.rs@/symbols/init@/body.rs:15 -->
```rust
let result = process(input, &config)?;
````

<!-- src/server.rs@/symbols/start@/body.rs:42 -->

```rust
let output = process(request.body(), &server_config)?;
```

```

**Editing workflow:**

```

Edit USAGES.md:
old_string: "process("
new_string: "process(Duration::from_secs(30), "
replace_all: true

```

nyne parses the HTML comment markers, maps each code block to its source location, and splices changes back to the originating files. One Read + one Edit = all call sites updated across all files.

The HTML comment metadata (`file@/symbols/symbol@/body.rs:line`) survives Edit tool find-and-replace operations since only code block content is modified. Only code block content is writable — markdown headers and comments are read-only structural markers.

### Importers directory

`importers/` shows which files have `use`/`import` statements that reference this file or symbol. Distinct from `references/` which shows all usage sites — `importers/` is scoped to import declarations only.

```

file.rs@/importers/ # files that import from this file
main.rs → main.rs@/symbols/@/imports/
server.rs → server.rs@/symbols/@/imports/

file.rs@/symbols/process@/importers/ # files that import 'process' specifically
main.rs → main.rs@/symbols/@/imports/process
server.rs → server.rs@/symbols/@/imports/process

file.rs@/IMPORTERS.md

# src/main.rs: use crate::lib::{process, Config}

# src/server.rs: use crate::lib::process

# src/handler.rs: use crate::lib::\*

````

Essential for cross-file moves: after moving a symbol between files, nyne (or PostToolUse hooks) references `importers/` to surface exactly which import statements need updating.

## Workspace Symbols — exploring

Browsable index of all symbols across the project, accessible via both tree-sitter and LSP backends.

### Tree-sitter based

Directory with symlinks to all workspace symbols for easy lookup with tools like `fd`:

```
@/symbols/process_data          → src/lib.rs@/symbols/process_data@/
@/symbols/Config                → src/config.rs@/symbols/Config@/
```

May use a filtered directory pattern (`@/search/substring/...` → symlinks) instead of a flat listing, depending on performance characteristics with large codebases.

### LSP based

LSP `workspace/symbol` results exposed the same way, providing richer type-aware results for languages with strong LSP support. Falls back to tree-sitter when LSP is unavailable.

## Decorators — planned

### `decorators.<ext>`

Extract decorators/annotations per symbol as a writable, appendable, truncatable virtual file:

```
file.py@/symbols/process@/decorators.py
file.rs@/symbols/process@/decorators.rs
```

Enables reading, modifying, or clearing decorators independently of the symbol body.

## Line Range Exposure — exploring

Body/decorators/imports should expose their line ranges to agents. Symlink approach was tried and reverted (editors resolve symlinks at open time, binding writes to stale line ranges after edits change line counts). Alternative approaches under consideration:

- xattr (`user.range`)
- Embed in OVERVIEW.md (already done for top-level symbols)
- Read-only companion symlink alongside the virtual file

## Extended Attributes (xattrs) — planned (partially implemented)

Expose metadata via filesystem extended attributes:

- `user.kind` — symbol kind (fn, struct, etc.)
- `user.range` — line range
- `user.language` — source language
- `user.hash` — content hash
- `user.tags` — symbol tags
- `user.tokens` — estimated token count
- `user.generation` — file generation counter
- `user.error` — last error message
- `user.git_status` — git status for the file
- `user.diagnostics_count` — number of active diagnostics

Some of these are already implemented. Completing the full set enables richer hook integrations and agent decision-making.

## Directory-Level Enhancements — exploring

### Directory-level `symbols/`

Symlinks to all symbols across files in a directory:

```
src/@/symbols/Config             → src/config.rs@/symbols/Config@/
src/@/symbols/process_data       → src/lib.rs@/symbols/process_data@/
```

Enables `ls src/@/symbols/` for a flat view of all symbols in a directory subtree.

## Discovery and Introspection — exploring

### @/todo/ — aggregated TODO/FIXME/HACK

Project-wide comment extraction as symlinks to line ranges or symbol bodies:

```sh
ls @/todo/
# → file.rs@/lines:42   "TODO: handle timeout"

# Scoped to directory
ls src/handlers/@/todo/
````

### @/metrics/ — code complexity

Per-file and per-symbol metrics: cyclomatic complexity, cognitive complexity, line count, dependency fan-in/fan-out.

```
file.rs@/METRICS.md
file.rs@/symbols/Foo@/METRICS.md
dir@/METRICS.md
```

### Symbol CONTEXT.md — context bundle

Synthesized "everything you need" for a symbol: body + used imports + referenced type definitions + docstring + active diagnostics. One read for full context. Expensive but high-value for complex functions.

### Symbol scaffolding via read-to-create — planned

Reading a non-existent symbol path returns a Jinja2-templated scaffold for that language and symbol kind, pre-populated with the name from the path:

```sh
# Reading a symbol that doesn't exist returns a scaffold
cat file.rs@/symbols/ValidateInput.rs
# → returns a templated struct/impl skeleton named ValidateInput

# Write it back to insert it into the source file
cat file.rs@/symbols/ValidateInput.rs >file.rs@/symbols/ValidateInput.rs

# Or use it as a starting point — pipe through sed, then insert after another symbol
cat file.rs@/symbols/ValidateInput.rs |
    sed 's/todo!()/self.inner.validate(input)?/' \
        >>file.rs@/symbols/OtherSymbol.rs

# Writing directly to a non-existent symbol populates the body and inserts it
echo 'fn validate(input: &str) -> Result<()> { Ok(()) }' >file.rs@/symbols/validate.rs
```

Default templates are provided for all supported languages, powered by Jinja2 (`minijinja`). Template selection is based on:

- Symbol kind inference from the name (e.g., `PascalCase` → struct/class, `snake_case` → function)
- Language conventions
- Context (e.g., inside an impl block → method template)

Users can override templates via `~/.config/nyne/templates/`.

### @/templates/ — explicit scaffolding directory — exploring

For cases where the read-to-create pattern doesn't fit (e.g., creating a new file with boilerplate):

```sh
echo "MyNewService" >@/templates/rust/struct
echo "validate_input" >@/templates/python/function
```

## Co-Change Analysis — exploring

Mine git history for symbols that historically change together. Agents lack institutional knowledge — this provides the statistical equivalent.

```
file.rs@/symbols/process@/git/CO-CHANGED.md
# Symbols that historically change with process:
# validate           80% (24/30 commits)  → file.rs@/symbols/validate@/
# ProcessError       65% (13/20 commits)  → types.rs@/symbols/ProcessError@/
# handle_timeout     45% (9/20 commits)   → file.rs@/symbols/handle_timeout@/

file.rs@/symbols/process@/git/co-changed/
  validate           → ../../validate@/              # symlink
  ProcessError       → ../../../types.rs@/symbols/ProcessError@/
```

### Implementation approach

**Algorithm:**

1. Walk commits touching the file via `git2::Repository::revwalk`
2. For each commit, get diff hunks via `git2::Diff::foreach` with hunk callback
3. Map each hunk's changed line range to intersecting symbols — requires tree-sitter parse of the blob at that commit's tree (`commit.tree().get_path()`)
4. Build co-occurrence matrix: for each commit, every symbol pair modified together gets +1
5. Normalize: `co_change_rate(A, B) = co_occurrences(A, B) / max(commits_touching(A), commits_touching(B))`
6. Threshold at ≥30% to filter noise

**Cross-file co-change:** same algorithm but across files in the same commit. Stored separately: `cross_file_co_changes(symbol_a_file, symbol_a_name, symbol_b_file, symbol_b_name, rate)`. Computed on-demand for the queried symbol only.

**Caching:**

- Cache per `(file_path, HEAD_commit_sha)` in SQLite
- Schema: `co_changes(file_path TEXT, symbol_a TEXT, symbol_b TEXT, rate REAL, co_count INT, total_count INT)`
- Invalidate when HEAD changes. Incremental update: only re-walk commits since last cached HEAD.

**Performance:**

- Lazy computation — only computed on first read of `co-changed/`
- Depth limit — walk last N commits (configurable, default 500)
- File-scoped — compute for the requested file only, not project-wide
- Tree-sitter parsing of historical blobs is the expensive part; cache parsed symbol tables per `(file_path, blob_oid)`

## Git Enhancements — exploring

### @/git/STATUS.md enhancements

Add stash count and recent commits summary to the existing STATUS.md output.

### Directory-level git

Expose git information at the directory level:

```
dir@/git/LOG.md                              # commit log for files in the directory
dir@/git/CONTRIBUTORS.md                     # contributors to files in the directory
dir@/diff/HEAD.diff                          # uncommitted changes in the directory
```

### @/git/stash/

Browse and apply stash entries like branches/tags:

```sh
ls @/git/stash/
cat @/git/stash/0-WIP-on-main/src/lib.rs@/symbols/process.rs
mv @/git/stash/0-WIP-on-main @/git/stash/apply
```

### @/git/conflicts/ — merge conflict resolution

Per-file conflict workspace:

```
file.rs@/git/conflicts/ours.ext
file.rs@/git/conflicts/theirs.ext
file.rs@/git/conflicts/base.ext
file.rs@/git/conflicts/resolution.ext        # write here to resolve
```

With recursive VFS, each side is decomposed — compare individual symbols across ours/theirs/base.

### Cross-file rename propagation review

After LSP rename, surface old-name occurrences in non-code files:

```
@/rename_review/old_name/
# → README.md@/lines:42
# → config.toml@/lines:15
```

## Per-Provider Configuration — exploring

Providers expose a config object which is picked up by the config handler. Config handler parses the config file and exposes the full configuration; providers can then extract their relevant config subset. Enables per-provider tuning without hardcoded defaults.

## Miscellaneous — exploring

### @/scratch/

Ephemeral writable workspace for agents. Session-scoped. For stashing intermediate content during multi-step operations.

### OVERVIEW.md depth control

`setxattr("file.ext@/OVERVIEW.md", "user.depth", "2")` to control nesting levels. Default depth=1 for large files.

### Glob-friendly symbol names

Ensure symbol directory names are shell-glob-friendly. Document escaping rules for symbols with special characters (operators, generics, lifetimes). Consider canonical ASCII-safe encoding.

### @/refactor/ — high-level refactoring plans — speculative

Write a structured spec or natural-language description, nyne generates corresponding staged edits:

```sh
echo "extract method 'validate' from 'process', lines 10-25" >@/refactor/plan
ls @/refactor/staged/
# → file.rs@/edit/  (with pre-populated actions ready for review and apply)
```

Could integrate with LLM backends for natural-language → edit translation.

### Semantic diff in history

Historical file versions decomposed via recursive VFS enable symbol-level diffs across commits:

```sh
# What changed in this specific function between two commits?
diff file.rs@/history/2025-01-15-abc1234.rs@/symbols/process.rs \
    file.rs@/history/2025-02-20-def5678.rs@/symbols/process.rs
```

More focused than whole-file diffs. Requires recursive VFS to decompose historical versions into symbol trees.

Implementation notes: `syndiff` for syntax-aware diff computation. `flickzeug`, `imara-diff`, `mergiraf` for text diffing and patch application.

### VFS discovery tree — planned

By convention, nyne `@/` virtual directories are not listed in normal `readdir` results (they overlay transparently). To enable discovery and enumeration, a parallel directory tree at `<root>/@/vfs/` mirrors the project structure but lists **only** the virtual `@/` directories — no real files:

```sh
# Discover what virtual directories exist for a file
ls @/vfs/src/lib.rs@/
# → symbols/  git/  edit/  diff/  history/  ...

# Discover virtual directories across a subtree
find @/vfs/src/ -type d -name "@"

# List all files that have virtual companions
ls @/vfs/src/
# → lib.rs@/  main.rs@/  config.rs@/  (only files with @ namespaces)
```

This enables agents and tools to enumerate available VFS features without knowing the schema upfront.

## LSP Client Coverage Gaps — exploring

Current client implements 20 requests + 6 notifications against LSP 3.17. Audit performed 2026-03-21 against the [full 3.17 spec](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/). LSP 3.18 is in draft.

### Currently implemented

| Category     | Methods                                                                                                    |
| ------------ | ---------------------------------------------------------------------------------------------------------- |
| Lifecycle    | `initialize`, `initialized`, `shutdown`, `exit`                                                            |
| Sync         | `didOpen`, `didChange`, `didClose` (full sync only)                                                        |
| Navigation   | `definition`, `declaration`, `typeDefinition`, `implementation`, `references`                              |
| Hierarchies  | `prepareCallHierarchy`, `incomingCalls`, `outgoingCalls`, `prepareTypeHierarchy`, `supertypes`, `subtypes` |
| Intelligence | `hover`, `inlayHint`, `codeAction`, `codeAction/resolve`                                                   |
| Diagnostics  | Pull model (`textDocument/diagnostic`) + push (`publishDiagnostics`)                                       |
| Editing      | `rename`, `workspace/willRenameFiles`, `workspace/didRenameFiles`                                          |

### High value — directly useful for agent workflows

| Method                              | Type    | Relevance to nyne                                                                      |
| ----------------------------------- | ------- | -------------------------------------------------------------------------------------- |
| `textDocument/documentSymbol`       | Request | Structured symbol tree from LSP — cross-validate/complement syntax provider output     |
| `textDocument/completion`           | Request | Agent-driven code insertion with server-validated completions; feeds Contextual Scope  |
| `completionItem/resolve`            | Request | Lazy resolution of completion details                                                  |
| `textDocument/signatureHelp`        | Request | Function parameter info — improves `@/symbols/Fn@/signature` accuracy                  |
| `textDocument/prepareRename`        | Request | Validate rename is possible before executing — fail fast on `mv`                       |
| `textDocument/formatting`           | Request | Format document via LSP (server-side rustfmt, prettier, etc.)                          |
| `textDocument/rangeFormatting`      | Request | Format only a changed range after batch edits                                          |
| `textDocument/semanticTokens/full`  | Request | Rich semantic classification beyond tree-sitter — distinguishes types/lifetimes/macros |
| `textDocument/semanticTokens/range` | Request | Semantic tokens for a specific range (cheaper than full)                               |
| `workspace/symbol`                  | Request | Cross-file symbol search — "find struct Foo anywhere"; feeds Workspace Symbols         |
| `workspace/executeCommand`          | Request | Run server commands (e.g., cargo-specific actions from code lenses)                    |
| `workspace/applyEdit`               | Request | Accept server-initiated edits — required for some code actions to work fully           |
| `textDocument/codeLens`             | Request | Actionable annotations (run test, show impls count, etc.)                              |
| `codeLens/resolve`                  | Request | Lazy code lens detail resolution                                                       |
| `textDocument/documentHighlight`    | Request | Related symbol occurrences in same file — useful for scope analysis                    |
| `textDocument/foldingRange`         | Request | Code structure boundaries — could improve range-based VFS operations                   |
| `textDocument/selectionRange`       | Request | Semantic selection expansion — "expand to enclosing scope"                             |
| `textDocument/linkedEditingRange`   | Request | Linked editing regions (HTML tags, etc.)                                               |

### Medium value

| Method                      | Type         | Notes                                                                   |
| --------------------------- | ------------ | ----------------------------------------------------------------------- |
| `textDocument/moniker`      | Request      | Cross-project symbol identity — useful for monorepo/multi-crate linking |
| `textDocument/inlineValue`  | Request      | Debug-time inline value display                                         |
| `inlayHint/resolve`         | Request      | Lazy inlay hint resolution (currently fetching eagerly)                 |
| `textDocument/documentLink` | Request      | Clickable links in docs (URLs, file refs)                               |
| `documentLink/resolve`      | Request      | Lazy link resolution                                                    |
| `workspace/symbolResolve`   | Request      | Lazy workspace symbol resolution                                        |
| `workspace/willCreateFiles` | Request      | Pre-creation hooks for import updates                                   |
| `workspace/didCreateFiles`  | Notification | Post-creation notification                                              |
| `workspace/willDeleteFiles` | Request      | Pre-deletion hooks for cleanup                                          |
| `workspace/didDeleteFiles`  | Notification | Post-deletion notification                                              |

### Low value / not applicable

| Method                                                                | Reason to skip                       |
| --------------------------------------------------------------------- | ------------------------------------ |
| `textDocument/didSave`, `willSave`, `willSaveWaitUntil`               | nyne doesn't model save lifecycle    |
| `textDocument/onTypeFormatting`                                       | Interactive typing — no keyboard     |
| `textDocument/documentColor`, `colorPresentation`                     | Visual UI feature                    |
| `notebookDocument/*` (4 methods)                                      | Not a notebook tool                  |
| `window/*` (showMessage, logMessage, showDocument, progress)          | No UI layer                          |
| `telemetry/event`                                                     | No telemetry consumer                |
| `$/setTrace`, `$/logTrace`                                            | Debug-only                           |
| `client/registerCapability`, `unregisterCapability`                   | Dynamic registration not used        |
| `workspace/configuration`, `didChangeConfiguration`                   | Config not exposed to server         |
| `workspace/workspaceFolders`, `didChangeWorkspaceFolders`             | Single-root model                    |
| `workspace/didChangeWatchedFiles`                                     | File watching handled externally     |
| `workspace/*/refresh` (codeLens, inlineValue, inlayHint, diagnostics) | Server→client refresh signals        |
| `$/cancelRequest`, `$/progress`                                       | Protocol plumbing                    |
| `semanticTokens/full/delta`                                           | Incremental optimization — premature |

### Sync gap

Currently full sync only (`TextDocumentSyncKind::Full`). Incremental sync reduces payload size on large files — perf optimization, not a feature gap.

### LSP 3.18 preview

| Feature                         | Notes                                                                    |
| ------------------------------- | ------------------------------------------------------------------------ |
| `textDocument/inlineCompletion` | Ghost text / inline suggestions — interesting for agent-assisted editing |
| `SnippetTextEdit`               | Template-based edits with tab stops in workspace edits                   |
| Markup in diagnostics           | Rich formatting in diagnostic messages                                   |

### Implementation tiers

**Tier 1 — immediate wins** (complement existing VFS features, low effort):

- `textDocument/prepareRename` — trivial addition, prevents failed renames via `mv`
- `workspace/applyEdit` — required for some code actions to work; without it certain `codeAction/resolve` results are silently incomplete
- `textDocument/formatting` + `rangeFormatting` — post-edit cleanup; feeds Blocking Write Semantics pipeline
- `textDocument/documentSymbol` — cross-validate with tree-sitter symbol extraction

**Tier 2 — enriches agent capabilities** (unlocks new VFS features):

- `workspace/symbol` — enables Workspace Symbols section without grep fallback
- `textDocument/semanticTokens/full` + `/range` — richer symbol classification for OVERVIEW.md
- `textDocument/codeLens` + `codeLens/resolve` — exposes "run test", impl counts; feeds Test Extraction
- `textDocument/completion` + `completionItem/resolve` — feeds Completions and Contextual Scope sections

**Tier 3 — nice to have** (incremental improvements):

- `textDocument/signatureHelp` — parameter-level info for `signature.ext`
- `textDocument/foldingRange` + `selectionRange` — structural queries for range-based operations
- `textDocument/documentHighlight` — scope analysis within a file
- File operation notifications (`willCreate`, `didCreate`, `willDelete`, `didDelete`) — lifecycle awareness

## Implementation Notes

Crate selections for key subsystems:

| Subsystem         | Crate(s)                  | Notes                                                                   |
| ----------------- | ------------------------- | ----------------------------------------------------------------------- |
| Text splicing     | `crop`                    | Rope-based text manipulation                                            |
| Text diffing      | `flickzeug`, `imara-diff` | Core diff algorithms                                                    |
| Syntax-aware diff | `syndiff`                 | Tree-sitter-powered structural diffs                                    |
| Patch/merge       | `mergiraf`                | Three-way merge, patch application                                      |
| URL fetching      | `reqwest`                 | Direct HTTP; `trafilatura` (Python, shelled out) for content extraction |
| HTML parsing      | `scraper`                 | Quick DOM traversal when trafilatura is overkill                        |
