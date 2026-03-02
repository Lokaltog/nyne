//! Git status view — working tree status and index state.

use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::templates::{TemplateEngine, TemplateView};

use crate::repo::GitRepo;

/// Git status view — renders working tree and index state.
pub(super) struct GitStatusView {
    pub repo: Arc<GitRepo>,
}

impl TemplateView for GitStatusView {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let data = self.repo.status()?;
        Ok(engine.render_bytes(template, &minijinja::context!(data)))
    }
}
