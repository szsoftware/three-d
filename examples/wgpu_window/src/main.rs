use three_d::*;

pub fn settings() -> WindowSettings {
    WindowSettings {
        title: "wgpu window!".to_string(),
        max_size: Some((1280, 720)),
        ..Default::default()
    }
}

// Not used on the web, see `lib.rs`
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn main() {
    // Create a window which renders with wgpu (Vulkan, Metal or DirectX 12) if the system supports it and with OpenGL otherwise.
    // In a browser, `Window::new_async` is needed to get WebGPU, see `lib.rs`.
    // The backend can also be chosen with an argument: `cargo run --features wgpu --example wgpu_window -- wgpu` (or `gl`),
    // then it is an error if the system does not support that backend.
    let preference = match std::env::args().nth(1).as_deref() {
        Some("wgpu") => BackendPreference::Wgpu,
        Some("gl") => BackendPreference::Gl,
        _ => BackendPreference::Auto,
    };
    let window = Window::new_with_backend(settings(), preference).unwrap();
    run(window);
}

pub fn run(window: Window) {
    // Get the graphics context from the window, it tells which backend is in use
    let context = window.gl();
    let backend = if context.as_gl().is_some() {
        "OpenGL/WebGL"
    } else {
        "wgpu"
    };
    println!("rendering with {backend}");

    let mut camera = Camera::new_perspective(
        window.viewport(),
        vec3(2.0, 2.0, 4.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        100.0,
    );
    let mut control = OrbitControl::new(camera.target(), 1.0, 100.0);

    // Everything else is the same for both backends
    let mut cube = Gm::new(
        Mesh::new(&context, &CpuMesh::cube()),
        PhysicalMaterial::new_opaque(
            &context,
            &CpuMaterial {
                albedo: Srgba::new(60, 140, 220, 255),
                roughness: 0.6,
                ..Default::default()
            },
        ),
    );
    cube.set_animation(|time| Mat4::from_angle_y(radians(time * 0.001)));
    let mut sphere = Gm::new(
        Mesh::new(&context, &CpuMesh::sphere(32)),
        PhysicalMaterial::new_opaque(
            &context,
            &CpuMaterial {
                albedo: Srgba::new(230, 120, 40, 255),
                ..Default::default()
            },
        ),
    );
    sphere.set_transformation(Mat4::from_translation(vec3(0.0, 1.6, 0.0)) * Mat4::from_scale(0.6));
    let axes = Axes::new(&context, 0.05, 2.5);

    // A click on an object puts a marker on it. That needs reading pixels back from the GPU
    let mut marker = Gm::new(
        Mesh::new(&context, &CpuMesh::sphere(16)),
        ColorMaterial {
            color: Srgba::new(255, 255, 0, 255),
            ..Default::default()
        },
    );
    let picked = std::rc::Rc::new(std::cell::RefCell::new(None::<Vec3>));

    let ambient = AmbientLight::new(&context, 0.3, Srgba::WHITE);
    let mut directional =
        DirectionalLight::new(&context, 3.0, Srgba::WHITE, vec3(-1.0, -2.0, -1.5));

    window.render_loop(move |mut frame_input| {
        camera.set_viewport(frame_input.viewport);
        for event in frame_input.events.iter() {
            if let Event::MousePress {
                button: MouseButton::Left,
                position,
                ..
            } = event
            {
                let geometries = cube.into_iter().chain(&sphere);
                // WebGPU cannot wait for the GPU, so a browser needs the asynchronous variant.
                // It works with every backend on every platform, the blocking variant is just simpler to use.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let hit = pick(&context, &camera, *position, geometries, Cull::Back).unwrap();
                    *picked.borrow_mut() = hit.map(|hit| hit.position);
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let hit = pick_async(&context, &camera, *position, geometries, Cull::Back);
                    let picked = picked.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let hit = hit.await.unwrap();
                        log::info!("picked {:?}", hit.map(|hit| hit.position));
                        *picked.borrow_mut() = hit.map(|hit| hit.position);
                    });
                }
            }
        }
        control.handle_events(&mut camera, &mut frame_input.events);
        cube.animate(frame_input.accumulated_time as f32);
        directional
            .generate_shadow_map(512, [&cube, &sphere])
            .unwrap();

        frame_input
            .screen()
            .clear(ClearState::color_and_depth(0.8, 0.8, 0.8, 1.0, 1.0))
            .render(
                &camera,
                cube.into_iter().chain(&sphere).chain(&axes),
                &[&ambient, &directional],
            );
        if let Some(position) = *picked.borrow() {
            marker.set_transformation(Mat4::from_translation(position) * Mat4::from_scale(0.08));
            frame_input.screen().render(&camera, &marker, &[]);
        }

        FrameOutput::default()
    });
}
