//!
//! Crate-internal backend layer.
//!
//! All graphics API calls made by the [core](crate::core) module go through the [BackendApi] trait defined here.
//! The public types (Context, Program, buffers, textures and render targets) only hold backend handles
//! and describe what they need with the backend neutral types in this module.
//! The interface follows the stricter WebGPU object model (buffers, textures, programs, passes and draw calls)
//! so that other backends can be added later without changing the public API.
//!
//! Currently there is one real backend, [gl], which contains the OpenGL/WebGL code that used to live directly in the public types.
//! In tests there is an additional recording backend without a GPU, see `null`.
//!

use crate::core::*;
use std::collections::HashMap;

pub(crate) mod gl;
#[cfg(test)]
pub(crate) mod null;

///
/// The backend of a [Context]. Dispatch to the actual backend happens in [Backend::api].
///
pub(crate) enum Backend {
    Gl(gl::GlBackend),
    #[cfg(test)]
    Null(null::NullBackend),
}

impl Backend {
    pub fn api(&self) -> &dyn BackendApi {
        match self {
            Self::Gl(backend) => backend,
            #[cfg(test)]
            Self::Null(backend) => backend,
        }
    }
}

// HANDLES

macro_rules! backend_handle {
    ($name:ident, $gl:ty) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) enum $name {
            Gl($gl),
            #[cfg(test)]
            Null(u32),
        }

        #[allow(dead_code)]
        impl $name {
            pub fn gl(self) -> $gl {
                match self {
                    Self::Gl(id) => id,
                    #[cfg(test)]
                    Self::Null(_) => wrong_backend(),
                }
            }
        }
    };
}

backend_handle!(BufferId, crate::context::Buffer);
backend_handle!(TextureId, crate::context::Texture);
backend_handle!(RenderbufferId, crate::context::Renderbuffer);
backend_handle!(FramebufferId, crate::context::Framebuffer);
backend_handle!(ProgramId, crate::context::Program);

///
/// The location of a uniform variable inside a program, as reported by the backend when creating the program.
///
#[derive(Clone, Debug)]
pub(crate) enum UniformLocation {
    Gl(crate::context::UniformLocation),
    #[cfg(test)]
    Null(String),
}

#[allow(dead_code)]
impl UniformLocation {
    pub fn gl(&self) -> &crate::context::UniformLocation {
        match self {
            Self::Gl(location) => location,
            #[cfg(test)]
            Self::Null(_) => wrong_backend(),
        }
    }
}

#[cfg(test)]
fn wrong_backend() -> ! {
    panic!("a handle from one backend was used with another backend")
}

// DATA DESCRIPTIONS

///
/// The primitive type of one component of a value, for example the `f32` in a `Vec3`.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveType {
    U8,
    U16,
    U32,
    I8,
    I16,
    I32,
    F16,
    F32,
}

impl PrimitiveType {
    /// True for the integer types.
    pub fn is_integer(self) -> bool {
        !matches!(self, Self::F16 | Self::F32)
    }

    /// True for the floating point types.
    pub fn is_float(self) -> bool {
        matches!(self, Self::F16 | Self::F32)
    }
}

///
/// Format of the texels in a color texture or of the pixels read from a color target:
/// a primitive type and the number of channels (1 to 4).
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColorFormat {
    pub primitive: PrimitiveType,
    pub channels: u32,
}

impl ColorFormat {
    pub fn of<T: data_type::DataType>() -> Self {
        Self {
            primitive: T::primitive_type(),
            channels: T::size(),
        }
    }
}

///
/// Format of a depth texture.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthFormat {
    Depth16,
    Depth24,
    Depth32F,
}

///
/// Format of a texture or a render buffer.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextureFormat {
    Color(ColorFormat),
    Depth(DepthFormat),
}

///
/// Format of a vertex attribute.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VertexFormat {
    pub primitive: PrimitiveType,
    pub components: u32,
    pub normalized: bool,
}

impl VertexFormat {
    pub fn of<T: data_type::DataType>() -> Self {
        Self {
            primitive: T::primitive_type(),
            components: T::size(),
            normalized: T::normalized(),
        }
    }
}

///
/// The type of a uniform value.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UniformType {
    Value,
    Vec2,
    Vec3,
    Vec4,
    Mat2,
    Mat3,
    Mat4,
}

