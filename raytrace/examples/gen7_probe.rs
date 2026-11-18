//!
//! DIAGNOSIS ONLY (experiment branch): one run of the compute tracer for the GPU hang on Intel Gen7.
//!
//! cargo run --release -p three-d-raytrace --features wgpu --example gen7_probe -- <samples_per_call> <features 0|1> [calls] [width] [height]
//!
//! Exits 0 when the image is read back with finite values. Run each case in its own process: after a GPU hang the
//! device is lost.
//!

#[path = "../scenes/garden.rs"]
mod garden;

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize, d: u32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let samples_per_call = arg(1, 1);
    let features = arg(2, 0) == 1;
    let calls = arg(3, 4);
    let width = arg(4, 160);
    let height = arg(5, 100);
    let context = three_d::Context::new_headless_wgpu().expect("a wgpu adapter");
    let (device, _) = context.as_wgpu().unwrap();
    let info = device.adapter_info();
    println!(
        "adapter: {} / {} ({:?})",
        info.name, info.driver_info, info.backend
    );
    let mut tracer = ComputeTracer::from_context(&context).expect("compute shaders");
    let scene = garden::scene();
    let camera = garden::camera(width, height);
    let mut image = if features {
        GpuRenderImage::with_features(tracer.gpu(), width, height)
    } else {
        GpuRenderImage::new(tracer.gpu(), width, height)
    };
    tracer.prepare(&scene).unwrap();
    let settings = TraceSettings {
        samples_per_call,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    for _ in 0..calls {
        tracer
            .trace_gpu(&scene, &camera, &mut image, &settings)
            .unwrap();
    }
    let render_image = image.read();
    let elapsed = start.elapsed();
    let pixels = render_image.pixels();
    let finite = pixels
        .iter()
        .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
    let mean: f32 =
        pixels.iter().map(|p| p.x + p.y + p.z).sum::<f32>() / (3.0 * pixels.len() as f32);
    println!(
        "RESULT spc={samples_per_call} features={features} samples={} time={elapsed:.2?} finite={finite} mean={mean:.4}",
        render_image.sample_count()
    );
    if !finite {
        std::process::exit(2);
    }
}
