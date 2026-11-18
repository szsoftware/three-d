//!
//! A small garden scene for the example and the reference image test: a house with glass windows and a gable roof, a
//! terrace, a hedge and instanced trees on a lawn, in Berlin on a summer afternoon.
//!

use three_d_raytrace::*;

/// Size of the image the camera is made for.
pub fn camera(width: u32, height: u32) -> Camera {
    Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        vec3(15.0, 5.5, 19.0),
        vec3(0.0, 1.8, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        1000.0,
    )
}

/// The sky over Berlin on midsummer day at 17:00 summer time, the sun in the west south west.
pub fn sky() -> Sky {
    Sky::new(
        GeoLocation::new(52.52, 13.405),
        LocalDateTime::new(2026, 6, 21, 17, 0, 2.0),
        SceneOrientation::default(),
    )
}

/// The scene, Y up and north along -Z, in meters.
pub fn scene() -> RtScene {
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Sky(
        sky().with_ground_albedo(vec3(0.08, 0.15, 0.05)),
    ));

    let lawn = scene.add_material(RtMaterial {
        roughness: 0.9,
        specular: 0.3,
        ..RtMaterial::diffuse(vec3(0.10, 0.22, 0.05))
    });
    let plaster = scene.add_material(RtMaterial {
        roughness: 0.8,
        ..RtMaterial::diffuse(vec3(0.78, 0.74, 0.66))
    });
    let tiles = scene.add_material(RtMaterial {
        base_color: vec3(0.35, 0.09, 0.05),
        roughness: 0.55,
        ..Default::default()
    });
    let paving = scene.add_texture(paving());
    let stone = scene.add_material(RtMaterial {
        base_color: vec3(0.45, 0.43, 0.40),
        base_color_texture: Some(paving),
        roughness: 0.7,
        ..Default::default()
    });
    let interior = scene.add_material(RtMaterial::diffuse(vec3(0.05, 0.05, 0.05)));
    let glass = scene.add_material(RtMaterial::glass());
    let bark = scene.add_material(RtMaterial::diffuse(vec3(0.12, 0.08, 0.05)));
    let leaves = scene.add_material(RtMaterial {
        roughness: 0.7,
        specular: 0.5,
        ..RtMaterial::diffuse(vec3(0.06, 0.16, 0.04))
    });
    let hedge = scene.add_material(RtMaterial {
        roughness: 0.9,
        ..RtMaterial::diffuse(vec3(0.04, 0.12, 0.03))
    });

    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
    let sphere = scene.add_mesh(&CpuMesh::sphere(24)).unwrap();
    let cylinder = scene.add_mesh(&CpuMesh::cylinder(16)).unwrap();
    let roof = scene.add_mesh(&gable_roof()).unwrap();

    // Lawn, the square lies in the xy plane facing +z
    scene
        .add_instance(
            square,
            lawn,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(200.0),
        )
        .unwrap();
    // House 10 x 3 x 7 meters with the roof on top
    let house =
        Mat4::from_translation(vec3(0.0, 1.5, 0.0)) * Mat4::from_nonuniform_scale(5.0, 1.5, 3.5);
    scene.add_instance(cube, plaster, house).unwrap();
    scene
        .add_instance(
            roof,
            tiles,
            Mat4::from_translation(vec3(0.0, 3.0, 0.0))
                * Mat4::from_nonuniform_scale(5.4, 2.2, 3.9),
        )
        .unwrap();
    // Windows on the south and east side: dark rooms behind clear glass
    let south_windows = [(-3.0, 1.6), (0.0, 1.6), (3.0, 1.6)];
    for (x, y) in south_windows {
        let at = |z: f32| {
            Mat4::from_translation(vec3(x, y, z)) * Mat4::from_nonuniform_scale(0.6, 0.7, 1.0)
        };
        scene.add_instance(square, interior, at(3.505)).unwrap();
        scene.add_instance(square, glass, at(3.52)).unwrap();
    }
    for z in [-1.5, 1.5] {
        let at = |x: f32| {
            Mat4::from_translation(vec3(x, 1.6, z))
                * Mat4::from_angle_y(degrees(90.0))
                * Mat4::from_nonuniform_scale(0.6, 0.7, 1.0)
        };
        scene.add_instance(square, interior, at(5.005)).unwrap();
        scene.add_instance(square, glass, at(5.02)).unwrap();
    }
    // Terrace in front of the house
    scene
        .add_instance(
            cube,
            stone,
            Mat4::from_translation(vec3(0.0, 0.05, 5.5))
                * Mat4::from_nonuniform_scale(4.0, 0.05, 2.0),
        )
        .unwrap();
    // Hedge along the east side
    scene
        .add_instance(
            cube,
            hedge,
            Mat4::from_translation(vec3(11.0, 0.8, 2.0))
                * Mat4::from_nonuniform_scale(0.5, 0.8, 9.0),
        )
        .unwrap();
    // Trees: one trunk and one crown mesh for all of them
    let trees = [
        (-9.0, -4.0, 1.0),
        (-8.0, 4.0, 0.8),
        (-12.0, 9.0, 1.2),
        (6.0, 10.0, 0.9),
        (8.0, -7.0, 1.1),
        (-3.0, -9.0, 1.0),
        (2.0, 13.0, 0.7),
    ];
    for (x, z, s) in trees {
        let base = Mat4::from_translation(vec3(x, 0.0, z)) * Mat4::from_scale(s);
        scene
            .add_instance(
                cylinder,
                bark,
                base * Mat4::from_angle_z(degrees(90.0))
                    * Mat4::from_nonuniform_scale(3.0, 0.18, 0.18),
            )
            .unwrap();
        scene
            .add_instance(
                sphere,
                leaves,
                base * Mat4::from_translation(vec3(0.0, 4.0, 0.0))
                    * Mat4::from_nonuniform_scale(1.8, 2.0, 1.8),
            )
            .unwrap();
    }
    scene
}

/// A gable roof over [-1, 1] in x and z, from height 0 to 1, ridge along x.
/// Square paving stones of slightly different brightness with dark joints, 8 × 4 stones.
fn paving() -> RtTexture {
    let (stones_x, stones_y, size) = (8u32, 4u32, 16u32);
    let (width, height) = (stones_x * size, stones_y * size);
    let mut texels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let (sx, sy) = (x / size, y / size);
            let joint = x % size == 0 || y % size == 0;
            let shade = if joint {
                110
            } else {
                // A fixed pattern instead of random numbers, so the image is the same everywhere
                225 + ((sx * 7 + sy * 13) % 5) as u8 * 6
            };
            texels.push([shade, shade, shade, 255]);
        }
    }
    RtTexture::new(width, height, texels)
}

fn gable_roof() -> CpuMesh {
    let p = [
        vec3(-1.0, 0.0, 1.0),
        vec3(1.0, 0.0, 1.0),
        vec3(1.0, 1.0, 0.0),
        vec3(-1.0, 1.0, 0.0),
        vec3(-1.0, 0.0, -1.0),
        vec3(1.0, 0.0, -1.0),
    ];
    let indices: Vec<u32> = vec![
        0, 1, 2, 0, 2, 3, // south slope
        5, 4, 3, 5, 3, 2, // north slope
        4, 0, 3, // west gable
        1, 5, 2, // east gable
    ];
    // Flat shading: one copy of each vertex per triangle
    let positions: Vec<Vec3> = indices.iter().map(|&i| p[i as usize]).collect();
    CpuMesh {
        positions: three_d_asset::Positions::F32(positions),
        ..Default::default()
    }
}
