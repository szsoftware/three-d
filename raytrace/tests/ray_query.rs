//!
//! The compute tracer with hardware ray queries against the same tracer walking its hierarchies in the shader. Both
//! draw the same random numbers, so the images agree pixel by pixel up to where the two triangle tests disagree in the
//! last bits. Skipped on devices without ray queries unless `THREE_D_REQUIRE_RAY_QUERY` is set (lavapipe has them).
//!
#![cfg(feature = "wgpu")]

mod common;

use three_d_raytrace::*;

/// The tracer walking the hierarchies and the one with ray queries, on the same device.
fn tracers() -> Option<(ComputeTracer, ComputeTracer)> {
    // Before the context: three-d asks the device for ray queries only on request
    std::env::set_var("THREE_D_RAY_QUERY", "1");
    let required = std::env::var("THREE_D_REQUIRE_RAY_QUERY").is_ok();
    let context = match common::headless_wgpu() {
        Ok(context) => context,
        Err(error) if required => panic!("no wgpu adapter: {error}"),
        Err(error) => {
            eprintln!("skipped, no wgpu adapter: {error}");
            return None;
        }
    };
    // WARP, the software DirectX 12 of the Windows runners, needs minutes for each build of the tracer's shaders
    let warp = context.wgpu_adapter_info().is_some_and(|info| {
        info.backend == three_d::wgpu::Backend::Dx12
            && info.device_type == three_d::wgpu::DeviceType::Cpu
    });
    if warp && !required {
        eprintln!("skipped on WARP");
        return None;
    }
    let with = ComputeTracer::from_context(&context)?;
    if !with.uses_ray_queries() {
        assert!(!required, "the device has no ray queries");
        eprintln!("skipped, the device has no ray queries");
        return None;
    }
    let without = ComputeTracer::from_context(&context)?.with_ray_queries(false);
    assert!(!without.uses_ray_queries());
    Some((without, with))
}

fn render(
    tracer: &mut ComputeTracer,
    demo: &scenes::DemoScene,
    size: (u32, u32),
    seed: u32,
) -> (RenderImage, f64) {
    let mut render_image = RenderImage::new(size.0, size.1);
    let settings = TraceSettings {
        samples_per_call: 16,
        seed,
        ..demo.settings
    };
    let camera = demo.camera(size.0, size.1);
    // The first call builds the shaders and structures, the second is timed
    tracer
        .trace(
            &demo.scene,
            &camera,
            &mut RenderImage::new(size.0, size.1),
            &TraceSettings {
                samples_per_call: 1,
                ..settings
            },
        )
        .unwrap();
    let start = std::time::Instant::now();
    tracer
        .trace(&demo.scene, &camera, &mut render_image, &settings)
        .unwrap();
    (render_image, start.elapsed().as_secs_f64())
}

/// Averages of 6 x 6 pixel blocks of two images: the mean and the largest difference of the blocks in sRGB steps.
fn block_differences(a: &[[u8; 4]], b: &[[u8; 4]], width: u32, height: u32) -> (f64, f64) {
    let block = |data: &[[u8; 4]], bx: u32, by: u32, c: usize| -> f64 {
        let mut sum = 0.0;
        for y in by * 6..by * 6 + 6 {
            for x in bx * 6..bx * 6 + 6 {
                sum += data[(y * width + x) as usize][c] as f64;
            }
        }
        sum / 36.0
    };
    let (mut worst, mut total, mut count) = (0.0f64, 0.0, 0);
    for by in 0..height / 6 {
        for bx in 0..width / 6 {
            for c in 0..3 {
                let d = (block(a, bx, by, c) - block(b, bx, by, c)).abs();
                worst = worst.max(d);
                total += d;
                count += 1;
            }
        }
    }
    (total / count as f64, worst)
}

