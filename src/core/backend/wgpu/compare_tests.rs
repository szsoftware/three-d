//!
//! Runs the same code on the OpenGL backend and on the wgpu backend and compares what is read back.
//! Both need no window: OpenGL through EGL, wgpu on whatever adapter it finds (software renderers are enough).
//! Without EGL or a wgpu adapter the tests are skipped, unless the environment variable `THREE_D_REQUIRE_WGPU` is set (as in CI).
//!

use crate::core::*;
use glutin::api::egl::{device::Device, display::Display};
use glutin::config::{ConfigSurfaceTypes, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::prelude::*;

/// An OpenGL context without a window and what keeps it alive.
fn gl_context() -> Result<(Context, impl Sized), String> {
    let error = |e: glutin::error::Error| e.to_string();
    let devices: Vec<Device> = Device::query_devices()
        .map_err(|e| e.to_string())?
        .collect();
    let device = devices.first().ok_or("no EGL device")?;
    let display = unsafe { Display::with_device(device, None) }.map_err(error)?;
    let template = ConfigTemplateBuilder::new()
        .with_surface_type(ConfigSurfaceTypes::empty())
        .build();
    let config = unsafe { display.find_configs(template) }
        .map_err(error)?
        .next()
        .ok_or("no EGL config")?;
    let attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
        .build(None);
    let current = unsafe { display.create_context(&config, &attributes) }
        .map_err(error)?
        .make_current_surfaceless()
        .map_err(error)?;
    let gl = unsafe {
        crate::context::Context::from_loader_function(|name| {
            display.get_proc_address(&std::ffi::CString::new(name).unwrap()) as *const _
        })
    };
    let context = Context::from_gl_context(std::sync::Arc::new(gl)).map_err(|e| e.to_string())?;
    Ok((context, (current, display)))
}

///
/// Runs the scenario on both backends and checks that the values it returns differ by at most the tolerance.
///
pub(super) fn compare(tolerance: f32, scenario: impl Fn(&Context) -> Vec<f32>) {
    compare_with_outliers(tolerance, 0.0, scenario)
}

///
/// Like [compare], but the given part of the values may differ by more than the tolerance.
/// For images where a small numerical difference decides between two colors, like the edges an effect looks for.
///
pub(super) fn compare_with_outliers(
    tolerance: f32,
    outliers: f32,
    scenario: impl Fn(&Context) -> Vec<f32>,
) {
    let required = std::env::var("THREE_D_REQUIRE_WGPU").is_ok();
    let skip = |what: &str, error: String| {
        if required {
            panic!("no {what}: {error}");
        }
        eprintln!("skipped, no {what}: {error}");
    };
    // All tests share one EGL display, which is initialized and terminated with the contexts
    super::one_gpu_test_at_a_time();
    let expected = {
        match gl_context() {
            Ok((context, _keep_alive)) => scenario(&context),
            Err(error) => return skip("OpenGL context", error),
        }
    };
    let actual = match Context::new_headless_wgpu() {
        Ok(context) => {
            let actual = scenario(&context);
            context.error_check().unwrap();
            actual
        }
        Err(error) => return skip("wgpu adapter", error.to_string()),
    };
    assert_eq!(expected.len(), actual.len(), "number of values");
    assert!(
        expected.iter().any(|v| *v != expected[0]),
        "the scenario returns the same value everywhere, it compares nothing"
    );
    let worst = expected
        .iter()
        .zip(&actual)
        .enumerate()
        .map(|(index, (e, a))| (index, (e - a).abs(), *e, *a))
        .fold((0, 0.0, 0.0, 0.0), |worst, value| {
            if value.1 > worst.1 {
                value
            } else {
                worst
            }
        });
    let beyond = expected
        .iter()
        .zip(&actual)
        .filter(|(e, a)| (*e - *a).abs() > tolerance)
        .count();
    assert!(
        beyond as f32 <= outliers * expected.len() as f32,
        "{beyond} of {} values differ by more than {tolerance}, the worst is value {}: {} on OpenGL and {} on wgpu",
        expected.len(),
        worst.0,
        worst.2,
        worst.3
    );
}

/// One step of a `u8` color channel, plus rounding.
pub(super) const STEP: f32 = 1.01 / 255.0;

pub(super) fn floats(pixels: Vec<[u8; 4]>) -> Vec<f32> {
    pixels
        .iter()
        .flatten()
        .map(|value| *value as f32 / 255.0)
        .collect()
}

pub(super) fn color_texture(context: &Context, width: u32, height: u32) -> Texture2D {
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

pub(super) fn depth_texture<T: DepthTextureDataType>(
    context: &Context,
    width: u32,
    height: u32,
) -> DepthTexture2D {
    DepthTexture2D::new::<T>(
        context,
        width,
        height,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    )
}

/// A program that draws a triangle over the whole target, the fragment shader gets `uvs`.
fn screen_program(context: &Context, fragment: &str) -> Program {
    Program::from_source(context, full_screen_vertex_shader_source(), fragment).unwrap()
}

/// Draws with a [screen_program] into a new texture of the given size and returns its pixels.
fn draw_screen(context: &Context, size: u32, program: &Program, bind: impl FnOnce()) -> Vec<f32> {
    let texture = color_texture(context, size, size);
    floats(
        texture
            .as_color_target(None)
            .clear(ClearState::color(0.0, 0.0, 0.0, 0.0))
            .write::<CoreError>(|| {
                bind();
                full_screen_draw(
                    context,
                    program,
                    RenderStates::default(),
                    Viewport::new_at_origo(size, size),
                );
                Ok(())
            })
            .unwrap()
            .read(),
    )
}

/// Colors that are different in every texel and far apart for neighbours.
pub(super) fn pattern(width: u32, height: u32, seed: u32) -> Vec<[u8; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            [
                ((x * 53 + seed * 31) % 256) as u8,
                ((y * 91 + seed * 17) % 256) as u8,
                (((x + y) * 37 + seed * 7) % 256) as u8,
                255,
            ]
        })
        .collect()
}

