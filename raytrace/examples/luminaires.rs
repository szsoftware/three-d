//!
//! Three downlights with a measured light distribution in front of a wall, rendered with the CPU reference tracer:
//! the scallops on the wall come from the IES profile.
//!
//! cargo run --release -p three-d-raytrace --example luminaires -- [samples] [width] [height]
//!

use std::sync::Arc;
use three_d_raytrace::*;

/// A downlight with a 60° beam, measured every 10° in one plane (rotationally symmetric).
const DOWNLIGHT: &str = "IESNA:LM-63-2002
[TEST] example downlight
TILT=NONE
1 800 1.0 10 1 1 2 0.1 0.1 0.0
1.0 1.0 10
0 10 20 30 40 50 60 70 80 90
0
900 880 800 600 250 60 20 8 2 0
";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let width: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let height: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(400);

    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 0.0,
    });
    let plaster = scene.add_material(RtMaterial {
        roughness: 0.9,
        ..RtMaterial::diffuse(vec3(0.8, 0.78, 0.74))
    });
    let floor = scene.add_material(RtMaterial {
        roughness: 0.6,
        ..RtMaterial::diffuse(vec3(0.35, 0.3, 0.25))
    });
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    // Floor, back wall and ceiling of a room 6 m wide and 2.8 m high
    scene
        .add_instance(
            square,
            floor,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(3.0),
        )
        .unwrap();
    scene
        .add_instance(
            square,
            plaster,
            Mat4::from_translation(vec3(0.0, 1.4, -1.5))
                * Mat4::from_nonuniform_scale(3.0, 1.4, 1.0),
        )
        .unwrap();
    scene
        .add_instance(
            square,
            plaster,
            Mat4::from_translation(vec3(0.0, 2.8, 0.0))
                * Mat4::from_angle_x(degrees(90.0))
                * Mat4::from_scale(3.0),
        )
        .unwrap();
    let profile = Arc::new(IesProfile::parse(DOWNLIGHT).unwrap());
    println!("The profile sends out {:.0} lm", profile.flux());
    for x in [-1.8, 0.0, 1.8] {
        scene.add_light(RtLight::luminaire(
            vec3(x, 2.75, -1.1),
            vec3(0.0, -1.0, 0.0),
            vec3(1.0, 0.0, 0.0),
            profile.clone(),
        ));
    }
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        vec3(0.0, 1.5, 4.5),
        vec3(0.0, 1.3, -1.5),
        vec3(0.0, 1.0, 0.0),
        degrees(50.0),
        0.05,
        100.0,
    );
    let mut tracer = CpuReferenceTracer::new();
    let mut image = RenderImage::with_features(width, height);
    let settings = TraceSettings {
        samples_per_call: 8,
        max_bounces: 6,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    while image.sample_count() < samples {
        tracer
            .trace(&scene, &camera, &mut image, &settings)
            .unwrap();
    }
    println!("{samples} samples in {:.1?}", start.elapsed());
    let tone_mapping = ToneMapping {
        exposure_ev100: 6.0,
        ..Default::default()
    };
    AtrousDenoiser::default()
        .denoise(&image)
        .unwrap()
        .save_png("luminaires.png", &tone_mapping)
        .unwrap();
    println!("Wrote luminaires.png");
}
