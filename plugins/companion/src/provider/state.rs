use std::path::PathBuf;
use std::sync::Arc;

use nyne::router::Request;

/// Set by `CompanionProvider` when it strips the companion suffix.
///
/// `source_file` is `None` for the mount-wide namespace root (bare `@`),
/// `Some(path)` for file/directory companions (`foo.rs@`).
///
/// Provides [`companion_name`](Self::companion_name) for building
/// companion-suffixed names without knowing the suffix directly.
#[derive(Debug, Clone)]
pub struct Companion {
    pub source_file: Option<PathBuf>,
    suffix: Arc<str>,
}

impl Companion {
    /// Create a new companion state with the given suffix.
    pub fn new(source_file: Option<PathBuf>, suffix: Arc<str>) -> Self {
        tracing::trace!("companion:state:Companion({source_file:?})");
        Self { source_file, suffix }
    }

    /// Build a companion directory name from a bare name.
    ///
    /// Appends the runtime-configured companion suffix:
    /// e.g., `"foo.rs"` → `"foo.rs@"` (with default suffix).
    pub fn companion_name(&self, name: &str) -> String { format!("{name}{}", self.suffix) }

    /// Strip the companion suffix from a name, returning the bare name.
    ///
    /// Returns `None` if the name doesn't end with the suffix or would be
    /// empty after stripping.
    pub fn strip_suffix<'a>(&self, name: &'a str) -> Option<&'a str> {
        let stripped = name.strip_suffix(&*self.suffix)?;
        if stripped.is_empty() { None } else { Some(stripped) }
    }
}

/// Extension trait for extracting companion state from a [`Request`].
pub trait CompanionRequest {
    /// The companion state, if the request is inside a companion namespace.
    fn companion(&self) -> Option<&Companion>;

    /// The source file path from companion state, if present.
    fn source_file(&self) -> Option<PathBuf>;

    /// The companion state and source file path, if both present.
    fn companion_context(&self) -> Option<(Companion, PathBuf)>;
}

impl CompanionRequest for Request {
    fn companion(&self) -> Option<&Companion> { self.state::<Companion>() }

    fn source_file(&self) -> Option<PathBuf> { self.companion()?.source_file.clone() }

    fn companion_context(&self) -> Option<(Companion, PathBuf)> {
        let c = self.companion()?.clone();
        let sf = c.source_file.clone()?;
        Some((c, sf))
    }
}
