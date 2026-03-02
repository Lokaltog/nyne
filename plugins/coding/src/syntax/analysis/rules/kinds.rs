//! Cross-language tree-sitter node kind constants shared across analysis rules.
//!
//! Each constant groups node kinds by semantic role. Tree-sitter reuses kind
//! strings across grammars (e.g. `"parameters"` in both Rust and Python), so
//! these lists are already deduplicated where possible.

use std::str::from_utf8;

/// Comment node kinds.
pub(super) const COMMENT: &[&str] = &["comment", "line_comment", "block_comment"];

/// If-expression/statement node kinds.
pub(super) const IF: &[&str] = &[
    "if_expression", // Rust
    "if_statement",  // Python, TypeScript, JavaScript
];

/// Function definition node kinds.
pub(super) const FUNCTION: &[&str] = &[
    "function_item",        // Rust
    "function_definition",  // Python
    "function_declaration", // JavaScript, TypeScript
    "method_definition",    // JavaScript, TypeScript
    "arrow_function",       // JavaScript, TypeScript
    "closure_expression",   // Rust
];

/// Loop node kinds.
pub(super) const LOOP: &[&str] = &[
    "for_expression",   // Rust
    "loop_expression",  // Rust
    "while_expression", // Rust
    "for_statement",    // Python, JS, TS
    "for_in_statement", // JS, TS
    "while_statement",  // Python, JS, TS
    "do_statement",     // JS, TS
];

/// Constant/static declaration kinds where literals are expected (not magic).
pub(super) const CONST_DECLARATION: &[&str] = &["const_item", "static_item", "const_declaration", "enum_variant"];

/// Local variable binding kinds.
pub(super) const BINDING: &[&str] = &[
    "let_declaration",      // Rust
    "let_condition",        // Rust (if let / while let)
    "variable_declaration", // JavaScript, TypeScript
    "lexical_declaration",  // JavaScript, TypeScript (let/const)
    "assignment",           // Python (first assignment = declaration)
];

/// Node kinds that represent early exits (return/continue/break/throw).
pub(super) const EXIT: &[&str] = &[
    "return_statement",
    "return_expression",
    "continue_statement",
    "continue_expression",
    "break_statement",
    "break_expression",
    "throw_statement",
    "throw_expression",
    "raise_statement", // Python
];

/// String literal node kinds.
pub(super) const STRING: &[&str] = &[
    "string_literal", // Rust
    "raw_string_literal",
    "string", // JavaScript, TypeScript, Python
    "template_string",
    "string_fragment",
];

/// Exception-handler / catch block kinds.
pub(super) const CATCH: &[&str] = &[
    "catch_clause",  // JavaScript, TypeScript
    "except_clause", // Python
    "rescue",        // Ruby
];

/// Match/switch expression node kinds.
pub(super) const MATCH: &[&str] = &[
    "match_expression",  // Rust
    "switch_statement",  // JavaScript, TypeScript
    "switch_expression", // Java, C#
    "match_statement",   // Python
];

/// Struct/class definition node kinds.
pub(super) const STRUCT_DEF: &[&str] = &[
    "struct_item",       // Rust
    "class_definition",  // Python
    "class_declaration", // JavaScript, TypeScript
];

/// Impl/class-body block node kinds.
pub(super) const IMPL_BLOCK: &[&str] = &[
    "impl_item",        // Rust
    "class_body",       // JavaScript, TypeScript
    "class_definition", // Python (methods are direct children)
];

/// Unary negation operator kinds.
pub(super) const UNARY_NOT: &[&str] = &[
    "unary_expression", // Rust, JavaScript, TypeScript
    "not_operator",     // Python
];

/// Call expression node kinds.
pub(super) const CALL: &[&str] = &[
    "call_expression", // Rust, JavaScript, TypeScript
    "call",            // Python
];

/// Type annotation node kinds.
pub(super) const TYPE_ANNOTATION: &[&str] = &[
    "type_identifier", // Rust, TypeScript
    "primitive_type",  // Rust
    "generic_type",    // Rust, TypeScript
    "predefined_type", // TypeScript
];

/// Field/member access expression kinds.
pub(super) const FIELD_ACCESS: &[&str] = &[
    "field_expression",  // Rust
    "member_expression", // JavaScript, TypeScript
];

/// Identifier node kind.
pub(super) const IDENTIFIER: &str = "identifier";

/// Expression statement wrapper kind.
pub(super) const EXPRESSION_STATEMENT: &str = "expression_statement";

