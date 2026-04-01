use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use nyne::ExtensionCounts;
use nyne::plugin::{instantiate, sort_by_deps};
use nyne::prelude::*;
use nyne::router::Chain;
use nyne::templates::TemplateEngine;

/// Per-provider row for the middleware chain table.
#[derive(serde::Serialize)]
struct ProviderRow {
    position: usize,
    plugin: String,
    provider: String,
}
/// Render the `.nyne.md` template with live mount data.
pub(super) fn render_status(
    engine: &TemplateEngine,
    template: &str,
    ctx: &Arc<ActivationContext>,
    start_time: Instant,
) -> Vec<u8> {
    let uptime = humantime::format_duration(start_time.elapsed()).to_string();

    let empty = ExtensionCounts::default();
    let ext_counts = ctx.get::<ExtensionCounts>().unwrap_or(&empty);
    let languages = languages_display(ext_counts.as_slice());

    // Build provider→plugin ownership map from sorted plugins.
    let sorted_plugins = sort_by_deps(instantiate()).unwrap_or_else(|_| instantiate());
    let mut provider_plugin: HashMap<String, String> = HashMap::new();
    let mut all_providers: Vec<Arc<dyn Provider>> = Vec::new();
    for plugin in &sorted_plugins {
        if let Ok(providers) = plugin.providers(ctx).inspect_err(|e| {
            tracing::warn!(plugin = plugin.id(), error = %e, "provider query failed in status view");
        }) {
            for p in &providers {
                provider_plugin.insert(p.id().to_string(), plugin.id().to_owned());
            }
            all_providers.extend(providers);
        }
    }

    // Sort providers into middleware chain dispatch order.
    let chain_order: Vec<ProviderRow> = Chain::build(all_providers)
        .map(|chain| {
            chain
                .order()
                .iter()
                .enumerate()
                .map(|(i, id)| ProviderRow {
                    position: i + 1,
                    plugin: provider_plugin.get(id.as_str()).cloned().unwrap_or_default(),
                    provider: id.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    let view = minijinja::context! {
        host_root => ctx.host_root().display().to_string(),
        source_dir => ctx.root().display().to_string(),
        languages,
        uptime,
        chain => chain_order,
    };
    engine.render_bytes(template, &view)
}
/// Format extension counts into a human-readable language list.
pub(super) fn languages_display(ext_counts: &[(String, usize)]) -> String {
    if ext_counts.is_empty() {
        return String::from("(none detected)");
    }
    ext_counts
        .iter()
        .map(|(ext, _)| ext.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
