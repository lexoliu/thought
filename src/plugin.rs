use crate::{
    plugin::bindings::{hook::HookRuntime, theme::ThemeRuntime},
    workspace::Workspace,
};

mod bindings;
mod resolver;

pub struct PluginManager {
    hooks: Vec<HookRuntime>,
    themes: Vec<ThemeRuntime>,
}

impl PluginManager {
    pub fn resolve_workspace(workspace: &Workspace) -> color_eyre::eyre::Result<Self> {
        let manifest = workspace.manifest();
        for (name, locator) in manifest.plugins() {}
        todo!()
    }
}
