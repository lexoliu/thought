use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use thought_core::{
    article::{Article, ArticlePreview},
    metadata::{PluginSource, WorkspaceMetadata},
};
use wasmtime::{Instance, Linker, Memory, Module, Store};

use crate::workspace::Workspace;

/// Manages Wasm plugins, including loading and running them.
pub struct PluginManager {
    engine: wasmtime::Engine,
    modules: BTreeMap<String, Module>,
    workspace: Workspace,
}

impl PluginManager {
    /// Loads plugins based on workspace metadata.
    ///
    /// Security configuration:
    /// - WASM plugins run in a sandboxed environment
    /// - Only random number generation and system time access are allowed
    /// - No file system, network, or environment variable access
    pub async fn load(workspace: &Workspace) -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);

        // Configure fuel consumption to prevent infinite loops
        config.consume_fuel(true);

        // Ensure WASM plugins cannot access host functions beyond what we explicitly allow
        config.wasm_simd(true); // Allow SIMD for performance
        config.wasm_bulk_memory(true); // Allow bulk memory operations

        config.wasm_multi_memory(false); // Disable multiple memories for security
        config.wasm_threads(false); // Disable threads for security

        let engine = wasmtime::Engine::new(&config)?;
        let modules = BTreeMap::new();

        Ok(Self {
            engine,
            modules,
            workspace: workspace.clone(),
        })
    }

    async fn fetch_plugin(&self, name: &str, source: &PluginSource) -> Result<Module> {
        let wasm_path = fetch_plugin(self.workspace.path(), name, source).await?;

        // Load the wasm module
        let wasm_bytes = smol::fs::read(&wasm_path).await?;
        Module::from_binary(&self.engine, &wasm_bytes)
            .with_context(|| format!("Failed to load WASM module from {:?}", wasm_path))
    }
    /// Creates a new, isolated Wasm plugin instance for a single operation.
    ///
    /// Security notes:
    /// - Plugins run in a sandboxed WASM environment
    /// - No host function imports are allowed (plugins are pure computation)
    /// - Plugins can only access random numbers and time through WASI preview2 (to be added)
    /// - Fuel consumption is enabled to prevent infinite loops
    async fn create_instance(&self, module: &Module) -> Result<(Store<()>, Instance, Memory)> {
        let linker = Linker::new(&self.engine);
        let mut store = Store::new(&self.engine, ());

        // Set fuel limit to prevent runaway execution
        store.set_fuel(10_000_000)?; // 10 million instructions

        let instance = linker.instantiate_async(&mut store, module).await?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .context("Failed to find `memory` export")?;

        Ok((store, instance, memory))
    }

    /// Calls a WASM function with serialized input data and returns deserialized output.
    ///
    /// This method handles the full lifecycle of WASM interaction:
    /// 1. Serializes the input data
    /// 2. Allocates memory in WASM and writes the input
    /// 3. Calls the specified WASM function
    /// 4. Reads the result from WASM memory
    /// 5. Deallocates all memory
    /// 6. Returns the result as a String
    async fn call_wasm_function<T: serde::Serialize>(
        &self,
        store: &mut Store<()>,
        instance: &Instance,
        memory: &Memory,
        function_name: &str,
        input_data: &T,
    ) -> Result<String> {
        // 1. Serialize the input data
        let input_bytes = bincode::serialize(input_data)?;

        // 2. Get WASM memory allocation function
        let alloc_func = instance.get_typed_func::<u32, u32>(&mut *store, "alloc")?;

        // 3. Allocate memory in WASM and write the data
        let data_len = input_bytes.len() as u32;
        let wasm_ptr = alloc_func.call_async(&mut *store, data_len).await?;
        memory.write(&mut *store, wasm_ptr as usize, &input_bytes)?;

        // 4. Call the specified WASM function
        let wasm_func = instance.get_typed_func::<(u32, u32), u64>(&mut *store, function_name)?;
        let result_ptr_len = wasm_func
            .call_async(&mut *store, (wasm_ptr, data_len))
            .await?;

        // 5. Read the result from WASM memory
        let result_ptr = (result_ptr_len >> 32) as u32;
        let result_len = result_ptr_len as u32;
        let mut result_buffer = vec![0u8; result_len as usize];
        memory.read(&mut *store, result_ptr as usize, &mut result_buffer)?;

        // 6. Deallocate memory
        let dealloc_func = instance.get_typed_func::<(u32, u32), ()>(&mut *store, "dealloc")?;
        dealloc_func
            .call_async(&mut *store, (wasm_ptr, data_len))
            .await?;
        dealloc_func
            .call_async(&mut *store, (result_ptr, result_len))
            .await?;

        // 7. Convert result to String
        String::from_utf8(result_buffer).context("Failed to decode result string from UTF-8")
    }

    /// Renders a single article using the theme plugin.
    pub async fn render_article(
        &self,
        workspace: WorkspaceMetadata,
        article: Article,
    ) -> Result<String> {
        let theme_name = workspace.theme().name().to_string();
        let module = self
            .modules
            .get(theme_name.as_str())
            .with_context(|| format!("Theme plugin `{}` not loaded", theme_name))?;
        let (mut store, instance, memory) = self.create_instance(module).await?;

        self.call_wasm_function(&mut store, &instance, &memory, "generate_page", &article)
            .await
    }

    /// Renders the index page using the theme plugin.
    pub async fn render_index(
        &self,
        workspace: WorkspaceMetadata,
        articles: Vec<ArticlePreview>,
    ) -> Result<String> {
        let theme_name = workspace.theme().name().to_string();
        let module = self
            .modules
            .get(theme_name.as_str())
            .with_context(|| format!("Theme plugin `{}` not loaded", theme_name))?;
        let (mut store, instance, memory) = self.create_instance(module).await?;

        self.call_wasm_function(&mut store, &instance, &memory, "generate_index", &articles)
            .await
    }
}

async fn fetch_plugin(
    workspace_root: &Path,
    plugin_name: &str,
    source: &PluginSource,
) -> Result<PathBuf> {
    todo!()
}
