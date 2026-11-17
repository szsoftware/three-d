//! Tests of the wgpu backend on a real adapter (a software renderer like lavapipe is enough).

use crate::core::*;

///
/// A context on whatever adapter wgpu finds, a software renderer like lavapipe is enough.
/// Without any adapter the test is skipped, unless the environment variable `THREE_D_REQUIRE_WGPU` is set (as in CI).
///
fn context() -> Option<Context> {
    match Context::new_headless_wgpu() {
        Ok(context) => Some(context),
        Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
            panic!("no wgpu adapter: {error}")
        }
        Err(error) => {
            eprintln!("skipped, no wgpu adapter: {error}");
            None
        }
    }
}

fn target_texture(context: &Context, width: u32, height: u32) -> Texture2D {
    Texture2D::new_empty::<[u8; 4]>(
        context,
        width,
        height,
        Interpolation::Nearest,
        Interpolation::Nearest,
        None,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    )
}

const VERTEX: &str = "
    uniform mat4 transformation;
    in vec3 position;
    in vec4 color;
    out vec4 col;
    void main() { col = color; gl_Position = transformation * vec4(position, 1.0); }
";
const FRAGMENT: &str = "
    uniform vec3 tint;
    in vec4 col;
    layout (location = 0) out vec4 outColor;
    void main() { outColor = col * vec4(tint, 1.0); }
";

#[test]
fn context_is_wgpu() {
    let Some(context) = context() else { return };
    assert!(context.as_wgpu().is_some());
    assert!(context.as_gl().is_none());
}

///
/// The triangle of the `triangle_core` example: the result must have the orientation it has on OpenGL.
///
#[test]
fn triangle() {
    let Some(context) = context() else { return };
    let positions = VertexBuffer::new_with_data(
        &context,
        &[
            vec3(-0.5, -0.5, 0.0), // bottom left
            vec3(0.5, -0.5, 0.0),  // bottom right
            vec3(0.0, 0.5, 0.0),   // top
        ],
    );
    let colors = VertexBuffer::new_with_data(
        &context,
        &[
            vec4(0.0, 1.0, 0.0, 1.0f32),
            vec4(1.0, 0.0, 0.0, 1.0),
            vec4(0.0, 0.0, 1.0, 1.0),
        ],
    );
    let program = Program::from_source(&context, VERTEX, FRAGMENT).unwrap();
    assert!(program.requires_uniform("tint"));
    assert!(!program.requires_uniform("unknown"));

    let draw = |cull: Cull| -> Vec<[u8; 4]> {
        let texture = target_texture(&context, 64, 64);
        let target = texture.as_color_target(None);
        target
            .clear(ClearState::color(0.0, 0.0, 0.0, 1.0))
            .write::<CoreError>(|| {
                program.use_uniform("transformation", Mat4::identity());
                program.use_uniform("tint", vec3(1.0, 1.0, 0.5));
                program.use_vertex_attribute("position", &positions);
                program.use_vertex_attribute("color", &colors);
                program.draw_arrays(
                    RenderStates {
                        cull,
                        ..Default::default()
                    },
                    Viewport::new_at_origo(64, 64),
                    3,
                );
                Ok(())
            })
            .unwrap();
        target.read()
    };

    // read() returns the top row first
    let pixels = draw(Cull::None);
    let pixel = |x: usize, y_from_top: usize| pixels[y_from_top * 64 + x];
    assert_eq!(pixel(1, 1), [0, 0, 0, 255], "background");
    assert_eq!(pixel(32, 60), [0, 0, 0, 255], "below the triangle");
    let top = pixel(32, 18);
    assert!(
        top[2] > 100 && top[0] < 60 && top[1] < 60,
        "top is blue (times tint): {top:?}"
    );
    let right = pixel(46, 47);
    assert!(
        right[0] > 200 && right[1] < 60,
        "bottom right is red: {right:?}"
    );
    let left = pixel(17, 47);
    assert!(
        left[1] > 200 && left[0] < 60,
        "bottom left is green: {left:?}"
    );

    // The triangle is counter clockwise seen from the front, as on OpenGL
    assert!(draw(Cull::Back) == pixels);
    assert!(draw(Cull::Front).iter().all(|p| *p == [0, 0, 0, 255]));
}

