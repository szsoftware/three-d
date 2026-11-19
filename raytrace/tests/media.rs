//!
//! Participating media with the CPU reference tracer and, with the `wgpu` feature, the compute tracer: light absorbed
//! and scattered in closed meshes and in the fog over the scene, against closed forms.
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

/// A scene with a uniform environment of the luminance.
fn empty(luminance: f32) -> RtScene {
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance,
    });
    scene
}

/// The cube from -1 to 1 filled with the medium.
fn add_box(scene: &mut RtScene, medium: RtMedium) {
    let material = scene.add_material(RtMaterial::medium_boundary(medium));
    let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
    scene
        .add_instance(cube, material, Mat4::identity())
        .unwrap();
}

/// Looking along -z at the middle of the cube, orthographic over a small square: the mean luminance.
fn through_the_middle(
    tracer: &mut dyn Tracer,
    scene: &RtScene,
    size: f32,
    max_bounces: u32,
    samples: u32,
) -> Vec3 {
    let pixels = 4;
    let camera = Camera::new_orthographic(
        Viewport::new_at_origo(pixels, pixels),
        vec3(0.0, 0.0, 3.0),
        vec3(0.0, 0.0, 2.0),
        vec3(0.0, 1.0, 0.0),
        size,
        0.01,
        100.0,
    );
    let mut image = RenderImage::new(pixels, pixels);
    let settings = TraceSettings {
        samples_per_call: samples,
        max_bounces,
        ..Default::default()
    };
    tracer.trace(scene, &camera, &mut image, &settings).unwrap();
    let mut sum = vec3(0.0, 0.0, 0.0);
    for y in 0..pixels {
        for x in 0..pixels {
            let p = image.pixel(x, y);
            sum += vec3(p.x, p.y, p.z);
        }
    }
    sum / (pixels * pixels) as f32
}

fn henyey_greenstein(g: f32, cos: f32) -> f32 {
    let d = 1.0 + g * g - 2.0 * g * cos;
    (1.0 - g * g) / (4.0 * PI * d * d.sqrt())
}

#[test]
fn an_absorbing_box_takes_light_away_by_the_length_inside() {
    let mut scene = empty(1000.0);
    add_box(
        &mut scene,
        RtMedium {
            absorption: vec3(0.2, 0.5, 1.0),
            scattering: vec3(0.0, 0.0, 0.0),
            anisotropy: 0.0,
        },
    );
    for (name, mut tracer) in tracers() {
        let l = through_the_middle(tracer.as_mut(), &scene, 0.5, 4, 4);
        let expected = vec3((-0.4f32).exp(), (-1.0f32).exp(), (-2.0f32).exp()) * 1000.0;
        assert!(
            (l - expected).magnitude() < 0.01 * expected.magnitude(),
            "{name}: {l:?} {expected:?}"
        );
    }
}

#[test]
fn light_scattered_once_in_a_box_matches_the_closed_form() {
    let (sigma_s, sigma_a, g) = (0.15, 0.1, 0.5);
    let sigma_t = sigma_s + sigma_a;
    let e = 10_000.0;
    for (case, direction) in [
        ("from above", vec3(0.0, -1.0, 0.0)),
        ("from behind", vec3(0.0, 0.0, 1.0)),
    ] {
        let mut scene = empty(0.0);
        add_box(
            &mut scene,
            RtMedium {
                absorption: vec3(1.0, 1.0, 1.0) * sigma_a,
                scattering: vec3(1.0, 1.0, 1.0) * sigma_s,
                anisotropy: g,
            },
        );
        scene.add_light(RtLight::Directional {
            direction,
            color: vec3(1.0, 1.0, 1.0),
            illuminance: e,
        });
        // Towards the camera at +z: light from above turns by 90°, from behind it goes on straight
        let expected = if direction.y < 0.0 {
            // 1 m from the top to the line of sight, then s m to the front
            e * sigma_s
                * henyey_greenstein(g, 0.0)
                * (-sigma_t).exp()
                * (1.0 - (-2.0 * sigma_t).exp())
                / sigma_t
        } else {
            // 2 m in the box whereever it scatters
            e * sigma_s * henyey_greenstein(g, 1.0) * (-2.0 * sigma_t).exp() * 2.0
        };
        for (name, mut tracer) in tracers() {
            // One scattering only
            let l = through_the_middle(tracer.as_mut(), &scene, 0.05, 1, 256).y;
            assert!(
                (l - expected).abs() < 0.03 * expected,
                "{name} {case}: {l} {expected}"
            );
        }
    }
}

