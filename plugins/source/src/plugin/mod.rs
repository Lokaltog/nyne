pub mod config;
use std::path::Path;
use std::sync::Arc;

use linkme::distributed_slice;
use nyne::ActivationContext;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use nyne::router::{Filesystem, NamedNode, Node, NodeKind, Provider, Request, RouteCtx};
use nyne::templates::{HandleBuilder, TemplateHandle};
use nyne_companion::{CompanionContextExt, CompanionExtensions, CompanionRequest};
use nyne_diff::DiffRequest;
use tracing::info;

use crate::context::SourceContextExt;
use crate::edit::staging::{BatchEditAction, ClearWritable, EditStaging};
use crate::paths::SourcePaths;
use crate::plugin::config::Config;
use crate::plugin::config::vfs::Vfs;
use crate::provider::syntax::{SyntaxProvider, file_doc_text, routes};
use crate::syntax::SyntaxRegistry;
use crate::syntax::decomposed::DecompositionCache;
use crate::syntax::view::{SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC};

/// Entry point for the source plugin, implementing the [`Plugin`] trait.
///
/// This is a unit struct that serves as the anchor for plugin lifecycle
/// methods. It is instantiated by [`SOURCE_PLUGIN`] and registered into the
/// global plugin slice at link time.
pub struct SourcePlugin;

/// Two-phase lifecycle for the source plugin.
///
/// During `activate`, all heavyweight services (syntax registry,
/// decomposition cache) are constructed and inserted individually into
/// the `AnyMap`. The `providers` phase creates provider instances
/// that read them back.
impl Plugin for SourcePlugin {
    nyne::provider_graph!(SyntaxProvider);

    nyne::plugin_config!(Config);

    /// Returns the unique identifier for this plugin (`"source"`).
    fn id(&self) -> &'static str { "source" }

    /// Constructs and registers source services into the activation context.
    ///
    /// Inserts `Arc<SyntaxRegistry>` and `DecompositionCache` as separate
    /// `AnyMap` entries so downstream plugins can retrieve them individually.
    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let config = Config::from_context(ctx, self.id());
        let syntax = SyntaxRegistry::global();
        let decomposition = DecompositionCache::new(Arc::clone(ctx.fs()), Arc::clone(&syntax), config.max_depth);

        info!(
            languages = syntax.extensions().len(),
            max_depth = config.max_depth,
            "source plugin activated",
        );

        let staging = EditStaging::new();
        let fs = Arc::clone(ctx.fs());

        ctx.insert(Arc::new(SourcePaths::from_vfs(&config.vfs.dir)));
        ctx.insert(Arc::clone(&syntax));
        ctx.insert(decomposition.clone());
        ctx.insert(staging.clone());
        ctx.source_extensions_mut();

        // Register @/edit/staged.diff into the mount-wide companion.
        register_staged_diff(
            ctx.companion_extensions_mut(),
            &config.vfs,
            staging,
            decomposition.clone(),
            Arc::clone(&syntax),
            Arc::clone(&fs),
        );

        // Register dir@/OVERVIEW.md into the directory companion.
        let mut b = HandleBuilder::new();
        let key = b.register(
            "syntax/dir_overview",
            include_str!("../provider/syntax/templates/dir_overview.md.j2"),
        );
        register_dir_overview(
            ctx.companion_extensions_mut(),
            TemplateHandle::new(&b.finish(), key),
            config.vfs.file.overview,
            decomposition,
            syntax,
            fs,
        );

        Ok(())
    }

    #[expect(clippy::expect_used, reason = "source plugin activation is a lifecycle invariant")]
    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let config = Config::from_context(ctx, self.id());
        let syntax = ctx.syntax_registry().expect("SyntaxRegistry missing");
        let decomposition = ctx.decomposition_cache().expect("DecompositionCache missing");
        let staging = ctx.edit_staging().expect("EditStaging missing");
        let exts = ctx.source_extensions().expect("SourceExtensions missing");

        let mut b = HandleBuilder::new();
        b.register_partial(SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC);
        let overview_key = b.register(
            "syntax/overview",
            include_str!("../provider/syntax/templates/overview.md.j2"),
        );
        let file_overview_key = b.register(
            "syntax/file_overview",
            include_str!("../provider/syntax/templates/file_overview.md.j2"),
        );
        let engine = b.finish();

        Ok(vec![Arc::new(SyntaxProvider {
            tree: routes::build_tree(&config.vfs, exts),
            overview: TemplateHandle::new(&engine, overview_key),
            file_overview: TemplateHandle::new(&engine, file_overview_key),
            registry: Arc::clone(syntax),
            decomposition: decomposition.clone(),
            staging: staging.clone(),
            fs: Arc::clone(ctx.fs()),
            vfs: config.vfs,
        })])
    }
}

