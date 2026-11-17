//!
//! Draws with a program written in WGSL on the wgpu backend and with the same program written in GLSL on the OpenGL backend
//! and compares what is read back, see [compare_tests](super::compare_tests).
//!

use super::compare_tests::*;
use crate::core::*;

const GLSL_VERTEX: &str = "
    uniform vec4 placement;
    in vec3 position;
    in vec2 offset;
    out vec2 uv;
    void main() {
        uv = 0.5 * position.xy + 0.5;
        gl_Position = vec4(position.xy * placement.zw + placement.xy + offset, position.z, 1.0);
    }
";

const GLSL_FRAGMENT: &str = "
    uniform Colors {
        vec4 first;
        vec3 second;
        float factor;
    };
    uniform sampler2D image;
    uniform sampler2D other;
    uniform vec3 list[3];
    uniform mat3 matrix;
    uniform int index;
    uniform vec4 tint;
    in vec2 uv;
    layout (location = 0) out vec4 outColor;
    void main() {
        vec3 color = uv.x < 0.5 ? texture(image, uv).rgb : texture(other, uv).rgb * second * factor;
        color = matrix * color + list[index] + 0.25 * first.rgb;
        if (gl_FragCoord.y < 3.0) {
            color.b = 1.0 - color.b;
        }
        outColor = vec4(color, 1.0) * tint;
    }
";

const WGSL: &str = "
    struct Placement {
        placement: vec4<f32>,
    }
    struct Values {
        list: array<vec3<f32>, 3>,
        matrix: mat3x3<f32>,
        index: i32,
        tint: vec4<f32>,
    }
    struct ColorBlock {
        first: vec4<f32>,
        second: vec3<f32>,
        factor: f32,
    }
    @group(0) @binding(0) var<uniform> first_values: Placement;
    @group(0) @binding(1) var<uniform> values: Values;
    @group(1) @binding(0) var image: texture_2d<f32>;
    @group(1) @binding(1) var image_sampler: sampler;
    @group(1) @binding(2) var other: texture_2d<f32>;
    @group(1) @binding(3) var other_sampler: sampler;
    @group(2) @binding(0) var<uniform> Colors: ColorBlock;

    struct Instance {
        @location(1) offset: vec2<f32>,
    }
    struct Varyings {
        @builtin(position) position: vec4<f32>,
        @location(0) uv: vec2<f32>,
    }

    @vertex
    fn vertex(@location(0) position: vec3<f32>, instance: Instance) -> Varyings {
        var out: Varyings;
        out.uv = 0.5 * position.xy + 0.5;
        out.position = vec4(position.xy * first_values.placement.zw + first_values.placement.xy + instance.offset, position.z, 1.0);
        if (position.z > 5.0) {
            // Never taken, but a second return statement
            return out;
        }
        return out;
    }

    @fragment
    fn fragment(in: Varyings) -> @location(0) vec4<f32> {
        let a = textureSample(image, image_sampler, in.uv).rgb;
        let b = textureSample(other, other_sampler, in.uv).rgb * Colors.second * Colors.factor;
        var color = select(b, a, in.uv.x < 0.5);
        color = values.matrix * color + values.list[values.index] + 0.25 * Colors.first.rgb;
        if (in.position.y < 3.0) {
            color.b = 1.0 - color.b;
        }
        return vec4(color, 1.0) * values.tint;
    }
";

fn texture(context: &Context, seed: u32) -> Texture2D {
    Texture2D::new(
        context,
        &CpuTexture {
            data: TextureData::RgbaU8(pattern(4, 4, seed)),
            width: 4,
            height: 4,
            min_filter: Interpolation::Nearest,
            mag_filter: Interpolation::Nearest,
            mipmap: None,
            wrap_s: Wrapping::ClampToEdge,
            wrap_t: Wrapping::ClampToEdge,
            ..Default::default()
        },
    )
}