#[test]
fn a_white_box_of_fog_in_a_white_world_disappears() {
    // Nothing absorbs, so every path ends in the environment: the box has the luminance of the world
    let mut scene = empty(1000.0);
    add_box(&mut scene, RtMedium::fog(0.7, 1.0, 0.3));
    for (name, mut tracer) in tracers() {
        let l = through_the_middle(tracer.as_mut(), &scene, 1.5, 256, 8192).y;
        assert!((l - 1000.0).abs() < 20.0, "{name}: {l}");
    }
}

#[test]
fn the_sky_through_fog_that_thins_out_upwards() {
    let mut scene = empty(1000.0);
    let fog = RtFog {
        medium: RtMedium {
            absorption: vec3(0.05, 0.05, 0.05),
            scattering: vec3(0.0, 0.0, 0.0),
            anisotropy: 0.0,
        },
        height: 0.0,
        scale_height: 10.0,
    };
    scene.set_fog(Some(fog));
    let pixels = 4;
    // Straight up and at 30° above the horizon, from 5 m up
    for elevation in [90.0f32, 30.0] {
        let direction = vec3(
            elevation.to_radians().cos(),
            elevation.to_radians().sin(),
            0.0,
        );
        let at = vec3(0.0, 5.0, 0.0);
        let up = if elevation == 90.0 {
            vec3(0.0, 0.0, -1.0)
        } else {
            vec3(0.0, 1.0, 0.0)
        };
        let camera = Camera::new_orthographic(
            Viewport::new_at_origo(pixels, pixels),
            at,
            at + direction,
            up,
            0.01,
            0.01,
            100.0,
        );
        // The density falls from e^-0.5 at 5 m by e every 10 m of height
        let expected =
            1000.0 * (-0.05 * (-0.5f32).exp() * 10.0 / elevation.to_radians().sin()).exp();
        for (name, mut tracer) in tracers() {
            let mut image = RenderImage::new(pixels, pixels);
            tracer
                .trace(&scene, &camera, &mut image, &TraceSettings::default())
                .unwrap();
            let l = image.pixel(1, 1).y;
            assert!(
                (l - expected).abs() < 0.005 * expected,
                "{name} {elevation}: {l} {expected}"
            );
        }
    }
}

#[test]
fn tinted_glass_darkens_its_shadow_by_its_thickness() {
    // A 10 cm slab of glass 0.5 m over a white floor, the sun straight above; seen from below the slab
    let slab = |medium: Option<RtMedium>| {
        let mut scene = empty(0.0);
        scene.add_light(RtLight::Directional {
            direction: vec3(0.0, -1.0, 0.0),
            color: vec3(1.0, 1.0, 1.0),
            illuminance: 10_000.0,
        });
        let floor = scene.add_material(RtMaterial::diffuse(vec3(0.8, 0.8, 0.8)));
        let square = scene.add_mesh(&CpuMesh::square()).unwrap();
        scene
            .add_instance(
                square,
                floor,
                Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(20.0),
            )
            .unwrap();
        let glass = scene.add_material(RtMaterial {
            base_color: vec3(1.0, 1.0, 1.0),
            roughness: 0.0,
            transmission: 1.0,
            thin_walled: false,
            medium,
            ..Default::default()
        });
        let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
        scene
            .add_instance(
                cube,
                glass,
                Mat4::from_translation(vec3(0.0, 0.55, 0.0))
                    * Mat4::from_nonuniform_scale(1.0, 0.05, 1.0),
            )
            .unwrap();
        scene
    };
    let floor = |tracer: &mut dyn Tracer, scene: &RtScene, photons: u32| {
        let camera = Camera::new_orthographic(
            Viewport::new_at_origo(8, 8),
            vec3(0.0, 0.2, 0.0),
            vec3(0.0, -0.8, 0.0),
            vec3(0.0, 0.0, -1.0),
            0.5,
            0.01,
            1.0,
        );
        let mut image = RenderImage::new(8, 8);
        let settings = TraceSettings {
            samples_per_call: 2,
            max_bounces: 4,
            caustic_photons: photons,
            caustic_radius: 0.05,
            ..Default::default()
        };
        for _ in 0..if photons > 0 { 12 } else { 4 } {
            tracer.trace(scene, &camera, &mut image, &settings).unwrap();
        }
        let mut sum = vec3(0.0, 0.0, 0.0);
        for y in 0..8 {
            for x in 0..8 {
                let p = image.pixel(x, y);
                sum += vec3(p.x, p.y, p.z);
            }
        }
        sum / 64.0
    };
    // Green passes half of it in 10 cm, red and blue a tenth
    let tint = RtMedium::absorbing(vec3(0.1, 0.5, 0.1), 0.1);
    for (name, mut tracer) in tracers() {
        for photons in [0, 40_000] {
            let clear = floor(tracer.as_mut(), &slab(None), photons);
            let tinted = floor(tracer.as_mut(), &slab(Some(tint)), photons);
            let ratio = vec3(tinted.x / clear.x, tinted.y / clear.y, tinted.z / clear.z);
            // A little light goes back and forth between floor and glass, a bit more through the glass
            assert!(
                (ratio.y - 0.5).abs() < 0.03
                    && (ratio.x - 0.1).abs() < 0.02
                    && (ratio.z - 0.1).abs() < 0.02,
                "{name} with {photons} photons: {ratio:?}"
            );
        }
    }
}

