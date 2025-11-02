use std::{
    iter::{once, repeat},
    path::Path,
    sync::Arc,
    thread,
};

use wasmer::{imports, Engine, Instance, Memory, MemoryType, Module, SharedMemory, Store};

pub struct Plugin {
    name: String,
    instance: Instance,
}

pub struct Thread {
    store: Store,
    plugins: Vec<Plugin>,
}

pub struct Global {
    engine: Engine,
    store: Store,
    memory: SharedMemory,
}

impl Global {
    pub fn new() -> Self {
        let engine = Engine::default();
        let mut store = Store::new(engine.clone());
        let memory = Memory::new(&mut store, MemoryType::new(1, None, true)).unwrap();
        let memory = memory.as_shared(&store).unwrap();
        Self {
            engine,
            store,
            memory,
        }
    }

    pub fn engine(&self) -> Engine {
        self.engine.clone()
    }
}

impl Global {
    pub fn mount(&self, store: &mut Store) {
        self.memory
            .memory()
            .share_in_store(&self.store, store)
            .unwrap();
    }
}

impl Plugin {
    pub fn new(name: String, module: &Module, global: &Global) -> Plugin {
        let mut store = Store::new(global.engine.clone());
        let instance = Instance::new(&mut store, module, &imports! {}).unwrap();
        Plugin { name, instance }
    }

    pub fn load(path: impl AsRef<Path>) -> Self {
        todo!()
    }

    pub fn call(&self, store: &mut Store) {
        let f = self.instance.exports.get_function("thought_event").unwrap();
        f.call(store, &[]).unwrap();
    }
}

impl Thread {
    pub fn new(global: &Global, plugins: &[Plugin]) -> Self {
        let mut store = Store::new(global.engine());

        todo!()
    }

    pub fn run(self) {}
}

fn load_plugins() -> Vec<Plugin> {
    todo!()
}

fn init(plugins: &[Plugin], global: &Global) {
    let thread = Thread::new(global, plugins);
    thread.run();
}

pub fn lanuch() {
    let global = Global::new();
    let plugins = load_plugins();
    thread::scope(|s| {
        let handles = (0..5).map(|_| s.spawn(|| init(&plugins, &global)));

        for handle in handles {
            handle.join().unwrap();
        }
    });
}
