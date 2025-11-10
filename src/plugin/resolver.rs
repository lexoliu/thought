use std::path::PathBuf;

use thiserror::Error;

use crate::{
    metadata::{FailToOpenMetadata, MetadataExt, PluginLocator, PluginManifest},
    workspace::Workspace,
};

pub struct ResolvedPlugin {
    built: bool, // Whether the plugin has been built
    manifest: PluginManifest,
    dir: PathBuf,
}

impl ResolvedPlugin {
    #[must_use]
    pub const fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub const fn is_built(&self) -> bool {
        self.built
    }

    pub async fn build(&mut self) -> color_eyre::eyre::Result<()> {
        if self.built {
            return Ok(());
        }

        todo!(); // TODO: implement plugin building logic, using WASI preview 2

        self.built = true;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ResolvePluginError {
    #[error("Fail to open plugin manifest: {0}")]
    FailToOpenPluginManifest(#[from] FailToOpenMetadata),
}

pub async fn resolve_plugin(
    workspace: &Workspace,
    name: &str,
    locator: &PluginLocator,
) -> Result<ResolvedPlugin, ResolvePluginError> {
    let built = false;
    // prepare plugin to be used within the workspace's cache directory
    let dir: PathBuf = match locator {
        PluginLocator::CratesIo { version } => {
            // if here is a github repo, may we can try to download pre-built version from github release first

            todo!()
        }
        PluginLocator::Git { url, rev } => {
            // if it is a github repo, may we can try to download pre-built version from github release first
            todo!()
        }
        PluginLocator::Local { path } => {
            // copy it directly
            todo!()
        }
    };

    let manifest = PluginManifest::open(dir.join("Plugin.toml")).await?;

    Ok(ResolvedPlugin {
        built,
        manifest,
        dir,
    })
}

async fn try_github_release(
    author: &str,
    repo: &str,
    tag: &str, // git tag
) -> Result<Option<PathBuf>, ResolvePluginError> {
    // attempt to download release from GitHub
    todo!()
}
