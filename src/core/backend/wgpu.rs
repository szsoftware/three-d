//!
//! The wgpu backend (only available with the `wgpu` feature). See `docs/wgpu-backend.md` for what users of the crate need to know.
//!
//! It covers everything the [core](crate::core) module needs: buffers, all kinds of textures including mipmaps,
//! multisample targets, programs translated from GLSL (see [glsl]), draw calls, copies between targets, reading back colors and depth
//! and the screen of a window (see [screen]).
//!
//! Conventions: The backend keeps the conventions of OpenGL instead of changing them.
//! The vertex shader epilogue added by the translation flips the y axis and converts the depth range,
//! so the first row of a texture in memory is the bottom row and all coordinates (viewport, scissor box, `gl_FragCoord`, UVs)
//! mean the same as on OpenGL. Because of the flip, the front face is clockwise for wgpu.
//!
//! Order of execution: The calls take effect in the order they are made, like on OpenGL, but they are not sent to the GPU one by one.
//! Draw calls and clears on the same target are recorded into one render pass, and all passes are recorded into one command encoder.
//! The encoder is submitted when the outermost pass ends, when pixels are read and before data is written into a buffer or texture
//! that recorded commands may use (see [WgpuBackend::flush]).
//! The uniforms of all recorded draw calls live in one ring buffer which is uploaded right before the encoder is submitted.
//!

use super::*;
use ::wgpu as w;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

mod bits;
mod builtin;
// wgpu draws with OpenGL on these systems only
#[cfg(all(
    feature = "wgpu-gl",
    any(target_os = "linux", target_os = "windows", target_os = "android")
))]
mod raw_gl;
mod screen;
#[cfg_attr(not(feature = "window"), allow(unused_imports))]
pub(crate) use screen::{AdapterPreference, ScreenDesc};
#[cfg(all(test, not(target_arch = "wasm32")))]
mod compare_tests;
pub(crate) mod glsl;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod renderer_compare_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod shader_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
#[cfg(feature = "wgsl")]
mod wgsl;
#[cfg(all(test, feature = "wgsl", not(target_arch = "wasm32")))]
mod wgsl_tests;

/// The size of the ring buffer for uniforms, it grows if a single program needs more.
const RING_CAPACITY: usize = 1 << 20;

/// The number of errors that are kept until the next error check.
const MAX_ERRORS: usize = 64;

const NO_SCREEN: &str = "the context has no screen, it was not created for a window";

///
/// The wgpu backend, see the module documentation.
///
pub(crate) struct WgpuBackend {
    pub device: w::Device,
    pub queue: w::Queue,
    state: Mutex<State>,
    /// The validation errors since the last error check, only collected if the backend created the device itself.
    errors: Option<Arc<Mutex<Vec<String>>>>,
    /// Offsets into the ring buffer must be a multiple of this.
    uniform_alignment: usize,
    pub(super) float32_filterable: bool,
    max_vertex_buffers: usize,
    /// The device draws with OpenGL, where wgpu needs help with cube maps, texture arrays and multisampling. See the `flat` function.
    gl: bool,
    /// Whether the device can render into textures of 32 bit floats. If not, see [TextureEntry::bits].
    pub(super) float32_renderable: bool,
}

// The backend only holds handles and plain data behind a mutex, a panic cannot leave it in a state that is unsafe to observe.
// Without these, enabling the `wgpu` feature would remove the traits from the public Context type.
impl std::panic::UnwindSafe for WgpuBackend {}
impl std::panic::RefUnwindSafe for WgpuBackend {}

struct State {
    next_id: u32,
    buffers: HashMap<u32, BufferEntry>,
    textures: HashMap<u32, TextureEntry>,
    /// Multisample textures.
    renderbuffers: HashMap<u32, w::Texture>,
    programs: HashMap<u32, ProgramEntry>,
    /// The texture bound to each texture unit, as on OpenGL.
    units: HashMap<u32, u32>,
    /// The targets and scissor boxes of the passes that are open, innermost last.
    passes: Vec<(PassTarget, ScissorBox)>,
    pipelines: HashMap<PipelineKey, w::RenderPipeline>,
    /// The bind groups with the textures of a program, by program and textures.
    texture_groups: HashMap<(u32, Vec<u32>), w::BindGroup>,
    /// Instance buffers that are combined into one buffer, by the buffers they are made of. See [WgpuBackend::draw].
    merged_buffers: HashMap<Vec<u32>, w::Buffer>,
    builtin: builtin::Builtin,
    /// The screen, if the backend has one.
    screen: Option<screen::Screen>,
    ring: Ring,
    /// Increased with every submit, the uniforms uploaded before are gone after it.
    epoch: u64,
    frame: Frame,
    /// The number of render passes for draw calls and clears and the number of submits so far.
    #[cfg(test)]
    statistics: (u32, u32),
}

/// The commands that are recorded but not submitted yet. The pass must be dropped before the encoder is finished.
#[derive(Default)]
struct Frame {
    pass: Option<OpenPass>,
    encoder: Option<w::CommandEncoder>,
}

struct OpenPass {
    pass: w::RenderPass<'static>,
    target: PassTarget,
    formats: TargetFormats,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct TargetFormats {
    colors: Vec<w::TextureFormat>,
    depth: Option<w::TextureFormat>,
    samples: u32,
}

/// Pixels that are on their way from the GPU into memory.
struct PendingRead {
    device: w::Device,
    buffer: w::Buffer,
    /// Whether the buffer is mapped, and who waits for that.
    mapped: Arc<Mutex<(bool, Option<std::task::Waker>)>>,
    /// The size of a row in the buffer and without padding.
    padded: usize,
    unpadded: usize,
    /// The type, the number of channels of the texels and the number of channels that are asked for.
    convert: Option<(PrimitiveType, usize, usize)>,
}

impl PendingRead {
    /// The pixels without padding. The buffer must be mapped.
    fn finish(&self) -> Vec<u8> {
        let data = self.buffer.slice(..).get_mapped_range();
        let mut tight = Vec::with_capacity(self.unpadded * (data.len() / self.padded));
        for row in data.chunks_exact(self.padded) {
            tight.extend_from_slice(&row[..self.unpadded]);
        }
        drop(data);
        self.buffer.unmap();
        match self.convert {
            Some((primitive, channels, wanted)) => {
                convert_channels(&tight, primitive, channels, wanted)
            }
            None => tight,
        }
    }

    /// Blocks until the pixels are there and writes them to the bytes.
    #[cfg(not(target_arch = "wasm32"))]
    fn wait_into(self, bytes: &mut [u8]) {
        self.device
            .poll(w::PollType::wait_indefinitely())
            .expect("failed to wait for the GPU");
        self.finish_into(bytes);
    }

    /// Writes the pixels to the bytes, without a copy in between if the channels are the ones asked for. The buffer must be mapped.
    fn finish_into(&self, bytes: &mut [u8]) {
        if self
            .convert
            .is_some_and(|(_, channels, wanted)| channels != wanted)
        {
            bytes.copy_from_slice(&self.finish());
            return;
        }
        let data = self.buffer.slice(..).get_mapped_range();
        for (row, tight) in data
            .chunks_exact(self.padded)
            .zip(bytes.chunks_exact_mut(self.unpadded))
        {
            tight.copy_from_slice(&row[..self.unpadded]);
        }
        drop(data);
        self.buffer.unmap();
    }

    /// Blocks until the pixels are there, which is not possible in a browser.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn wait(self) -> Vec<u8> {
        self.device
            .poll(w::PollType::wait_indefinitely())
            .expect("failed to wait for the GPU");
        self.finish()
    }
}

impl PendingRead {
    /// Asks whether the pixels are there, without waiting for them.
    fn poll_mapped(&self, context: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        // A browser maps the buffer by itself and then calls the waker, everywhere else the device has to be asked to do it.
        // That is done without waiting, so the future does not block while the graphics card works.
        #[cfg(not(target_arch = "wasm32"))]
        if !self.mapped.lock().unwrap_or_else(|e| e.into_inner()).0 {
            let _ = self.device.poll(w::PollType::Poll);
        }
        let mut mapped = self.mapped.lock().unwrap_or_else(|e| e.into_inner());
        if mapped.0 {
            std::task::Poll::Ready(())
        } else {
            mapped.1 = Some(context.waker().clone());
            // Nothing asks the device again, so the future asks to be polled again.
            // The thread makes room first, for when it is all the executor does (as in a blocking wait).
            #[cfg(not(target_arch = "wasm32"))]
            {
                drop(mapped);
                std::thread::yield_now();
                context.waker().wake_by_ref();
            }
            std::task::Poll::Pending
        }
    }
}

impl std::future::Future for PendingRead {
    type Output = Vec<u8>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Vec<u8>> {
        self.poll_mapped(context).map(|()| self.finish())
    }
}

/// One mip level of one layer of a texture.
struct Slice {
    texture: w::Texture,
    mip_level: u32,
    layer: u32,
}

impl Slice {
    fn view(&self) -> w::TextureView {
        attachment_view(&self.texture, self.mip_level, self.layer)
    }
}

/// The uniforms of the recorded draw calls, uploaded in one go before the commands are submitted.
struct Ring {
    buffer: w::Buffer,
    capacity: usize,
    staging: Vec<u8>,
    /// Increased when the buffer is replaced by a larger one.
    generation: u64,
}

struct BufferEntry {
    buffer: Option<w::Buffer>,
    usage: w::BufferUsages,
    /// The content as it was written, kept for element buffers and for vertex buffers that are stored in another layout.
    bytes: Option<Vec<u8>>,
    /// 8 bit indices do not exist in wgpu, this is the same content with 16 bit indices, created when it is drawn.
    wide_indices: Option<w::Buffer>,
    /// The format of the data, if it is stored with more components per vertex than the data has.
    widened: Option<VertexFormat>,
}

struct TextureEntry {
    texture: w::Texture,
    format: TextureFormat,
    sampler: w::Sampler,
    /// Used when the format cannot be filtered.
    nearest_sampler: w::Sampler,
    /// The views used for sampling the texture.
    views: HashMap<w::TextureViewDimension, w::TextureView>,
    /// For a texture of 32 bit floats on a device that cannot render into it: a texture of integers that is the target instead. See [bits].
    bits: Option<w::Texture>,
    /// The mip levels and layers that were rendered into the bits and not copied into the texture yet.
    stale: Vec<(u32, u32)>,
    /// Whether the texture has content that its bits must get when they are created.
    has_content: bool,
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
    /// Where the blocks were uploaded to the ring buffer and in which epoch, `None` if they changed since.
    uploaded: Option<(u64, [u32; 2])>,
    uniforms: Vec<UniformSlot>,
    /// The names of the entry points of the vertex and fragment shader.
    entry_points: [String; 2],
    samplers: Vec<(glsl::SamplerDecl, u32)>,
    /// Set if this is a compute program, then `vertex` is the module with the compute entry point `entry_points[0]`.
    compute: Option<ComputeEntry>,
    /// The buffer bound to each vertex attribute location.
    attributes: HashMap<u32, (u32, VertexFormat, u32)>,
    attribute_locations: Vec<u32>,
    uniform_layout: w::BindGroupLayout,
    /// The bind group for the uniform blocks in the ring buffer of the given generation.
    uniform_group: Option<(u64, w::BindGroup)>,
    /// The layouts of the textures, by the sample type of each texture.
    texture_layouts: HashMap<Vec<SampleKind>, w::BindGroupLayout>,
    /// The uniform blocks declared in the shaders and the buffers bound to them.
    user_blocks: Vec<(String, Option<u32>)>,
    user_block_layout: w::BindGroupLayout,
    number_of_outputs: usize,
    /// The module with the fragment shader, kept if the device cannot render into textures of 32 bit floats.
    /// From it come the fragment shaders that write bits to the color targets in the mask, see [bits].
    fragment_source: Option<naga::Module>,
    bits_fragments: HashMap<u32, w::ShaderModule>,
}

