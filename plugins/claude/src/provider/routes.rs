//! Route tree and handler/content functions for the Claude provider.

use nyne::router::{NamedNode, Node, RouteTree};
use nyne::templates::{TemplateHandle, serialize_view};

use super::ClaudeProvider;
use super::skills::{SKILLS, SkillDef};
use super::views::{SettingsContent, SkillView, SystemPromptView};

/// Settings file name.
const SETTINGS_FILE: &str = "settings.json";
/// Default output style file name.
const OUTPUT_STYLE_FILE: &str = "principal-swe.md";
/// Skill definition file name.
const SKILL_FILE: &str = "SKILL.md";
/// Directory name for skill reference documents.
const REFERENCES_DIR: &str = "references";

/// Build the mount-root route tree (`.claude/` directory).
#[allow(clippy::excessive_nesting)]
pub fn build_root_tree() -> RouteTree<ClaudeProvider> {
    RouteTree::builder()
        .dir(".claude", |d| {
            d
                // .claude/settings.json -- merged settings
                .content_always(|p: &ClaudeProvider, _ctx, _req| {
                    Node::file()
                        .with_readable(SettingsContent {
                            root: p.root.clone(),
                        })
                        .named(SETTINGS_FILE)
                })
                .dir("skills", |d| {
                    d.on_readdir(|_p, _ctx, req| {
                        for skill in SKILLS {
                            req.nodes.add(NamedNode::dir(skill.dir));
                        }
                        Ok(())
                    })
                    .on_lookup(|_p, _ctx, req, name| {
                        if SKILLS.iter().any(|s| s.dir == name) {
                            req.nodes.add(NamedNode::dir(name));
                        }
                        Ok(())
                    })
                    .capture("skill", |d| {
                        d.content(|p, ctx, _req| {
                            let s = ctx.param("skill").and_then(SkillDef::find)?;
                            Some(p.skill_node(s.skill_tmpl, SKILL_FILE))
                        })
                        .content(|_p, ctx, _req| {
                            let s = ctx.param("skill").and_then(SkillDef::find)?;
                            (!s.references.is_empty()).then(|| NamedNode::dir(REFERENCES_DIR))
                        })
                        .dir("references", |d| {
                            d.on_readdir(|p, ctx, req| {
                                let Some(skill) = ctx.param("skill").and_then(SkillDef::find) else {
                                    return Ok(());
                                };
                                for &(name, tmpl) in skill.references {
                                    req.nodes.add(p.skill_node(tmpl, name));
                                }
                                Ok(())
                            })
                            .on_lookup(|p, ctx, req, name| {
                                let Some(skill) = ctx.param("skill").and_then(SkillDef::find) else {
                                    return Ok(());
                                };
                                if let Some(&(_, tmpl)) = skill.references.iter().find(|(n, _)| *n == name) {
                                    req.nodes.add(p.skill_node(tmpl, name));
                                }
                                Ok(())
                            })
                        })
                    })
                })
                // .claude/agents/nyne.md -- agent template
                .dir("agents", |d| {
                    d.content_always(|p: &ClaudeProvider, _ctx, _req| {
                        TemplateHandle::new(&p.templates, "claude/agent-nyne")
                            .named_node("nyne.md", serialize_view(&p.skill_view()))
                    })
                })
                // .claude/output-styles/principal-swe.md -- output style template
                .dir("output-styles", |d| {
                    d.content_always(|p: &ClaudeProvider, _ctx, _req| {
                        TemplateHandle::new(&p.templates, "claude/output-style")
                            .named_node(OUTPUT_STYLE_FILE, serialize_view(&p.skill_view()))
                    })
                })
        })
        .build()
}

#[allow(clippy::excessive_nesting)]
/// Build the companion-scoped route tree (`@/agents/`).
pub fn build_at_tree() -> RouteTree<ClaudeProvider> {
    RouteTree::builder()
        .dir("agents", |d| {
            d.dir("claude-code", |d| {
                // @/agents/claude-code/system-prompts/default.md -- system prompt
                d.dir("system-prompts", |d| {
                    d.content_always(|p: &ClaudeProvider, _ctx, _req| {
                        TemplateHandle::new(&p.templates, "claude/system-prompt").named_node(
                            "default.md",
                            SystemPromptView {
                                root: p.root.clone(),
                                ext: p.ext.clone(),
                            },
                        )
                    })
                })
            })
        })
        .build()
}

/// Complex handler methods and shared helpers.
impl ClaudeProvider {
    /// Skills handler: add skill directories for Readdir/Lookup.
    #[allow(clippy::unused_self)] // signature required by HandlerFn
    /// Build a [`NamedNode`] for a skill template.
    fn skill_node(&self, tmpl: &'static str, name: impl Into<String>) -> NamedNode {
        TemplateHandle::new(&self.templates, tmpl).named_node(name, serialize_view(&self.skill_view()))
    }

    /// Build the skill view with current project root and extension.
    fn skill_view(&self) -> SkillView {
        SkillView {
            source_dir: self.root.display().to_string(),
            ext: self.ext.clone(),
        }
    }
}
