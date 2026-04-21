use super::*;

#[derive(Debug)]
struct Item {
    keys: Vec<&'static str>,
    deps: Vec<&'static str>,
}

fn run(items: &[Item]) -> Result<Toposort, Cycle> {
    sort(items, |i| i.keys.clone(), |i| i.deps.clone())
}

fn item(keys: &[&'static str], deps: &[&'static str]) -> Item {
    Item { keys: keys.to_vec(), deps: deps.to_vec() }
}

#[test]
fn empty_input_produces_empty_output() {
    let out = run(&[]).expect("empty graph toposorts");
    assert!(out.order.is_empty());
    assert!(out.depths.is_empty());
}

#[test]
fn single_key_deps_sort_in_dependency_order() {
    let items = [item(&["a"], &[]), item(&["b"], &["a"]), item(&["c"], &["b"])];
    let out = run(&items).expect("dag sorts");
    assert_eq!(out.order, vec![0, 1, 2]);
    assert_eq!(out.depths, vec![0, 1, 2]);
}

#[test]
fn missing_dep_keys_are_soft_skipped() {
    let items = [item(&["a"], &["ghost"]), item(&["b"], &["a"])];
    let out = run(&items).expect("missing deps don't fail");
    assert_eq!(out.order, vec![0, 1]);
}

#[test]
fn multi_key_items_support_shared_ownership() {
    // Item 0 exports two keys (a, b); item 1 depends on b.
    let items = [item(&["a", "b"], &[]), item(&["c"], &["b"])];
    let out = run(&items).expect("multi-key sorts");
    assert_eq!(out.order, vec![0, 1]);
}

#[test]
fn self_dep_via_sibling_key_filtered_not_cycle() {
    // Item 0 owns keys {a, b} and declares a dep on "b" (its own sibling).
    // This must not trip the cycle detector.
    let items = [item(&["a", "b"], &["b"])];
    let out = run(&items).expect("self-sibling dep is filtered");
    assert_eq!(out.order, vec![0]);
}

#[test]
fn cycle_reports_participating_item() {
    let items = [item(&["a"], &["b"]), item(&["b"], &["a"])];
    let err = run(&items).expect_err("cycle detected");
    assert!(err.cycle_item < 2);
}

#[test]
fn depths_track_longest_path() {
    //        a
    //       / \
    //      b   c
    //       \ /
    //        d
    // depth: a=0, b=1, c=1, d=2
    let items = [
        item(&["a"], &[]),
        item(&["b"], &["a"]),
        item(&["c"], &["a"]),
        item(&["d"], &["b", "c"]),
    ];
    let out = run(&items).expect("diamond sorts");
    assert_eq!(out.depths[0], 0);
    assert_eq!(out.depths[1], 1);
    assert_eq!(out.depths[2], 1);
    assert_eq!(out.depths[3], 2);
}