#[test]
fn white_fog_over_the_scene_in_a_white_world_disappears() {
    let mut scene = empty(1000.0);
    scene.set_fog(Some(RtFog {
        medium: RtMedium::fog(5.0, 1.0, 0.6),
        height: 0.0,
        scale_height: 20.0,
    }));
    // On white ground, 1 m below the line of sight, which keeps the paths out of the ever denser fog below
    let white = scene.add_material(RtMaterial::diffuse(vec3(1.0, 1.0, 1.0)));
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            square,
            white,
            Mat4::from_translation(vec3(0.0, -1.0, 0.0))
                * Mat4::from_angle_x(degrees(-90.0))
                * Mat4::from_scale(1000.0),
        )
        .unwrap();
    for (name, mut tracer) in tracers() {
        let l = through_the_middle(tracer.as_mut(), &scene, 1.5, 256, 8192).y;
        assert!((l - 1000.0).abs() < 20.0, "{name}: {l}");
    }
}

/// A box from -size to size filled with an absorbing medium that light passes unbent.
fn add_absorbing_box(scene: &mut RtScene, absorption: Vec3, size: f32, priority: u32) {
    let material = scene.add_material(RtMaterial {
        medium_priority: priority,
        ..RtMaterial::medium_boundary(RtMedium {
            absorption,
            scattering: vec3(0.0, 0.0, 0.0),
            anisotropy: 0.0,
        })
    });
    let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
    scene
        .add_instance(cube, material, Mat4::from_scale(size))
        .unwrap();
}

#[test]
fn a_camera_inside_smoke_sees_through_it() {
    let mut scene = empty(1000.0);
    let sigma = vec3(0.2, 0.5, 1.0);
    add_absorbing_box(&mut scene, sigma, 1.0, 0);
    // From the middle along -z: 1 m to the wall
    let camera = Camera::new_orthographic(
        Viewport::new_at_origo(4, 4),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, -1.0),
        vec3(0.0, 1.0, 0.0),
        0.05,
        0.01,
        100.0,
    );
    let expected = vec3((-sigma.x).exp(), (-sigma.y).exp(), (-sigma.z).exp()) * 1000.0;
    for (name, mut tracer) in tracers() {
        let mut image = RenderImage::new(4, 4);
        tracer
            .trace(&scene, &camera, &mut image, &TraceSettings::default())
            .unwrap();
        let p = image.pixel(1, 1);
        let l = vec3(p.x, p.y, p.z);
        assert!(
            (l - expected).magnitude() < 0.01 * expected.magnitude(),
            "{name}: {l:?} {expected:?}"
        );
    }
}

#[test]
fn ice_in_water_by_priority() {
    // A cube of water 2 m wide with one of ice 1 m wide in it, seen through the middle
    let (water, ice) = (vec3(0.4, 0.1, 0.1), vec3(0.1, 0.1, 0.6));
    for (ice_priority, expected_depth) in [
        // The ice wins: 1 m of water, 1 m of ice
        (1, water + ice),
        // The water wins: the ice is not there
        (0, water * 2.0),
    ] {
        let mut scene = empty(1000.0);
        add_absorbing_box(
            &mut scene,
            water,
            1.0,
            if ice_priority == 1 { 0 } else { 1 },
        );
        add_absorbing_box(&mut scene, ice, 0.5, ice_priority);
        let expected = vec3(
            (-expected_depth.x).exp(),
            (-expected_depth.y).exp(),
            (-expected_depth.z).exp(),
        ) * 1000.0;
        for (name, mut tracer) in tracers() {
            let l = through_the_middle(tracer.as_mut(), &scene, 0.2, 4, 4);
            assert!(
                (l - expected).magnitude() < 0.01 * expected.magnitude(),
                "{name} ice priority {ice_priority}: {l:?} {expected:?}"
            );
        }
    }
}

