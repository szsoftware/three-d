//!
//! Caustics of the CPU tracer from photons: light bent by solid glass onto a floor.
//!

use three_d_raytrace::*;

const E: f32 = 10_000.0;

/// A white floor under light from straight above, with the given glass.
fn scene(glass: &CpuMesh, transform: Mat4) -> RtScene {
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 0.0,
    });
    scene.add_light(RtLight::Directional {
        direction: vec3(0.0, -1.0, 0.0),
        color: vec3(1.0, 1.0, 1.0),
        illuminance: E,
    });
    let floor = scene.add_material(RtMaterial {
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
    let glass_material = scene.add_material(RtMaterial {
        base_color: vec3(1.0, 1.0, 1.0),
        roughness: 0.0,
        transmission: 1.0,
        thin_walled: false,
        ..Default::default()
    });
    let mesh = scene.add_mesh(glass).unwrap();
    scene.add_instance(mesh, glass_material, transform).unwrap();
    scene
}

/// The floor seen straight from below the glass, orthographic over a square of the given size: luminance per pixel.
fn floor_below(scene: &RtScene, size: f32, pixels: u32, photons: u32, calls: u32) -> Vec<f32> {
    let camera = Camera::new_orthographic(
        Viewport::new_at_origo(pixels, pixels),
        vec3(0.0, 0.2, 0.0),
        // One unit away, three-d scales the height of the view by the distance to the target
        vec3(0.0, -0.8, 0.0),
        vec3(0.0, 0.0, -1.0),
        size,
        0.01,
        1.0,
    );
    let mut image = RenderImage::new(pixels, pixels);
    let mut tracer = CpuReferenceTracer::new();
    let settings = TraceSettings {
        samples_per_call: 2,
        max_bounces: 4,
        caustic_photons: photons,
        caustic_radius: 0.05,
        ..Default::default()
    };
    for _ in 0..calls {
        tracer.trace(scene, &camera, &mut image, &settings).unwrap();
    }
    (0..pixels)
        .flat_map(|y| (0..pixels).map(move |x| (x, y)))
        .map(|(x, y)| image.pixel(x, y).y)
        .collect()
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

#[test]
fn light_through_a_glass_slab_is_as_bright_with_photons() {
    // 2 × 2 m, 10 cm thick, its bottom at 0.5 m
    let slab = scene(
        &CpuMesh::cube(),
        Mat4::from_translation(vec3(0.0, 0.55, 0.0)) * Mat4::from_nonuniform_scale(1.0, 0.05, 1.0),
    );
    let without = mean(&floor_below(&slab, 0.5, 8, 0, 4));
    let with = mean(&floor_below(&slab, 0.5, 8, 40_000, 12));
    // Lambert floor of 0.8 behind two surfaces that pass 96 % each, and a little light the floor sends up
    // and the glass reflects back
    let expected = 0.8 / std::f32::consts::PI * E * 0.96 * 0.96;
    assert!(
        without > expected && without < 1.08 * expected,
        "{without} {expected}"
    );
    assert!((with - without).abs() < 0.04 * without, "{with} {without}");
}

#[test]
fn a_glass_ball_focuses_the_light() {
    // A ball lens of radius 0.5 focuses about 0.75 below its center, onto the floor
    let ball = scene(
        &CpuMesh::sphere(48),
        Mat4::from_translation(vec3(0.0, 0.75, 0.0)) * Mat4::from_scale(0.5),
    );
    let pixels = 24;
    let without = floor_below(&ball, 2.4, pixels, 0, 2);
    let with = floor_below(&ball, 2.4, pixels, 40_000, 12);
    let center = |image: &[f32]| {
        let c = pixels / 2;
        [c - 1, c]
            .iter()
            .flat_map(|y| [c - 1, c].map(|x| image[(y * pixels + x) as usize]))
            .sum::<f32>()
            / 4.0
    };
    // With photons the light gathers in the middle, without the glass only lets it through
    assert!(
        center(&with) > 3.0 * center(&without),
        "{} {}",
        center(&with),
        center(&without)
    );
    // and no light is made: over the whole floor about the same, less what the ball reflects away
    let (a, b) = (mean(&without), mean(&with));
    assert!((a - b).abs() < 0.06 * a, "{a} {b}");
}
