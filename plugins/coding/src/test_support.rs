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
pub fn stub_activation_context() -> Arc<nyne::dispatch::activation::ActivationContext> {
    use nyne::dispatch::activation::ActivationContext;
    use nyne::process::Spawner;
    use nyne::types::OsFs;

    let tmp = std::env::temp_dir().join("nyne-coding-test");
    let config = nyne::config::NyneConfig::default();
    let real_fs: Arc<dyn nyne::types::RealFs> = Arc::new(OsFs::new(tmp.clone()));
    let spawner = Arc::new(Spawner::new());
    let mut ctx = ActivationContext::new(tmp.clone(), tmp.clone(), tmp, real_fs, &config, spawner);
    // Insert a SyntaxRegistry so providers can use it.
    ctx.insert(Arc::new(registry()));
    Arc::new(ctx)
}
