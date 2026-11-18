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
fn previews(context: &Context, width: u32, height: u32) -> Vec<RaytracePreview> {
    vec![
        RaytracePreview::new(context, width, height),
        RaytracePreview::new_on_cpu(context, width, height),
    ]
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
        assert_eq!(
            RaytracePreview::new(&context, 16, 8).kind(),
            TracerKind::Compute
        );
    }
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
}
