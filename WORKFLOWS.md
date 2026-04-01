# nyne Workflows

Filesystem-native workflows using standard coreutils and bash. Every virtual file in the nyne VFS is a real file — `cat`, `grep`, `sed`, `awk`, `tee`, `diff`, `find`, `xargs`, pipes, and redirects all compose naturally.

**Path convention:** `Foo@/` enters a symbol's virtual namespace (metadata, body, LSP, git). `Foo.rs` is shorthand for `Foo@/body.rs`.

## Reading and Exploring

```sh
# Understand a file's structure (always start here)
cat file.rs@/OVERVIEW.md

# Read a specific symbol (shorthand)
cat file.rs@/symbols/process.rs

# Read a specific symbol (explicit form)
cat file.rs@/symbols/process@/body.rs

# Read multiple symbols at once
cat file.rs@/symbols/{process,validate,Config}@/body.rs

# Read just the signature (declaration line)
cat file.rs@/symbols/process@/signature.rs

# Read the docstring
cat file.rs@/symbols/process@/docstring.txt

# Read decorators/attributes
cat file.rs@/symbols/process@/decorators.rs

# Read the import block
cat file.rs@/symbols/imports.rs

# Read specific lines from a file
cat file.rs@/lines:10-20

# Filter symbols by kind
ls file.rs@/symbols/by-kind/fn/

# Explore the call graph by following symlinks
ls -la file.rs@/symbols/process@/callers/
cat file.rs@/symbols/process@/callers/some_caller@/body.rs

# Search across symbol bodies
grep -r "TODO" file.rs@/symbols/*@/body.rs

# Quick overview of all files in a directory
cat src/@/OVERVIEW.md

# Slice git history (last 10 commits)
cat file.rs@/git/LOG.md:-10

# Blame for a specific line range
cat file.rs@/git/BLAME.md:42-55

# Check compiler diagnostics
cat file.rs@/DIAGNOSTICS.md

# Check a symbol's generated documentation
cat file.rs@/symbols/process@/DOC.md
```

## Workspace Symbol Search

```sh
# Find a symbol by name across the entire project
ls @/search/symbols/EditOutcome
# → edit.rs::EditOutcome -> ../../../../nyne/src/edit.rs@/symbols/at-line/14
# → mod.rs::EditOutcome  -> ../../../../plugins/source/src/edit/plan/mod.rs@/symbols/at-line/261

# Results are symlinks — follow them to read, edit, or inspect
cat @/search/symbols/EditOutcome/edit.rs::EditOutcome

# Partial matches work (LSP workspace/symbol query under the hood)
ls @/search/symbols/Provider

# Pipe into other tools
ls @/search/symbols/Cache | xargs -I{} readlink @/search/symbols/Cache/{}
```

Results are directories of symlinks pointing to `<file>@/symbols/at-line/<line>`, which resolve to the matching symbol's body. LSP servers are spawned eagerly at mount time, so searches work immediately.

## Editing

```sh
# Rewrite a function
echo 'fn process() { /* new impl */ }' >file.rs@/symbols/process@/body.rs

# Change a function's signature without touching the body
echo 'fn process(input: &str, timeout: Duration) -> Result<()>' \
    >file.rs@/symbols/process@/signature.rs

# Append content after an existing symbol
cat new_function.rs >>file.rs@/symbols/process@/body.rs

# Edit the import block
echo 'use std::collections::HashMap;' >>file.rs@/symbols/imports.rs

# Splice multiple symbols from one file into another
cat file.rs@/symbols/{Foo,Bar}@/body.rs >>other.rs@/symbols/Baz@/body.rs

# Edit a specific line range
echo 'let x = 42;' >file.rs@/lines:10

# Replace a range of lines
echo 'new content' >file.rs@/lines:10-15

# Clear a docstring (truncate preserves symbol)
>file.rs@/symbols/process@/docstring.txt

# Add a decorator/attribute
echo '#[instrument]' >>file.rs@/symbols/process@/decorators.rs

# Delete a symbol from source
rmdir file.rs@/symbols/old_helper@/
```

## Batch Edit Staging

Symbols have an `edit/` namespace for staging multi-step edits before applying them.

