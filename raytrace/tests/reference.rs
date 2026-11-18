//!
//! Checks of the CPU reference tracer against values known from physics, and a reference image.
//!
//! Write the reference image again with `THREE_D_WRITE_REFERENCE=1 cargo test --release -p three-d-raytrace`.
//!

#[path = "../scenes/garden.rs"]
mod garden;

use three_d_raytrace::*;

fn ground_plane(scene: &mut RtScene, material: MaterialId) {
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            square,
            material,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(1000.0),
        )
        .unwrap();
}

/// A camera above the origin looking straight down.
fn camera_looking_down(size: u32) -> Camera {
    Camera::new_perspective(
        Viewport::new_at_origo(size, size),
        vec3(0.0, 2.0, 0.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, -1.0),
        degrees(20.0),
        0.1,
        100.0,
    )
}

fn render(
    scene: &RtScene,
    camera: &Camera,
    size: (u32, u32),
    samples: u32,
    max_bounces: u32,
) -> RenderImage {
    let mut render_image = RenderImage::new(size.0, size.1);
    CpuReferenceTracer::new()
        .trace(
            scene,
            camera,
            &mut render_image,
            &TraceSettings {
                samples_per_call: samples,
                max_bounces,
                ..Default::default()
            },
        )
        .unwrap();
    render_image
}

fn mean(render_image: &RenderImage) -> Vec3 {
    let pixels = render_image.pixels();
    pixels.iter().fold(vec3(0.0, 0.0, 0.0), |a, b| a + *b) / pixels.len() as f32
}

#[test]
fn white_furnace() {
    // A white diffuse sphere in a uniform environment disappears: every pixel shows the environment
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 1000.0,
    });
    let white = scene.add_material(RtMaterial::diffuse(vec3(1.0, 1.0, 1.0)));
    let sphere = scene.add_mesh(&CpuMesh::sphere(32)).unwrap();
    scene.add_instance(sphere, white, Mat4::identity()).unwrap();
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(16, 16),
        vec3(0.0, 0.0, 4.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(30.0),
        0.1,
        100.0,
    );
    let render_image = render(&scene, &camera, (16, 16), 256, 64);
    for p in render_image.pixels() {
        assert!((p.x - 1000.0).abs() < 60.0, "{p:?}");
    }
    assert!(
        (mean(&render_image).x - 1000.0).abs() < 5.0,
        "{:?}",
        mean(&render_image)
    );
}

#[test]
fn white_ground_under_the_sky() {
    // A white diffuse ground reflects the illuminance of sun and sky: L = E / π. The sky computes the same for its
    // ground with another albedo, by integrating over the sky instead of sampling it.
    let mut scene = RtScene::new();
    let sky = Sky::from_sun_position(
        SunPosition {
            azimuth: 200.0,
            elevation: 50.0,
        },
        SceneOrientation::default(),
    )
    .with_ground_albedo(vec3(1.0, 1.0, 1.0));
    let expected = sky.radiance(vec3(0.0, -1.0, 0.0));
    scene.set_environment(RtEnvironment::Sky(sky));
    let white = scene.add_material(RtMaterial::diffuse(vec3(1.0, 1.0, 1.0)));
    ground_plane(&mut scene, white);
    let got = mean(&render(&scene, &camera_looking_down(8), (8, 8), 512, 2));
    for c in 0..3 {
        assert!(
            (got[c] / expected[c] - 1.0).abs() < 0.02,
            "{got:?} vs {expected:?}"
        );
    }
}

#[test]
fn point_light_falls_off_with_the_square_of_the_distance() {
    for height in [1.0f32, 3.0] {
        let mut scene = RtScene::new();
        let grey = scene.add_material(RtMaterial::diffuse(vec3(0.5, 0.5, 0.5)));
        ground_plane(&mut scene, grey);
        scene.add_light(RtLight::Point {
            position: vec3(0.0, height, 0.0),
            color: vec3(1.0, 1.0, 1.0),
            intensity: 100.0,
        });
        let camera = Camera::new_perspective(
            Viewport::new_at_origo(1, 1),
            vec3(0.0, 0.5, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, -1.0),
            degrees(0.5),
            0.01,
            100.0,
        );
        let got = render(&scene, &camera, (1, 1), 16, 1).pixel(0, 0).x;
        let expected = 100.0 / (height * height) * 0.5 / std::f32::consts::PI;
        assert!(
            (got / expected - 1.0).abs() < 0.01,
            "height {height}: {got} vs {expected}"
        );
    }
}