///
/// Uniform data, already converted to the types a shader accepts.
/// Matrices are stored column by column.
///
#[derive(Clone, Debug, PartialEq)]
pub enum UniformValue {
    F32(UniformType, Vec<f32>),
    I32(UniformType, Vec<i32>),
    U32(UniformType, Vec<u32>),
}

// BUFFERS

///
/// What a buffer is used for.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BufferKind {
    Vertex,
    Element,
    Uniform,
}

///
/// A hint on how often a buffer is written to.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BufferUsage {
    Static,
    Dynamic,
}

// TEXTURES

///
/// The dimensionality of a texture.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextureKind {
    D2,
    D2Array,
    D3,
    CubeMap,
}

///
/// How a texture is sampled. In this crate the sampler state belongs to the texture.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SamplerDesc {
    pub min_filter: Interpolation,
    pub mag_filter: Interpolation,
    pub mipmap: Option<Mipmap>,
    pub wrap_s: Wrapping,
    pub wrap_t: Wrapping,
    pub wrap_r: Option<Wrapping>,
}

///
/// Size and format of the storage of a texture.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextureStorage {
    pub format: TextureFormat,
    pub mip_levels: u32,
    pub width: u32,
    pub height: u32,
    /// Number of layers for arrays, depth for 3D textures, ignored otherwise.
    pub depth: u32,
}

///
/// Description of a new texture.
/// If `storage` is `None`, the texture is created without storage (only used by the `new_unchecked` constructors on GL).
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextureDesc {
    pub kind: TextureKind,
    pub sampler: SamplerDesc,
    pub storage: Option<TextureStorage>,
}

///
/// The part of a texture that is written to by [BackendApi::write_texture]. Always mip level 0 and the full width and height.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextureRegion {
    /// The whole 2D or 3D texture.
    All,
    /// One layer of a texture array.
    Layer(u32),
    /// One side of a cube map.
    Side(CubeMapSide),
}

// RENDER TARGETS

///
/// The color part of a render target.
///
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ColorAttachment {
    Texture2D {
        texture: TextureId,
        mip_level: u32,
    },
    Texture2DArray {
        texture: TextureId,
        layers: Vec<u32>,
        mip_level: u32,
    },
    CubeMap {
        texture: TextureId,
        sides: Vec<CubeMapSide>,
        mip_level: u32,
    },
    Multisample(RenderbufferId),
}

///
/// The depth part of a render target.
///
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DepthAttachment {
    Texture2D(TextureId),
    Texture2DArray {
        texture: TextureId,
        layer: u32,
    },
    CubeMap {
        texture: TextureId,
        side: CubeMapSide,
    },
    Multisample(RenderbufferId),
}

///
/// What a pass renders into. `framebuffer: None` is the screen (or the default framebuffer).
///
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PassTarget {
    pub framebuffer: Option<FramebufferId>,
    pub color: Option<ColorAttachment>,
    pub depth: Option<DepthAttachment>,
    pub width: u32,
    pub height: u32,
}

///
/// Whether the existing content of a target is kept at the start of a pass.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LoadOp {
    Load,
}

///
/// What [BackendApi::copy_target] copies.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CopyMask {
    pub color: bool,
    pub depth: bool,
}

///
/// The format of the data returned by [BackendApi::read_pixels].
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadFormat {
    Color(ColorFormat),
    /// One `f32` per pixel.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Depth,
}

// PROGRAMS AND DRAWS

///
/// What the backend found out about a program when creating it.
///
#[derive(Clone, Debug, Default)]
pub(crate) struct ProgramReflection {
    /// Vertex attribute name to location.
    pub attributes: HashMap<String, u32>,
    /// Uniform name (without array brackets) to location.
    pub uniforms: HashMap<String, UniformLocation>,
}

///
/// The index buffer of a draw call.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndexBuffer {
    pub buffer: BufferId,
    pub index_type: PrimitiveType,
    pub first: u32,
}

///
/// A draw of triangles with one program. The vertex attributes, uniforms and textures are set on the program beforehand.
///
#[derive(Clone, Debug)]
pub(crate) struct DrawCall<'a> {
    pub program: ProgramId,
    /// The locations of all vertex attributes of the program.
    pub attribute_locations: &'a [u32],
    pub render_states: RenderStates,
    pub viewport: Viewport,
    pub indices: Option<IndexBuffer>,
    pub count: u32,
    /// `None` for a non-instanced draw.
    pub instances: Option<u32>,
}