fn cpu_texture(size: u32, seed: u32, mipmap: Option<Mipmap>) -> CpuTexture {
    CpuTexture {
        data: TextureData::RgbaU8(pattern(size, size, seed)),
        width: size,
        height: size,
        min_filter: Interpolation::Linear,
        mag_filter: Interpolation::Linear,
        mipmap,
        wrap_s: Wrapping::Repeat,
        wrap_t: Wrapping::MirroredRepeat,
        ..Default::default()
    }
}

const POSITION_VERTEX: &str = "
    uniform vec4 placement;
    in vec3 position;
    void main() { gl_Position = vec4(position.xy * placement.zw + placement.xy, position.z, 1.0); }
";

const COLOR_FRAGMENT: &str = "
    uniform vec4 color;
    layout (location = 0) out vec4 outColor;
    void main() { outColor = color; }
";

/// A quad from -1 to 1 at the given depth (in normalized device coordinates).
fn quad(context: &Context, z: f32) -> VertexBuffer<Vec3> {
    VertexBuffer::new_with_data(
        context,
        &[
            vec3(-1.0, -1.0, z),
            vec3(1.0, -1.0, z),
            vec3(1.0, 1.0, z),
            vec3(1.0, 1.0, z),
            vec3(-1.0, 1.0, z),
            vec3(-1.0, -1.0, z),
        ],
    )
}

#[test]
fn mipmaps_and_filtering() {
    compare(2.0 * STEP, |context| {
        let texture = Texture2D::new(context, &cpu_texture(16, 1, Some(Mipmap::default())));
        assert_eq!(texture.number_of_mip_maps(), 4);
        let program = screen_program(
            context,
            "
            uniform sampler2D tex;
            uniform float lod;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = textureLod(tex, uvs * 1.5 - 0.25, lod); }
        ",
        );
        // Whole levels, a blend of two levels, magnification with wrapping and what the derivatives select
        let mut values = Vec::new();
        for lod in [0.0, 1.0, 2.0, 2.5, 3.0] {
            values.extend(draw_screen(context, 8, &program, || {
                program.use_texture("tex", &texture);
                program.use_uniform("lod", lod);
            }));
        }
        let minified = screen_program(
            context,
            "
            uniform sampler2D tex;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = texture(tex, uvs); }
        ",
        );
        values.extend(draw_screen(context, 4, &minified, || {
            minified.use_texture("tex", &texture)
        }));
        values
    });
}

#[test]
fn texture_without_mipmaps_and_formats() {
    compare(STEP, |context| {
        let program = screen_program(
            context,
            "
            uniform sampler2D tex;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = texture(tex, uvs); }
        ",
        );
        let mut values = Vec::new();
        let mut draw = |data: TextureData| {
            let texture = Texture2D::new(
                context,
                &CpuTexture {
                    data,
                    width: 2,
                    height: 2,
                    min_filter: Interpolation::Nearest,
                    mag_filter: Interpolation::Nearest,
                    mipmap: None,
                    ..Default::default()
                },
            );
            values.extend(draw_screen(context, 4, &program, || {
                program.use_texture("tex", &texture)
            }));
        };
        draw(TextureData::RU8(vec![0, 80, 160, 240]));
        draw(TextureData::RgU8(vec![
            [0, 250],
            [80, 170],
            [160, 90],
            [240, 10],
        ]));
        draw(TextureData::RgbU8(vec![
            [10, 20, 30],
            [40, 50, 60],
            [70, 80, 90],
            [100, 110, 120],
        ]));
        draw(TextureData::RgbaF16(
            [0.1, 0.4, 0.7, 1.0]
                .map(|v| [v, 1.0 - v, 0.5 * v, 0.5].map(f16::from_f32))
                .to_vec(),
        ));
        draw(TextureData::RgbaF32(
            [0.2, 0.5, 0.8, 0.9]
                .map(|v| [v, 1.0 - v, 0.5 * v, 0.25])
                .to_vec(),
        ));
        draw(TextureData::RF32(vec![0.2, 0.5, 0.8, 0.9]));
        values
    });
}

