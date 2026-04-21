use rstest::rstest;

use super::*;

#[derive(Debug, Clone)]
struct Item {
    keys: Vec<&'static str>,
    deps: Vec<&'static str>,
}

fn item(keys: &[&'static str], deps: &[&'static str]) -> Item {
    Item {
        keys: keys.to_vec(),
        deps: deps.to_vec(),
    }
}

fn run(items: &[Item]) -> Result<Toposort, Cycle> { sort(items, |i| i.keys.clone(), |i| i.deps.clone()) }

/// Order-assertion cases: scenarios where toposort produces a unique,
/// deterministic order and we can pin down both `order` and `depths`.
#[rstest]
#[case::empty(vec![], vec![], vec![])]
#[case::linear(
    vec![item(&["a"], &[]), item(&["b"], &["a"]), item(&["c"], &["b"])],
    vec![0, 1, 2],
    vec![0, 1, 2],
)]
#[case::soft_missing_dep(
    // "ghost" is never emitted as a key → dependency is silently dropped.
    vec![item(&["a"], &["ghost"]), item(&["b"], &["a"])],
    vec![0, 1],
    vec![0, 1],
)]
#[case::multi_key_shared_ownership(
    // Item 0 emits two keys (a, b); item 1 depends on b.
    vec![item(&["a", "b"], &[]), item(&["c"], &["b"])],
    vec![0, 1],
    vec![0, 1],
)]
#[case::self_sibling_filtered(
    // Item 0 owns {a, b} and declares a self-sibling dep ("b") — must
    // be filtered, not flagged as a cycle.
    vec![item(&["a", "b"], &["b"])],
    vec![0],
    vec![0],
)]
#[case::diamond(
    //        a
    //       / \
    //      b   c
    //       \ /
    //        d
    //
    // petgraph's Kahn's-algorithm toposort pops in-degree-0 nodes in
    // reverse-insertion order, so a -> c -> b -> d is what lands here.
    // We don't rely on that ordering contractually — the `depths` assertion
    // below is the invariant; `order` is pinned here as a regression
    // guard against petgraph's behaviour drifting.
    vec![
        item(&["a"], &[]),
        item(&["b"], &["a"]),
        item(&["c"], &["a"]),
        item(&["d"], &["b", "c"]),
    ],
    vec![0, 2, 1, 3],
    vec![0, 1, 1, 2],
)]
fn sort_produces_expected_order_and_depths(
    #[case] items: Vec<Item>,
    #[case] expected_order: Vec<usize>,
    #[case] expected_depths: Vec<usize>,
) {
    let out = run(&items).expect("expected dag to sort");
    assert_eq!(out.order, expected_order, "order mismatch");
    assert_eq!(out.depths, expected_depths, "depths mismatch");
}

/// Cycle-detection cases: expect `Err(Cycle)` and that the reported
/// `cycle_item` points into the input slice.
#[rstest]
#[case::two_node_cycle(
    vec![item(&["a"], &["b"]), item(&["b"], &["a"])],
    2,
)]
#[case::three_node_cycle(
    vec![item(&["a"], &["c"]), item(&["b"], &["a"]), item(&["c"], &["b"])],
    3,
)]
fn cycle_reports_item_in_bounds(#[case] items: Vec<Item>, #[case] item_count: usize) {
    let err = run(&items).expect_err("expected cycle");
    assert!(
        err.cycle_item < item_count,
        "cycle_item {} out of bounds (items: {})",
        err.cycle_item,
        item_count
    );
}