#[test]
fn sunlight_passes_through_a_window_pane() {
    // Ground lit by a directional light from straight above, half of it under a pane of glass
    let mut scene = RtScene::new();
    let grey = scene.add_material(RtMaterial::diffuse(vec3(0.5, 0.5, 0.5)));
    let glass = scene.add_material(RtMaterial::glass());
    let wall = scene.add_material(RtMaterial::diffuse(vec3(0.5, 0.5, 0.5)));
    ground_plane(&mut scene, grey);
    let square = scene.add_mesh(&CpuMesh::square()).unwrap();
    let above = |x: f32| {
        Mat4::from_translation(vec3(x, 1.0, 0.0))
            * Mat4::from_angle_x(degrees(-90.0))
            * Mat4::from_scale(0.5)
    };
    let pane = scene.add_instance(square, glass, above(-0.5)).unwrap();
    scene.add_instance(square, wall, above(0.5)).unwrap();
    scene.add_light(RtLight::Directional {
        direction: vec3(0.0, -1.0, 0.0),
        color: vec3(1.0, 1.0, 1.0),
        illuminance: 1000.0,
    });
    // Look at the ground through the gap between the panes and the camera from below the panes
    let camera = |x: f32| {
        Camera::new_perspective(
            Viewport::new_at_origo(1, 1),
            vec3(x, 0.5, 0.0),
            vec3(x, 0.0, 0.0),
            vec3(0.0, 0.0, -1.0),
            degrees(1.0),
            0.01,
            100.0,
        )
    };
    let lit = 1000.0 * 0.5 / std::f32::consts::PI;
    let under_glass = render(&scene, &camera(-0.5), (1, 1), 4, 1).pixel(0, 0).x;
    let under_wall = render(&scene, &camera(0.5), (1, 1), 4, 1).pixel(0, 0).x;
    // Two surfaces of 4 % reflection each would be 0.92, one sheet is 0.96
    assert!(
        (under_glass / lit - 0.96).abs() < 0.01,
        "{under_glass} of {lit}"
    );
    assert_eq!(under_wall, 0.0);

    // Without transmission the pane casts a full shadow
    scene.set_instance_material(pane, wall).unwrap();
    assert_eq!(
        render(&scene, &camera(-0.5), (1, 1), 4, 1).pixel(0, 0).x,
        0.0
    );
}

#[test]
fn same_image_with_any_number_of_threads() {
    let scene = garden::scene();
    let camera = garden::camera(24, 16);
    let settings = TraceSettings {
        samples_per_call: 2,
        seed: 7,
        ..Default::default()
    };
    let mut a = RenderImage::new(24, 16);
    let mut b = RenderImage::new(24, 16);
    CpuReferenceTracer::with_threads(1)
        .trace(&scene, &camera, &mut a, &settings)
        .unwrap();
    CpuReferenceTracer::with_threads(3)
        .trace(&scene, &camera, &mut b, &settings)
        .unwrap();
    assert_eq!(a.pixels(), b.pixels());
}

#[test]
fn garden_reference_image() {
    let (width, height) = (96, 60);
    let scene = garden::scene();
    let camera = garden::camera(width, height);
    let render_image = render(&scene, &camera, (width, height), 64, 8);
    let tone_mapping = ToneMapping {
        exposure_ev100: 14.0,
        ..Default::default()
    };
    let image: Vec<u8> = render_image
        .to_srgb8(&tone_mapping)
        .into_iter()
        .flatten()
        .collect();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reference/garden.png");
    if std::env::var("THREE_D_WRITE_REFERENCE").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbaImage::from_raw(width, height, image)
            .unwrap()
            .save(&path)
            .unwrap();
        return;
    }
    let reference = image::open(&path).expect("reference image").to_rgba8();
    assert_eq!(reference.dimensions(), (width, height));
    // Other platforms round some functions differently, which changes single paths but not the picture: compare
    // blocks of 4 x 4 pixels
    let block = |data: &[u8], bx: u32, by: u32, c: usize| -> f32 {
        let mut sum = 0.0;
        for y in by * 4..by * 4 + 4 {
            for x in bx * 4..bx * 4 + 4 {
                sum += data[((y * width + x) * 4) as usize + c] as f32;
            }
        }
        sum / 16.0
    };
    let mut worst = 0.0f32;
    let mut total = 0.0f32;
    let mut count = 0;
    for by in 0..height / 4 {
        for bx in 0..width / 4 {
            for c in 0..3 {
                let d = (block(&image, bx, by, c) - block(reference.as_raw(), bx, by, c)).abs();
                worst = worst.max(d);
                total += d;
                count += 1;
            }
        }
    }
    let average = total / count as f32;
    assert!(
        average < 2.0 && worst < 24.0,
        "average difference {average}, worst block {worst}"
    );
}
