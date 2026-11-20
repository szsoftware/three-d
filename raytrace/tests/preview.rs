//!
//! The progressive preview with three-d, on the GPU and on the CPU. Skipped without a wgpu adapter unless
//! `THREE_D_REQUIRE_WGPU` is set.
//!
#![cfg(feature = "wgpu")]

use three_d::{Context, Interpolation, Texture2D, Wrapping};
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

/// Waits for the GPU, so dropping the context does not time out on slow adapters such as WARP.
fn finish(context: &Context) {
    if let Some(gpu) = GpuDevice::from_context(context) {
        let _ = gpu
            .device
            .poll(three_d::wgpu::PollType::wait_indefinitely());
    }
}

fn sky_only(luminance: f32) -> RtScene {
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 0.5, 0.25),
        luminance,
    });
    scene
}

fn camera(width: u32, height: u32, at: Vec3) -> Camera {
    Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        at,
        at + vec3(0.0, 0.0, -1.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        100.0,
    )
}

/// The previews to check: the one the context chooses and one on the CPU.
/// Both trace in every update, also on the CPU, so the samples can be counted, and the GPU keeps its resolution on
/// slow adapters.
fn previews(context: &Context, width: u32, height: u32) -> Vec<RaytracePreview> {
    let mut previews = vec![
        RaytracePreview::new(context, width, height),
        RaytracePreview::new_on_cpu(context, width, height),
    ];
    for p in &mut previews {
        p.wait_for_gpu();
        p.set_settings(PreviewSettings {
            cpu_in_background: false,
            gpu_frame_time: None,
            ..*p.settings()
        });
    }
    previews
}

#[test]
fn preview_gets_finer_and_starts_again() {
    let Some(context) = context() else { return };
    let scene = sky_only(1000.0);
    for mut preview in previews(&context, 16, 8) {
        let kind = preview.kind();
        assert!(preview
            .update(&scene, &camera(16, 8, vec3(0.0, 0.0, 0.0)))
            .unwrap());
        assert!(preview
            .update(&scene, &camera(16, 8, vec3(0.0, 0.0, 0.0)))
            .unwrap());
        assert_eq!(preview.sample_count(), 2, "{kind:?}");
        // Another camera
        preview
            .update(&scene, &camera(16, 8, vec3(1.0, 0.0, 0.0)))
            .unwrap();
        assert_eq!(preview.sample_count(), 1, "{kind:?}");
        // Another scene
        let other = sky_only(500.0);
        preview
            .update(&other, &camera(16, 8, vec3(1.0, 0.0, 0.0)))
            .unwrap();
        assert_eq!(preview.sample_count(), 1, "{kind:?}");
        // Done after the most samples
        preview.set_settings(PreviewSettings {
            max_samples: 2,
            ..*preview.settings()
        });
        assert!(preview
            .update(&other, &camera(16, 8, vec3(1.0, 0.0, 0.0)))
            .unwrap());
        assert!(preview.is_done());
        assert!(!preview
            .update(&other, &camera(16, 8, vec3(1.0, 0.0, 0.0)))
            .unwrap());
    }
    // The CPU traces at a lower resolution
    let cpu = RaytracePreview::new_on_cpu(&context, 16, 8);
    assert_eq!(cpu.texture().width(), 4);
    assert!(matches!(
        cpu.availability(),
        PreviewAvailability::CpuOnly(_)
    ));
    if RaytraceSupport::of(&context) >= RaytraceSupport::Compute {
        assert_eq!(PreviewAvailability::of(&context), PreviewAvailability::Gpu);
        let mut preview = RaytracePreview::new(&context, 16, 8);
        preview.wait_for_gpu();
        assert!(!preview.is_compiling());
        assert_eq!(preview.kind(), TracerKind::Compute);
        // The same size again keeps the samples
        preview
            .update(&scene, &camera(16, 8, vec3(0.0, 0.0, 0.0)))
            .unwrap();
        preview.resize(16, 8);
        assert_eq!(preview.sample_count(), 1);
    }
    finish(&context);
}

