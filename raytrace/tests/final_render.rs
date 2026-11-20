//!
//! The final render in print size: bands that join without a seam, upright on the CPU and on the GPU, progress and
//! PNG. The GPU parts are skipped without a wgpu adapter unless `THREE_D_REQUIRE_WGPU` is set.
//!
#![cfg(feature = "three-d")]

mod common;

use three_d_raytrace::*;

/// Bright sky above, a dark ground below and a light grey box in the middle, which the bands cut through.
fn scene() -> RtScene {
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 5000.0,
    });
    let dark = scene.add_material(RtMaterial::diffuse(vec3(0.05, 0.05, 0.05)));
    let grey = scene.add_material(RtMaterial::diffuse(vec3(0.6, 0.6, 0.6)));
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
    scene
        .add_instance(
            square,
            dark,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(1000.0),
        )
        .unwrap();
    scene
        .add_instance(
            cube,
            grey,
            Mat4::from_translation(vec3(0.0, 1.0, -4.0)) * Mat4::from_scale(0.6),
        )
        .unwrap();
    scene
}

fn camera() -> Camera {
    // The final render gives the camera its own viewport
    Camera::new_perspective(
        Viewport::new_at_origo(4, 3),
        vec3(0.0, 1.0, 0.0),
        vec3(0.0, 1.0, -1.0),
        vec3(0.0, 1.0, 0.0),
        degrees(60.0),
        0.1,
        100.0,
    )
}

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;
const TONE: ToneMapping = ToneMapping {
    exposure_ev100: 13.0,
    operator: ToneOperator::Aces,
};

fn render(samples: u32, band_pixels: u32, denoiser: Option<AtrousDenoiser>) -> FinalRender {
    FinalRender::new(&camera(), WIDTH, HEIGHT, samples)
        .with_tone_mapping(TONE)
        .with_denoiser(denoiser)
        .with_band_pixels(band_pixels)
        .with_frame_time(std::time::Duration::from_millis(5))
}

fn on_cpu(mut render: FinalRender) -> FinalRender {
    let scene = scene();
    let mut progress = 0.0;
    while !render.trace_cpu(&scene, 2).unwrap() {
        assert!(render.progress() >= progress, "the progress goes back");
        progress = render.progress();
        assert!(render.rgba8().is_none());
    }
    assert_eq!(render.progress(), 1.0);
    render
}

fn mean_difference(a: &[[u8; 4]], b: &[[u8; 4]]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum: f64 = a
        .iter()
        .zip(b)
        .map(|(a, b)| {
            (0..3)
                .map(|c| (a[c] as f64 - b[c] as f64).abs())
                .sum::<f64>()
                / 3.0
        })
        .sum();
    sum / a.len() as f64
}

fn mean_green(pixels: &[[u8; 4]], rows: std::ops::Range<u32>) -> f64 {
    let rows = &pixels[(rows.start * WIDTH) as usize..(rows.end * WIDTH) as usize];
    rows.iter().map(|p| p[1] as f64).sum::<f64>() / rows.len() as f64
}

fn check_upright(pixels: &[[u8; 4]], name: &str) {
    let top = mean_green(pixels, 0..4);
    let bottom = mean_green(pixels, HEIGHT - 4..HEIGHT);
    assert!(top > bottom + 50.0, "{name}: top {top} bottom {bottom}");
}

#[test]
fn the_first_band_is_the_top_of_the_whole_image() {
    // Without a denoiser the bands have no margin; the first one has the random numbers of the whole image
    let whole = on_cpu(render(4, u32::MAX, None));
    let banded = on_cpu(render(4, WIDTH * 16, None));
    let rows = (16 * WIDTH) as usize;
    let (whole, banded) = (whole.rgba8().unwrap(), banded.rgba8().unwrap());
    let largest = whole[..rows]
        .iter()
        .zip(&banded[..rows])
        .flat_map(|(a, b)| (0..3).map(move |c| a[c].abs_diff(b[c])))
        .max()
        .unwrap();
    assert!(largest <= 2, "the first band differs by {largest}");
    check_upright(whole, "whole");
    check_upright(banded, "banded");
}

#[test]
fn denoised_bands_join_without_a_seam() {
    let denoiser = Some(AtrousDenoiser::default());
    let whole = on_cpu(render(16, u32::MAX, denoiser));
    // Bands of 16 rows; the denoiser's margin traces more rows around each
    let banded = on_cpu(render(16, WIDTH * 16, denoiser));
    let (whole, banded) = (whole.rgba8().unwrap(), banded.rgba8().unwrap());
    let difference = mean_difference(whole, banded);
    assert!(difference < 4.0, "mean difference {difference}");
    // Around the seams no worse than elsewhere
    for seam in [16, 32] {
        let rows = ((seam - 2) * WIDTH) as usize..((seam + 2) * WIDTH) as usize;
        let at_seam = mean_difference(&whole[rows.clone()], &banded[rows]);
        assert!(at_seam < 8.0, "mean difference {at_seam} at row {seam}");
    }
    check_upright(banded, "denoised");
}

#[cfg(feature = "export")]
#[test]
fn the_final_render_is_a_png() {
    let render = on_cpu(render(1, u32::MAX, None));
    let png = render.png().unwrap();
    let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (WIDTH, HEIGHT));
    assert_eq!(
        decoded.get_pixel(3, 5).0,
        render.rgba8().unwrap()[(5 * WIDTH + 3) as usize]
    );
}

