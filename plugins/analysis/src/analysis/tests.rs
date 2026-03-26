use std::collections::HashSet;
use std::path::PathBuf;

use nyne_source::syntax::fragment::DEFAULT_MAX_DEPTH;
use nyne_source::test_support::registry;
use rstest::rstest;

use super::{AnalysisEngine, DEFAULT_DISABLED_RULES, Hint};
use crate::config::AnalysisConfig;

/// Load a test fixture file relative to this crate's `src/analysis/fixtures/`.
fn load_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/analysis/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to load fixture {}: {e}", path.display()))
}

/// Analyze a fixture file and return all hints.
fn analyze_fixture(ext: &str, name: &str) -> Vec<Hint> {
    let source = load_fixture(name);
    analyze_source(ext, &source)
}

/// Analyze inline source code and return all hints.
fn analyze_source(ext: &str, source: &str) -> Vec<Hint> {
    let reg = registry();
    let decomposer = reg.get(ext).expect("no decomposer for extension");
    let (_file, tree) = decomposer.decompose(source, DEFAULT_MAX_DEPTH);
    let tree = tree.expect("parse should succeed");

    let engine = AnalysisEngine::build();

    engine.analyze(&tree, source)
}

/// Filter hints to only those matching a specific rule ID.
fn hints_by_rule<'a>(hints: &'a [Hint], rule_id: &str) -> Vec<&'a Hint> {
    hints.iter().filter(|h| h.rule_id == rule_id).collect()
}

/// Verifies that clean code fixtures produce no analysis hints.
#[rstest]
#[case("rs", "clean.rs")]
#[case("rs", "shallow-nesting.rs")]
#[case("rs", "short-params.rs")]
#[case("rs", "necessary-else.rs")]
#[case("rs", "few-locals.rs")]
#[case("rs", "no-string-concat-loop.rs")]
fn clean_code_produces_no_hints(#[case] ext: &str, #[case] fixture: &str) {
    let hints = analyze_fixture(ext, fixture);
    assert!(hints.is_empty(), "expected no hints for {fixture}");
}

/// Verifies that empty source produces no analysis hints.
#[test]
fn empty_source_produces_no_hints() {
    let hints = analyze_source("rs", "");
    assert!(hints.is_empty());
}

/// Verifies that each analysis rule triggers on its corresponding fixture.
#[rstest]
#[case("rs", "long-params.rs", "long-parameter-list", 1)]
#[case("rs", "unnecessary-else.rs", "unnecessary-else", 1)]
#[case("rs", "else-if-chain.rs", "else-if-chain", 1)]
#[case("rs", "too-many-locals.rs", "too-many-locals", 1)]
#[case("rs", "string-concat-loop.rs", "string-concat-loop", 1)]
#[case("rs", "repeated-field-access.rs", "repeated-field-access", 1)]
#[case("ts", "empty-catch.js", "empty-catch", 1)]
#[case("rs", "negated-condition.rs", "negated-condition", 1)]
#[case("rs", "single-use-variable.rs", "single-use-variable", 1)]
#[case("rs", "redundant-clone.rs", "redundant-clone", 1)]
#[case("rs", "boolean-parameter.rs", "boolean-parameter", 1)]
#[case("rs", "type-in-variable-name.rs", "type-in-variable-name", 1)]
#[case("rs", "unwrap-chain.rs", "unwrap-chain", 1)]
#[case("rs", "manual-map.rs", "manual-map", 1)]
#[case("rs", "index-in-loop.rs", "index-in-loop", 1)]
#[case("rs", "string-format-push.rs", "string-format-push", 1)]
#[case("rs", "god-struct.rs", "god-struct", 1)]
#[case("rs", "long-match.rs", "long-match", 1)]
#[case("rs", "deeply-nested-type.rs", "deeply-nested-type", 1)]
#[case("rs", "too-many-methods.rs", "too-many-methods", 1)]
#[case("rs", "magic-number.rs", "magic-number", 1)]
#[case("rs", "magic-string.rs", "magic-string", 1)]
#[case("rs", "deprecation-marker.rs", "deprecation-marker", 2)]
#[case("rs", "deep-super-import.rs", "deep-super-import", 2)]
#[case("rs", "stringly-typed-match.rs", "stringly-typed-match", 1)]
#[case("rs", "large-closure.rs", "large-closure", 1)]
#[case("rs", "fat-trait.rs", "fat-trait", 1)]
fn rule_triggers(#[case] ext: &str, #[case] fixture: &str, #[case] rule_id: &str, #[case] min_count: usize) {
    let hints = analyze_fixture(ext, fixture);
    let matched = hints_by_rule(&hints, rule_id);
    assert!(
        matched.len() >= min_count,
        "expected at least {min_count} `{rule_id}` hint(s) in {fixture}, got {}",
        matched.len(),
    );
}

