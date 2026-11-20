//!
//! A pergola in a hazy garden in the evening sun: sunbeams between the slats of its roof in the haze, a basin of
//! water that darkens and colors with depth, and a ball of smoke.
//!

use crate::*;

/// The pergola with its camera and exposure, see [super::DemoScene].
pub fn demo() -> super::DemoScene {
    super::DemoScene {
        name: "pergola",
        title: "Pergola in the haze",
        description: "Sunbeams between the slats of a pergola in the evening haze, a basin of water that darkens \
                      with depth and a ball of smoke: participating media.",
        scene: scene(),
        eye: vec3(5.2, 1.4, 4.6),
        target: vec3(-0.6, 1.3, -0.2),
        field_of_view: degrees(50.0),
        z_near: 0.05,
        z_far: 200.0,
        tone_mapping: ToneMapping {
            exposure_ev100: 13.5,
            ..Default::default()
        },
        settings: TraceSettings {
            max_bounces: 6,
            ..Default::default()
        },
    }
}

/// The scene, Y up, in meters.
pub fn scene() -> RtScene {
    let mut scene = RtScene::new();
    // Berlin, a September afternoon: the sun low behind the pergola
    let sky = Sky::new(
        GeoLocation::new(52.52, 13.405),
        LocalDateTime::new(2026, 9, 12, 17, 0, 2.0),
        SceneOrientation::default(),
    );
    scene.set_environment(RtEnvironment::Sky(sky));
    // Haze: half the light left after 25 m near the ground, thinning out over 3 m of height
    scene.set_fog(Some(RtFog {
        medium: RtMedium::fog(25.0, 0.9, 0.5),
        height: 0.0,
        scale_height: 3.0,
    }));

    let lawn = scene.add_material(RtMaterial::diffuse(vec3(0.18, 0.26, 0.09)));
    let gravel = scene.add_material(RtMaterial {
        roughness: 0.9,
        ..RtMaterial::diffuse(vec3(0.5, 0.47, 0.42))
    });
    let wood = scene.add_material(RtMaterial {
        roughness: 0.7,
        ..RtMaterial::diffuse(vec3(0.33, 0.22, 0.13))
    });
    let stone = scene.add_material(RtMaterial {
        roughness: 0.6,
        ..RtMaterial::diffuse(vec3(0.62, 0.6, 0.56))
    });
    // Clear water that keeps 60 % of the green and a fifth of the red after a meter
    let water = scene.add_material(RtMaterial {
        thin_walled: false,
        index_of_refraction: 1.33,
        medium: Some(RtMedium::absorbing(vec3(0.2, 0.6, 0.5), 1.0)),
        ..RtMaterial::glass()
    });
    let smoke = scene.add_material(RtMaterial::medium_boundary(RtMedium::fog(0.25, 0.95, 0.3)));

    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
    let ball = scene.add_mesh(&CpuMesh::sphere(48)).unwrap();
    let place = |scene: &mut RtScene, mesh, material, at: Vec3, size: Vec3| {
        scene
            .add_instance(
                mesh,
                material,
                Mat4::from_translation(at)
                    * Mat4::from_nonuniform_scale(size.x / 2.0, size.y / 2.0, size.z / 2.0),
            )
            .unwrap();
    };
    scene
        .add_instance(
            square,
            lawn,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(60.0),
        )
        .unwrap();
    // The gravel under the pergola, 4 by 3 m
    place(
        &mut scene,
        cube,
        gravel,
        vec3(0.0, 0.01, 0.0),
        vec3(4.4, 0.02, 3.4),
    );
    // Four posts and two beams, then the slats of the roof
    for x in [-2.0, 2.0] {
        for z in [-1.5, 1.5] {
            place(
                &mut scene,
                cube,
                wood,
                vec3(x, 1.25, z),
                vec3(0.14, 2.5, 0.14),
            );
        }
        place(
            &mut scene,
            cube,
            wood,
            vec3(x, 2.58, 0.0),
            vec3(0.14, 0.16, 3.6),
        );
    }
    for i in 0..13 {
        let x = -2.1 + i as f32 * 0.35;
        place(
            &mut scene,
            cube,
            wood,
            vec3(x, 2.72, 0.0),
            vec3(0.08, 0.12, 3.8),
        );
    }
    // The basin: stone rim, water 40 cm deep
    place(
        &mut scene,
        cube,
        stone,
        vec3(0.6, 0.02, 0.2),
        vec3(1.8, 0.04, 1.0),
    );
    for (at, size) in [
        (vec3(0.6, 0.23, -0.34), vec3(1.8, 0.46, 0.08)),
        (vec3(0.6, 0.23, 0.74), vec3(1.8, 0.46, 0.08)),
        (vec3(-0.34, 0.23, 0.2), vec3(0.08, 0.46, 1.0)),
        (vec3(1.54, 0.23, 0.2), vec3(0.08, 0.46, 1.0)),
    ] {
        place(&mut scene, cube, stone, at, size);
    }
    place(
        &mut scene,
        cube,
        water,
        vec3(0.6, 0.241, 0.2),
        vec3(1.79, 0.4, 0.99),
    );
    // A ball of smoke over the gravel
    place(
        &mut scene,
        ball,
        smoke,
        vec3(-1.0, 0.9, -0.4),
        vec3(1.0, 1.0, 1.0),
    );
    scene
}
