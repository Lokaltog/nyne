//! Git status view — working tree status and index state.

use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::templates::{TemplateEngine, TemplateView};

use crate::repo::GitRepo;

/// Git status view — renders working tree and index state.
///
/// Backs the `@/git/STATUS.md` virtual file. Calls [`GitRepo::status()`]
/// at read time to capture branch, tracking, staged/modified files, and
/// recent commits, then renders the result via Jinja template.
pub(super) struct GitStatusView {
    pub repo: Arc<GitRepo>,
}

/// [`TemplateView`] implementation for [`GitStatusView`].
impl TemplateView for GitStatusView {
    /// Renders the working tree status using a template.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let data = self.repo.status()?;
        Ok(engine.render_bytes(template, &minijinja::context!(data)))
    }
}
