use nyne::router::RouteExtension;

/// Named extension points for the companion namespace.
///
/// The companion plugin inserts this into [`ActivationContext`] during
/// `activate()`. Downstream plugins call
/// `ctx.get_or_insert_default::<CompanionExtensions>()` in their
/// `activate()` to register callbacks. The companion plugin reads it back
/// in `providers()` and applies the extensions to its route trees.
///
/// Three scopes exist:
/// - **`file`** — per-file companion content (`file.rs@/`): symbols,
///   diagnostics, git blame/log, etc.
/// - **`dir`** — per-directory companion content (`dir@/`): directory-level
///   features that don't require source decomposition.
/// - **`mount`** — mount-wide companion content (`./@/`): todo, branches,
///   workspace search, batch edit staging, etc.
///
/// [`ActivationContext`]: nyne::dispatch::activation::ActivationContext
#[derive(Default)]
pub struct CompanionExtensions {
    /// Per-file companion content (`file.rs@/`).
    ///
    /// Downstream plugins contribute content and directories that appear
    /// in a file's companion directory (e.g. source's `symbols/`,
    /// `OVERVIEW.md`, git's `git/`).
    pub file: RouteExtension,

    /// Per-directory companion content (`dir@/`).
    ///
    /// Downstream plugins contribute content and directories that appear
    /// in a directory's companion namespace. Only activated when the
    /// companion target is a directory, not a file.
    pub dir: RouteExtension,

    /// Mount-wide companion content (`./@/`).
    ///
    /// Downstream plugins contribute content and directories that appear
    /// in the root companion namespace (e.g. todo's `todo/`, git's
    /// `git/` branches/tags, lsp's `search/`).
    pub mount: RouteExtension,
}