///
/// The contract every backend fulfils.
///
/// All methods take `&self`; the GL backend executes them immediately in the given order.
/// Byte slices are tightly packed and, for textures, in the row order the GL backend expects (bottom row first).
///
pub(crate) trait BackendApi {
    // STATE (used by the public setters on Context)
    fn set_viewport(&self, viewport: Viewport);
    fn set_scissor(&self, scissor_box: ScissorBox);
    fn set_cull(&self, cull: Cull);
    fn set_write_mask(&self, write_mask: WriteMask);
    fn set_depth_test(&self, depth_test: DepthTest);
    fn set_blend(&self, blend: Blend);
    fn set_render_states(&self, render_states: RenderStates);
    fn error_check(&self) -> Result<(), CoreError>;
    /// The maximum number of samples for multisample targets.
    fn max_samples(&self) -> u32;

    // BUFFERS
    fn create_buffer(&self) -> BufferId;
    fn delete_buffer(&self, buffer: BufferId);
    /// Replaces the whole content and size of the buffer.
    fn write_buffer(&self, buffer: BufferId, kind: BufferKind, bytes: &[u8], usage: BufferUsage);
    /// Writes into the existing buffer at the given offset.
    fn write_buffer_subset(&self, buffer: BufferId, kind: BufferKind, offset: u32, bytes: &[u8]);

    // TEXTURES
    fn create_texture(&self, desc: &TextureDesc) -> TextureId;
    fn delete_texture(&self, texture: TextureId);
    fn write_texture(
        &self,
        texture: TextureId,
        kind: TextureKind,
        region: TextureRegion,
        width: u32,
        height: u32,
        depth: u32,
        format: ColorFormat,
        bytes: &[u8],
    );
    fn generate_mipmaps(&self, texture: TextureId, kind: TextureKind);
    fn create_renderbuffer(
        &self,
        format: TextureFormat,
        width: u32,
        height: u32,
        number_of_samples: u32,
    ) -> RenderbufferId;
    fn delete_renderbuffer(&self, renderbuffer: RenderbufferId);

    // RENDER TARGETS AND PASSES
    fn create_framebuffer(&self) -> FramebufferId;
    fn delete_framebuffer(&self, framebuffer: FramebufferId);
    /// Starts rendering into the given target, restricted to the scissor box.
    fn begin_pass(&self, target: &PassTarget, scissor_box: ScissorBox, load: LoadOp);
    /// Ends the pass started with [BackendApi::begin_pass].
    fn end_pass(&self);
    fn clear(&self, target: &PassTarget, scissor_box: ScissorBox, clear_state: ClearState);
    /// Copies color and/or depth from one target to another, used for resolving multisample targets.
    fn copy_target(&self, source: &PassTarget, destination: &PassTarget, mask: CopyMask);
    /// Reads pixels inside the rectangle, bottom row first.
    fn read_pixels(&self, source: &PassTarget, rect: ScissorBox, format: ReadFormat) -> Vec<u8>;

    // PROGRAMS
    fn create_program(
        &self,
        vertex_shader_source: &str,
        fragment_shader_source: &str,
    ) -> Result<(ProgramId, ProgramReflection), CoreError>;
    fn delete_program(&self, program: ProgramId);
    fn set_uniform(&self, program: ProgramId, location: &UniformLocation, value: &UniformValue);
    fn bind_texture(&self, unit: u32, texture: TextureId, kind: TextureKind);
    fn uniform_block_index(&self, program: ProgramId, name: &str) -> Option<u32>;
    fn bind_uniform_block(
        &self,
        program: ProgramId,
        block_index: u32,
        binding: u32,
        buffer: BufferId,
    );
    fn bind_vertex_attribute(
        &self,
        program: ProgramId,
        location: u32,
        buffer: BufferId,
        format: VertexFormat,
        divisor: u32,
    );
    /// Prepares a draw without vertex attributes (the full screen triangle).
    fn prepare_attributeless_draw(&self);
    fn draw(&self, call: &DrawCall);
}
