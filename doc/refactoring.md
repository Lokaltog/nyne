# Refactoring and Migration

## Migration Policy

- **Clean breaks only.** When changing interfaces, rename/move/delete in one pass. Never leave the old version around.
- **No backwards compatibility.** No deprecated wrappers, shims, or fallback paths for old call sites.
- **No compat re-exports.** Never re-export a moved symbol from its old location. Update every import.
- **No partial migrations.** Every migration must be completed across the entire codebase in a single change. No "we'll fix the rest later."
- **No `_old`/`_legacy`/`_compat` suffixes.** If something is replaced, the old version is deleted.
- **No unused parameters.** If a function parameter is never read in the body, remove it and update all call sites. Do not leave parameters with `# unused` comments or `_ =` assignments.

### What Counts as Speculative?

- **Known requirement with one implementation today** → not speculative. When a documented requirement or architectural plan calls for extensibility (e.g., multi-language support), design the abstraction upfront.
- **Staged multi-wave work** → not speculative. Code introduced in wave N for wiring in wave N+1 is staged. Do not delete symbols that are explicitly part of an in-progress plan.
- **"Might need this someday"** → speculative. Delete it. If you remove the last consumer of a symbol, delete the symbol.

### How to Execute a Clean Migration

1. Introduce the new interface (new name, new signature, new location).
2. Update every call site in one pass.
3. Delete the old interface entirely.
4. Verify: no import of the old name remains anywhere in the codebase.

Never do step 1 and "plan to do step 2 later." All four steps happen in one logical change.

## DRY and Reuse

SSOT rules are in `CLAUDE.md` (root) — that's the authoritative source for extraction thresholds and duplication policy. The guidance below covers refactoring-specific nuance only.

- **Check before building.** Before writing any new utility, parameter type, or helper — search the codebase for existing abstractions that do the same thing. Extend what exists rather than creating a parallel version.
- **Build abstractions first, wire consumers second.** When a change touches multiple commands, implement the shared component first with tests, then integrate it into each consumer. Never implement the same pattern inline across multiple commands and "extract later."
- **Name and test abstractions independently.** A shared component must have a clear name, a documented contract, and its own tests — not just tests through its consumers.