///
/// Depth range, depth test and an element buffer: a near quad in front of a far quad, drawn first.
///
#[test]
fn depth_test_and_elements() {
    let Some(context) = context() else { return };
    let color = target_texture(&context, 16, 16);
    let depth = DepthTexture2D::new::<f32>(
        &context,
        16,
        16,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    );
    let quad = |z: f32, size: f32| {
        VertexBuffer::new_with_data(
            &context,
            &[
                vec3(-size, -size, z),
                vec3(size, -size, z),
                vec3(size, size, z),
                vec3(-size, size, z),
            ],
        )
    };
    let elements = ElementBuffer::new_with_data(&context, &[0u16, 1, 2, 2, 3, 0]);
    // 8 bit indices do not exist in wgpu and are widened by the backend
    let elements_u8 = ElementBuffer::new_with_data(&context, &[0u8, 1, 2, 2, 3, 0]);
    let (near, far) = (quad(-0.5, 0.5), quad(0.9, 1.0));
    let white = VertexBuffer::new_with_data(&context, &[vec4(1.0, 1.0, 1.0, 1.0f32); 4]);
    let program = Program::from_source(&context, VERTEX, FRAGMENT).unwrap();
    let pixels: Vec<[u8; 4]> =
        RenderTarget::new(color.as_color_target(None), depth.as_depth_target())
            .clear(ClearState::color_and_depth(0.0, 0.0, 0.0, 1.0, 1.0))
            .write::<CoreError>(|| {
                program.use_uniform("transformation", Mat4::identity());
                program.use_vertex_attribute("color", &white);
                // Near (z = -0.5 in normalized device coordinates of OpenGL) and small, red
                program.use_uniform("tint", vec3(1.0, 0.0, 0.0));
                program.use_vertex_attribute("position", &near);
                program.draw_elements(
                    RenderStates::default(),
                    Viewport::new_at_origo(16, 16),
                    &elements,
                );
                // Far and large, green
                program.use_uniform("tint", vec3(0.0, 1.0, 0.0));
                program.use_vertex_attribute("position", &far);
                program.draw_elements(
                    RenderStates::default(),
                    Viewport::new_at_origo(16, 16),
                    &elements_u8,
                );
                Ok(())
            })
            .unwrap()
            .read_color();
    assert_eq!(pixels[8 * 16 + 8], [255, 0, 0, 255], "the near quad wins");
    assert_eq!(
        pixels[0],
        [0, 255, 0, 255],
        "the far quad is visible around it"
    );
}

///
/// A texture is sampled with the same orientation as on OpenGL: the first row of the data is the top row and has v = 1.
///
#[test]
fn texture_sampling() {
    let Some(context) = context() else { return };
    let source = Texture2D::new(
        &context,
        &CpuTexture {
            data: TextureData::RgbU8(vec![[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]]),
            width: 2,
            height: 2,
            min_filter: Interpolation::Nearest,
            mag_filter: Interpolation::Nearest,
            mipmap: None,
            wrap_s: Wrapping::ClampToEdge,
            wrap_t: Wrapping::ClampToEdge,
            ..Default::default()
        },
    );
    let program = Program::from_source(
        &context,
        full_screen_vertex_shader_source(),
        "
            uniform sampler2D tex;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = texture(tex, uvs); }
        ",
    )
    .unwrap();
    let texture = target_texture(&context, 2, 2);
    let pixels: Vec<[u8; 4]> = texture
        .as_color_target(None)
        .write::<CoreError>(|| {
            program.use_texture("tex", &source);
            full_screen_draw(
                &context,
                &program,
                RenderStates::default(),
                Viewport::new_at_origo(2, 2),
            );
            Ok(())
        })
        .unwrap()
        .read();
    assert_eq!(
        pixels,
        [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255]
        ]
    );
}

