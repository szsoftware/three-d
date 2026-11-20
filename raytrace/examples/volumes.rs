//!
//! A pergola in a hazy garden in the evening sun, rendered with the CPU reference tracer: sunbeams between the slats of
//! its roof in the haze over the scene, a basin of water that darkens and colors with depth, and a ball of smoke.
//!
//! cargo run --release -p three-d-raytrace --example volumes -- [samples] [width] [height]
//!

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let width: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let height: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(400);

    let scene = scenes::pergola::scene();
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        vec3(5.2, 1.4, 4.6),
        vec3(-0.6, 1.3, -0.2),
        vec3(0.0, 1.0, 0.0),
        degrees(50.0),
        0.05,
        200.0,
    );
    let tone_mapping = ToneMapping {
        exposure_ev100: 13.5,
        ..Default::default()
    };
    let mut tracer = CpuReferenceTracer::new();
    let mut image = RenderImage::with_features(width, height);
    let settings = TraceSettings {
        samples_per_call: 1,
        max_bounces: 6,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    while image.sample_count() < samples {
        tracer
            .trace(&scene, &camera, &mut image, &settings)
            .unwrap();
    }
    println!("volumes: {samples} samples in {:.1?}", start.elapsed());
    image.save_png("volumes.png", &tone_mapping).unwrap();
    AtrousDenoiser::default()
        .denoise(&image)
        .unwrap()
        .save_png("volumes_denoised.png", &tone_mapping)
        .unwrap();
}
