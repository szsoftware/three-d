//!
//! The passes the backend needs for itself, written in WGSL: generating mipmaps, copying and reading depth and clearing a part of a target.
//! They all draw one triangle that covers the whole target.
//!

use super::*;

///
/// What a builtin pass does with its source texture.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    /// Writes the average of 2x2 texels of a 2D texture.
    Mip2D,
    /// Writes the average of 2x2x2 texels of a 3D texture, the instance index is the slice that is written.
    Mip3D,
    /// Writes the depth into the red channel of a color target.
    DepthToColor { multisampled: bool },
    /// Writes the depth into a depth target.
    DepthToDepth { multisampled: bool },
    /// Writes the color upside down and opaque, for a surface.
    Present,
}

#[derive(Default)]
pub(super) struct Builtin {
    pipelines: HashMap<(Kind, w::TextureFormat), (w::RenderPipeline, w::BindGroupLayout)>,
    clear_pipelines: HashMap<(TargetFormats, u8, bool), (w::RenderPipeline, w::BindGroupLayout)>,
}

// The shaders are written in the GLSL dialect that the translated shaders of the programs use as well, so no other shader language has to be compiled in

const VERTEX: &str = "#version 450
layout(location = 0) flat out uint slice;
void main() {
    vec2 position = vec2(float(gl_VertexIndex & 1), float(gl_VertexIndex >> 1)) * 4.0 - 1.0;
    slice = uint(gl_InstanceIndex);
    gl_Position = vec4(position, 0.0, 1.0);
}
";

fn fragment(kind: Kind) -> String {
    let (texture_type, main) = match kind {
        Kind::Mip2D => (
            "texture2D",
            "
    ivec2 last = textureSize(sampler2D(source_t, source_s), 0) - ivec2(1);
    ivec2 texel = ivec2(gl_FragCoord.xy) * 2;
    color = 0.25 * (
        texelFetch(sampler2D(source_t, source_s), min(texel, last), 0)
        + texelFetch(sampler2D(source_t, source_s), min(texel + ivec2(1, 0), last), 0)
        + texelFetch(sampler2D(source_t, source_s), min(texel + ivec2(0, 1), last), 0)
        + texelFetch(sampler2D(source_t, source_s), min(texel + ivec2(1, 1), last), 0));",
        ),
        Kind::Mip3D => (
            "texture3D",
            "
    ivec3 last = textureSize(sampler3D(source_t, source_s), 0) - ivec3(1);
    ivec3 texel = ivec3(ivec2(gl_FragCoord.xy), int(slice)) * 2;
    color = vec4(0.0);
    for (int i = 0; i < 8; i++) {
        ivec3 offset = ivec3(i & 1, (i >> 1) & 1, i >> 2);
        color += 0.125 * texelFetch(sampler3D(source_t, source_s), min(texel + offset, last), 0);
    }",
        ),
        Kind::Present => (
            "texture2D",
            "
    ivec2 texel = ivec2(gl_FragCoord.xy);
    texel.y = textureSize(sampler2D(source_t, source_s), 0).y - 1 - texel.y;
    color = vec4(texelFetch(sampler2D(source_t, source_s), texel, 0).rgb, 1.0);",
        ),
        Kind::DepthToColor {
            multisampled: false,
        } => (
            "texture2D",
            "
    color = vec4(texelFetch(sampler2D(source_t, source_s), ivec2(gl_FragCoord.xy), 0).r, 0.0, 0.0, 1.0);",
        ),
        Kind::DepthToColor { multisampled: true } => (
            "texture2DMS",
            "
    color = vec4(texelFetch(sampler2DMS(source_t, source_s), ivec2(gl_FragCoord.xy), 0).r, 0.0, 0.0, 1.0);",
        ),
        Kind::DepthToDepth {
            multisampled: false,
        } => (
            "texture2D",
            "
    gl_FragDepth = texelFetch(sampler2D(source_t, source_s), ivec2(gl_FragCoord.xy), 0).r;",
        ),
        Kind::DepthToDepth { multisampled: true } => (
            "texture2DMS",
            "
    gl_FragDepth = texelFetch(sampler2DMS(source_t, source_s), ivec2(gl_FragCoord.xy), 0).r;",
        ),
    };
    let output = if matches!(kind, Kind::DepthToDepth { .. }) {
        ""
    } else {
        "layout(location = 0) out vec4 color;"
    };
    format!(
        "#version 450
layout(set = 0, binding = 0) uniform {texture_type} source_t;
layout(set = 0, binding = 1) uniform sampler source_s;
layout(location = 0) flat in uint slice;
{output}
void main() {{{main}
}}
"
    )
}

