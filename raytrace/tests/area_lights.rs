//!
//! Area lights, with the CPU reference tracer and, with the `wgpu` feature, the compute tracer. Each light is compared
//! with a value known in closed form or with the same light built another way, which the tracer only finds by chance.
//!

use std::f32::consts::PI;
use three_d_raytrace::*;

fn tracers() -> Vec<(&'static str, Box<dyn Tracer>)> {
    #[allow(unused_mut)]
    let mut tracers: Vec<(&'static str, Box<dyn Tracer>)> =
        vec![("cpu", Box::new(CpuReferenceTracer::new()))];
    #[cfg(feature = "wgpu")]
    match three_d::Context::new_headless_wgpu() {
        Ok(context) => match ComputeTracer::from_context(&context) {
            Some(tracer) => tracers.push(("compute", Box::new(tracer))),
            None if std::env::var("WGPU_BACKEND").as_deref() == Ok("gl") => {}
            None => panic!("no compute shaders: {:?}", RaytraceSupport::of(&context)),
        },
        Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
            panic!("no wgpu adapter: {error}")
        }
        Err(error) => eprintln!("compute tracer skipped, no wgpu adapter: {error}"),
    }
    tracers
}

/// A floor of 200 × 200 at height 0 with the given material.
fn floor(scene: &mut RtScene, material: RtMaterial) {
    let material = scene.add_material(material);
    let mesh = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            mesh,
            material,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(100.0),
        )
        .unwrap();
}

/// The average luminance a small camera at `from` sees looking straight down.
fn average_below(tracer: &mut dyn Tracer, scene: &RtScene, from: Vec3, samples: u32) -> Vec3 {
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(8, 8),
        from,
        from - vec3(0.0, 1.0, 0.0),
        vec3(0.0, 0.0, -1.0),
        degrees(10.0),
        0.01,
        100.0,
    );
    let mut image = RenderImage::new(8, 8);
    tracer
        .trace(
            scene,
            &camera,
            &mut image,
            &TraceSettings {
                samples_per_call: samples,
                max_bounces: 1,
                ..Default::default()
            },
        )
        .unwrap();
    let mut sum = vec3(0.0, 0.0, 0.0);
    for y in 0..8 {
        for x in 0..8 {
            sum += image.pixel(x, y);
        }
    }
    sum / 64.0
}

#[test]
fn a_large_panel_lights_like_the_sky() {
    // A panel covering nearly the whole sky above a white floor reflects its luminance
    let mut scene = RtScene::new();
    floor(&mut scene, RtMaterial::diffuse(vec3(0.5, 0.5, 0.5)));
    scene.add_light(RtLight::Rect {
        corner: vec3(-1000.0, 1.0, -1000.0),
        edge_u: vec3(2000.0, 0.0, 0.0),
        edge_v: vec3(0.0, 0.0, 2000.0),
        color: vec3(1.0, 1.0, 1.0),
        luminance: 1000.0,
        two_sided: false,
    });
    for (name, mut tracer) in tracers() {
        let got = average_below(tracer.as_mut(), &scene, vec3(0.0, 0.5, 0.0), 64);
        assert!((got.x / 500.0 - 1.0).abs() < 0.02, "{name}: {got:?}");
    }
}

#[test]
fn a_small_ball_lights_like_a_point() {
    // 1000 lm from a ball of 5 cm, 2 m above the floor, as a point of 1000 / 4π cd
    let flux = 1000.0;
    let mut scene = RtScene::new();
    floor(&mut scene, RtMaterial::diffuse(vec3(0.5, 0.5, 0.5)));
    scene.add_light(RtLight::sphere_with_flux(
        vec3(0.0, 2.0, 0.0),
        0.05,
        vec3(1.0, 1.0, 1.0),
        flux,
    ));
    let expected = 0.5 / PI * flux / (4.0 * PI) / 4.0;
    for (name, mut tracer) in tracers() {
        // Seen from just below the ball, so the camera looks at the lit spot right under it
        let got = average_below(tracer.as_mut(), &scene, vec3(0.0, 1.9, 0.0), 32);
        assert!(
            (got.x / expected - 1.0).abs() < 0.02,
            "{name}: {got:?} vs {expected}"
        );
    }
}

#[test]
fn panels_agree_with_glowing_surfaces() {
    // A glossy floor under a panel: sampling the light and the material, weighted, gives what a glowing square of the
    // same luminance gives, which is only found by the material's samples
    let glossy = RtMaterial {
        metallic: 1.0,
        roughness: 0.3,
        ..RtMaterial::diffuse(vec3(0.9, 0.9, 0.9))
    };
    let luminance = 2000.0;
    let mut with_light = RtScene::new();
    floor(&mut with_light, glossy.clone());
    // Shines down: x × z points down
    with_light.add_light(RtLight::Rect {
        corner: vec3(-0.5, 1.0, -0.5),
        edge_u: vec3(1.0, 0.0, 0.0),
        edge_v: vec3(0.0, 0.0, 1.0),
        color: vec3(1.0, 1.0, 1.0),
        luminance,
        two_sided: false,
    });
    let mut with_surface = RtScene::new();
    floor(&mut with_surface, glossy);
    let glow = with_surface.add_material(RtMaterial {
        emission: vec3(luminance, luminance, luminance),
        ..RtMaterial::diffuse(vec3(0.0, 0.0, 0.0))
    });
    let square = with_surface.add_mesh(&CpuMesh::square()).unwrap();
    with_surface
        .add_instance(
            square,
            glow,
            Mat4::from_translation(vec3(0.0, 1.0, 0.0))
                * Mat4::from_angle_x(degrees(90.0))
                * Mat4::from_scale(0.5),
        )
        .unwrap();
    for (name, mut tracer) in tracers() {
        let from = vec3(0.3, 0.6, 0.2);
        let light = average_below(tracer.as_mut(), &with_light, from, 256);
        let surface = average_below(tracer.as_mut(), &with_surface, from, 4096);
        assert!(light.x > 10.0, "{name}: {light:?}");
        assert!(
            (light.x / surface.x - 1.0).abs() < 0.03,
            "{name}: light {light:?} surface {surface:?}"
        );
    }
}
