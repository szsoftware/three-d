//!
//! The uniffi surface used the way the Kotlin DSL uses it: the pergola demo scene built through it renders the same
//! image as the scene built in Rust, uploads are written through their raw addresses as Kotlin does, models and
//! textures are read from files, and objects that hold a context refuse other threads.
//!

use std::sync::Arc;
use three_d_raytrace_uniffi::*;

fn v(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

/// A box of `size` centered at `at`, as `box(size) of material at at` in the DSL: the unit cube scaled by half.
fn boxed(at: V3, size: V3) -> Vec<f32> {
    #[rustfmt::skip]
    let m = vec![
        size.x / 2.0, 0.0, 0.0, 0.0,
        0.0, size.y / 2.0, 0.0, 0.0,
        0.0, 0.0, size.z / 2.0, 0.0,
        at.x, at.y, at.z, 1.0,
    ];
    m
}

fn diffuse(color: V3, roughness: f32) -> Material {
    Material {
        base_color: color,
        alpha: 1.0,
        metallic: 0.0,
        roughness,
        specular: 0.0,
        emission: v(0.0, 0.0, 0.0),
        transmission: 0.0,
        index_of_refraction: 1.5,
        alpha_cutout: None,
        thin_walled: true,
        medium: None,
        medium_priority: 0,
        base_color_texture: None,
        metallic_roughness_texture: None,
        normal_texture: None,
        emission_texture: None,
    }
}

/// The pergola of three-d-raytrace's scenes/pergola.rs, built through the surface like the DSL's Pergola.kt does.
fn pergola() -> Arc<Scene> {
    let scene = Scene::new();
    scene.set_sky(SkySpec {
        latitude: 52.52,
        longitude: 13.405,
        year: 2026,
        month: 9,
        day: 12,
        hour: 17,
        minute: 0,
        utc_offset_hours: 2.0,
        north: v(0.0, 0.0, -1.0),
        turbidity: 3.0,
        ground_albedo: v(0.2, 0.2, 0.2),
    });
    scene.set_fog(Some(Fog {
        medium: Medium::Fog {
            half_distance: 25.0,
            albedo: 0.9,
            anisotropy: 0.5,
        },
        height: 0.0,
        scale_height: 3.0,
    }));
    let m = |mat: Material| scene.add_material(mat).unwrap();
    let lawn = m(diffuse(v(0.18, 0.26, 0.09), 1.0));
    let gravel = m(diffuse(v(0.5, 0.47, 0.42), 0.9));
    let wood = m(diffuse(v(0.33, 0.22, 0.13), 0.7));
    let stone = m(diffuse(v(0.62, 0.6, 0.56), 0.6));
    let water = m(Material {
        base_color: v(1.0, 1.0, 1.0),
        roughness: 0.0,
        specular: 1.0,
        transmission: 1.0,
        index_of_refraction: 1.33,
        thin_walled: false,
        medium: Some(Medium::Absorbing {
            color: v(0.2, 0.6, 0.5),
            distance: 1.0,
        }),
        ..diffuse(v(1.0, 1.0, 1.0), 0.0)
    });
    let smoke = m(Material {
        base_color: v(1.0, 1.0, 1.0),
        roughness: 0.0,
        transmission: 1.0,
        index_of_refraction: 1.0,
        thin_walled: false,
        medium: Some(Medium::Fog {
            half_distance: 0.25,
            albedo: 0.95,
            anisotropy: 0.3,
        }),
        ..diffuse(v(1.0, 1.0, 1.0), 0.0)
    });
    let square = scene.builtin_mesh(Builtin::Square, 0);
    let cube = scene.builtin_mesh(Builtin::Cube, 0);
    let ball = scene.builtin_mesh(Builtin::Sphere, 48);
    // ground(120.m x 120.m): rot_x(-90°) * scale(60, 60, 1)
    #[rustfmt::skip]
    scene.add_instance(square, lawn, vec![
        60.0, 0.0, 0.0, 0.0,
        0.0, 0.0, -60.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ]).unwrap();
    let place = |mat: u32, at: V3, size: V3| {
        scene.add_instance(cube, mat, boxed(at, size)).unwrap();
    };
    place(gravel, v(0.0, 0.01, 0.0), v(4.4, 0.02, 3.4));
    for x in [-2.0, 2.0] {
        for z in [-1.5, 1.5] {
            place(wood, v(x, 1.25, z), v(0.14, 2.5, 0.14));
        }
        place(wood, v(x, 2.58, 0.0), v(0.14, 0.16, 3.6));
    }
    for i in 0..13 {
        place(
            wood,
            v(-2.1 + i as f32 * 0.35, 2.72, 0.0),
            v(0.08, 0.12, 3.8),
        );
    }
    place(stone, v(0.6, 0.02, 0.2), v(1.8, 0.04, 1.0));
    place(stone, v(0.6, 0.23, -0.34), v(1.8, 0.46, 0.08));
    place(stone, v(0.6, 0.23, 0.74), v(1.8, 0.46, 0.08));
    place(stone, v(-0.34, 0.23, 0.2), v(0.08, 0.46, 1.0));
    place(stone, v(1.54, 0.23, 0.2), v(0.08, 0.46, 1.0));
    place(water, v(0.6, 0.241, 0.2), v(1.79, 0.4, 0.99));
    scene
        .add_instance(ball, smoke, boxed(v(-1.0, 0.9, -0.4), v(1.0, 1.0, 1.0)))
        .unwrap();
    scene
}

fn render_cpu(
    scene: Arc<Scene>,
    camera: CameraSpec,
    look: Look,
    size: (u32, u32),
    samples: u32,
) -> Vec<u8> {
    let renderer = Renderer::cpu_only();
    let job = renderer
        .final_render(scene, camera, size.0, size.1, samples, look, 50)
        .unwrap();
    while !job.step().unwrap() {}
    let rgba = job.rgba().unwrap().expect("complete");
    let bytes = rgba.bytes();
    unsafe { std::slice::from_raw_parts(bytes.address as *const u8, bytes.length as usize) }
        .to_vec()
}

#[test]
fn pergola_through_the_surface_is_the_demo_scene() {
    let demo = demo_scene("pergola".to_owned()).unwrap();
    let ours = pergola();
    let (a, b) = (demo.scene.counts(), ours.counts());
    assert_eq!(a.instances, 28);
    assert_eq!(
        (a.instances, a.materials, a.meshes),
        (b.instances, b.materials, b.meshes)
    );
    let look = Look {
        denoise: false,
        ..demo.look
    };
    let expected = render_cpu(demo.scene.clone(), demo.camera, look, (48, 32), 2);
    let got = render_cpu(ours, demo.camera, look, (48, 32), 2);
    let diff: u64 = expected
        .iter()
        .zip(&got)
        .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64)
        .sum();
    let mean = diff as f64 / expected.len() as f64;
    assert!(mean < 0.5, "mean difference {mean} per channel");
    assert!(
        expected.chunks(4).any(|p| p[0] > 20),
        "the image is not black"
    );
}

