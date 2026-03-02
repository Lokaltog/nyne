//! Prelude of common imports for language decomposer implementations.

pub(super) use crate::syntax::fragment::{Fragment, FragmentKind, FragmentMetadata, SymbolKind};
pub(super) use crate::syntax::fs_mapping::{ConflictStrategy, NamingStrategy};
pub(super) use crate::syntax::parser::{CodeFragmentSpec, TsNode, build_code_fragment};
pub(super) use crate::syntax::spec::{
    LanguageSpec, SpliceMode, WrapperInfo, extract_child_visibility, extract_preceding_decorator_range,
    extract_preceding_doc_range, strip_line_comment_prefixes, wrap_line_doc_comment,
};
pub(super) use crate::syntax::{register_syntax, symbol_map};
