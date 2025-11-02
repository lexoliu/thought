use std::thread;

use wasmer::{imports, Instance, Module, Store, Value};

fn test() {
    let module_wat = r#"
    (module
      (type $t0 (func (param i32) (result i32)))
      (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
        get_local $p0
        i32.const 1
        i32.add))
    "#;

    let mut store = Store::default();
    let module = Module::new(&store, &module_wat).unwrap();
    // The module doesn't import anything, so we create an empty import object.
    let import_object = imports! {};
    let instance = Instance::new(&mut store, &module, &import_object).unwrap();

    thread::spawn(move || {
        let add_one = instance.exports.get_function("add_one").unwrap();

        let result = add_one.call(&mut store, &[Value::I32(42)]).unwrap();
        assert_eq!(result[0], Value::I32(43));
    });
}
