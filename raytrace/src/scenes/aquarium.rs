//!
//! Media inside media: a glass tank of water with two ice cubes floating in it, the water a little larger than the
//! inside of the tank, the glass winning there by its priority; next to it a pane of glass lying on the floor without
//! a gap.
//!

use crate::*;

/// The aquarium with its camera and exposure, see [super::DemoScene].
pub fn demo() -> super::DemoScene {
    super::DemoScene {
        name: "aquarium",
        title: "Aquarium and glass on the floor",
        description: "A glass tank of water with floating ice cubes, and a pane of glass lying on the floor: \
                      nested media and touching surfaces.",
        scene: scene(),
        eye: vec3(0.42, 0.3, 0.55),
        target: vec3(0.02, 0.09, 0.05),
        field_of_view: degrees(45.0),
        z_near: 0.01,
        z_far: 100.0,
        tone_mapping: ToneMapping {
            exposure_ev100: 14.6,
            ..Default::default()
        },
        settings: TraceSettings {
            max_bounces: 12,
            caustic_photons: 100_000,
            ..Default::default()
        },
    }
}

/// The scene, Y up, in meters.
pub fn scene() -> RtScene {
    let mut scene = RtScene::new();
    let sky = Sky::new(
        GeoLocation::new(52.52, 13.405),
        LocalDateTime::new(2026, 6, 21, 15, 0, 2.0),
        SceneOrientation::default(),
    );
    scene.set_environment(RtEnvironment::Sky(sky));

    let wood = scene.add_material(RtMaterial {
        roughness: 0.6,
        ..RtMaterial::diffuse(vec3(0.3, 0.19, 0.11))
    });
    // Priorities: glass over ice over water
    let glass = scene.add_material(RtMaterial {
        thin_walled: false,
        medium: Some(RtMedium::absorbing(vec3(0.85, 0.95, 0.9), 0.1)),
        medium_priority: 3,
        ..RtMaterial::glass()
    });
    let ice = scene.add_material(RtMaterial {
        thin_walled: false,
        index_of_refraction: 1.31,
        medium: Some(RtMedium::absorbing(vec3(0.8, 0.9, 1.0), 0.2)),
        medium_priority: 2,
        ..RtMaterial::glass()
    });
    let water = scene.add_material(RtMaterial {
        thin_walled: false,
        index_of_refraction: 1.33,
        medium: Some(RtMedium::absorbing(vec3(0.25, 0.7, 0.6), 0.2)),
        medium_priority: 1,
        ..RtMaterial::glass()
    });

    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
    let place = |scene: &mut RtScene, material, at: Vec3, size: Vec3, turn: f32| {
        scene
            .add_instance(
                cube,
                material,
                Mat4::from_translation(at)
                    * Mat4::from_angle_y(degrees(turn))
                    * Mat4::from_nonuniform_scale(size.x / 2.0, size.y / 2.0, size.z / 2.0),
            )
            .unwrap();
    };
    scene
        .add_instance(
            square,
            wood,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(5.0),
        )
        .unwrap();
    // The tank, 30 by 20 by 20 cm with walls of 8 mm, standing on the floor
    let (w, d, h, wall) = (0.3, 0.2, 0.2, 0.008);
    place(
        &mut scene,
        glass,
        vec3(0.0, wall / 2.0, 0.0),
        vec3(w, wall, d),
        0.0,
    );
    for z in [-1.0, 1.0] {
        place(
            &mut scene,
            glass,
            vec3(0.0, h / 2.0, z * (d - wall) / 2.0),
            vec3(w, h, wall),
            0.0,
        );
    }
    for x in [-1.0, 1.0] {
        place(
            &mut scene,
            glass,
            vec3(x * (w - wall) / 2.0, h / 2.0, 0.0),
            vec3(wall, h, d - 2.0 * wall),
            0.0,
        );
    }
    // The water 15 cm deep, reaching into the walls and the bottom by half their thickness
    let depth = 0.15;
    place(
        &mut scene,
        water,
        vec3(
            0.0,
            wall / 2.0 + (depth - wall / 2.0) / 2.0 + wall / 4.0,
            0.0,
        ),
        vec3(w - wall, depth, d - wall),
        0.0,
    );
    // Two ice cubes of 4 cm, a tenth of each above the water
    for (x, z, turn) in [(-0.05, 0.02, 20.0), (0.06, -0.03, -35.0)] {
        place(
            &mut scene,
            ice,
            vec3(x, wall + depth - 0.04 * 0.4, z),
            vec3(0.04, 0.04, 0.04),
            turn,
        );
    }
    // A pane of glass of 6 mm lying on the floor
    place(
        &mut scene,
        glass,
        vec3(0.0, 0.003, 0.28),
        vec3(0.3, 0.006, 0.2),
        10.0,
    );
    scene
}
