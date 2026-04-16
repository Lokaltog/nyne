//! Shared template engine and view abstractions for virtual file content.
//!
//! Provides [`TemplateEngine`] (thin wrapper around [`minijinja::Environment`])
//! and the [`TemplateView`] / [`TemplateContent`] pattern for read-time
//! template rendering.
//!
//! **Provider workflow:**
//! 1. Create [`TemplateHandle`]s via [`HandleBuilder`] — each handle binds a
//!    template key to a shared engine.
//! 2. Call [`TemplateHandle::router_node`] at resolve time to produce `NamedNode`s.

/// Pre-bound template handles for producing virtual file nodes.
mod handle;

use std::borrow::Cow;

use color_eyre::eyre;
use convert_case::{Case, Casing};
use minijinja::Environment;
use serde::Serialize;

use crate::prelude::*;
use crate::router::{ReadContext, Readable, Writable, WriteContext};

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

    /// Add a single global variable available to all templates.
    ///
    /// For bulk registration from a config struct, prefer
    /// [`add_globals_from`](Self::add_globals_from) or the
    /// [`TemplateGlobals`] trait.
    pub fn add_global(&mut self, name: impl Into<Cow<'static, str>>, value: impl Into<String>) {
        self.env.add_global(name, minijinja::Value::from(value.into()));
    }

    /// Register every string field of a [`Serialize`] value as a template
    /// global, with keys derived from the nesting path.
    ///
    /// Each string leaf produces a global whose name is the path segments
    /// joined with `_` and converted to `UPPER_SNAKE_CASE`. The root
    /// value's own name is not included.
    ///
    /// ```ignore
    /// struct Vfs { dir: VfsDirs, file: VfsFiles }
    /// struct VfsDirs { git: String }          // → DIR_GIT
    /// struct VfsFiles { head_diff: String }   // → FILE_HEAD_DIFF
    /// ```
    ///
    /// Types that want to customize naming should implement
    /// [`TemplateGlobals`] manually instead of relying on the default impl.
    ///
    /// # Panics
    ///
    /// Panics if serialization fails. Call sites pass plain config structs,
    /// so this is a programming error, not a runtime condition.
    #[allow(clippy::expect_used)]
    pub fn add_globals_from<T: Serialize + ?Sized>(&mut self, value: &T) {
        walk_globals(
            &serde_json::to_value(value).expect("serialize to json value for template globals"),
            &mut Vec::new(),
            self,
        );
    }

    /// Render a named template to bytes.
    ///
    /// Convenience wrapper around [`Self::render`] for the common case where
    /// callers need `Vec<u8>` (i.e., every [`TemplateView`] impl).
    pub fn render_bytes(&self, name: &str, view: &impl Serialize) -> Vec<u8> { self.render(name, view).into_bytes() }
}

/// Walk a JSON value and register every string leaf as a template global.
///
/// Keys are built from `path` joined with `_`, each segment converted to
/// `UPPER_SNAKE_CASE`. Non-string, non-object leaves are ignored.
fn walk_globals<'a>(v: &'a serde_json::Value, path: &mut Vec<&'a str>, engine: &mut TemplateEngine) {
    match v {
        serde_json::Value::String(s) => {
            engine.add_global(
                path.iter()
                    .map(|p| p.to_case(Case::UpperSnake))
                    .collect::<Vec<_>>()
                    .join("_"),
                s.clone(),
            );
        }
        serde_json::Value::Object(map) =>
            for (k, child) in map {
                path.push(k);
                walk_globals(child, path, engine);
                path.pop();
            },
        _ => {}
    }
}

/// Register a type's fields as template globals.
///
/// The default implementation walks the serialized form via
/// [`TemplateEngine::add_globals_from`], producing one global per string
/// leaf with a key derived from the nesting path (`UPPER_SNAKE_CASE`, joined
/// by `_`, root struct name dropped).
///
/// Override this method to customize naming or skip fields.
pub trait TemplateGlobals: Serialize {
    /// Register this value's string fields as template globals.
    fn register_globals(&self, engine: &mut TemplateEngine) { engine.add_globals_from(self); }
}