#[test]
fn preview_shows_the_tone_mapped_image() {
    // The sky alone: every pixel shows its luminance, exposed and tone mapped like RenderImage::to_srgb8
    let Some(context) = context() else { return };
    let luminance = 20000.0;
    let mut expected = RenderImage::new(1, 1);
    expected.accumulate(&[[luminance, luminance * 0.5, luminance * 0.25]], 1);
    let tone_mapping = ToneMapping::default();
    let expected = expected.to_srgb8(&tone_mapping)[0];
    for mut preview in previews(&context, 8, 8) {
        preview
            .update(&sky_only(luminance), &camera(8, 8, vec3(0.0, 0.0, 0.0)))
            .unwrap();
        let target = Texture2D::new_empty::<[u8; 4]>(
            &context,
            8,
            8,
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        preview.render_to_color(&target.as_color_target(None));
        let pixels: Vec<[u8; 4]> = target.as_color_target(None).read();
        for p in pixels {
            for c in 0..3 {
                assert!(
                    (p[c] as i32 - expected[c] as i32).abs() <= 1,
                    "{:?}: {p:?} vs {expected:?}",
                    preview.kind()
                );
            }
        }
    }
    finish(&context);
}

/// Ground below, sky above: the top rows of the screen must be the bright sky on every backend.
fn check_upright(context: &Context, name: &str) {
    let (width, height) = (32, 32);
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 5000.0,
    });
    let dark = scene.add_material(RtMaterial::diffuse(vec3(0.05, 0.05, 0.05)));
    let ground = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            ground,
            dark,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(1000.0),
        )
        .unwrap();
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        vec3(0.0, 1.0, 0.0),
        vec3(0.0, 1.0, -1.0),
        vec3(0.0, 1.0, 0.0),
        degrees(60.0),
        0.1,
        100.0,
    );
    for mut preview in previews(context, width, height) {
        preview.update(&scene, &camera).unwrap();
        let target = three_d::RenderTarget::screen(context, width, height);
        target.clear(three_d::ClearState::color(0.0, 0.0, 0.0, 1.0));
        preview.render(&target);
        // The screen is read top row first
        let pixels: Vec<[u8; 4]> = target.read_color();
        let top = pixels[(width + 1) as usize];
        let bottom = pixels[((height - 2) * width + 1) as usize];
        assert!(
            top[1] > bottom[1] + 50,
            "{name} {:?}: top {top:?} bottom {bottom:?}",
            preview.kind()
        );
    }
}

#[test]
fn preview_is_upright_on_wgpu() {
    let context = match Context::new_headless_wgpu_with_screen(32, 32, true, false) {
        Ok(context) => context,
        Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
            panic!("no wgpu adapter: {error}")
        }
        Err(_) => return,
    };
    check_upright(&context, "wgpu");
    finish(&context);
}

#[cfg(target_os = "linux")]
#[test]
fn preview_is_upright_on_opengl() {
    match Context::new_headless_gl_with_screen(32, 32, true, false) {
        Ok(context) => check_upright(&context, "opengl"),
        Err(error) => eprintln!("skipped, no OpenGL: {error}"),
    }
}

