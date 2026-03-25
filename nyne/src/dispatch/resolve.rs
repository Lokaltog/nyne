//! Directory resolution pipeline with provider conflict negotiation.
//!
//! Handles multi-provider directory composition, name conflict detection,
//! and provider negotiation via the conflict protocol.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::panic;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use color_eyre::eyre::{Report, Result, bail};

use super::cache::CachedNodeKind;
use crate::dispatch::context::RequestContext;
use crate::node::VirtualNode;
use crate::node::kind::NodeKind;
use crate::provider::{ConflictInfo, ConflictResolution, Provider, ProviderId};

/// Catch panics from a provider closure, logging them with provider identity.
///
/// Returns `None` on panic (logged as error), `Some(result)` on normal return.
/// Callers decide what `None` means for their context.
fn catch_provider_panic<R>(pid: ProviderId, op: &str, f: impl FnOnce() -> R) -> Option<R> {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => Some(result),
        Err(payload) => {
            let msg = panic_message(&payload);
            tracing::error!(provider = %pid, op, "provider panicked: {msg}");
            None
        }
    }
}

/// Call a provider method, catching panics and converting them to `None` (decline).
///
/// Providers must not take down the FUSE handler thread. A panic is logged
/// and treated as a decline — same as returning `Ok(None)`.
fn catch_provider<T>(pid: ProviderId, op: &str, f: impl FnOnce() -> Result<Option<T>>) -> Result<Option<T>> {
    catch_provider_panic(pid, op, f).unwrap_or(Ok(None))
}

/// Extract a human-readable message from a panic payload.
fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic".to_owned()
    }
}

/// A resolved node paired with the provider that produced it.
pub(super) struct OwnedNode {
    pub(super) node: VirtualNode,
    pub(super) provider_id: ProviderId,
}

/// Conversion and conflict detection for provider-owned nodes.
impl OwnedNode {
    /// Convert into a `CachedNodeKind::Virtual`, wrapping the node in `Arc`.
    pub(super) fn into_cached_kind(self) -> CachedNodeKind {
        CachedNodeKind::Virtual {
            node: Arc::new(self.node),
            provider_id: self.provider_id,
        }
    }

    /// Detect name conflicts: names emitted by two or more providers.
    ///
    /// Returns a map of conflicting name -> list of provider IDs that emitted it.
    /// Directory-only conflicts are excluded — multiple providers may contribute
    /// a directory with the same name; the dispatch layer merges their children
    /// when that directory is resolved.
    pub(super) fn detect_conflicts(nodes: &[Self]) -> HashMap<String, Vec<ProviderId>> {
        let mut by_name: HashMap<&str, Vec<(ProviderId, bool)>> = HashMap::new();
        for owned in nodes {
            let is_dir = matches!(owned.node.kind(), NodeKind::Directory);
            by_name
                .entry(owned.node.name())
                .or_default()
                .push((owned.provider_id, is_dir));
        }

        by_name
            .into_iter()
            .filter_map(|(name, entries)| {
                let all_dirs = entries.iter().all(|(_, is_dir)| *is_dir);
                let mut pids: Vec<ProviderId> = entries.into_iter().map(|(pid, _)| pid).collect();
                pids.sort_by_key(ProviderId::as_str);
                pids.dedup();
                // Multiple providers for the same name is only a conflict if
                // at least one is not a directory (dirs merge naturally).
                (pids.len() > 1 && !all_dirs).then(|| (name.to_owned(), pids))
            })
            .collect()
    }
}

/// Outcome of calling `on_conflict` on all involved providers for a single name.
struct ConflictOutcome {
    forces: Vec<(ProviderId, Vec<VirtualNode>)>,
    retries: Vec<OwnedNode>,
}

