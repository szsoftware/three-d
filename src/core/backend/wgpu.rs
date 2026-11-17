//!
//! The wgpu backend (work in progress, only available with the `wgpu` feature).
//!
//! This is the first stage of the backend: a context without a window, buffers, 2D textures as render targets,
//! programs translated from GLSL (see [glsl]), draw calls and reading back colors. It is deliberately simple:
//! every clear and every draw call is its own render pass which is submitted immediately, so the calls take effect
//! in the order they are made, like on OpenGL. Batching passes, caching bind groups, multisampling, mipmaps,
//! the screen and the remaining formats follow in the next stages and panic with a message until then.
//!
//! Conventions: The backend keeps the conventions of OpenGL instead of changing them.
//! The vertex shader epilogue added by the translation flips the y axis and converts the depth range,
//! so the first row of a texture in memory is the bottom row and all coordinates (viewport, scissor box, `gl_FragCoord`, UVs)
//! mean the same as on OpenGL. Because of the flip, the front face is clockwise for wgpu.
//!

use super::*;
use ::wgpu as w;
use std::sync::Mutex;

pub(crate) mod glsl;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod shader_tests;

///
/// The wgpu backend, see the module documentation.
///
pub(crate) struct WgpuBackend {
    pub device: w::Device,
    pub queue: w::Queue,
    state: Mutex<State>,
}

// The backend only holds handles and plain data behind a mutex, a panic cannot leave it in a state that is unsafe to observe.
// Without these, enabling the `wgpu` feature would remove the traits from the public Context type.
impl std::panic::UnwindSafe for WgpuBackend {}
impl std::panic::RefUnwindSafe for WgpuBackend {}

#[derive(Default)]
struct State {
    next_id: u32,
    buffers: HashMap<u32, Option<w::Buffer>>,
    /// The content of the element buffers, kept because 8 bit indices do not exist in wgpu and are widened when they are drawn.
    element_bytes: HashMap<u32, Vec<u8>>,
    textures: HashMap<u32, TextureEntry>,
    programs: HashMap<u32, ProgramEntry>,
    /// The texture bound to each texture unit, as on OpenGL.
    units: HashMap<u32, u32>,
    /// The targets and scissor boxes of the passes that are open, innermost last.
    passes: Vec<(PassTarget, ScissorBox)>,
    pipelines: HashMap<String, w::RenderPipeline>,
}

struct TextureEntry {
    texture: w::Texture,
    format: TextureFormat,
    kind: TextureKind,
    sampler: w::Sampler,
    /// Used when the format cannot be filtered.
    nearest_sampler: w::Sampler,
}

/// Where a uniform lives inside one of the two uniform blocks of a program.
#[derive(Clone, Copy, Debug)]
struct Member {
    /// 0 is the block of the vertex shader, 1 the one of the fragment shader.
    block: usize,
    offset: usize,
    /// Distance in bytes between the columns of a matrix.
    column_stride: usize,
    /// Distance in bytes between the elements of an array.
    array_stride: usize,
}

enum UniformSlot {
    Value(Vec<Member>),
    /// Index into [ProgramEntry::samplers].
    Sampler(usize),
}

struct ProgramEntry {
    vertex: w::ShaderModule,
    fragment: w::ShaderModule,
    /// The content of the uniform blocks of the vertex and fragment shader, empty if the stage has no uniforms.
    blocks: [Vec<u8>; 2],
    uniforms: Vec<UniformSlot>,
    samplers: Vec<(glsl::SamplerDecl, u32)>,
    /// The buffer bound to each vertex attribute location.
    attributes: HashMap<u32, (u32, VertexFormat, u32)>,
    attribute_locations: Vec<u32>,
    uniform_layout: w::BindGroupLayout,
    number_of_outputs: usize,
}

fn unsupported(what: &str) -> ! {
    panic!("{what} is not supported by the wgpu backend yet")
}

impl WgpuBackend {
    pub fn new(device: w::Device, queue: w::Queue) -> Self {
        Self {
            device,
            queue,
            state: Mutex::new(State::default()),
        }
    }

