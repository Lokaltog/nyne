use std::mem;

use color_eyre::eyre::Result;
use extension::RouteExtension;

use crate::path_utils::PathExt;
use crate::router::pipeline::route::{HandlerFn, LookupFn, OpGuard, ReaddirFn, RouteCtx};
use crate::router::{NamedNode, Next, Op, Request};

pub mod extension;

/// Boxed content factory that optionally produces a [`NamedNode`].
///
/// Returning `None` signals "nothing here" — the node is silently omitted from
/// both Readdir and Lookup results. Accepts closures (can capture state) and
/// plain function pointers via `Box::new(...)`.
pub(crate) type ContentFn<T> = Box<dyn Fn(&T, &RouteCtx, &Request) -> Option<NamedNode> + Send + Sync>;

/// A declarative route tree. Nodes are virtual filesystem entities.
///
/// Static entries auto-emit for `Readdir`/`Lookup`. Handlers provide middleware
/// behavior.
pub struct RouteTree<T> {
    entries: Vec<Entry<ContentFn<T>, Self>>,
    handler: Option<HandlerFn<T>>,
    on_readdir: Vec<ReaddirFn<T>>,
    on_lookup: Vec<LookupFn<T>>,
    on_op: Vec<(OpGuard, HandlerFn<T>)>,
}

