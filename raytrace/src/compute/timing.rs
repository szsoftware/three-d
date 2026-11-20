//!
//! A log of how long the GPU tracer takes to get going, see [set_gpu_timing_log].
//!

use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);

///
/// Writes how long the GPU tracer takes to get going: the background compile of the shaders in
/// [crate::ComputeTracer::from_context_async], how long the GPU is busy after the tracer is made, and for the first
/// calls of [crate::ComputeTracer::trace_gpu] the time on the CPU and until the GPU has done the work. Off by default;
/// for finding out what holds a slow GPU.
///
/// Where the application installed a logger, for example the log bridge of the cargo-uniffi plugin, the lines go
/// through the [log] crate at level info with the target `three_d_raytrace::gpu_timing`. Without one they go to the
/// console of the browser, outside of browsers to standard error.
///
pub fn set_gpu_timing_log(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

pub(crate) fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Milliseconds since some moment, for differences.
pub(crate) fn now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    return js_sys::Date::now();
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_secs_f64()
            * 1000.0
    }
}

pub(crate) fn log(message: &str) {
    // Without a logger the maximum level stays off
    if log::max_level() != log::LevelFilter::Off {
        log::info!(target: "three_d_raytrace::gpu_timing", "{message}");
        return;
    }
    #[cfg(target_arch = "wasm32")]
    console_log(&format!("three-d-raytrace: {message}"));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("three-d-raytrace: {message}");
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn console_log(message: &str);
}

/// Logs when the GPU has done all work queued so far, with the time since `start`.
pub(crate) fn when_gpu_done(queue: &three_d::wgpu::Queue, start: f64, what: String) {
    queue.on_submitted_work_done(move || {
        log(&format!("{what}: GPU done after {:.0} ms", now() - start))
    });
}