/// Block node kinds.
pub(super) const BLOCK: &[&str] = &[
    "block",           // Rust, Python
    "statement_block", // JavaScript, TypeScript
];

/// Macro invocation node kinds.
pub(super) const MACRO_INVOCATION: &[&str] = &[
    "macro_invocation", // Rust
];

/// Boolean type names across languages.
pub(super) const BOOL_TYPES: &[&str] = &["bool", "boolean", "Boolean"];

/// Index expression node kinds.
pub(super) const INDEX_EXPRESSION: &[&str] = &[
    "index_expression",     // Rust
    "subscript_expression", // JavaScript, TypeScript, Python
];

/// Match arm / switch case node kinds.
pub(super) const MATCH_ARM: &[&str] = &[
    "match_arm",   // Rust
    "switch_case", // JavaScript, TypeScript
    "case_clause", // JavaScript, TypeScript (alternate grammar)
    "case",        // Python match
];

/// Generic / parameterized type node kinds that count as a nesting level.
pub(super) const GENERIC_TYPE: &[&str] = &[
    "generic_type",       // Rust, TypeScript
    "parameterized_type", // Java
];

/// Container nodes for generic type arguments (traversed but don't count as nesting).
pub(super) const GENERIC_TYPE_ARGS: &[&str] = &[
    "type_arguments",   // Rust, TypeScript
    "generic_argument", // Rust (turbofish)
];

/// Struct/class field declaration node kinds.
pub(super) const FIELD_DECLARATION: &[&str] = &[
    "field_declaration",       // Rust
    "field_definition",        // Rust (alternate)
    "public_field_definition", // TypeScript
    "property_definition",     // JavaScript
    "expression_statement",    // Python class body (assignments)
];

/// Extract the byte span of a tree-sitter node from source.
///
/// Returns an empty slice if the byte range is out of bounds (structurally
/// impossible for a valid parse tree, but satisfies `clippy::indexing_slicing`).
pub(super) fn node_bytes<'a>(node: &tree_sitter::Node<'_>, source: &'a [u8]) -> &'a [u8] {
    source.get(node.start_byte()..node.end_byte()).unwrap_or_default()
}

/// Extract the UTF-8 text of a tree-sitter node from source.
///
/// Returns `None` if the byte range is invalid UTF-8 (or out of bounds).
pub(super) fn node_str<'a>(node: &tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    from_utf8(node_bytes(node, source)).ok()
}

/// Count how many times `name` appears as an identifier in the subtree.
pub(super) fn count_identifier_uses(node: &tree_sitter::Node<'_>, name: &[u8], source: &[u8]) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    count_identifier_recursive(&mut cursor, name, source, &mut count);
    count
}

fn count_identifier_recursive(cursor: &mut tree_sitter::TreeCursor<'_>, name: &[u8], source: &[u8], count: &mut usize) {
    let node = cursor.node();
    if node.kind() == IDENTIFIER && node_bytes(&node, source) == name {
        *count += 1;
    }
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        count_identifier_recursive(cursor, name, source, count);
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    cursor.goto_parent();
}

/// Common comment prefixes across languages, ordered longest-first for
/// greedy matching.
const COMMENT_PREFIXES: &[&str] = &[
    "///", "//!", "//", "##", "#!", "#", "--", ";;", ";", "%%", "%", "/*", "*/", "*",
];

/// Strip common comment prefixes from a line to get the content portion.
///
/// Shared across analysis rules that inspect comment text. For language-specific
/// doc comment stripping (with unwrapping), use `Decomposer::strip_doc_comment`.
pub(super) fn strip_comment_prefix(line: &str) -> &str {
    let trimmed = line.trim();
    for prefix in COMMENT_PREFIXES {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim();
        }
    }
    trimmed
}

/// Check if a node or any ancestor up to a function/block boundary is a
/// "safe" context for a literal (const, static, enum variant, etc.).
///
/// `extra_safe` allows rules to extend the set with rule-specific parent kinds.
pub(super) fn is_safe_literal_context(node: tree_sitter::Node<'_>, extra_safe: &[&str]) -> bool {
    let mut current = node;
    loop {
        let kind = current.kind();
        if CONST_DECLARATION.contains(&kind) || extra_safe.contains(&kind) {
            return true;
        }
        // Stop at function/block boundaries.
        if FUNCTION.contains(&kind) || kind == "block" || kind == "source_file" {
            return false;
        }
        match current.parent() {
            Some(p) => current = p,
            None => return false,
        }
    }
}