///
/// The renderer module on top: a lit sphere with a physical material.
///
#[test]
fn renderer_draws_lit_mesh() {
    use crate::renderer::*;
    let Some(context) = context() else { return };
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(32, 32),
        vec3(0.0, 0.0, 4.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        10.0,
    );
    let sphere = Gm::new(
        Mesh::new(&context, &CpuMesh::sphere(16)),
        PhysicalMaterial::new_opaque(
            &context,
            &CpuMaterial {
                albedo: Srgba::RED,
                ..Default::default()
            },
        ),
    );
    // The light shines from the top
    let light = DirectionalLight::new(&context, 2.0, Srgba::WHITE, vec3(0.0, -1.0, 0.0));
    let color = target_texture(&context, 32, 32);
    let depth = DepthTexture2D::new::<f32>(
        &context,
        32,
        32,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    );
    let pixels: Vec<[u8; 4]> =
        RenderTarget::new(color.as_color_target(None), depth.as_depth_target())
            .clear(ClearState::color_and_depth(0.0, 0.0, 0.0, 1.0, 1.0))
            .render(&camera, &sphere, &[&light])
            .read_color();
    let pixel = |x: usize, y_from_top: usize| pixels[y_from_top * 32 + x];
    assert_eq!(pixel(0, 0), [0, 0, 0, 255], "background");
    let (top, bottom) = (pixel(16, 8), pixel(16, 24));
    assert!(
        top[0] > 100 && top[1] < 30 && top[2] < 30,
        "the top is lit and red: {top:?}"
    );
    assert!(
        bottom[0] < top[0] / 2,
        "the bottom is in the dark: {bottom:?}"
    );
}

///
/// Draw calls and clears on the same target are one render pass, and everything is submitted once.
///
#[test]
fn draw_calls_are_batched() {
    let Some(context) = context() else { return };
    let statistics = || context.wgpu_backend().lock().statistics;
    let positions = VertexBuffer::new_with_data(
        &context,
        &[
            vec3(-0.5, -0.5, 0.0),
            vec3(0.5, -0.5, 0.0),
            vec3(0.0, 0.5, 0.0),
        ],
    );
    let colors = VertexBuffer::new_with_data(&context, &[vec4(1.0, 1.0, 1.0, 1.0f32); 3]);
    let program = Program::from_source(&context, VERTEX, FRAGMENT).unwrap();
    let texture = target_texture(&context, 16, 16);
    let other = target_texture(&context, 16, 16);
    let draw = |count: u32| {
        for i in 0..count {
            program.use_uniform("transformation", Mat4::identity());
            program.use_uniform("tint", vec3(i as f32 / count as f32, 0.0, 0.0));
            program.use_vertex_attribute("position", &positions);
            program.use_vertex_attribute("color", &colors);
            program.draw_arrays(RenderStates::default(), Viewport::new_at_origo(16, 16), 3);
        }
        Ok::<(), CoreError>(())
    };
    let before = statistics();
    texture
        .as_color_target(None)
        .clear(ClearState::color(0.0, 0.0, 0.0, 1.0))
        .write(|| draw(100))
        .unwrap();
    assert_eq!(statistics(), (before.0 + 1, before.1 + 1));

    // Another target in between ends the pass, but not the recording
    texture
        .as_color_target(None)
        .write(|| {
            draw(10)?;
            other.as_color_target(None).write(|| draw(10))?;
            draw(10)
        })
        .unwrap();
    assert_eq!(statistics(), (before.0 + 4, before.1 + 2));
    context.error_check().unwrap();
}

///
/// A validation error is reported by the error check instead of a panic.
///
#[test]
fn errors_are_collected() {
    let Some(context) = context() else { return };
    context.error_check().unwrap();
    // A buffer for a uniform block that is too small for the block
    let program = Program::from_source(
        &context,
        full_screen_vertex_shader_source(),
        "
            uniform Block { mat4 matrix; };
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = matrix * vec4(uvs, 0.0, 1.0); }
        ",
    )
    .unwrap();
    let mut buffer = UniformBuffer::new(&context, &[4]);
    buffer.update(0, &[1.0, 2.0, 3.0, 4.0]);
    let texture = target_texture(&context, 2, 2);
    // The draw call checks for errors itself in debug builds
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        texture
            .as_color_target(None)
            .write::<CoreError>(|| {
                program.use_uniform_block("Block", &buffer);
                full_screen_draw(
                    &context,
                    &program,
                    RenderStates::default(),
                    Viewport::new_at_origo(2, 2),
                );
                Ok(())
            })
            .unwrap();
    }));
    let error = match result {
        Ok(()) => context.error_check().unwrap_err().to_string(),
        Err(panic) => panic.downcast_ref::<String>().cloned().unwrap_or_default(),
    };
    assert!(
        error.contains("Validation Error") || error.contains("alidation"),
        "{error}"
    );
    let _ = context.error_check();
    context.error_check().unwrap();
}
