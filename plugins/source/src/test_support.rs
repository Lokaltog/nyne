//! Test helpers for nyne-coding.

use std::path::PathBuf;
use std::sync::Arc;

use crate::syntax::SyntaxRegistry;

/// Build a `SyntaxRegistry` with all compiled-in languages.
///
/// Shorthand for [`SyntaxRegistry::build()`] so tests don't need to import
/// the registry type directly.
pub fn registry() -> SyntaxRegistry { SyntaxRegistry::build() }

/// Build a [`VfsPath`] from a string, panicking on invalid paths.
///
/// Convenience wrapper that avoids `unwrap()` noise in test assertions
/// while still failing loudly on bad input.
pub fn vfs(path: &str) -> nyne::types::vfs_path::VfsPath {
    nyne::types::vfs_path::VfsPath::new(path).expect("invalid test path")
}

/// Load a test fixture file relative to `nyne-coding/src/`.
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

/// Create a stub `ActivationContext` for testing.
///
/// Inserts a [`Services`] bundle with default config and a real syntax
/// registry. This mirrors production activation so tests exercise the same
/// code paths.
pub fn stub_activation_context() -> Arc<nyne::dispatch::activation::ActivationContext> {
    use nyne::dispatch::activation::ActivationContext;
    use nyne::process::Spawner;
    use nyne::types::OsFs;

    use crate::config::Config;
    use crate::services::Services;
    use crate::syntax::decomposed::DecompositionCache;

    let tmp = std::env::temp_dir().join("nyne-source-test");
    let config = Arc::new(nyne::config::NyneConfig::default());
    let real_fs: Arc<dyn nyne::types::RealFs> = Arc::new(OsFs::new(tmp.clone()));
    let spawner = Arc::new(Spawner::new());
    let mut ctx = ActivationContext::new(tmp.clone(), tmp.clone(), tmp.clone(), real_fs.clone(), config, spawner);

    let source_config = Config::default();
    let syntax = Arc::new(registry());
    ctx.insert(Services {
        decomposition: DecompositionCache::new(real_fs, Arc::clone(&syntax)),
        syntax,
        config: source_config,
    });
    Arc::new(ctx)
}