/// Verifies that each analysis rule does not trigger on its negative fixture.
#[rstest]
#[case("rs", "no-repeated-field-access.rs", "repeated-field-access")]
#[case("ts", "no-empty-catch.js", "empty-catch")]
#[case("rs", "no-negated-condition.rs", "negated-condition")]
#[case("rs", "no-single-use-variable.rs", "single-use-variable")]
#[case("rs", "no-redundant-clone.rs", "redundant-clone")]
#[case("rs", "no-boolean-parameter.rs", "boolean-parameter")]
#[case("rs", "no-type-in-variable-name.rs", "type-in-variable-name")]
#[case("rs", "no-unwrap-chain.rs", "unwrap-chain")]
#[case("rs", "no-manual-map.rs", "manual-map")]
#[case("rs", "no-index-in-loop.rs", "index-in-loop")]
#[case("rs", "no-string-format-push.rs", "string-format-push")]
#[case("rs", "no-god-struct.rs", "god-struct")]
#[case("rs", "no-long-match.rs", "long-match")]
#[case("rs", "no-deeply-nested-type.rs", "deeply-nested-type")]
#[case("rs", "no-too-many-methods.rs", "too-many-methods")]
#[case("rs", "no-deprecation-marker.rs", "deprecation-marker")]
#[case("rs", "no-magic-number.rs", "magic-number")]
#[case("rs", "no-magic-string.rs", "magic-string")]
#[case("rs", "no-deep-super-import.rs", "deep-super-import")]
#[case("rs", "no-stringly-typed-match.rs", "stringly-typed-match")]
#[case("rs", "no-large-closure.rs", "large-closure")]
#[case("rs", "no-fat-trait.rs", "fat-trait")]
#[case("rs", "shallow-nesting.rs", "deep-nesting")]
#[case("rs", "short-params.rs", "long-parameter-list")]
#[case("rs", "necessary-else.rs", "unnecessary-else")]
#[case("rs", "short-else-if.rs", "else-if-chain")]
#[case("rs", "few-locals.rs", "too-many-locals")]
#[case("rs", "no-string-concat-loop.rs", "string-concat-loop")]
#[case("rs", "clean.rs", "todo-fixme")]
fn rule_does_not_trigger(#[case] ext: &str, #[case] fixture: &str, #[case] rule_id: &str) {
    let hints = analyze_fixture(ext, fixture);
    assert!(
        hints_by_rule(&hints, rule_id).is_empty(),
        "unexpected `{rule_id}` hint in {fixture}",
    );
}

/// Verifies that the deep-nesting rule triggers across multiple languages.
#[rstest]
#[case("rs", "deep-nesting.rs")]
#[case("py", "deep-nesting.py")]
fn deep_nesting_triggers(#[case] ext: &str, #[case] fixture: &str) {
    let hints = analyze_fixture(ext, fixture);
    let matched = hints_by_rule(&hints, "deep-nesting");
    assert!(!matched.is_empty(), "expected deep-nesting hint in {fixture}");
    assert!(!matched[0].suggestions.is_empty(), "should provide suggestions");
}

/// Verifies that TODO/FIXME markers in comments are detected.
#[rstest]
#[case("TODO", "todo-comment.rs")]
fn todo_markers_detected(#[case] marker: &str, #[case] fixture: &str) {
    let hints = analyze_fixture("rs", fixture);
    let matched = hints_by_rule(&hints, "todo-fixme");
    assert!(!matched.is_empty(), "expected todo-fixme hint for {marker}");
    assert!(matched[0].message.contains(marker), "hint should mention {marker}");
}

/// Verifies that prose mentions of TODO without a colon do not trigger hints.
#[test]
fn todo_in_prose_no_hint() {
    let hints = analyze_fixture("rs", "no-todo-in-prose.rs");
    assert!(
        hints_by_rule(&hints, "todo-fixme").is_empty(),
        "prose mentions of TODO/FIXME without a colon should not trigger",
    );
}

/// Verifies that a detected hint has the correct zero-based line range.
#[test]
fn hint_has_correct_line_range() {
    let hints = analyze_fixture("rs", "todo-comment.rs");
    let matched = hints_by_rule(&hints, "todo-fixme");
    assert_eq!(matched.len(), 1);
    // 0-based: "// TODO" is on line 1 in the fixture
    assert_eq!(matched[0].line_range.start, 1);
}

/// Verifies that ASCII separator comments are detected across languages.
#[rstest]
#[case("rs", "separator-pure.rs")]
#[case("rs", "separator-header.rs")]
#[case("rs", "separator-equals.rs")]
#[case("rs", "separator-unicode.rs")]
#[case("py", "separator-python.py")]
#[case("rs", "no-todo-in-prose.rs")]
fn separator_detected(#[case] ext: &str, #[case] fixture: &str) {
    let hints = analyze_fixture(ext, fixture);
    let matched = hints_by_rule(&hints, "ascii-separator");
    assert_eq!(matched.len(), 1, "expected ascii-separator in {fixture}");
}