/// Structural entry in a route tree — shared between provider-typed
/// [`RouteTree`] and provider-agnostic [`RouteExtension`](extension::RouteExtension).
///
/// - `C`: content callback type (`ContentFn<T>` or `ContentCb`)
/// - `S`: subtree type (`RouteTree<T>` or `RouteExtension`)
pub(crate) enum Entry<C, S> {
    /// Content producer. Factory returns a [`NamedNode`] with name and capabilities.
    Content { content_fn: C },
    /// Static named directory with a subtree of children.
    Dir { name: String, tree: S },
    /// Dynamic single-segment capture. Matches any name, binds to param.
    Capture { param: &'static str, tree: S },
    /// Dynamic rest capture. Matches 1+ remaining segments, with a subtree.
    Rest { param: &'static str, tree: S },
}

/// Builder for constructing a [`RouteTree`].
pub struct TreeBuilder<T> {
    entries: Vec<Entry<ContentFn<T>, RouteTree<T>>>,
    handler: Option<HandlerFn<T>>,
    on_readdir: Vec<ReaddirFn<T>>,
    on_lookup: Vec<LookupFn<T>>,
    on_op: Vec<(OpGuard, HandlerFn<T>)>,
}

impl<T> TreeBuilder<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            handler: None,
            on_readdir: Vec::new(),
            on_lookup: Vec::new(),
            on_op: Vec::new(),
        }
    }

    /// Add a content producer. The factory returns `Option<NamedNode>` — `None`
    /// silently omits the node. It is the single source of truth for both name
    /// and capabilities.
    #[must_use]
    pub fn content(mut self, f: impl Fn(&T, &RouteCtx, &Request) -> Option<NamedNode> + Send + Sync + 'static) -> Self {
        self.entries.push(Entry::Content {
            content_fn: Box::new(f),
        });
        self
    }

    /// Add an infallible content producer that always emits a node.
    ///
    /// Same as [`content`](Self::content) but the factory returns `NamedNode`
    /// directly — internally wrapped in `Some`.
    #[must_use]
    pub fn content_always(mut self, f: impl Fn(&T, &RouteCtx, &Request) -> NamedNode + Send + Sync + 'static) -> Self {
        self.entries.push(Entry::Content {
            content_fn: Box::new(move |t, ctx, req| Some(f(t, ctx, req))),
        });
        self
    }

    /// Add a static named directory with a subtree.
    #[must_use]
    pub fn dir(mut self, name: impl Into<String>, f: impl FnOnce(Self) -> Self) -> Self {
        self.entries.push(Entry::Dir {
            name: name.into(),
            tree: f(Self::new()).build(),
        });
        self
    }

    /// Add a dynamic single-segment capture with a subtree.
    #[must_use]
    pub fn capture(mut self, param: &'static str, f: impl FnOnce(Self) -> Self) -> Self {
        self.entries.push(Entry::Capture {
            param,
            tree: f(Self::new()).build(),
        });
        self
    }

    /// Add a dynamic rest capture (1+ segments) with a subtree.
    ///
    /// After consuming all remaining path segments, the subtree's
    /// [`handle_here`](RouteTree::handle_here) runs — giving access to
    /// handlers, `on_readdir`/`on_lookup` callbacks, content producers,
    /// and auto-emit, just like `dir()` and `capture()`.
    #[must_use]
    pub fn rest(mut self, param: &'static str, f: impl FnOnce(Self) -> Self) -> Self {
        self.entries.push(Entry::Rest {
            param,
            tree: f(Self::new()).build(),
        });
        self
    }

    /// Set the middleware handler for this tree level.
    #[must_use]
    pub fn handler(
        mut self,
        f: impl for<'a> Fn(&T, &RouteCtx, &mut Request, &Next<'a>) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.handler = Some(Box::new(f));
        self
    }

    /// Add an op-specific callback for `Readdir`. Runs before `next`, does not
    /// manage the chain — just contributes nodes. Multiple callbacks are allowed.
    #[must_use]
    pub fn on_readdir(mut self, f: impl Fn(&T, &RouteCtx, &mut Request) -> Result<()> + Send + Sync + 'static) -> Self {
        self.on_readdir.push(Box::new(f));
        self
    }

    /// Add an op-specific callback for `Lookup`. Receives the looked-up name
    /// as `&str`. Runs before `next`, does not manage the chain. Multiple
    /// callbacks are allowed.
    #[must_use]
    pub fn on_lookup(
        mut self,
        f: impl Fn(&T, &RouteCtx, &mut Request, &str) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.on_lookup.push(Box::new(f));
        self
    }

    /// Add an op-guarded handler. When the tree dispatches to this level
    /// and `guard(req.op())` returns `true`, the handler runs with full
    /// chain control (it receives `next` and decides whether to continue).
    ///
    /// Multiple `on_op` entries are tried in registration order — the
    /// first match wins. Unmatched ops fall through to `on_readdir` /
    /// `on_lookup` callbacks, or `next.run()` if none are registered.
    ///
    /// Use [`Op`] predicate methods as guards:
    ///
    /// ```rust,ignore
    /// RouteTree::builder()
    ///     .on_op(Op::is_rename, |provider, _ctx, req, next| {
    ///         // handle rename at this tree level
    ///         Ok(())
    ///     })
    ///     .on_readdir(|provider, _ctx, req| {
    ///         // contribute nodes for readdir (simpler API)
    ///         Ok(())
    ///     })
    ///     .build()
    /// ```
    #[must_use]
    pub fn on_op(
        mut self,
        guard: OpGuard,
        handler: impl for<'a> Fn(&T, &RouteCtx, &mut Request, &Next<'a>) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.on_op.push((guard, Box::new(handler)));
        self
    }

    /// Apply a provider-agnostic [`RouteExtension`] at this tree level.
    ///
    /// Each extension callback is wrapped in a closure that ignores the `&T`
    /// provider reference. Subdirectories, captures, and rest entries are
    /// applied recursively.
    #[must_use]
    pub fn apply(self, ext: &RouteExtension) -> Self
    where
        T: Send + Sync + 'static,
    {
        ext.apply_to(self)
    }

    /// Build the tree, merging same-named `Dir` entries at each level.
    ///
    /// Two `.dir("foo", ...)` calls produce one `Dir` entry with the
    /// combined subtree — not two entries where the second shadows the
    /// first. This is recursive: nested same-named dirs are also merged.
    /// Handler follows last-writer-wins.
    pub fn build(self) -> RouteTree<T> {
        let mut tree = RouteTree {
            entries: self.entries,
            handler: self.handler,
            on_readdir: self.on_readdir,
            on_lookup: self.on_lookup,
            on_op: self.on_op,
        };
        tree.dedup_dirs();
        tree
    }
}

impl<T> Default for TreeBuilder<T> {
    fn default() -> Self { Self::new() }
}

impl<T> RouteTree<T> {
    pub fn builder() -> TreeBuilder<T> { TreeBuilder::new() }

    /// Merge same-named `Dir` entries at this tree level.
    ///
    /// Called by [`TreeBuilder::build`] to ensure first-match-wins dispatch
    /// is correct — without dedup, a second `Dir` with the same name would
    /// be unreachable. Recurses into merged subtrees to handle nested
    /// duplicates created by the merge.
    fn dedup_dirs(&mut self) {
        let entries = mem::take(&mut self.entries);
        let mut deduped = Vec::with_capacity(entries.len());

        for entry in entries {
            let Entry::Dir { name, tree } = entry else {
                deduped.push(entry);
                continue;
            };
            let Some(target) = deduped.iter_mut().find_map(|e| match e {
                Entry::Dir { name: n, tree } if *n == name => Some(tree),
                _ => None,
            }) else {
                deduped.push(Entry::Dir { name, tree });
                continue;
            };
            target.merge(tree);
            target.dedup_dirs();
        }

        self.entries = deduped;
    }

