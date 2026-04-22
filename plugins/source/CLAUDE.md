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

## Language test macro

Per-language fragment-decomposition tests share a canonical shape:
fixture load + `fragment_count` + `fragment_names` + `fragment_kinds`.
The `crate::language_tests!` macro (in `src/test_support.rs`) generates
all three, plus the `load_basic()` fixture loader and the `basic`
rstest fixture:

```rust
crate::language_tests! {
    ext: "rs",
    fixture_module: "syntax/languages/rust",
    fixture_file: "basic.rs",
    fragment_count: 9,
    fragment_names: ["imports", "MAX_SIZE", ...],
    fragment_kinds: [FragmentKind::Imports, FragmentKind::Symbol(SymbolKind::Const), ...],
}

// Language-specific tests (class children, import-coalescing, etc.)
// stay as `#[rstest]` functions alongside the macro invocation.
```

Applies to rust/python/typescript/fennel/nix/toml/markdown. jinja2 has
a bespoke structure (nested blocks, macros) and is tested directly
without the macro.
