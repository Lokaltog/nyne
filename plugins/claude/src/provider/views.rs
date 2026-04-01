use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use nyne::router::{ReadContext, Readable};
use nyne::templates::{TemplateEngine, TemplateView};

use super::settings;

/// Readable that layers nyne defaults, user `settings.json`, and injected hooks.
///
/// On read, merges three sources in priority order: nyne's built-in defaults
/// (hook entries, permissions), the user's on-disk `settings.json` (if present),
/// and dynamically injected hook scripts. The merge is non-destructive — user
/// settings are preserved and nyne entries are additive.
pub(super) struct SettingsContent {
    pub(super) root: PathBuf,
}
/// [`Readable`] implementation for [`SettingsContent`].
impl Readable for SettingsContent {
    /// Read merged settings layering nyne defaults, user config, and injected hooks.
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        let settings_path = Path::new(".claude").join("settings.json");
        let real_json = ctx.fs.read_file(&settings_path).ok();
        settings::render_settings(real_json.as_deref(), &self.root)
    }
}
/// Template view data for a skill directory listing.
#[derive(Clone, serde::Serialize)]
pub(super) struct SkillView {
    pub(super) source_dir: String,
    pub(super) ext: String,
}
/// Dynamic view for system prompt — computes environment data at render time.
pub(super) struct SystemPromptView {
    pub(super) root: PathBuf,
    pub(super) ext: String,
}
/// [`TemplateView`] implementation for [`SystemPromptView`].
impl TemplateView for SystemPromptView {
    /// Render the system prompt with environment data.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let view = minijinja::context! {
            ext => &self.ext,
            source_dir => self.root.display().to_string(),
        };
        Ok(engine.render_bytes(template, &view))
    }
}
