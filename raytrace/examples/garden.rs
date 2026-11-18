//!
//! Renders the garden scene with the CPU reference tracer and writes a PNG and an OpenEXR file.
//!
//! cargo run --release -p three-d-raytrace --example garden -- [samples] [width] [height] [output name]
//!

#[path = "../scenes/garden.rs"]
mod garden;

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let width: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let height: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(400);
    let name = args.get(4).cloned().unwrap_or_else(|| "garden".to_owned());

    let scene = garden::scene();
    let camera = garden::camera(width, height);
    let sun = garden::sky().sun_position();
    println!(
        "Sun at azimuth {:.1}°, elevation {:.1}°, {} lux",
        sun.azimuth,
        sun.elevation,
        garden::sky().sun_illuminance().round()
    );

    let mut tracer = CpuReferenceTracer::new();
    let mut film = Film::new(width, height);
    let start = std::time::Instant::now();
    tracer.prepare(&scene).unwrap();
    println!("Acceleration structure built in {:.2?}", start.elapsed());
    let settings = TraceSettings {
        samples_per_call: 8,
        ..Default::default()
    };
    while film.sample_count() < samples {
        tracer.trace(&scene, &camera, &mut film, &settings).unwrap();
        println!(
            "{} samples after {:.1?}",
            film.sample_count(),
            start.elapsed()
        );
    }
    let tone_mapping = ToneMapping {
        exposure_ev100: 14.0,
        ..Default::default()
    };
    film.save_png(format!("{name}.png"), &tone_mapping).unwrap();
    film.save_exr(format!("{name}.exr")).unwrap();
    println!("Wrote {name}.png and {name}.exr");
}
