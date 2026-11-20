//!
//! Renders the garden scene with the compute tracer on the wgpu backend, without a window, and writes a PNG and an
//! OpenEXR file.
//!
//! cargo run --release -p three-d-raytrace --features wgpu --example garden_gpu -- [samples] [width] [height] [output name]
//!

use three_d_raytrace::scenes::garden;

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let width: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let height: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(400);
    let name = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| "garden_gpu".to_owned());

    let context = three_d::Context::new_headless_wgpu().expect("a wgpu adapter");
    let Some(mut tracer) = ComputeTracer::from_context(&context) else {
        eprintln!(
            "The GPU has no compute shaders: {:?}",
            RaytraceSupport::of(&context)
        );
        return;
    };
    let scene = garden::scene();
    let camera = garden::camera(width, height);
    let mut image = GpuRenderImage::with_features(tracer.gpu(), width, height);
    let start = std::time::Instant::now();
    tracer.prepare(&scene).unwrap();
    println!("Scene on the GPU after {:.2?}", start.elapsed());
    let settings = TraceSettings {
        samples_per_call: 8,
        ..Default::default()
    };
    while image.sample_count() < samples {
        tracer
            .trace_gpu(&scene, &camera, &mut image, &settings)
            .unwrap();
    }
    // Reading waits for the GPU
    let render_image = image.read();
    println!(
        "{} samples after {:.1?}",
        render_image.sample_count(),
        start.elapsed()
    );
    let tone_mapping = ToneMapping {
        exposure_ev100: 14.0,
        ..Default::default()
    };
    render_image
        .save_png(format!("{name}.png"), &tone_mapping)
        .unwrap();
    render_image.save_exr(format!("{name}.exr")).unwrap();
    AtrousDenoiser::default()
        .denoise(&render_image)
        .unwrap()
        .save_png(format!("{name}_denoised.png"), &tone_mapping)
        .unwrap();
    println!("Wrote {name}.png, {name}.exr and {name}_denoised.png");
}