#[test]
fn the_print_sizes_have_their_aspect() {
    let (w, h) = A4_LANDSCAPE_300_DPI;
    assert!((w as f64 / h as f64 - 2f64.sqrt()).abs() < 0.001);
    let render = FinalRender::new(&camera(), w, h, 64);
    assert_eq!(render.camera().viewport().width, w);
    assert_eq!(render.camera().viewport().height, h);
    assert_eq!(render.progress(), 0.0);
    assert!(render.remaining().is_none());
}

#[test]
fn every_demo_scene_traces() {
    let demos = scenes::all();
    let names: Vec<_> = demos.iter().map(|d| d.name).collect();
    assert_eq!(
        names,
        ["garden", "pergola", "aquarium", "caustics", "downlights"]
    );
    for demo in demos {
        let mut image = RenderImage::new(8, 6);
        let settings = TraceSettings {
            caustic_photons: demo.settings.caustic_photons.min(1000),
            ..demo.settings
        };
        CpuReferenceTracer::with_threads(2)
            .trace(&demo.scene, &demo.camera(8, 6), &mut image, &settings)
            .unwrap();
        let pixels = image.to_srgb8(&demo.tone_mapping);
        let lit = pixels
            .iter()
            .filter(|p| p[0].max(p[1]).max(p[2]) > 10)
            .count();
        assert!(lit > 0, "{} is black", demo.name);
    }
}

#[cfg(feature = "wgpu")]
mod gpu {
    use super::*;
    use three_d::Context;

    fn context() -> Option<Context> {
        match common::headless_wgpu() {
            Ok(context) => Some(context),
            Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
                panic!("no wgpu adapter: {error}")
            }
            Err(error) => {
                eprintln!("skipped, no wgpu adapter: {error}");
                None
            }
        }
    }

    fn on_gpu(preview: &mut RaytracePreview, mut render: FinalRender) -> FinalRender {
        let scene = scene();
        let mut progress = 0.0;
        for _ in 0..100_000 {
            if preview.trace_final(&scene, &mut render).unwrap() {
                break;
            }
            assert!(render.progress() >= progress, "the progress goes back");
            progress = render.progress();
        }
        assert!(render.is_done());
        render
    }

    #[test]
    fn the_gpu_renders_the_same_bands_as_the_cpu() {
        let Some(context) = context() else { return };
        let mut preview = RaytracePreview::new(&context, 16, 12);
        preview.wait_for_gpu();
        if preview.kind() != TracerKind::Compute {
            eprintln!("skipped, the preview runs on the CPU");
            return;
        }
        let denoiser = Some(AtrousDenoiser::default());
        let cpu = on_cpu(render(16, WIDTH * 16, denoiser));
        for band_pixels in [u32::MAX, WIDTH * 16] {
            let gpu = on_gpu(&mut preview, render(16, band_pixels, denoiser));
            let (cpu, gpu) = (cpu.rgba8().unwrap(), gpu.rgba8().unwrap());
            check_upright(gpu, "gpu");
            let difference = mean_difference(cpu, gpu);
            assert!(
                difference < 5.0,
                "mean difference {difference} with bands of {band_pixels} pixels"
            );
        }
        // Without a denoiser too
        let gpu = on_gpu(&mut preview, render(4, WIDTH * 16, None));
        check_upright(gpu.rgba8().unwrap(), "gpu without denoiser");
        // The preview goes on afterwards
        preview
            .update(&scene(), &camera())
            .expect("the preview goes on");
    }

    /// The preview's image as shown, top row first.
    fn shown(context: &Context, preview: &RaytracePreview) -> Vec<[u8; 4]> {
        let target = three_d::RenderTarget::screen(context, 32, 24);
        target.clear(three_d::ClearState::color(0.0, 0.0, 0.0, 1.0));
        preview.render(&target);
        target.read_color()
    }

    #[test]
    fn the_preview_stays_on_screen_while_the_final_render_traces() {
        let context = match Context::new_headless_wgpu_with_screen(32, 24, true, false) {
            Ok(context) => context,
            Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
                panic!("no wgpu adapter: {error}")
            }
            Err(_) => return,
        };
        let scene = scene();
        let mut camera = camera();
        camera.set_viewport(Viewport::new_at_origo(32, 24));
        // On the GPU, and on the CPU with its worker thread as on the desktop
        for mut preview in [
            RaytracePreview::new(&context, 32, 24),
            RaytracePreview::new_on_cpu(&context, 32, 24),
        ] {
            preview.wait_for_gpu();
            let kind = preview.kind();
            for _ in 0..10_000 {
                preview.update(&scene, &camera).unwrap();
                if preview.sample_count() >= 2 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let before = shown(&context, &preview);
            check_upright_at(&before, 32, 24, &format!("{kind:?} before"));
            let mut print = render(2, WIDTH * 16, None);
            let mut steps = 0;
            while !preview.trace_final(&scene, &mut print).unwrap() {
                steps += 1;
                if steps % 5 == 1 {
                    let during = shown(&context, &preview);
                    check_upright_at(&during, 32, 24, &format!("{kind:?} during"));
                }
            }
            check_upright_at(
                &shown(&context, &preview),
                32,
                24,
                &format!("{kind:?} after"),
            );
        }
    }

    fn check_upright_at(pixels: &[[u8; 4]], width: u32, height: u32, name: &str) {
        let green = |rows: std::ops::Range<u32>| {
            let rows = &pixels[(rows.start * width) as usize..(rows.end * width) as usize];
            rows.iter().map(|p| p[1] as f64).sum::<f64>() / rows.len() as f64
        };
        let (top, bottom) = (green(0..3), green(height - 3..height));
        assert!(top > bottom + 50.0, "{name}: top {top} bottom {bottom}");
    }
}
