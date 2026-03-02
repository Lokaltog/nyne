// Core LSP feature dispatch — LspFeature enum, LspHandles, LspTarget.

use std::ops::Range as StdRange;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use lsp_types::{Position, Range};
use nyne::templates::TemplateHandle;
use strum::{EnumCount, IntoEnumIterator};

use super::views::{LspQueryResult, hierarchy_item, type_hierarchy_item};
use crate::lsp::query::FileQuery;
use crate::providers::names;

/// Single source of truth for all per-symbol LSP features.
///
/// Every LSP feature — its file name, directory name, template key,
/// query dispatch, view construction, and symlink target generation —
/// is derived from this enum. Adding a new feature means adding a
/// variant here plus a template file.
///
/// Variant order is significant: `handle_index()` derives from the
/// discriminant, and the template registration array in
/// `SyntaxProvider::new()` must match.
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

impl LspFeature {
    /// Single metadata table: (`file_name`, `dir_name`) per variant.
    ///
    /// File/dir name constants live in `names` — this match is the only
    /// place that maps variant → constant pair.
    const fn metadata(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::Definition => (names::FILE_DEFINITION, Some(names::DIR_DEFINITION)),
            Self::Declaration => (names::FILE_DECLARATION, Some(names::DIR_DECLARATION)),
            Self::TypeDefinition => (names::FILE_TYPE_DEFINITION, Some(names::DIR_TYPE_DEFINITION)),
            Self::References => (names::FILE_REFERENCES, Some(names::DIR_REFERENCES)),
            Self::Implementation => (names::FILE_IMPLEMENTATION, Some(names::DIR_IMPLEMENTATION)),
            Self::Callers => (names::FILE_CALLERS, Some(names::DIR_CALLERS)),
            Self::Deps => (names::FILE_DEPS, Some(names::DIR_DEPS)),
            Self::Supertypes => (names::FILE_SUPERTYPES, Some(names::DIR_SUPERTYPES)),
            Self::Subtypes => (names::FILE_SUBTYPES, Some(names::DIR_SUBTYPES)),
            Self::Doc => (names::FILE_DOC, None),
            Self::Hints => (names::FILE_HINTS, None),
        }
    }

    pub(super) const fn file_name(self) -> &'static str { self.metadata().0 }

    pub(super) const fn dir_name(self) -> Option<&'static str> { self.metadata().1 }

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
