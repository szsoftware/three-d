//!
//! Measures the compute tracer with and without hardware ray queries on the demo scenes, on the same device, and
//! writes both images of each scene as PNG. For graphics cards with ray tracing units (Vulkan, DirectX 12, Metal).
//!
//! cargo run --release -p three-d-raytrace --features wgpu --example ray_query_bench -- [samples] [width] [height] [scene]
//!
//! The device gets ray queries only on request, which this example makes itself (THREE_D_RAY_QUERY=1).
//!

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let width: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(960);
    let height: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(600);
    let only = args.get(4).cloned();

    std::env::set_var("THREE_D_RAY_QUERY", "1");
    let context = three_d::Context::new_headless_wgpu().expect("a wgpu adapter");
    if let Some(info) = context.wgpu_adapter_info() {
        println!(
            "{} ({:?}, {}, {})",
            info.name, info.backend, info.driver, info.driver_info
        );
    }
    let Some(with) = ComputeTracer::from_context(&context) else {
        eprintln!(
            "The GPU has no compute shaders: {:?}",
            RaytraceSupport::of(&context)
        );
        return;
    };
    if !with.uses_ray_queries() {
        eprintln!("The device has no hardware ray queries, only the compute tracer is measured");
    }
    let without = ComputeTracer::from_context(&context)
        .expect("the same device")
        .with_ray_queries(false);
    let mut tracers = vec![("shader", without)];
    if with.uses_ray_queries() {
        tracers.push(("ray-query", with));
    }

    println!("{samples} samples at {width}x{height}");
    for demo in scenes::all() {
        if only.as_deref().is_some_and(|name| name != demo.name) {
            continue;
        }
        let camera = demo.camera(width, height);
        let mut times = Vec::new();
        for (name, tracer) in &mut tracers {
            let settings = TraceSettings {
                samples_per_call: 1,
                ..demo.settings
            };
            // The first sample builds the shaders and the scene's structures, it is not measured
            let mut image = GpuRenderImage::new(tracer.gpu(), width, height);
            let start = std::time::Instant::now();
            tracer
                .trace_gpu(&demo.scene, &camera, &mut image, &settings)
                .unwrap();
            image.read();
            let first = start.elapsed();
            image.reset();
            let start = std::time::Instant::now();
            while image.sample_count() < samples {
                tracer
                    .trace_gpu(
                        &demo.scene,
                        &camera,
                        &mut image,
                        &TraceSettings {
                            samples_per_call: 4,
                            ..settings
                        },
                    )
                    .unwrap();
            }
            // Reading waits for the GPU
            let render_image = image.read();
            let elapsed = start.elapsed().as_secs_f64();
            if let Some(error) = tracer.gpu_error() {
                eprintln!("{}: the GPU failed with {name}: {error}", demo.name);
            }
            render_image
                .save_png(format!("{}-{name}.png", demo.name), &demo.tone_mapping)
                .unwrap();
            println!(
                "{:>10} {:>9}: {:7.1} ms per sample, {:5.2} M camera paths/s, first sample with setup {:.0?}",
                demo.name,
                name,
                1000.0 * elapsed / render_image.sample_count() as f64,
                (width * height * render_image.sample_count()) as f64 / elapsed / 1e6,
                first
            );
            times.push(elapsed);
        }
        if let [a, b] = times[..] {
            println!("{:>10} {:>9}: {:.2}x", demo.name, "speedup", a / b);
        }
    }
}