#[test]
fn cube_map() {
    compare(2.0 * STEP, |context| {
        let sides: Vec<CpuTexture> = (0..6)
            .map(|side| cpu_texture(8, side + 2, Some(Mipmap::default())))
            .collect();
        let texture = TextureCubeMap::new(
            context, &sides[0], &sides[1], &sides[2], &sides[3], &sides[4], &sides[5],
        );
        let program = screen_program(
            context,
            "
            uniform samplerCube tex;
            uniform vec3 direction;
            uniform vec3 right;
            uniform vec3 up;
            uniform float lod;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() {
                vec2 p = 1.6 * uvs - 0.8;
                outColor = textureLod(tex, direction + p.x * right + p.y * up, lod);
            }
        ",
        );
        let mut values = Vec::new();
        for (direction, right, up) in [
            (
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 0.0, 1.0),
                vec3(0.0, 1.0, 0.0),
            ),
            (
                vec3(-1.0, 0.0, 0.0),
                vec3(0.0, 0.0, 1.0),
                vec3(0.0, 1.0, 0.0),
            ),
            (
                vec3(0.0, 1.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 0.0, 1.0),
            ),
            (
                vec3(0.0, -1.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 0.0, 1.0),
            ),
            (
                vec3(0.0, 0.0, 1.0),
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 1.0, 0.0),
            ),
            (
                vec3(0.0, 0.0, -1.0),
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 1.0, 0.0),
            ),
        ] {
            for lod in [0.0, 1.0] {
                values.extend(draw_screen(context, 8, &program, || {
                    program.use_texture_cube("tex", &texture);
                    program.use_uniform("direction", direction);
                    program.use_uniform("right", right);
                    program.use_uniform("up", up);
                    program.use_uniform("lod", lod);
                }));
            }
        }
        values
    });
}

