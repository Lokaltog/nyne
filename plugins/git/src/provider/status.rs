//! Git status view — working tree status and index state.

use nyne::prelude::*;
use nyne::templates::TemplateEngine;

use crate::plugin::config::Limits;
use crate::repo::Repo;
use crate::status::StatusQueries as _;

/// Git status view — renders working tree and index state.
///
/// Backs the `@/git/STATUS.md` virtual file. Calls [`Repo::status()`]
/// at read time to capture branch, tracking, staged/modified files, and
/// recent commits, then renders the result via Jinja template.
pub(super) struct StatusView {
    pub repo: Arc<Repo>,
    pub limits: Limits,
}

/// [`TemplateView`] implementation for [`StatusView`].
impl TemplateView for StatusView {
    /// Renders the working tree status using a template.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let data = self.repo.status(self.limits.recent_commits)?;
        Ok(engine.render_bytes(template, &minijinja::context!(data)))
    }
}