/// The fragment shader that writes the same color to the given number of color targets.
fn clear_shader(number_of_colors: usize) -> String {
    let mut source = "#version 450
layout(set = 0, binding = 0) uniform Uniforms { vec4 color; } uniforms;
"
    .to_string();
    for index in 0..number_of_colors {
        source.push_str(&format!(
            "layout(location = {index}) out vec4 color{index};\n"
        ));
    }
    source.push_str("void main() {\n");
    for index in 0..number_of_colors {
        source.push_str(&format!("color{index} = uniforms.color;\n"));
    }
    source.push_str("}\n");
    source
}

impl WgpuBackend {
    fn builtin_module(&self, stage: glsl::Stage, source: &str) -> w::ShaderModule {
        let (module, _) = parse_shader(stage, source).unwrap_or_else(|error| {
            panic!("a builtin shader of the wgpu backend is invalid: {error}")
        });
        self.shader_module(module)
    }

    ///
    /// Records a pass that reads from the source and writes to the whole destination, which has the given format.
    /// If the destination is a 3D texture, `slice` is the slice that is written.
    ///
    pub(super) fn builtin_pass(
        &self,
        state: &mut State,
        kind: Kind,
        source: &w::TextureView,
        destination: &w::TextureView,
        format: w::TextureFormat,
        slice: Option<u32>,
    ) {
        let to_depth = matches!(kind, Kind::DepthToDepth { .. });
        let (pipeline, layout) = state
            .builtin
            .pipelines
            .entry((kind, format))
            .or_insert_with(|| {
                let (sample_type, view_dimension, multisampled) = match kind {
                    Kind::Mip2D | Kind::Present => (
                        w::TextureSampleType::Float { filterable: false },
                        w::TextureViewDimension::D2,
                        false,
                    ),
                    Kind::Mip3D => (
                        w::TextureSampleType::Float { filterable: false },
                        w::TextureViewDimension::D3,
                        false,
                    ),
                    // A depth texture can be bound as a float texture that is not filtered
                    Kind::DepthToColor { multisampled } | Kind::DepthToDepth { multisampled } => (
                        w::TextureSampleType::Float { filterable: false },
                        w::TextureViewDimension::D2,
                        multisampled,
                    ),
                };
                let layout = self
                    .device
                    .create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                        label: None,
                        entries: &[w::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: w::ShaderStages::FRAGMENT,
                            ty: w::BindingType::Texture {
                                sample_type,
                                view_dimension,
                                multisampled,
                            },
                            count: None,
                        }],
                    });
                let vertex = self.builtin_module(glsl::Stage::Vertex, VERTEX);
                let module = self.builtin_module(glsl::Stage::Fragment, &fragment(kind));
                let color_target = Some(w::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: w::ColorWrites::ALL,
                });
                let pipeline_layout =
                    self.device
                        .create_pipeline_layout(&w::PipelineLayoutDescriptor {
                            label: None,
                            bind_group_layouts: &[Some(&layout)],
                            immediate_size: 0,
                        });
                let pipeline = self
                    .device
                    .create_render_pipeline(&w::RenderPipelineDescriptor {
                        label: None,
                        layout: Some(&pipeline_layout),
                        vertex: w::VertexState {
                            module: &vertex,
                            entry_point: Some("main"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        primitive: Default::default(),
                        depth_stencil: to_depth.then(|| w::DepthStencilState {
                            format,
                            depth_write_enabled: Some(true),
                            depth_compare: Some(w::CompareFunction::Always),
                            stencil: Default::default(),
                            bias: Default::default(),
                        }),
                        multisample: Default::default(),
                        fragment: Some(w::FragmentState {
                            module: &module,
                            entry_point: Some("main"),
                            compilation_options: Default::default(),
                            targets: if to_depth {
                                &[]
                            } else {
                                std::slice::from_ref(&color_target)
                            },
                        }),
                        multiview_mask: None,
                        cache: None,
                    });
                (pipeline, layout)
            })
            .clone();
        let group = self.device.create_bind_group(&w::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[w::BindGroupEntry {
                binding: 0,
                resource: w::BindingResource::TextureView(source),
            }],
        });
        let color_attachment = Some(w::RenderPassColorAttachment {
            view: destination,
            depth_slice: slice,
            resolve_target: None,
            ops: w::Operations {
                load: w::LoadOp::Clear(w::Color::BLACK),
                store: w::StoreOp::Store,
            },
        });
        let mut pass = self
            .encoder(state)
            .begin_render_pass(&w::RenderPassDescriptor {
                label: None,
                color_attachments: if to_depth {
                    &[]
                } else {
                    std::slice::from_ref(&color_attachment)
                },
                depth_stencil_attachment: to_depth.then_some(w::RenderPassDepthStencilAttachment {
                    view: destination,
                    depth_ops: Some(w::Operations {
                        load: w::LoadOp::Clear(1.0),
                        store: w::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        let slice = slice.unwrap_or(0);
        pass.draw(0..3, slice..slice + 1);
    }

    ///
    /// Clears the part of the target inside the rectangle, and only the channels that have a value in the clear state.
    /// A clear of the whole target with all channels is a load operation of the pass instead, see [WgpuBackend::pass].
    ///
    pub(super) fn clear_partially(
        &self,
        state: &mut State,
        target: &PassTarget,
        rectangle: (u32, u32, u32, u32),
        clear_state: ClearState,
    ) {
        let formats = self.pass(state, target, None).formats.clone();
        let channels = [
            clear_state.red,
            clear_state.green,
            clear_state.blue,
            clear_state.alpha,
        ];
        let mask = channels
            .iter()
            .enumerate()
            .fold(0, |mask, (index, channel)| {
                mask | (u8::from(channel.is_some()) << index)
            });
        let write_depth = clear_state.depth.is_some() && formats.depth.is_some();
        if (mask == 0 || formats.colors.is_empty()) && !write_depth {
            return;
        }
        let (pipeline, layout) = state
            .builtin
            .clear_pipelines
            .entry((formats.clone(), mask, write_depth))
            .or_insert_with(|| {
                let layout = self
                    .device
                    .create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                        label: None,
                        entries: &[w::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: w::ShaderStages::FRAGMENT,
                            ty: w::BindingType::Buffer {
                                ty: w::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        }],
                    });
                let vertex = self.builtin_module(glsl::Stage::Vertex, VERTEX);
                let module =
                    self.builtin_module(glsl::Stage::Fragment, &clear_shader(formats.colors.len()));
                let pipeline_layout =
                    self.device
                        .create_pipeline_layout(&w::PipelineLayoutDescriptor {
                            label: None,
                            bind_group_layouts: &[Some(&layout)],
                            immediate_size: 0,
                        });
                let targets: Vec<_> = formats
                    .colors
                    .iter()
                    .map(|format| {
                        Some(w::ColorTargetState {
                            format: *format,
                            blend: None,
                            write_mask: w::ColorWrites::from_bits_truncate(mask as u32),
                        })
                    })
                    .collect();
                let pipeline = self
                    .device
                    .create_render_pipeline(&w::RenderPipelineDescriptor {
                        label: None,
                        layout: Some(&pipeline_layout),
                        vertex: w::VertexState {
                            module: &vertex,
                            entry_point: Some("main"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        primitive: Default::default(),
                        depth_stencil: formats.depth.map(|format| w::DepthStencilState {
                            format,
                            depth_write_enabled: Some(write_depth),
                            depth_compare: Some(w::CompareFunction::Always),
                            stencil: Default::default(),
                            bias: Default::default(),
                        }),
                        multisample: w::MultisampleState {
                            count: formats.samples,
                            ..Default::default()
                        },
                        fragment: Some(w::FragmentState {
                            module: &module,
                            entry_point: Some("main"),
                            compilation_options: Default::default(),
                            targets: &targets,
                        }),
                        multiview_mask: None,
                        cache: None,
                    });
                (pipeline, layout)
            })
            .clone();
        let color: Vec<u8> = channels
            .iter()
            .flat_map(|channel| channel.unwrap_or(0.0).to_le_bytes())
            .collect();
        let buffer = self.new_buffer(&color, w::BufferUsages::UNIFORM);
        let group = self.device.create_bind_group(&w::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[w::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let pass = &mut state.frame.pass.as_mut().unwrap().pass;
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        // The triangle is at depth 0 in the viewport, so it gets the depth the viewport starts at
        let depth = clear_state.depth.unwrap_or(0.0).clamp(0.0, 1.0);
        pass.set_viewport(
            0.0,
            0.0,
            target.width as f32,
            target.height as f32,
            depth,
            depth,
        );
        pass.set_scissor_rect(rectangle.0, rectangle.1, rectangle.2, rectangle.3);
        pass.draw(0..3, 0..1);
    }
}
