//!
//! Renders one of the demo scenes ([scenes::all]) in print size with [FinalRender], on the GPU of a context without a
//! window or on the CPU, and writes it as PNG.
//!
//! cargo run --release -p three-d-raytrace --features wgpu --example print -- [scene] [samples] [width] [height]
//!
//! The scene is one of garden, pergola, aquarium, caustics and downlights; the size is A4 in landscape at 300 dpi by
//! default.
//!

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let name = args.get(1).map(String::as_str).unwrap_or("garden");
    let samples: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(256);
    let width: u32 = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(A4_LANDSCAPE_300_DPI.0);
    let height: u32 = args
        .get(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(A4_LANDSCAPE_300_DPI.1);
    let Some(demo) = scenes::all().into_iter().find(|d| d.name == name) else {
        eprintln!("No scene {name}; there are garden, pergola, aquarium, caustics and downlights.");
        return;
    };

    let context = three_d::Context::new_headless_wgpu().expect("a wgpu adapter");
    let mut preview = RaytracePreview::new(&context, 64, 64);
    preview.wait_for_gpu();
    println!(
        "{}: {:?} on {:?}",
        demo.title,
        (width, height),
        preview.kind()
    );
    // The camera's viewport is only for its aspect; the final render gives it the print size
    let mut render = FinalRender::new(&demo.camera(width, height), width, height, samples)
        .with_settings(demo.settings)
        .with_tone_mapping(demo.tone_mapping)
        .with_frame_time(std::time::Duration::from_millis(200));
    let mut shown = 0;
    while !preview.trace_final(&demo.scene, &mut render).unwrap() {
        let percent = (render.progress() * 100.0) as u32;
        if percent >= shown + 5 {
            shown = percent;
            println!(
                "{percent}% after {:.0?}, {:.0?} to go",
                render.elapsed(),
                render.remaining().unwrap_or_default()
            );
        }
    }
    let file = format!("{name}_{width}x{height}.png");
    std::fs::write(&file, render.png().expect("complete")).unwrap();
    println!("{file} after {:.1?}", render.elapsed());
}
