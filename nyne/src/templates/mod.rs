//! Shared template engine and view abstractions for virtual file content.
//!
//! Provides [`TemplateEngine`] (thin wrapper around [`minijinja::Environment`])
//! and the [`TemplateView`] / [`TemplateContent`] pattern for read-time
//! template rendering.
//!
//! **Provider workflow:**
//! 1. Create [`TemplateHandle`]s via [`HandleBuilder`] — each handle binds a
//!    template key to a shared engine.
//! 2. Call [`TemplateHandle::node`] at resolve time to produce `VirtualNode`s.

/// Pre-bound template handles for producing virtual file nodes.
mod handle;

use std::borrow::Cow;
use std::sync::Arc;

use color_eyre::eyre::Result;
use minijinja::Environment;
use serde::Serialize;

/// Template engine wrapping a `minijinja::Environment`.
///
/// Constructed via [`TemplateEngine::new`], which applies project-wide defaults
/// (`trim_blocks`, `lstrip_blocks`, shared filters). Providers register templates
/// at initialization time; rendering is infallible at runtime because templates
/// are compiled at registration.
pub struct TemplateEngine {
    env: Environment<'static>,
}

/// Default implementation for `TemplateEngine`.
impl Default for TemplateEngine {
    /// Returns the default value.
    fn default() -> Self { Self::new() }
}

/// Template registration, rendering, and global variable management.
impl TemplateEngine {
    /// Create a new engine with shared settings and filters.
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_trim_blocks(true);
        env.set_lstrip_blocks(true);
        env.add_filter("ljust", |v: String, w: usize| format!("{v:<w$}"));
        env.add_filter("rjust", |v: String, w: usize| format!("{v:>w$}"));
        env.add_filter("tokens", format_tokens);
        env.add_filter("first_line", first_line);
        env.add_filter("strip_prefix", strip_prefix);
        Self { env }
    }

    /// Register a named template from a source string.
    ///
    /// # Panics
    ///
    /// Panics if the template has a syntax error. Templates are compiled
    /// from `include_str!` constants at init time — a failure here is a
    /// bug in the `.j2` source file, not a runtime condition.
    #[allow(clippy::expect_used)]
    pub fn add_template(&mut self, name: &'static str, source: &'static str) {
        self.env
            .add_template(name, source)
            .expect("invalid template syntax — this is a bug in the .j2 file");
    }

    /// Render a named template with the given view model.
    ///
    /// # Panics
    ///
    /// Panics if `name` was not registered via [`add_template`](Self::add_template),
    /// or if the template references variables not present in `view`.
    /// Both conditions are programming errors — all templates and their
    /// view contracts are established at compile time.
    #[allow(clippy::expect_used)]
    pub fn render(&self, name: &str, view: &impl Serialize) -> String {
        let tmpl = self
            .env
            .get_template(name)
            .expect("template not registered — missing add_template call");
        tmpl.render(view)
            .expect("template render failed — view contract mismatch")
    }

    /// Add a global variable available to all templates.
    ///
    /// Used by `register_template_globals` to
    /// inject VFS name constants so templates can reference them as
    /// `{{ FILE_OVERVIEW }}`, `{{ FILE_CALLERS }}`, etc.
    pub fn add_global(&mut self, name: impl Into<Cow<'static, str>>, value: impl Into<String>) {
        self.env.add_global(name, minijinja::Value::from(value.into()));
    }

    /// Render a named template to bytes.
    ///
    /// Convenience wrapper around [`Self::render`] for the common case where
    /// callers need `Vec<u8>` (i.e., every [`TemplateView`] impl).
    pub fn render_bytes(&self, name: &str, view: &impl Serialize) -> Vec<u8> { self.render(name, view).into_bytes() }
}

/// Produce rendered bytes at read time.
///
/// Implement this trait for views that need to compute, fetch, or transform
/// data during rendering. For simple `Serialize` structs, use [`serialize_view`]
/// instead.
pub trait TemplateView: Send + Sync {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>>;
}

/// Adapts any [`Serialize`] value into a [`TemplateView`].
struct SerializeView<T>(T);

/// Renders by serializing the inner value directly into the template engine.
impl<T: Serialize + Send + Sync> TemplateView for SerializeView<T> {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        Ok(engine.render_bytes(template, &self.0))
    }
}

/// Blanket impl: `Arc<dyn TemplateView>` delegates to the inner view.
impl<T: TemplateView + ?Sized> TemplateView for Arc<T> {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> { (**self).render(engine, template) }
}

/// Adapt any [`Serialize`] value into a [`TemplateView`].
///
/// Use this for static views — structs that are pure data bags with no
/// computation at render time.
pub fn serialize_view(val: impl Serialize + Send + Sync + 'static) -> impl TemplateView { SerializeView(val) }

/// Single [`Readable`](crate::node::Readable) for all template-backed
/// virtual files.
///
/// Owns the template engine, template name, and view. On read, delegates to
/// [`TemplateView::render`], which receives the engine and template name.
pub struct TemplateContent {
    engine: Arc<TemplateEngine>,
    template: &'static str,
    view: Box<dyn TemplateView>,
}

/// Construction for template-backed readable nodes.
impl TemplateContent {
    /// Create a new template-backed readable.
    ///
    /// The `view` controls what happens at render time — from simple
    /// serialization to complex data fetching and transformation.
    pub fn new(engine: &Arc<TemplateEngine>, template: &'static str, view: impl TemplateView + 'static) -> Self {
        Self {
            engine: Arc::clone(engine),
            template,
            view: Box::new(view),
        }
    }
}

/// Delegates reads to the inner [`TemplateView`] for on-demand rendering.
impl Readable for TemplateContent {
    /// Renders the template with the stored view and returns the result as bytes.
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> { self.view.render(&self.engine, self.template) }
}

/// Estimate tokens from a byte count and format in compact form.
///
/// Converts bytes → tokens (`bytes / 4`) then formats (e.g. `~2.1k` for
/// 8400 bytes, `~850` for 3400 bytes). Registered as the `tokens`
/// minijinja filter — all callers pass raw byte counts.
fn format_tokens(bytes: usize) -> String {
    let n = bytes / 4;
    if n >= 1000 {
        let whole = n / 1000;
        let frac = (n % 1000) / 100;
        format!("~{whole}.{frac}k")
    } else {
        format!("~{n}")
    }
}

/// Extract the first non-empty trimmed line from a string.
///
/// Registered as a minijinja filter (`first_line`).
fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_owned()
}

/// Strip a prefix from a string, returning the original if no match.
///
/// Registered as a minijinja filter (`strip_prefix`).
fn strip_prefix(v: String, prefix: &str) -> String {
    match v.strip_prefix(prefix) {
        Some(rest) => rest.to_owned(),
        None => v,
    }
}
/// Register constants as template globals, using the constant's identifier
/// as the template variable name.
///
/// Eliminates the manual `engine.add_global("NAME", NAME)` duplication —
/// the string key is derived via `stringify!`.
#[macro_export]
macro_rules! register_globals {
    ($engine:expr, $($name:ident),+ $(,)?) => {
        $($engine.add_global(stringify!($name), $name);)+
    };
}

pub use self::handle::{HandleBuilder, TemplateHandle};
use crate::dispatch::context::RequestContext;
use crate::node::Readable;

/// Unit tests.
#[cfg(test)]
mod tests;
