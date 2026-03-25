// Core LSP feature dispatch — LspFeature enum, LspHandles, LspTarget.

use std::ops::Range as StdRange;
use std::path::PathBuf;
use std::sync::LazyLock;

use color_eyre::eyre::Result;
use convert_case::{Case, Casing};
use lsp_types::{Position, Range};
use nyne::templates::{TemplateEngine, TemplateHandle};
use strum::{EnumCount, IntoEnumIterator};

use super::views::{LspQueryResult, hierarchy_item, type_hierarchy_item};
use crate::lsp::query::FileQuery;

/// Internal metadata for a single LSP feature variant.
struct FeatureMeta {
    slug: &'static str,
    file_name: String,
    dir_name: Option<String>,
    template_key: &'static str,
    template_src: &'static str,
}
/// Single source of truth for all per-symbol LSP features.
///
/// Every LSP feature — its file name, directory name, template key,
/// query dispatch, view construction, and symlink target generation —
/// is derived from this enum. Adding a new feature means adding a
/// variant here plus a template file.
#[derive(Clone, Copy, strum::EnumIter, strum::EnumCount)]
#[repr(u8)]
pub(in crate::providers::syntax) enum LspFeature {
    Definition,
    Declaration,
    TypeDefinition,
    References,
    Implementation,
    Callers,
    Deps,
    Supertypes,
    Subtypes,
    Doc,
    Hints,
}

