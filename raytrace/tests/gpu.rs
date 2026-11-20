//!
//! The ray tracing crate together with three-d's wgpu backend. Skipped without a wgpu adapter unless
//! `THREE_D_REQUIRE_WGPU` is set.
//!
#![cfg(feature = "wgpu")]

use three_d::Context;
use three_d_raytrace::*;

fn context() -> Option<Context> {
    match Context::new_headless_wgpu() {
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

/// A render image with a different value in every pixel: its index from the top left.
fn numbered_image(width: u32, height: u32) -> RenderImage {
    let mut render_image = RenderImage::new(width, height);
    let sums: Vec<[f32; 3]> = (0..width * height).map(|i| [i as f32; 3]).collect();
    render_image.accumulate(&sums, 1);
    render_image
}

#[test]
fn compute_is_supported_on_wgpu() {
    let Some(context) = context() else { return };
    let support = RaytraceSupport::of(&context);
    // Software adapters (lavapipe, WARP) and every desktop GPU have compute shaders
    if std::env::var("WGPU_BACKEND").as_deref() != Ok("gl") {
        assert!(support >= RaytraceSupport::Compute, "{support:?}");
    }
    assert!(GpuDevice::from_context(&context).is_some());
}

#[test]
fn render_image_shows_upright_in_three_d() {
    let Some(context) = context() else { return };
    let render_image = numbered_image(3, 2);
    let texture = render_image.to_three_d_texture(&context);
    // Reading a texture returns the rows in the order they were filled, top first like the render image
    let pixels: Vec<[f32; 4]> = texture.as_color_target(None).read();
    let numbers: Vec<f32> = pixels.iter().map(|p| p[0]).collect();
    assert_eq!(numbers, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
}

#[test]
fn render_image_updates_the_texture_in_place() {
    let Some(context) = context() else { return };
    let mut render_image = numbered_image(4, 4);
    let texture = render_image.to_three_d_texture(&context);
    render_image.reset();
    render_image.write_to_three_d_texture(&texture);
    let pixels: Vec<[f32; 4]> = texture.as_color_target(None).read();
    assert!(pixels.iter().all(|p| p[0] == 0.0));
}

// ---------------------------------------------------------------------------------------------------------------------
// The compute tracer against values known from physics and against the CPU reference

#[path = "../scenes/garden.rs"]
mod garden;

fn compute_tracer() -> Option<ComputeTracer> {
    let context = context()?;
    let tracer = ComputeTracer::from_context(&context);
    if tracer.is_none() && std::env::var("WGPU_BACKEND").as_deref() != Ok("gl") {
        panic!("no compute shaders on {:?}", RaytraceSupport::of(&context));
    }
    tracer
}

fn render(
    tracer: &mut impl Tracer,
    scene: &RtScene,
    camera: &Camera,
    size: (u32, u32),
    samples: u32,
    max_bounces: u32,
) -> RenderImage {
    let mut render_image = RenderImage::new(size.0, size.1);
    tracer
        .trace(
            scene,
            camera,
            &mut render_image,
            &TraceSettings {
                samples_per_call: samples,
                max_bounces,
                ..Default::default()
            },
        )
        .unwrap();
    render_image
}

fn mean(render_image: &RenderImage) -> Vec3 {
    let pixels = render_image.pixels();
    pixels.iter().fold(vec3(0.0, 0.0, 0.0), |a, b| a + *b) / pixels.len() as f32
}

fn ground_plane(scene: &mut RtScene, material: MaterialId) {
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            square,
            material,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(1000.0),
        )
        .unwrap();
}

fn camera_at(position: Vec3, target: Vec3, up: Vec3, fov: f32, size: u32) -> Camera {
    Camera::new_perspective(
        Viewport::new_at_origo(size, size),
        position,
        target,
        up,
        degrees(fov),
        0.01,
        100.0,
    )
}

#[test]
fn compute_white_furnace() {
    let Some(mut tracer) = compute_tracer() else {
        return;
    };
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 1000.0,
    });
    let white = scene.add_material(RtMaterial::diffuse(vec3(1.0, 1.0, 1.0)));
    let sphere = scene.add_mesh(&CpuMesh::sphere(32)).unwrap();
    scene.add_instance(sphere, white, Mat4::identity()).unwrap();
    let camera = camera_at(
        vec3(0.0, 0.0, 4.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        30.0,
        16,
    );
    let render_image = render(&mut tracer, &scene, &camera, (16, 16), 256, 64);
    for p in render_image.pixels() {
        assert!((p.x - 1000.0).abs() < 60.0, "{p:?}");
    }
    let m = mean(&render_image);
    assert!((m.x - 1000.0).abs() < 5.0, "{m:?}");
}

#[test]
fn compute_white_ground_under_the_sky() {
    let Some(mut tracer) = compute_tracer() else {
        return;
    };
    let mut scene = RtScene::new();
    let sky = Sky::from_sun_position(
        SunPosition {
            azimuth: 200.0,
            elevation: 50.0,
        },
        SceneOrientation::default(),
    )
    .with_ground_albedo(vec3(1.0, 1.0, 1.0));
    let expected = sky.radiance(vec3(0.0, -1.0, 0.0));
    scene.set_environment(RtEnvironment::Sky(sky));
    let white = scene.add_material(RtMaterial::diffuse(vec3(1.0, 1.0, 1.0)));
    ground_plane(&mut scene, white);
    let camera = camera_at(
        vec3(0.0, 2.0, 0.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, -1.0),
        20.0,
        8,
    );
    let got = mean(&render(&mut tracer, &scene, &camera, (8, 8), 512, 2));
    for c in 0..3 {
        assert!(
            (got[c] / expected[c] - 1.0).abs() < 0.02,
            "{got:?} vs {expected:?}"
        );
    }
}

#[test]
fn compute_point_light_falls_off_with_the_square_of_the_distance() {
    let Some(mut tracer) = compute_tracer() else {
        return;
    };
    for height in [1.0f32, 3.0] {
        let mut scene = RtScene::new();
        let grey = scene.add_material(RtMaterial::diffuse(vec3(0.5, 0.5, 0.5)));
        ground_plane(&mut scene, grey);
        scene.add_light(RtLight::Point {
            position: vec3(0.0, height, 0.0),
            color: vec3(1.0, 1.0, 1.0),
            intensity: 100.0,
        });
        let camera = camera_at(
            vec3(0.0, 0.5, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, -1.0),
            0.5,
            1,
        );
        let got = render(&mut tracer, &scene, &camera, (1, 1), 16, 1)
            .pixel(0, 0)
            .x;
        let expected = 100.0 / (height * height) * 0.5 / std::f32::consts::PI;
        assert!(
            (got / expected - 1.0).abs() < 0.01,
            "height {height}: {got} vs {expected}"
        );
    }
}

#[test]
fn compute_sunlight_passes_through_a_window_pane() {
    let Some(mut tracer) = compute_tracer() else {
        return;
    };
    let mut scene = RtScene::new();
    let grey = scene.add_material(RtMaterial::diffuse(vec3(0.5, 0.5, 0.5)));
    let glass = scene.add_material(RtMaterial::glass());
    let wall = scene.add_material(RtMaterial::diffuse(vec3(0.5, 0.5, 0.5)));
    ground_plane(&mut scene, grey);
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    let above = |x: f32| {
        Mat4::from_translation(vec3(x, 1.0, 0.0))
            * Mat4::from_angle_x(degrees(-90.0))
            * Mat4::from_scale(0.5)
    };
    let pane = scene.add_instance(square, glass, above(-0.5)).unwrap();
    scene.add_instance(square, wall, above(0.5)).unwrap();
    scene.add_light(RtLight::Directional {
        direction: vec3(0.0, -1.0, 0.0),
        color: vec3(1.0, 1.0, 1.0),
        illuminance: 1000.0,
    });
    let camera = |x: f32| {
        camera_at(
            vec3(x, 0.5, 0.0),
            vec3(x, 0.0, 0.0),
            vec3(0.0, 0.0, -1.0),
            1.0,
            1,
        )
    };
    let lit = 1000.0 * 0.5 / std::f32::consts::PI;
    let under_glass = render(&mut tracer, &scene, &camera(-0.5), (1, 1), 4, 1)
        .pixel(0, 0)
        .x;
    let under_wall = render(&mut tracer, &scene, &camera(0.5), (1, 1), 4, 1)
        .pixel(0, 0)
        .x;
    assert!(
        (under_glass / lit - 0.96).abs() < 0.01,
        "{under_glass} of {lit}"
    );
    assert_eq!(under_wall, 0.0);
    scene.set_instance_material(pane, wall).unwrap();
    assert_eq!(
        render(&mut tracer, &scene, &camera(-0.5), (1, 1), 4, 1)
            .pixel(0, 0)
            .x,
        0.0
    );
}

#[test]
fn compute_tiles_do_not_change_the_image() {
    let Some(tracer) = compute_tracer() else {
        return;
    };
    let scene = garden::scene();
    let camera = garden::camera(40, 24);
    let gpu = tracer.gpu().clone();
    let mut large = tracer;
    let mut small = ComputeTracer::new(&gpu).with_tile_size(8);
    let a = render(&mut large, &scene, &camera, (40, 24), 2, 8);
    let b = render(&mut small, &scene, &camera, (40, 24), 2, 8);
    assert_eq!(a.pixels(), b.pixels());
}

#[test]
fn compute_garden_agrees_with_the_cpu_reference() {
    // The tracers draw other random numbers, so their images agree in the averages of larger blocks, not in the noise.
    let Some(mut tracer) = compute_tracer() else {
        return;
    };
    let (width, height) = (96, 60);
    let scene = garden::scene();
    let camera = garden::camera(width, height);
    let gpu = render(&mut tracer, &scene, &camera, (width, height), 256, 8);
    let cpu = render(
        &mut CpuReferenceTracer::new(),
        &scene,
        &camera,
        (width, height),
        64,
        8,
    );
    let tone_mapping = ToneMapping {
        exposure_ev100: 14.0,
        ..Default::default()
    };
    let (a, b) = (gpu.to_srgb8(&tone_mapping), cpu.to_srgb8(&tone_mapping));
    let block = |data: &[[u8; 4]], bx: u32, by: u32, c: usize| -> f32 {
        let mut sum = 0.0;
        for y in by * 6..by * 6 + 6 {
            for x in bx * 6..bx * 6 + 6 {
                sum += data[(y * width + x) as usize][c] as f32;
            }
        }
        sum / 36.0
    };
    let (mut worst, mut total, mut count) = (0.0f32, 0.0f32, 0);
    for by in 0..height / 6 {
        for bx in 0..width / 6 {
            for c in 0..3 {
                let d = (block(&a, bx, by, c) - block(&b, bx, by, c)).abs();
                worst = worst.max(d);
                total += d;
                count += 1;
            }
        }
    }
    let average = total / count as f32;
    eprintln!("average difference {average}, worst block {worst}");
    assert!(
        average < 1.5 && worst < 10.0,
        "average difference {average}, worst block {worst}"
    );
}

#[test]
fn gpu_render_image_shows_in_three_d_without_the_cpu() {
    let Some(context) = context() else { return };
    let Some(mut tracer) = ComputeTracer::from_context(&context) else {
        return;
    };
    let scene = garden::scene();
    let camera = garden::camera(20, 12);
    let mut image = GpuRenderImage::new(tracer.gpu(), 20, 12);
    let settings = TraceSettings {
        samples_per_call: 4,
        ..Default::default()
    };
    tracer
        .trace_gpu(&scene, &camera, &mut image, &settings)
        .unwrap();
    tracer
        .trace_gpu(&scene, &camera, &mut image, &settings)
        .unwrap();
    assert_eq!(image.sample_count(), 8);
    let on_cpu = image.read();
    let texture = image.to_three_d_texture(&context);
    // Reading returns the top row first, as the render image
    let pixels: Vec<[f32; 4]> = texture.as_color_target(None).read();
    for y in 0..12 {
        for x in 0..20 {
            let p = pixels[(y * 20 + x) as usize];
            let e = on_cpu.pixel(x, y);
            let close = |a: f32, b: f32| (a - b).abs() <= 1e-3 * b.abs().max(1.0);
            assert!(
                close(p[0], e.x) && close(p[1], e.y) && close(p[2], e.z) && p[3] == 1.0,
                "{x} {y}: {p:?} vs {e:?}"
            );
        }
    }
    image.reset();
    assert_eq!(image.read().pixel(3, 3), vec3(0.0, 0.0, 0.0));
}

#[test]
fn compute_calls_in_a_row_each_measure_their_time() {
    let Some(mut tracer) = compute_tracer() else {
        return;
    };
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 1000.0,
    });
    let camera = camera_at(
        vec3(0.0, 0.0, 4.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        30.0,
        16,
    );
    // Calls without waiting in between take the timestamps of one slot after the other
    let mut image = GpuRenderImage::new(tracer.gpu(), 16, 16);
    for _ in 0..6 {
        tracer
            .trace_gpu(&scene, &camera, &mut image, &TraceSettings::default())
            .unwrap();
    }
    let _ = tracer
        .gpu()
        .device
        .poll(three_d::wgpu::PollType::wait_indefinitely());
    assert_eq!(tracer.gpu_error(), None);
    tracer
        .trace_gpu(&scene, &camera, &mut image, &TraceSettings::default())
        .unwrap();
}