/// Link-time registration of the source plugin into the global `PLUGINS` slice.
///
/// The binary's `main.rs` pulls in this crate with `use nyne_source as _;`,
/// which is enough for `linkme` to include this static in the final binary.
/// At startup, the framework iterates `PLUGINS` and calls each factory to
/// obtain a `Box<dyn Plugin>`.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static SOURCE_PLUGIN: PluginFactory = || Box::new(SourcePlugin);

/// Register `@/edit/staged.diff` into the mount-wide companion extension point.
///
/// The staged.diff node is handled by the diff middleware:
/// - **Readable**: preview all staged edits as a unified diff (via middleware)
/// - **Unlinkable**: `rm staged.diff` applies all edits atomically (via middleware)
/// - **Writable**: `> staged.diff` (truncating write) clears all staged edits
#[allow(clippy::excessive_nesting)] // route registration closures nest inherently
fn register_staged_diff(
    exts: &mut CompanionExtensions,
    vfs: &Vfs,
    staging: EditStaging,
    decomposition: DecompositionCache,
    registry: Arc<SyntaxRegistry>,
    fs: Arc<dyn Filesystem>,
) {
    // Bare file node carrying `ClearWritable` so that `> staged.diff`
    // drains the staging area. Used on both the readdir and lookup paths
    // so the looked-up node ends up with `DiffPreview` (readable, added
    // by the diff middleware) and `ClearWritable` (writable, added here)
    // merged together by `NodeAccumulator::add` (first-writer-wins).
    fn staged_node(staging: &EditStaging, name: &str) -> NamedNode {
        Node::file()
            .with_writable(ClearWritable {
                staging: staging.clone(),
            })
            .named(name)
    }

    let dir_edit = vfs.dir.edit.clone();
    let file_staged = vfs.file.staged_diff.clone();

    exts.mount.scoped("source", |ext| {
        ext.dir(dir_edit, move |d| {
            let content_staging = staging.clone();
            let content_name = file_staged.clone();

            // Readdir: contribute the staged.diff entry.
            d.content(move |_ctx: &RouteCtx, _req: &Request| Some(staged_node(&content_staging, &content_name)));

            // Lookup: set DiffCapable for the diff middleware AND add the
            // `ClearWritable` node. Without this the lookup path produced
            // a readable-only node and `> staged.diff` never drained.
            let action = BatchEditAction {
                staging,
                decomposition,
                registry,
            };
            d.on_lookup(move |_ctx: &RouteCtx, req: &mut Request, name: &str| {
                if name != file_staged.as_str() {
                    return Ok(());
                }
                req.set_diff_source(action.clone(), Arc::clone(&fs));
                req.nodes.add(staged_node(&action.staging, name));
                Ok(())
            });
        });
    });
}

/// Register `dir@/OVERVIEW.md` into the directory companion extension point.
///
/// Lists all parseable source files in the directory with their language,
/// line count, and the first line of the file-level docstring.
fn register_dir_overview(
    exts: &mut CompanionExtensions,
    handle: TemplateHandle,
    overview_name: String,
    decomposition: DecompositionCache,
    registry: Arc<SyntaxRegistry>,
    fs: Arc<dyn Filesystem>,
) {
    exts.dir.scoped("source", |ext| {
        ext.content(move |_ctx: &RouteCtx, req: &Request| {
            let source_dir = req.source_file()?;
            let fs = Arc::clone(&fs);
            let decomposition = decomposition.clone();
            let registry = Arc::clone(&registry);
            Some(handle.lazy_node(overview_name.clone(), move |engine, tmpl| {
                let dirname = source_dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
                let files = dir_overview_files(&source_dir, &fs, &registry, &decomposition);
                let view = minijinja::context! { dirname, files };
                Ok(engine.render_bytes(tmpl, &view))
            }))
        });
    });
}

/// Collect per-file summary data for a directory overview.
fn dir_overview_files(
    dir: &Path,
    fs: &Arc<dyn Filesystem>,
    registry: &Arc<SyntaxRegistry>,
    decomposition: &DecompositionCache,
) -> Vec<minijinja::Value> {
    let Ok(mut entries) = fs.read_dir(dir) else {
        return Vec::new();
    };
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
        .iter()
        .filter(|e| e.kind == NodeKind::File)
        .filter_map(|e| {
            let path = dir.join(&e.name);
            let decomposer = registry.decomposer_for(&path)?;
            let language = decomposer.language_name().to_owned();
            let shared = decomposition.get(&path).ok()?;
            let lines = shared.source.lines().count();
            let description = file_doc_text(&shared)
                .and_then(|doc| doc.lines().next().map(str::to_owned))
                .unwrap_or_default();
            Some(minijinja::context! {
                name => e.name,
                language,
                lines,
                description,
            })
        })
        .collect()
}
