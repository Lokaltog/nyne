//! Git helpers for integration tests.

use crate::harness::NyneMount;

/// RAII guard that restores all tracked files in the mount to HEAD on drop.
///
/// Use in mutating tests to guarantee cleanup even on panic:
///
/// ```no_run
/// # use nyne_integration_tests::NyneMount;
/// # let mount = NyneMount::start().unwrap();
/// let _guard = mount.cleanup_guard();
/// // ... mutations ...
/// // _guard restores on drop
/// ```
pub struct CleanupGuard<'a> {
    mount: &'a NyneMount,
}

impl<'a> CleanupGuard<'a> {
    pub(crate) const fn new(mount: &'a NyneMount) -> Self { Self { mount } }
}

impl Drop for CleanupGuard<'_> {
    fn drop(&mut self) {
        let out = self.mount.sh("git checkout HEAD -- .");
        if !out.is_ok() {
            tracing::warn!(
                exit = out.exit_code,
                stderr = %out.stderr,
                "cleanup_guard: `git checkout HEAD -- .` failed",
            );
        }
    }
}
