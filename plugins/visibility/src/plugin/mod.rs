mod config;
use std::iter;
use std::sync::Arc;

use linkme::distributed_slice;
use nyne::ControlCommand;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use nyne::router::Provider;

use crate::context::VisibilityContextExt;
use crate::control::handle_set_visibility;
use crate::plugin::config::VisibilityConfig;
use crate::process_visibility::ProcessVisibility;
use crate::provider::{Visibility, VisibilityPolicy, VisibilityProvider};
use crate::visibility_map::VisibilityMap;

struct VisibilityPlugin;

impl Plugin for VisibilityPlugin {
    nyne::plugin_config!(VisibilityConfig);

    fn id(&self) -> &'static str { "visibility" }

    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let config = VisibilityConfig::from_context(ctx, self.id());

        // Merge config passthrough names with plugin-contributed names.
        let plugin_procs = ctx
            .passthrough_processes()
            .map_or_else(Vec::new, |p| p.as_slice().to_vec());
        let name_rules = config
            .passthrough_processes
            .into_iter()
            .chain(plugin_procs)
            .map(|name| (name, ProcessVisibility::None));

        let vis = Arc::new(VisibilityMap::new(name_rules).with_cgroup_tracking());
        ctx.insert(vis);

        Ok(())
    }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let vis = ctx
            .visibility_map()
            .cloned()
            .unwrap_or_else(|| Arc::new(VisibilityMap::new(iter::empty())));

        let policy: VisibilityPolicy = Box::new(move |req| {
            let pid = req.process()?.pid;
            Some(match vis.resolve(pid) {
                ProcessVisibility::None => Visibility::Hidden,
                ProcessVisibility::Default => Visibility::Default,
                ProcessVisibility::All => Visibility::Force,
            })
        });

        Ok(vec![Arc::new(VisibilityProvider { policy })])
    }

    fn control_commands(&self, ctx: &Arc<ActivationContext>) -> Vec<ControlCommand> {
        let vis = ctx.visibility_map().cloned();
        vec![ControlCommand {
            name: "SetVisibility",
            handler: Box::new(move |payload, ctrl_ctx| handle_set_visibility(payload, ctrl_ctx, vis.as_ref())),
        }]
    }
}

#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static VISIBILITY_PLUGIN: PluginFactory = || Box::new(VisibilityPlugin);
