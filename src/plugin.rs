use anyhow::{Context, Result};
use core::sync::atomic::{AtomicBool, AtomicUsize};
use std::{
    collections::{BTreeMap, HashSet},
    env::temp_dir,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    thread::spawn,
};
use thought_core::{
    article::{Article, ArticlePreview},
    metadata::{PluginSource, WorkspaceMetadata},
};
use tokio::io::AsyncWrite;
use wasmtime::{Module, Store, component::Linker};

use url::Url;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, p2::add_to_linker_async};

use crate::workspace::Workspace;

/// Manages Wasm plugins, including loading and running them.
pub struct PluginManager {
    workspace: Workspace,
}

impl PluginManager {}

/// Represents a runtime instance of a theme plugin.
///
/// Theme is a pure function, taking article data as input and producing HTML as output.
///
/// Theme runtime has no access to filesystem,time,random,or network.
struct ThemeRuntime {
    name: String,
}

impl ThemeRuntime {
    pub fn new(name: String, binary: &[u8]) -> Self {
        let engine = wasmtime::Engine::default();
        todo!()
    }
}

/// Represents a runtime instance of a plugin.
///
/// Plugins operate with restricted capabilities for security and isolation:
///
/// ## Capabilities
/// - **Random**: Access to random number generation
/// - **Time**: Access to system time
///
/// ## Filesystem
/// Each plugin has an isolated virtual filesystem:
/// - `/tmp` - Read/write temporary storage (/tmp will be cleared on each run)
/// - `/cache` - Read/write cache storage (we will make efforts to persist cache between runs)
/// - `/build` - Read/write build artifacts
/// - `/assets` - Read-only bundled resources
///
/// ## Network
/// Network access is not supported by now. It is planned to be added in the future.
struct PluginRuntime {
    name: String,
    enable_network: bool,
}

impl PluginRuntime {
    pub async fn new(
        name: String,
        binary: &[u8],
        cache_path: &Path,
        assets_path: &Path,
        build_path: Option<&Path>,
    ) -> Self {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        let engine = wasmtime::Engine::new(&config).expect("Failed to create Wasmtime engine");

        let mut wasi = WasiCtx::builder();

        let tmp_dir = temp_dir().join("thought-plugins").join(&name);
        let cache_dir = cache_path.join("thought-plugins").join(&name);

        wasi.preopened_dir(tmp_dir, "/tmp", DirPerms::all(), FilePerms::all());
        wasi.preopened_dir(cache_dir, "/cache", DirPerms::all(), FilePerms::all());
        wasi.preopened_dir(assets_path, "/assets", DirPerms::READ, FilePerms::READ);

        if let Some(build_path) = build_path {
            wasi.preopened_dir(build_path, "/build", DirPerms::all(), FilePerms::all());
        }

        let module = Module::new(&engine, binary).expect("Failed to create Wasm module");

        let mut store = Store::new(&engine, wasi);

        todo!()
    }

    pub fn attach_assets_path(&mut self, path: impl AsRef<Path>) {
        todo!()
    }

    pub fn attach_build_path(&mut self, path: impl AsRef<Path>) {
        todo!()
    }

    pub fn enable_network(&mut self) {
        self.enable_network = true;
    }
}