    /// Merge another tree's entries and callbacks into this one.
    ///
    /// Handler follows last-writer-wins: if `other` has a handler, it
    /// replaces the existing one.
    fn merge(&mut self, other: Self) {
        self.entries.extend(other.entries);
        self.on_readdir.extend(other.on_readdir);
        self.on_lookup.extend(other.on_lookup);
        self.on_op.extend(other.on_op);
        if other.handler.is_some() {
            self.handler = other.handler;
        }
    }

    /// Guard on typed state, then dispatch.
    ///
    /// If `S` is present in the request state, dispatches through this tree.
    /// Otherwise, passes through to `next.run(req)` (the provider has nothing
    /// to contribute for this request).
    ///
    /// Replaces the common four-line guard pattern:
    ///
    /// ```rust,ignore
    /// fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
    ///     if req.state::<Companion>().is_none() {
    ///         return next.run(req);
    ///     }
    ///     self.tree.dispatch(self, req, next)
    /// }
    /// ```
    ///
    /// With:
    ///
    /// ```rust,ignore
    /// fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
    ///     self.tree.dispatch_when::<Companion>(self, req, next)
    /// }
    /// ```
    ///
    /// Providers that need additional guards (visibility, op-specific
    /// interception) should use [`dispatch`](Self::dispatch) directly
    /// with their own guard logic.
    pub fn dispatch_when<S: Clone + Send + Sync + 'static>(
        &self,
        provider: &T,
        req: &mut Request,
        next: &Next,
    ) -> Result<()> {
        if req.state::<S>().is_none() {
            return next.run(req);
        }
        self.dispatch(provider, req, next)
    }

    /// Dispatch using request path components as route segments.
    pub fn dispatch(&self, provider: &T, req: &mut Request, next: &Next) -> Result<()> {
        let segments = req.path().segments();
        let seg_refs: Vec<&str> = segments.iter().map(String::as_str).collect();
        tracing::trace!(segments = ?seg_refs, "tree dispatch");
        let ctx = RouteCtx::new();
        self.dispatch_at(provider, &seg_refs, ctx, req, next)
    }

    fn dispatch_at(
        &self,
        provider: &T,
        segments: &[&str],
        mut ctx: RouteCtx,
        req: &mut Request,
        next: &Next,
    ) -> Result<()> {
        let Some((&head, tail)) = segments.split_first() else {
            return self.handle_here(provider, &ctx, req, next);
        };

        // Static dirs first (exact match takes priority)
        for entry in &self.entries {
            if let Entry::Dir { name, tree } = entry
                && name == head
            {
                return tree.dispatch_at(provider, tail, ctx, req, next);
            }
        }

        // Then captures
        for entry in &self.entries {
            match entry {
                Entry::Capture { param, tree } => {
                    ctx.push(param, head);
                    return tree.dispatch_at(provider, tail, ctx, req, next);
                }
                Entry::Rest { param, tree } => {
                    ctx.push_rest(param, segments);
                    return tree.handle_here(provider, &ctx, req, next);
                }
                _ => {}
            }
        }

        // No match — passthrough
        next.run(req)
    }

    /// Arrived at the target tree level — run the handler, then auto-emit.
    ///
    /// Priority order:
    /// 1. Full `handler` — owns the chain for all ops.
    /// 2. `on_op` guard match — first matching guard wins, owns the chain
    ///    for that op.
    /// 3. `on_readdir` / `on_lookup` callbacks — contribute nodes, then
    ///    `next.run()` is called automatically.
    ///
    /// After the handler/callbacks return, static entries (content producers
    /// and dirs) are auto-emitted for `Readdir`/`Lookup` ops. Mutation ops
    /// skip auto-emit entirely — see [`auto_emit`](Self::auto_emit).
    fn handle_here(&self, provider: &T, ctx: &RouteCtx, req: &mut Request, next: &Next) -> Result<()> {
        // 1. Full handler takes priority — it owns the chain.
        if let Some(handler) = &self.handler {
            handler(provider, ctx, req, next)?;
        } else if let Some((_, handler)) = self.on_op.iter().find(|(guard, _)| guard(req.op())) {
            // 2. Op-guarded handler — first match wins, full chain control.
            handler(provider, ctx, req, next)?;
        } else {
            // 3. Op-specific callbacks — contribute nodes, don't manage chain.
            self.run_op_callbacks(provider, ctx, req)?;
            next.run(req)?;
        }

        // 4. Fire Rest subtree callbacks for resolution ops so dynamic
        //    captures can contribute nodes and attach capabilities (e.g.
        //    Renameable) when a lookup/readdir targets the parent level.
        self.dispatch_into_rest(provider, ctx, req)?;

        // 5. Auto-emit static entries based on op
        self.auto_emit(provider, ctx, req);

        Ok(())
    }

    /// Fire `Rest` subtree callbacks for `Readdir`/`Lookup` at this level.
    ///
    /// When path traversal stops at a level that has a `Rest` entry, the
    /// rest subtree's callbacks haven't fired yet. This method enters each
    /// `Rest` subtree so its `on_readdir`/`on_lookup` callbacks can
    /// contribute nodes and attach capabilities (e.g. `Renameable`).
    ///
    /// For `Lookup`, the lookup name is captured as the rest parameter
    /// (e.g. `path = "Foo"`) so fragment-level callbacks can resolve it.
    /// For `Readdir`, an empty rest parameter is used — the subtree's
    /// readdir callbacks emit their entries at this level.
    fn dispatch_into_rest(&self, provider: &T, ctx: &RouteCtx, req: &mut Request) -> Result<()> {
        if req.op().is_mutation() {
            return Ok(());
        }
        let name;
        let segments: &[&str] = match req.op().target_name() {
            Some(n) => {
                name = n.to_owned();
                &[name.as_str()]
            }
            None => &[],
        };
        for entry in &self.entries {
            let Entry::Rest { param, tree } = entry else { continue };
            let mut rest_ctx = ctx.clone();
            rest_ctx.push_rest(param, segments);
            tree.run_op_callbacks(provider, &rest_ctx, req)?;
        }
        Ok(())
    }

    /// Run op-specific callbacks registered via `on_readdir` / `on_lookup`.
    ///
    /// **Lookup fallback:** when `on_lookup` is empty but `on_readdir` is not,
    /// runs the readdir callbacks and retains only the node matching the lookup
    /// name. This ensures anything emitted by readdir is also resolvable by
    /// lookup without requiring every provider to register both callbacks.
    fn run_op_callbacks(&self, provider: &T, ctx: &RouteCtx, req: &mut Request) -> Result<()> {
        match req.op() {
            Op::Readdir =>
                for f in &self.on_readdir {
                    f(provider, ctx, req)?;
                },
            Op::Lookup { name } | Op::Remove { name } if !self.on_lookup.is_empty() => {
                let name = name.clone();
                for f in &self.on_lookup {
                    f(provider, ctx, req, &name)?;
                }
            }
            Op::Lookup { name } if !self.on_readdir.is_empty() => {
                // Fallback: run readdir, keep only the matching node.
                let name = name.clone();
                for f in &self.on_readdir {
                    f(provider, ctx, req)?;
                }
                req.nodes.retain(|n| n.name() == name);
            }
            _ => {}
        }
        Ok(())
    }

    /// Auto-emit static entries for resolution ops (`Readdir`/`Lookup`).
    ///
    /// Mutation ops (`Create`, `Mkdir`, `Remove`, `Rename`) are intentionally
    /// skipped — they don't produce directory listings. Mutations require
    /// explicit [`HandlerFn`] at the appropriate tree level to intercept
    /// and handle the op.
    fn auto_emit(&self, provider: &T, ctx: &RouteCtx, req: &mut Request) {
        if !req.op().is_query() {
            return;
        }
        let op = req.op().clone();

        // Collect nodes first (ContentFn takes &Request, add takes &mut Request)
        let nodes = self.collect_auto_nodes(provider, ctx, req, &op);
        for node in nodes {
            req.nodes.add(node);
        }
    }

    /// Collect nodes from static entries at this tree level.
    ///
    /// Uses [`Op::target_name`] to unify `Readdir` and `Lookup` handling:
    /// when a target name is present (lookup), only matching entries are
    /// returned; otherwise (readdir), all entries are emitted.
    /// `Capture` and `Rest` entries are always skipped (dynamic — not listable).
    ///
    /// Only called for query ops — the caller ([`auto_emit`](Self::auto_emit))
    /// guards against mutation ops via [`Op::is_query`].
    fn collect_auto_nodes(&self, provider: &T, ctx: &RouteCtx, req: &Request, op: &Op) -> Vec<NamedNode> {
        let lookup_name = op.target_name();

        self.entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Content { content_fn } => {
                    let node = content_fn(provider, ctx, req)?;
                    match lookup_name {
                        Some(name) => (node.name() == name).then_some(node),
                        None => Some(node),
                    }
                }
                Entry::Dir { name, .. } => match lookup_name {
                    Some(target) if name.as_str() != target => None,
                    _ => Some(NamedNode::dir(name.as_str())),
                },
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
