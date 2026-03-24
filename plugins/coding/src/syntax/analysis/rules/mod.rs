//! Analysis rules for detecting code smells and potential improvements.

/// Detect ASCII art separators in comments.
mod ascii_separator;
/// Detect boolean parameters that should be enums.
mod boolean_parameter;
/// Detect excessive code nesting depth.
mod deep_nesting;
/// Detect deep `super::` chains in `use` declarations.
mod deep_super_import;
/// Detect deeply nested generic types.
mod deeply_nested_type;
/// Detect deprecation markers in comments.
mod deprecation_marker;
/// Detect long else-if chains that could be match expressions.
mod else_if_chain;
/// Detect empty catch/rescue blocks.
mod empty_catch;
/// Detect traits with too many required methods.
mod fat_trait;
/// Detect structs with too many fields.
mod god_struct;
/// Detect array indexing inside loops.
mod index_in_loop;
/// Cross-language tree-sitter node kind constants shared across rules.
mod kinds;
/// Detect closures with overly large bodies.
mod large_closure;
/// Detect match expressions with too many arms.
mod long_match;
/// Detect functions with too many parameters.
mod long_parameter_list;
/// Detect magic numbers outside constant contexts.
mod magic_number;
/// Detect magic strings outside constant contexts.
mod magic_string;
/// Detect manual map patterns replaceable with combinators.
mod manual_map;
/// Detect negated conditions with else branches.
mod negated_condition;
/// Detect redundant `.clone()` and `.to_string()` calls.
mod redundant_clone;
/// Detect repeated field access chains that should use a local binding.
mod repeated_field_access;
/// Detect single-use variables that could be inlined.
mod single_use_variable;
/// Detect string concatenation inside loops.
mod string_concat_loop;
/// Detect `format!()` used only as a `push_str` argument.
mod string_format_push;
/// Detect match expressions dispatching on string literals.
mod stringly_typed_match;
/// Detect TODO and FIXME comments.
mod todo_fixme;
/// Detect functions with too many local variables.
mod too_many_locals;
/// Detect impl blocks with too many methods.
mod too_many_methods;
/// Detect type names encoded in variable names.
mod type_in_variable_name;
/// Detect unnecessary else blocks after early returns.
mod unnecessary_else;
/// Detect chained `.unwrap()` calls on method results.
mod unwrap_chain;
