# Tree-sitter syntax provider

## Tree-sitter Byte Ranges

- Tree-sitter node ranges for line-based constructs (Rust `line_comment`, attributes) **include the trailing newline**. Raw `node.end_byte()` points past the `\n`, not at the last content byte.
- `merge_preceding_sibling_ranges` and `collect_import_span` in `syntax/parser.rs` trim trailing newlines from merged ranges. Any new range-merging code must do the same — otherwise splicing at the range will eat the `\n` separator between the collected content and the following symbol.
- `wrap_doc_comment` / `strip_doc_comment` use `.lines()` which swallows trailing newlines — they are NOT responsible for preserving the separator. The range must be correct.
- When debugging byte-range issues, always write Rust tests that exercise the actual tree-sitter parse — never simulate with Python/shell scripts, which can't reproduce grammar-specific node boundary behavior.
