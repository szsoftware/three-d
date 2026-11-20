//!
//! Luminaires with IES profiles, with the CPU reference tracer and, with the `wgpu` feature, the compute tracer: the
//! floor below is lit with the intensity of the profile in each direction.
//!

mod common;

use std::f32::consts::PI;
use std::sync::Arc;
use three_d_raytrace::*;

fn tracers() -> Vec<(&'static str, Box<dyn Tracer>)> {
    #[allow(unused_mut)]
    let mut tracers: Vec<(&'static str, Box<dyn Tracer>)> =
        vec![("cpu", Box::new(CpuReferenceTracer::new()))];
    #[cfg(feature = "wgpu")]
    match common::headless_wgpu() {
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

/// Measured in the planes C0, C90 and C180: brighter towards the front than to the sides.
const DOWNLIGHT: &str = "IESNA:LM-63-2002
[TEST] test
TILT=NONE
1 1000 1.0 5 3 1 2 0.1 0.1 0.0
1.0 1.0 20
0 22.5 45 67.5 90
0 90 180
400 380 300 100 0
400 300 200 50 0
400 380 300 100 0
";

const ALBEDO: f32 = 0.8;
const HEIGHT: f32 = 2.0;

/// A white floor in the dark with the luminaire `HEIGHT` above the origin, its front along +X.
fn scene(profile: Arc<IesProfile>) -> RtScene {
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 0.0,
    });
    let floor = scene.add_material(RtMaterial {
        base_color: vec3(ALBEDO, ALBEDO, ALBEDO),
        roughness: 1.0,
        specular: 0.0,
        ..Default::default()
    });
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            square,
            floor,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(20.0),
        )
        .unwrap();
    scene.add_light(RtLight::luminaire(
        vec3(0.0, HEIGHT, 0.0),
        vec3(0.0, -1.0, 0.0),
        vec3(1.0, 0.0, 0.0),
        profile,
    ));
    scene
}

/// Luminance of the floor at a point, seen from straight above through a tiny orthographic view.
fn floor_at(tracer: &mut dyn Tracer, scene: &RtScene, x: f32, z: f32) -> f32 {
    let camera = Camera::new_orthographic(
        Viewport::new_at_origo(2, 2),
        vec3(x, 1.0, z),
        vec3(x, 0.0, z),
        vec3(0.0, 0.0, -1.0),
        0.002,
        0.01,
        2.0,
    );
    let mut image = RenderImage::new(2, 2);
    let settings = TraceSettings {
        samples_per_call: 4,
        max_bounces: 1,
        ..Default::default()
    };
    tracer.trace(scene, &camera, &mut image, &settings).unwrap();
    image.pixels().iter().map(|p| p.y).sum::<f32>() / 4.0
}

/// Luminance of a Lambert floor lit with intensity `i` from the luminaire, at horizontal distance `r`.
fn expected(i: f32, r: f32) -> f32 {
    let d2 = HEIGHT * HEIGHT + r * r;
    let cos = HEIGHT / d2.sqrt();
    ALBEDO / PI * i * cos / d2
}

#[test]
fn the_floor_gets_the_intensity_of_the_profile() {
    let profile = Arc::new(IesProfile::parse(DOWNLIGHT).unwrap());
    let scene = scene(profile);
    for (name, mut tracer) in tracers() {
        // Straight below 400 cd, towards the front at 45° 300 cd, to the left and right 200 cd
        for (x, z, i) in [
            (0.0, 0.0, 400.0),
            (HEIGHT, 0.0, 300.0),
            (-HEIGHT, 0.0, 300.0),
            (0.0, -HEIGHT, 200.0),
            (0.0, HEIGHT, 200.0),
        ] {
            let l = floor_at(tracer.as_mut(), &scene, x, z);
            let e = expected(i, (x * x + z * z).sqrt());
            assert!(
                (l - e).abs() < 0.01 * e,
                "{name} at ({x}, {z}): {l}, expected {e}"
            );
        }
        // Above 90° nothing
        assert_eq!(floor_at(tracer.as_mut(), &scene, 100.0, 0.0), 0.0, "{name}");
    }
}

#[test]
fn the_flux_scales_the_profile() {
    let profile = Arc::new(IesProfile::from_vertical(&[300.0, 300.0, 0.0]));
    let light = RtLight::luminaire_with_flux(
        Vec3::zero(),
        vec3(0.0, -1.0, 0.0),
        vec3(1.0, 0.0, 0.0),
        profile.clone(),
        1000.0,
    );
    let RtLight::Luminaire { scale, .. } = light else {
        panic!()
    };
    assert!((scale * profile.flux() - 1000.0).abs() < 0.1);
}