/// Writes through the raw address as Kotlin does with asByteBuffer() or a Uint8Array view.
fn write<T: Copy>(bytes: NativeBytes, values: &[T]) {
    assert_eq!(bytes.length as usize, std::mem::size_of_val(values));
    unsafe {
        std::ptr::copy_nonoverlapping(values.as_ptr(), bytes.address as *mut T, values.len())
    };
}

#[test]
fn a_mesh_written_into_an_upload() {
    // One set of uploads for several meshes in a row, as a CAD import does it: resize, write, hand over
    let positions = UploadBytes::new(0);
    let normals = UploadBytes::new(0);
    let indices = UploadBytes::new(0);
    write(
        positions.resize(12 * 4),
        &[-1f32, 0., -1., 1., 0., -1., 1., 0., 1., -1., 0., 1.],
    );
    write(
        normals.resize(12 * 4),
        &[0f32, 1., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0.],
    );
    write(indices.resize(6 * 4), &[0u32, 2, 1, 0, 3, 2]);
    let scene = Scene::new();
    let mesh = scene
        .add_uploaded_mesh(
            positions.clone(),
            indices.clone(),
            Some(normals.clone()),
            None,
        )
        .unwrap();
    assert_eq!(scene.counts().meshes, 1);
    assert_eq!(
        scene.add_uploaded_mesh(positions.clone(), indices.clone(), None, None),
        Err(RtFfiError::Consumed)
    );
    // The memory stays for the next mesh
    let capacity = positions.capacity();
    assert!(capacity >= 48);
    write(
        positions.resize(9 * 4),
        &[0f32, 0., 0., 1., 0., 0., 0., 0., 1.],
    );
    assert_eq!(positions.capacity(), capacity);
    write(indices.resize(3 * 4), &[0u32, 2, 1]);
    scene
        .add_uploaded_mesh(positions.clone(), indices.clone(), None, None)
        .unwrap();
    assert_eq!(scene.counts().meshes, 2);
    let material = scene.add_material(diffuse(v(0.5, 0.5, 0.5), 1.0)).unwrap();
    // Many instances from one upload of 16 floats each
    let transforms = UploadBytes::new(3 * 16 * 4);
    let mut values = Vec::new();
    for i in 0..3 {
        values.extend(boxed(v(i as f32 * 3.0, 0.0, 0.0), v(2.0, 2.0, 2.0)));
    }
    write(transforms.bytes(), &values);
    assert_eq!(scene.add_instances(mesh, material, transforms).unwrap(), 0);
    assert_eq!(scene.counts().instances, 3);
    // Indices out of range are refused
    write(positions.resize(9 * 4), &[0f32; 9]);
    write(indices.resize(3 * 4), &[0u32, 1, 7]);
    assert!(matches!(
        scene.add_uploaded_mesh(positions, indices, None, None),
        Err(RtFfiError::InvalidData { .. })
    ));
}