/// Verifies that short or absent separators do not trigger the ascii-separator rule.
#[rstest]
#[case("rs", "separator-short.rs")]
#[case("rs", "clean.rs")]
fn separator_not_triggered(#[case] ext: &str, #[case] fixture: &str) {
    let hints = analyze_fixture(ext, fixture);
    assert!(
        hints_by_rule(&hints, "ascii-separator").is_empty(),
        "should not trigger in {fixture}",
    );
}

/// Recursively collect all tree-sitter node kinds from a syntax tree.
fn collect_kinds(node: tree_sitter::Node<'_>, kinds: &mut std::collections::BTreeSet<String>) {
    kinds.insert(node.kind().to_string());
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_kinds(child, kinds);
    }
}

/// Debug helper that dumps all tree-sitter node kinds for a set of fixtures.
#[test]
fn debug_dump_tree_kinds() {
    let fixtures = &[
        ("rs", "too-many-methods.rs"),
        ("rs", "repeated-field-access.rs"),
        ("rs", "manual-map.rs"),
        ("rs", "long-match.rs"),
        ("rs", "index-in-loop.rs"),
        ("ts", "empty-catch.js"),
        ("rs", "no-deeply-nested-type.rs"),
    ];
    for (ext, name) in fixtures {
        let source = load_fixture(name);
        let reg = registry();
        let decomposer = reg.get(ext).expect("no decomposer");
        let (_file, tree) = decomposer.decompose(&source, DEFAULT_MAX_DEPTH);
        let tree = tree.expect("parse should succeed");

        let mut kinds = std::collections::BTreeSet::new();
        collect_kinds(tree.root_node(), &mut kinds);
        tracing::debug!("\n=== {name} ===");
        for kind in &kinds {
            tracing::debug!("  {kind}");
        }
    }
}

/// Analyze inline source with a filtered AnalysisConfig and return all hints.
fn analyze_source_filtered(ext: &str, source: &str, config: &AnalysisConfig) -> Vec<Hint> {
    let reg = registry();
    let decomposer = reg.get(ext).expect("no decomposer for extension");
    let (_file, tree) = decomposer.decompose(source, DEFAULT_MAX_DEPTH);
    let tree = tree.expect("parse should succeed");

    let engine = AnalysisEngine::build_filtered(config);

    engine.analyze(&tree, source)
}

/// Verifies that default config excludes rules listed in DEFAULT_DISABLED_RULES.
#[test]
fn default_config_excludes_noisy_rules() {
    let config = AnalysisConfig::default();
    let engine = AnalysisEngine::build_filtered(&config);

    // Default-disabled rules should not appear in dispatch.
    for rule_id in DEFAULT_DISABLED_RULES {
        let all_ids: Vec<&str> = engine
            .dispatch
            .values()
            .flatten()
            .chain(&engine.catch_all)
            .map(|r| r.id())
            .collect();
        assert!(!all_ids.contains(rule_id), "{rule_id} should be excluded by default",);
    }
}

/// Verifies that an explicit empty rules set runs all rules including disabled ones.
#[test]
fn explicit_empty_rules_runs_all() {
    let config = AnalysisConfig {
        enabled: true,
        rules: Some(HashSet::new()),
    };
    let all = AnalysisEngine::build();
    let filtered = AnalysisEngine::build_filtered(&config);

    assert_eq!(all.dispatch.len(), filtered.dispatch.len());
    assert_eq!(all.catch_all.len(), filtered.catch_all.len());
}

/// Verifies that a filtered config keeps only the named rules.
#[test]
fn filtered_keeps_only_named_rules() {
    let config = AnalysisConfig {
        enabled: true,
        rules: Some(HashSet::from(["deep-nesting".into()])),
    };
    let source = load_fixture("deep-nesting.rs");
    let hints = analyze_source_filtered("rs", &source, &config);

    assert!(!hints.is_empty(), "deep-nesting rule should fire");
    assert!(
        hints.iter().all(|h| h.rule_id == "deep-nesting"),
        "only the selected rule should produce hints",
    );
}

/// Verifies that a filtered config excludes rules not in the selected set.
#[test]
fn filtered_excludes_unselected_rules() {
    let config = AnalysisConfig {
        enabled: true,
        rules: Some(HashSet::from(["deep-nesting".into()])),
    };
    let source = load_fixture("magic-number.rs");
    let hints = analyze_source_filtered("rs", &source, &config);

    assert!(
        hints.iter().all(|h| h.rule_id != "magic-number"),
        "unselected rule should not produce hints",
    );
}

/// Verifies that a disabled analysis engine produces no hints.
#[test]
fn disabled_produces_no_hints() {
    let config = AnalysisConfig {
        enabled: false,
        rules: None,
    };
    let source = load_fixture("deep-nesting.rs");
    let hints = analyze_source_filtered("rs", &source, &config);

    assert!(hints.is_empty(), "disabled engine should produce no hints");
}