#[test]
fn texture_array_and_several_color_targets() {
    compare(STEP, |context| {
        let layers = [cpu_texture(4, 20, None), cpu_texture(4, 21, None)];
        let source = Texture2DArray::new(context, &[&layers[0], &layers[1]]);
        // Both layers of the source are copied to the layers of the target in one draw call, swapped
        let target = Texture2DArray::new_empty::<[u8; 4]>(
            context,
            4,
            4,
            3,
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        let program = screen_program(
            context,
            "
            uniform sampler2DArray tex;
            in vec2 uvs;
            layout (location = 0) out vec4 first;
            layout (location = 1) out vec4 second;
            void main() {
                first = texture(tex, vec3(uvs, 1.0));
                second = texture(tex, vec3(uvs, 0.0));
            }
        ",
        );
        target
            .as_color_target(&[2, 0], None)
            .clear(ClearState::color(0.0, 0.0, 0.0, 1.0))
            .write::<CoreError>(|| {
                program.use_texture_array("tex", &source);
                full_screen_draw(
                    context,
                    &program,
                    RenderStates::default(),
                    Viewport::new_at_origo(4, 4),
                );
                Ok(())
            })
            .unwrap();
        let mut values = Vec::new();
        for layer in [0, 2] {
            values.extend(floats(target.as_color_target(&[layer], None).read()));
        }
        values
    });
}

#[test]
fn texture_3d() {
    compare(2.0 * STEP, |context| {
        let data: Vec<[u8; 4]> = (0..4).flat_map(|z| pattern(4, 4, 30 + z)).collect();
        let texture = Texture3D::new_empty::<[u8; 4]>(
            context,
            4,
            4,
            4,
            Interpolation::Linear,
            Interpolation::Linear,
            Some(Mipmap::default()),
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        texture.fill(&data);
        let program = screen_program(
            context,
            "
            uniform sampler3D tex;
            uniform float w;
            uniform float lod;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = textureLod(tex, vec3(uvs, w), lod); }
        ",
        );
        let mut values = Vec::new();
        for (w, lod) in [
            (0.125, 0.0),
            (0.5, 0.0),
            (0.875, 0.0),
            (0.25, 1.0),
            (0.5, 2.0),
        ] {
            values.extend(draw_screen(context, 4, &program, || {
                program.use_texture_3d("tex", &texture);
                program.use_uniform("w", w);
                program.use_uniform("lod", lod);
            }));
        }
        values
    });
}

#[test]
fn render_to_mip_level_and_cube_side() {
    compare(STEP, |context| {
        let texture = Texture2D::new(context, &cpu_texture(8, 40, Some(Mipmap::default())));
        let program = Program::from_source(context, POSITION_VERTEX, COLOR_FRAGMENT).unwrap();
        let positions = quad(context, 0.0);
        let draw = |x: f32, color: Vec4, size: u32| {
            program.use_uniform("placement", vec4(x, 0.0, 0.5, 1.0));
            program.use_uniform("color", color);
            program.use_vertex_attribute("position", &positions);
            program.draw_arrays(
                RenderStates::default(),
                Viewport::new_at_origo(size, size),
                6,
            );
        };
        texture
            .as_color_target(Some(1))
            .write::<CoreError>(|| {
                draw(-0.5, vec4(1.0, 0.0, 0.5, 1.0), 4);
                Ok(())
            })
            .unwrap();
        let mut values = floats(texture.as_color_target(Some(1)).read());
        values.extend(floats(texture.as_color_target(None).read()));

        let cube = TextureCubeMap::new_empty::<[u8; 4]>(
            context,
            4,
            4,
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        for (index, side) in CubeMapSide::iter().enumerate() {
            let sides = [side];
            let target = cube.as_color_target(&sides, None);
            target
                .clear(ClearState::color(0.1 * index as f32, 0.0, 0.0, 1.0))
                .write::<CoreError>(|| {
                    draw(0.5, vec4(0.0, 1.0, 0.2 * index as f32, 1.0), 4);
                    Ok(())
                })
                .unwrap();
            values.extend(floats(target.read()));
        }
        values
    });
}

#[test]
fn depth_formats_and_reading_depth() {
    compare(1.0e-4, |context| {
        let program = Program::from_source(context, POSITION_VERTEX, COLOR_FRAGMENT).unwrap();
        let (near, far) = (quad(context, -0.5), quad(context, 0.25));
        let mut values = Vec::new();
        let mut draw = |depth: DepthTexture2D| {
            let color = color_texture(context, 8, 8);
            let target = RenderTarget::new(color.as_color_target(None), depth.as_depth_target());
            target
                .clear(ClearState::color_and_depth(0.0, 0.0, 0.0, 1.0, 1.0))
                .write::<CoreError>(|| {
                    program.use_uniform("color", vec4(1.0, 0.0, 0.0, 1.0));
                    program.use_uniform("placement", vec4(-0.5, 0.0, 0.5, 0.5));
                    program.use_vertex_attribute("position", &near);
                    program.draw_arrays(RenderStates::default(), Viewport::new_at_origo(8, 8), 6);
                    // Behind the first quad where they overlap
                    program.use_uniform("color", vec4(0.0, 1.0, 0.0, 1.0));
                    program.use_uniform("placement", vec4(0.0, 0.25, 0.5, 0.5));
                    program.use_vertex_attribute("position", &far);
                    program.draw_arrays(RenderStates::default(), Viewport::new_at_origo(8, 8), 6);
                    Ok(())
                })
                .unwrap();
            values.extend(target.read_depth());
            values.extend(target.read_depth_partially(ScissorBox {
                x: 1,
                y: 2,
                width: 5,
                height: 3,
            }));
            values.extend(floats(target.read_color()));
            values.extend(floats(target.read_color_partially(ScissorBox {
                x: 3,
                y: 1,
                width: 2,
                height: 6,
            })));
        };
        draw(depth_texture::<f32>(context, 8, 8));
        draw(depth_texture::<f24>(context, 8, 8));
        draw(depth_texture::<f16>(context, 8, 8));
        values
    });
}

#[test]
fn depth_texture_is_sampled() {
    compare(1.0e-4, |context| {
        let program = Program::from_source(context, POSITION_VERTEX, COLOR_FRAGMENT).unwrap();
        let positions = quad(context, 0.5);
        let depth = depth_texture::<f32>(context, 8, 8);
        depth
            .as_depth_target()
            .clear(ClearState::depth(1.0))
            .write::<CoreError>(|| {
                program.use_uniform("color", vec4(1.0, 0.0, 0.0, 1.0));
                program.use_uniform("placement", vec4(0.25, -0.25, 0.5, 0.5));
                program.use_vertex_attribute("position", &positions);
                program.draw_arrays(RenderStates::default(), Viewport::new_at_origo(8, 8), 6);
                Ok(())
            })
            .unwrap();
        let sample = screen_program(
            context,
            "
            uniform sampler2D depthMap;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = vec4(texture(depthMap, uvs).r, 0.0, 0.0, 1.0); }
        ",
        );
        let target = Texture2D::new_empty::<[f32; 4]>(
            context,
            8,
            8,
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        let pixels: Vec<[f32; 4]> = target
            .as_color_target(None)
            .write::<CoreError>(|| {
                sample.use_depth_texture("depthMap", &depth);
                full_screen_draw(
                    context,
                    &sample,
                    RenderStates::default(),
                    Viewport::new_at_origo(8, 8),
                );
                Ok(())
            })
            .unwrap()
            .read();
        pixels.into_iter().flatten().collect()
    });
}

#[test]
fn partial_clear() {
    compare(1.0e-4, |context| {
        let color = color_texture(context, 8, 8);
        let depth = depth_texture::<f32>(context, 8, 8);
        let target = RenderTarget::new(color.as_color_target(None), depth.as_depth_target());
        target
            .clear(ClearState::color_and_depth(0.2, 0.4, 0.6, 0.8, 0.9))
            // A part of the target
            .clear_partially(
                ScissorBox {
                    x: 1,
                    y: 2,
                    width: 4,
                    height: 3,
                },
                ClearState::color_and_depth(1.0, 0.0, 0.0, 1.0, 0.25),
            )
            // Only some channels, in a part that reaches out of the target
            .clear_partially(
                ScissorBox {
                    x: 4,
                    y: 4,
                    width: 10,
                    height: 10,
                },
                ClearState {
                    red: None,
                    green: Some(1.0),
                    blue: None,
                    alpha: Some(0.0),
                    depth: None,
                },
            )
            // Only some channels of the whole target
            .clear(ClearState {
                red: None,
                green: None,
                blue: Some(0.0),
                alpha: None,
                depth: None,
            })
            // Only the depth of a part
            .clear_partially(
                ScissorBox {
                    x: 0,
                    y: 6,
                    width: 8,
                    height: 2,
                },
                ClearState::depth(0.5),
            );
        let mut values = floats(target.read_color());
        values.extend(target.read_depth());
        values
    });
}

/// Draws overlapping triangles with depth test into a multisample target.
fn draw_multisample_scene(context: &Context, program: &Program) {
    let triangle = |z: f32, x: f32| {
        VertexBuffer::new_with_data(
            context,
            &[
                vec3(-0.8 + x, -0.7, z),
                vec3(0.7 + x, -0.5, z),
                vec3(-0.1 + x, 0.8, z),
            ],
        )
    };
    program.use_uniform("placement", vec4(0.0, 0.0, 1.0, 1.0));
    for (z, x, color) in [
        (0.0, 0.0, vec4(1.0, 0.0, 0.0, 1.0)),
        (0.5, 0.2, vec4(0.0, 1.0, 0.0, 1.0)),
    ] {
        let positions = triangle(z, x);
        program.use_uniform("color", color);
        program.use_vertex_attribute("position", &positions);
        program.draw_arrays(
            RenderStates {
                cull: Cull::None,
                ..Default::default()
            },
            Viewport::new_at_origo(16, 16),
            3,
        );
    }
}

#[test]
fn multisample() {
    // The edges of the triangles are smoothed the same way, since both backends have the same positions of the 4 samples
    compare(STEP, |context| {
        let program = Program::from_source(context, POSITION_VERTEX, COLOR_FRAGMENT).unwrap();
        let target = RenderTargetMultisample::<[u8; 4], f32>::new(context, 16, 16, 4);
        target
            .clear(ClearState::color_and_depth(0.0, 0.0, 1.0, 1.0, 1.0))
            .write::<CoreError>(|| {
                draw_multisample_scene(context, &program);
                Ok(())
            })
            .unwrap();
        let (color, depth) = target.resolve();
        let mut values = floats(color.as_color_target(None).read());
        assert!(
            values.iter().any(|v| *v > 0.1 && *v < 0.9),
            "edges are smoothed"
        );
        // The depth of a pixel is the depth of one of its samples, which one is not defined, so only pixels far from edges are compared
        let depths = depth.as_depth_target().read();
        for (x, y) in [(0, 0), (5, 5), (8, 8), (11, 6), (15, 15)] {
            values.push(depths[y * 16 + x]);
        }

        let color_only = ColorTargetMultisample::<[u8; 4]>::new(context, 16, 16, 4);
        color_only
            .clear(ClearState::color(0.5, 0.5, 0.5, 1.0))
            .write::<CoreError>(|| {
                draw_multisample_scene(context, &program);
                Ok(())
            })
            .unwrap();
        values.extend(floats(color_only.resolve().as_color_target(None).read()));
        values
    });
}

#[test]
fn uniform_block_and_arrays() {
    compare(STEP, |context| {
        let program = screen_program(
            context,
            "
            uniform Colors {
                vec4 first;
                vec3 second;
                float factor;
            };
            uniform vec3 list[3];
            uniform mat3 matrix;
            uniform int index;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() {
                vec3 color = uvs.x < 0.5 ? first.rgb : second * factor;
                outColor = vec4(matrix * color + list[index], 1.0);
            }
        ",
        );
        let mut buffer = UniformBuffer::new(context, &[4, 3, 1]);
        buffer.update(0, &[0.1, 0.2, 0.3, 1.0]);
        buffer.update(1, &[0.8, 0.6, 0.4]);
        buffer.update(2, &[0.5]);
        let mut values = Vec::new();
        for index in 0..3 {
            values.extend(draw_screen(context, 4, &program, || {
                program.use_uniform_block("Colors", &buffer);
                program.use_uniform_array(
                    "list",
                    &[
                        vec3(0.0, 0.0, 0.0),
                        vec3(0.1, 0.0, 0.2),
                        vec3(0.0, 0.3, 0.0),
                    ],
                );
                program.use_uniform(
                    "matrix",
                    Mat3::new(0.5, 0.0, 0.0, 0.25, 1.0, 0.0, 0.0, 0.0, 0.75),
                );
                program.use_uniform("index", index);
            }));
        }
        values
    });
}

#[test]
fn vertex_formats_instances_and_subsets() {
    compare(STEP, |context| {
        let program = Program::from_source(
            context,
            "
            in vec2 position;
            in uvec3 bytes;
            in ivec3 shorts;
            in uint wide;
            in vec2 half_floats;
            in vec3 offset;
            out vec4 col;
            void main() {
                col = 0.33 * (vec4(vec3(bytes) / 255.0, 1.0) + vec4(vec3(shorts) / 1000.0, 1.0)
                    + vec4(float(wide) / 60000.0)) + vec4(half_floats, 0.0, 0.0);
                gl_Position = vec4(0.4 * position + offset.xy, offset.z, 1.0);
            }
        ",
            "
            in vec4 col;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = col; }
        ",
        )
        .unwrap();
        let positions = VertexBuffer::new_with_data(
            context,
            &[
                vec2(-1.0, -1.0),
                vec2(1.0, -1.0),
                vec2(1.0, 1.0),
                vec2(-1.0, 1.0f32),
            ],
        );
        let bytes = VertexBuffer::new_with_data(
            context,
            &[[255u8, 0, 0], [0, 255, 0], [0, 0, 255], [9; 3]],
        );
        let mut shorts = VertexBuffer::new_with_data(
            context,
            &[[1000i16, 0, 500], [0; 3], [0; 3], [-100, 1000, 0]],
        );
        // Replaces the second and third vertex, the offset is in bytes
        shorts.fill_subset(6, &[[0i16, 1000, 0], [300, 300, 300]]);
        let wide = VertexBuffer::new_with_data(context, &[60000u16, 30000, 0, 15000]);
        let half_floats = VertexBuffer::new_with_data(
            context,
            &[[0.0, 0.1], [0.1, 0.0], [0.05, 0.05], [0.0, 0.0]].map(|v| v.map(f16::from_f32)),
        );
        let offsets = InstanceBuffer::new_with_data(
            context,
            &[
                vec3(-0.5, -0.5, 0.0),
                vec3(0.5, -0.5, 0.0),
                vec3(0.0, 0.5, 0.0f32),
            ],
        );
        // 8 and 32 bit indices, the former with a part that is replaced
        let mut elements_u8 = ElementBuffer::new_with_data(context, &[0u8, 1, 2, 0, 0, 0]);
        elements_u8.fill_subset(3, &[2u8, 3, 0]);
        let elements_u32 = ElementBuffer::new_with_data(context, &[0u32, 1, 2]);
        let mut values = Vec::new();
        for pass in 0..2 {
            let texture = color_texture(context, 16, 16);
            values.extend(floats(
                texture
                    .as_color_target(None)
                    .clear(ClearState::color(0.0, 0.0, 0.0, 1.0))
                    .write::<CoreError>(|| {
                        program.use_vertex_attribute("position", &positions);
                        program.use_vertex_attribute("bytes", &bytes);
                        program.use_vertex_attribute("shorts", &shorts);
                        program.use_vertex_attribute("wide", &wide);
                        program.use_vertex_attribute("half_floats", &half_floats);
                        program.use_instance_attribute("offset", &offsets);
                        let render_states = RenderStates {
                            cull: Cull::None,
                            ..Default::default()
                        };
                        // The viewport is larger than the target and starts outside of it
                        let viewport = Viewport {
                            x: -4,
                            y: -2,
                            width: 24,
                            height: 20,
                        };
                        if pass == 0 {
                            program.draw_elements_instanced(
                                render_states,
                                viewport,
                                &elements_u8,
                                3,
                            );
                        } else {
                            program.draw_elements_instanced(
                                render_states,
                                viewport,
                                &elements_u32,
                                2,
                            );
                        }
                        Ok(())
                    })
                    .unwrap()
                    .read(),
            ));
        }
        values
    });
}

#[test]
fn render_states_and_scissor() {
    compare(STEP, |context| {
        let program = Program::from_source(context, POSITION_VERTEX, COLOR_FRAGMENT).unwrap();
        let positions = quad(context, 0.0);
        let color = color_texture(context, 8, 8);
        let depth = depth_texture::<f32>(context, 8, 8);
        let target = RenderTarget::new(color.as_color_target(None), depth.as_depth_target());
        let draw = |placement: Vec4, color: Vec4, render_states: RenderStates| {
            program.use_uniform("placement", placement);
            program.use_uniform("color", color);
            program.use_vertex_attribute("position", &positions);
            program.draw_arrays(render_states, Viewport::new_at_origo(8, 8), 6);
        };
        target
            .clear(ClearState::color_and_depth(0.2, 0.2, 0.2, 1.0, 1.0))
            .write::<CoreError>(|| {
                draw(
                    vec4(-0.5, -0.5, 0.5, 0.5),
                    vec4(1.0, 0.5, 0.25, 0.5),
                    RenderStates {
                        blend: Blend::TRANSPARENCY,
                        ..Default::default()
                    },
                );
                draw(
                    vec4(0.5, -0.5, 0.5, 0.5),
                    vec4(1.0, 1.0, 1.0, 1.0),
                    RenderStates {
                        write_mask: WriteMask {
                            red: false,
                            green: true,
                            blue: false,
                            alpha: true,
                            depth: false,
                        },
                        ..Default::default()
                    },
                );
                draw(
                    vec4(0.5, 0.5, 0.5, 0.5),
                    vec4(0.5, 0.5, 0.5, 1.0),
                    RenderStates {
                        blend: Blend::ADD,
                        depth_test: DepthTest::Always,
                        ..Default::default()
                    },
                );
                // Fails the depth test where the first quad is
                draw(
                    vec4(-0.25, 0.0, 0.5, 0.75),
                    vec4(0.0, 0.0, 1.0, 1.0),
                    RenderStates {
                        depth_test: DepthTest::Less,
                        cull: Cull::Back,
                        ..Default::default()
                    },
                );
                // The quad is counter clockwise, so its front is culled here
                draw(
                    vec4(0.0, 0.0, 1.0, 1.0),
                    vec4(1.0, 0.0, 1.0, 1.0),
                    RenderStates {
                        cull: Cull::Front,
                        ..Default::default()
                    },
                );
                Ok(())
            })
            .unwrap()
            // Only a part of the target is written
            .write_partially::<CoreError>(
                ScissorBox {
                    x: 3,
                    y: 0,
                    width: 2,
                    height: 5,
                },
                || {
                    draw(
                        vec4(0.0, 0.0, 1.0, 1.0),
                        vec4(0.0, 1.0, 1.0, 1.0),
                        RenderStates {
                            depth_test: DepthTest::Always,
                            ..Default::default()
                        },
                    );
                    Ok(())
                },
            )
            .unwrap();
        let mut values = floats(target.read_color());
        values.extend(target.read_depth());
        values
    });
}

///
/// More draw calls in one pass than fit into the buffer for the uniforms, and another target in between.
///
#[test]
fn many_draw_calls() {
    compare(STEP, |context| {
        let program = Program::from_source(context, POSITION_VERTEX, COLOR_FRAGMENT).unwrap();
        let positions = quad(context, 0.0);
        let textured = screen_program(
            context,
            "
            uniform sampler2D tex;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = texture(tex, uvs); }
        ",
        );
        let size = 64;
        let color = color_texture(context, size, size);
        let small = color_texture(context, 2, 2);
        let target = color.as_color_target(None);
        target.clear(ClearState::color(0.0, 0.0, 0.0, 1.0));
        for part in 0..2u32 {
            if part == 1 {
                // Another target in between, which is then drawn into the corner of the first one
                small
                    .as_color_target(None)
                    .clear(ClearState::color(1.0, 1.0, 0.0, 1.0))
                    .write::<CoreError>(|| {
                        program.use_uniform("placement", vec4(-0.5, -0.5, 0.5, 0.5));
                        program.use_uniform("color", vec4(0.0, 1.0, 1.0, 1.0));
                        program.use_vertex_attribute("position", &positions);
                        program.draw_arrays(
                            RenderStates::default(),
                            Viewport::new_at_origo(2, 2),
                            6,
                        );
                        Ok(())
                    })
                    .unwrap();
            }
            target
                .write::<CoreError>(|| {
                    if part == 1 {
                        textured.use_texture("tex", &small);
                        full_screen_draw(
                            context,
                            &textured,
                            RenderStates::default(),
                            Viewport::new_at_origo(4, 4),
                        );
                    }
                    for i in part * 2500..(part + 1) * 2500 {
                        let (x, y) = (i % size, (i / size) % size);
                        program.use_uniform(
                            "placement",
                            vec4(
                                (x as f32 + 0.5) / size as f32 * 2.0 - 1.0,
                                (y as f32 + 0.5) / size as f32 * 2.0 - 1.0,
                                1.0 / size as f32,
                                1.0 / size as f32,
                            ),
                        );
                        program.use_uniform(
                            "color",
                            vec4(
                                (i % 7) as f32 / 6.0,
                                (i % 11) as f32 / 10.0,
                                (i / 1000) as f32 / 5.0,
                                1.0,
                            ),
                        );
                        program.use_vertex_attribute("position", &positions);
                        program.draw_arrays(
                            RenderStates::default(),
                            Viewport::new_at_origo(size, size),
                            6,
                        );
                    }
                    Ok(())
                })
                .unwrap();
        }
        floats(target.read())
    });
}