/// What a compute program has in addition to the uniforms, textures and uniform blocks of every program.
// Compute programs can only be created with the `wgsl` feature
#[cfg_attr(not(feature = "wgsl"), allow(dead_code))]
struct ComputeEntry {
    /// The textures the program writes to: name, format and the texture that is bound.
    storage: Vec<(String, w::TextureFormat, Option<u32>)>,
    storage_layout: w::BindGroupLayout,
    /// The pipelines by the way the textures of the program are sampled.
    pipelines: HashMap<Vec<SampleKind>, w::ComputePipeline>,
}

/// How a texture is sampled, which depends on its format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SampleKind {
    Filterable,
    /// Float textures that cannot be filtered on the device and depth textures, which are sampled as float textures.
    Unfilterable,
}

/// The layout of a vertex buffer: whether it is per instance, the size of a vertex and the location, format and offset of the attributes in it.
type VertexLayout = (bool, u64, Vec<(u32, w::VertexFormat, u64)>);

/// Everything a render pipeline depends on.
#[derive(Clone, PartialEq, Eq, Hash)]
struct PipelineKey {
    program: u32,
    cull: u8,
    depth_test: u8,
    write_mask: u8,
    blend: Option<[u8; 6]>,
    formats: TargetFormats,
    vertex_buffers: Vec<VertexLayout>,
    textures: Vec<SampleKind>,
}

///
/// Makes the test that calls this the only one that uses a graphics driver until its thread ends.
/// The tests run on the software drivers of Mesa for Vulkan and OpenGL. These share state inside the test process (one EGL display, one LLVM)
/// and crashed now and then in CI when several tests used them at the same time.
///
#[cfg(test)]
pub(crate) fn one_gpu_test_at_a_time() {
    use std::cell::RefCell;
    use std::sync::MutexGuard;
    static LOCK: Mutex<()> = Mutex::new(());
    thread_local!(static HELD: RefCell<Option<MutexGuard<'static, ()>>> = const { RefCell::new(None) });
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        if held.is_none() {
            *held = Some(LOCK.lock().unwrap_or_else(|e| e.into_inner()));
        }
    });
}

/// Whether the device draws with OpenGL.
fn gl_device(device: &w::Device) -> bool {
    device.adapter_info().backend == w::Backend::Gl
}

fn unsupported(what: &str) -> ! {
    panic!("{what} is not supported by the wgpu backend yet")
}

