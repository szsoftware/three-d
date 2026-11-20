//!
//! Media inside media with the CPU reference tracer: a glass tank of water with two ice cubes floating in it, the water
//! a little larger than the inside of the tank, the glass winning there by its priority; next to it a pane of glass
//! lying on the floor without a gap.
//!
//! cargo run --release -p three-d-raytrace --example nested_media -- [samples] [width] [height]
//!

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let width: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let height: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(400);

    let scene = scenes::aquarium::scene();
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        vec3(0.42, 0.3, 0.55),
        vec3(0.02, 0.09, 0.05),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.01,
        100.0,
    );
    let tone_mapping = ToneMapping {
        exposure_ev100: 14.6,
        ..Default::default()
    };
    let mut tracer = CpuReferenceTracer::new();
    let mut image = RenderImage::with_features(width, height);
    let settings = TraceSettings {
        samples_per_call: 1,
        max_bounces: 12,
        caustic_photons: 100_000,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    while image.sample_count() < samples {
        tracer
            .trace(&scene, &camera, &mut image, &settings)
            .unwrap();
    }
    println!("nested_media: {samples} samples in {:.1?}", start.elapsed());
    image.save_png("nested_media.png", &tone_mapping).unwrap();
    AtrousDenoiser::default()
        .denoise(&image)
        .unwrap()
        .save_png("nested_media_denoised.png", &tone_mapping)
        .unwrap();
}