///
/// A texture and a buffer that are changed between draw calls: every draw call sees the content at the time it is made.
///
#[test]
fn changes_between_draw_calls() {
    compare(STEP, |context| {
        let program = Program::from_source(
            context,
            "
            uniform vec2 offset;
            in vec2 position;
            out vec2 uvs;
            void main() { uvs = position; gl_Position = vec4(0.5 * position + offset, 0.0, 1.0); }
        ",
            "
            uniform sampler2D tex;
            in vec2 uvs;
            layout (location = 0) out vec4 outColor;
            void main() { outColor = texture(tex, 0.5 * uvs + 0.5); }
        ",
        )
        .unwrap();
        let texture = Texture2D::new(context, &cpu_texture(4, 50, None));
        let mut positions = VertexBuffer::new_with_data(
            context,
            &[
                vec2(-1.0, -1.0),
                vec2(1.0, -1.0),
                vec2(1.0, 1.0f32),
                vec2(-1.0, -1.0),
                vec2(1.0, 1.0),
                vec2(-1.0, 1.0),
            ],
        );
        let color = color_texture(context, 16, 16);
        let target = color.as_color_target(None);
        target.clear(ClearState::color(0.0, 0.0, 0.0, 1.0));
        for step in 0..3 {
            target
                .write::<CoreError>(|| {
                    program.use_uniform("offset", vec2(-0.5 + 0.5 * step as f32, 0.0));
                    program.use_texture("tex", &texture);
                    program.use_vertex_attribute("position", &positions);
                    program.draw_arrays(RenderStates::default(), Viewport::new_at_origo(16, 16), 6);
                    // Changed inside the pass, after the draw call that uses the old content
                    texture.fill(&pattern(4, 4, 60 + step));
                    // The offset is in bytes, this is the last vertex
                    positions.fill_subset(5 * 8, &[vec2(-1.0, 0.0 + 0.5 * step as f32)]);
                    Ok(())
                })
                .unwrap();
        }
        floats(target.read())
    });
}