impl WgpuBackend {
    pub fn new(device: w::Device, queue: w::Queue) -> Self {
        #[cfg(all(
            feature = "wgpu-gl",
            any(target_os = "linux", target_os = "windows", target_os = "android")
        ))]
        if gl_device(&device) {
            raw_gl::seamless_cube_maps(&device);
        }
        let ring = Ring::new(&device, RING_CAPACITY, 0);
        Self {
            uniform_alignment: device.limits().min_uniform_buffer_offset_alignment as usize,
            float32_filterable: device.features().contains(w::Features::FLOAT32_FILTERABLE),
            max_vertex_buffers: device.limits().max_vertex_buffers as usize,
            gl: gl_device(&device),
            // Who has the adapter knows better, see [WgpuBackend::request]
            float32_renderable: !gl_device(&device),
            state: Mutex::new(State {
                next_id: 0,
                buffers: HashMap::new(),
                textures: HashMap::new(),
                renderbuffers: HashMap::new(),
                programs: HashMap::new(),
                units: HashMap::new(),
                passes: Vec::new(),
                pipelines: HashMap::new(),
                texture_groups: HashMap::new(),
                merged_buffers: HashMap::new(),
                builtin: Default::default(),
                screen: None,
                ring,
                epoch: 0,
                frame: Default::default(),
                #[cfg(test)]
                statistics: (0, 0),
            }),
            errors: None,
            device,
            queue,
        }
    }

    ///
    /// Creates a device without a window or surface.
    ///
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_headless() -> Result<Self, CoreError> {
        Self::new_headless_with(AdapterPreference::Any)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_headless_with(preference: AdapterPreference) -> Result<Self, CoreError> {
        #[cfg(test)]
        one_gpu_test_at_a_time();
        // From the environment: `WGPU_BACKEND` chooses the graphics API
        let instance =
            w::Instance::new(w::InstanceDescriptor::new_without_display_handle_from_env());
        let (backend, _) = pollster::block_on(Self::request(&instance, preference, None))?;
        Ok(backend)
    }

    ///
    /// Creates a shader module from a translated shader.
    ///
    fn shader_module(&self, module: naga::Module) -> w::ShaderModule {
        // WebGPU in a browser refuses shaders that sample a texture with automatic mip level inside a branch
        // that depends on values which differ from pixel to pixel ("derivative uniformity").
        // GLSL allows that and the shaders of three-d and its users do it, as do the other platforms of wgpu.
        // The check can only be turned off in WGSL text, so on the web the shader is handed over as text.
        #[cfg(target_arch = "wasm32")]
        let source = {
            let info = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("a translated shader is invalid: {error:?}"));
            let text = naga::back::wgsl::write_string(
                &module,
                &info,
                naga::back::wgsl::WriterFlags::empty(),
            )
            .unwrap_or_else(|error| panic!("a translated shader cannot be written: {error}"));
            w::ShaderSource::Wgsl(format!("diagnostic(off, derivative_uniformity);\n{text}").into())
        };
        #[cfg(not(target_arch = "wasm32"))]
        let source = w::ShaderSource::Naga(std::borrow::Cow::Owned(module));
        self.device.create_shader_module(w::ShaderModuleDescriptor {
            label: None,
            source,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while the state was locked (for example about unsupported functionality) must not turn every later call into a panic
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn id(state: &mut State) -> u32 {
        state.next_id += 1;
        state.next_id
    }

    ///
    /// The encoder all commands are recorded into, after ending the open render pass.
    ///
    fn encoder<'a>(&self, state: &'a mut State) -> &'a mut w::CommandEncoder {
        state.frame.pass = None;
        state
            .frame
            .encoder
            .get_or_insert_with(|| self.device.create_command_encoder(&Default::default()))
    }

    ///
    /// The wgpu texture of a texture for use outside three-d, see [crate::Texture2D::as_wgpu]. Submits everything recorded so far,
    /// and forgets the integer twin of a 32 bit float texture (see [bits]), which is made again from the texture when it is rendered into,
    /// because the caller may change the texture.
    ///
    pub(crate) fn external_texture(&self, texture: TextureId) -> w::Texture {
        let state = &mut *self.lock();
        let id = texture.wgpu();
        self.synchronize_bits(state, id);
        self.flush(state);
        let entry = state.textures.get_mut(&id).unwrap();
        entry.has_content = true;
        entry.bits = None;
        entry.texture.clone()
    }

    ///
    /// Submits everything that is recorded. Must be called before data is written through the queue into a resource that recorded commands may use,
    /// because such writes take effect before the commands of an encoder that is submitted later.
    ///
    fn flush(&self, state: &mut State) {
        state.frame.pass = None;
        if let Some(encoder) = state.frame.encoder.take() {
            if !state.ring.staging.is_empty() {
                let size = state.ring.staging.len().next_multiple_of(4);
                state.ring.staging.resize(size, 0);
                self.queue
                    .write_buffer(&state.ring.buffer, 0, &state.ring.staging);
                state.ring.staging.clear();
            }
            state.epoch += 1;
            self.queue.submit([encoder.finish()]);
            #[cfg(test)]
            {
                state.statistics.1 += 1;
            }
            // Lets wgpu free what the submitted commands no longer need
            #[cfg(not(target_arch = "wasm32"))]
            let _ = self.device.poll(w::PollType::Poll);
        }
    }

    ///
    /// Reserves space for the given uniform block in the ring buffer and returns the offset.
    /// Submits the recorded commands if the ring buffer is full, so this must be called before the pass of a draw call is looked up.
    ///
    fn upload_uniforms(&self, state: &mut State, bytes: &[u8]) -> u32 {
        let mut offset = state
            .ring
            .staging
            .len()
            .next_multiple_of(self.uniform_alignment);
        if offset + bytes.len() > state.ring.capacity {
            self.flush(state);
            offset = 0;
            if bytes.len() > state.ring.capacity {
                state.ring = Ring::new(
                    &self.device,
                    bytes.len().next_power_of_two(),
                    state.ring.generation + 1,
                );
            }
        }
        state.ring.staging.resize(offset, 0);
        state.ring.staging.extend_from_slice(bytes);
        offset as u32
    }

    /// The color attachments of a target.
    fn color_slices(&self, state: &State, target: &PassTarget) -> Vec<Slice> {
        let slice = |texture: &TextureId, mip_level: u32, layer: u32| {
            let entry = &state.textures[&texture.wgpu()];
            Slice {
                // See [WgpuBackend::prepare_bits]
                texture: entry.bits.as_ref().unwrap_or(&entry.texture).clone(),
                mip_level,
                layer,
            }
        };
        if target.framebuffer.is_none() {
            return vec![Slice {
                texture: state.screen.as_ref().expect(NO_SCREEN).color().clone(),
                mip_level: 0,
                layer: 0,
            }];
        }
        match &target.color {
            None => Vec::new(),
            Some(ColorAttachment::Texture2D { texture, mip_level }) => {
                vec![slice(texture, *mip_level, 0)]
            }
            Some(ColorAttachment::Texture2DArray {
                texture,
                layers,
                mip_level,
            }) => layers
                .iter()
                .map(|layer| slice(texture, *mip_level, *layer))
                .collect(),
            Some(ColorAttachment::CubeMap {
                texture,
                sides,
                mip_level,
            }) => sides
                .iter()
                .map(|side| slice(texture, *mip_level, cube_layer(*side)))
                .collect(),
            Some(ColorAttachment::Multisample(renderbuffer)) => vec![Slice {
                texture: state.renderbuffers[&renderbuffer.wgpu()].clone(),
                mip_level: 0,
                layer: 0,
            }],
        }
    }

    /// The depth attachment of a target.
    fn depth_slice(&self, state: &State, target: &PassTarget) -> Option<Slice> {
        let slice = |texture: &TextureId, layer: u32| Slice {
            texture: state.textures[&texture.wgpu()].texture.clone(),
            mip_level: 0,
            layer,
        };
        if target.framebuffer.is_none() {
            let depth = state.screen.as_ref().expect(NO_SCREEN).depth()?;
            return Some(Slice {
                texture: depth.clone(),
                mip_level: 0,
                layer: 0,
            });
        }
        match &target.depth {
            None => None,
            Some(DepthAttachment::Texture2D(texture)) => Some(slice(texture, 0)),
            Some(DepthAttachment::Texture2DArray { texture, layer }) => {
                Some(slice(texture, *layer))
            }
            Some(DepthAttachment::CubeMap { texture, side }) => {
                Some(slice(texture, cube_layer(*side)))
            }
            Some(DepthAttachment::Multisample(renderbuffer)) => Some(Slice {
                texture: state.renderbuffers[&renderbuffer.wgpu()].clone(),
                mip_level: 0,
                layer: 0,
            }),
        }
    }

    ///
    /// Makes sure that a render pass on the given target is open and returns it.
    /// A pass on the same target that is already open is continued, unless the target is cleared.
    ///
    fn pass<'a>(
        &self,
        state: &'a mut State,
        target: &PassTarget,
        clear: Option<ClearState>,
    ) -> &'a mut OpenPass {
        let same_target = |open: &OpenPass| {
            open.target.color == target.color
                && open.target.depth == target.depth
                && open.target.width == target.width
                && open.target.height == target.height
        };
        if clear.is_some() || !state.frame.pass.as_ref().is_some_and(same_target) {
            self.prepare_bits(state, target, true);
            let colors = self.color_slices(state, target);
            let depth = self.depth_slice(state, target);
            let formats = TargetFormats {
                colors: colors.iter().map(|s| s.texture.format()).collect(),
                depth: depth.as_ref().map(|s| s.texture.format()),
                samples: colors
                    .first()
                    .or(depth.as_ref())
                    .map(|s| s.texture.sample_count())
                    .unwrap_or(1),
            };
            let color_load = |format: w::TextureFormat| match clear {
                Some(ClearState {
                    red: Some(r),
                    green: Some(g),
                    blue: Some(b),
                    alpha: Some(a),
                    ..
                }) => {
                    // A target of integers gets the bit patterns of the floats, see [bits]
                    let value = |v: f32| {
                        if bits::is_bits(format) {
                            v.to_bits() as f64
                        } else {
                            v as f64
                        }
                    };
                    w::LoadOp::Clear(w::Color {
                        r: value(r),
                        g: value(g),
                        b: value(b),
                        a: value(a),
                    })
                }
                _ => w::LoadOp::Load,
            };
            let depth_load = match clear.and_then(|c| c.depth) {
                Some(depth) => w::LoadOp::Clear(depth),
                None => w::LoadOp::Load,
            };
            let color_views: Vec<_> = colors.iter().map(Slice::view).collect();
            let depth_view = depth.as_ref().map(Slice::view);
            let color_attachments: Vec<_> = color_views
                .iter()
                .zip(&formats.colors)
                .map(|(view, format)| {
                    Some(w::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: w::Operations {
                            load: color_load(*format),
                            store: w::StoreOp::Store,
                        },
                    })
                })
                .collect();
            let pass = self
                .encoder(state)
                .begin_render_pass(&w::RenderPassDescriptor {
                    label: None,
                    color_attachments: &color_attachments,
                    depth_stencil_attachment: depth_view.as_ref().map(|view| {
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
                })
                .forget_lifetime();
            #[cfg(test)]
            {
                state.statistics.0 += 1;
            }
            state.frame.pass = Some(OpenPass {
                pass,
                target: target.clone(),
                formats,
            });
        }
        state.frame.pass.as_mut().unwrap()
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
        let (mag_filter, min_filter) = (filter(desc.mag_filter), filter(desc.min_filter));
        let mipmap_filter = match desc.mipmap {
            Some(mipmap) if filter(mipmap.filter) == w::FilterMode::Linear => {
                w::MipmapFilterMode::Linear
            }
            _ => w::MipmapFilterMode::Nearest,
        };
        // Anisotropic filtering needs linear filtering everywhere
        let anisotropy_clamp = match desc.mipmap {
            Some(mipmap)
                if mag_filter == w::FilterMode::Linear
                    && min_filter == w::FilterMode::Linear
                    && mipmap_filter == w::MipmapFilterMode::Linear =>
            {
                mipmap.max_ratio.clamp(1, 16) as u16
            }
            _ => 1,
        };
        self.device.create_sampler(&w::SamplerDescriptor {
            address_mode_u: address(desc.wrap_s),
            address_mode_v: address(desc.wrap_t),
            address_mode_w: address(desc.wrap_r.unwrap_or(desc.wrap_s)),
            mag_filter,
            min_filter,
            mipmap_filter,
            // Without mipmaps only the base level is sampled, even if the texture has more levels
            lod_max_clamp: if desc.mipmap.is_some() { 32.0 } else { 0.0 },
            anisotropy_clamp,
            ..Default::default()
        })
    }

    ///
    /// Starts to copy a rectangle of a texture with the given size of a texel into memory.
    ///
    fn start_read(
        &self,
        state: &mut State,
        source: &Slice,
        rect: ScissorBox,
        texel_size: usize,
    ) -> PendingRead {
        let unpadded = rect.width as usize * texel_size;
        let padded = unpadded.next_multiple_of(w::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let buffer = self.device.create_buffer(&w::BufferDescriptor {
            label: None,
            size: (padded * rect.height as usize) as u64,
            usage: w::BufferUsages::COPY_DST | w::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        self.encoder(state).copy_texture_to_buffer(
            w::TexelCopyTextureInfo {
                texture: &source.texture,
                mip_level: source.mip_level,
                origin: w::Origin3d {
                    x: rect.x as u32,
                    y: rect.y as u32,
                    z: source.layer,
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
        self.flush(state);
        let mapped = Arc::new(Mutex::new((false, None::<std::task::Waker>)));
        let shared = mapped.clone();
        buffer.slice(..).map_async(w::MapMode::Read, move |result| {
            // An error means that the read was dropped before the pixels arrived, then nobody waits for them
            if result.is_err() {
                return;
            }
            let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
            shared.0 = true;
            if let Some(waker) = shared.1.take() {
                waker.wake();
            }
        });
        PendingRead {
            device: self.device.clone(),
            buffer,
            mapped,
            padded,
            unpadded,
            convert: None,
        }
    }

    ///
    /// Copies a rectangle of a texture with the given size of a texel into memory, without padding.
    ///
    #[cfg(test)]
    fn read_texture(
        &self,
        state: &mut State,
        source: &Slice,
        rect: ScissorBox,
        texel_size: usize,
    ) -> Vec<u8> {
        self.start_read(state, source, rect, texel_size).wait()
    }

    /// The usage of a texture that makes it a render target, if the device can render into its format.
    fn render_usage(&self, format: w::TextureFormat) -> w::TextureUsages {
        use w::TextureFormat as F;
        if !self.float32_renderable && matches!(format, F::R32Float | F::Rg32Float | F::Rgba32Float)
        {
            w::TextureUsages::empty()
        } else {
            w::TextureUsages::RENDER_ATTACHMENT
        }
    }

    ///
    /// The usage of a multisample texture. On OpenGL, wgpu can only create one that is rendered into and resolved,
    /// see [WgpuBackend::resolved_depth] for its depth.
    ///
    pub(super) fn multisample_usage(&self) -> w::TextureUsages {
        if self.gl {
            w::TextureUsages::RENDER_ATTACHMENT
        } else {
            w::TextureUsages::TEXTURE_BINDING
                | w::TextureUsages::COPY_SRC
                | w::TextureUsages::RENDER_ATTACHMENT
        }
    }

    ///
    /// The depth of a multisample texture on OpenGL as a texture with one sample, where the depth of a pixel is the depth of one of its samples.
    /// wgpu cannot do that there, so OpenGL is used directly. Without the `wgpu-gl` feature (with a device of the application
    /// that draws with OpenGL), the result is the largest depth everywhere.
    ///
    fn resolved_depth(&self, state: &mut State, from: &Slice) -> Slice {
        let size = from.texture.size();
        let resolved = Slice {
            texture: self.device.create_texture(&w::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: w::TextureDimension::D2,
                format: from.texture.format(),
                usage: w::TextureUsages::RENDER_ATTACHMENT | w::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }),
            mip_level: 0,
            layer: 0,
        };
        // wgpu clears a texture before its first use, which must not happen after the depth is copied behind its back
        let view = resolved.view();
        self.encoder(state)
            .begin_render_pass(&w::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(w::RenderPassDepthStencilAttachment {
                    view: &view,
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
        self.flush(state);
        #[cfg(all(
            feature = "wgpu-gl",
            any(target_os = "linux", target_os = "windows", target_os = "android")
        ))]
        raw_gl::resolve_depth(&self.device, &from.texture, &resolved.texture);
        resolved
    }

    ///
    /// A slice that the builtin passes can sample as a 2D texture and that can be copied into a buffer or another texture.
    /// That is the slice itself, except on OpenGL: there wgpu cannot copy from a cube map or a depth texture
    /// and cannot bind one layer of a cube map or of an array as a 2D texture. So the slice is drawn into a 2D texture first.
    ///
    fn flat(&self, state: &mut State, slice: Slice) -> Slice {
        let texture = &slice.texture;
        let depth = texture.format().is_depth_stencil_format();
        let layers = texture.depth_or_array_layers();
        if !self.gl || texture.dimension() != w::TextureDimension::D2 || layers == 1 {
            return slice;
        }
        let size = texture.size();
        let cube = size.width == size.height && layers == 6;
        // Layers of an array with colors can be copied, everything else is drawn
        let copy = !cube && !depth;
        let flat = Slice {
            texture: self.device.create_texture(&w::TextureDescriptor {
                label: None,
                size: w::Extent3d {
                    width: (size.width >> slice.mip_level).max(1),
                    height: (size.height >> slice.mip_level).max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: w::TextureDimension::D2,
                format: texture.format(),
                usage: w::TextureUsages::COPY_SRC
                    | w::TextureUsages::TEXTURE_BINDING
                    | if copy {
                        w::TextureUsages::COPY_DST
                    } else {
                        w::TextureUsages::RENDER_ATTACHMENT
                    },
                view_formats: &[],
            }),
            mip_level: 0,
            layer: 0,
        };
        if copy {
            self.copy_slice(state, &slice, &flat);
            return flat;
        }
        let source = texture.create_view(&w::TextureViewDescriptor {
            dimension: Some(if cube {
                w::TextureViewDimension::Cube
            } else {
                w::TextureViewDimension::D2Array
            }),
            ..Default::default()
        });
        self.builtin_pass(
            state,
            builtin::Kind::Flatten {
                cube,
                depth,
                bits: false,
            },
            &source,
            &flat.view(),
            texture.format(),
            Some(slice.layer | (slice.mip_level << 8)),
        );
        flat
    }

    /// A texture with one sample that the builtin passes can render into and that can be read back.
    fn temporary_texture(&self, format: w::TextureFormat, width: u32, height: u32) -> Slice {
        Slice {
            texture: self.device.create_texture(&w::TextureDescriptor {
                label: None,
                size: w::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: w::TextureDimension::D2,
                format,
                usage: w::TextureUsages::RENDER_ATTACHMENT | w::TextureUsages::COPY_SRC,
                view_formats: &[],
            }),
            mip_level: 0,
            layer: 0,
        }
    }

    /// Resolves a multisample color texture into a texture with one sample.
    fn resolve_color(&self, state: &mut State, source: &Slice, destination: &Slice) {
        let (source, destination) = (source.view(), destination.view());
        self.encoder(state)
            .begin_render_pass(&w::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(w::RenderPassColorAttachment {
                    view: &source,
                    depth_slice: None,
                    resolve_target: Some(&destination),
                    ops: w::Operations {
                        load: w::LoadOp::Load,
                        store: w::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
    }

    /// Copies one layer of one mip level of a texture to another texture with the same format and number of samples.
    fn copy_slice(&self, state: &mut State, source: &Slice, destination: &Slice) {
        fn info(slice: &Slice) -> w::TexelCopyTextureInfo<'_> {
            w::TexelCopyTextureInfo {
                texture: &slice.texture,
                mip_level: slice.mip_level,
                origin: w::Origin3d {
                    x: 0,
                    y: 0,
                    z: slice.layer,
                },
                aspect: w::TextureAspect::All,
            }
        }
        let size = source.texture.size();
        self.encoder(state).copy_texture_to_texture(
            info(source),
            info(destination),
            w::Extent3d {
                width: (size.width >> source.mip_level).max(1),
                height: (size.height >> source.mip_level).max(1),
                depth_or_array_layers: 1,
            },
        );
    }
}

impl Ring {
    fn new(device: &w::Device, capacity: usize, generation: u64) -> Self {
        Self {
            buffer: device.create_buffer(&w::BufferDescriptor {
                label: Some("three-d uniforms"),
                size: capacity as u64,
                usage: w::BufferUsages::UNIFORM | w::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            capacity,
            staging: Vec::new(),
            generation,
        }
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

/// The formats a compute program can write to get the usage for it, which only matters with the `wgsl` feature.
fn storage_usage(format: w::TextureFormat) -> w::TextureUsages {
    use w::TextureFormat as F;
    if cfg!(feature = "wgsl")
        && matches!(
            format,
            F::Rgba8Unorm | F::Rgba16Float | F::Rgba32Float | F::R32Float
        )
    {
        w::TextureUsages::STORAGE_BINDING
    } else {
        w::TextureUsages::empty()
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

/// The primitive type and number of channels of the color formats this backend creates.
fn texel_layout(format: w::TextureFormat) -> (PrimitiveType, usize) {
    use w::TextureFormat as F;
    match format {
        F::R8Unorm => (PrimitiveType::U8, 1),
        F::Rg8Unorm => (PrimitiveType::U8, 2),
        F::Rgba8Unorm => (PrimitiveType::U8, 4),
        F::R16Float => (PrimitiveType::F16, 1),
        F::Rg16Float => (PrimitiveType::F16, 2),
        F::Rgba16Float => (PrimitiveType::F16, 4),
        // The integer formats hold the bits of floats, see [bits]
        F::R32Float | F::R32Uint => (PrimitiveType::F32, 1),
        F::Rg32Float | F::Rg32Uint => (PrimitiveType::F32, 2),
        F::Rgba32Float | F::Rgba32Uint => (PrimitiveType::F32, 4),
        _ => unsupported(&format!("reading from the format {format:?}")),
    }
}

///
/// The number of components that are stored for a vertex attribute.
/// The size of a vertex must be a multiple of 4 bytes in wgpu, so small formats are stored with more components than they have.
///
fn stored_components(format: VertexFormat) -> u32 {
    match (primitive_size(format.primitive), format.components) {
        (1, _) => 4,
        (2, 1) => 2,
        (2, 3) => 4,
        (_, components) => components,
    }
}

/// Converts tightly packed vertices to the number of components that are stored, see [stored_components].
fn widen_vertices(bytes: &[u8], format: VertexFormat) -> Vec<u8> {
    let (from, to) = (
        format.components as usize,
        stored_components(format) as usize,
    );
    let size = primitive_size(format.primitive);
    // A missing fourth component is one, as for a vertex attribute with less components than the shader expects
    let one: Vec<u8> = match (format.primitive, format.normalized) {
        (PrimitiveType::U8, true) => vec![255],
        (PrimitiveType::I8, true) => vec![127],
        (PrimitiveType::U16, true) => u16::MAX.to_le_bytes().to_vec(),
        (PrimitiveType::I16, true) => i16::MAX.to_le_bytes().to_vec(),
        (PrimitiveType::F16, _) => f16::from_f32(1.0).to_le_bytes().to_vec(),
        (PrimitiveType::U8 | PrimitiveType::I8, false) => vec![1],
        _ => 1u16.to_le_bytes().to_vec(),
    };
    let mut output = Vec::with_capacity(bytes.len() / from * to);
    for vertex in bytes.chunks_exact(from * size) {
        output.extend_from_slice(vertex);
        for component in from..to {
            if component == 3 {
                output.extend_from_slice(&one);
            } else {
                output.extend(std::iter::repeat_n(0, size));
            }
        }
    }
    output
}

/// The format of a vertex attribute as it is stored, see [stored_components].
fn vertex_format(format: VertexFormat) -> w::VertexFormat {
    use w::VertexFormat as F;
    use PrimitiveType as P;
    match (
        format.primitive,
        stored_components(format),
        format.normalized,
    ) {
        (P::F32, 1, _) => F::Float32,
        (P::F32, 2, _) => F::Float32x2,
        (P::F32, 3, _) => F::Float32x3,
        (P::F32, 4, _) => F::Float32x4,
        (P::F16, 2, _) => F::Float16x2,
        (P::F16, 4, _) => F::Float16x4,
        (P::U8, 4, true) => F::Unorm8x4,
        (P::U8, 4, false) => F::Uint8x4,
        (P::I8, 4, true) => F::Snorm8x4,
        (P::I8, 4, false) => F::Sint8x4,
        (P::U16, 2, true) => F::Unorm16x2,
        (P::U16, 4, true) => F::Unorm16x4,
        (P::U16, 2, false) => F::Uint16x2,
        (P::U16, 4, false) => F::Uint16x4,
        (P::I16, 2, true) => F::Snorm16x2,
        (P::I16, 4, true) => F::Snorm16x4,
        (P::I16, 2, false) => F::Sint16x2,
        (P::I16, 4, false) => F::Sint16x4,
        (P::U32, 1, _) => F::Uint32,
        (P::U32, 2, _) => F::Uint32x2,
        (P::U32, 3, _) => F::Uint32x3,
        (P::U32, 4, _) => F::Uint32x4,
        (P::I32, 1, _) => F::Sint32,
        (P::I32, 2, _) => F::Sint32x2,
        (P::I32, 3, _) => F::Sint32x3,
        (P::I32, 4, _) => F::Sint32x4,
        _ => unsupported(&format!("the vertex format {format:?}")),
    }
}

/// The part of a rectangle that is inside the target.
fn clamp_to_target(target: &PassTarget, rectangle: ScissorBox) -> (u32, u32, u32, u32) {
    let x0 = rectangle.x.clamp(0, target.width as i32);
    let y0 = rectangle.y.clamp(0, target.height as i32);
    let x1 = (rectangle.x + rectangle.width as i32).clamp(x0, target.width as i32);
    let y1 = (rectangle.y + rectangle.height as i32).clamp(y0, target.height as i32);
    (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
}

fn compare_function(depth_test: DepthTest) -> w::CompareFunction {
    match depth_test {
        DepthTest::Never => w::CompareFunction::Never,
        DepthTest::Less => w::CompareFunction::Less,
        DepthTest::Equal => w::CompareFunction::Equal,
        DepthTest::LessOrEqual => w::CompareFunction::LessEqual,
        DepthTest::Greater => w::CompareFunction::Greater,
        DepthTest::NotEqual => w::CompareFunction::NotEqual,
        DepthTest::GreaterOrEqual => w::CompareFunction::GreaterEqual,
        DepthTest::Always => w::CompareFunction::Always,
    }
}

fn blend_state(blend: Blend) -> Option<w::BlendState> {
    match blend {
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
    }
}

/// The blend state as plain numbers, for the key of a pipeline.
fn blend_key(blend: Blend) -> Option<[u8; 6]> {
    match blend {
        Blend::Disabled => None,
        Blend::Enabled {
            source_rgb_multiplier,
            source_alpha_multiplier,
            destination_rgb_multiplier,
            destination_alpha_multiplier,
            rgb_equation,
            alpha_equation,
        } => Some([
            source_rgb_multiplier as u8,
            source_alpha_multiplier as u8,
            destination_rgb_multiplier as u8,
            destination_alpha_multiplier as u8,
            rgb_equation as u8,
            alpha_equation as u8,
        ]),
    }
}

fn color_writes(write_mask: WriteMask) -> w::ColorWrites {
    let mut color_writes = w::ColorWrites::empty();
    color_writes.set(w::ColorWrites::RED, write_mask.red);
    color_writes.set(w::ColorWrites::GREEN, write_mask.green);
    color_writes.set(w::ColorWrites::BLUE, write_mask.blue);
    color_writes.set(w::ColorWrites::ALPHA, write_mask.alpha);
    color_writes
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
    fn set_scissor(&self, scissor_box: ScissorBox) {
        // As on OpenGL, this replaces the scissor box of the render target that is written to
        if let Some((_, scissor)) = self.lock().passes.last_mut() {
            *scissor = scissor_box;
        }
    }
    fn set_cull(&self, _cull: Cull) {}
    fn set_write_mask(&self, _write_mask: WriteMask) {}
    fn set_depth_test(&self, _depth_test: DepthTest) {}
    fn set_blend(&self, _blend: Blend) {}
    fn set_render_states(&self, _render_states: RenderStates) {}

    fn error_check(&self) -> Result<(), CoreError> {
        // Without the list, the device was created by the user of the crate and reports errors to the handler set there (by default it panics)
        let Some(errors) = &self.errors else {
            return Ok(());
        };
        let errors = std::mem::take(&mut *errors.lock().unwrap_or_else(|e| e.into_inner()));
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CoreError::ContextError(errors.join("\n")))
        }
    }

    fn max_samples(&self) -> u32 {
        4
    }

    fn create_buffer(&self) -> BufferId {
        let mut state = self.lock();
        let id = Self::id(&mut state);
        state.buffers.insert(
            id,
            BufferEntry {
                buffer: None,
                usage: w::BufferUsages::empty(),
                bytes: None,
                wide_indices: None,
                widened: None,
            },
        );
        BufferId::Wgpu(id)
    }

    fn delete_buffer(&self, buffer: BufferId) {
        let state = &mut *self.lock();
        let id = buffer.wgpu();
        state.buffers.remove(&id);
        state.merged_buffers.retain(|ids, _| !ids.contains(&id));
    }

    fn write_buffer(&self, buffer: BufferId, kind: BufferKind, bytes: &[u8], _usage: BufferUsage) {
        // The buffer is replaced, so commands that are already recorded keep using the old content
        let mut entry = BufferEntry {
            buffer: None,
            usage: w::BufferUsages::UNIFORM,
            bytes: None,
            wide_indices: None,
            widened: None,
        };
        match kind {
            BufferKind::Vertex(format) => {
                entry.usage = w::BufferUsages::VERTEX;
                if stored_components(format) != format.components {
                    entry.widened = Some(format);
                    entry.bytes = Some(bytes.to_vec());
                }
            }
            BufferKind::Instance(format) => {
                entry.usage = w::BufferUsages::VERTEX;
                if stored_components(format) != format.components {
                    entry.widened = Some(format);
                }
                // Kept because instance buffers are combined when a draw call has more attributes than vertex buffers are allowed
                entry.bytes = Some(bytes.to_vec());
            }
            BufferKind::Element => {
                entry.usage = w::BufferUsages::INDEX;
                entry.bytes = Some(bytes.to_vec());
            }
            BufferKind::Uniform => {}
        }
        entry.buffer = Some(match entry.widened {
            Some(format) => self.new_buffer(&widen_vertices(bytes, format), entry.usage),
            None => self.new_buffer(bytes, entry.usage),
        });
        let state = &mut *self.lock();
        let id = buffer.wgpu();
        state.buffers.insert(id, entry);
        state.merged_buffers.retain(|ids, _| !ids.contains(&id));
    }

    fn write_buffer_subset(&self, buffer: BufferId, _kind: BufferKind, offset: u32, bytes: &[u8]) {
        let state = &mut *self.lock();
        let id = buffer.wgpu();
        state.merged_buffers.retain(|ids, _| !ids.contains(&id));
        let entry = state.buffers.get_mut(&id).unwrap();
        if let Some(content) = &mut entry.bytes {
            // The buffer is not stored as it was written or the write may not be aligned to 4 bytes, so the buffer is replaced
            let end = offset as usize + bytes.len();
            if content.len() < end {
                content.resize(end, 0);
            }
            content[offset as usize..end].copy_from_slice(bytes);
            entry.buffer = Some(match entry.widened {
                Some(format) => self.new_buffer(&widen_vertices(content, format), entry.usage),
                None => self.new_buffer(content, entry.usage),
            });
            entry.wide_indices = None;
        } else {
            let buffer = entry
                .buffer
                .clone()
                .expect("a subset of a buffer is written before the buffer is filled");
            if !bytes.len().is_multiple_of(4) || !offset.is_multiple_of(4) {
                unsupported("writing a part of a buffer that is not aligned to 4 bytes");
            }
            // Recorded commands must see the old content
            self.flush(state);
            self.queue.write_buffer(&buffer, offset as u64, bytes);
        }
    }

    fn create_texture(&self, desc: &TextureDesc) -> TextureId {
        let Some(storage) = desc.storage else {
            unsupported("a texture without storage")
        };
        let (format, _) = texture_format(storage.format);
        let (dimension, layers) = match desc.kind {
            TextureKind::D2 => (w::TextureDimension::D2, 1),
            // On OpenGL, wgpu makes a cube map out of every square texture with 6 layers and a plain 2D texture out of one layer.
            // Neither can be sampled as an array, so such an array gets one layer more than it needs.
            TextureKind::D2Array
                if self.gl
                    && (storage.depth == 1
                        || (storage.width == storage.height && storage.depth % 6 == 0)) =>
            {
                (w::TextureDimension::D2, storage.depth + 1)
            }
            TextureKind::D2Array => (w::TextureDimension::D2, storage.depth),
            TextureKind::D3 => (w::TextureDimension::D3, storage.depth),
            TextureKind::CubeMap => (w::TextureDimension::D2, 6),
        };
        let texture = self.device.create_texture(&w::TextureDescriptor {
            label: None,
            size: w::Extent3d {
                width: storage.width,
                height: storage.height,
                depth_or_array_layers: layers,
            },
            mip_level_count: storage.mip_levels.max(1),
            sample_count: 1,
            dimension,
            format,
            usage: w::TextureUsages::TEXTURE_BINDING
                | w::TextureUsages::COPY_DST
                | w::TextureUsages::COPY_SRC
                | self.render_usage(format)
                | storage_usage(format),
            view_formats: &[],
        });
        let entry = TextureEntry {
            texture,
            format: storage.format,
            sampler: self.sampler(&desc.sampler, false),
            nearest_sampler: self.sampler(&desc.sampler, true),
            views: HashMap::new(),
            bits: None,
            stale: Vec::new(),
            has_content: false,
        };
        let mut state = self.lock();
        let id = Self::id(&mut state);
        state.textures.insert(id, entry);
        TextureId::Wgpu(id)
    }

    fn delete_texture(&self, texture: TextureId) {
        let state = &mut *self.lock();
        let id = texture.wgpu();
        state.textures.remove(&id);
        state
            .texture_groups
            .retain(|(_, textures), _| !textures.contains(&id));
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
        let state = &mut *self.lock();
        // Recorded commands must see the old content
        self.flush(state);
        let entry = state.textures.get_mut(&texture.wgpu()).unwrap();
        entry.has_content = true;
        let entry = &*entry;
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
        // The first row of the data is the bottom row, which is the first row in memory in this backend as well.
        // The bits of the texture get the same bytes, see [bits].
        for texture in [Some(&entry.texture), entry.bits.as_ref()]
            .into_iter()
            .flatten()
        {
            self.queue.write_texture(
                w::TexelCopyTextureInfo {
                    texture,
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
    }

    fn generate_mipmaps(&self, texture: TextureId, _kind: TextureKind) {
        let state = &mut *self.lock();
        let id = texture.wgpu();
        // A texture that the device cannot render into gets its mipmaps through its bits
        let bits = self.bits(state, id);
        let texture = bits
            .clone()
            .unwrap_or_else(|| state.textures[&id].texture.clone());
        let format = texture.format();
        let kind = if bits.is_some() {
            builtin::Kind::Mip2DBits
        } else {
            builtin::Kind::Mip2D
        };
        if bits.is_some() {
            let entry = state.textures.get_mut(&id).unwrap();
            let layers = entry.texture.depth_or_array_layers();
            for level in 1..texture.mip_level_count() {
                for layer in 0..layers {
                    if !entry.stale.contains(&(level, layer)) {
                        entry.stale.push((level, layer));
                    }
                }
            }
        }
        if !texture
            .usage()
            .contains(w::TextureUsages::RENDER_ATTACHMENT)
        {
            unsupported(
                "mipmaps of a 3D texture of 32 bit floats on a device that cannot render into it",
            );
        }
        for level in 1..texture.mip_level_count() {
            if texture.dimension() == w::TextureDimension::D3 {
                let view = |level: u32| {
                    texture.create_view(&w::TextureViewDescriptor {
                        base_mip_level: level,
                        mip_level_count: Some(1),
                        ..Default::default()
                    })
                };
                let (source, destination) = (view(level - 1), view(level));
                for slice in 0..(texture.depth_or_array_layers() >> level).max(1) {
                    self.builtin_pass(
                        state,
                        builtin::Kind::Mip3D,
                        &source,
                        &destination,
                        format,
                        Some(slice),
                    );
                }
            } else {
                for layer in 0..texture.depth_or_array_layers() {
                    let source = self.flat(
                        state,
                        Slice {
                            texture: texture.clone(),
                            mip_level: level - 1,
                            layer,
                        },
                    );
                    self.builtin_pass(
                        state,
                        kind,
                        &source.view(),
                        &attachment_view(&texture, level, layer),
                        format,
                        None,
                    );
                }
            }
        }
    }

    fn create_renderbuffer(
        &self,
        format: TextureFormat,
        width: u32,
        height: u32,
        number_of_samples: u32,
    ) -> RenderbufferId {
        let (format, _) = texture_format(format);
        let texture = self.device.create_texture(&w::TextureDescriptor {
            label: None,
            size: w::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            // WebGPU only knows 1 and 4 samples
            sample_count: if number_of_samples > 1 { 4 } else { 1 },
            dimension: w::TextureDimension::D2,
            format,
            usage: self.multisample_usage(),
            view_formats: &[],
        });
        let mut state = self.lock();
        let id = Self::id(&mut state);
        state.renderbuffers.insert(id, texture);
        RenderbufferId::Wgpu(id)
    }

    fn delete_renderbuffer(&self, renderbuffer: RenderbufferId) {
        self.lock().renderbuffers.remove(&renderbuffer.wgpu());
    }

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
        let state = &mut *self.lock();
        state.passes.pop();
        if state.passes.is_empty() {
            self.flush(state);
        }
    }

    fn clear(&self, target: &PassTarget, scissor_box: ScissorBox, clear_state: ClearState) {
        let state = &mut *self.lock();
        let rectangle = clamp_to_target(target, scissor_box);
        if rectangle.2 == 0 || rectangle.3 == 0 {
            return;
        }
        let channels = [
            clear_state.red,
            clear_state.green,
            clear_state.blue,
            clear_state.alpha,
        ];
        let all = channels.iter().all(Option::is_some);
        let none = channels.iter().all(Option::is_none);
        if none && clear_state.depth.is_none() {
            return;
        }
        if rectangle == (0, 0, target.width, target.height) && (all || none) {
            // The clear is the load operation of a new pass
            self.pass(state, target, Some(clear_state));
        } else {
            self.clear_partially(state, target, rectangle, clear_state);
        }
    }

    fn copy_target(&self, source: &PassTarget, destination: &PassTarget, mask: CopyMask) {
        let state = &mut *self.lock();
        if mask.color {
            self.prepare_bits(state, source, false);
            self.prepare_bits(state, destination, true);
            let from = self.color_slices(state, source).into_iter().next();
            let to = self.color_slices(state, destination).into_iter().next();
            if let (Some(from), Some(to)) = (from, to) {
                if from.texture.sample_count() > to.texture.sample_count() {
                    self.resolve_color(state, &from, &to);
                } else {
                    let from = self.flat(state, from);
                    self.copy_slice(state, &from, &to);
                }
            }
        }
        if mask.depth {
            let from = self.depth_slice(state, source);
            let to = self.depth_slice(state, destination);
            if let (Some(from), Some(to)) = (from, to) {
                if self.gl {
                    // wgpu cannot copy depth on OpenGL, so the depth is drawn
                    let from = if from.texture.sample_count() > 1 {
                        self.resolved_depth(state, &from)
                    } else {
                        self.flat(state, from)
                    };
                    self.builtin_pass(
                        state,
                        builtin::Kind::DepthToDepth {
                            multisampled: false,
                        },
                        &from.view(),
                        &to.view(),
                        to.texture.format(),
                        None,
                    );
                } else if from.texture.sample_count() > to.texture.sample_count() {
                    // Like OpenGL, the depth of a pixel is the depth of one of its samples
                    self.builtin_pass(
                        state,
                        builtin::Kind::DepthToDepth { multisampled: true },
                        &from.view(),
                        &to.view(),
                        to.texture.format(),
                        None,
                    );
                } else {
                    self.copy_slice(state, &from, &to);
                }
            }
        }
    }

    fn read_pixels(&self, source: &PassTarget, rect: ScissorBox, format: ReadFormat) -> Vec<u8> {
        if cfg!(target_arch = "wasm32") {
            // A browser cannot block until the pixels are there
            unsupported("reading pixels synchronously in the browser, use the asynchronous functions like RenderTarget::read_color_async or pick_async instead");
        }
        self.start_read_pixels(source, rect, format).wait()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_pixels_into(
        &self,
        source: &PassTarget,
        rect: ScissorBox,
        format: ReadFormat,
        bytes: &mut [u8],
    ) {
        self.start_read_pixels(source, rect, format)
            .wait_into(bytes)
    }

    fn read_pixels_async_into(
        &self,
        source: &PassTarget,
        rect: ScissorBox,
        format: ReadFormat,
        storage: Box<dyn PixelStorage>,
    ) -> StorageFuture {
        let read = self.start_read_pixels(source, rect, format);
        let mut storage = Some(storage);
        Box::pin(std::future::poll_fn(move |context| {
            read.poll_mapped(context).map(|()| {
                let mut storage = storage
                    .take()
                    .expect("the pixels have already been handed out");
                read.finish_into(storage.bytes());
                storage
            })
        }))
    }

    fn read_pixels_async(
        &self,
        source: &PassTarget,
        rect: ScissorBox,
        format: ReadFormat,
    ) -> PixelsFuture {
        Box::pin(self.start_read_pixels(source, rect, format))
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
        let mut uniform_names = translation.vertex_uniforms;
        uniform_names.extend(translation.fragment_uniforms);
        Ok(self.insert_program(ProgramParts {
            number_of_outputs: number_of_outputs(&fragment_module),
            vertex: vertex_module,
            fragment: Some(fragment_module),
            entry_points: ["main".to_string(), "main".to_string()],
            members: [vertex_members, fragment_members],
            sizes: [vertex_size, fragment_size],
            block_visibility: [w::ShaderStages::VERTEX, w::ShaderStages::FRAGMENT],
            uniform_names,
            samplers: translation.samplers,
            attributes: translation.attributes,
            uniform_blocks: translation.uniform_blocks,
            storage: None,
        }))
    }

    #[cfg(feature = "wgsl")]
    fn create_compute_program(
        &self,
        source: &str,
    ) -> Result<(ProgramId, ProgramReflection), CoreError> {
        let shader = wgsl::parse(source, true).map_err(|e| shader_error("WGSL", source, e))?;
        let (members, size) = block_members(&shader.module, 0);
        let (more_members, more_size) = block_members(&shader.module, 1);
        let mut uniform_names: Vec<String> =
            members.keys().chain(more_members.keys()).cloned().collect();
        uniform_names.sort();
        if let Some(name) = uniform_names.windows(2).find(|n| n[0] == n[1]) {
            return Err(shader_error(
                "WGSL",
                source,
                format!("the uniform {} is in both uniform structs", name[0]),
            ));
        }
        let storage = shader
            .storage
            .into_iter()
            .map(|(name, format)| {
                let format = match format {
                    naga::StorageFormat::Rgba8Unorm => w::TextureFormat::Rgba8Unorm,
                    naga::StorageFormat::Rgba16Float => w::TextureFormat::Rgba16Float,
                    naga::StorageFormat::Rgba32Float => w::TextureFormat::Rgba32Float,
                    _ => w::TextureFormat::R32Float,
                };
                (name, format)
            })
            .collect();
        Ok(self.insert_program(ProgramParts {
            number_of_outputs: 0,
            vertex: shader.module,
            fragment: None,
            entry_points: shader.entry_points,
            members: [members, more_members],
            sizes: [size, more_size],
            block_visibility: [w::ShaderStages::COMPUTE; 2],
            uniform_names,
            samplers: shader.samplers,
            attributes: Vec::new(),
            uniform_blocks: shader.uniform_blocks,
            storage: Some(storage),
        }))
    }

    #[cfg(feature = "wgsl")]
    fn bind_storage_texture(&self, program: ProgramId, name: &str, texture: TextureId) -> bool {
        let mut state = self.lock();
        let compute = state
            .programs
            .get_mut(&program.wgpu())
            .and_then(|program| program.compute.as_mut())
            .expect("not a compute program");
        match compute.storage.iter_mut().find(|(n, ..)| n == name) {
            Some(slot) => {
                slot.2 = Some(texture.wgpu());
                true
            }
            None => false,
        }
    }

    #[cfg(feature = "wgsl")]
    fn dispatch(&self, program: ProgramId, groups: [u32; 3]) {
        let program_id = program.wgpu();
        let state = &mut *self.lock();
        let bindings = self.bindings(state, program_id);

        // The program writes to the textures themselves, so their bits are created again when they are needed, see [bits]
        let written: Vec<u32> = state.programs[&program_id]
            .compute
            .as_ref()
            .unwrap()
            .storage
            .iter()
            .filter_map(|(_, _, texture)| *texture)
            .collect();
        for texture in written {
            self.synchronize_bits(state, texture);
            if let Some(entry) = state.textures.get_mut(&texture) {
                entry.bits = None;
                entry.has_content = true;
            }
        }

        // The textures the program writes to
        let compute = state.programs[&program_id].compute.as_ref().unwrap();
        let views: Vec<w::TextureView> = compute
            .storage
            .iter()
            .map(|(name, format, texture)| {
                let entry = texture
                    .and_then(|texture| state.textures.get(&texture))
                    .unwrap_or_else(|| panic!("no texture is bound to the storage texture {name}"));
                if entry.texture.format() != *format
                    || entry.texture.dimension() != w::TextureDimension::D2
                    || entry.texture.depth_or_array_layers() != 1
                    || !entry.texture.usage().contains(w::TextureUsages::STORAGE_BINDING)
                {
                    panic!(
                        "the texture bound to the storage texture {name} must be a 2D texture with the format {format:?}, it has the format {:?}",
                        entry.texture.format()
                    );
                }
                entry.texture.create_view(&w::TextureViewDescriptor {
                    dimension: Some(w::TextureViewDimension::D2),
                    base_mip_level: 0,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let storage_group = self.device.create_bind_group(&w::BindGroupDescriptor {
            label: None,
            layout: &compute.storage_layout,
            entries: &views
                .iter()
                .enumerate()
                .map(|(binding, view)| w::BindGroupEntry {
                    binding: binding as u32,
                    resource: w::BindingResource::TextureView(view),
                })
                .collect::<Vec<_>>(),
        });

        let program = state.programs.get_mut(&program_id).unwrap();
        if !program
            .compute
            .as_ref()
            .unwrap()
            .pipelines
            .contains_key(&bindings.sample_kinds)
        {
            let compute = program.compute.as_ref().unwrap();
            let layout = self
                .device
                .create_pipeline_layout(&w::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[
                        Some(&program.uniform_layout),
                        Some(&program.texture_layouts[&bindings.sample_kinds]),
                        Some(&program.user_block_layout),
                        Some(&compute.storage_layout),
                    ],
                    immediate_size: 0,
                });
            let pipeline = self
                .device
                .create_compute_pipeline(&w::ComputePipelineDescriptor {
                    label: None,
                    layout: Some(&layout),
                    module: &program.vertex,
                    entry_point: Some(&program.entry_points[0]),
                    compilation_options: Default::default(),
                    cache: None,
                });
            program
                .compute
                .as_mut()
                .unwrap()
                .pipelines
                .insert(bindings.sample_kinds.clone(), pipeline);
        }
        let pipeline = program.compute.as_ref().unwrap().pipelines[&bindings.sample_kinds].clone();
        let empty_blocks = bindings.user_block_group.is_none().then(|| {
            self.device.create_bind_group(&w::BindGroupDescriptor {
                label: None,
                layout: &program.user_block_layout,
                entries: &[],
            })
        });

        // Recorded after what was drawn so far, a render pass that is open ends here
        let mut pass = self
            .encoder(state)
            .begin_compute_pass(&w::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings.uniform_group, &bindings.dynamic_offsets);
        pass.set_bind_group(1, &bindings.texture_group, &[]);
        pass.set_bind_group(
            2,
            bindings.user_block_group.as_ref().or(empty_blocks.as_ref()),
            &[],
        );
        pass.set_bind_group(3, &storage_group, &[]);
        pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
    }

    #[cfg(feature = "wgsl")]
    fn create_program_wgsl(
        &self,
        source: &str,
    ) -> Result<(ProgramId, ProgramReflection), CoreError> {
        let shader = wgsl::parse(source, false).map_err(|e| shader_error("WGSL", source, e))?;
        let (members, size) = block_members(&shader.module, 0);
        let (more_members, more_size) = block_members(&shader.module, 1);
        let mut uniform_names: Vec<String> =
            members.keys().chain(more_members.keys()).cloned().collect();
        uniform_names.sort();
        if let Some(name) = uniform_names.windows(2).find(|n| n[0] == n[1]) {
            return Err(shader_error(
                "WGSL",
                source,
                format!("the uniform {} is in both uniform structs", name[0]),
            ));
        }
        Ok(self.insert_program(ProgramParts {
            number_of_outputs: number_of_outputs(&shader.module),
            vertex: shader.module,
            fragment: None,
            entry_points: shader.entry_points,
            members: [members, more_members],
            sizes: [size, more_size],
            block_visibility: [w::ShaderStages::VERTEX_FRAGMENT; 2],
            uniform_names,
            samplers: shader.samplers,
            attributes: shader.attributes,
            uniform_blocks: shader.uniform_blocks,
            storage: None,
        }))
    }

    fn delete_program(&self, program: ProgramId) {
        let state = &mut *self.lock();
        let id = program.wgpu();
        state.programs.remove(&id);
        state.pipelines.retain(|key, _| key.program != id);
        state
            .texture_groups
            .retain(|(program, _), _| *program != id);
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
                for member in members {
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
                program.uploaded = None;
            }
        }
    }

    fn bind_texture(&self, unit: u32, texture: TextureId, _kind: TextureKind) {
        self.lock().units.insert(unit, texture.wgpu());
    }

    fn uniform_block_index(&self, program: ProgramId, name: &str) -> Option<u32> {
        self.lock().programs[&program.wgpu()]
            .user_blocks
            .iter()
            .position(|(block, _)| block == name)
            .map(|index| index as u32)
    }

    fn bind_uniform_block(
        &self,
        program: ProgramId,
        block_index: u32,
        _binding: u32,
        buffer: BufferId,
    ) {
        // There are no global binding points, the buffer is bound to the block of the program directly
        self.lock()
            .programs
            .get_mut(&program.wgpu())
            .unwrap()
            .user_blocks[block_index as usize]
            .1 = Some(buffer.wgpu());
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
        let state = &mut *self.lock();
        let (target, scissor_box) = state
            .passes
            .last()
            .cloned()
            .expect("a draw call must be made inside the write method of a render target");
        let scissor = clamp_to_target(&target, scissor_box);
        if scissor.2 == 0 || scissor.3 == 0 || call.viewport.width == 0 || call.viewport.height == 0
        {
            return;
        }
        let program_id = call.program.wgpu();

        // Vertex buffers, one per attribute
        let program = &state.programs[&program_id];
        let bound: Vec<(u32, u32, VertexFormat, bool)> = program
            .attribute_locations
            .iter()
            .map(|location| {
                let (buffer, format, divisor) =
                    program.attributes.get(location).unwrap_or_else(|| {
                        panic!("no buffer is bound to the vertex attribute at location {location}")
                    });
                (*location, *buffer, *format, *divisor != 0)
            })
            .collect();
        // If there are more attributes than vertex buffers are allowed, the attributes per instance share one buffer
        let merge = bound.len() > self.max_vertex_buffers;
        let mut vertex_buffers = Vec::new();
        let mut vertex_layouts = Vec::new();
        for (location, buffer, format, instanced) in &bound {
            if merge && *instanced {
                continue;
            }
            vertex_buffers.push(
                state.buffers[buffer]
                    .buffer
                    .clone()
                    .expect("a vertex buffer is used before it is filled"),
            );
            let format = vertex_format(*format);
            vertex_layouts.push((*instanced, format.size(), vec![(*location, format, 0)]));
        }
        if merge {
            let instanced: Vec<_> = bound.iter().filter(|a| a.3).collect();
            if vertex_buffers.len() + 1 > self.max_vertex_buffers || instanced.is_empty() {
                unsupported(&format!(
                    "a draw call with more than {} vertex attributes that are not per instance",
                    self.max_vertex_buffers - 1
                ));
            }
            let mut offset = 0;
            let mut layout = Vec::new();
            let mut parts = Vec::new();
            for (location, buffer, format, _) in &instanced {
                let wgpu_format = vertex_format(*format);
                layout.push((*location, wgpu_format, offset));
                offset += wgpu_format.size();
                parts.push((*buffer, *format, wgpu_format.size() as usize));
            }
            let key: Vec<u32> = parts.iter().map(|(buffer, _, _)| *buffer).collect();
            if !state.merged_buffers.contains_key(&key) {
                let contents: Vec<(Vec<u8>, usize)> = parts
                    .iter()
                    .map(|(buffer, format, size)| {
                        let bytes = state.buffers[buffer]
                            .bytes
                            .as_ref()
                            .expect("an instance buffer is used before it is filled");
                        let stored = if stored_components(*format) != format.components {
                            widen_vertices(bytes, *format)
                        } else {
                            bytes.clone()
                        };
                        (stored, *size)
                    })
                    .collect();
                let count = contents
                    .iter()
                    .map(|(bytes, size)| bytes.len() / size)
                    .min()
                    .unwrap_or(0);
                let mut interleaved = Vec::with_capacity(count * offset as usize);
                for instance in 0..count {
                    for (bytes, size) in &contents {
                        interleaved
                            .extend_from_slice(&bytes[instance * size..(instance + 1) * size]);
                    }
                }
                let buffer = self.new_buffer(&interleaved, w::BufferUsages::VERTEX);
                state.merged_buffers.insert(key.clone(), buffer);
            }
            vertex_buffers.push(state.merged_buffers[&key].clone());
            vertex_layouts.push((true, offset, layout));
        }
        // Uniforms, textures and uniform blocks. This can submit the recorded commands, so it comes before the pass is looked up
        let Bindings {
            sample_kinds,
            texture_group,
            uniform_group,
            dynamic_offsets,
            user_block_group,
        } = self.bindings(state, program_id);

        let index_buffer = call.indices.map(|indices| {
            let entry = state.buffers.get_mut(&indices.buffer.wgpu()).unwrap();
            match indices.index_type {
                PrimitiveType::U8 => {
                    // 8 bit indices do not exist in wgpu
                    if entry.wide_indices.is_none() {
                        let wide: Vec<u8> = entry
                            .bytes
                            .as_ref()
                            .expect("an element buffer is used before it is filled")
                            .iter()
                            .flat_map(|index| (*index as u16).to_le_bytes())
                            .collect();
                        entry.wide_indices = Some(self.new_buffer(&wide, w::BufferUsages::INDEX));
                    }
                    (entry.wide_indices.clone().unwrap(), w::IndexFormat::Uint16)
                }
                index_type => (
                    entry
                        .buffer
                        .clone()
                        .expect("an element buffer is used before it is filled"),
                    if index_type == PrimitiveType::U16 {
                        w::IndexFormat::Uint16
                    } else {
                        w::IndexFormat::Uint32
                    },
                ),
            }
        });

        let formats = self.pass(state, &target, None).formats.clone();
        let write_mask = call.render_states.write_mask;
        let key = PipelineKey {
            program: program_id,
            cull: call.render_states.cull as u8,
            depth_test: call.render_states.depth_test as u8,
            write_mask: color_writes(write_mask).bits() as u8 | (u8::from(write_mask.depth) << 4),
            blend: blend_key(call.render_states.blend),
            formats,
            vertex_buffers: vertex_layouts,
            textures: sample_kinds,
        };
        if !state.pipelines.contains_key(&key) {
            // The color targets that take the bits of floats, see [bits]
            let bits_mask = key
                .formats
                .colors
                .iter()
                .enumerate()
                .fold(0, |mask, (index, format)| {
                    mask | (u32::from(bits::is_bits(*format)) << index)
                });
            let bits_fragment = (bits_mask != 0).then(|| {
                let program = state.programs.get_mut(&program_id).unwrap();
                if !program.bits_fragments.contains_key(&bits_mask) {
                    let mut module = program
                        .fragment_source
                        .clone()
                        .expect("the program has no fragment shader that can write to this target");
                    bits::output_bits(&mut module, &program.entry_points[1], bits_mask);
                    let module = self.shader_module(module);
                    program.bits_fragments.insert(bits_mask, module);
                }
                program.bits_fragments[&bits_mask].clone()
            });
            let program = &state.programs[&program_id];
            let mut bind_group_layouts = vec![
                Some(&program.uniform_layout),
                Some(&program.texture_layouts[&key.textures]),
            ];
            if !program.user_blocks.is_empty() {
                bind_group_layouts.push(Some(&program.user_block_layout));
            }
            let layout = self
                .device
                .create_pipeline_layout(&w::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &bind_group_layouts,
                    immediate_size: 0,
                });
            let vertex_attributes: Vec<Vec<w::VertexAttribute>> = key
                .vertex_buffers
                .iter()
                .map(|(_, _, attributes)| {
                    attributes
                        .iter()
                        .map(|(location, format, offset)| w::VertexAttribute {
                            format: *format,
                            offset: *offset,
                            shader_location: *location,
                        })
                        .collect()
                })
                .collect();
            let vertex_layouts: Vec<_> = key
                .vertex_buffers
                .iter()
                .zip(&vertex_attributes)
                .map(
                    |((instanced, stride, _), attributes)| w::VertexBufferLayout {
                        array_stride: *stride,
                        step_mode: if *instanced {
                            w::VertexStepMode::Instance
                        } else {
                            w::VertexStepMode::Vertex
                        },
                        attributes,
                    },
                )
                .collect();
            let targets: Vec<_> = key
                .formats
                .colors
                .iter()
                .enumerate()
                .map(|(index, format)| {
                    Some(w::ColorTargetState {
                        format: *format,
                        // Integers cannot be blended
                        blend: blend_state(call.render_states.blend)
                            .filter(|_| !bits::is_bits(*format)),
                        // A target the fragment shader does not write to keeps its content
                        write_mask: if index < program.number_of_outputs {
                            color_writes(write_mask)
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
                        entry_point: Some(&program.entry_points[0]),
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
                    depth_stencil: key.formats.depth.map(|format| w::DepthStencilState {
                        format,
                        depth_write_enabled: Some(write_mask.depth),
                        depth_compare: Some(compare_function(call.render_states.depth_test)),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: w::MultisampleState {
                        count: key.formats.samples,
                        ..Default::default()
                    },
                    fragment: Some(w::FragmentState {
                        module: bits_fragment.as_ref().unwrap_or(&program.fragment),
                        entry_point: Some(&program.entry_points[1]),
                        compilation_options: Default::default(),
                        targets: &targets,
                    }),
                    multiview_mask: None,
                    cache: None,
                });
            state.pipelines.insert(key.clone(), pipeline);
        }
        let pipeline = &state.pipelines[&key];

        let pass = &mut state.frame.pass.as_mut().unwrap().pass;
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &uniform_group, &dynamic_offsets);
        pass.set_bind_group(1, &texture_group, &[]);
        if let Some(group) = &user_block_group {
            pass.set_bind_group(2, group, &[]);
        }
        pass.set_viewport(
            call.viewport.x as f32,
            call.viewport.y as f32,
            call.viewport.width as f32,
            call.viewport.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
        for (slot, buffer) in vertex_buffers.iter().enumerate() {
            pass.set_vertex_buffer(slot as u32, buffer.slice(..));
        }
        let instances = 0..call.instances.unwrap_or(1);
        match (&index_buffer, call.indices) {
            (Some((buffer, format)), Some(indices)) => {
                pass.set_index_buffer(buffer.slice(..), *format);
                pass.draw_indexed(indices.first..indices.first + call.count, 0, instances);
            }
            _ => pass.draw(0..call.count, instances),
        }
    }
}

impl WgpuBackend {
    /// Records the commands that copy the pixels and submits them.
    fn start_read_pixels(
        &self,
        source: &PassTarget,
        rect: ScissorBox,
        format: ReadFormat,
    ) -> PendingRead {
        let state = &mut *self.lock();
        match format {
            ReadFormat::Color(format) => {
                self.prepare_bits(state, source, false);
                let mut slice = self
                    .color_slices(state, source)
                    .into_iter()
                    .next()
                    .expect("the target that is read from has no color");
                if slice.texture.sample_count() > 1 {
                    let resolved =
                        self.temporary_texture(slice.texture.format(), source.width, source.height);
                    self.resolve_color(state, &slice, &resolved);
                    slice = resolved;
                }
                let slice = self.flat(state, slice);
                let (primitive, channels) = texel_layout(slice.texture.format());
                if primitive != format.primitive {
                    unsupported("reading pixels as another type than the texture has");
                }
                let mut read =
                    self.start_read(state, &slice, rect, channels * primitive_size(primitive));
                read.convert = Some((primitive, channels, format.channels as usize));
                read
            }
            ReadFormat::Depth => {
                let slice = self
                    .depth_slice(state, source)
                    .expect("the target that is read from has no depth");
                let slice = if self.gl && slice.texture.sample_count() > 1 {
                    self.resolved_depth(state, &slice)
                } else {
                    self.flat(state, slice)
                };
                // Not every depth format can be copied into a buffer, so the depth is drawn into a color texture first.
                // Its texels are the bits of the depth, since OpenGL cannot always render into a texture of 32 bit floats.
                let color =
                    self.temporary_texture(w::TextureFormat::R32Uint, source.width, source.height);
                self.builtin_pass(
                    state,
                    builtin::Kind::DepthToColor {
                        multisampled: slice.texture.sample_count() > 1,
                    },
                    &slice.view(),
                    &color.view(),
                    w::TextureFormat::R32Uint,
                    None,
                );
                self.start_read(state, &color, rect, 4)
            }
        }
    }
}

/// What is needed to create a program, from translated GLSL or from WGSL.
struct ProgramParts {
    vertex: naga::Module,
    /// `None` if the fragment shader is in the same module as the vertex shader.
    fragment: Option<naga::Module>,
    /// The names of the entry points of the vertex and fragment shader.
    entry_points: [String; 2],
    /// The members and the sizes of the two uniform blocks with the values set by name, and the stages which see them.
    members: [HashMap<String, Member>; 2],
    sizes: [usize; 2],
    block_visibility: [w::ShaderStages; 2],
    uniform_names: Vec<String>,
    samplers: Vec<glsl::SamplerDecl>,
    attributes: Vec<(String, u32)>,
    uniform_blocks: Vec<String>,
    number_of_outputs: usize,
    /// For a compute program, the names and formats of the textures it writes to.
    storage: Option<Vec<(String, w::TextureFormat)>>,
}

/// The number of color outputs of the fragment shader.
fn number_of_outputs(module: &naga::Module) -> usize {
    module
        .entry_points
        .iter()
        .find(|entry| entry.stage == naga::ShaderStage::Fragment)
        .and_then(|entry| entry.function.result.as_ref())
        .map(|result| match &module.types[result.ty].inner {
            naga::TypeInner::Struct { members, .. } => members
                .iter()
                .filter(|m| matches!(m.binding, Some(naga::Binding::Location { .. })))
                .count(),
            _ => 1,
        })
        .unwrap_or(0)
}

impl WgpuBackend {
    fn insert_program(&self, parts: ProgramParts) -> (ProgramId, ProgramReflection) {
        let ProgramParts {
            vertex,
            fragment,
            entry_points,
            members: [vertex_members, fragment_members],
            sizes: [vertex_size, fragment_size],
            block_visibility,
            uniform_names,
            samplers,
            attributes,
            uniform_blocks,
            number_of_outputs,
            storage,
        } = parts;
        let mut reflection = ProgramReflection::default();
        let mut uniforms = Vec::new();
        for name in &uniform_names {
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
        for (index, sampler) in samplers.iter().enumerate() {
            reflection.uniforms.insert(
                sampler.name.clone(),
                UniformLocation::Wgpu(uniforms.len() as u32),
            );
            uniforms.push(UniformSlot::Sampler(index));
        }
        reflection.attributes = attributes.iter().cloned().collect();

        // The blocks with the loose uniforms live in the ring buffer, at an offset that changes with every draw call
        let block_entry =
            |binding: u32, visibility: w::ShaderStages, size: usize| w::BindGroupLayoutEntry {
                binding,
                visibility,
                ty: w::BindingType::Buffer {
                    ty: w::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(size as u64),
                },
                count: None,
            };
        let mut entries = Vec::new();
        if vertex_size > 0 {
            entries.push(block_entry(0, block_visibility[0], vertex_size));
        }
        if fragment_size > 0 {
            entries.push(block_entry(1, block_visibility[1], fragment_size));
        }
        let uniform_layout = self
            .device
            .create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                label: None,
                entries: &entries,
            });
        // The uniform blocks declared in the shaders are whole buffers
        let user_block_entries: Vec<_> = (0..uniform_blocks.len())
            .map(|binding| w::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: if storage.is_some() {
                    w::ShaderStages::COMPUTE
                } else {
                    w::ShaderStages::VERTEX_FRAGMENT
                },
                ty: w::BindingType::Buffer {
                    ty: w::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let user_block_layout =
            self.device
                .create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &user_block_entries,
                });

        let fragment_source =
            (!self.float32_renderable && number_of_outputs > 0 && storage.is_none())
                .then(|| fragment.as_ref().unwrap_or(&vertex).clone());
        let vertex = self.shader_module(vertex);
        let fragment = match fragment {
            Some(fragment) => self.shader_module(fragment),
            // Both stages are in the same module
            None => vertex.clone(),
        };
        let compute = storage.map(|storage| ComputeEntry {
            storage_layout: self
                .device
                .create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &storage
                        .iter()
                        .enumerate()
                        .map(|(binding, (_, format))| w::BindGroupLayoutEntry {
                            binding: binding as u32,
                            visibility: w::ShaderStages::COMPUTE,
                            ty: w::BindingType::StorageTexture {
                                access: w::StorageTextureAccess::WriteOnly,
                                format: *format,
                                view_dimension: w::TextureViewDimension::D2,
                            },
                            count: None,
                        })
                        .collect::<Vec<_>>(),
                }),
            storage: storage
                .into_iter()
                .map(|(name, format)| (name, format, None))
                .collect(),
            pipelines: HashMap::new(),
        });
        let entry = ProgramEntry {
            vertex,
            fragment,
            entry_points,
            compute,
            blocks: [vec![0; vertex_size], vec![0; fragment_size]],
            uploaded: None,
            uniforms,
            samplers: samplers.into_iter().map(|s| (s, 0)).collect(),
            attributes: HashMap::new(),
            attribute_locations: {
                let mut locations: Vec<u32> = attributes.iter().map(|(_, l)| *l).collect();
                locations.sort();
                locations
            },
            uniform_layout,
            uniform_group: None,
            texture_layouts: HashMap::new(),
            user_blocks: uniform_blocks
                .into_iter()
                .map(|name| (name, None))
                .collect(),
            user_block_layout,
            number_of_outputs,
            fragment_source,
            bits_fragments: HashMap::new(),
        };
        let mut state = self.lock();
        let id = Self::id(&mut state);
        state.programs.insert(id, entry);
        (ProgramId::Wgpu(id), reflection)
    }
}

/// The bind groups of a program with what is currently bound to it.
struct Bindings {
    sample_kinds: Vec<SampleKind>,
    texture_group: w::BindGroup,
    uniform_group: w::BindGroup,
    /// Where the uniform blocks of the program are in the ring buffer.
    dynamic_offsets: Vec<u32>,
    user_block_group: Option<w::BindGroup>,
}

impl WgpuBackend {
    ///
    /// Creates or looks up the bind groups for the uniforms, textures and uniform blocks of the program.
    /// Submits the recorded commands if the ring buffer for the uniforms is full.
    ///
    fn bindings(&self, state: &mut State, program_id: u32) -> Bindings {
        if !self.float32_renderable {
            // What was rendered into the textures must be there when they are sampled, see [bits]
            let textures: Vec<u32> = state.programs[&program_id]
                .samplers
                .iter()
                .filter_map(|(_, unit)| state.units.get(unit).copied())
                .collect();
            for texture in textures {
                self.synchronize_bits(state, texture);
            }
        }
        let program = &state.programs[&program_id];

        // Textures and samplers
        let mut sample_kinds = Vec::new();
        let mut textures = Vec::new();
        for (sampler, unit) in &program.samplers {
            let texture = state
                .units
                .get(unit)
                .filter(|texture| state.textures.contains_key(texture))
                .unwrap_or_else(|| panic!("no texture is bound for the uniform {}", sampler.name));
            sample_kinds.push(match state.textures[texture].format {
                TextureFormat::Depth(_) => SampleKind::Unfilterable,
                TextureFormat::Color(ColorFormat {
                    primitive: PrimitiveType::F32,
                    ..
                }) if !self.float32_filterable => SampleKind::Unfilterable,
                _ => SampleKind::Filterable,
            });
            textures.push(*texture);
        }
        let texture_key = (program_id, textures);
        if !state.texture_groups.contains_key(&texture_key) {
            let program = state.programs.get_mut(&program_id).unwrap();
            let compute = program.compute.is_some();
            let samplers = &program.samplers;
            let layout = program
                .texture_layouts
                .entry(sample_kinds.clone())
                .or_insert_with(|| {
                    let entries: Vec<_> = samplers
                        .iter()
                        .zip(&sample_kinds)
                        .enumerate()
                        .flat_map(|(index, ((sampler, _), kind))| {
                            let mut visibility = w::ShaderStages::empty();
                            if compute {
                                visibility |= w::ShaderStages::COMPUTE;
                            }
                            if sampler.in_vertex {
                                visibility |= w::ShaderStages::VERTEX;
                            }
                            if sampler.in_fragment {
                                visibility |= w::ShaderStages::FRAGMENT;
                            }
                            [
                                w::BindGroupLayoutEntry {
                                    binding: 2 * index as u32,
                                    visibility,
                                    ty: w::BindingType::Texture {
                                        sample_type: match kind {
                                            SampleKind::Filterable => {
                                                w::TextureSampleType::Float { filterable: true }
                                            }
                                            SampleKind::Unfilterable => {
                                                w::TextureSampleType::Float { filterable: false }
                                            }
                                        },
                                        view_dimension: texture_view_dimension(&sampler.glsl_type),
                                        multisampled: false,
                                    },
                                    count: None,
                                },
                                w::BindGroupLayoutEntry {
                                    binding: 2 * index as u32 + 1,
                                    visibility,
                                    ty: w::BindingType::Sampler(
                                        if *kind == SampleKind::Filterable {
                                            w::SamplerBindingType::Filtering
                                        } else {
                                            w::SamplerBindingType::NonFiltering
                                        },
                                    ),
                                    count: None,
                                },
                            ]
                        })
                        .collect();
                    self.device
                        .create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                            label: None,
                            entries: &entries,
                        })
                });
            let mut resources = Vec::new();
            for (((sampler, _), kind), texture) in
                samplers.iter().zip(&sample_kinds).zip(&texture_key.1)
            {
                let entry = state.textures.get_mut(texture).unwrap();
                let dimension = texture_view_dimension(&sampler.glsl_type);
                let view = entry
                    .views
                    .entry(dimension)
                    .or_insert_with(|| {
                        entry.texture.create_view(&w::TextureViewDescriptor {
                            dimension: Some(dimension),
                            ..Default::default()
                        })
                    })
                    .clone();
                let sampler = if *kind == SampleKind::Filterable {
                    entry.sampler.clone()
                } else {
                    entry.nearest_sampler.clone()
                };
                resources.push((view, sampler));
            }
            let entries: Vec<_> = resources
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
            let group = self.device.create_bind_group(&w::BindGroupDescriptor {
                label: None,
                layout,
                entries: &entries,
            });
            state.texture_groups.insert(texture_key.clone(), group);
        }
        let texture_group = state.texture_groups[&texture_key].clone();

        // Uniforms: the blocks are only uploaded again if they changed since the last draw call with this program
        let offsets = match state.programs[&program_id].uploaded {
            Some((epoch, offsets)) if epoch == state.epoch => offsets,
            _ => {
                let blocks =
                    std::mem::take(&mut state.programs.get_mut(&program_id).unwrap().blocks);
                // Both blocks in one piece, so that the first one cannot be submitted away while the second one is added
                let second = blocks[0].len().next_multiple_of(self.uniform_alignment);
                let mut bytes = blocks[0].clone();
                bytes.resize(second, 0);
                bytes.extend_from_slice(&blocks[1]);
                let first = self.upload_uniforms(state, &bytes);
                let offsets = [first, first + second as u32];
                let program = state.programs.get_mut(&program_id).unwrap();
                program.blocks = blocks;
                program.uploaded = Some((state.epoch, offsets));
                offsets
            }
        };
        let generation = state.ring.generation;
        let program = state.programs.get_mut(&program_id).unwrap();
        if !matches!(&program.uniform_group, Some((g, _)) if *g == generation) {
            let entries: Vec<_> = program
                .blocks
                .iter()
                .enumerate()
                .filter(|(_, bytes)| !bytes.is_empty())
                .map(|(binding, bytes)| w::BindGroupEntry {
                    binding: binding as u32,
                    resource: w::BindingResource::Buffer(w::BufferBinding {
                        buffer: &state.ring.buffer,
                        offset: 0,
                        size: NonZeroU64::new(bytes.len() as u64),
                    }),
                })
                .collect();
            let group = self.device.create_bind_group(&w::BindGroupDescriptor {
                label: None,
                layout: &program.uniform_layout,
                entries: &entries,
            });
            program.uniform_group = Some((generation, group));
        }
        let uniform_group = program.uniform_group.as_ref().unwrap().1.clone();
        let dynamic_offsets: Vec<u32> = (0..2)
            .filter(|block| !program.blocks[*block].is_empty())
            .map(|block| offsets[block])
            .collect();

        // The uniform blocks declared in the shaders
        let user_block_group = (!program.user_blocks.is_empty()).then(|| {
            let buffers: Vec<_> = program
                .user_blocks
                .iter()
                .map(|(name, buffer)| {
                    buffer
                        .and_then(|buffer| state.buffers.get(&buffer))
                        .and_then(|entry| entry.buffer.clone())
                        .unwrap_or_else(|| panic!("no buffer is bound to the uniform block {name}"))
                })
                .collect();
            let entries: Vec<_> = buffers
                .iter()
                .enumerate()
                .map(|(binding, buffer)| w::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            self.device.create_bind_group(&w::BindGroupDescriptor {
                label: None,
                layout: &program.user_block_layout,
                entries: &entries,
            })
        });

        Bindings {
            sample_kinds,
            texture_group,
            uniform_group,
            dynamic_offsets,
            user_block_group,
        }
    }
}
