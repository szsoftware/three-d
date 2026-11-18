//!
//! The path tracer in compute shaders of the wgpu backend: the hierarchies of the CPU builder in storage buffers and
//! the sums of the samples in a buffer on the GPU.
//!

use crate::{
    AccelBuilder, Camera, CpuAccel, CpuBvhBuilder, GpuDevice, RaytraceSupport, RenderImage,
    RtEnvironment, RtError, RtLight, RtScene, TraceSettings, Tracer, TracerKind,
};
use three_d::wgpu as w;
use three_d::wgpu::util::DeviceExt;
use three_d_asset::prelude::*;
use three_d_asset::ProjectionType;

/// Size of a workgroup in both directions, as in the shaders.
const WORKGROUP: u32 = 8;

/// Bytes of one tile entry in the uniform buffer, before the alignment of dynamic offsets.
const TILE_BYTES: u64 = 32;

///
/// A path tracer in compute shaders on the wgpu backend, the same algorithm and material model as the
/// [CpuReferenceTracer](crate::CpuReferenceTracer).
///
/// The image is traced in tiles of [ComputeTracer::tile_size] pixels, one dispatch per tile and sample, so that a
/// single dispatch never runs long enough for the operating system to reset the GPU (about two seconds on Windows).
///
/// The fast way is [ComputeTracer::trace_gpu] into a [GpuRenderImage], which stays on the GPU and is shown with
/// [GpuRenderImage::write_to_three_d_texture]. The [Tracer] implementation reads every call back into a [RenderImage],
/// for exporting or comparing with the CPU, and is not available in a browser, where reading has to wait
/// ([GpuRenderImage::read_async]).
///
/// The random numbers differ from the CPU tracer (32 bit generator), so the images agree in their averages but not in
/// their noise.
///
#[derive(Debug)]
pub struct ComputeTracer {
    gpu: GpuDevice,
    builder: CpuBvhBuilder,
    accel: Option<CpuAccel>,
    buffers: Option<SceneBuffers>,
    layout: w::BindGroupLayout,
    pipeline: w::ComputePipeline,
    tile_size: u32,
    scratch: Option<GpuRenderImage>,
}

