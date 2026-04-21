# nyne-plugin-source

Source plugin — syntax decomposition, batch editing, developer-experience features.

## AnyMap

- **Inserts:** `Arc<SourcePaths>`, `Arc<SyntaxRegistry>`, `DecompositionCache`, `EditStaging`, `SourceExtensions` (default)
- **Reads:** `SourceExtensions` in `providers()`, `CompanionExtensions` in `activate()`

## Fragment resolution helpers

Downstream plugins (lsp, analysis, git) that register `SourceExtensions::fragment_path` callbacks should use the shared helpers on `DecompositionCache` rather than re-implementing the preamble:

- `decomposition.resolve_from_ctx(ctx, req) -> Option<ResolvedFragment>` — extracts `source_file` + `path` param, validates fragment exists. Returns `None` on any missing component.
- `decomposition.resolver(source_file) -> FragmentResolver` — convenience over `FragmentResolver::new(cache.clone(), path)`.

`ResolvedFragment` bundles `source_file` + owned `segments` + `Arc<DecomposedSource>` + guaranteed-present `&Fragment` accessor, and exposes `segments_arc()` for closure captures.