/// Call `on_conflict` on all providers involved in a conflict for a given name.
///
/// Collects Force/Retry/Yield responses. The caller decides what to do
/// with the results based on the conflict type (provider-vs-provider or
/// provider-vs-real).
fn collect_conflict_responses(
    providers: &[Arc<dyn Provider>],
    involved_pids: &HashSet<ProviderId>,
    conflict_infos: &[ConflictInfo],
    ctx: &RequestContext<'_>,
) -> ConflictOutcome {
    let mut forces: Vec<(ProviderId, Vec<VirtualNode>)> = Vec::new();
    let mut retries: Vec<OwnedNode> = Vec::new();

    for provider in providers.iter().filter(|p| involved_pids.contains(&p.id())) {
        let pid = provider.id();
        let p = Arc::clone(provider);
        let Some(result) = catch_provider_panic(pid, "on_conflict", || p.on_conflict(ctx, conflict_infos)) else {
            continue;
        };
        match result {
            Ok(ConflictResolution::Yield) => {}
            Ok(ConflictResolution::Force(nodes)) => forces.push((pid, nodes)),
            Ok(ConflictResolution::Retry(nodes)) => {
                retries.extend(nodes.into_iter().map(|node| OwnedNode { node, provider_id: pid }));
            }
            Err(e) => tracing::warn!(provider = %pid, "on_conflict failed: {e}"),
        }
    }

    ConflictOutcome { forces, retries }
}

/// Apply force resolution: exactly one force wins, zero falls through, tied drops all.
///
/// Returns the winning nodes if exactly one provider forced, or `None` otherwise.
/// Logs warnings for tied conflicts.
fn apply_force_resolution(name: &str, forces: Vec<(ProviderId, Vec<VirtualNode>)>) -> Option<Vec<OwnedNode>> {
    match forces.len() {
        0 => None,
        1 => {
            let (pid, nodes) = forces.into_iter().next()?;
            // Force'd nodes shadow a real file — different processes may see
            // different content (passthrough vs virtual). The FUSE kernel cache
            // is per-inode not per-process, so any TTL > 0 would let a
            // non-passthrough response poison the cache for passthrough callers.
            // `mark_shadows_real()` causes the FUSE layer to set TTL=0 for
            // these inodes, forcing the kernel to always call our handlers
            // where resolve_for_request can demote appropriately.
            Some(
                nodes
                    .into_iter()
                    .map(|node| OwnedNode {
                        node: node.mark_shadows_real(),
                        provider_id: pid,
                    })
                    .collect(),
            )
        }
        n => {
            let force_pids: Vec<_> = forces.iter().map(|(pid, _)| pid.to_string()).collect();
            tracing::warn!(
                name,
                providers = ?force_pids,
                "tied force conflict ({n} providers), dropping all nodes"
            );
            None
        }
    }
}

/// Result of attempting to resolve a naming conflict through provider negotiation.
enum ConflictResult {
    /// Exactly one provider forced — these nodes win.
    Forced(Vec<OwnedNode>),
    /// No provider forced — retries available for caller-specific handling.
    Unforced { retries: Vec<OwnedNode> },
}

/// Negotiate a single naming conflict through the provider conflict protocol.
///
/// Calls `on_conflict` on all involved providers, then applies force
/// resolution. Returns [`ConflictResult::Forced`] if exactly one provider
/// wins, or [`ConflictResult::Unforced`] with retry nodes for the caller
/// to filter/validate based on context (provider-vs-provider or
/// provider-vs-real).
fn resolve_conflict(
    name: &str,
    providers: &[Arc<dyn Provider>],
    involved: &HashSet<ProviderId>,
    conflict_infos: &[ConflictInfo],
    ctx: &RequestContext<'_>,
) -> ConflictResult {
    let outcome = collect_conflict_responses(providers, involved, conflict_infos, ctx);
    match apply_force_resolution(name, outcome.forces) {
        Some(winners) => ConflictResult::Forced(winners),
        None => ConflictResult::Unforced {
            retries: outcome.retries,
        },
    }
}