/// Produce rendered bytes at read time.
///
/// Implement this trait for views that need to compute, fetch, or transform
/// data during rendering. For simple `Serialize` structs, use [`serialize_view`]
/// instead.
pub trait TemplateView: Send + Sync {
    /// Render this view through the given template, returning the output bytes.
    ///
    /// Called on every FUSE read of the virtual file. Implementations should
    /// be idempotent -- repeated calls with the same state produce identical output.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>>;
}

/// Adapts a pre-serialized [`minijinja::Value`] into a [`TemplateView`].
///
/// The value is captured eagerly at construction time via [`serialize_view`],
/// so the original `Serialize` type does not need to be `'static`.
struct SerializeView(minijinja::Value);

/// Renders the pre-serialized value through the template engine.
impl TemplateView for SerializeView {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        Ok(engine.render_bytes(template, &self.0))
    }
}

/// Blanket impl: `Arc<dyn TemplateView>` delegates to the inner view.
impl<T: TemplateView + ?Sized> TemplateView for Arc<T> {
    /// Delegates rendering to the inner view.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> { (**self).render(engine, template) }
}

/// Adapt any [`Serialize`] value into a [`TemplateView`].
///
/// Eagerly captures the value as a [`minijinja::Value`] at call time, so
/// the input type does not need to be `'static` — borrowed views work fine.
pub fn serialize_view<T: Serialize>(val: &T) -> impl TemplateView + use<T> {
    SerializeView(minijinja::Value::from_serialize(val))
}

/// Closure-backed [`TemplateView`] — captures state at dispatch time,
/// renders lazily at read time.
///
/// When `W` is a `Fn(&WriteContext, &[u8]) -> Result<AffectedFiles>` the
/// view also implements [`Writable`](crate::router::Writable). The default
/// `W = ()` produces a read-only view.
pub struct LazyView<R, W = ()> {
    read_fn: R,
    write_fn: W,
}


impl<R> LazyView<R, ()> {
    /// Create a read-only lazy view.
    pub const fn new(read_fn: R) -> Self { Self { read_fn, write_fn: () } }

    /// Attach a write callback, producing a read+write view.
    pub fn writable<W>(self, write_fn: W) -> LazyView<R, W> {
        LazyView {
            read_fn: self.read_fn,
            write_fn,
        }
    }
}


impl<R, W> TemplateView for LazyView<R, W>
where
    R: Fn(&TemplateEngine, &str) -> Result<Vec<u8>> + Send + Sync,
    W: Send + Sync,
{
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> { (self.read_fn)(engine, template) }
}


impl<R, W> Writable for LazyView<R, W>
where
    R: Fn(&TemplateEngine, &str) -> Result<Vec<u8>> + Send + Sync,
    W: Fn(&WriteContext<'_>, &[u8]) -> Result<AffectedFiles> + Send + Sync,
{
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> { (self.write_fn)(ctx, data) }
}


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
impl Readable for TemplateContent {
    fn read(&self, _ctx: &ReadContext<'_>) -> eyre::Result<Vec<u8>> { self.view.render(&self.engine, self.template) }
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
///
/// Returns `String` because minijinja's `Function` trait requires `Rv: FunctionResult`
/// with no input lifetime threading (`Args: for<'a> FunctionArgs<'a>`), so the return
/// type cannot borrow from the input `&str`. `Cow<str>` would still allocate here.
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
fn strip_prefix(mut v: String, prefix: &str) -> String {
    if v.starts_with(prefix) {
        v.replace_range(..prefix.len(), "");
    }
    v
}

pub use self::handle::{HandleBuilder, TemplateHandle};

/// Unit tests.
#[cfg(test)]
mod tests;