#[test]
fn the_shadow_of_ice_in_water() {
    // The boxes 2 m over a white floor in the sun from straight above, the floor seen from below the boxes
    let (water, ice) = (vec3(0.3, 0.1, 0.1), vec3(0.1, 0.1, 0.5));
    let floor_under = |boxes: bool| {
        let mut scene = empty(0.0);
        scene.add_light(RtLight::Directional {
            direction: vec3(0.0, -1.0, 0.0),
            color: vec3(1.0, 1.0, 1.0),
            illuminance: 10_000.0,
        });
        let white = scene.add_material(RtMaterial::diffuse(vec3(0.8, 0.8, 0.8)));
        let square = scene.add_mesh(&CpuMesh::square()).unwrap();
        scene
            .add_instance(
                square,
                white,
                Mat4::from_translation(vec3(0.0, -3.0, 0.0))
                    * Mat4::from_angle_x(degrees(-90.0))
                    * Mat4::from_scale(50.0),
            )
            .unwrap();
        if boxes {
            add_absorbing_box(&mut scene, water, 1.0, 0);
            add_absorbing_box(&mut scene, ice, 0.5, 1);
        }
        scene
    };
    let camera = Camera::new_orthographic(
        Viewport::new_at_origo(4, 4),
        vec3(0.0, -1.5, 0.0),
        vec3(0.0, -2.5, 0.0),
        vec3(0.0, 0.0, -1.0),
        0.1,
        0.01,
        10.0,
    );
    let expected = water + ice;
    for (name, mut tracer) in tracers() {
        let mut lit = [vec3(0.0, 0.0, 0.0); 2];
        for (i, boxes) in [false, true].into_iter().enumerate() {
            let mut image = RenderImage::new(4, 4);
            let settings = TraceSettings {
                samples_per_call: 4,
                max_bounces: 1,
                ..Default::default()
            };
            tracer
                .trace(&floor_under(boxes), &camera, &mut image, &settings)
                .unwrap();
            let p = image.pixel(1, 1);
            lit[i] = vec3(p.x, p.y, p.z);
        }
        let ratio = vec3(
            lit[1].x / lit[0].x,
            lit[1].y / lit[0].y,
            lit[1].z / lit[0].z,
        );
        let want = vec3(
            (-expected.x).exp(),
            (-expected.y).exp(),
            (-expected.z).exp(),
        );
        assert!(
            (ratio - want).magnitude() < 0.01,
            "{name}: {ratio:?} {want:?}"
        );
    }
}

#[test]
fn a_pane_of_glass_lying_on_a_black_floor_does_not_let_the_light_through() {
    // A white world above and below a black floor, a 2 cm pane of glass lying on it without a gap
    let mut scene = empty(1000.0);
    let black = scene.add_material(RtMaterial::diffuse(vec3(0.0, 0.0, 0.0)));
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            square,
            black,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(10.0),
        )
        .unwrap();
    let glass = scene.add_material(RtMaterial {
        thin_walled: false,
        ..RtMaterial::glass()
    });
    let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
    scene
        .add_instance(
            cube,
            glass,
            Mat4::from_translation(vec3(0.0, 0.01, 0.0))
                * Mat4::from_nonuniform_scale(1.0, 0.01, 1.0),
        )
        .unwrap();
    // Looking down a little from the side, so the view does not go straight back up
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(16, 16),
        vec3(0.0, 2.0, 1.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(10.0),
        0.01,
        10.0,
    );
    for (name, mut tracer) in tracers() {
        let mut image = RenderImage::new(16, 16);
        let settings = TraceSettings {
            samples_per_call: 16,
            max_bounces: 8,
            ..Default::default()
        };
        tracer
            .trace(&scene, &camera, &mut image, &settings)
            .unwrap();
        let mut mean = 0.0;
        for y in 0..16 {
            for x in 0..16 {
                mean += image.pixel(x, y).y / 256.0;
            }
        }
        // Only the reflections of the glass, a few percent of the world
        assert!(mean < 120.0, "{name}: {mean}");
    }
}