```sh
# Stage an insertion after a symbol
echo 'fn post_process() { ... }' >file.rs@/symbols/process@/edit/insert-after

# Stage an insertion before a symbol
echo '/// Important documentation' >file.rs@/symbols/process@/edit/insert-before

# Stage a replacement for a symbol's body
echo 'fn process() { /* rewritten */ }' >file.rs@/symbols/process@/edit/replace

# Stage appending content to a symbol
echo '    // extra logic' >file.rs@/symbols/process@/edit/append

# Stage a symbol deletion
echo 1 >file.rs@/symbols/process@/edit/delete

# Preview staged changes for a symbol as a unified diff
cat file.rs@/symbols/process@/edit/staged.diff

# Preview all staged edits across the project
cat @/edit/staged.diff
```

## Refactoring

```sh
# Rename a symbol (LSP-powered, project-wide)
mv file.rs@/symbols/old_name@/ file.rs@/symbols/new_name@/

# Preview a rename before applying
cat file.rs@/symbols/old_name@/rename/new_name.diff

# Preview a file rename (shows import updates across the project)
cat file.rs@/rename/new_name.rs.diff

# Apply a code action from the actions directory
patch -p1 < file.rs@/symbols/process@/actions/10-add-error-handling.diff
```

## Git

```sh
# File-level blame
cat file.rs@/git/BLAME.md

# Blame for a specific line range
cat file.rs@/git/BLAME.md:42-55

# Commit log for a file
cat file.rs@/git/LOG.md

# Last 10 commits
cat file.rs@/git/LOG.md:-10

# Contributors ranked by commit count
cat file.rs@/git/CONTRIBUTORS.md

# Symbol-level blame
cat file.rs@/symbols/process@/git/BLAME.md

# Blame for specific lines within a symbol
cat file.rs@/symbols/process@/git/BLAME.md:5-10

# Symbol-level commit log
cat file.rs@/symbols/process@/git/LOG.md

# Uncommitted changes for a file
cat file.rs@/diff/HEAD.diff

# Diff against any branch or ref
cat file.rs@/diff/feature-branch.diff

# Working tree status
cat @/git/STATUS.md

# Browse files at a specific branch
ls @/git/branches/feature-branch/src/

# Compare a symbol across branches
diff <(cat @/git/branches/main/src/lib.rs@/symbols/process@/body.rs) \
    <(cat @/git/branches/feature/src/lib.rs@/symbols/process@/body.rs)

# Browse historical versions of a file
ls file.rs@/history/

# Compare current symbol with a historical version
diff file.rs@/symbols/process@/body.rs \
    file.rs@/history/001_2025-01-15_abc1234_refactor.rs@/symbols/process@/body.rs

# Rename a branch
mv @/git/branches/old-name @/git/branches/new-name
```

## Pipes and Composition

```sh
# Pipe LLM output directly into a symbol body
llm "write a validation function for email addresses in Rust" |
    tee file.rs@/symbols/validate@/body.rs

# Edit a symbol and log what was written
cat new_impl.rs | tee file.rs@/symbols/process@/body.rs >>/tmp/edit-log.txt

# Stage an edit and immediately preview it
echo "fn new_func() {}" >file.rs@/symbols/process@/edit/insert-after &&
    cat file.rs@/symbols/process@/edit/staged.diff
```

## sed — Targeted In-Body Edits

```sh
# Rename a local variable within a single function (local scope, not LSP)
sed -i 's/old_var/new_var/g' file.rs@/symbols/process@/body.rs

# Add error handling to every return point in a function
sed -i 's/return \(.*\);/return \1.map_err(|e| anyhow!("failed: {e}"))?;/g' \
    file.rs@/symbols/process@/body.rs

# Strip all comments from a symbol body
sed -i '/^\s*\/\//d' file.rs@/symbols/process@/body.rs

# Add a decorator/attribute via sed
sed -i '1i #[instrument]' file.rs@/symbols/process@/decorators.rs
```

## awk — Symbol Analytics

```sh
# Symbol size report from OVERVIEW.md (largest first)
awk '/tokens/ {print $1, $NF}' file.rs@/OVERVIEW.md | sort -k2 -rn | head -10

# Aggregate token counts across a directory
cat src/**/*.rs@/OVERVIEW.md | awk '/^##/ {file=$2} /tokens/ {sum+=$NF} END {print sum " total tokens"}'

# Find symbols with high line counts
awk '/lines/ && $NF > 50 {print FILENAME, $0}' src/**/*.rs@/OVERVIEW.md
```

## grep — Cross-Project Search