fn upload(bytes: &[u8]) -> Arc<UploadBytes> {
    let u = UploadBytes::new(bytes.len() as u64);
    write(u.bytes(), bytes);
    u
}

#[test]
fn an_obj_with_its_mtl() {
    // As a CAD or Blender exports it, with texture coordinates and normals
    let obj = b"mtllib box.mtl\no box\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nvn 0 0 1\nusemtl red\ns off\nf 1/1/1 2/2/1 3/3/1\nf 1/1/1 3/3/1 4/4/1\n";
    let mtl = b"newmtl red\nNs 250\nKa 1 1 1\nKd 0.8 0.1 0.1\nKs 0.5 0.5 0.5\nKe 0 0 0\nNi 1.45\nd 1\nillum 2\n";
    let scene = Scene::new();
    let model = scene
        .import_model(
            "box.obj".to_owned(),
            vec![
                ModelFile {
                    name: "box.obj".to_owned(),
                    bytes: upload(obj),
                },
                ModelFile {
                    name: "box.mtl".to_owned(),
                    bytes: upload(mtl),
                },
            ],
        )
        .unwrap();
    assert_eq!(model.parts.len(), 1);
    assert_eq!((model.min, model.max), (v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0)));
    let material = scene.material(model.parts[0].material).unwrap();
    assert!(
        material.base_color.x > 0.5 && material.base_color.y < 0.1,
        "{material:?}"
    );
    // Not placed yet
    assert_eq!(scene.counts().instances, 0);
}

#[test]
fn a_png_texture() {
    let mut png = Vec::new();
    let image = image::RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255]));
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let scene = Scene::new();
    let texture = scene.add_texture(upload(&png), true, true).unwrap();
    let mut m = diffuse(v(1.0, 1.0, 1.0), 1.0);
    m.base_color_texture = Some(texture);
    scene.add_material(m.clone()).unwrap();
    m.base_color_texture = Some(texture + 1);
    assert!(matches!(
        scene.add_material(m),
        Err(RtFfiError::UnknownId { .. })
    ));
    assert!(matches!(
        scene.add_texture(upload(b"no image"), true, true),
        Err(RtFfiError::Import { .. })
    ));
}

