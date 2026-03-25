//! Test helpers for nyne-coding.

use std::path::PathBuf;
use std::sync::Arc;

use crate::syntax::SyntaxRegistry;

/// Build a `SyntaxRegistry` with all compiled-in languages.
pub fn registry() -> SyntaxRegistry { SyntaxRegistry::build() }

/// Build a `VfsPath` helper from a decomposed file.
pub fn vfs(path: &str) -> nyne::types::vfs_path::VfsPath {
    nyne::types::vfs_path::VfsPath::new(path).expect("invalid test path")
}

/// Load a test fixture file relative to `nyne-coding/src/`.
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
/// Inserts a [`CodingServices`] bundle with default config, a real syntax
/// registry, a stub LSP manager (disabled), and a default analysis engine.
/// This mirrors production activation so tests exercise the same code paths.
pub fn stub_activation_context() -> Arc<nyne::dispatch::activation::ActivationContext> {
    use nyne::dispatch::activation::ActivationContext;
    use nyne::process::Spawner;
    use nyne::types::OsFs;

    use crate::config::CodingConfig;
    use crate::lsp::LspRegistry;
    use crate::lsp::manager::LspManager;
    use crate::lsp::path::LspPathResolver;
    use crate::services::CodingServices;
    use crate::syntax::analysis::AnalysisEngine;
    use crate::syntax::decomposed::DecompositionCache;

    let tmp = std::env::temp_dir().join("nyne-coding-test");
    let config = nyne::config::NyneConfig::default();
    let real_fs: Arc<dyn nyne::types::RealFs> = Arc::new(OsFs::new(tmp.clone()));
    let spawner = Arc::new(Spawner::new());
    let mut ctx = ActivationContext::new(
        tmp.clone(),
        tmp.clone(),
        tmp.clone(),
        real_fs.clone(),
        &config,
        spawner.clone(),
    );

    let coding_config = CodingConfig::default();
    let syntax = Arc::new(registry());
    let lsp = Arc::new(LspManager::new(
        LspRegistry::build_with_config(&coding_config.lsp),
        Arc::clone(&syntax),
        coding_config.lsp.clone(),
        spawner,
        Default::default(),
        LspPathResolver::new(tmp.clone(), tmp),
    ));
    ctx.insert(CodingServices {
        decomposition: DecompositionCache::new(real_fs, Arc::clone(&syntax)),
        analysis: Arc::new(AnalysisEngine::build_filtered(&coding_config.analysis)),
        syntax,
        lsp,
        config: coding_config,
    });
    Arc::new(ctx)
}
