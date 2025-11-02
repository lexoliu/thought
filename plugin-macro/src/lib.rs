extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, ItemFn, Lit, Meta, Token};

/// Sets up the necessary memory management functions (`alloc` and `dealloc`)
/// that the host calls to interact with the Wasm module's memory.
///
/// This should be called once in the main `lib.rs` of your plugin crate.
#[proc_macro]
pub fn setup_thought_plugin(_input: TokenStream) -> TokenStream {
    TokenStream::from(quote! {
        /// Allocates memory of a given size in the Wasm module and returns a pointer.
        /// This is called by the host to create a buffer for input data.
        #[no_mangle]
        pub unsafe extern "C" fn alloc(size: u32) -> *mut u8 {
            let mut buf = Vec::with_capacity(size as usize);
            let ptr = buf.as_mut_ptr();
            std::mem::forget(buf); // Prevent Rust from freeing the memory
            ptr
        }

        /// Deallocates memory that was previously allocated with `alloc`.
        /// Called by the host to free memory for both input and output buffers.
        #[no_mangle]
        pub unsafe extern "C" fn dealloc(ptr: *mut u8, size: u32) {
            unsafe {
                let _ = Vec::from_raw_parts(ptr, 0, size as usize);
            }
        }
    })
}

/// Wraps a Rust function to be a Wasm-exported entry point for Thought.
///
/// It handles the boilerplate for data serialization between the host and the Wasm guest.
///
/// # Attributes
///
/// - `export_as`: The name of the function to be exported from the Wasm module.
///   (e.g., `"generate_page"`, `"generate_index"`)
#[proc_macro_attribute]
pub fn thought_plugin_entry(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let func_name = &func.sig.ident;
    let func_generics = &func.sig.generics;
    let func_inputs = &func.sig.inputs;
    // Parse the attribute to get the exported function name
    let attrs = parse_macro_input!(attr with Punctuated::<Meta, Token![,]>::parse_terminated);
    let export_name_str = match attrs.first() {
        Some(Meta::NameValue(nv)) if nv.path.is_ident("export_as") => {
            if let syn::Expr::Lit(expr_lit) = &nv.value {
                if let Lit::Str(lit_str) = &expr_lit.lit {
                    lit_str.value()
                } else {
                    panic!("'export_as' value must be a string literal");
                }
            } else {
                panic!("'export_as' value must be a string literal");
            }
        }
        _ => panic!(
            "Attribute must be in the form #[thought_plugin_entry(export_as = \"function_name\")]"
        ),
    };
    let export_name = syn::Ident::new(&export_name_str, func_name.span());

    let input_type = if let Some(syn::FnArg::Typed(pt)) = func_inputs.first() {
        &pt.ty
    } else {
        panic!("The function must take one argument.");
    };

    let user_func_body = &func.block;

    let expanded = quote! {
        #[no_mangle]
        pub unsafe extern "C" fn #export_name #func_generics (ptr: *mut u8, len: u32) -> u64 {
            let #func_name = |#func_inputs| #user_func_body;

            // Reconstruct the input data from the pointer and length
            let data = unsafe { Vec::from_raw_parts(ptr, len as usize, len as usize) };
            let input: #input_type = bincode::deserialize(&data).expect("Failed to deserialize input data");

            // Call the user's actual render function
            let result_string = #func_name(input);

            // Serialize the result string to bytes
            let result_bytes = result_string.into_bytes();
            let result_len = result_bytes.len() as u32;
            let result_ptr = result_bytes.as_ptr() as u32;

            // Prevent Rust from freeing the result buffer
            std::mem::forget(result_bytes);

            // Combine pointer and length into a single u64
            ((result_ptr as u64) << 32) | (result_len as u64)
        }
    };

    TokenStream::from(expanded)
}
