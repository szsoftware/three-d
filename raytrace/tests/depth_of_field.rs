//!
//! Depth of field with the CPU reference tracer and, with the `wgpu` feature, the compute tracer: an edge at the focus
//! distance stays sharp, an edge behind it is blurred by the circle of confusion of a thin lens.
//!

mod common;

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

const WIDTH: u32 = 96;
const HEIGHT: u32 = 96;
const DISTANCE: f32 = 4.0;
const FOV: f32 = 10.0;

/// A glowing half plane left of x = 0 at z = 0 on black, seen from `DISTANCE` in front of it.
fn edge() -> (RtScene, Camera) {
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 0.0,
    });
    let glow = scene.add_material(RtMaterial {
        base_color: vec3(0.0, 0.0, 0.0),
        emission: vec3(1000.0, 1000.0, 1000.0),
        specular: 0.0,
        ..Default::default()
    });
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            square,
            glow,
            Mat4::from_translation(vec3(-10.0, 0.0, 0.0)) * Mat4::from_scale(10.0),
        )
        .unwrap();
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(WIDTH, HEIGHT),
        vec3(0.0, 0.0, DISTANCE),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(FOV),
        0.01,
        100.0,
    );
    (scene, camera)
}

/// Luminance along the row, averaged over the rows.
fn profile(tracer: &mut dyn Tracer, dof: Option<DepthOfField>) -> Vec<f32> {
    let (scene, camera) = edge();
    let mut image = RenderImage::new(WIDTH, HEIGHT);
    let settings = TraceSettings {
        samples_per_call: 32,
        max_bounces: 1,
        depth_of_field: dof,
        ..Default::default()
    };
    tracer
        .trace(&scene, &camera, &mut image, &settings)
        .unwrap();
    (0..WIDTH)
        .map(|x| (0..HEIGHT).map(|y| image.pixel(x, y).y).sum::<f32>() / HEIGHT as f32)
        .collect()
}

/// Pixels between 10 % and 90 % of the full brightness.
fn edge_width(profile: &[f32]) -> usize {
    profile
        .iter()
        .filter(|l| **l > 100.0 && **l < 900.0)
        .count()
}

#[test]
fn the_focus_plane_is_sharp_and_the_rest_blurred() {
    for (name, mut tracer) in tracers() {
        let pinhole = profile(tracer.as_mut(), None);
        let lens = |f_number: f32, focus_distance: f32| DepthOfField {
            f_number,
            focus_distance,
        };
        let focused = profile(tracer.as_mut(), Some(lens(1.0, DISTANCE)));
        let blurred = profile(tracer.as_mut(), Some(lens(1.0, 1.0)));
        assert!(edge_width(&pinhole) <= 2, "{name}: {pinhole:?}");
        assert!(edge_width(&focused) <= 2, "{name}: {focused:?}");

        // A thin lens: the cone of rays through the focus point spreads to R (D - F) / F on the plane
        let (_, camera) = edge();
        let radius = lens(1.0, 1.0).aperture_radius(&camera);
        let spread = radius * (DISTANCE - 1.0) / 1.0;
        let pixel = 2.0 * DISTANCE * (FOV.to_radians() / 2.0).tan() / HEIGHT as f32;
        let blur = spread / pixel;
        // Over a disc of radius r, the edge rises from 10 % to 90 % within 1.37 r (a chord 0.69 r from the center
        // cuts off a tenth of the disc)
        let width = edge_width(&blurred) as f32;
        assert!(
            (width - 1.37 * blur).abs() < 0.1 * blur,
            "{name}: edge {width} pixels, blur radius {blur}"
        );
        // Blur moves light around, it does not make or lose any
        let sum = |p: &[f32]| p.iter().sum::<f32>();
        assert!(
            (sum(&blurred) - sum(&pinhole)).abs() < 0.01 * sum(&pinhole),
            "{name}"
        );
    }
}

#[test]
fn focused_on_a_point_puts_it_at_the_focus_distance() {
    let (_, camera) = edge();
    let dof = DepthOfField::focused_on(&camera, vec3(3.0, 1.0, 0.0), 2.8);
    assert!((dof.focus_distance - DISTANCE).abs() < 1e-5);
    // 10° on a full frame sensor is a focal length of 137 mm, at f/2.8 an aperture of 49 mm
    let radius = dof.aperture_radius(&camera);
    assert!((radius - 0.137 / 2.8 / 2.0).abs() < 0.001, "{radius}");
}
