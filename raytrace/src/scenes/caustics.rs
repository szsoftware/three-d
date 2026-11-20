//!
//! A glass ball and a glass block on a stone terrace in the afternoon sun: the light the glass focuses onto the
//! stone (caustics), with depth of field focused on the ball at f/1.4 if wanted.
//!

use crate::*;

/// The glass on the terrace with its camera and exposure, see [super::DemoScene].
pub fn demo() -> super::DemoScene {
    super::DemoScene {
        name: "caustics",
        title: "Glass in the sun",
        description: "A glass ball and a glass block on a stone terrace: the light they focus onto the stone \
                      (caustics).",
        scene: scene(),
        eye: vec3(1.6, 1.3, 2.2),
        target: vec3(-0.1, 0.15, 0.0),
        field_of_view: degrees(40.0),
        z_near: 0.05,
        z_far: 100.0,
        tone_mapping: ToneMapping {
            exposure_ev100: 14.5,
            ..Default::default()
        },
        settings: TraceSettings {
            caustic_photons: 200_000,
            caustic_radius: 0.03,
            ..Default::default()
        },
    }
}

/// Depth of field focused on the glass ball at f/1.4, for the camera of [demo] or one like it.
pub fn depth_of_field(camera: &Camera) -> DepthOfField {
    DepthOfField::focused_on(camera, vec3(0.0, 0.25, 0.0), 1.4)
}

/// The scene, Y up, in meters.
pub fn scene() -> RtScene {
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
