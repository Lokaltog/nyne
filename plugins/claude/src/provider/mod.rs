//! Claude Code integration — hooks, settings, and tool dispatch.
//!
//! Provides a virtual `.claude/` directory tree containing merged settings,
//! injected hook scripts, skill definitions, and a system prompt. The
//! provider activates only when `claude.enabled` is true in source config.
//!
//! Hook scripts are registered via [`script_entries`] and execute as
//! [`Script`](nyne::Script) trait objects — one per
//! Claude Code hook event type (pre-tool-use, post-tool-use, session-start,
//! stop, statusline).

use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{Next, Provider, Request, RouteTree};
use nyne::templates::TemplateEngine;
use nyne_companion::{CompanionProvider, CompanionRequest};
use nyne_visibility::{Visibility, VisibilityRequest};

use crate::plugin::config::Config;

/// Stable identifiers for nyne-injected Claude Code hook scripts.
pub mod hook_id;
/// Typed serde schemas for hook inputs and outputs.
pub mod hook_schema;
/// Claude Code hook implementations as script trait objects.
pub mod hooks;
/// Route tree and handler/content functions.
pub mod routes;
/// Claude Code user settings and configuration.
mod settings;
/// Skill definitions and template registration.
mod skills;
/// Shared template partials and macros registered into every engine.
mod templates_shared;
/// View types for settings, skills, and system prompt.
pub mod views;

use skills::register_skill_templates;

/// Provider for Claude Code integration — hooks, settings, and tool dispatch.
///
/// Contributes the virtual `.claude/` directory containing `settings.json`
/// (merged from nyne defaults + user config + injected hooks), a system
/// prompt, skill directories with reference docs, and an output style file.
/// Activates only when `claude.enabled` is true; merges non-destructively
/// with any pre-existing `.claude/` directory on the real filesystem.
pub struct ClaudeProvider {
    pub(crate) root: PathBuf,
    pub(crate) config: Config,
    pub(crate) root_tree: RouteTree<Self>,
    pub(crate) at_tree: RouteTree<Self>,
    pub(crate) templates: Arc<TemplateEngine>,
    pub(crate) ext: String,
}

pub fn build_templates() -> Arc<TemplateEngine> {
    let mut b = templates_shared::new_builder();
    register_skill_templates(&mut b);
    b.register("claude/output-style", include_str!("templates/output-style.md.j2"));
    b.register("claude/system-prompt", include_str!("templates/system-prompt.md.j2"));
    b.register("claude/agent-nyne", include_str!("templates/agents/nyne.md.j2"));
    b.finish()
}
/// [`Provider`] implementation for [`ClaudeProvider`].
impl Provider for ClaudeProvider {
    fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
        if !self.config.enabled {
            return next.run(req);
        }

        let Some(companion) = req.companion() else {
            // Non-companion (.claude/) — merge virtual files into the real directory.
            // Hidden processes (e.g. git) see only on-disk files.
            if matches!(req.visibility(), Some(Visibility::Hidden)) {
                return next.run(req);
            }
            return self.root_tree.dispatch(self, req, next);
        };

        // Per-file companions — nothing to contribute.
        if companion.source_file.is_some() {
            return next.run(req);
        }

        // Mount-wide companion (@/) — agents/claude-code/system-prompts/.
        self.at_tree.dispatch(self, req, next)
    }
}

nyne::define_provider!(ClaudeProvider, "claude", deps: [CompanionProvider]);