impl ComputeTracer {
    ///
    /// A tracer on the device. The device needs compute shaders, see [RaytraceSupport::of].
    ///
    pub fn new(gpu: &GpuDevice) -> Self {
        let device = &gpu.device;
        let uniform = |binding, dynamic, size| w::BindGroupLayoutEntry {
            binding,
            visibility: w::ShaderStages::COMPUTE,
            ty: w::BindingType::Buffer {
                ty: w::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: std::num::NonZeroU64::new(size),
            },
            count: None,
        };
        let storage = |binding, read_only| w::BindGroupLayoutEntry {
            binding,
            visibility: w::ShaderStages::COMPUTE,
            ty: w::BindingType::Buffer {
                ty: w::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&w::BindGroupLayoutDescriptor {
            label: Some("ray tracing"),
            entries: &[
                uniform(0, false, 0),
                uniform(1, true, TILE_BYTES),
                storage(2, true),
                storage(3, true),
                storage(4, true),
                storage(5, true),
                storage(6, true),
                storage(7, true),
                storage(8, false),
            ],
        });
        let module = device.create_shader_module(w::ShaderModuleDescriptor {
            label: Some("ray tracing"),
            source: w::ShaderSource::Wgsl(include_str!("compute/trace.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&w::PipelineLayoutDescriptor {
            label: Some("ray tracing"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&w::ComputePipelineDescriptor {
            label: Some("ray tracing"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("trace"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            gpu: gpu.clone(),
            builder: CpuBvhBuilder::default(),
            accel: None,
            buffers: None,
            layout,
            pipeline,
            tile_size: 256,
            scratch: None,
        }
    }

    /// A tracer on the device of a three-d context, `None` if the context can not run compute shaders.
    pub fn from_context(context: &three_d::Context) -> Option<Self> {
        if RaytraceSupport::of(context) < RaytraceSupport::Compute {
            return None;
        }
        GpuDevice::from_context(context).map(|gpu| Self::new(&gpu))
    }

    ///
    /// Width and height in pixels of the tiles traced by one dispatch, 256 by default. Smaller tiles keep each
    /// dispatch short on slow GPUs, larger ones need fewer dispatches.
    ///
    pub fn with_tile_size(mut self, tile_size: u32) -> Self {
        self.tile_size = tile_size.max(WORKGROUP);
        self
    }

    /// Width and height of the tiles.
    pub fn tile_size(&self) -> u32 {
        self.tile_size
    }

    /// The device the tracer runs on.
    pub fn gpu(&self) -> &GpuDevice {
        &self.gpu
    }

    ///
    /// Adds [TraceSettings::samples_per_call] samples to every pixel of a render image on the GPU. The work is queued
    /// and runs while the CPU goes on, the image's samples have to be from the same scene and camera.
    ///
    pub fn trace_gpu(
        &mut self,
        scene: &RtScene,
        camera: &Camera,
        image: &mut GpuRenderImage,
        settings: &TraceSettings,
    ) -> Result<(), RtError> {
        if image.width == 0 || image.height == 0 {
            return Err(RtError::EmptyImage);
        }
        self.prepare(scene)?;
        let buffers = self.buffers.as_ref().expect("prepared");
        let device = &self.gpu.device;

        let params = device.create_buffer_init(&w::util::BufferInitDescriptor {
            label: Some("ray tracing parameters"),
            contents: &params_bytes(scene, camera, image.width, image.height, settings),
            usage: w::BufferUsages::UNIFORM,
        });

        // One entry per dispatch: a tile and the sample it adds
        let stride = TILE_BYTES.max(device.limits().min_uniform_buffer_offset_alignment as u64);
        let mut tiles = Vec::new();
        for y in (0..image.height).step_by(self.tile_size as usize) {
            for x in (0..image.width).step_by(self.tile_size as usize) {
                tiles.push([
                    x,
                    y,
                    self.tile_size.min(image.width - x),
                    self.tile_size.min(image.height - y),
                ]);
            }
        }
        let samples = settings.samples_per_call;
        let mut tile_data =
            vec![0u8; (stride * tiles.len() as u64 * samples.max(1) as u64) as usize];
        for s in 0..samples {
            for (t, rect) in tiles.iter().enumerate() {
                let at = ((s as usize * tiles.len() + t) as u64 * stride) as usize;
                let words = [
                    rect[0],
                    rect[1],
                    rect[2],
                    rect[3],
                    image.samples + s,
                    1,
                    0,
                    0,
                ];
                for (i, word) in words.iter().enumerate() {
                    tile_data[at + 4 * i..at + 4 * i + 4].copy_from_slice(&word.to_le_bytes());
                }
            }
        }
        let tile_buffer = device.create_buffer_init(&w::util::BufferInitDescriptor {
            label: Some("ray tracing tiles"),
            contents: &tile_data,
            usage: w::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&w::BindGroupDescriptor {
            label: Some("ray tracing"),
            layout: &self.layout,
            entries: &[
                w::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 1,
                    resource: w::BindingResource::Buffer(w::BufferBinding {
                        buffer: &tile_buffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(TILE_BYTES),
                    }),
                },
                w::BindGroupEntry {
                    binding: 2,
                    resource: buffers.nodes.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 3,
                    resource: buffers.triangles.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 4,
                    resource: buffers.vertices.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 5,
                    resource: buffers.instances.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 6,
                    resource: buffers.materials.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 7,
                    resource: buffers.lights.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 8,
                    resource: image.sums.as_entire_binding(),
                },
            ],
        });

        // One submission per sample, so the GPU can go on with other work between them
        for s in 0..samples {
            let mut encoder = device.create_command_encoder(&w::CommandEncoderDescriptor {
                label: Some("ray tracing"),
            });
            {
                let mut pass = encoder.begin_compute_pass(&w::ComputePassDescriptor {
                    label: Some("ray tracing"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                for (t, rect) in tiles.iter().enumerate() {
                    let offset = (s as u64 * tiles.len() as u64 + t as u64) * stride;
                    pass.set_bind_group(0, &bind_group, &[offset as u32]);
                    pass.dispatch_workgroups(
                        rect[2].div_ceil(WORKGROUP),
                        rect[3].div_ceil(WORKGROUP),
                        1,
                    );
                }
            }
            self.gpu.queue.submit([encoder.finish()]);
        }
        image.samples += samples;
        Ok(())
    }
}

impl Tracer for ComputeTracer {
    fn kind(&self) -> TracerKind {
        TracerKind::Compute
    }

    fn prepare(&mut self, scene: &RtScene) -> Result<(), RtError> {
        let rebuilt = match &mut self.accel {
            Some(accel) if accel.is_up_to_date(scene) => false,
            Some(accel) => {
                self.builder.refit_instances(accel, scene)?;
                true
            }
            None => {
                self.accel = Some(self.builder.build(scene)?);
                true
            }
        };
        let current = self
            .buffers
            .as_ref()
            .is_some_and(|b| b.revision == scene.revision());
        if rebuilt || !current {
            self.buffers = Some(SceneBuffers::new(
                &self.gpu.device,
                scene,
                self.accel.as_ref().expect("built"),
            ));
        }
        Ok(())
    }

    ///
    /// Traces on the GPU and adds the samples to the render image, waiting for the GPU.
    ///
    /// # Errors
    /// In a browser, where waiting is not possible: use [ComputeTracer::trace_gpu] and [GpuRenderImage::read_async] there.
    ///
    fn trace(
        &mut self,
        scene: &RtScene,
        camera: &Camera,
        render_image: &mut RenderImage,
        settings: &TraceSettings,
    ) -> Result<(), RtError> {
        render_image.check_not_empty()?;
        if cfg!(target_arch = "wasm32") {
            return Err(RtError::Gpu(
                "a browser can not wait for the GPU, use trace_gpu and read_async".to_owned(),
            ));
        }
        let (width, height) = (render_image.width(), render_image.height());
        let mut scratch = match self.scratch.take() {
            Some(s) if s.width == width && s.height == height => s,
            _ => GpuRenderImage::new(&self.gpu, width, height),
        };
        scratch.reset();
        // Go on with the samples of the render image, so every call adds new ones
        scratch.samples = render_image.sample_count();
        let result = self.trace_gpu(scene, camera, &mut scratch, settings);
        if result.is_ok() {
            render_image.accumulate(&scratch.read_sums(), settings.samples_per_call);
        }
        self.scratch = Some(scratch);
        result
    }
}

///
/// Sums of samples per pixel on the GPU, filled by [ComputeTracer::trace_gpu]. Rows go from the top of the image to the
/// bottom, as in [RenderImage].
///
#[derive(Debug)]
pub struct GpuRenderImage {
    gpu: GpuDevice,
    width: u32,
    height: u32,
    samples: u32,
    sums: w::Buffer,
    resolve: Option<Resolve>,
}

impl GpuRenderImage {
    /// A black render image with the given size.
    pub fn new(gpu: &GpuDevice, width: u32, height: u32) -> Self {
        let sums = gpu.device.create_buffer(&w::BufferDescriptor {
            label: Some("render image"),
            size: (width.max(1) as u64) * (height.max(1) as u64) * 16,
            usage: w::BufferUsages::STORAGE | w::BufferUsages::COPY_SRC | w::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            gpu: gpu.clone(),
            width,
            height,
            samples: 0,
            sums,
            resolve: None,
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Number of samples every pixel has.
    pub fn sample_count(&self) -> u32 {
        self.samples
    }

    /// Forgets all samples, for example after the camera or the scene changed.
    pub fn reset(&mut self) {
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&w::CommandEncoderDescriptor {
                label: Some("reset render image"),
            });
        encoder.clear_buffer(&self.sums, 0, None);
        self.gpu.queue.submit([encoder.finish()]);
        self.samples = 0;
    }

    /// Changes the size and forgets all samples.
    pub fn resize(&mut self, width: u32, height: u32) {
        *self = Self::new(&self.gpu, width, height);
    }

    ///
    /// Copies the samples into a [RenderImage], waiting for the GPU. Not possible in a browser, use
    /// [GpuRenderImage::read_async] there.
    ///
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(&self) -> RenderImage {
        let mut render_image = RenderImage::new(self.width, self.height);
        render_image.accumulate(&self.read_sums(), self.samples);
        render_image
    }

    /// Copies the samples into a [RenderImage] once the GPU is done, also in a browser.
    pub async fn read_async(&self) -> RenderImage {
        let sums = self.start_read().await;
        let mut render_image = RenderImage::new(self.width, self.height);
        render_image.accumulate(&sums, self.samples);
        render_image
    }

    fn read_sums(&self) -> Vec<[f32; 3]> {
        let pending = self.start_read();
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.gpu.device.poll(w::PollType::wait_indefinitely());
        pending.finish()
    }

    fn start_read(&self) -> PendingSums {
        let size = self.sums.size();
        let staging = self.gpu.device.create_buffer(&w::BufferDescriptor {
            label: Some("read render image"),
            size,
            usage: w::BufferUsages::COPY_DST | w::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&w::CommandEncoderDescriptor {
                label: Some("read render image"),
            });
        encoder.copy_buffer_to_buffer(&self.sums, 0, &staging, 0, size);
        self.gpu.queue.submit([encoder.finish()]);
        let state = std::sync::Arc::new(std::sync::Mutex::new(MapState::default()));
        let shared = state.clone();
        staging
            .slice(..)
            .map_async(w::MapMode::Read, move |result| {
                let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
                shared.done = Some(result.is_ok());
                if let Some(waker) = shared.waker.take() {
                    waker.wake();
                }
            });
        PendingSums {
            device: self.gpu.device.clone(),
            buffer: staging,
            pixels: (self.width * self.height) as usize,
            state,
        }
    }

    ///
    /// Writes the average of every pixel into a texture of three-d with 32 bit float RGBA, as made by
    /// [RenderImage::to_three_d_texture], without going through the CPU. The texture shows the image upright.
    ///
    /// # Panics
    /// If the texture is not from a context with the wgpu backend on this device or has another size.
    ///
    pub fn write_to_three_d_texture(&mut self, texture: &three_d::Texture2D) {
        assert!(
            texture.width() == self.width && texture.height() == self.height,
            "the texture has another size than the render image"
        );
        let target = texture
            .as_wgpu()
            .expect("the texture is not from the wgpu backend");
        let resolve = self
            .resolve
            .get_or_insert_with(|| Resolve::new(&self.gpu.device, self.width, self.height));
        let pitch = resolve.pitch;
        let params: Vec<u8> = [
            self.width,
            self.height,
            pitch,
            0,
            (1.0 / self.samples.max(1) as f32).to_bits(),
            0,
            0,
            0,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
        self.gpu.queue.write_buffer(&resolve.params, 0, &params);
        let device = &self.gpu.device;
        let bind_group = device.create_bind_group(&w::BindGroupDescriptor {
            label: Some("resolve render image"),
            layout: &resolve.pipeline.get_bind_group_layout(0),
            entries: &[
                w::BindGroupEntry {
                    binding: 0,
                    resource: resolve.params.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 1,
                    resource: self.sums.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 2,
                    resource: resolve.averages.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&w::CommandEncoderDescriptor {
            label: Some("resolve render image"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&w::ComputePassDescriptor {
                label: Some("resolve render image"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&resolve.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(
                self.width.div_ceil(WORKGROUP),
                self.height.div_ceil(WORKGROUP),
                1,
            );
        }
        // Rows of wgpu textures start at the top like the render image, which three-d shows upright
        encoder.copy_buffer_to_texture(
            w::TexelCopyBufferInfo {
                buffer: &resolve.averages,
                layout: w::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch * 16),
                    rows_per_image: Some(self.height),
                },
            },
            w::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: w::Origin3d::ZERO,
                aspect: w::TextureAspect::All,
            },
            w::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.gpu.queue.submit([encoder.finish()]);
    }

    /// A three-d texture with the average of every pixel, see [GpuRenderImage::write_to_three_d_texture].
    pub fn to_three_d_texture(&mut self, context: &three_d::Context) -> three_d::Texture2D {
        let texture = three_d::Texture2D::new_empty::<[f32; 4]>(
            context,
            self.width,
            self.height,
            three_d::Interpolation::Nearest,
            three_d::Interpolation::Nearest,
            None,
            three_d::Wrapping::ClampToEdge,
            three_d::Wrapping::ClampToEdge,
        );
        self.write_to_three_d_texture(&texture);
        texture
    }
}

/// The pipeline and buffers that turn the sums into averages in rows a texture copy accepts.
#[derive(Debug)]
struct Resolve {
    pipeline: w::ComputePipeline,
    params: w::Buffer,
    averages: w::Buffer,
    /// Pixels per row, a multiple of 16 (256 bytes)
    pitch: u32,
}

impl Resolve {
    fn new(device: &w::Device, width: u32, height: u32) -> Self {
        let module = device.create_shader_module(w::ShaderModuleDescriptor {
            label: Some("resolve render image"),
            source: w::ShaderSource::Wgsl(include_str!("compute/resolve.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&w::ComputePipelineDescriptor {
            label: Some("resolve render image"),
            layout: None,
            module: &module,
            entry_point: Some("resolve"),
            compilation_options: Default::default(),
            cache: None,
        });
        let pitch = width.div_ceil(16) * 16;
        Self {
            pipeline,
            params: device.create_buffer(&w::BufferDescriptor {
                label: Some("resolve render image"),
                size: 32,
                usage: w::BufferUsages::UNIFORM | w::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            averages: device.create_buffer(&w::BufferDescriptor {
                label: Some("resolve render image"),
                size: pitch as u64 * height.max(1) as u64 * 16,
                usage: w::BufferUsages::STORAGE | w::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            pitch,
        }
    }
}

#[derive(Default)]
struct MapState {
    done: Option<bool>,
    waker: Option<std::task::Waker>,
}

/// Sums on their way from the GPU.
struct PendingSums {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    device: w::Device,
    buffer: w::Buffer,
    pixels: usize,
    state: std::sync::Arc<std::sync::Mutex<MapState>>,
}

impl PendingSums {
    fn finish(&self) -> Vec<[f32; 3]> {
        let ok = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .done
            .unwrap_or(false);
        if !ok {
            // The device was lost: no samples
            return vec![[0.0; 3]; self.pixels];
        }
        let data = self.buffer.slice(..).get_mapped_range();
        let floats: Vec<f32> = data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        drop(data);
        self.buffer.unmap();
        floats
            .as_chunks::<4>()
            .0
            .iter()
            .take(self.pixels)
            .map(|p| [p[0], p[1], p[2]])
            .collect()
    }
}

impl std::future::Future for PendingSums {
    type Output = Vec<[f32; 3]>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // A browser maps the buffer by itself, everywhere else the device has to be asked to do it
        #[cfg(not(target_arch = "wasm32"))]
        if self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .done
            .is_none()
        {
            let _ = self.device.poll(w::PollType::wait_indefinitely());
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.done.is_some() {
            drop(state);
            std::task::Poll::Ready(self.finish())
        } else {
            state.waker = Some(context.waker().clone());
            std::task::Poll::Pending
        }
    }
}

/// The scene in storage buffers, see trace.wgsl for the layouts.
#[derive(Debug)]
struct SceneBuffers {
    revision: u64,
    nodes: w::Buffer,
    triangles: w::Buffer,
    vertices: w::Buffer,
    instances: w::Buffer,
    materials: w::Buffer,
    lights: w::Buffer,
}

/// Words of a buffer, floats by their bits.
#[derive(Default)]
struct Words(Vec<u32>);

impl Words {
    fn f(&mut self, v: f32) {
        self.0.push(v.to_bits());
    }
    fn u(&mut self, v: u32) {
        self.0.push(v);
    }
    fn v3(&mut self, v: Vec3, w: f32) {
        self.f(v.x);
        self.f(v.y);
        self.f(v.z);
        self.f(w);
    }
    fn m4(&mut self, m: &Mat4) {
        let a: &[f32; 16] = m.as_ref();
        a.iter().for_each(|v| self.f(*v));
    }
    fn bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = self.0.iter().flat_map(|w| w.to_le_bytes()).collect();
        // Bindings can not be empty and have to hold at least one element of the largest struct
        if bytes.len() < 256 {
            bytes.resize(256, 0);
        }
        bytes
    }
}

impl SceneBuffers {
    fn new(device: &w::Device, scene: &RtScene, accel: &CpuAccel) -> Self {
        // Boxes without anything in them are infinite, which shaders do not compare reliably
        let finite = |v: f32| v.clamp(-1e30, 1e30);
        let mut nodes = Words::default();
        let push_nodes = |nodes: &mut Words, bvh: &crate::Bvh| {
            for n in &bvh.nodes {
                n.min.iter().for_each(|v| nodes.f(finite(*v)));
                nodes.u(n.first);
                n.max.iter().for_each(|v| nodes.f(finite(*v)));
                nodes.u(n.count);
            }
        };
        let top = accel.instance_hierarchy();
        push_nodes(&mut nodes, top);

        // The triangles of each mesh in the order of its leaves, so leaves refer to them directly
        let mut triangles = Words::default();
        let mut vertices = Words::default();
        let mut mesh_nodes = Vec::new();
        let mut mesh_triangles = Vec::new();
        for (mesh, bvh) in scene.meshes().iter().zip(accel.mesh_hierarchies()) {
            mesh_nodes.push((nodes.0.len() / 8) as u32);
            mesh_triangles.push((triangles.0.len() / 4) as u32);
            push_nodes(&mut nodes, bvh);
            let first_vertex = (vertices.0.len() / 8) as u32;
            for &t in &bvh.indices {
                let [a, b, c] = mesh.triangles[t as usize];
                triangles.u(first_vertex + a);
                triangles.u(first_vertex + b);
                triangles.u(first_vertex + c);
                triangles.u(t);
            }
            for (i, p) in mesh.positions.iter().enumerate() {
                vertices.v3(*p, 1.0);
                match &mesh.normals {
                    Some(normals) => vertices.v3(normals[i], 1.0),
                    None => vertices.v3(vec3(0.0, 0.0, 0.0), 0.0),
                }
            }
        }

        // The instances in the order of the leaves of the top level
        let mut instances = Words::default();
        for &i in &top.indices {
            let instance = &scene.instances()[i as usize];
            let material = &scene.materials()[instance.material.0 as usize];
            instances.m4(&instance.transform);
            instances.m4(&instance.inverse);
            instances.u(mesh_nodes[instance.mesh.0 as usize]);
            instances.u(mesh_triangles[instance.mesh.0 as usize]);
            instances.u(instance.material.0);
            let cut_out = material.alpha_cutout.is_some_and(|c| material.alpha < c);
            instances.u(u32::from(!cut_out));
        }

        let mut materials = Words::default();
        for m in scene.materials() {
            materials.v3(m.base_color, m.alpha);
            materials.f(m.metallic);
            materials.f(m.roughness);
            materials.f(m.specular);
            materials.f(m.transmission);
            materials.v3(m.emission, m.index_of_refraction);
            materials.v3(vec3(if m.thin_walled { 1.0 } else { 0.0 }, 0.0, 0.0), 0.0);
        }

        let mut lights = Words::default();
        for light in scene.lights() {
            match light {
                RtLight::Directional {
                    direction,
                    color,
                    illuminance,
                } => {
                    lights.v3(*direction, 0.0);
                    lights.v3(*color * *illuminance, 0.0);
                }
                RtLight::Point {
                    position,
                    color,
                    intensity,
                } => {
                    lights.v3(*position, 1.0);
                    lights.v3(*color * *intensity, 0.0);
                }
            }
        }

        let storage = |label, words: &Words| {
            device.create_buffer_init(&w::util::BufferInitDescriptor {
                label: Some(label),
                contents: &words.bytes(),
                usage: w::BufferUsages::STORAGE,
            })
        };
        Self {
            revision: scene.revision(),
            nodes: storage("ray tracing nodes", &nodes),
            triangles: storage("ray tracing triangles", &triangles),
            vertices: storage("ray tracing vertices", &vertices),
            instances: storage("ray tracing instances", &instances),
            materials: storage("ray tracing materials", &materials),
            lights: storage("ray tracing lights", &lights),
        }
    }
}

/// The uniform block `Params` of trace.wgsl.
fn params_bytes(
    scene: &RtScene,
    camera: &Camera,
    width: u32,
    height: u32,
    settings: &TraceSettings,
) -> Vec<u8> {
    let mut p = Words::default();
    let perspective = matches!(camera.projection_type(), ProjectionType::Perspective { .. });
    p.u(u32::from(perspective));
    p.u(width);
    p.u(height);
    p.u(settings.max_bounces);
    p.v3(camera.position(), 1.0);
    let mut view = camera.view();
    view[3] = vec4(0.0, 0.0, 0.0, 1.0);
    let screen_to_direction = (camera.projection() * view)
        .invert()
        .unwrap_or(Mat4::identity());
    p.m4(&screen_to_direction);
    let at = |u: f32, v: f32| camera.position_at_uv_coordinates((u, v));
    let origin = at(0.0, 0.0);
    p.v3(origin, 0.0);
    p.v3(at(1.0, 0.0) - origin, 0.0);
    p.v3(at(0.0, 1.0) - origin, 0.0);
    p.v3(camera.view_direction_at_uv_coordinates((0.5, 0.5)), 0.0);

    p.u(settings.seed);
    p.u(scene.lights().len() as u32);
    let sky = match scene.environment() {
        RtEnvironment::Sky(sky) => Some(sky),
        RtEnvironment::Uniform { .. } => None,
    };
    p.u(u32::from(sky.is_some()));
    p.u(u32::from(settings.indirect_clamp.is_some()));
    p.f(settings.indirect_clamp.unwrap_or(0.0));
    match scene.environment() {
        RtEnvironment::Uniform { color, luminance } => {
            let c = *color * *luminance;
            p.f(c.x);
            p.f(c.y);
            p.f(c.z);
        }
        RtEnvironment::Sky(_) => (0..3).for_each(|_| p.f(0.0)),
    }
    match sky {
        Some(sky) => {
            p.v3(sky.orientation.up.normalize(), sky.sky_scale);
            p.v3(sky.sun_direction(), sky.sun_cos_radius());
            p.v3(
                sky.sun_color() * sky.sun_illuminance(),
                sky.sun_illuminance(),
            );
            p.v3(sky.sun_radiance(), 0.0);
            let mut perez: Vec<f32> = sky.perez.iter().flatten().copied().collect();
            perez.resize(16, 0.0);
            perez.iter().for_each(|v| p.f(*v));
            p.v3(Vec3::from(sky.zenith), 0.0);
            p.v3(Vec3::from(sky.norm), 0.0);
            p.v3(sky.ground_luminance, 0.0);
        }
        None => (0..4 * 11).for_each(|_| p.f(0.0)),
    }
    p.bytes()
}

#[cfg(test)]
mod tests {
    /// wgpu reports invalid shaders only to an error handler, so the shaders are checked here on every platform.
    #[test]
    fn shaders_are_valid() {
        for (name, source) in [
            ("trace.wgsl", include_str!("compute/trace.wgsl")),
            ("resolve.wgsl", include_str!("compute/resolve.wgsl")),
        ] {
            let module = naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }
}