#[test]
fn the_cpu_traces_in_the_background() {
    let Some(context) = context() else { return };
    let scene = sky_only(1000.0);
    let mut preview = RaytracePreview::new_on_cpu(&context, 16, 8);
    assert!(preview.settings().cpu_in_background);
    let wait_for = |preview: &mut RaytracePreview, at: Vec3, samples: u32| {
        let start = std::time::Instant::now();
        loop {
            preview.update(&scene, &camera(16, 8, at)).unwrap();
            if preview.sample_count() >= samples {
                break;
            }
            assert!(start.elapsed().as_secs() < 30, "no samples from the worker");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    };
    wait_for(&mut preview, vec3(0.0, 0.0, 0.0), 3);
    // Another camera starts again
    preview
        .update(&scene, &camera(16, 8, vec3(1.0, 0.0, 0.0)))
        .unwrap();
    assert_eq!(preview.sample_count(), 0);
    wait_for(&mut preview, vec3(1.0, 0.0, 0.0), 2);
    // The worker stops at the most samples (set before a new start, a fast worker may be past them already)
    preview.set_settings(PreviewSettings {
        max_samples: 4,
        ..*preview.settings()
    });
    wait_for(&mut preview, vec3(2.0, 0.0, 0.0), 4);
    std::thread::sleep(std::time::Duration::from_millis(50));
    preview
        .update(&scene, &camera(16, 8, vec3(2.0, 0.0, 0.0)))
        .unwrap();
    assert_eq!(preview.sample_count(), 4);
    assert!(preview.is_done());
}

#[test]
fn the_preview_tells_why_it_started_again() {
    let Some(context) = context() else { return };
    let mut preview = RaytracePreview::new_on_cpu(&context, 16, 8);
    let scene = sky_only(1000.0);
    let at = vec3(0.0, 0.0, 0.0);
    assert_eq!(preview.last_restart(), None);
    preview.update(&scene, &camera(16, 8, at)).unwrap();
    assert_eq!(preview.last_restart(), Some(PreviewRestart::First));
    // The same camera and scene, also made anew: no restart
    preview.update(&scene, &camera(16, 8, at)).unwrap();
    assert_eq!(preview.restarts(), 1);
    preview
        .update(&scene, &camera(16, 8, vec3(1.0, 0.0, 0.0)))
        .unwrap();
    assert_eq!(preview.last_restart(), Some(PreviewRestart::Camera));
    preview.update(&scene, &camera(16, 9, at)).unwrap();
    assert_eq!(preview.last_restart(), Some(PreviewRestart::Viewport));
    // Another scene with the same content
    preview
        .update(&sky_only(1000.0), &camera(16, 9, at))
        .unwrap();
    assert_eq!(preview.last_restart(), Some(PreviewRestart::Scene));
    let scene = sky_only(1000.0);
    preview.update(&scene, &camera(16, 9, at)).unwrap();
    preview.resize(32, 18);
    preview.update(&scene, &camera(16, 9, at)).unwrap();
    assert_eq!(preview.last_restart(), Some(PreviewRestart::Resize));
    preview.set_settings(PreviewSettings {
        max_bounces: preview.settings().max_bounces + 1,
        ..*preview.settings()
    });
    preview.update(&scene, &camera(16, 9, at)).unwrap();
    assert_eq!(preview.last_restart(), Some(PreviewRestart::Settings));
    let restarts = preview.restarts();
    assert_eq!(restarts, 7);
    // A minimized window: nothing changes, the image waits
    preview.resize(0, 0);
    assert!(!preview.update(&scene, &camera(0, 0, at)).unwrap());
    preview.update(&scene, &camera(16, 9, at)).unwrap();
    assert_eq!(preview.restarts(), restarts);
}

#[test]
fn a_slow_gpu_gets_a_lighter_preview() {
    let Some(context) = context() else { return };
    let mut preview = RaytracePreview::new(&context, 64, 32);
    preview.wait_for_gpu();
    if preview.kind() != TracerKind::Compute {
        return;
    }
    // Every frame is far too slow for this limit: at once to the CPU
    preview.set_settings(PreviewSettings {
        gpu_frame_time: Some(std::time::Duration::from_nanos(1)),
        cpu_in_background: false,
        ..*preview.settings()
    });
    let scene = sky_only(1000.0);
    let mut scales = vec![preview.gpu_downscale()];
    for _ in 0..40 {
        preview
            .update(&scene, &camera(64, 32, vec3(0.0, 0.0, 0.0)))
            .unwrap();
        if preview.kind() == TracerKind::Compute && scales.last() != Some(&preview.gpu_downscale())
        {
            scales.push(preview.gpu_downscale());
            assert_eq!(preview.texture().width(), 64 / preview.gpu_downscale());
        }
    }
    finish(&context);
    assert_eq!(scales, [1]);
    // The texture is smaller than the viewport, resizing to the viewport keeps the CPU
    preview.resize(64, 32);
    assert_eq!(preview.kind(), TracerKind::CpuReference);
    assert_eq!(preview.kind(), TracerKind::CpuReference);
    assert!(
        matches!(preview.availability(), PreviewAvailability::CpuOnly(why) if why.contains("1/4"))
    );
}

#[test]
fn the_cpu_goes_on_after_the_gpu_changed_its_resolution() {
    let Some(context) = context() else { return };
    let mut preview = RaytracePreview::new(&context, 64, 32);
    preview.wait_for_gpu();
    if preview.kind() != TracerKind::Compute {
        return;
    }
    let scene = sky_only(1000.0);
    let camera = camera(64, 32, vec3(0.0, 0.0, 0.0));
    preview.update(&scene, &camera).unwrap();
    // Half the resolution on the GPU (as a slow GPU gets it), then far too slow for the GPU at all
    preview.set_settings(PreviewSettings {
        gpu_downscale: 2,
        ..*preview.settings()
    });
    preview.update(&scene, &camera).unwrap();
    assert_eq!(preview.texture().width(), 32);
    preview.set_settings(PreviewSettings {
        gpu_frame_time: Some(std::time::Duration::from_nanos(1)),
        ..*preview.settings()
    });
    for _ in 0..10 {
        preview.update(&scene, &camera).unwrap();
    }
    finish(&context);
    // On the CPU engine from before the GPU, at the CPU's resolution
    assert_eq!(preview.kind(), TracerKind::CpuReference);
    assert_eq!(
        preview.texture().width(),
        64 / preview.settings().cpu_downscale
    );
}

#[test]
fn the_cpu_traces_while_the_gpu_compiles() {
    let Some(context) = context() else { return };
    let mut preview = RaytracePreview::new(&context, 16, 8);
    if preview.availability() != &PreviewAvailability::Gpu {
        return;
    }
    let scene = sky_only(1000.0);
    let start = std::time::Instant::now();
    while preview.is_compiling() {
        assert_eq!(preview.kind(), TracerKind::CpuReference);
        preview
            .update(&scene, &camera(16, 8, vec3(0.0, 0.0, 0.0)))
            .unwrap();
        assert!(
            start.elapsed().as_secs() < 120,
            "the shaders never compiled"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    preview
        .update(&scene, &camera(16, 8, vec3(0.0, 0.0, 0.0)))
        .unwrap();
    assert_eq!(preview.kind(), TracerKind::Compute);
    assert!(preview.sample_count() >= 1);
    finish(&context);
}

#[test]
fn the_cpu_traces_a_sample_in_parts_within_the_frame_time() {
    let Some(context) = context() else { return };
    let scene = sky_only(1000.0);
    let mut preview = RaytracePreview::new_on_cpu(&context, 64, 32);
    preview.set_settings(PreviewSettings {
        cpu_in_background: false,
        // Shorter than a row: one row per update
        cpu_frame_time: Some(std::time::Duration::from_nanos(1)),
        ..*preview.settings()
    });
    let rows = preview.texture().height();
    let at = camera(64, 32, vec3(0.0, 0.0, 0.0));
    for _ in 0..rows - 1 {
        assert!(!preview.update(&scene, &at).unwrap());
        assert_eq!(preview.sample_count(), 0);
    }
    assert!(preview.update(&scene, &at).unwrap());
    assert_eq!(preview.sample_count(), 1);
    finish(&context);
}

#[test]
fn the_cpu_keeps_its_samples_when_the_gpu_hands_back() {
    let Some(context) = context() else { return };
    let mut preview = RaytracePreview::new(&context, 16, 8);
    if preview.availability() != &PreviewAvailability::Gpu {
        return;
    }
    preview.set_settings(PreviewSettings {
        cpu_in_background: false,
        // Not done before the GPU takes over on fast machines
        max_samples: u32::MAX,
        ..*preview.settings()
    });
    let scene = sky_only(1000.0);
    let view = camera(16, 8, vec3(0.0, 0.0, 0.0));
    // The CPU's samples until the GPU takes over
    let mut on_cpu = 0;
    let start = std::time::Instant::now();
    while preview.kind() == TracerKind::CpuReference {
        on_cpu = preview.sample_count();
        preview.update(&scene, &view).unwrap();
        assert!(
            start.elapsed().as_secs() < 120,
            "the shaders never compiled"
        );
    }
    // Far too slow for this limit: back to the CPU, with the samples it had and one more
    preview.set_settings(PreviewSettings {
        gpu_frame_time: Some(std::time::Duration::from_nanos(1)),
        ..*preview.settings()
    });
    while preview.kind() == TracerKind::Compute {
        preview.update(&scene, &view).unwrap();
    }
    finish(&context);
    eprintln!(
        "{on_cpu} samples on the CPU before the GPU, {} after",
        preview.sample_count()
    );
    assert!(
        preview.sample_count() > on_cpu,
        "{} after {on_cpu}",
        preview.sample_count()
    );
}

#[test]
fn on_wgpu_over_opengl_the_shaders_compile_once_the_cpu_shows_an_image() {
    let Some(context) = context() else { return };
    if context
        .wgpu_adapter_info()
        .is_none_or(|info| info.backend != three_d::wgpu::Backend::Gl)
    {
        return;
    }
    let mut preview = RaytracePreview::new(&context, 16, 8);
    if preview.availability() != &PreviewAvailability::Gpu {
        return;
    }
    preview.set_settings(PreviewSettings {
        cpu_in_background: false,
        ..PreviewSettings::default()
    });
    let scene = sky_only(1000.0);
    // No thread compiles: wgpu's OpenGL backend gives up when another thread holds the context for a second
    let mut updates = 0;
    while preview.sample_count() == 0 {
        assert!(preview.is_compiling());
        preview
            .update(&scene, &camera(16, 8, vec3(0.0, 0.0, 0.0)))
            .unwrap();
        updates += 1;
        assert!(updates < 1000, "the CPU never showed an image");
    }
    preview
        .update(&scene, &camera(16, 8, vec3(0.0, 0.0, 0.0)))
        .unwrap();
    assert!(!preview.is_compiling());
    assert_eq!(preview.kind(), TracerKind::Compute);
    finish(&context);
}
