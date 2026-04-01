mod state;
use color_eyre::eyre::Result;
use nyne::router::{Next, Provider, Request};
pub use state::*;
use tracing::debug;

/// Middleware that sets [`Visibility`] state and post-filters virtual nodes.
///
/// Runs first in the chain (priority -100). After the full chain completes,
/// strips virtual nodes (those without a `backing_path`) for hidden
/// processes. Real filesystem nodes always pass through.
///
/// - `Default` -- keep all (providers may self-filter)
/// - `Force` -- keep all
/// - `Hidden` -- strip virtual on both readdir and lookup
/// - No state -- keep all
pub struct VisibilityProvider {
    pub(crate) policy: VisibilityPolicy,
}

nyne::define_provider!(VisibilityProvider, "visibility", priority: -100);

impl Provider for VisibilityProvider {
    fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
        let visibility = (self.policy)(req);
        if let Some(v) = visibility {
            debug!(
                pid = req.process().map(|p| p.pid),
                name = req.process().and_then(|p| p.name.as_deref()),
                visibility = ?v,
                path = %req.path().display(),
                op = ?req.op(),
                "visibility:set",
            );
            req.set_state(v);
        }
        next.run(req)?;

        // Post-filter: strip virtual nodes for hidden processes (e.g. git).
        // Real nodes (those with a backing_path) are always retained so
        // hidden processes still see the real filesystem.
        //
        // Uses the locally-computed visibility rather than re-reading from
        // state — the cache middleware may have overwritten state with a
        // snapshot from a different process's request.
        if matches!(visibility, Some(Visibility::Hidden)) {
            let before = req.nodes.len();
            req.nodes
                .retain(|n| n.readable().is_some_and(|r| r.backing_path().is_some()));
            let stripped = before - req.nodes.len();
            if stripped > 0 {
                debug!(stripped, path = %req.path().display(), "visibility:post-filter stripped virtual nodes");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
