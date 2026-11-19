//!
//! Media inside media with the CPU reference tracer: a glass tank of water with two ice cubes floating in it, the water
//! a little larger than the inside of the tank, the glass winning there by its priority; next to it a pane of glass
//! lying on the floor without a gap.
//!
//! cargo run --release -p three-d-raytrace --example nested_media -- [samples] [width] [height]
//!

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let width: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let height: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(400);

    let scene = scene();
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        vec3(0.42, 0.3, 0.55),
        vec3(0.02, 0.09, 0.05),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.01,
        100.0,
    );
    let tone_mapping = ToneMapping {
        exposure_ev100: 14.6,
        ..Default::default()
    };
    let mut tracer = CpuReferenceTracer::new();
    let mut image = RenderImage::with_features(width, height);
    let settings = TraceSettings {
        samples_per_call: 1,
        max_bounces: 12,
        caustic_photons: 100_000,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    while image.sample_count() < samples {
        tracer
            .trace(&scene, &camera, &mut image, &settings)
            .unwrap();
    }
    println!("nested_media: {samples} samples in {:.1?}", start.elapsed());
    image.save_png("nested_media.png", &tone_mapping).unwrap();
    AtrousDenoiser::default()
        .denoise(&image)
        .unwrap()
        .save_png("nested_media_denoised.png", &tone_mapping)
        .unwrap();
}

fn scene() -> RtScene {
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
