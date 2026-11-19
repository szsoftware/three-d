//!
//! A glass ball and a glass block on a stone terrace in the afternoon sun, rendered with the CPU reference tracer
//! three times: without caustics (shadow rays pass the glass straight), with caustic photons, and with caustics and
//! depth of field focused on the ball at f/1.4.
//!
//! cargo run --release -p three-d-raytrace --example caustics -- [samples] [width] [height] [photons per call]
//!

use three_d_raytrace::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let width: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let height: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(400);
    let photons: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(200_000);

    let scene = scene();
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        vec3(1.6, 1.3, 2.2),
        vec3(-0.1, 0.15, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(40.0),
        0.05,
        100.0,
    );
    let tone_mapping = ToneMapping {
        exposure_ev100: 14.5,
        ..Default::default()
    };
    let focused = DepthOfField::focused_on(&camera, vec3(0.0, 0.25, 0.0), 1.4);
    for (name, caustic_photons, depth_of_field) in [
        ("caustics_off", 0, None),
        ("caustics_on", photons, None),
        ("caustics_dof", photons, Some(focused)),
    ] {
        let mut tracer = CpuReferenceTracer::new();
        let mut image = RenderImage::with_features(width, height);
        let settings = TraceSettings {
            samples_per_call: 1,
            caustic_photons,
            caustic_radius: 0.03,
            depth_of_field,
            ..Default::default()
        };
        let start = std::time::Instant::now();
        while image.sample_count() < samples {
            tracer
                .trace(&scene, &camera, &mut image, &settings)
                .unwrap();
        }
        println!("{name}: {samples} samples in {:.1?}", start.elapsed());
        image
            .save_png(format!("{name}.png"), &tone_mapping)
            .unwrap();
        AtrousDenoiser::default()
            .denoise(&image)
            .unwrap()
            .save_png(format!("{name}_denoised.png"), &tone_mapping)
            .unwrap();
    }
}

fn scene() -> RtScene {
    let mut scene = RtScene::new();
    // Berlin, late afternoon in May: the sun low enough for long caustics
    let sky = Sky::new(
        GeoLocation::new(52.52, 13.405),
        LocalDateTime::new(2026, 5, 20, 17, 30, 2.0),
        SceneOrientation::default(),
    );
    scene.set_environment(RtEnvironment::Sky(sky));
    let stone = scene.add_material(RtMaterial {
        roughness: 0.8,
        ..RtMaterial::diffuse(vec3(0.55, 0.52, 0.47))
    });
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            square,
            stone,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(10.0),
        )
        .unwrap();
    let glass = scene.add_material(RtMaterial {
        thin_walled: false,
        ..RtMaterial::glass()
    });
    let tinted = scene.add_material(RtMaterial {
        base_color: vec3(0.55, 0.85, 0.75),
        thin_walled: false,
        ..RtMaterial::glass()
    });
    let ball = scene.add_mesh(&CpuMesh::sphere(64)).unwrap();
    scene
        .add_instance(
            ball,
            glass,
            Mat4::from_translation(vec3(0.0, 0.25, 0.0)) * Mat4::from_scale(0.25),
        )
        .unwrap();
    let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
    scene
        .add_instance(
            cube,
            tinted,
            // 2 mm above the stone: light leaving the glass through a face that lies on the floor would start
            // below it
            Mat4::from_translation(vec3(-0.7, 0.202, -0.4))
                * Mat4::from_angle_y(degrees(30.0))
                * Mat4::from_scale(0.2),
        )
        .unwrap();
    scene
}
