//! Standard fragment extraction pipeline for trait-based language decomposers.

use super::fragment::Fragment;
use super::parser::{CodeFragmentSpec, TsNode, build_code_fragment};
use super::spec::{LanguageSpec, WrapperInfo};

/// The standard extraction loop for languages using the trait-based pipeline.
///
/// For each child of `root`:
/// 1. Try wrapper unwrapping (`LanguageSpec::unwrap_wrapper`)
/// 2. Try symbol kind mapping (`LanguageSpec::map_symbol_kind`)
/// 3. Try extra extractors (`LanguageSpec::extract_extra`)
pub(super) fn extract_fragments<L: LanguageSpec>(
    root: TsNode<'_>,
    remaining_depth: usize,
    parent_name: Option<&str>,
) -> Vec<Fragment> {
    let mut fragments = Vec::new();

    for child in root.children() {
        // 1. Try wrapper unwrapping.
        if let Some((inner, wrapper_info)) = L::unwrap_wrapper(child)
            && let Some(frag) =
                build_symbol_fragment::<L>(inner, Some(child), &wrapper_info, remaining_depth, parent_name)
        {
            fragments.push(frag);
            continue;
        }

        // 2. Try direct symbol mapping.
        if L::map_symbol_kind(child.kind()).is_some() {
            let wrapper_info = WrapperInfo::default();
            if let Some(frag) = build_symbol_fragment::<L>(child, None, &wrapper_info, remaining_depth, parent_name) {
                fragments.push(frag);
                continue;
            }
        }

        // 3. Try extra extractors.
        if let Some(frag) = L::extract_extra(child, remaining_depth, parent_name) {
            fragments.push(frag);
        }
    }

    fragments
}

/// Build a [`Fragment`] for a symbol node using the language spec methods.
fn build_symbol_fragment<L: LanguageSpec>(
    node: TsNode<'_>,
    wrapper: Option<TsNode<'_>>,
    wrapper_info: &WrapperInfo,
    remaining_depth: usize,
    parent_name: Option<&str>,
) -> Option<Fragment> {
    let kind = L::map_symbol_kind(node.kind())?;
    let name = L::extract_name(node, kind);
    let span_node = wrapper.unwrap_or(node);

    let visibility = wrapper_info.visibility.clone().or_else(|| L::extract_visibility(node));
    let doc_comment_range = L::extract_doc_range(node);
    let decorator_range = wrapper_info
        .decorator_range
        .clone()
        .or_else(|| L::extract_decorator_range(node));
    let signature = L::build_signature(span_node, kind);
    let name_offset = node.name_start_byte().unwrap_or_else(|| span_node.start_byte());
    let full_span = L::full_symbol_range(
        &span_node.byte_range(),
        doc_comment_range.as_ref(),
        decorator_range.as_ref(),
    );

    // Recurse into scoping constructs when depth allows.
    let children = if remaining_depth > 1 && L::RECURSABLE_KINDS.contains(&node.kind()) {
        node.field(L::BODY_FIELD).map_or_else(Vec::new, |body| {
            extract_fragments::<L>(body, remaining_depth - 1, Some(&name))
        })
    } else {
        Vec::new()
    };

    Some(build_code_fragment(
        span_node,
        CodeFragmentSpec {
            name,
            kind,
            signature,
            name_byte_offset: name_offset,
            visibility,
            doc_comment_range,
            decorator_range,
            full_span,
            children,
        },
        parent_name,
    ))
}