/// Resolve a directory by collecting nodes from all providers and negotiating conflicts.
pub(super) fn resolve_directory(providers: &[Arc<dyn Provider>], ctx: &RequestContext<'_>) -> Result<Vec<OwnedNode>> {
    let mut all_nodes: Vec<OwnedNode> = Vec::new();
    let mut any_success = false;
    let mut last_error: Option<Report> = None;

    for provider in providers {
        let pid = provider.id();
        let p = Arc::clone(provider);
        match catch_provider(pid, "children", || p.children(ctx)) {
            Ok(Some(nodes)) => {
                any_success = true;
                all_nodes.extend(nodes.into_iter().map(|node| OwnedNode { node, provider_id: pid }));
            }
            Ok(None) => {
                any_success = true;
            }
            Err(e) => {
                tracing::warn!(
                    provider = %pid,
                    path = %ctx.path,
                    "provider children failed: {e}"
                );
                last_error = Some(e);
            }
        }
    }

    // If every provider errored, propagate the last error.
    if !any_success && let Some(e) = last_error {
        return Err(e.wrap_err(format!("all providers failed for {}", ctx.path)));
    }

    let conflicts = OwnedNode::detect_conflicts(&all_nodes);
    if conflicts.is_empty() {
        return Ok(all_nodes);
    }

    let conflicting_names: Vec<&String> = conflicts.keys().collect();
    tracing::debug!(?conflicting_names, "provider-vs-provider conflicts detected");

    // Separate non-conflicting nodes.
    let mut resolved: Vec<OwnedNode> = all_nodes
        .into_iter()
        .filter(|owned| !conflicts.contains_key(owned.node.name()))
        .collect();

    for (name, provider_ids) in &conflicts {
        let conflict_infos = ConflictInfo::for_providers(name, provider_ids.iter().copied());

        let involved: HashSet<ProviderId> = provider_ids.iter().copied().collect();

        match resolve_conflict(name, providers, &involved, &conflict_infos, ctx) {
            ConflictResult::Forced(winners) => resolved.extend(winners),
            ConflictResult::Unforced { retries } if retries.is_empty() => {
                let pids: Vec<_> = provider_ids.iter().map(ProviderId::as_str).collect();
                tracing::warn!(name, providers = ?pids, "all providers yielded, dropping conflicting nodes");
            }
            ConflictResult::Unforced { retries } => {
                let retry_conflicts = OwnedNode::detect_conflicts(&retries);
                if retry_conflicts.contains_key(name.as_str()) {
                    tracing::warn!(name, "conflict unresolved after retry, dropping nodes");
                } else {
                    resolved.extend(retries);
                }
            }
        }
    }

    Ok(resolved)
}

/// Dispatch an operation that expects at most one provider to claim a name.
///
/// Iterates all providers, calling `f` on each. Returns `Ok(None)` if no
/// provider claims the name, the single claimed node if exactly one does.
///
/// When multiple providers claim the same name, the conflict protocol is
/// invoked: each provider's `on_conflict` is called, and the single
/// `Force` winner (if any) keeps its original claim. If all yield, the
/// name is dropped. Tied forces are an error.
fn single_claim_dispatch(
    providers: &[Arc<dyn Provider>],
    op: &str,
    name: &str,
    ctx: &RequestContext<'_>,
    f: impl Fn(Arc<dyn Provider>, &RequestContext<'_>, &str) -> Result<Option<VirtualNode>>,
) -> Result<Option<OwnedNode>> {
    let claims: Vec<OwnedNode> = providers
        .iter()
        .filter_map(|provider| {
            let pid = provider.id();
            let p = Arc::clone(provider);
            match catch_provider(pid, op, || f(p, ctx, name)) {
                Ok(Some(node)) => Some(OwnedNode { node, provider_id: pid }),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(
                        provider = %pid,
                        path = %ctx.path,
                        name,
                        "provider {op} failed: {e}"
                    );
                    None
                }
            }
        })
        .collect();

    match claims.len() {
        0 => Ok(None),
        1 => Ok(claims.into_iter().next()),
        _ => resolve_competing_claims(providers, op, name, ctx, claims),
    }
}

