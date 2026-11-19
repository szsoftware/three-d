//!
//! A pergola in a hazy garden in the evening sun, rendered with the CPU reference tracer: sunbeams between the slats of
//! its roof in the haze over the scene, a basin of water that darkens and colors with depth, and a ball of smoke.
//!
//! cargo run --release -p three-d-raytrace --example volumes -- [samples] [width] [height]
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
        vec3(5.2, 1.4, 4.6),
        vec3(-0.6, 1.3, -0.2),
        vec3(0.0, 1.0, 0.0),
        degrees(50.0),
        0.05,
        200.0,
    );
    let tone_mapping = ToneMapping {
        exposure_ev100: 13.5,
        ..Default::default()
    };
    let mut tracer = CpuReferenceTracer::new();
    let mut image = RenderImage::with_features(width, height);
    let settings = TraceSettings {
        samples_per_call: 1,
        max_bounces: 6,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    while image.sample_count() < samples {
        tracer
            .trace(&scene, &camera, &mut image, &settings)
            .unwrap();
    }
    println!("volumes: {samples} samples in {:.1?}", start.elapsed());
    image.save_png("volumes.png", &tone_mapping).unwrap();
    AtrousDenoiser::default()
        .denoise(&image)
        .unwrap()
        .save_png("volumes_denoised.png", &tone_mapping)
        .unwrap();
}

fn scene() -> RtScene {
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
