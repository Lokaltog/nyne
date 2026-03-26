//! Git-backed project cloning for overlay lowerdirs.
//!
//! Implements [`ProjectCloner`] for snapshot and hardlink cloning strategies.
//! Registered at link time via the [`PROJECT_CLONERS`] distributed slice.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr};
use git2::build::CloneLocal;
use git2::{ObjectType, Odb, Oid, Repository, Tree};
use linkme::distributed_slice;
use nyne::config::StorageStrategy;
use nyne::{ClonerFactory, PROJECT_CLONERS, ProjectCloner};
use tracing::{debug, info};

/// Git-backed [`ProjectCloner`] for overlay lowerdir construction.
///
/// Supports two strategies: [`Snapshot`](StorageStrategy::Snapshot) copies only
/// HEAD tree objects via the ODB (fast, minimal), while
/// [`Hardlink`](StorageStrategy::Hardlink) does a full `git clone --local`.
/// Registered at link time via the [`PROJECT_CLONERS`] distributed slice.
struct GitCloner;

/// Dispatches to [`clone_snapshot`] or [`clone_hardlink`] based on the
/// requested [`StorageStrategy`]. Panics on `Passthrough` (no cloning needed).
impl ProjectCloner for GitCloner {
    fn clone_project(&self, source: &Path, target: &Path, strategy: StorageStrategy) -> Result<()> {
        fs::create_dir_all(target).wrap_err_with(|| format!("creating clone target {}", target.display()))?;

        match strategy {
            StorageStrategy::Snapshot => clone_snapshot(source, target),
            StorageStrategy::Hardlink => clone_hardlink(source, target),
            StorageStrategy::Passthrough => unreachable!("passthrough strategy does not clone"),
        }
    }
}

/// Cloner factory registered via `linkme` distributed slice.
#[allow(unsafe_code)]
#[distributed_slice(PROJECT_CLONERS)]
static GIT_CLONER: ClonerFactory = || Box::new(GitCloner);

/// Snapshot clone: copy only HEAD tree objects via the git object database.
///
/// 1. Opens the source repo and locates the HEAD commit's tree.
/// 2. Initialises an empty repo at `target`.
/// 3. Recursively copies every tree and blob object reachable from HEAD
///    into the target's object store using `Odb::read` / `Odb::write`.
/// 4. Creates a single snapshot commit pointing at the copied tree.
/// 5. Checks out the working tree.
///
/// No alternates, no hardlinks, no filesystem assumptions. The target
/// repo is fully self-contained after this function returns.
fn clone_snapshot(source: &Path, target: &Path) -> Result<()> {
    debug!(
        source = %source.display(),
        target = %target.display(),
        "cloning project (snapshot — ODB copy of HEAD tree)"
    );

    let source_repo =
        Repository::open(source).wrap_err_with(|| format!("opening source repo at {}", source.display()))?;
    let head_tree = source_repo
        .head()
        .wrap_err("resolving HEAD")?
        .peel_to_commit()
        .wrap_err("peeling HEAD to commit")?
        .tree()
        .wrap_err("getting HEAD tree")?;

    let target_repo =
        Repository::init(target).wrap_err_with(|| format!("initialising target repo at {}", target.display()))?;

    // Copy all tree + blob objects reachable from HEAD.
    let source_odb = source_repo.odb().wrap_err("opening source ODB")?;
    let target_odb = target_repo.odb().wrap_err("opening target ODB")?;
    let mut seen = HashSet::new();
    copy_tree_objects(&source_repo, &source_odb, &target_odb, &head_tree, &mut seen)
        .wrap_err("copying tree objects")?;

    debug!(objects = seen.len(), "HEAD tree objects copied");

    // Create a snapshot commit so the repo has a valid HEAD.
    let tree = target_repo.find_tree(head_tree.id()).wrap_err("finding copied tree")?;
    let sig = git2::Signature::now("nyne", "nyne@local").wrap_err("creating signature")?;
    target_repo
        .commit(Some("HEAD"), &sig, &sig, "snapshot", &tree, &[])
        .wrap_err("creating snapshot commit")?;

    target_repo
        .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .wrap_err("checking out HEAD")?;

    info!(
        source = %source.display(),
        target = %target.display(),
        objects = seen.len(),
        "project cloned (snapshot)"
    );

    Ok(())
}

/// Recursively copy tree and blob objects from source to target ODB.
///
/// Uses `Tree::iter()` to walk entries — only objects reachable from the
/// given tree are copied. Each object is copied exactly once (tracked
/// via `seen`). No history, tags, or refs are transferred.
fn copy_tree_objects(
    repo: &Repository,
    source: &Odb<'_>,
    target: &Odb<'_>,
    tree: &Tree<'_>,
    seen: &mut HashSet<Oid>,
) -> Result<()> {
    if !seen.insert(tree.id()) {
        return Ok(());
    }

    let obj = source.read(tree.id()).wrap_err("reading tree object")?;
    target.write(obj.kind(), obj.data()).wrap_err("writing tree object")?;

    for entry in tree {
        match entry.kind() {
            Some(ObjectType::Tree) => {
                let subtree = repo.find_tree(entry.id()).wrap_err("finding subtree")?;
                copy_tree_objects(repo, source, target, &subtree, seen)?;
            }
            Some(ObjectType::Blob) if seen.insert(entry.id()) => {
                let blob = source.read(entry.id()).wrap_err("reading blob")?;
                target.write(blob.kind(), blob.data()).wrap_err("writing blob")?;
            }
            _ => {}
        }
    }

    Ok(())
}

/// Hardlink clone: full `git clone --local` via `RepoBuilder`.
///
/// Uses `CloneLocal::Auto` which hardlinks objects from the source repo's
/// object store when source and target are on the same filesystem. Falls
/// back to a full copy when they differ — this can be very large for
/// repos with extensive history.
fn clone_hardlink(source: &Path, target: &Path) -> Result<()> {
    debug!(
        source = %source.display(),
        target = %target.display(),
        "cloning project (hardlink — CloneLocal::Auto)"
    );

    git2::build::RepoBuilder::new()
        .clone_local(CloneLocal::Auto)
        .clone(&source.to_string_lossy(), target)
        .wrap_err_with(|| format!("git clone failed: {} → {}", source.display(), target.display()))?;

    info!(
        source = %source.display(),
        target = %target.display(),
        "project cloned (hardlink)"
    );

    Ok(())
}