```sh
# Find all uses of a pattern across symbol bodies
grep -rl "unwrap()" src/**/*.rs@/symbols/*@/body.rs

# Find TODOs with context (which symbol they're in)
grep -rn "TODO" src/**/*.rs@/symbols/*@/body.rs

# Find all symbols that call a specific function
grep -rl "database::connect" src/**/*.rs@/symbols/*@/body.rs

# Search diagnostics across the project
grep -r "error" src/**/*.rs@/DIAGNOSTICS.md
```

## find — Structured Traversal

```sh
# Find all symbols with diagnostics
find . -name "DIAGNOSTICS.md" -path "*@/symbols/*" -exec grep -l "error" {} \;

# Find all functions longer than 100 lines
find . -name "body.rs" -path "*@/*" -exec sh -c \
    'lines=$(wc -l < "$1"); [ "$lines" -gt 100 ] && echo "$1: $lines lines"' _ {} \;

# Find all files with uncommitted changes
find . -name "HEAD.diff" -path "*@/diff/*" -not -empty -exec echo {} \;
```

## xargs — Bulk Operations

```sh
# Stage replacements for all functions matching a pattern
grep -rl "old_api_call" src/**/*.rs@/symbols/*@/body.rs |
    xargs -I{} sh -c \
        'sym=$(echo "{}" | grep -oP "symbols/\K[^@]+"); file=$(echo "{}" | sed "s/@.*//"); \
   sed "s/old_api_call/new_api_call/g" "{}" > "${file}@/symbols/${sym}@/edit/replace"'
```

## diff — Comparison Workflows

```sh
# Compare two symbols side by side
diff <(cat file.rs@/symbols/process_v1@/body.rs) \
    <(cat file.rs@/symbols/process_v2@/body.rs)

# Compare a symbol's callers vs dependencies
diff <(ls file.rs@/symbols/process@/callers/) \
    <(ls file.rs@/symbols/process@/deps/)

# Compare symbols across two branches
diff <(cat @/git/branches/main/src/lib.rs@/symbols/process@/body.rs) \
    <(cat @/git/branches/feature/src/lib.rs@/symbols/process@/body.rs)

# Compare a symbol with a historical version
diff file.rs@/symbols/process@/body.rs \
    file.rs@/history/001_2025-01-15_abc1234_refactor.rs@/symbols/process@/body.rs
```

## tee — Simultaneous Write + Capture

```sh
# Edit a symbol and keep a backup
cat new_impl.rs | tee file.rs@/symbols/process@/body.rs >/tmp/backup-process.rs

# Write to multiple symbol bodies at once (same content)
echo "todo!()" | tee \
    file.rs@/symbols/handler_a@/body.rs \
    file.rs@/symbols/handler_b@/body.rs \
    >/dev/null
```

## Heredocs — Multi-Line Writes

```sh
# Write a complete new function
cat <<'EOF' >file.rs@/symbols/validate@/body.rs
fn validate(input: &str) -> Result<()> {
    if input.is_empty() {
        bail!("input cannot be empty");
    }
    Ok(())
}
EOF

# Stage an insertion with a heredoc
cat <<'EOF' >file.rs@/symbols/process@/edit/insert-after
fn post_process(data: &[u8]) -> Result<()> {
    log::info!("post-processing {} bytes", data.len());
    Ok(())
}
EOF
```

## Multi-File Batch Workflows

```sh
# Find-and-replace across a project using VFS edit staging
for file in $(grep -rl "deprecated_fn" src/**/*.rs@/symbols/*@/body.rs); do
    src_file=$(echo "$file" | sed 's/@.*//')
    symbol=$(echo "$file" | grep -oP 'symbols/\K[^@]+')
    sed 's/deprecated_fn/replacement_fn/g' "$file" \
        >"${src_file}@/symbols/${symbol}@/edit/replace"
done
# Preview all changes
cat @/edit/staged.diff

# Export all symbols from a file for offline analysis
mkdir -p /tmp/snapshot
for sym in $(ls file.rs@/symbols/); do
    cp "file.rs@/symbols/${sym}@/body.rs" "/tmp/snapshot/$sym.rs" 2>/dev/null
done

# Cross-project symbol copy (two nyne mounts)
cat ~/project-a/src/utils.rs@/symbols/helper@/body.rs \
    >>~/project-b/src/lib.rs@/symbols/Utils~Impl@/body.rs
```
