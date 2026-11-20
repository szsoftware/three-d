//!
//! Three downlights with a measured light distribution (IES) in front of a wall: the scallops on the wall come from
//! the profile.
//!

use crate::*;
use std::sync::Arc;

/// A downlight with a 60° beam, measured every 10° in one plane (rotationally symmetric).
pub const DOWNLIGHT: &str = "IESNA:LM-63-2002
[TEST] example downlight
TILT=NONE
1 800 1.0 10 1 1 2 0.1 0.1 0.0
1.0 1.0 10
0 10 20 30 40 50 60 70 80 90
0
900 880 800 600 250 60 20 8 2 0
";

/// The downlights with their camera and exposure, see [super::DemoScene].
pub fn demo() -> super::DemoScene {
    super::DemoScene {
        name: "downlights",
        title: "Downlights on a wall",
        description: "Three downlights with a measured light distribution (IES) in a dark room: the scallops on \
                      the wall come from the profile.",
        scene: scene(),
        eye: vec3(0.0, 1.5, 4.5),
        target: vec3(0.0, 1.3, -1.5),
        field_of_view: degrees(50.0),
        z_near: 0.05,
        z_far: 100.0,
        tone_mapping: ToneMapping {
            exposure_ev100: 6.0,
            ..Default::default()
        },
        settings: TraceSettings {
            max_bounces: 6,
            ..Default::default()
        },
    }
}

/// The scene, Y up, in meters: floor, back wall and ceiling of a room 6 m wide and 2.8 m high.
pub fn scene() -> RtScene {
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
    for x in [-1.8, 0.0, 1.8] {
        scene.add_light(RtLight::luminaire(
            vec3(x, 2.75, -1.1),
            vec3(0.0, -1.0, 0.0),
            vec3(1.0, 0.0, 0.0),
            profile.clone(),
        ));
    }
    scene
}