/// Resolve a multi-claim conflict for a single name using the conflict protocol.
///
/// Each claimant's `on_conflict` is called. Exactly one `Force` wins (its
/// original claim is returned). All-yield returns `None`. Tied forces bail.
fn resolve_competing_claims(
    providers: &[Arc<dyn Provider>],
    op: &str,
    name: &str,
    ctx: &RequestContext<'_>,
    claims: Vec<OwnedNode>,
) -> Result<Option<OwnedNode>> {
    let involved: HashSet<ProviderId> = claims.iter().map(|c| c.provider_id).collect();

    // Build conflict info: each claimant is told about the other claimants.
    let conflict_infos = ConflictInfo::for_providers(name, claims.iter().map(|c| c.provider_id));

    let outcome = collect_conflict_responses(providers, &involved, &conflict_infos, ctx);

    match outcome.forces.len() {
        0 => {
            tracing::debug!(
                path = %ctx.path,
                name,
                "all providers yielded on {op} conflict, dropping name"
            );
            Ok(None)
        }
        1 => {
            let &[(winner_pid, _)] = outcome.forces.as_slice() else {
                unreachable!()
            };
            Ok(claims.into_iter().find(|c| c.provider_id == winner_pid))
        }
        n => {
            let force_pids: Vec<_> = outcome.forces.iter().map(|(pid, _)| pid.to_string()).collect();
            tracing::warn!(
                path = %ctx.path,
                name,
                providers = ?force_pids,
                "tied force conflict ({n} providers) on {op}, dropping all claims"
            );
            bail!("tied {op}: {n} providers force-claimed name \"{name}\" at {}", ctx.path)
        }
    }
}

/// Look up a specific name via fallback provider lookup.
///
/// Returns `Ok(None)` if no provider claims the name, or `Err` if
/// multiple providers claim it (ambiguous).
pub(super) fn lookup_name(
    providers: &[Arc<dyn Provider>],
    name: &str,
    ctx: &RequestContext<'_>,
) -> Result<Option<OwnedNode>> {
    single_claim_dispatch(providers, "lookup", name, ctx, Provider::lookup)
}

/// Create a file via provider delegation.
///
/// Uses single-claim semantics: at most one provider may handle the
/// creation. Returns `Ok(None)` if no provider claims it, or `Err`
/// if multiple providers claim it (ambiguous).
pub(super) fn create_in_directory(
    providers: &[Arc<dyn Provider>],
    name: &str,
    ctx: &RequestContext<'_>,
) -> Result<Option<OwnedNode>> {
    single_claim_dispatch(providers, "create", name, ctx, Provider::create)
}

/// Create a directory via provider delegation.
///
/// Uses single-claim semantics: at most one provider may handle the
/// creation. Returns `Ok(None)` if no provider claims it, or `Err`
/// if multiple providers claim it (ambiguous).
pub(super) fn mkdir_in_directory(
    providers: &[Arc<dyn Provider>],
    name: &str,
    ctx: &RequestContext<'_>,
) -> Result<Option<OwnedNode>> {
    single_claim_dispatch(providers, "mkdir", name, ctx, Provider::mkdir)
}

/// Resolve conflicts between virtual nodes and real filesystem entries.
///
/// Returns the surviving virtual nodes and the set of real names shadowed by forced wins.
pub(super) fn resolve_real_conflicts(
    providers: &[Arc<dyn Provider>],
    virtual_nodes: Vec<OwnedNode>,
    real_names: &HashSet<&str>,
    ctx: &RequestContext<'_>,
) -> (Vec<OwnedNode>, HashSet<String>) {
    let mut non_conflicting: Vec<OwnedNode> = Vec::new();
    let mut conflicting: HashMap<String, Vec<OwnedNode>> = HashMap::new();

    for owned in virtual_nodes {
        if real_names.contains(owned.node.name()) {
            conflicting.entry(owned.node.name().to_owned()).or_default().push(owned);
        } else {
            non_conflicting.push(owned);
        }
    }

    if conflicting.is_empty() {
        return (non_conflicting, HashSet::new());
    }

    let conflicting_names: Vec<&String> = conflicting.keys().collect();
    tracing::debug!(?conflicting_names, "virtual-vs-real conflicts detected");

    let mut shadowed: HashSet<String> = HashSet::new();

    for (name, nodes) in conflicting {
        let involved: HashSet<ProviderId> = nodes.iter().map(|o| o.provider_id).collect();
        let conflict_infos = ConflictInfo::for_real_conflict(&name, involved.iter().copied());

        match resolve_conflict(&name, providers, &involved, &conflict_infos, ctx) {
            ConflictResult::Forced(winners) => {
                shadowed.insert(name);
                non_conflicting.extend(winners);
            }
            ConflictResult::Unforced { retries } => {
                // No force — real file wins. Keep retries that don't collide with real names.
                non_conflicting.extend(retries.into_iter().filter(|r| !real_names.contains(r.node.name())));
            }
        }
    }

    (non_conflicting, shadowed)
}
