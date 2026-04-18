//! Provider serving `/.nyne.md` at the mount root.
//!
//! Hidden (lookup-only) — the file does not appear in readdir but is
//! accessible by name. Content is rendered at read time so uptime and
//! provider state are always fresh.

mod views;

use std::sync::Arc;
use std::time::{Duration, Instant};

use nyne::prelude::*;
use nyne::templates::TemplateHandle;
use views::render_status;

/// File name served at mount root.
const FILE_NAME: &str = ".nyne.md";

pub struct NyneProvider {
    pub(crate) template: TemplateHandle,
    pub(crate) ctx: Arc<ActivationContext>,
    pub(crate) start_time: Instant,
}

nyne::define_provider!(NyneProvider, "nyne");

impl Provider for NyneProvider {
    fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
        // Only intercept lookup for `.nyne.md` at mount root (hidden — not in readdir).
        if req.path().as_os_str().is_empty() && req.op().lookup_name() == Some(FILE_NAME) {
            let ctx = Arc::clone(&self.ctx);
            let start = self.start_time;
            let (_, node) = self
                .template
                .lazy_node(FILE_NAME, move |engine, tmpl| {
                    Ok(render_status(engine, tmpl, &ctx, start))
                })
                .into_parts();
            req.nodes.add(
                node.with_cache_policy(CachePolicy::Ttl(Duration::from_secs(1)))
                    .named(FILE_NAME),
            );
            return Ok(());
        }
        next.run(req)
    }
}

#[cfg(test)]
mod tests;