/// Share of the pixels that differ by more than a few steps in a channel.
fn pixels_off(a: &[[u8; 4]], b: &[[u8; 4]]) -> f64 {
    let off = a
        .iter()
        .zip(b)
        .filter(|(p, q)| (0..3).any(|c| p[c].abs_diff(q[c]) > 8))
        .count();
    off as f64 / a.len() as f64
}

/// With `THREE_D_RAY_QUERY_IMAGES` set to a folder, the images are saved there.
fn save(name: &str, data: &[[u8; 4]], size: (u32, u32)) {
    let Ok(folder) = std::env::var("THREE_D_RAY_QUERY_IMAGES") else {
        return;
    };
    let bytes: Vec<u8> = data.iter().flatten().copied().collect();
    image::RgbaImage::from_raw(size.0, size.1, bytes)
        .unwrap()
        .save(std::path::Path::new(&folder).join(format!("{name}.png")))
        .unwrap();
}

#[test]
fn ray_queries_give_the_same_images_as_the_walk_in_the_shader() {
    let Some((mut without, mut with)) = tracers() else {
        return;
    };
    let size = (96, 60);
    let mut failed = Vec::new();
    for demo in scenes::all() {
        let to8 = |i: &RenderImage| i.to_srgb8(&demo.tone_mapping);
        let (a, time_a) = render(&mut without, &demo, size, 0);
        let (b, time_b) = render(&mut with, &demo, size, 0);
        // The noise: the same tracer with other random numbers
        let (c, _) = render(&mut without, &demo, size, 1);
        let (a, b, c) = (to8(&a), to8(&b), to8(&c));
        save(&format!("{}-shader", demo.name), &a, size);
        save(&format!("{}-ray-query", demo.name), &b, size);
        let (average, worst) = block_differences(&a, &b, size.0, size.1);
        let (noise_average, noise_worst) = block_differences(&a, &c, size.0, size.1);
        let off = pixels_off(&a, &b);
        eprintln!(
            "{}: blocks differ by {average:.2} on average, {worst:.1} at most (other random numbers: {noise_average:.2}, \
             {noise_worst:.1}), {:.2} % of the pixels off; {:.0} ms in the shader, {:.0} ms with ray queries",
            demo.name,
            100.0 * off,
            1000.0 * time_a,
            1000.0 * time_b
        );
        // Where paths took other ways the images differ by noise, never by more. The aquarium has solids whose
        // surfaces lie exactly on each other (the front walls of the tank and its floor plate share their front
        // face): which of them a ray enters first is decided by rounding, and the hardware's triangle test rounds
        // otherwise than the shader's, so the strip of the floor plate looks otherwise there
        let agrees = if demo.name == "aquarium" {
            average <= noise_average && off < 0.06
        } else {
            average <= 0.5 * noise_average + 0.05 && worst <= noise_worst + 1.0
        };
        if !agrees {
            failed.push(demo.name);
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn a_moved_instance_moves_with_ray_queries() {
    let Some((mut without, mut with)) = tracers() else {
        return;
    };
    let mut demo = scenes::garden::demo();
    let size = (48, 30);
    let (before, _) = render(&mut with, &demo, size, 0);
    // Moving the first instance builds the structures again
    let instance = InstanceId(1);
    let transform = demo.scene.instances()[1].transform;
    demo.scene
        .set_transform(
            instance,
            Mat4::from_translation(vec3(0.0, 0.5, 0.0)) * transform,
        )
        .unwrap();
    let (a, _) = render(&mut without, &demo, size, 0);
    let (b, _) = render(&mut with, &demo, size, 0);
    let to8 = |i: &RenderImage| i.to_srgb8(&demo.tone_mapping);
    let (before, a, b) = (to8(&before), to8(&a), to8(&b));
    assert!(pixels_off(&before, &b) > 0.01, "the image did not change");
    assert!(
        pixels_off(&a, &b) < 0.02,
        "{:.2} % of the pixels differ",
        100.0 * pixels_off(&a, &b)
    );
}
