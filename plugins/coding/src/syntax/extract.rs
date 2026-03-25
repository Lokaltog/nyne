//! Standard fragment extraction pipeline for trait-based language decomposers.

use super::fragment::{Fragment, FragmentKind};
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
///
/// Docstrings and decorators are created as child fragments rather than
/// metadata byte ranges, giving a uniform tree representation.
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
    let signature = L::build_signature(span_node, kind);
    let name_offset = node.name_start_byte().unwrap_or_else(|| span_node.start_byte());

    // Build metadata children: docstring, decorator.
    let mut children = Vec::new();
    let parent = Some(name.clone());

    children.extend(Fragment::docstring_child(L::extract_doc_range(node), parent.clone()));

    if let Some(range) = wrapper_info
        .decorator_range
        .clone()
        .or_else(|| L::extract_decorator_range(node))
    {
        children.push(Fragment::structural(
            "decorators",
            FragmentKind::Decorator,
            range,
            parent,
        ));
    }

    // Recurse into scoping constructs when depth allows.
    if remaining_depth > 1
        && L::RECURSABLE_KINDS.contains(&node.kind())
        && let Some(body) = node.field(L::BODY_FIELD)
    {
        children.extend(extract_fragments::<L>(body, remaining_depth - 1, Some(&name)));
    }

    Some(build_code_fragment(
        span_node,
        CodeFragmentSpec {
            name,
            kind,
            signature,
            name_byte_offset: name_offset,
            visibility,
            children,
        },
        parent_name,
    ))
}
