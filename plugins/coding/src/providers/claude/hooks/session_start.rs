//! `SessionStart` hook — surfaces VFS guidance and project context on startup/resume.

use std::env;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::dispatch::script::{Script, ScriptContext};
use nyne::templates::TemplateEngine;

use crate::providers::claude::hook_schema::HookOutput;
use crate::providers::names;

/// Template key for the session start hook.
const TMPL_SESSION_START: &str = "claude/session-start";

/// `SessionStart` hook script implementation.
pub(in crate::providers::claude) struct SessionStart {
    engine: Arc<TemplateEngine>,
}

/// Methods for [`SessionStart`].
impl SessionStart {
    /// Create a new session start hook with registered templates.
    pub fn new() -> Self {
        let mut b = names::handle_builder();
        b.register(TMPL_SESSION_START, include_str!("templates/session-start.md.j2"));
        Self { engine: b.finish() }
    }
}

/// [`Script`] implementation for [`SessionStart`].
impl Script for SessionStart {
    /// Render session start context with mount status and project instructions.
    fn exec(&self, ctx: &ScriptContext<'_>, _stdin: &[u8]) -> Result<Vec<u8>> {
        let activation = ctx.activation();
        let branch = {
            #[cfg(feature = "git-symbols")]
            {
                activation
                    .get::<Arc<nyne_git::GitRepo>>()
                    .map_or_else(|| "(no repo)".to_owned(), |r| r.head_branch())
            }
            #[cfg(not(feature = "git-symbols"))]
            {
                "(no repo)".to_owned()
            }
        };

        let view = minijinja::context! {
            current_date => jiff::Zoned::now().strftime("%Y-%m-%d").to_string(),
            worktree_path => activation.root().display().to_string(),
            branch,
            project_root => activation.host_root().display().to_string(),
            platform => format!("{}/{}", env::consts::OS, env::consts::ARCH),
            shell => env::var("SHELL").unwrap_or_default(),
        };

        let msg = self.engine.render(TMPL_SESSION_START, &view);
        Ok(HookOutput::context("SessionStart", msg.trim().to_owned()).to_bytes())
    }
}
