//! Generic dependency-graph topological sort.
//!
//! Shared between the provider middleware chain and the plugin lifecycle —
//! both need to order items by declared dependencies with soft-dep semantics
//! (missing deps are skipped) and deterministic cycle reporting.
//!
//! Items may expose multiple keys (e.g. a plugin emits several providers,
//! each with its own id). Dependencies are resolved against the full
//! key set, but edges between keys that resolve to the same item are
//! filtered so an item depending on its own siblings doesn't self-cycle.

use std::collections::HashMap;
use std::hash::Hash;

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};

/// Result of a successful topological sort.
#[derive(Debug, Clone)]
pub struct Toposort {
    /// Item indices in topological order: each entry is an index into the
    /// original `items` slice.
    pub order: Vec<usize>,
    /// Depth (longest path from any root) per item, indexed by the original
    /// item position. Callers that need a stable secondary sort can combine
    /// this with per-item priority/id tiebreakers.
    pub depths: Vec<usize>,
}

/// A dependency cycle was detected. `cycle_item` is the index of one item
/// participating in the cycle — callers format the error in their own style.
#[derive(Debug, Clone, Copy)]
pub struct Cycle {
    pub cycle_item: usize,
}

/// Topologically sort `items` by their declared dependencies.
///
/// - `keys_of(item)` returns every key identifying the item (most callers
///   return one; plugins exposing multiple providers return several).
/// - `deps_of(item)` returns the keys this item depends on. Missing keys
///   are treated as soft dependencies and skipped — this allows partial
///   graphs (e.g. an LSP plugin loaded without its optional analysis
///   sibling).
///
/// Both closures return `Vec<K>` rather than an iterator because most
/// callers need to materialise the keys from a `&T`-borrowing chain,
/// and expressing the higher-ranked lifetime on a generic `Fn(&T) -> impl
/// Iterator + '_` is not possible with stable traits.
///
/// Returns items in topological order along with their graph depth for
/// stable secondary ordering. A [`Cycle`] is returned if toposort fails.
pub fn sort<T, K, FK, FD>(items: &[T], keys_of: FK, deps_of: FD) -> Result<Toposort, Cycle>
where
    K: Eq + Hash,
    FK: Fn(&T) -> Vec<K>,
    FD: Fn(&T) -> Vec<K>,
{
    let mut graph = DiGraph::<usize, ()>::new();
    let nodes: Vec<NodeIndex> = (0..items.len()).map(|i| graph.add_node(i)).collect();

    // Every key -> owning item's graph node.
    let mut key_to_node: HashMap<K, NodeIndex> = HashMap::new();
    for (item, &node) in items.iter().zip(&nodes) {
        for key in keys_of(item) {
            key_to_node.insert(key, node);
        }
    }

    // Edges: dependency -> dependent. Self-edges (same node on both ends)
    // are filtered so an item depending on a sibling key it owns doesn't
    // produce a degenerate cycle.
    for (item, &self_node) in items.iter().zip(&nodes) {
        for dep in deps_of(item) {
            if let Some(&dep_node) = key_to_node.get(&dep)
                && dep_node != self_node
            {
                graph.add_edge(dep_node, self_node, ());
            }
        }
    }

    let sorted = toposort(&graph, None).map_err(|cycle| Cycle {
        cycle_item: graph[cycle.node_id()],
    })?;

    Ok(Toposort {
        order: sorted.iter().map(|&n| graph[n]).collect(),
        depths: compute_depths(&graph, &sorted, items.len()),
    })
}

/// Depth (longest path from any root) for each item position.
fn compute_depths(graph: &DiGraph<usize, ()>, topo_order: &[NodeIndex], n_items: usize) -> Vec<usize> {
    let mut depths = vec![0usize; n_items];
    for &node in topo_order {
        depths[graph[node]] = graph
            .neighbors_directed(node, petgraph::Direction::Incoming)
            .map(|pred| depths[graph[pred]] + 1)
            .max()
            .unwrap_or(0);
    }
    depths
}

#[cfg(test)]
mod tests;
