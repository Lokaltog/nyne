//! Test helpers for nyne-source.

use std::path::PathBuf;
use std::sync::Arc;

use crate::syntax::SyntaxRegistry;

/// Build a `SyntaxRegistry` with all compiled-in languages.
///
/// Shorthand for [`SyntaxRegistry::build()`] so tests don't need to import
/// the registry type directly.
pub fn registry() -> SyntaxRegistry { SyntaxRegistry::build() }

/// Load a test fixture file relative to `nyne-source/src/`.
///
/// Resolves `src/{module}/fixtures/{name}` using `CARGO_MANIFEST_DIR` so
/// fixtures work regardless of the working directory. Panics with a
/// descriptive message if the file is missing.
pub fn load_fixture(module: &str, name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(module)
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to load fixture {}: {e}", path.display()))
}

/// Create a source-plugin-flavored stub `ActivationContext` for testing.
///
/// Unlike [`nyne::test_support::stub_activation_context`] (minimal, uses `StubFs`),
/// this version uses `OsFilesystem` and inserts `Arc<SyntaxRegistry>` and
/// `DecompositionCache`, mirroring production activation so tests exercise the
/// same code paths.
pub fn stub_activation_context() -> Arc<nyne::ActivationContext> {
    use nyne::ActivationContext;
    use nyne::process::Spawner;

    use crate::syntax::decomposed::DecompositionCache;

    let tmp = std::env::temp_dir().join("nyne-source-test");
    let config = Arc::new(nyne::config::NyneConfig::default());
    let fs: Arc<dyn nyne::router::Filesystem> = Arc::new(nyne::router::fs::os::OsFilesystem::new(&tmp));
    let mut ctx = ActivationContext::new(
        tmp.clone(),
        tmp.clone(),
        tmp,
        Arc::clone(&fs),
        config,
        Arc::new(Spawner::new()),
    );

    let syntax = Arc::new(registry());
    ctx.insert(DecompositionCache::new(Arc::clone(&fs), Arc::clone(&syntax), 5));
    ctx.insert(syntax);
    Arc::new(ctx)
}