#[test]
fn a_sample_spread_over_calls_adds_up_to_the_same_image() {
    let Some(tracer) = compute_tracer() else {
        return;
    };
    let mut tracer = tracer.with_tile_size(16);
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 0.8, 0.6),
        luminance: 1000.0,
    });
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    let material = scene.add_material(RtMaterial::default());
    scene
        .add_instance(square, material, Mat4::identity())
        .unwrap();
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(40, 24),
        vec3(0.5, 0.3, 3.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.01,
        100.0,
    );
    // 3 by 2 tiles of 16 pixels
    assert_eq!(tracer.tiles_per_sample(40, 24), 6);
    let settings = TraceSettings::default();
    let mut whole = GpuRenderImage::new(tracer.gpu(), 40, 24);
    for _ in 0..2 {
        tracer
            .trace_gpu(&scene, &camera, &mut whole, &settings)
            .unwrap();
    }
    let mut parts = GpuRenderImage::new(tracer.gpu(), 40, 24);
    // Four tiles, then the two to the end of the sample
    assert!(!tracer
        .trace_gpu_tiles(&scene, &camera, &mut parts, &settings, 4)
        .unwrap());
    assert!(parts.sample_in_progress());
    assert_eq!(parts.sample_count(), 0);
    assert!(tracer
        .trace_gpu_tiles(&scene, &camera, &mut parts, &settings, 4)
        .unwrap());
    assert!(!parts.sample_in_progress());
    assert_eq!(parts.sample_count(), 1);
    // A tile of the second sample, which trace_gpu completes
    assert!(!tracer
        .trace_gpu_tiles(&scene, &camera, &mut parts, &settings, 1)
        .unwrap());
    tracer
        .trace_gpu(&scene, &camera, &mut parts, &settings)
        .unwrap();
    assert_eq!(parts.sample_count(), 2);
    assert!(!parts.sample_in_progress());
    assert_eq!(tracer.gpu_error(), None);
    // The same samples of the same pixels: the random numbers go with pixel and sample, not with the tile
    let (whole, parts) = (whole.read(), parts.read());
    assert_eq!(whole.sample_count(), 2);
    let differs = whole
        .pixels()
        .iter()
        .zip(parts.pixels())
        .filter(|(a, b)| (**a - *b).magnitude() > 1e-3 * a.magnitude().max(1.0))
        .count();
    assert_eq!(differs, 0);
    // Not all of one color: the square is in the picture
    let pixels = whole.pixels();
    assert!(pixels.iter().any(|p| (*p - pixels[0]).magnitude() > 1.0));
}
