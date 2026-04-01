use color_eyre::eyre::Result;

use crate::router::chain::Next;
use crate::router::request::{Op, Request};

/// Captured route parameters from pattern matching.
#[derive(Clone, Debug)]
pub struct RouteCtx {
    params: Vec<(&'static str, String)>,
}

impl RouteCtx {
    pub const fn new() -> Self { Self { params: Vec::new() } }

    /// Get a single captured parameter by name.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    /// Push a single capture.
    pub(crate) fn push(&mut self, name: &'static str, value: &str) { self.params.push((name, value.to_owned())); }

    /// Push rest captures as a joined path.
    pub(crate) fn push_rest(&mut self, name: &'static str, values: &[&str]) {
        self.params.push((name, values.join("/")));
    }
}

impl Default for RouteCtx {
    fn default() -> Self { Self::new() }
}

/// Boxed handler closure. Takes full control of the middleware chain — must
/// call `next.run(req)` explicitly if dispatch should continue.
pub type HandlerFn<T> = Box<dyn for<'a> Fn(&T, &RouteCtx, &mut Request, &Next<'a>) -> Result<()> + Send + Sync>;

/// Boxed callback for `Readdir` — contributes nodes without managing `next`.
pub type ReaddirFn<T> = Box<dyn Fn(&T, &RouteCtx, &mut Request) -> Result<()> + Send + Sync>;

/// Boxed callback for `Lookup` — receives the looked-up name, contributes
/// nodes without managing `next`.
pub type LookupFn<T> = Box<dyn Fn(&T, &RouteCtx, &mut Request, &str) -> Result<()> + Send + Sync>;

/// Predicate for op-guarded dispatch — tests whether the current [`Op`]
/// should be handled by the associated handler.
///
/// Use [`Op`] predicate methods as guards: `Op::is_rename`, `Op::is_create`, etc.
pub type OpGuard = fn(&Op) -> bool;