#[test]
fn renderer_objects_stay_on_their_thread() {
    let renderer = Renderer::cpu_only();
    assert!(renderer.cpu_reason().is_some());
    let r = renderer.clone();
    let result = std::thread::spawn(move || r.set_cpu_threads(2))
        .join()
        .unwrap();
    assert_eq!(result, Err(RtFfiError::WrongThread));
    assert_eq!(renderer.set_cpu_threads(2), Ok(()));
}

#[test]
fn a_final_render_as_png() {
    let demo = demo_scene("caustics".to_owned()).unwrap();
    let renderer = Renderer::cpu_only();
    let job = renderer
        .final_render(demo.scene, demo.camera, 40, 30, 1, demo.look, 50)
        .unwrap();
    assert!(job.png().unwrap().is_none());
    while !job.step().unwrap() {}
    assert!(job.is_done() && job.progress() >= 1.0);
    // The OwnedBytes must live while the bytes are read
    let owned = job.png().unwrap().unwrap();
    let png = owned.bytes();
    let bytes =
        unsafe { std::slice::from_raw_parts(png.address as *const u8, png.length as usize) };
    let image = image::load_from_memory(bytes).unwrap();
    assert_eq!((image.width(), image.height()), (40, 30));
}

/// Textured materials on meshes without texture coordinates or tangents: the realtime view drops what the mesh cannot
/// carry instead of failing the whole frame (on wgpu a pipeline that misses an attribute left every pixel empty).
#[test]
fn realtime_view_with_textures_on_meshes_without_uvs_or_tangents() {
    if std::env::var_os("THREE_D_SKIP_GPU_TESTS").is_some() {
        return;
    }
    let renderer = Renderer::headless(true);
    if renderer.cpu_reason().is_some() && std::env::var("THREE_D_REQUIRE_WGPU").is_err() {
        eprintln!("no GPU: {:?}", renderer.cpu_reason());
        return;
    }
    let demo = demo_scene("pergola".to_owned()).unwrap();
    let scene = &demo.scene;
    let pixels = UploadBytes::new(4 * 4 * 4);
    write(pixels.resize(4 * 4 * 4), &[0x80ff80ffu32; 16]);
    let texture = scene.add_texture_rgba(4, 4, pixels, true).unwrap();
    let mut material = scene.material(0).unwrap();
    material.base_color_texture = Some(texture);
    material.normal_texture = Some(texture);
    let material = scene.add_material(material).unwrap();
    // A floor of two triangles under the pergola: once with nothing but positions, once with texture coordinates
    let quad = |uvs: bool| {
        let positions = UploadBytes::new(0);
        write(
            positions.resize(12 * 4),
            &[-2f32, 0.05, -2., 2., 0.05, -2., 2., 0.05, 2., -2., 0.05, 2.],
        );
        let indices = UploadBytes::new(0);
        write(indices.resize(6 * 4), &[0u32, 2, 1, 0, 3, 2]);
        let uv = uvs.then(|| {
            let uv = UploadBytes::new(0);
            write(uv.resize(8 * 4), &[0f32, 0., 1., 0., 1., 1., 0., 1.]);
            uv
        });
        scene
            .add_uploaded_mesh(positions, indices, None, uv)
            .unwrap()
    };
    let identity = vec![
        1f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    scene
        .add_instance(quad(false), material, identity.clone())
        .unwrap();
    scene.add_instance(quad(true), material, identity).unwrap();
    let realtime = RealtimeSpec {
        sun_shadow_map: 0,
        raytrace_when_still_ms: None,
        look: demo.look,
    };
    let view = renderer
        .scene_view(demo.scene.clone(), demo.camera, 64, 48, realtime)
        .unwrap();
    let frame = view.next_frame().unwrap();
    let pixels = unsafe {
        std::slice::from_raw_parts(frame.rgba.address as *const u8, frame.rgba.length as usize)
    };
    assert!(
        pixels.chunks(4).all(|p| p[3] == 255),
        "the frame was not drawn"
    );
}

/// The realtime view, the preview and a final render on the GPU; only where a wgpu adapter is (lavapipe in CI).
#[test]
fn realtime_view_preview_and_pick_on_the_gpu() {
    if std::env::var_os("THREE_D_SKIP_GPU_TESTS").is_some() {
        return;
    }
    let renderer = Renderer::headless(true);
    if renderer.cpu_reason().is_some() && std::env::var("THREE_D_REQUIRE_WGPU").is_err() {
        eprintln!("no GPU: {:?}", renderer.cpu_reason());
        return;
    }
    let demo = demo_scene("pergola".to_owned()).unwrap();
    let realtime = RealtimeSpec {
        sun_shadow_map: 512,
        raytrace_when_still_ms: None,
        look: demo.look,
    };
    let view = renderer
        .clone()
        .scene_view(demo.scene.clone(), demo.camera, 64, 48, realtime)
        .unwrap();
    let frame = view.next_frame().unwrap();
    assert_eq!((frame.width, frame.height, frame.samples), (64, 48, 0));
    let pixels = unsafe {
        std::slice::from_raw_parts(frame.rgba.address as *const u8, frame.rgba.length as usize)
    };
    assert!(
        pixels.chunks(4).any(|p| p[0] > 20),
        "the raster frame is not black"
    );
    // The middle of the view looks at the pergola
    let hit = view
        .pick(32.0, 24.0)
        .unwrap()
        .expect("something in the middle");
    assert!(hit.instance < demo.scene.counts().instances);
    view.request_pick(32.0, 24.0).unwrap();
    view.next_frame().unwrap();
    assert_eq!(view.take_pick().unwrap(), Some(Some(hit)));
    // Orbiting moves the camera
    let before = view.camera().unwrap();
    view.input(InputEvent::Press {
        x: 10.0,
        y: 10.0,
        button: 0,
    })
    .unwrap();
    assert!(view
        .input(InputEvent::Move {
            x: 30.0,
            y: 10.0,
            dx: 20.0,
            dy: 0.0,
            button: Some(0)
        })
        .unwrap());
    assert_ne!(view.camera().unwrap().eye, before.eye);
    // Walking with W moves forward at eye height
    view.set_navigation(Navigation::Walk {
        eye_height: 1.6,
        speed: 1.4,
    })
    .unwrap();
    view.input(InputEvent::Key {
        key: "W".to_owned(),
        down: true,
    })
    .unwrap();
    view.next_frame().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    view.next_frame().unwrap();
    let walked = view.camera().unwrap();
    assert!((walked.eye.y - 1.6).abs() < 1e-4);
    view.input(InputEvent::Key {
        key: "W".to_owned(),
        down: false,
    })
    .unwrap();

    // The arrow keys turn the view where the eyes are: left turns to the left
    let key = |key: &str, down: bool| {
        view.input(InputEvent::Key {
            key: key.to_owned(),
            down,
        })
        .unwrap()
    };
    key("ArrowLeft", true);
    view.next_frame().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    view.next_frame().unwrap();
    key("ArrowLeft", false);
    let turned = view.camera().unwrap();
    assert_eq!(turned.eye, walked.eye, "turning does not move the eyes");
    let dir = |c: &CameraSpec| {
        let (x, z) = (c.target.x - c.eye.x, c.target.z - c.eye.z);
        let n = (x * x + z * z).sqrt();
        (x / n, z / n)
    };
    let (before, after) = (dir(&walked), dir(&turned));
    // Seen from above (y up, right-handed), a turn to the left is counterclockwise around +y: cross(before, after).y > 0
    let cross_y = before.1 * after.0 - before.0 * after.1;
    assert!(
        cross_y > 1e-3,
        "turned to the left: {before:?} -> {after:?}"
    );

    // The switch for the raytraced preview costs nothing and takes over when the camera stands still
    view.set_raytrace_when_still(Some(0)).unwrap();
    let mut traced = false;
    for _ in 0..200 {
        if view.next_frame().unwrap().samples > 0 {
            traced = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(traced, "the preview took over");

    // Its status in one call, and a photo that goes on with its samples once it traces at the full resolution
    let mut status = view.preview_status().unwrap().expect("a preview");
    for _ in 0..400 {
        if status.width == status.full_width && status.samples >= 4 {
            break;
        }
        view.next_frame().unwrap();
        status = view.preview_status().unwrap().unwrap();
    }
    assert_eq!((status.full_width, status.full_height), (64, 48));
    assert!(
        status.samples > 0 && status.last_restart.is_some(),
        "{status:?}"
    );
    if status.tracer != TracerKind::Cpu && status.width == status.full_width {
        let photo = view
            .clone()
            .photo(status.samples + 4, 50)
            .unwrap()
            .expect("goes on with the preview");
        assert!(photo.continues_preview());
        assert_eq!(photo.size(), vec![64, 48]);
        let mut steps = 0;
        while !photo.step().unwrap() {
            // The view shows the preview's image meanwhile and does not trace on
            assert!(view.next_frame().unwrap().samples > 0);
            steps += 1;
            assert!(steps < 2000, "the photo does not finish");
        }
        assert!(photo.rgba().unwrap().is_some());
        assert!(photo.elapsed_ms() > 0 || steps == 0);
    } else {
        eprintln!("the preview stayed on the CPU or below the full resolution: {status:?}");
        assert!(view.clone().photo(16, 50).unwrap().is_none());
    }
    view.set_raytrace_when_still(None).unwrap();
    assert_eq!(
        view.next_frame().unwrap().samples,
        0,
        "back to the rasterizer"
    );

    let preview = renderer
        .clone()
        .preview(demo.scene.clone(), demo.camera, 32, 24, demo.look)
        .unwrap();
    let mut frame = None;
    for _ in 0..200 {
        if let Some(f) = preview.next_frame().unwrap() {
            if f.samples > 0 {
                frame = Some(f);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        frame.is_some(),
        "the preview traced: {}",
        preview.availability().unwrap()
    );
    let status = preview.status().unwrap();
    assert!(status.samples > 0 && (status.full_width, status.full_height) == (32, 24));
    if let Some(photo) = preview.clone().photo(status.samples + 2, 50).unwrap() {
        assert!(photo.continues_preview());
        while !photo.step().unwrap() {
            // The preview waits while the photo traces on its engine
            assert!(preview.next_frame().unwrap().is_none());
        }
        assert!(preview.next_frame().is_ok());
    }

    let job = renderer
        .final_render(demo.scene, demo.camera, 32, 24, 2, demo.look, 50)
        .unwrap();
    while !job.step().unwrap() {}
    assert!(job.png().unwrap().is_some());
}

/// Preview options set by hand: the CPU, a fixed resolution on the GPU, and what the device can do.
#[test]
fn preview_options_by_hand() {
    if std::env::var_os("THREE_D_SKIP_GPU_TESTS").is_some() {
        return;
    }
    let renderer = Renderer::headless(true);
    if renderer.cpu_reason().is_some() && std::env::var("THREE_D_REQUIRE_WGPU").is_err() {
        eprintln!("no GPU: {:?}", renderer.cpu_reason());
        return;
    }
    let demo = demo_scene("pergola".to_owned()).unwrap();
    let realtime = RealtimeSpec {
        sun_shadow_map: 512,
        raytrace_when_still_ms: Some(0),
        look: demo.look,
    };
    let view = renderer
        .clone()
        .scene_view(demo.scene.clone(), demo.camera, 64, 48, realtime)
        .unwrap();
    let caps = view.preview_capabilities().unwrap();
    assert!(caps.gpu, "{caps:?}");
    assert_eq!(caps.gpu_reason, None);
    assert_eq!(caps.gpu_downscales, vec![1, 2, 4]);
    // Ray queries or why not (lavapipe has them, the tests do not ask the device for them)
    assert_eq!(caps.ray_query, caps.ray_query_reason.is_none(), "{caps:?}");
    assert_eq!(view.preview_options().unwrap(), PreviewOptions::AUTOMATIC);

    // Downscales the preview cannot have are errors, the options stay
    let wrong = PreviewOptions {
        gpu_downscale: Some(3),
        ..PreviewOptions::AUTOMATIC
    };
    assert!(view.set_preview_options(wrong).is_err());
    assert!(caps.cpu_cores >= 1, "{caps:?}");
    for threads in [0, caps.cpu_cores + 1] {
        let wrong = PreviewOptions {
            cpu_threads: Some(threads),
            ..PreviewOptions::AUTOMATIC
        };
        assert!(
            view.set_preview_options(wrong).is_err(),
            "{threads} threads"
        );
    }
    assert_eq!(view.preview_options().unwrap(), PreviewOptions::AUTOMATIC);

    let traced = |view: &SceneView| {
        for _ in 0..400 {
            if view.next_frame().unwrap().samples > 0 {
                return view.preview_status().unwrap().expect("a preview");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the preview did not trace");
    };

    // The CPU by hand, at half the resolution
    view.set_preview_options(PreviewOptions {
        tracer: TracerChoice::Cpu,
        cpu_downscale: Some(2),
        denoise: false,
        cpu_threads: Some(1),
        ..PreviewOptions::AUTOMATIC
    })
    .unwrap();
    let status = traced(&view);
    assert_eq!(status.tracer, TracerKind::Cpu, "{status:?}");
    assert_eq!((status.width, status.height), (32, 24), "{status:?}");
    assert_eq!(view.preview_options().unwrap().cpu_threads, Some(1));
    // More threads for the running preview: it goes on tracing
    view.set_preview_options(PreviewOptions {
        tracer: TracerChoice::Cpu,
        cpu_downscale: Some(2),
        denoise: false,
        cpu_threads: Some(caps.cpu_cores),
        ..PreviewOptions::AUTOMATIC
    })
    .unwrap();
    let samples = status.samples;
    let mut more = false;
    for _ in 0..400 {
        view.next_frame().unwrap();
        if view.preview_status().unwrap().unwrap().samples > samples {
            more = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(more, "the CPU traced on with more threads");

    // The GPU by hand at a quarter of the resolution, fixed: after the shaders compiled it stays there
    view.set_preview_options(PreviewOptions {
        tracer: TracerChoice::Gpu,
        gpu_downscale: Some(4),
        cpu_downscale: None,
        denoise: true,
        cpu_threads: None,
        ray_queries: true,
    })
    .unwrap();
    let mut status = traced(&view);
    for _ in 0..2000 {
        if !status.compiling && status.tracer != TracerKind::Cpu {
            break;
        }
        view.next_frame().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        status = view.preview_status().unwrap().unwrap();
    }
    assert_ne!(status.tracer, TracerKind::Cpu, "{status:?}");
    for _ in 0..20 {
        view.next_frame().unwrap();
    }
    let status = view.preview_status().unwrap().unwrap();
    assert_ne!(status.tracer, TracerKind::Cpu, "{status:?}");
    assert_eq!((status.width, status.height), (16, 12), "{status:?}");

    // A new downscale changes the running preview
    view.set_preview_options(PreviewOptions {
        tracer: TracerChoice::Gpu,
        gpu_downscale: Some(1),
        cpu_downscale: None,
        denoise: true,
        cpu_threads: None,
        ray_queries: true,
    })
    .unwrap();
    let status = traced(&view);
    assert_ne!(status.tracer, TracerKind::Cpu, "{status:?}");
    assert_eq!((status.width, status.height), (64, 48), "{status:?}");
}

/// egui over the realtime view and over the raytraced preview: it lies over both pictures, clicks on it are egui's and
/// leave the camera alone, drags beside it turn the camera.
#[test]
fn realtime_view_with_egui_over_it() {
    use three_d_egui_uniffi::*;
    if std::env::var_os("THREE_D_SKIP_GPU_TESTS").is_some() {
        return;
    }
    let renderer = Renderer::headless(true);
    if renderer.cpu_reason().is_some() && std::env::var("THREE_D_REQUIRE_WGPU").is_err() {
        eprintln!("no GPU: {:?}", renderer.cpu_reason());
        return;
    }
    let (w, h) = (240u32, 160u32);
    let demo = demo_scene("pergola".to_owned()).unwrap();
    let realtime = RealtimeSpec {
        sun_shadow_map: 512,
        raytrace_when_still_ms: None,
        look: demo.look,
    };
    let view = renderer
        .clone()
        .scene_view(demo.scene.clone(), demo.camera, w, h, realtime)
        .unwrap();
    let overlay = EguiOverlay::new();
    let node = |id, parent, widget| UiNode {
        id,
        parent,
        widget,
        enabled: true,
        tooltip: None,
        modifier: None,
    };
    overlay
        .set_tree(vec![
            node(
                1,
                None,
                Widget::SidePanel {
                    side: Side::Left,
                    title: None,
                    width: 80.0,
                    resizable: false,
                    min_width: None,
                    max_width: None,
                    expanded: true,
                },
            ),
            node(
                2,
                Some(1),
                Widget::Checkbox {
                    text: "Sun".into(),
                    checked: false,
                },
            ),
        ])
        .unwrap();
    view.set_overlay(Some(overlay.clone())).unwrap();
    let gray = |frame: &Frame, x: u32, y: u32| {
        let pixels = unsafe {
            std::slice::from_raw_parts(frame.rgba.address as *const u8, frame.rgba.length as usize)
        };
        let p = &pixels[((y * w + x) * 4) as usize..][..3];
        p.iter().all(|&c| c.abs_diff(27) <= 3)
    };
    view.next_frame().unwrap();
    let frame = view.next_frame().unwrap();
    assert!(gray(&frame, 70, 150), "egui's panel over the raster frame");
    assert!(!gray(&frame, 200, 80), "the scene beside the panel");

    // A drag that starts on the panel is egui's to its end: the camera stays
    let camera = view.camera().unwrap();
    let b = overlay.bounds(2).unwrap();
    let (x, y) = (b.x + b.width / 2.0, b.y + b.height / 2.0);
    let moved = |x, dx, button| InputEvent::Move {
        x,
        y,
        dx,
        dy: 0.0,
        button,
    };
    assert!(view.input(InputEvent::Press { x, y, button: 0 }).unwrap());
    assert!(view.input(moved(x + 120.0, 120.0, Some(0))).unwrap());
    assert!(view
        .input(InputEvent::Release {
            x: x + 120.0,
            y,
            button: 0
        })
        .unwrap());
    view.next_frame().unwrap();
    // A click on the checkbox ticks it
    assert!(view.input(moved(x, 0.0, None)).unwrap());
    view.next_frame().unwrap();
    assert!(view.input(InputEvent::Press { x, y, button: 0 }).unwrap());
    view.next_frame().unwrap();
    assert!(view.input(InputEvent::Release { x, y, button: 0 }).unwrap());
    view.next_frame().unwrap();
    assert_eq!(view.camera().unwrap(), camera);
    assert_eq!(
        overlay.take_events(),
        vec![UiEvent::Checked {
            id: 2,
            checked: true
        }]
    );
    // A drag beside the panel turns the camera, also when it passes over the panel
    view.input(InputEvent::Press {
        x: 200.0,
        y: 80.0,
        button: 0,
    })
    .unwrap();
    assert!(view
        .input(InputEvent::Move {
            x: 60.0,
            y: 80.0,
            dx: -140.0,
            dy: 0.0,
            button: Some(0)
        })
        .unwrap());
    view.input(InputEvent::Release {
        x: 60.0,
        y: 80.0,
        button: 0,
    })
    .unwrap();
    assert_ne!(view.camera().unwrap(), camera);

    // The raytraced preview has the panel too
    view.set_raytrace_when_still(Some(0)).unwrap();
    for _ in 0..400 {
        let frame = view.next_frame().unwrap();
        if frame.samples > 0 {
            assert!(gray(&frame, 70, 150), "egui's panel over the preview");
            assert!(!gray(&frame, 200, 80), "the preview beside the panel");
            view.set_overlay(None).unwrap();
            let frame = view.next_frame().unwrap();
            assert!(!gray(&frame, 70, 150), "without the overlay");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the preview did not trace");
}
