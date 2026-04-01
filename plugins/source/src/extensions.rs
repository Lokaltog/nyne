use nyne::router::RouteExtension;

/// Named extension points for the source plugin's route tree.
///
/// The source plugin inserts this into [`ActivationContext`] during
/// `activate()`. Downstream plugins call
/// `ctx.get_or_insert_default::<SourceExtensions>()` in their
/// `activate()` to register callbacks. The source plugin reads it back
/// in `providers()` and applies the extensions when building its tree.
///
/// [`ActivationContext`]: nyne::dispatch::activation::ActivationContext
#[derive(Default)]
pub struct SourceExtensions {
    /// Extensions inside fragment directories (`symbols/{..path}`).
    ///
    /// Downstream plugins contribute content, readdir/lookup callbacks,
    /// and subdirectories that appear alongside source's body, signature,
    /// docstring, and other meta-files.
    ///
    /// **LSP:** `CALLERS.md`, `DEPS.md`, `REFERENCES.md`, `actions/`,
    /// `callers/`, `deps/`, `references/`, `rename/` subdirectories.
    /// **Analysis:** `ANALYSIS.md`.
    pub fragment_path: RouteExtension,
}