///
/// The renderer module on both backends: lights with shadows, which render depth and sample it, and a deferred pipeline.
///
#[test]
fn renderer_with_shadows() {
    use crate::renderer::*;
    // The surfaces are lit with a few numerical differences, the edges are the same
    compare(4.0 * STEP, |context| {
        let camera = Camera::new_perspective(
            Viewport::new_at_origo(64, 64),
            vec3(2.0, 3.0, 5.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            degrees(45.0),
            0.1,
            100.0,
        );
        let mut sphere = Gm::new(
            Mesh::new(context, &CpuMesh::sphere(16)),
            PhysicalMaterial::new_opaque(
                context,
                &CpuMaterial {
                    albedo: Srgba::new(200, 60, 40, 255),
                    metallic: 0.3,
                    roughness: 0.4,
                    ..Default::default()
                },
            ),
        );
        sphere.set_transformation(Mat4::from_translation(vec3(0.0, 0.2, 0.0)));
        let mut ground = Gm::new(
            Mesh::new(context, &CpuMesh::square()),
            PhysicalMaterial::new_opaque(
                context,
                &CpuMaterial {
                    albedo: Srgba::new(150, 150, 150, 255),
                    ..Default::default()
                },
            ),
        );
        ground.set_transformation(
            Mat4::from_translation(vec3(0.0, -1.0, 0.0))
                * Mat4::from_angle_x(degrees(-90.0))
                * Mat4::from_scale(4.0),
        );
        let ambient = AmbientLight::new(context, 0.3, Srgba::WHITE);
        let mut directional =
            DirectionalLight::new(context, 2.0, Srgba::WHITE, vec3(-1.0, -2.0, -0.5));
        directional
            .generate_shadow_map(256, sphere.into_iter().chain(&ground))
            .unwrap();
        let mut spot = SpotLight::new(
            context,
            2.0,
            Srgba::new(255, 220, 150, 255),
            vec3(2.0, 3.0, 0.0),
            vec3(-2.0, -3.0, 0.0),
            degrees(30.0),
            Attenuation::default(),
        );
        spot.generate_shadow_map(256, sphere.into_iter().chain(&ground))
            .unwrap();
        let color = color_texture(context, 64, 64);
        let depth = depth_texture::<f32>(context, 64, 64);
        let pixels = RenderTarget::new(color.as_color_target(None), depth.as_depth_target())
            .clear(ClearState::color_and_depth(0.1, 0.1, 0.2, 1.0, 1.0))
            .render(
                &camera,
                sphere.into_iter().chain(&ground),
                &[&ambient, &directional, &spot],
            )
            .read_color();
        floats(pixels)
    });
}
