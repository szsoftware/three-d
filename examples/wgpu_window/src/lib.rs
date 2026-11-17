#![allow(special_module_name)]
mod main;

// Entry point for wasm
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_log::init_with_level(log::Level::Debug).unwrap();

    use log::info;
    info!("Logging works!");

    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    // A browser can only wait for WebGPU asynchronously
    wasm_bindgen_futures::spawn_local(async {
        let window = three_d::Window::new_async(main::settings(), three_d::BackendPreference::Auto)
            .await
            .unwrap();
        // WebGPU if the browser supports it, otherwise WebGL2
        if window.gl().as_gl().is_some() {
            info!("rendering with WebGL");
        } else {
            info!("rendering with WebGPU");
        }
        main::run(window);
    });
    Ok(())
}
