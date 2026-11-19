//!
//! The à-trous denoiser on the CPU and, with the `wgpu` feature, on the GPU.
//!

use three_d_raytrace::*;

/// A floor with four large checkers and a box, under a sky and a low light, so there are textures, edges and soft light.
fn scene() -> RtScene {
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(0.6, 0.7, 1.0),
        luminance: 2000.0,
    });
    scene.add_light(RtLight::Directional {
        direction: vec3(-1.0, -1.0, -0.5),
        color: vec3(1.0, 0.95, 0.9),
        illuminance: 20000.0,
    });
    let mut texels = Vec::new();
    for y in 0..2 {
        for x in 0..2 {
            texels.push(if (x + y) % 2 == 0 {
                [230, 230, 230, 255]
            } else {
                [60, 90, 160, 255]
            });
        }
    }
    let checker = scene.add_texture(RtTexture {
        nearest: true,
        ..RtTexture::new(2, 2, texels)
    });
    let floor_material = scene.add_material(RtMaterial {
        base_color_texture: Some(checker),
        ..RtMaterial::diffuse(vec3(1.0, 1.0, 1.0))
    });
    let floor = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            floor,
            floor_material,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(4.0),
        )
        .unwrap();
    let red = scene.add_material(RtMaterial::diffuse(vec3(0.7, 0.15, 0.1)));
    let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
    scene
        .add_instance(
            cube,
            red,
            Mat4::from_translation(vec3(0.0, 0.5, 0.0)) * Mat4::from_scale(0.5),
        )
        .unwrap();
    scene
}

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

fn camera() -> Camera {
    Camera::new_perspective(
        Viewport::new_at_origo(WIDTH, HEIGHT),
        vec3(1.6, 1.3, 2.0),
        vec3(0.0, 0.4, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(50.0),
        0.1,
        100.0,
    )
}

fn trace(tracer: &mut dyn Tracer, image: &mut RenderImage, samples: u32) {
    tracer
        .trace(
            &scene(),
            &camera(),
            image,
            &TraceSettings {
                samples_per_call: samples,
                ..Default::default()
            },
        )
        .unwrap();
}

/// Mean difference of the tone mapped 8 bit images, as seen on screen.
fn error(image: &RenderImage, reference: &RenderImage) -> f32 {
    let tm = ToneMapping {
        exposure_ev100: 13.0,
        ..Default::default()
    };
    let (a, b) = (image.to_srgb8(&tm), reference.to_srgb8(&tm));
    let sum: i32 = a
        .iter()
        .zip(&b)
        .map(|(a, b)| {
            (0..3)
                .map(|c| (a[c] as i32 - b[c] as i32).abs())
                .sum::<i32>()
        })
        .sum();
    sum as f32 / (3 * a.len()) as f32
}

fn reference() -> RenderImage {
    let mut reference = RenderImage::new(WIDTH, HEIGHT);
    trace(&mut CpuReferenceTracer::new(), &mut reference, 1024);
    reference
}

#[test]
fn denoising_brings_few_samples_closer() {
    let reference = reference();
    for samples in [1, 4, 16] {
        let mut noisy = RenderImage::with_features(WIDTH, HEIGHT);
        trace(&mut CpuReferenceTracer::new(), &mut noisy, samples);
        let denoised = AtrousDenoiser::default().denoise(&noisy).unwrap();
        let (before, after) = (error(&noisy, &reference), error(&denoised, &reference));
        println!("{samples} samples: error {before:.3} → {after:.3}");
        // Few samples get much better; with more the filter steps back
        let limit = if samples < 16 { 0.75 } else { 1.0 };
        assert!(
            after < limit * before,
            "{samples} samples: {before} → {after}"
        );
    }
}

#[test]
fn features_are_averaged() {
    let mut image = RenderImage::with_features(WIDTH, HEIGHT);
    trace(&mut CpuReferenceTracer::new(), &mut image, 4);
    // The top left corner shows the sky: albedo one, the largest distance
    let sky = image.features(0, 0).unwrap();
    assert_eq!(sky.albedo, vec3(1.0, 1.0, 1.0));
    assert!(sky.distance > 1e8);
    // The middle shows the red box, facing the camera
    let f = image.features(WIDTH / 2, HEIGHT / 2).unwrap();
    assert!(
        (f.albedo - vec3(0.7, 0.15, 0.1)).magnitude() < 1e-4,
        "{f:?}"
    );
    assert!(f.distance > 2.0 && f.distance < 5.0, "{f:?}");
    assert!(f.normal.magnitude() > 0.5);
    // Without features the denoiser says so
    let plain = RenderImage::new(4, 4);
    assert_eq!(
        AtrousDenoiser::default().denoise(&plain).err(),
        Some(RtError::NoFeatures)
    );
}

#[test]
fn many_samples_are_left_almost_alone() {
    // The filter weakens as the variance falls
    let reference = reference();
    let denoised = AtrousDenoiser::default().denoise(&{
        let mut image = RenderImage::with_features(WIDTH, HEIGHT);
        trace(&mut CpuReferenceTracer::new(), &mut image, 1024);
        image
    });
    assert!(error(&denoised.unwrap(), &reference) < 0.5);
}

#[cfg(feature = "wgpu")]
#[test]
fn the_gpu_denoises_like_the_cpu() {
    let context = match three_d::Context::new_headless_wgpu() {
        Ok(context) => context,
        Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
            panic!("no wgpu adapter: {error}")
        }
        Err(_) => return,
    };
    let Some(mut tracer) = ComputeTracer::from_context(&context) else {
        return;
    };
    for samples in [2, 8] {
        let mut image = GpuRenderImage::with_features(tracer.gpu(), WIDTH, HEIGHT);
        tracer
            .trace_gpu(
                &scene(),
                &camera(),
                &mut image,
                &TraceSettings {
                    samples_per_call: samples,
                    ..Default::default()
                },
            )
            .unwrap();
        // The same samples denoised on the CPU
        let read = image.read();
        let f = read.features(WIDTH / 2, HEIGHT / 2).unwrap();
        assert!(
            (f.albedo - vec3(0.7, 0.15, 0.1)).magnitude() < 1e-4,
            "{f:?}"
        );
        let expected = AtrousDenoiser::default().denoise(&read).unwrap();
        let texture = image.to_three_d_texture(&context);
        image.write_denoised_to_three_d_texture(&texture, &AtrousDenoiser::default());
        let got: Vec<[f32; 4]> = texture.as_color_target(None).read();
        // three-d reads the top row first
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let g = got[(y * WIDTH + x) as usize];
                let e = expected.pixel(x, y);
                for c in 0..3 {
                    assert!(
                        (g[c] - e[c]).abs() <= 1e-2 * e[c].abs().max(10.0),
                        "{samples} samples at {x} {y}: {g:?} vs {e:?}"
                    );
                }
            }
        }
    }
    let gpu = GpuDevice::from_context(&context).unwrap();
    let _ = gpu
        .device
        .poll(three_d::wgpu::PollType::wait_indefinitely());
}