/// Methods for [`LspFeature`].
impl LspFeature {
    /// Cached metadata for all variants, computed once on first access.
    ///
    /// Returns a `&'static` reference — no per-call allocation.
    fn metadata(self) -> &'static FeatureMeta {
        static TABLE: LazyLock<Vec<FeatureMeta>> =
            LazyLock::new(|| LspFeature::iter().map(LspFeature::build_metadata).collect());
        // SAFETY: TABLE has exactly LspFeature::COUNT entries (one per variant),
        // and `self as usize` is always a valid variant index.
        #[expect(clippy::indexing_slicing, reason = "variant index is always in bounds")]
        &TABLE[self as usize]
    }

    /// Single metadata table per variant — the **only** place that maps
    /// variant → per-variant data.
    ///
    /// All names are derived from the slug via `convert_case`. Adding a
    /// new feature = adding one arm here (plus `is_supported` and `query`).
    fn build_metadata(self) -> FeatureMeta {
        /// Build a [`FeatureMeta`] from a single slug.
        ///
        /// Derives `file_name` and `dir_name` via case conversion,
        /// `template_key` and `template_src` via `concat!`/`include_str!`.
        macro_rules! meta {
            ($slug:literal) => {
                FeatureMeta {
                    slug: $slug,
                    file_name: format!("{}.md", $slug.to_case(Case::UpperKebab)),
                    dir_name: Some($slug.to_case(Case::Kebab)),
                    template_key: concat!("syntax/lsp/", $slug),
                    template_src: include_str!(concat!("../../templates/lsp/", $slug, ".md.j2")),
                }
            };
            ($slug:literal,no_dir) => {
                FeatureMeta {
                    slug: $slug,
                    file_name: format!("{}.md", $slug.to_case(Case::UpperKebab)),
                    dir_name: None,
                    template_key: concat!("syntax/lsp/", $slug),
                    template_src: include_str!(concat!("../../templates/lsp/", $slug, ".md.j2")),
                }
            };
        }
        match self {
            Self::Definition => meta!("definition"),
            Self::Declaration => meta!("declaration"),
            Self::TypeDefinition => meta!("type_definition"),
            Self::References => meta!("references"),
            Self::Implementation => meta!("implementation"),
            Self::Callers => meta!("callers"),
            Self::Deps => meta!("deps"),
            Self::Supertypes => meta!("supertypes"),
            Self::Subtypes => meta!("subtypes"),
            Self::Doc => meta!("doc", no_dir),
            Self::Hints => meta!("hints", no_dir),
        }
    }

    /// Virtual file name derived from the feature slug.
    pub(super) fn file_name(self) -> &'static str { &self.metadata().file_name }

    /// Symlink directory name for this feature, if it has one.
    pub(super) fn dir_name(self) -> Option<&'static str> { self.metadata().dir_name.as_deref() }

    /// Template registration key and source for this feature.
    ///
    /// Used by `SyntaxProvider::new()` to build the handles array by
    /// iterating `LspFeature::iter()` — no positional coupling.
    pub(in crate::providers::syntax) fn template(self) -> (&'static str, &'static str) {
        let m = self.metadata();
        (m.template_key, m.template_src)
    }

    /// Register all LSP feature file names as template globals.
    ///
    /// Each feature registers `FILE_{UPPER_SNAKE}` → `{UPPER-KEBAB}.md`,
    /// e.g. `FILE_TYPE_DEFINITION` → `TYPE-DEFINITION.md`. Called once
    /// during provider initialization.
    pub(in crate::providers::syntax) fn register_globals(engine: &mut TemplateEngine) {
        for feature in Self::iter() {
            let m = feature.metadata();
            let mut key = String::from("FILE_");
            key.push_str(&m.slug.to_case(Case::UpperSnake));
            engine.add_global(key, &m.file_name);
        }
    }

    /// Index into a `LspHandles` array to get the handle for this feature.
    pub(in crate::providers::syntax) const fn handle_index(self) -> usize { self as usize }

    /// Look up a feature by its symlink directory name.
    pub(in crate::providers::syntax) fn from_dir_name(name: &str) -> Option<Self> {
        Self::iter().find(|f| f.dir_name() == Some(name))
    }

    /// Whether the server supports this feature, based on advertised capabilities.
    ///
    /// Used at resolve time to suppress directories and files for features
    /// the LSP server does not implement.
    pub(in crate::providers::syntax) const fn is_supported(self, caps: &lsp_types::ServerCapabilities) -> bool {
        match self {
            Self::Definition => caps.definition_provider.is_some(),
            Self::Declaration => caps.declaration_provider.is_some(),
            Self::TypeDefinition => caps.type_definition_provider.is_some(),
            Self::References => true, // fundamental — always available if LSP is present
            Self::Implementation => caps.implementation_provider.is_some(),
            Self::Callers | Self::Deps => caps.call_hierarchy_provider.is_some(),
            // lsp-types 0.97 lacks `type_hierarchy_provider` — hidden until the
            // crate exposes it.  The -32601 fallback in hierarchy_query! ensures
            // graceful degradation if directories are force-resolved.
            Self::Supertypes | Self::Subtypes => false,
            Self::Doc => caps.hover_provider.is_some(),
            Self::Hints => caps.inlay_hint_provider.is_some(),
        }
    }

    /// Execute the LSP query for this feature and return results as
    /// an `LspQueryResult`. This is the **single dispatch point** —
    /// both markdown views and symlink directory population use it.
    pub(super) fn query(
        self,
        fq: &FileQuery<'_>,
        pos: Position,
        line_range: &StdRange<usize>,
    ) -> Result<LspQueryResult> {
        Ok(match self {
            Self::Definition => LspQueryResult::Locations(fq.definition(pos.line, pos.character)?),
            Self::Declaration => LspQueryResult::Locations(fq.declaration(pos.line, pos.character)?),
            Self::TypeDefinition => LspQueryResult::Locations(fq.type_definition(pos.line, pos.character)?),
            Self::References => LspQueryResult::Locations(fq.references(pos.line, pos.character)?),
            Self::Implementation => LspQueryResult::Locations(fq.implementations(pos.line, pos.character)?),
            Self::Callers => {
                let calls = fq.incoming_calls(pos.line, pos.character)?;
                LspQueryResult::HierarchyItems(calls.iter().map(|c| hierarchy_item(&c.from)).collect())
            }
            Self::Deps => {
                let calls = fq.outgoing_calls(pos.line, pos.character)?;
                LspQueryResult::HierarchyItems(calls.iter().map(|c| hierarchy_item(&c.to)).collect())
            }
            Self::Supertypes => {
                let items = fq.supertypes(pos.line, pos.character)?;
                LspQueryResult::HierarchyItems(items.iter().map(type_hierarchy_item).collect())
            }
            Self::Subtypes => {
                let items = fq.subtypes(pos.line, pos.character)?;
                LspQueryResult::HierarchyItems(items.iter().map(type_hierarchy_item).collect())
            }
            Self::Doc => {
                let hover = fq.hover(pos.line, pos.character)?;
                LspQueryResult::Hover(hover)
            }
            Self::Hints => {
                let start = u32::try_from(line_range.start).unwrap_or(u32::MAX);
                let end = u32::try_from(line_range.end).unwrap_or(u32::MAX);
                let range = Range {
                    start: Position {
                        line: start,
                        character: 0,
                    },
                    end: Position {
                        line: end,
                        character: u32::MAX,
                    },
                };
                LspQueryResult::InlayHints(fq.inlay_hints(range)?)
            }
        })
    }
}

/// Template handles for all per-symbol LSP features, indexed by
/// [`LspFeature::handle_index()`].
pub(in crate::providers::syntax) struct LspHandles {
    pub features: [TemplateHandle; LspFeature::COUNT],
    pub diagnostics: TemplateHandle,
}

/// A raw LSP result target before reverse-mapping to symbols.
pub(in crate::providers::syntax) struct LspTarget {
    /// Absolute file path from the LSP URI.
    pub abs_path: PathBuf,
    /// 0-based line number from the LSP result.
    pub line: u32,
    /// Optional symbol name (from call/type hierarchy items).
    pub name: Option<String>,
}