    ///
    /// Creates a device without a window or surface.
    ///
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_headless() -> Result<Self, CoreError> {
        let instance = w::Instance::new(w::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&w::RequestAdapterOptions {
            power_preference: w::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|e| CoreError::ContextCreation(e.to_string()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&w::DeviceDescriptor {
            label: Some("three-d"),
            // Optional features are used when the adapter has them
            required_features: adapter.features() & w::Features::FLOAT32_FILTERABLE,
            ..Default::default()
        }))
        .map_err(|e| CoreError::ContextCreation(e.to_string()))?;
        Ok(Self::new(device, queue))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while the state was locked (for example about unsupported functionality) must not turn every later call into a panic
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn id(state: &mut State) -> u32 {
        state.next_id += 1;
        state.next_id
    }

    fn color_views(
        &self,
        state: &State,
        target: &PassTarget,
    ) -> Vec<(w::TextureView, w::TextureFormat)> {
        let view = |texture: TextureId, mip_level: u32, layer: u32| {
            let entry = &state.textures[&texture.wgpu()];
            (
                attachment_view(&entry.texture, mip_level, layer),
                entry.texture.format(),
            )
        };
        match &target.color {
            None => Vec::new(),
            Some(ColorAttachment::Texture2D { texture, mip_level }) => {
                vec![view(*texture, *mip_level, 0)]
            }
            Some(ColorAttachment::Texture2DArray {
                texture,
                layers,
                mip_level,
            }) => layers
                .iter()
                .map(|layer| view(*texture, *mip_level, *layer))
                .collect(),
            Some(ColorAttachment::CubeMap {
                texture,
                sides,
                mip_level,
            }) => sides
                .iter()
                .map(|side| view(*texture, *mip_level, cube_layer(*side)))
                .collect(),
            Some(ColorAttachment::Multisample(_)) => unsupported("a multisample render target"),
        }
    }

    fn depth_view(
        &self,
        state: &State,
        target: &PassTarget,
    ) -> Option<(w::TextureView, w::TextureFormat)> {
        let view = |texture: TextureId, layer: u32| {
            let entry = &state.textures[&texture.wgpu()];
            (
                attachment_view(&entry.texture, 0, layer),
                entry.texture.format(),
            )
        };
        match &target.depth {
            None => None,
            Some(DepthAttachment::Texture2D(texture)) => Some(view(*texture, 0)),
            Some(DepthAttachment::Texture2DArray { texture, layer }) => {
                Some(view(*texture, *layer))
            }
            Some(DepthAttachment::CubeMap { texture, side }) => {
                Some(view(*texture, cube_layer(*side)))
            }
            Some(DepthAttachment::Multisample(_)) => unsupported("a multisample render target"),
        }
    }

    /// Runs one render pass on the given target and submits it.
    fn run_pass(
        &self,
        state: &State,
        target: &PassTarget,
        clear: Option<ClearState>,
        render: impl FnOnce(&mut w::RenderPass, &[w::TextureFormat], Option<w::TextureFormat>),
    ) {
        if target.framebuffer.is_none() {
            unsupported("rendering to the screen");
        }
        let colors = self.color_views(state, target);
        let depth = self.depth_view(state, target);
        let color_load = match clear {
            Some(ClearState {
                red: Some(r),
                green: Some(g),
                blue: Some(b),
                alpha: Some(a),
                ..
            }) => w::LoadOp::Clear(w::Color {
                r: r as f64,
                g: g as f64,
                b: b as f64,
                a: a as f64,
            }),
            Some(ClearState {
                red: None,
                green: None,
                blue: None,
                alpha: None,
                ..
            })
            | None => w::LoadOp::Load,
            _ => unsupported("clearing only some of the color channels"),
        };
        let depth_load = match clear.and_then(|c| c.depth) {
            Some(depth) => w::LoadOp::Clear(depth),
            None => w::LoadOp::Load,
        };
        let color_attachments: Vec<_> = colors
            .iter()
            .map(|(view, _)| {
                Some(w::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: w::Operations {
                        load: color_load,
                        store: w::StoreOp::Store,
                    },
                })
            })
            .collect();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&w::RenderPassDescriptor {
                label: None,
                color_attachments: &color_attachments,
                depth_stencil_attachment: depth.as_ref().map(|(view, _)| {
                    w::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(w::Operations {
                            load: depth_load,
                            store: w::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let formats: Vec<_> = colors.iter().map(|(_, format)| *format).collect();
            render(
                &mut pass,
                &formats,
                depth.as_ref().map(|(_, format)| *format),
            );
        }
        self.queue.submit([encoder.finish()]);
    }

    fn new_buffer(&self, bytes: &[u8], usage: w::BufferUsages) -> w::Buffer {
        // The size of a buffer write must be a multiple of 4 bytes
        let size = bytes.len().next_multiple_of(4).max(4);
        let buffer = self.device.create_buffer(&w::BufferDescriptor {
            label: None,
            size: size as u64,
            usage: usage | w::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if bytes.len() == size {
            self.queue.write_buffer(&buffer, 0, bytes);
        } else {
            let mut padded = bytes.to_vec();
            padded.resize(size, 0);
            self.queue.write_buffer(&buffer, 0, &padded);
        }
        buffer
    }

    fn sampler(&self, desc: &SamplerDesc, nearest: bool) -> w::Sampler {
        let filter = |interpolation: Interpolation| match interpolation {
            Interpolation::Nearest => w::FilterMode::Nearest,
            _ if nearest => w::FilterMode::Nearest,
            _ => w::FilterMode::Linear,
        };
        let address = |wrapping: Wrapping| match wrapping {
            Wrapping::Repeat => w::AddressMode::Repeat,
            Wrapping::MirroredRepeat => w::AddressMode::MirrorRepeat,
            Wrapping::ClampToEdge => w::AddressMode::ClampToEdge,
        };
        self.device.create_sampler(&w::SamplerDescriptor {
            address_mode_u: address(desc.wrap_s),
            address_mode_v: address(desc.wrap_t),
            address_mode_w: address(desc.wrap_r.unwrap_or(desc.wrap_s)),
            mag_filter: filter(desc.mag_filter),
            min_filter: filter(desc.min_filter),
            ..Default::default()
        })
    }
}

fn attachment_view(texture: &w::Texture, mip_level: u32, layer: u32) -> w::TextureView {
    texture.create_view(&w::TextureViewDescriptor {
        dimension: Some(w::TextureViewDimension::D2),
        base_mip_level: mip_level,
        mip_level_count: Some(1),
        base_array_layer: layer,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

fn cube_layer(side: CubeMapSide) -> u32 {
    match side {
        CubeMapSide::Right => 0,
        CubeMapSide::Left => 1,
        CubeMapSide::Top => 2,
        CubeMapSide::Bottom => 3,
        CubeMapSide::Front => 4,
        CubeMapSide::Back => 5,
    }
}

/// The wgpu format and the number of bytes per texel. Three channels do not exist in wgpu, they are stored with four.
fn texture_format(format: TextureFormat) -> (w::TextureFormat, usize) {
    use w::TextureFormat as F;
    match format {
        TextureFormat::Color(ColorFormat {
            primitive,
            channels,
        }) => match (primitive, channels) {
            (PrimitiveType::U8, 1) => (F::R8Unorm, 1),
            (PrimitiveType::U8, 2) => (F::Rg8Unorm, 2),
            (PrimitiveType::U8, 3 | 4) => (F::Rgba8Unorm, 4),
            (PrimitiveType::F16, 1) => (F::R16Float, 2),
            (PrimitiveType::F16, 2) => (F::Rg16Float, 4),
            (PrimitiveType::F16, 3 | 4) => (F::Rgba16Float, 8),
            (PrimitiveType::F32, 1) => (F::R32Float, 4),
            (PrimitiveType::F32, 2) => (F::Rg32Float, 8),
            (PrimitiveType::F32, 3 | 4) => (F::Rgba32Float, 16),
            _ => unsupported(&format!(
                "a texture with {channels} channels of type {primitive:?}"
            )),
        },
        TextureFormat::Depth(DepthFormat::Depth16) => (F::Depth16Unorm, 2),
        TextureFormat::Depth(DepthFormat::Depth24) => (F::Depth24Plus, 4),
        TextureFormat::Depth(DepthFormat::Depth32F) => (F::Depth32Float, 4),
    }
}

fn primitive_size(primitive: PrimitiveType) -> usize {
    match primitive {
        PrimitiveType::U8 | PrimitiveType::I8 => 1,
        PrimitiveType::U16 | PrimitiveType::I16 | PrimitiveType::F16 => 2,
        PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => 4,
    }
}

/// The value of the alpha channel that stands for "opaque", used when a format without alpha is widened.
fn opaque(primitive: PrimitiveType) -> Vec<u8> {
    match primitive {
        PrimitiveType::U8 => vec![255],
        PrimitiveType::F16 => f16::from_f32(1.0).to_le_bytes().to_vec(),
        PrimitiveType::F32 => 1.0f32.to_le_bytes().to_vec(),
        _ => vec![0; primitive_size(primitive)],
    }
}

/// Converts tightly packed texels from one number of channels to another.
fn convert_channels(bytes: &[u8], primitive: PrimitiveType, from: usize, to: usize) -> Vec<u8> {
    if from == to {
        return bytes.to_vec();
    }
    let size = primitive_size(primitive);
    let opaque = opaque(primitive);
    let mut output = Vec::with_capacity(bytes.len() / from * to);
    for texel in bytes.chunks_exact(from * size) {
        for channel in 0..to {
            if channel < from {
                output.extend_from_slice(&texel[channel * size..(channel + 1) * size]);
            } else if channel == 3 {
                output.extend_from_slice(&opaque);
            } else {
                output.extend(std::iter::repeat_n(0, size));
            }
        }
    }
    output
}

fn vertex_format(format: VertexFormat) -> w::VertexFormat {
    use w::VertexFormat as F;
    match (format.primitive, format.components, format.normalized) {
        (PrimitiveType::F32, 1, _) => F::Float32,
        (PrimitiveType::F32, 2, _) => F::Float32x2,
        (PrimitiveType::F32, 3, _) => F::Float32x3,
        (PrimitiveType::F32, 4, _) => F::Float32x4,
        (PrimitiveType::F16, 1, _) => F::Float16,
        (PrimitiveType::F16, 2, _) => F::Float16x2,
        (PrimitiveType::F16, 4, _) => F::Float16x4,
        (PrimitiveType::U8, 1, true) => F::Unorm8,
        (PrimitiveType::U8, 2, true) => F::Unorm8x2,
        (PrimitiveType::U8, 4, true) => F::Unorm8x4,
        (PrimitiveType::U8, 1, false) => F::Uint8,
        (PrimitiveType::U8, 2, false) => F::Uint8x2,
        (PrimitiveType::U8, 4, false) => F::Uint8x4,
        (PrimitiveType::U32, 1, _) => F::Uint32,
        (PrimitiveType::U32, 2, _) => F::Uint32x2,
        (PrimitiveType::U32, 3, _) => F::Uint32x3,
        (PrimitiveType::U32, 4, _) => F::Uint32x4,
        (PrimitiveType::I32, 1, _) => F::Sint32,
        (PrimitiveType::I32, 2, _) => F::Sint32x2,
        (PrimitiveType::I32, 3, _) => F::Sint32x3,
        (PrimitiveType::I32, 4, _) => F::Sint32x4,
        _ => unsupported(&format!("the vertex format {format:?}")),
    }
}

fn blend_factor(multiplier: BlendMultiplierType) -> w::BlendFactor {
    match multiplier {
        BlendMultiplierType::Zero => w::BlendFactor::Zero,
        BlendMultiplierType::One => w::BlendFactor::One,
        BlendMultiplierType::SrcColor => w::BlendFactor::Src,
        BlendMultiplierType::OneMinusSrcColor => w::BlendFactor::OneMinusSrc,
        BlendMultiplierType::DstColor => w::BlendFactor::Dst,
        BlendMultiplierType::OneMinusDstColor => w::BlendFactor::OneMinusDst,
        BlendMultiplierType::SrcAlpha => w::BlendFactor::SrcAlpha,
        BlendMultiplierType::OneMinusSrcAlpha => w::BlendFactor::OneMinusSrcAlpha,
        BlendMultiplierType::DstAlpha => w::BlendFactor::DstAlpha,
        BlendMultiplierType::OneMinusDstAlpha => w::BlendFactor::OneMinusDstAlpha,
        BlendMultiplierType::SrcAlphaSaturate => w::BlendFactor::SrcAlphaSaturated,
    }
}

fn blend_component(
    source: BlendMultiplierType,
    destination: BlendMultiplierType,
    equation: BlendEquationType,
) -> w::BlendComponent {
    let operation = match equation {
        BlendEquationType::Add => w::BlendOperation::Add,
        BlendEquationType::Subtract => w::BlendOperation::Subtract,
        BlendEquationType::ReverseSubtract => w::BlendOperation::ReverseSubtract,
        BlendEquationType::Max => w::BlendOperation::Max,
        BlendEquationType::Min => w::BlendOperation::Min,
    };
    // The factors are ignored for min and max on OpenGL and must be one on wgpu
    let ignore_factors = matches!(equation, BlendEquationType::Max | BlendEquationType::Min);
    w::BlendComponent {
        src_factor: if ignore_factors {
            w::BlendFactor::One
        } else {
            blend_factor(source)
        },
        dst_factor: if ignore_factors {
            w::BlendFactor::One
        } else {
            blend_factor(destination)
        },
        operation,
    }
}

fn shader_error(stage: &str, source: &str, log: String) -> CoreError {
    let lines: Vec<String> = source
        .lines()
        .enumerate()
        .map(|(index, l)| format!("{:0>3}: {}", index + 1, l))
        .collect();
    CoreError::ShaderCompilation(stage.to_string(), lines.join("\n"), log)
}

///
/// Parses and validates a translated shader with naga.
///
pub(crate) fn parse_shader(
    stage: glsl::Stage,
    source: &str,
) -> Result<(naga::Module, naga::valid::ModuleInfo), String> {
    let naga_stage = match stage {
        glsl::Stage::Vertex => naga::ShaderStage::Vertex,
        glsl::Stage::Fragment => naga::ShaderStage::Fragment,
    };
    let module = naga::front::glsl::Frontend::default()
        .parse(&naga::front::glsl::Options::from(naga_stage), source)
        .map_err(|e| e.emit_to_string(source))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .map_err(|e| e.emit_to_string(source))?;
    Ok((module, info))
}

/// The members of the uniform block generated for the loose uniforms of one stage, and the size of the block in bytes.
fn block_members(module: &naga::Module, block: usize) -> (HashMap<String, Member>, usize) {
    let mut members = HashMap::new();
    let mut size = 0;
    for (_, variable) in module.global_variables.iter() {
        let is_block = variable
            .binding
            .as_ref()
            .is_some_and(|b| b.group == glsl::UNIFORM_GROUP && b.binding == block as u32);
        if !is_block {
            continue;
        }
        if let naga::TypeInner::Struct {
            members: struct_members,
            span,
        } = &module.types[variable.ty].inner
        {
            size = *span as usize;
            for member in struct_members {
                let mut inner = &module.types[member.ty].inner;
                let mut array_stride = 0;
                if let naga::TypeInner::Array { base, stride, .. } = inner {
                    array_stride = *stride as usize;
                    inner = &module.types[*base].inner;
                }
                let column_stride = match inner {
                    naga::TypeInner::Matrix { rows, scalar, .. } => {
                        let rows = if *rows == naga::VectorSize::Bi { 2 } else { 4 };
                        rows * scalar.width as usize
                    }
                    _ => 0,
                };
                if let Some(name) = &member.name {
                    members.insert(
                        name.clone(),
                        Member {
                            block,
                            offset: member.offset as usize,
                            column_stride,
                            array_stride,
                        },
                    );
                }
            }
        }
    }
    (members, size)
}

/// Number of components per column and number of columns.
fn uniform_shape(ty: UniformType) -> (usize, usize) {
    match ty {
        UniformType::Value => (1, 1),
        UniformType::Vec2 => (2, 1),
        UniformType::Vec3 => (3, 1),
        UniformType::Vec4 => (4, 1),
        UniformType::Mat2 => (2, 2),
        UniformType::Mat3 => (3, 3),
        UniformType::Mat4 => (4, 4),
    }
}

fn texture_view_dimension(glsl_type: &str) -> w::TextureViewDimension {
    if glsl_type.contains("2DArray") {
        w::TextureViewDimension::D2Array
    } else if glsl_type.contains("Cube") {
        w::TextureViewDimension::Cube
    } else if glsl_type.contains("3D") {
        w::TextureViewDimension::D3
    } else {
        w::TextureViewDimension::D2
    }
}

impl BackendApi for WgpuBackend {
    // The render states are part of every draw call, the global state of OpenGL does not exist here.
    fn set_viewport(&self, _viewport: Viewport) {}
    fn set_scissor(&self, _scissor_box: ScissorBox) {}
    fn set_cull(&self, _cull: Cull) {}
    fn set_write_mask(&self, _write_mask: WriteMask) {}
    fn set_depth_test(&self, _depth_test: DepthTest) {}
    fn set_blend(&self, _blend: Blend) {}
    fn set_render_states(&self, _render_states: RenderStates) {}
    fn error_check(&self) -> Result<(), CoreError> {
        // wgpu reports validation errors through its error handler, which panics with the message by default
        Ok(())
    }
    fn max_samples(&self) -> u32 {
        4
    }

    fn create_buffer(&self) -> BufferId {
        let mut state = self.lock();
        let id = Self::id(&mut state);
        state.buffers.insert(id, None);
        BufferId::Wgpu(id)
    }

    fn delete_buffer(&self, buffer: BufferId) {
        let mut state = self.lock();
        state.buffers.remove(&buffer.wgpu());
        state.element_bytes.remove(&buffer.wgpu());
    }

    fn write_buffer(&self, buffer: BufferId, kind: BufferKind, bytes: &[u8], _usage: BufferUsage) {
        let usage = match kind {
            BufferKind::Vertex => w::BufferUsages::VERTEX,
            BufferKind::Element => w::BufferUsages::INDEX,
            BufferKind::Uniform => w::BufferUsages::UNIFORM,
        };
        let new_buffer = self.new_buffer(bytes, usage);
        let mut state = self.lock();
        state.buffers.insert(buffer.wgpu(), Some(new_buffer));
        if kind == BufferKind::Element {
            state.element_bytes.insert(buffer.wgpu(), bytes.to_vec());
        }
    }

    fn write_buffer_subset(&self, buffer: BufferId, _kind: BufferKind, offset: u32, bytes: &[u8]) {
        let mut state = self.lock();
        if let Some(content) = state.element_bytes.get_mut(&buffer.wgpu()) {
            if let Some(part) = content.get_mut(offset as usize..offset as usize + bytes.len()) {
                part.copy_from_slice(bytes);
            }
        }
        let buffer = state.buffers[&buffer.wgpu()]
            .as_ref()
            .expect("a subset of a buffer is written before the buffer is filled");
        if !bytes.len().is_multiple_of(4) || !offset.is_multiple_of(4) {
            unsupported("writing a part of a buffer that is not aligned to 4 bytes");
        }
        self.queue.write_buffer(buffer, offset as u64, bytes);
    }

    fn create_texture(&self, desc: &TextureDesc) -> TextureId {
        let Some(storage) = desc.storage else {
            unsupported("a texture without storage")
        };
        let (format, _) = texture_format(storage.format);
        let (dimension, layers) = match desc.kind {
            TextureKind::D2 => (w::TextureDimension::D2, 1),
            TextureKind::D2Array => (w::TextureDimension::D2, storage.depth),
            TextureKind::D3 => (w::TextureDimension::D3, storage.depth),
            TextureKind::CubeMap => (w::TextureDimension::D2, 6),
        };
        let mut usage = w::TextureUsages::TEXTURE_BINDING
            | w::TextureUsages::COPY_DST
            | w::TextureUsages::COPY_SRC;
        if dimension == w::TextureDimension::D2 {
            usage |= w::TextureUsages::RENDER_ATTACHMENT;
        }
        let texture = self.device.create_texture(&w::TextureDescriptor {
            label: None,
            size: w::Extent3d {
                width: storage.width,
                height: storage.height,
                depth_or_array_layers: layers,
            },
            // Mipmaps are generated in a later stage of the backend, until then only the base level exists
            mip_level_count: 1,
            sample_count: 1,
            dimension,
            format,
            usage,
            view_formats: &[],
        });
        let entry = TextureEntry {
            texture,
            format: storage.format,
            kind: desc.kind,
            sampler: self.sampler(&desc.sampler, false),
            nearest_sampler: self.sampler(&desc.sampler, true),
        };
        let mut state = self.lock();
        let id = Self::id(&mut state);
        state.textures.insert(id, entry);
        TextureId::Wgpu(id)
    }

    fn delete_texture(&self, texture: TextureId) {
        self.lock().textures.remove(&texture.wgpu());
    }

    fn write_texture(
        &self,
        texture: TextureId,
        _kind: TextureKind,
        region: TextureRegion,
        width: u32,
        height: u32,
        depth: u32,
        format: ColorFormat,
        bytes: &[u8],
    ) {
        let state = self.lock();
        let entry = &state.textures[&texture.wgpu()];
        let TextureFormat::Color(stored) = entry.format else {
            unsupported("writing to a depth texture")
        };
        if stored.primitive != format.primitive {
            unsupported("writing texture data of another type than the texture has");
        }
        let (_, texel_size) = texture_format(entry.format);
        let stored_channels = texel_size / primitive_size(stored.primitive);
        let bytes = convert_channels(
            bytes,
            format.primitive,
            format.channels as usize,
            stored_channels,
        );
        let (layer, layers) = match region {
            TextureRegion::All => (0, depth.max(1)),
            TextureRegion::Layer(layer) => (layer, 1),
            TextureRegion::Side(side) => (cube_layer(side), 1),
        };
        // The first row of the data is the bottom row, which is the first row in memory in this backend as well
        self.queue.write_texture(
            w::TexelCopyTextureInfo {
                texture: &entry.texture,
                mip_level: 0,
                origin: w::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: w::TextureAspect::All,
            },
            &bytes,
            w::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * texel_size as u32),
                rows_per_image: Some(height),
            },
            w::Extent3d {
                width,
                height,
                depth_or_array_layers: layers,
            },
        );
    }

    fn generate_mipmaps(&self, _texture: TextureId, _kind: TextureKind) {
        // Textures only have the base level for now, see create_texture
    }

    fn create_renderbuffer(
        &self,
        _format: TextureFormat,
        _width: u32,
        _height: u32,
        _number_of_samples: u32,
    ) -> RenderbufferId {
        unsupported("a multisample render target")
    }

    fn delete_renderbuffer(&self, _renderbuffer: RenderbufferId) {}

    fn create_framebuffer(&self) -> FramebufferId {
        // A render target is fully described by its attachments, the id only tells it apart from the screen
        FramebufferId::Wgpu(Self::id(&mut self.lock()))
    }

    fn delete_framebuffer(&self, _framebuffer: FramebufferId) {}

    fn begin_pass(&self, target: &PassTarget, scissor_box: ScissorBox, load: LoadOp) {
        match load {
            LoadOp::Load => {}
        }
        self.lock().passes.push((target.clone(), scissor_box));
    }

    fn end_pass(&self) {
        self.lock().passes.pop();
    }

    fn clear(&self, target: &PassTarget, scissor_box: ScissorBox, clear_state: ClearState) {
        if scissor_box.x > 0
            || scissor_box.y > 0
            || scissor_box.x + (scissor_box.width as i32) < target.width as i32
            || scissor_box.y + (scissor_box.height as i32) < target.height as i32
        {
            unsupported("clearing a part of a render target");
        }
        let state = self.lock();
        self.run_pass(&state, target, Some(clear_state), |_, _, _| {});
    }

    fn copy_target(&self, _source: &PassTarget, _destination: &PassTarget, _mask: CopyMask) {
        unsupported("copying between render targets")
    }

    fn read_pixels(&self, source: &PassTarget, rect: ScissorBox, format: ReadFormat) -> Vec<u8> {
        if cfg!(target_arch = "wasm32") {
            // A browser cannot block until the pixels are there, it needs the asynchronous read functions which follow in a later stage
            unsupported("reading pixels synchronously in the browser");
        }
        let ReadFormat::Color(format) = format else {
            unsupported("reading depth")
        };
        let Some(ColorAttachment::Texture2D { texture, mip_level }) = &source.color else {
            unsupported("reading from anything but a 2D color texture")
        };
        let state = self.lock();
        let entry = &state.textures[&texture.wgpu()];
        let TextureFormat::Color(stored) = entry.format else {
            unreachable!()
        };
        if stored.primitive != format.primitive {
            unsupported("reading pixels as another type than the texture has");
        }
        let (_, texel_size) = texture_format(entry.format);
        let unpadded = rect.width as usize * texel_size;
        let padded = unpadded.next_multiple_of(w::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let buffer = self.device.create_buffer(&w::BufferDescriptor {
            label: None,
            size: (padded * rect.height as usize) as u64,
            usage: w::BufferUsages::COPY_DST | w::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            w::TexelCopyTextureInfo {
                texture: &entry.texture,
                mip_level: *mip_level,
                origin: w::Origin3d {
                    x: rect.x as u32,
                    y: rect.y as u32,
                    z: 0,
                },
                aspect: w::TextureAspect::All,
            },
            w::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: w::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(rect.height),
                },
            },
            w::Extent3d {
                width: rect.width,
                height: rect.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(w::MapMode::Read, |result| {
            result.expect("failed to map the buffer for reading pixels")
        });
        // Blocks until the buffer is mapped
        self.device
            .poll(w::PollType::wait_indefinitely())
            .expect("failed to wait for the GPU");
        let data = slice.get_mapped_range();
        let mut tight = Vec::with_capacity(unpadded * rect.height as usize);
        for row in data.chunks_exact(padded) {
            tight.extend_from_slice(&row[..unpadded]);
        }
        drop(data);
        buffer.unmap();
        convert_channels(
            &tight,
            format.primitive,
            texel_size / primitive_size(stored.primitive),
            format.channels as usize,
        )
    }

    fn create_program(
        &self,
        vertex_shader_source: &str,
        fragment_shader_source: &str,
    ) -> Result<(ProgramId, ProgramReflection), CoreError> {
        let translation =
            glsl::translate(vertex_shader_source, fragment_shader_source).map_err(|e| {
                CoreError::ShaderCompilation(
                    "vertex or fragment".to_string(),
                    format!("{vertex_shader_source}\n\n{fragment_shader_source}"),
                    e,
                )
            })?;
        let (vertex_module, _) = parse_shader(glsl::Stage::Vertex, &translation.vertex)
            .map_err(|e| shader_error("vertex", &translation.vertex, e))?;
        let (fragment_module, _) = parse_shader(glsl::Stage::Fragment, &translation.fragment)
            .map_err(|e| shader_error("fragment", &translation.fragment, e))?;

        let (vertex_members, vertex_size) = block_members(&vertex_module, 0);
        let (fragment_members, fragment_size) = block_members(&fragment_module, 1);
        let number_of_outputs = fragment_module
            .entry_points
            .first()
            .and_then(|entry| entry.function.result.as_ref())
            .map(|result| match &fragment_module.types[result.ty].inner {
                naga::TypeInner::Struct { members, .. } => members
                    .iter()
                    .filter(|m| matches!(m.binding, Some(naga::Binding::Location { .. })))
                    .count(),
                _ => 1,
            })
            .unwrap_or(0);

        let mut reflection = ProgramReflection::default();
        let mut uniforms = Vec::new();
        let mut names: Vec<&String> = translation
            .vertex_uniforms
            .iter()
            .chain(&translation.fragment_uniforms)
            .collect();
        names.dedup();
        for name in names {
            if reflection.uniforms.contains_key(name) {
                continue;
            }
            let members = [vertex_members.get(name), fragment_members.get(name)]
                .into_iter()
                .flatten()
                .copied()
                .collect();
            reflection
                .uniforms
                .insert(name.clone(), UniformLocation::Wgpu(uniforms.len() as u32));
            uniforms.push(UniformSlot::Value(members));
        }
        for (index, sampler) in translation.samplers.iter().enumerate() {
            reflection.uniforms.insert(
                sampler.name.clone(),
                UniformLocation::Wgpu(uniforms.len() as u32),
            );
            uniforms.push(UniformSlot::Sampler(index));
        }
        reflection.attributes = translation.attributes.iter().cloned().collect();

        let block_entry = |binding: u32, visibility: w::ShaderStages| w::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: w::BindingType::Buffer {
                ty: w::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mut entries = Vec::new();
        if vertex_size > 0 {
            entries.push(block_entry(0, w::ShaderStages::VERTEX));
        }
        if fragment_size > 0 {
            entries.push(block_entry(1, w::ShaderStages::FRAGMENT));
        }
        let uniform_layout = self
            .device
            .create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                label: None,
                entries: &entries,
            });

        let module = |module: naga::Module| {
            self.device.create_shader_module(w::ShaderModuleDescriptor {
                label: None,
                source: w::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
            })
        };
        let entry = ProgramEntry {
            vertex: module(vertex_module),
            fragment: module(fragment_module),
            blocks: [vec![0; vertex_size], vec![0; fragment_size]],
            uniforms,
            samplers: translation.samplers.into_iter().map(|s| (s, 0)).collect(),
            attributes: HashMap::new(),
            attribute_locations: translation.attributes.iter().map(|(_, l)| *l).collect(),
            uniform_layout,
            number_of_outputs,
        };
        let mut state = self.lock();
        let id = Self::id(&mut state);
        state.programs.insert(id, entry);
        Ok((ProgramId::Wgpu(id), reflection))
    }

    fn delete_program(&self, program: ProgramId) {
        let mut state = self.lock();
        state.programs.remove(&program.wgpu());
        let prefix = format!("{}|", program.wgpu());
        state.pipelines.retain(|key, _| !key.starts_with(&prefix));
    }

    fn set_uniform(&self, program: ProgramId, location: &UniformLocation, value: &UniformValue) {
        let mut state = self.lock();
        let program = state.programs.get_mut(&program.wgpu()).unwrap();
        let (ty, bytes): (UniformType, Vec<u8>) = match value {
            UniformValue::F32(ty, data) => {
                (*ty, data.iter().flat_map(|v| v.to_le_bytes()).collect())
            }
            UniformValue::I32(ty, data) => {
                (*ty, data.iter().flat_map(|v| v.to_le_bytes()).collect())
            }
            UniformValue::U32(ty, data) => {
                (*ty, data.iter().flat_map(|v| v.to_le_bytes()).collect())
            }
        };
        match &program.uniforms[location.wgpu() as usize] {
            UniformSlot::Sampler(index) => {
                let unit = match value {
                    UniformValue::I32(_, data) => data[0] as u32,
                    _ => panic!("a texture uniform must be set to the index of a texture unit"),
                };
                program.samplers[*index].1 = unit;
            }
            UniformSlot::Value(members) => {
                let (rows, columns) = uniform_shape(ty);
                let column_size = rows * 4;
                let element_size = column_size * columns;
                for member in members.clone() {
                    let block = &mut program.blocks[member.block];
                    for (element, data) in bytes.chunks_exact(element_size).enumerate() {
                        for (column, data) in data.chunks_exact(column_size).enumerate() {
                            let start = member.offset
                                + element * member.array_stride
                                + column * member.column_stride;
                            if let Some(destination) = block.get_mut(start..start + column_size) {
                                destination.copy_from_slice(data);
                            }
                        }
                    }
                }
            }
        }
    }

    fn bind_texture(&self, unit: u32, texture: TextureId, _kind: TextureKind) {
        self.lock().units.insert(unit, texture.wgpu());
    }

    fn uniform_block_index(&self, _program: ProgramId, _name: &str) -> Option<u32> {
        unsupported("a uniform block")
    }

    fn bind_uniform_block(
        &self,
        _program: ProgramId,
        _block_index: u32,
        _binding: u32,
        _buffer: BufferId,
    ) {
        unsupported("a uniform block")
    }

    fn bind_vertex_attribute(
        &self,
        program: ProgramId,
        location: u32,
        buffer: BufferId,
        format: VertexFormat,
        divisor: u32,
    ) {
        self.lock()
            .programs
            .get_mut(&program.wgpu())
            .unwrap()
            .attributes
            .insert(location, (buffer.wgpu(), format, divisor));
    }

    fn prepare_attributeless_draw(&self) {}

    fn draw(&self, call: &DrawCall) {
        if call.render_states.cull == Cull::FrontAndBack {
            // Only triangles are drawn, so nothing is visible
            return;
        }
        let mut state = self.lock();
        let (target, scissor_box) = state
            .passes
            .last()
            .cloned()
            .expect("a draw call must be made inside the write method of a render target");
        let program_id = call.program.wgpu();
        let program = &state.programs[&program_id];

        // Vertex buffers, one per attribute
        let mut locations = program.attribute_locations.clone();
        locations.sort();
        let mut vertex_buffers = Vec::new();
        let mut vertex_attributes = Vec::new();
        for location in &locations {
            let (buffer, format, divisor) = program.attributes.get(location).unwrap_or_else(|| {
                panic!("no buffer is bound to the vertex attribute at location {location}")
            });
            let buffer = state.buffers[buffer]
                .clone()
                .expect("a vertex buffer is used before it is filled");
            let format = vertex_format(*format);
            vertex_buffers.push((buffer, format.size(), *divisor));
            vertex_attributes.push([w::VertexAttribute {
                format,
                offset: 0,
                shader_location: *location,
            }]);
        }
        let vertex_layouts: Vec<_> = vertex_buffers
            .iter()
            .zip(&vertex_attributes)
            .map(|((_, stride, divisor), attributes)| w::VertexBufferLayout {
                array_stride: *stride,
                step_mode: if *divisor == 0 {
                    w::VertexStepMode::Vertex
                } else {
                    w::VertexStepMode::Instance
                },
                attributes,
            })
            .collect();

        // Textures and samplers
        let float32_filterable = self
            .device
            .features()
            .contains(w::Features::FLOAT32_FILTERABLE);
        let mut texture_layout_entries = Vec::new();
        let mut texture_views = Vec::new();
        for (index, (sampler, unit)) in program.samplers.iter().enumerate() {
            let entry = state
                .units
                .get(unit)
                .and_then(|texture| state.textures.get(texture))
                .unwrap_or_else(|| panic!("no texture is bound for the uniform {}", sampler.name));
            let (sample_type, filtering) = match entry.format {
                TextureFormat::Depth(_) => (w::TextureSampleType::Depth, false),
                TextureFormat::Color(ColorFormat {
                    primitive: PrimitiveType::F32,
                    ..
                }) if !float32_filterable => {
                    (w::TextureSampleType::Float { filterable: false }, false)
                }
                _ => (w::TextureSampleType::Float { filterable: true }, true),
            };
            let mut visibility = w::ShaderStages::empty();
            if sampler.in_vertex {
                visibility |= w::ShaderStages::VERTEX;
            }
            if sampler.in_fragment {
                visibility |= w::ShaderStages::FRAGMENT;
            }
            let view_dimension = texture_view_dimension(&sampler.glsl_type);
            texture_layout_entries.push(w::BindGroupLayoutEntry {
                binding: 2 * index as u32,
                visibility,
                ty: w::BindingType::Texture {
                    sample_type,
                    view_dimension,
                    multisampled: false,
                },
                count: None,
            });
            texture_layout_entries.push(w::BindGroupLayoutEntry {
                binding: 2 * index as u32 + 1,
                visibility,
                ty: w::BindingType::Sampler(if filtering {
                    w::SamplerBindingType::Filtering
                } else {
                    w::SamplerBindingType::NonFiltering
                }),
                count: None,
            });
            let view = entry.texture.create_view(&w::TextureViewDescriptor {
                dimension: Some(view_dimension),
                ..Default::default()
            });
            let _ = entry.kind;
            texture_views.push((
                view,
                if filtering {
                    entry.sampler.clone()
                } else {
                    entry.nearest_sampler.clone()
                },
            ));
        }
        let texture_layout = self
            .device
            .create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                label: None,
                entries: &texture_layout_entries,
            });
        let texture_entries: Vec<_> = texture_views
            .iter()
            .enumerate()
            .flat_map(|(index, (view, sampler))| {
                [
                    w::BindGroupEntry {
                        binding: 2 * index as u32,
                        resource: w::BindingResource::TextureView(view),
                    },
                    w::BindGroupEntry {
                        binding: 2 * index as u32 + 1,
                        resource: w::BindingResource::Sampler(sampler),
                    },
                ]
            })
            .collect();
        let texture_group = self.device.create_bind_group(&w::BindGroupDescriptor {
            label: None,
            layout: &texture_layout,
            entries: &texture_entries,
        });

        // Uniforms
        let uniform_buffers: Vec<_> = program
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, bytes)| !bytes.is_empty())
            .map(|(binding, bytes)| {
                (
                    binding as u32,
                    self.new_buffer(bytes, w::BufferUsages::UNIFORM),
                )
            })
            .collect();
        let uniform_entries: Vec<_> = uniform_buffers
            .iter()
            .map(|(binding, buffer)| w::BindGroupEntry {
                binding: *binding,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        let uniform_group = self.device.create_bind_group(&w::BindGroupDescriptor {
            label: None,
            layout: &program.uniform_layout,
            entries: &uniform_entries,
        });

        let index_buffer = call.indices.map(|indices| {
            let id = indices.buffer.wgpu();
            match indices.index_type {
                PrimitiveType::U8 => {
                    // 8 bit indices do not exist in wgpu
                    let wide: Vec<u8> = state.element_bytes[&id]
                        .iter()
                        .flat_map(|index| (*index as u16).to_le_bytes())
                        .collect();
                    (
                        self.new_buffer(&wide, w::BufferUsages::INDEX),
                        w::IndexFormat::Uint16,
                        indices.first,
                    )
                }
                index_type => (
                    state.buffers[&id]
                        .clone()
                        .expect("an element buffer is used before it is filled"),
                    if index_type == PrimitiveType::U16 {
                        w::IndexFormat::Uint16
                    } else {
                        w::IndexFormat::Uint32
                    },
                    indices.first,
                ),
            }
        });

        let color_formats: Vec<_> = self
            .color_views(&state, &target)
            .into_iter()
            .map(|(_, format)| format)
            .collect();
        let depth_format = self.depth_view(&state, &target).map(|(_, format)| format);
        // The debug representation of everything that goes into the pipeline is used as key, a proper key follows with the pipeline cache
        let key = format!(
            "{program_id}|{:?}|{color_formats:?}|{depth_format:?}|{vertex_layouts:?}|{texture_layout_entries:?}",
            call.render_states
        );
        if !state.pipelines.contains_key(&key) {
            let program = &state.programs[&program_id];
            let layout = self
                .device
                .create_pipeline_layout(&w::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[Some(&program.uniform_layout), Some(&texture_layout)],
                    immediate_size: 0,
                });
            let write_mask = call.render_states.write_mask;
            let mut color_writes = w::ColorWrites::empty();
            color_writes.set(w::ColorWrites::RED, write_mask.red);
            color_writes.set(w::ColorWrites::GREEN, write_mask.green);
            color_writes.set(w::ColorWrites::BLUE, write_mask.blue);
            color_writes.set(w::ColorWrites::ALPHA, write_mask.alpha);
            let blend = match call.render_states.blend {
                Blend::Disabled => None,
                Blend::Enabled {
                    source_rgb_multiplier,
                    source_alpha_multiplier,
                    destination_rgb_multiplier,
                    destination_alpha_multiplier,
                    rgb_equation,
                    alpha_equation,
                } => Some(w::BlendState {
                    color: blend_component(
                        source_rgb_multiplier,
                        destination_rgb_multiplier,
                        rgb_equation,
                    ),
                    alpha: blend_component(
                        source_alpha_multiplier,
                        destination_alpha_multiplier,
                        alpha_equation,
                    ),
                }),
            };
            let targets: Vec<_> = color_formats
                .iter()
                .enumerate()
                .map(|(index, format)| {
                    Some(w::ColorTargetState {
                        format: *format,
                        blend,
                        // A target the fragment shader does not write to keeps its content
                        write_mask: if index < program.number_of_outputs {
                            color_writes
                        } else {
                            w::ColorWrites::empty()
                        },
                    })
                })
                .collect();
            let pipeline = self
                .device
                .create_render_pipeline(&w::RenderPipelineDescriptor {
                    label: None,
                    layout: Some(&layout),
                    vertex: w::VertexState {
                        module: &program.vertex,
                        entry_point: Some("main"),
                        compilation_options: Default::default(),
                        buffers: &vertex_layouts,
                    },
                    primitive: w::PrimitiveState {
                        // Clockwise because the vertex shader flips the y axis, see the module documentation
                        front_face: w::FrontFace::Cw,
                        cull_mode: match call.render_states.cull {
                            Cull::None | Cull::FrontAndBack => None,
                            Cull::Back => Some(w::Face::Back),
                            Cull::Front => Some(w::Face::Front),
                        },
                        ..Default::default()
                    },
                    depth_stencil: depth_format.map(|format| w::DepthStencilState {
                        format,
                        depth_write_enabled: Some(write_mask.depth),
                        depth_compare: Some(match call.render_states.depth_test {
                            DepthTest::Never => w::CompareFunction::Never,
                            DepthTest::Less => w::CompareFunction::Less,
                            DepthTest::Equal => w::CompareFunction::Equal,
                            DepthTest::LessOrEqual => w::CompareFunction::LessEqual,
                            DepthTest::Greater => w::CompareFunction::Greater,
                            DepthTest::NotEqual => w::CompareFunction::NotEqual,
                            DepthTest::GreaterOrEqual => w::CompareFunction::GreaterEqual,
                            DepthTest::Always => w::CompareFunction::Always,
                        }),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(w::FragmentState {
                        module: &program.fragment,
                        entry_point: Some("main"),
                        compilation_options: Default::default(),
                        targets: &targets,
                    }),
                    multiview_mask: None,
                    cache: None,
                });
            state.pipelines.insert(key.clone(), pipeline);
        }
        let pipeline = &state.pipelines[&key];

        // The part of the viewport and the scissor box that is inside the target
        let clamp = |x: i32, y: i32, width: u32, height: u32| {
            let x0 = x.clamp(0, target.width as i32);
            let y0 = y.clamp(0, target.height as i32);
            let x1 = (x + width as i32).clamp(x0, target.width as i32);
            let y1 = (y + height as i32).clamp(y0, target.height as i32);
            (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
        };
        let scissor = clamp(
            scissor_box.x,
            scissor_box.y,
            scissor_box.width,
            scissor_box.height,
        );
        if scissor.2 == 0 || scissor.3 == 0 || call.viewport.width == 0 || call.viewport.height == 0
        {
            return;
        }
        self.run_pass(&state, &target, None, |pass, _, _| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &uniform_group, &[]);
            pass.set_bind_group(1, &texture_group, &[]);
            pass.set_viewport(
                call.viewport.x as f32,
                call.viewport.y as f32,
                call.viewport.width as f32,
                call.viewport.height as f32,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
            for (slot, (buffer, _, _)) in vertex_buffers.iter().enumerate() {
                pass.set_vertex_buffer(slot as u32, buffer.slice(..));
            }
            let instances = 0..call.instances.unwrap_or(1);
            match &index_buffer {
                Some((buffer, format, first)) => {
                    pass.set_index_buffer(buffer.slice(..), *format);
                    pass.draw_indexed(*first..*first + call.count, 0, instances);
                }
                None => pass.draw(0..call.count, instances),
            }
        });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
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
                data: TextureData::RgbU8(vec![
                    [255, 0, 0],
                    [0, 255, 0],
                    [0, 0, 255],
                    [255, 255, 0],
                ]),
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
}
