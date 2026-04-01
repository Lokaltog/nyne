# nyne-plugin-source

Source plugin — syntax decomposition, batch editing, developer-experience features.

## AnyMap

- **Inserts:** `Arc<SourcePaths>`, `Arc<SyntaxRegistry>`, `DecompositionCache`, `EditStaging`, `SourceExtensions` (default)
- **Reads:** `SourceExtensions` in `providers()`, `CompanionExtensions` in `activate()`
