//!
//! Three downlights with a measured light distribution in front of a wall, rendered with the CPU reference tracer:
//! the scallops on the wall come from the IES profile.
//!
//! cargo run --release -p three-d-raytrace --example luminaires -- [samples] [width] [height]
//!

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let width: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let height: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(400);

    let scene = scenes::downlights::scene();
    let profile = IesProfile::parse(scenes::downlights::DOWNLIGHT).unwrap();
    println!("The profile sends out {:.0} lm", profile.flux());
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        vec3(0.0, 1.5, 4.5),
        vec3(0.0, 1.3, -1.5),
        vec3(0.0, 1.0, 0.0),
        degrees(50.0),
        0.05,
        100.0,
    );
    let mut tracer = CpuReferenceTracer::new();
    let mut image = RenderImage::with_features(width, height);
    let settings = TraceSettings {
        samples_per_call: 8,
        max_bounces: 6,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    while image.sample_count() < samples {
        tracer
            .trace(&scene, &camera, &mut image, &settings)
            .unwrap();
    }
    println!("{samples} samples in {:.1?}", start.elapsed());
    let tone_mapping = ToneMapping {
        exposure_ev100: 6.0,
        ..Default::default()
    };
    AtrousDenoiser::default()
        .denoise(&image)
        .unwrap()
        .save_png("luminaires.png", &tone_mapping)
        .unwrap();
    println!("Wrote luminaires.png");
}
