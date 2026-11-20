//!
//! The GPU timing log goes through the logger of the application where one is installed. Its own test binary, since
//! the logger is global. Skipped without a wgpu adapter unless `THREE_D_REQUIRE_WGPU` is set.
//!
#![cfg(feature = "wgpu")]

use std::sync::Mutex;
use three_d_raytrace::*;

static LINES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // Only the tracer's lines, not those of wgpu and its drivers
        metadata.target().starts_with("three_d_raytrace")
    }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        LINES
            .lock()
            .unwrap()
            .push((record.target().to_owned(), record.args().to_string()));
    }
    fn flush(&self) {}
}

#[test]
fn the_gpu_timing_log_goes_through_an_installed_logger() {
    let context = match three_d::Context::new_headless_wgpu() {
        Ok(context) => context,
        Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
            panic!("no wgpu adapter: {error}")
        }
        Err(_) => return,
    };
    log::set_logger(&Capture).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    set_gpu_timing_log(true);
    let Some(mut tracer) = ComputeTracer::from_context(&context) else {
        return;
    };
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 1000.0,
    });
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(8, 8),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, -1.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        10.0,
    );
    let mut image = GpuRenderImage::new(tracer.gpu(), 8, 8);
    tracer
        .trace_gpu(&scene, &camera, &mut image, &TraceSettings::default())
        .unwrap();
    // Waits for the GPU, so its callbacks have run and dropping the context does not time out on slow adapters such
    // as WARP
    let _ = tracer
        .gpu()
        .device
        .poll(three_d::wgpu::PollType::wait_indefinitely());
    set_gpu_timing_log(false);
    let lines = LINES.lock().unwrap();
    assert!(
        lines
            .iter()
            .any(|(target, _)| target == "three_d_raytrace::gpu_timing"),
        "{lines:?}"
    );
}