#[test]
fn wgsl_program_matches_glsl_program() {
    compare(STEP, |context| {
        let program = if context.as_wgpu().is_some() {
            Program::from_wgsl_source(context, WGSL).unwrap()
        } else {
            Program::from_source(context, GLSL_VERTEX, GLSL_FRAGMENT).unwrap()
        };
        let image = texture(context, 1);
        let other = texture(context, 5);
        let mut block = UniformBuffer::new(context, &[4, 3, 1]);
        block.update(0, &[0.1, 0.2, 0.3, 1.0]);
        block.update(1, &[0.8, 0.6, 0.4]);
        block.update(2, &[0.5]);
        let offsets = InstanceBuffer::new_with_data(context, &[vec2(0.0, 0.0), vec2(0.75, 0.5)]);
        let color = color_texture(context, 16, 16);
        let depth = depth_texture::<f32>(context, 16, 16);
        let draw = |z: f32, placement: Vec4, tint: Vec4, index: i32| {
            // Not symmetric, so a flipped triangle order or axis shows
            let positions = VertexBuffer::new_with_data(
                context,
                &[
                    vec3(-1.0, -1.0, z),
                    vec3(1.0, -1.0, z),
                    vec3(1.0, 0.5, z),
                    vec3(1.0, 0.5, z),
                    vec3(-1.0, 1.0, z),
                    vec3(-1.0, -1.0, z),
                ],
            );
            program.use_uniform("placement", placement);
            program.use_uniform("tint", tint);
            program.use_uniform("index", index);
            program.use_uniform(
                "matrix",
                Mat3::new(0.5, 0.0, 0.0, 0.25, 1.0, 0.0, 0.0, 0.0, 0.75),
            );
            program.use_uniform_array(
                "list",
                &[
                    vec3(0.0, 0.0, 0.0),
                    vec3(0.1, 0.0, 0.2),
                    vec3(0.0, 0.3, 0.0),
                ],
            );
            program.use_texture("image", &image);
            program.use_texture("other", &other);
            program.use_uniform_block("Colors", &block);
            program.use_vertex_attribute("position", &positions);
            program.use_instance_attribute("offset", &offsets);
            program.draw_arrays_instanced(
                RenderStates {
                    cull: Cull::Back,
                    ..Default::default()
                },
                Viewport::new_at_origo(16, 16),
                6,
                2,
            );
        };
        let pixels = floats(
            RenderTarget::new(color.as_color_target(None), depth.as_depth_target())
                .clear(ClearState::color_and_depth(0.2, 0.2, 0.2, 1.0, 1.0))
                .write::<CoreError>(|| {
                    // In front, close to the near plane of OpenGL, which is outside the clip space of wgpu without the conversion
                    draw(
                        -0.9,
                        vec4(-0.4, -0.3, 0.4, 0.5),
                        vec4(1.0, 1.0, 1.0, 1.0),
                        1,
                    );
                    // Behind the first one where they overlap, close to the far plane
                    draw(0.9, vec4(-0.1, 0.1, 0.6, 0.4), vec4(0.5, 1.0, 0.5, 1.0), 2);
                    Ok(())
                })
                .unwrap()
                .read_color(),
        );
        pixels
    });
}

#[test]
fn wgsl_needs_wgpu_and_reports_mistakes() {
    let Ok(context) = Context::new_headless_wgpu() else {
        assert!(std::env::var("THREE_D_REQUIRE_WGPU").is_err());
        return;
    };
    let error = |source: &str| match Program::from_wgsl_source(&context, source) {
        Err(CoreError::ShaderCompilation(_, _, log)) => log,
        Err(error) => panic!("unexpected error {error}"),
        Ok(_) => panic!("the program should not compile"),
    };
    assert!(error("fn nothing() {}").contains("no vertex entry point"));
    assert!(error(
        "@vertex fn v() -> @builtin(position) vec4<f32> { return vec4(0.0); }
         @fragment fn f() -> @location(0) vec4<f32> { return missing; }"
    )
    .contains("missing"));
    assert!(error(
        "@group(3) @binding(0) var<uniform> wrong: vec4<f32>;
         @vertex fn v() -> @builtin(position) vec4<f32> { return wrong; }
         @fragment fn f() -> @location(0) vec4<f32> { return vec4(1.0); }"
    )
    .contains("group 3 is not supported"));
    assert!(error(
        "@group(1) @binding(2) var image: texture_2d<f32>;
         @vertex fn v() -> @builtin(position) vec4<f32> { return vec4(0.0); }
         @fragment fn f() -> @location(0) vec4<f32> { return textureLoad(image, vec2(0, 0), 0); }"
    )
    .contains("without a gap"));

    // The position can be the result itself
    Program::from_wgsl_source(
        &context,
        "@vertex fn v(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> { return vec4(position, 1.0); }
         @fragment fn f() -> @location(0) vec4<f32> { return vec4(1.0); }",
    )
    .unwrap();

    if let Ok((gl, _keep)) = gl_context() {
        assert!(matches!(
            Program::from_wgsl_source(&gl, WGSL),
            Err(CoreError::ShaderCompilation(..))
        ));
    }
}

/// The module with the converted position must be writable for every platform, the browser gets it as WGSL text.
#[test]
fn converted_wgsl_can_be_written_for_every_platform() {
    let shader = super::wgsl::parse(WGSL).unwrap();
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&shader.module)
    .unwrap();
    let text = naga::back::wgsl::write_string(
        &shader.module,
        &info,
        naga::back::wgsl::WriterFlags::empty(),
    )
    .unwrap();
    assert!(text.contains("three_d_result"));
    naga::front::wgsl::parse_str(&text).unwrap();
    naga::back::msl::write_string(
        &shader.module,
        &info,
        &naga::back::msl::Options {
            lang_version: (2, 3),
            ..Default::default()
        },
        &naga::back::msl::PipelineOptions::default(),
    )
    .unwrap();
    let mut hlsl = String::new();
    let pipeline_options = Default::default();
    naga::back::hlsl::Writer::new(
        &mut hlsl,
        &naga::back::hlsl::Options::default(),
        &pipeline_options,
    )
    .write(&shader.module, &info, None)
    .unwrap();
    for (stage, entry_point) in [
        (naga::ShaderStage::Vertex, &shader.entry_points[0]),
        (naga::ShaderStage::Fragment, &shader.entry_points[1]),
    ] {
        naga::back::spv::write_vec(
            &shader.module,
            &info,
            &naga::back::spv::Options::default(),
            Some(&naga::back::spv::PipelineOptions {
                shader_stage: stage,
                entry_point: entry_point.clone(),
            }),
        )
        .unwrap();
    }
}
