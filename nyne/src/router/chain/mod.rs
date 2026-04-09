use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::router::{Op, Provider, ProviderId, Request};

/// Continuation handle — invoke to run the remaining providers in the chain.
pub struct Next<'a> {
    remaining: &'a [Arc<dyn Provider>],
}

impl Next<'_> {
    /// Create an empty `Next` with no remaining providers (for testing).
    pub fn empty() -> Next<'static> { Next { remaining: &[] } }

    /// Run the next provider in the chain.
    pub fn run(&self, req: &mut Request) -> Result<()> {
        let Some((current, rest)) = self.remaining.split_first() else {
            return Ok(());
        };
        let next = Next { remaining: rest };
        current.accept(req, &next)
    }
}

/// An ordered middleware chain of providers.
///
/// Providers are sorted by dependency graph (topological sort with
/// lexicographic tiebreaker for deterministic ordering of siblings).
pub struct Chain {
    providers: Vec<Arc<dyn Provider>>,
}

impl fmt::Debug for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct ProviderIds<'a>(&'a [Arc<dyn Provider>]);
        impl fmt::Debug for ProviderIds<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_list().entries(self.0.iter().map(|p| p.id())).finish()
            }
        }
        f.debug_struct("Chain")
            .field("providers", &ProviderIds(&self.providers))
            .finish()
    }
}

impl Chain {
    /// Build a chain from a set of providers, ordering them by dependency graph.
    ///
    /// Returns an error if the dependency graph contains a cycle or references
    /// an unknown provider.
    #[allow(clippy::needless_pass_by_value)]
    pub fn build(providers: Vec<Arc<dyn Provider>>) -> Result<Self> {
        Ok(Self {
            providers: topological_sort(&providers)?,
        })
    }

    /// Dispatch a request through the middleware chain.
    pub fn dispatch(&self, req: &mut Request) -> Result<()> {
        Next {
            remaining: &self.providers,
        }
        .run(req)
    }

    /// Evaluate a path through the middleware chain and return the resulting request.
    ///
    /// Creates a [`Request`], dispatches it through the full chain, and returns
    /// it with all middleware state populated. Useful for inspecting what state
    /// the pipeline would produce for a given path — e.g., from scripts or
    /// hooks that operate outside the normal dispatch flow (FUSE, etc.).
    ///
    /// ```ignore
    /// let req = chain.evaluate(path, Op::Readdir)?;
    /// let companion = req.state::<Companion>();
    /// ```
    pub fn evaluate(&self, path: PathBuf, op: Op) -> Result<Request> {
        let mut req = Request::new(path, op);
        self.dispatch(&mut req)?;
        Ok(req)
    }

    /// The ordered provider IDs in this chain (for testing/debugging).
    pub fn order(&self) -> Vec<ProviderId> { self.providers.iter().map(|p| p.id()).collect() }

    /// Access the ordered provider list (for invalidation, introspection).
    pub fn providers(&self) -> &[Arc<dyn Provider>] { &self.providers }
}

/// Sort providers by dependency graph with lexicographic tiebreaker.
/// Terminal providers are partitioned to the end of the chain.
fn topological_sort(providers: &[Arc<dyn Provider>]) -> Result<Vec<Arc<dyn Provider>>> {
    // Partition: non-terminal providers get sorted, terminal appended last
    let (regular, terminal): (Vec<_>, Vec<_>) = providers.iter().partition(|p| !p.terminal());

    let mut sorted = sort_by_deps(&regular)?;

    // Terminal providers appended in lexicographic order
    let mut terminal_sorted = terminal;
    terminal_sorted.sort_by_key(|p| p.id());
    sorted.extend(terminal_sorted.into_iter().cloned());

    Ok(sorted)
}

fn sort_by_deps(providers: &[&Arc<dyn Provider>]) -> Result<Vec<Arc<dyn Provider>>> {
    let mut graph = DiGraph::<usize, ()>::new();
    let mut id_to_idx: HashMap<ProviderId, NodeIndex> = HashMap::new();
    let mut node_indices: Vec<NodeIndex> = Vec::new();

    // Add nodes
    for (i, p) in providers.iter().enumerate() {
        let idx = graph.add_node(i);
        id_to_idx.insert(p.id(), idx);
        node_indices.push(idx);
    }

    // Add edges (dependency → dependent)
    for (i, p) in providers.iter().enumerate() {
        #[expect(clippy::indexing_slicing, reason = "node_indices built in lockstep with providers")]
        let self_idx = node_indices[i];
        for dep_id in p.dependencies() {
            // Soft dependency: if the dependency isn't in the provider set,
            // skip the edge. This allows partial chains (e.g. LSP without syntax).
            let Some(&dep_idx) = id_to_idx.get(dep_id) else {
                continue;
            };
            // Edge from dependency to dependent (dep must come first)
            graph.add_edge(dep_idx, self_idx, ());
        }
    }

    // Topological sort
    let sorted_indices = toposort(&graph, None).map_err(|cycle| {
        let provider_idx = graph[cycle.node_id()];
        let id = providers.get(provider_idx).map_or("unknown", |p| p.id().as_str());
        color_eyre::eyre::eyre!("dependency cycle detected involving provider {:?}", id)
    })?;

    // petgraph's toposort is deterministic but doesn't guarantee lexicographic
    // order among siblings. We do a stable sort that preserves topological
    // order while sorting siblings by priority then lexicographically.
    //
    // Approach: assign each node its topological depth (longest path from a root),
    // then stable-sort by (depth, priority, provider_id).
    let depths = compute_depths(&graph, &sorted_indices);

    let mut result: Vec<(usize, i32, ProviderId, Arc<dyn Provider>)> = sorted_indices
        .iter()
        .filter_map(|&node_idx| {
            let provider_idx = graph[node_idx];
            let provider = providers.get(provider_idx)?;
            let depth = depths.get(&node_idx).copied().unwrap_or(0);
            Some((depth, provider.priority(), provider.id(), Arc::clone(provider)))
        })
        .collect();

    result.sort_by(|(depth_a, pri_a, id_a, _), (depth_b, pri_b, id_b, _)| {
        depth_a
            .cmp(depth_b)
            .then_with(|| pri_a.cmp(pri_b))
            .then_with(|| id_a.cmp(id_b))
    });

    Ok(result.into_iter().map(|(_, _, _, p)| p).collect())
}

/// Compute the depth (longest path from any root) for each node.
fn compute_depths(graph: &DiGraph<usize, ()>, topo_order: &[NodeIndex]) -> HashMap<NodeIndex, usize> {
    let mut depths: HashMap<NodeIndex, usize> = HashMap::new();

    for &node in topo_order {
        let depth = graph
            .neighbors_directed(node, petgraph::Direction::Incoming)
            .filter_map(|pred| depths.get(&pred).map(|d| d + 1))
            .max()
            .unwrap_or(0);
        depths.insert(node, depth);
    }

    depths
}
