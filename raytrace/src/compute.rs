//!
//! The path tracer in compute shaders of the wgpu backend: the hierarchies of the CPU builder in storage buffers and
//! the sums of the samples in a buffer on the GPU.
//!

use crate::render_image::FEATURE_WORDS;
use crate::{
    AccelBuilder, Camera, CpuAccel, CpuBvhBuilder, GpuDevice, RaytraceSupport, RenderImage,
    RtEnvironment, RtError, RtLight, RtScene, RtTexture, RtWrapping, TextureId, TraceSettings,
    Tracer, TracerKind,
};
#[cfg(target_arch = "wasm32")]
use std::sync::atomic::AtomicBool;
use three_d::wgpu as w;

#[cfg(target_arch = "wasm32")]
mod browser;
mod clock;
mod denoise_gpu;
pub(crate) mod timing;
use three_d_asset::prelude::*;
use three_d_asset::ProjectionType;
pub use timing::set_gpu_timing_log;

/// Size of a workgroup in both directions, as in the shaders.
const WORKGROUP: u32 = 8;

/// Bytes of one tile entry in the uniform buffer, before the alignment of dynamic offsets.
const TILE_BYTES: u64 = 32;

/// A binding of the tracer's bind groups, all of them buffers seen by compute shaders.
#[derive(Clone, Copy)]
enum Slot {
    Uniform { dynamic: bool, min_size: u64 },
    Storage { read_only: bool },
}

const PARAMS: Slot = Slot::Uniform {
    dynamic: false,
    min_size: 0,
};
const TILE: Slot = Slot::Uniform {
    dynamic: true,
    min_size: TILE_BYTES,
};
const READ: Slot = Slot::Storage { read_only: true };
const WRITE: Slot = Slot::Storage { read_only: false };

/// The bindings of the trace kernel.
const TRACE_SLOTS: [(u32, Slot); 10] = [
    (0, PARAMS),
    (1, TILE),
    (2, READ),
    (3, READ),
    (4, READ),
    (5, READ),
    (6, READ),
    (7, READ),
    (8, WRITE),
    (9, READ),
];
/// The bindings of the photon kernels: without the render image, with the photons written.
const PHOTON_SLOTS: [(u32, Slot); 9] = [
    (0, PARAMS),
    (1, TILE),
    (2, READ),
    (3, READ),
    (4, READ),
    (5, READ),
    (6, READ),
    (7, WRITE),
    (9, READ),
];
/// The photon kernels: shoot, count per cell, find where cells start, sort.
const PHOTON_ENTRIES: [&str; 4] = [
    "shoot_photons",
    "count_photons",
    "find_cell_starts",
    "sort_photons",
];
/// The constants of the tracer for scenes with participating media.
const MEDIA_CONSTANTS: [(&str, f64); 1] = [("MEDIA", 1.0)];

/// Whether the media tracer takes the surfaces of solids along shadow rays in their order on the adapter. Not on WARP,
/// the software adapter of DirectX 12 (Microsoft Basic Render Driver): with that code its compiler computes zeros in
/// the whole media tracer, the code it is given even where it never runs.
fn orders_shadow_crossings(adapter: &w::AdapterInfo) -> bool {
    !(adapter.backend == w::Backend::Dx12 && adapter.device_type == w::DeviceType::Cpu)
}

/// The constants of all pipelines on the adapter: without CONTACT where the tracer would get too large (Intel graphics
/// of the 7th generation, see [crate::media_problem]).
fn pipeline_constants(adapter: &w::AdapterInfo) -> Vec<(&'static str, f64)> {
    if crate::media_problem(adapter).is_some() {
        vec![("CONTACT", 0.0)]
    } else {
        Vec::new()
    }
}

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
    /// The tracer with participating media (MEDIA in trace.wgsl), made for the first scene with media: larger, so the
    /// tracer for scenes without stays small for the compilers of small GPUs
    media_pipeline: Option<w::ComputePipeline>,
    module: w::ShaderModule,
    pipeline_layout: w::PipelineLayout,
    /// The passes of caustic photons: shoot, count per cell, find where cells start, sort
    photon_layout: w::BindGroupLayout,
    photon_pipelines: [w::ComputePipeline; 4],
    /// The lights and, with caustics, the photons and their grid (binding 7)
    extras: Option<w::Buffer>,
    tile_size: u32,
    scratch: Option<GpuRenderImage>,
    /// Submissions that may still run, oldest first, see [MAX_SUBMISSIONS_IN_FLIGHT]
    in_flight: std::collections::VecDeque<w::SubmissionIndex>,
    /// The adapter as the context tells it, with the browser's vendor and architecture in a browser
    adapter: w::AdapterInfo,
    /// Errors of the GPU in the work of the tracer, which the browser reports later
    errors: ErrorWatch,
    /// The pipelines the browser compiled with `createComputePipelineAsync`, kept so that the browser finds the same
    /// pipelines of wgpu in its cache
    #[cfg(target_arch = "wasm32")]
    warm: Option<wasm_bindgen::JsValue>,
    /// Calls of [ComputeTracer::trace_gpu] whose work the browser has not finished yet, see
    /// [ComputeTracer::unfinished_calls]
    #[cfg(target_arch = "wasm32")]
    unfinished: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// How long the GPU took for the calls of [ComputeTracer::trace_gpu] it finished, see
    /// [ComputeTracer::take_call_times]
    call_times: std::sync::Arc<std::sync::Mutex<CallTimes>>,
    /// The GPU's time of the calls from timestamp queries, where the device has them
    clock: Option<clock::GpuClock>,
    /// Calls of [ComputeTracer::trace_gpu] logged so far, see [set_gpu_timing_log]
    timed_calls: u32,
}

/// How long the GPU took for the calls of [ComputeTracer::trace_gpu], see [ComputeTracer::take_call_times].
#[derive(Debug, Default)]
struct CallTimes {
    /// Counted up by [ComputeTracer::restart_call_times]; calls queued before then are not kept
    round: u64,
    /// In the browser: when the GPU finished the last call, in milliseconds of `timing::now`
    #[cfg(target_arch = "wasm32")]
    last_done: f64,
    /// Each call finished since the last take
    finished: Vec<CallTime>,
}

/// The GPU's time for one call of the tracer, see [ComputeTracer::take_call_times].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CallTime {
    /// Milliseconds
    pub ms: f64,
    /// Tiles the call traced, a tile for each sample
    pub tiles: u32,
}

/// The first calls of [ComputeTracer::trace_gpu] that [set_gpu_timing_log] logs.
const TIMED_CALLS: u32 = 8;

/// Submissions of the tracer the GPU may have queued at once; the tracer waits for the oldest before it adds another.
/// GPUs that can not interrupt compute work (Intel graphics of the 7th generation) run everything queued before the
/// driver's check that the GPU is alive, and Linux resets the GPU when that waits a few seconds. One tile and sample per
/// submission and few of them keep the queue short. In the browser the tracer can not wait; there the queue is not
/// limited.
#[cfg(not(target_arch = "wasm32"))]
const MAX_SUBMISSIONS_IN_FLIGHT: usize = 2;

impl ComputeTracer {
    ///
    /// A tracer on the device. The device needs compute shaders, see [RaytraceSupport::of].
    ///
    pub fn new(gpu: &GpuDevice) -> Self {
        Self::for_adapter(gpu, None)
    }

    ///
    /// A tracer on the device, for the adapter as the context tells it (see [three_d::Context::wgpu_adapter_info]),
    /// which knows more in a browser than the device.
    ///
    pub(crate) fn for_adapter(gpu: &GpuDevice, adapter: Option<w::AdapterInfo>) -> Self {
        let device = &gpu.device;
        let adapter = adapter.unwrap_or_else(|| device.adapter_info());
        let constants = pipeline_constants(&adapter);
        let errors = ErrorWatch::default();
        let scopes = ErrorScopes::push(device);
        let layout_of = |label, slots: &[(u32, Slot)]| {
            let entries: Vec<_> = slots
                .iter()
                .map(|&(binding, slot)| w::BindGroupLayoutEntry {
                    binding,
                    visibility: w::ShaderStages::COMPUTE,
                    ty: w::BindingType::Buffer {
                        ty: match slot {
                            Slot::Uniform { .. } => w::BufferBindingType::Uniform,
                            Slot::Storage { read_only } => {
                                w::BufferBindingType::Storage { read_only }
                            }
                        },
                        has_dynamic_offset: matches!(slot, Slot::Uniform { dynamic: true, .. }),
                        min_binding_size: match slot {
                            Slot::Uniform { min_size, .. } => std::num::NonZeroU64::new(min_size),
                            Slot::Storage { .. } => None,
                        },
                    },
                    count: None,
                })
                .collect();
            device.create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &entries,
            })
        };
        let layout = layout_of("ray tracing", &TRACE_SLOTS);
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
            compilation_options: w::PipelineCompilationOptions {
                constants: &constants,
                ..Default::default()
            },
            cache: None,
        });
        let photon_layout = layout_of("caustic photons", &PHOTON_SLOTS);
        let photon_pipeline_layout = device.create_pipeline_layout(&w::PipelineLayoutDescriptor {
            label: Some("caustic photons"),
            bind_group_layouts: &[Some(&photon_layout)],
            immediate_size: 0,
        });
        let photon_pipelines = PHOTON_ENTRIES.map(|entry| {
            device.create_compute_pipeline(&w::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&photon_pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: w::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                cache: None,
            })
        });
        scopes.pop_into(&errors);
        Self {
            gpu: gpu.clone(),
            builder: CpuBvhBuilder::default(),
            accel: None,
            buffers: None,
            layout,
            pipeline,
            media_pipeline: None,
            module,
            pipeline_layout,
            photon_layout,
            photon_pipelines,
            extras: None,
            tile_size: 256,
            scratch: None,
            in_flight: std::collections::VecDeque::new(),
            adapter,
            errors,
            #[cfg(target_arch = "wasm32")]
            warm: None,
            #[cfg(target_arch = "wasm32")]
            unfinished: Default::default(),
            call_times: Default::default(),
            clock: clock::GpuClock::new(&gpu.device, &gpu.queue),
            timed_calls: 0,
        }
    }

    ///
    /// A tracer on the device of a three-d context, `None` if the context can not run compute shaders.
    ///
    /// Compiling the shaders takes seconds on some drivers, and in a browser that stops the page meanwhile; there
    /// [ComputeTracer::from_context_async] lets the browser compile them in the background.
    ///
    pub fn from_context(context: &three_d::Context) -> Option<Self> {
        if RaytraceSupport::of(context) < RaytraceSupport::Compute {
            return None;
        }
        Some(Self::for_adapter(
            &GpuDevice::from_context(context)?,
            context.wgpu_adapter_info(),
        ))
    }

    ///
    /// As [ComputeTracer::from_context], but in a browser the shaders are compiled first with WebGPU's
    /// `createComputePipelineAsync`, which does not stop the page, and the tracer is made from the compiled pipelines.
    /// `None` also if they fail to compile there. Outside of browsers it is [ComputeTracer::from_context].
    ///
    pub async fn from_context_async(context: &three_d::Context) -> Option<Self> {
        #[cfg(target_arch = "wasm32")]
        {
            if RaytraceSupport::of(context) < RaytraceSupport::Compute {
                return None;
            }
            // The browser's driver for Intel graphics of the 7th generation crashes on the shaders, see preview_problem
            if context
                .wgpu_adapter_info()
                .is_some_and(|info| crate::gpu::preview_problem(&info).is_some())
            {
                return None;
            }
            let warm = match context.webgpu_device() {
                Some(device) => {
                    let info = context.wgpu_adapter_info()?;
                    let media = crate::media_problem(&info).is_none();
                    let start = timing::now();
                    let compiled =
                        browser::compile(&device, media, &pipeline_constants(&info)).await;
                    if timing::enabled() {
                        timing::log(&format!(
                            "createComputePipelineAsync of the tracer and denoiser took {:.0} ms ({})",
                            timing::now() - start,
                            if compiled.is_ok() { "compiled" } else { "failed" }
                        ));
                    }
                    Some(compiled.ok()?)
                }
                None => None,
            };
            let start = timing::now();
            let mut tracer = Self::from_context(context)?;
            tracer.warm = warm;
            if timing::enabled() {
                timing::log(&format!(
                    "made the tracer's pipelines with wgpu in {:.0} ms on the CPU",
                    timing::now() - start
                ));
                timing::when_gpu_done(&tracer.gpu.queue, start, "making the tracer".to_owned());
                let info = &tracer.adapter;
                timing::log(&format!(
                    "adapter {:?} vendor {:#x} driver {:?} driver info {:?}, frames ahead {}, media {}",
                    info.name,
                    info.vendor,
                    info.driver,
                    info.driver_info,
                    crate::gpu::frames_ahead(info),
                    tracer.media_problem().is_none()
                ));
            }
            tracer.warm_up().await;
            Some(tracer)
        }
        #[cfg(not(target_arch = "wasm32"))]
        Self::from_context(context)
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

    ///
    /// Why the tracer can not trace scenes with participating media on its device (fog, smoke, tinted glass), `None`
    /// if it can. [ComputeTracer::trace_gpu] returns [RtError::Gpu] for such scenes there; trace them on the CPU.
    ///
    pub fn media_problem(&self) -> Option<String> {
        crate::media_problem(&self.adapter)
    }

    ///
    /// The first error the GPU reported in the work of the tracer, for example a shader that failed to compile or too
    /// little memory, `None` while there is none. Browsers report errors a little later, so this may only show an
    /// error after a frame or two. [ComputeTracer::trace_gpu] returns it as [RtError::Gpu] from then on.
    ///
    pub fn gpu_error(&mut self) -> Option<String> {
        self.errors.check()
    }

    ///
    /// Whether shadow rays in scenes with media take the surfaces of solids in their order, with the media of nested
    /// solids by their [RtMaterial::medium_priority](crate::RtMaterial::medium_priority) as the paths do. Everywhere but on WARP, the software adapter of
    /// DirectX 12, whose compiler gets that code wrong: there shadow rays add up the media of the solids they cross,
    /// which is right as long as solids do not overlap, and counts the media of nested solids both.
    ///
    pub fn orders_shadow_crossings(&self) -> bool {
        orders_shadow_crossings(&self.adapter)
    }

    /// The adapter as the context told it.
    pub(crate) fn adapter_info(&self) -> &w::AdapterInfo {
        &self.adapter
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
        self.trace_gpu_checked(scene, camera, image, settings, None)
            .map(|_| ())
    }

    ///
    /// Like [ComputeTracer::trace_gpu], but traces only the next `max_tiles` tiles of the sample the image is at (at
    /// least one, at most up to the sample's end), so that a sample of a large image can be spread over several calls
    /// and each call stays short, for example within the time of a frame. Returns whether the call completed the
    /// sample; the image counts it from then on.
    ///
    /// Until then the tiles traced so far hold one sample more than [GpuRenderImage::sample_count]: show or read the
    /// image only when no sample is in progress ([GpuRenderImage::sample_in_progress]). [ComputeTracer::trace_gpu]
    /// first completes a sample in progress. The tiles of a sample are [ComputeTracer::tiles_per_sample] for this
    /// tracer, use the same tracer until the sample is complete.
    ///
    pub fn trace_gpu_tiles(
        &mut self,
        scene: &RtScene,
        camera: &Camera,
        image: &mut GpuRenderImage,
        settings: &TraceSettings,
        max_tiles: u32,
    ) -> Result<bool, RtError> {
        self.trace_gpu_checked(scene, camera, image, settings, Some(max_tiles.max(1)))
    }

    /// The tiles of one sample of an image of this size, see [ComputeTracer::trace_gpu_tiles].
    pub fn tiles_per_sample(&self, width: u32, height: u32) -> u32 {
        width.div_ceil(self.tile_size) * height.div_ceil(self.tile_size)
    }

    fn trace_gpu_checked(
        &mut self,
        scene: &RtScene,
        camera: &Camera,
        image: &mut GpuRenderImage,
        settings: &TraceSettings,
        max_tiles: Option<u32>,
    ) -> Result<bool, RtError> {
        if image.width == 0 || image.height == 0 {
            return Err(RtError::EmptyImage);
        }
        if let Some(error) = self.gpu_error() {
            return Err(RtError::Gpu(format!("the GPU failed: {error}")));
        }
        let scopes = ErrorScopes::push(&self.gpu.device);
        let start = timing::now();
        let result = self.trace_gpu_watched(scene, camera, image, settings, max_tiles);
        scopes.pop_into(&self.errors);
        if timing::enabled() && self.timed_calls < TIMED_CALLS {
            self.timed_calls += 1;
            let what = format!(
                "trace call {} at {}x{}, {}",
                self.timed_calls,
                image.width,
                image.height,
                match max_tiles {
                    Some(tiles) => format!("at most {tiles} tile(s)"),
                    None => format!("{} sample(s)", settings.samples_per_call),
                }
            );
            timing::log(&format!(
                "{what}: {:.0} ms on the CPU",
                timing::now() - start
            ));
            timing::when_gpu_done(&self.gpu.queue, start, what);
        }
        result
    }

    fn trace_gpu_watched(
        &mut self,
        scene: &RtScene,
        camera: &Camera,
        image: &mut GpuRenderImage,
        settings: &TraceSettings,
        max_tiles: Option<u32>,
    ) -> Result<bool, RtError> {
        let start = timing::now();
        // The number of the call if it is logged
        let timed =
            (timing::enabled() && self.timed_calls < TIMED_CALLS).then_some(self.timed_calls + 1);
        if let Some(call) = timed {
            // What was queued before this call, by the tracer or the application (textures, frames, the image)
            self.gpu.queue.submit([]);
            timing::when_gpu_done(
                &self.gpu.queue,
                start,
                format!("trace call {call}, work queued before it"),
            );
        }
        self.prepare(scene)?;
        if let Some(call) = timed {
            timing::log(&format!(
                "scene prepared (structures built and uploaded) in {:.0} ms on the CPU",
                timing::now() - start
            ));
            self.gpu.queue.submit([]);
            timing::when_gpu_done(
                &self.gpu.queue,
                start,
                format!("trace call {call}, scene structures"),
            );
        }
        if scene.has_media() {
            if let Some(why) = self.media_problem() {
                return Err(RtError::Gpu(why));
            }
        }
        if scene.has_media() && self.media_pipeline.is_none() {
            self.media_pipeline = Some(self.gpu.device.create_compute_pipeline(
                &w::ComputePipelineDescriptor {
                    label: Some("ray tracing with media"),
                    layout: Some(&self.pipeline_layout),
                    module: &self.module,
                    entry_point: Some("trace"),
                    compilation_options: w::PipelineCompilationOptions {
                        constants: &if orders_shadow_crossings(&self.adapter) {
                            MEDIA_CONSTANTS.to_vec()
                        } else {
                            [MEDIA_CONSTANTS.as_slice(), &[("ORDERED", 0.0)]].concat()
                        },
                        ..Default::default()
                    },
                    cache: None,
                },
            ));
        }
        let pipeline = match &self.media_pipeline {
            Some(pipeline) if scene.has_media() => pipeline,
            _ => &self.pipeline,
        };
        let device = &self.gpu.device;
        let buffers = self.buffers.as_ref().expect("prepared");
        let camera_inside = if scene.has_media() {
            crate::tracer::solids_containing(
                scene,
                self.accel.as_ref().expect("prepared"),
                camera.position(),
            )
        } else {
            crate::nesting::Inside::AIR
        };
        let caustics = CausticPlan::new(
            scene,
            self.accel.as_ref().expect("prepared"),
            settings,
            buffers.lights.0.len() as u32,
            device.limits().max_storage_buffer_binding_size,
        );

        // The lights and the room for the photons, written before the passes of this call
        let light_bytes = buffers.lights.bytes();
        let extras_size = match &caustics {
            Some(plan) => 4 * plan.words as u64,
            None => light_bytes.len().max(16) as u64,
        };
        if self.extras.as_ref().is_none_or(|b| b.size() < extras_size) {
            self.extras = Some(device.create_buffer(&w::BufferDescriptor {
                label: Some("ray tracing lights and photons"),
                size: extras_size,
                usage: w::BufferUsages::STORAGE | w::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let extras = self.extras.as_ref().expect("created");
        if !light_bytes.is_empty() {
            self.gpu.queue.write_buffer(extras, 0, &light_bytes);
        }
        if let Some(plan) = &caustics {
            self.gpu.queue.write_buffer(
                extras,
                4 * plan.sources as u64,
                &plan.emitter_bytes(&buffers.profiles),
            );
        }

        let params = filled_buffer(
            &self.gpu,
            "ray tracing parameters",
            &params_bytes(
                scene,
                camera,
                image.width,
                image.height,
                settings,
                caustics.as_ref(),
                &camera_inside,
            ),
            w::BufferUsages::UNIFORM,
        );

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
        // The dispatches of the call, a sample (counted from the image's samples) and a tile each: from the tile of
        // the sample in progress either to the end of that sample, at most `max_tiles`, or for `samples_per_call`
        // samples, the one in progress completed first
        let n = tiles.len() as u32;
        let begin = if image.next_tile < n {
            image.next_tile
        } else {
            0
        };
        let count = match max_tiles {
            Some(max) => max.clamp(1, n - begin),
            None => (settings.samples_per_call * n).saturating_sub(begin),
        };
        if count == 0 {
            return Ok(false);
        }
        let dispatches: Vec<(u32, usize)> = (begin..begin + count)
            .map(|g| (g / n, (g % n) as usize))
            .collect();
        let mut tile_data = vec![0u8; (stride * count as u64) as usize];
        for (k, &(s, t)) in dispatches.iter().enumerate() {
            let rect = tiles[t];
            let at = (k as u64 * stride) as usize;
            let words = [
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                image.samples + s,
                1,
                u32::from(image.features),
                caustics.as_ref().map_or(0, |_| {
                    crate::photons::gather_radius(settings.caustic_radius, image.samples + s)
                        .to_bits()
                }),
            ];
            for (i, word) in words.iter().enumerate() {
                tile_data[at + 4 * i..at + 4 * i + 4].copy_from_slice(&word.to_le_bytes());
            }
        }
        let tile_buffer = filled_buffer(
            &self.gpu,
            "ray tracing tiles",
            &tile_data,
            w::BufferUsages::UNIFORM,
        );

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
                    resource: extras.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 8,
                    resource: image.sums.as_entire_binding(),
                },
                w::BindGroupEntry {
                    binding: 9,
                    resource: buffers.texels.as_entire_binding(),
                },
            ],
        });

        let photon_bind_group = caustics.as_ref().map(|_| {
            fn entry(binding: u32, buffer: &w::Buffer) -> w::BindGroupEntry<'_> {
                w::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                }
            }
            device.create_bind_group(&w::BindGroupDescriptor {
                label: Some("caustic photons"),
                layout: &self.photon_layout,
                entries: &[
                    entry(0, &params),
                    w::BindGroupEntry {
                        binding: 1,
                        resource: w::BindingResource::Buffer(w::BufferBinding {
                            buffer: &tile_buffer,
                            offset: 0,
                            size: std::num::NonZeroU64::new(TILE_BYTES),
                        }),
                    },
                    entry(2, &buffers.nodes),
                    entry(3, &buffers.triangles),
                    entry(4, &buffers.vertices),
                    entry(5, &buffers.instances),
                    entry(6, &buffers.materials),
                    entry(7, extras),
                    entry(9, &buffers.texels),
                ],
            })
        });

        if let Some(call) = timed {
            // The uploads on their own, to tell them from the tracing
            self.gpu.queue.submit([]);
            timing::when_gpu_done(
                &self.gpu.queue,
                start,
                format!(
                    "trace call {call}, uploads ({} MB of scene, {} tiles, caustics {})",
                    buffers.bytes() / 1_000_000,
                    tiles.len(),
                    caustics.is_some()
                ),
            );
        }
        // The GPU's time of the call, from the beginning of its first pass to the end of its last one
        let passes = count
            + caustics.as_ref().map_or(0, |_| {
                dispatches.iter().filter(|(_, t)| *t == 0).count() as u32
            });
        let stamp = self
            .clock
            .as_ref()
            .and_then(|clock| Some((clock, clock.begin(passes)?)));
        let round = self
            .call_times
            .lock()
            .map(|times| times.round)
            .unwrap_or_default();
        let mut pass = 0;
        let mut writes = || {
            pass += 1;
            stamp.and_then(|(clock, stamp)| clock.writes(stamp, pass - 1))
        };
        // One submission per sample and tile, so the GPU can go on with other work between them, and only a few at a time
        for (k, &(s, t)) in dispatches.iter().enumerate() {
            let mut encoder = device.create_command_encoder(&w::CommandEncoderDescriptor {
                label: Some("ray tracing"),
            });
            let offset = (k as u64 * stride) as u32;
            if let (0, Some(plan), Some(bind_group)) = (t, &caustics, &photon_bind_group) {
                // The photons of this sample, with its first tile: shot, counted per cell, sorted by cell. Calls that
                // trace the sample's other tiles find them in the buffer.
                encoder.clear_buffer(
                    extras,
                    4 * plan.counter as u64,
                    Some(4 * (plan.counts + PHOTON_CELLS - plan.counter) as u64),
                );
                let mut pass = encoder.begin_compute_pass(&w::ComputePassDescriptor {
                    label: Some("caustic photons"),
                    timestamp_writes: writes(),
                });
                pass.set_bind_group(0, bind_group, &[offset]);
                let groups = plan.capacity().div_ceil(64);
                for (pipeline, groups) in self
                    .photon_pipelines
                    .iter()
                    .zip([groups, groups, 1, groups])
                {
                    pass.set_pipeline(pipeline);
                    pass.dispatch_workgroups(groups, 1, 1);
                }
            }
            let rect = tiles[t];
            let last = k + 1 == dispatches.len();
            {
                let mut pass = encoder.begin_compute_pass(&w::ComputePassDescriptor {
                    label: Some("ray tracing"),
                    timestamp_writes: writes(),
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind_group, &[offset]);
                pass.dispatch_workgroups(
                    rect[2].div_ceil(WORKGROUP),
                    rect[3].div_ceil(WORKGROUP),
                    1,
                );
            }
            if let (true, Some((clock, stamp))) = (last, stamp) {
                clock.resolve(&mut encoder, stamp);
            }
            submit(&self.gpu, &mut self.in_flight, encoder.finish());
            if let (true, Some((clock, stamp))) = (last, stamp) {
                clock.read(stamp, &self.call_times, round, count);
            }
            if let Some(call) = timed {
                timing::when_gpu_done(
                    &self.gpu.queue,
                    start,
                    format!(
                        "trace call {call}, sample {s}, tile {t} of {}x{} px{}",
                        rect[2],
                        rect[3],
                        if t == 0 && caustics.is_some() {
                            " with the photons"
                        } else {
                            ""
                        }
                    ),
                );
            }
        }
        let end = begin + count;
        image.samples += end / n;
        image.next_tile = end % n;
        // The browser tells when the work is done; until then the next call would only queue more
        #[cfg(target_arch = "wasm32")]
        {
            use std::sync::atomic::Ordering;
            self.unfinished.fetch_add(1, Ordering::Relaxed);
            let unfinished = self.unfinished.clone();
            let times = self.call_times.clone();
            // Without timestamps: the time until the browser tells
            let timed_here = self.clock.is_none();
            self.gpu.queue.on_submitted_work_done(move || {
                unfinished.fetch_sub(1, Ordering::Relaxed);
                let done = timing::now();
                if let Ok(mut times) = times.lock() {
                    // The GPU worked on the call from its start or, if it was busy then, from the end of the call
                    // before; that includes what the application queued in between, such as the denoiser, and the
                    // time until the page's thread is free for the browser's message, such as a frame of the page
                    let busy = done - start.max(times.last_done);
                    times.last_done = done;
                    if timed_here && times.round == round {
                        times.finished.push(CallTime {
                            ms: busy.max(0.0),
                            tiles: count,
                        });
                    }
                }
            });
        }
        Ok(end >= n)
    }

    ///
    /// Traces one workgroup of a tiny scene and waits until the GPU is done. Some drivers do work on the first dispatch
    /// of a pipeline (Chrome on Intel graphics of the 7th generation took 25 s there); that way it happens while the
    /// preview still traces on the CPU, and the first real frame is fast.
    ///
    #[cfg(target_arch = "wasm32")]
    async fn warm_up(&mut self) {
        let start = timing::now();
        let mut scene = RtScene::new();
        let Ok(square) = scene.add_mesh(&three_d_asset::TriMesh::square()) else {
            return;
        };
        let material = scene.add_material(crate::RtMaterial::default());
        if scene
            .add_instance(square, material, Mat4::identity())
            .is_err()
        {
            return;
        }
        let camera = Camera::new_perspective(
            three_d_asset::Viewport::new_at_origo(WORKGROUP, WORKGROUP),
            vec3(0.0, 0.0, 2.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            degrees(45.0),
            0.1,
            10.0,
        );
        let mut image = GpuRenderImage::new(&self.gpu, WORKGROUP, WORKGROUP);
        let timed_calls = self.timed_calls;
        // Not counted as one of the calls of the application
        self.timed_calls = TIMED_CALLS;
        let traced = self.trace_gpu(&scene, &camera, &mut image, &TraceSettings::default());
        self.timed_calls = timed_calls;
        if traced.is_err() {
            return;
        }
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        self.gpu
            .queue
            .on_submitted_work_done(move || flag.store(true, std::sync::atomic::Ordering::Relaxed));
        while !done.load(std::sync::atomic::Ordering::Relaxed) {
            browser::sleep(10).await;
        }
        // The structures of the tiny scene are not needed any more
        self.accel = None;
        self.buffers = None;
        if timing::enabled() {
            timing::log(&format!(
                "warm-up dispatch of one workgroup: GPU done after {:.0} ms",
                timing::now() - start
            ));
        }
    }

    ///
    /// The calls of [ComputeTracer::trace_gpu] whose work the GPU has not finished yet. Outside of browsers the tracer
    /// waits itself before the queue gets long (see [ComputeTracer::trace_gpu]), so this is 0 there. A browser can not
    /// wait: an application that traces every frame should skip the frame while this is more than one or two, so that
    /// the browser does not queue work for seconds, which other graphics of the page wait for.
    ///
    pub fn unfinished_calls(&self) -> usize {
        #[cfg(target_arch = "wasm32")]
        return self.unfinished.load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(target_arch = "wasm32"))]
        0
    }

    /// Whether the calls are timed with timestamp queries, pass by pass.
    pub(crate) fn times_from_timestamps(&self) -> bool {
        self.clock.is_some()
    }

    ///
    /// Whether [ComputeTracer::take_call_times] tells the GPU's time: where the device has timestamp queries, and in
    /// browsers always (without them from when the browser tells that the work is done).
    ///
    pub(crate) fn measures_call_times(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        return true;
        #[cfg(not(target_arch = "wasm32"))]
        self.clock.is_some()
    }

    ///
    /// The GPU's time for each call of [ComputeTracer::trace_gpu] finished since the last take, oldest first: from
    /// timestamps at the beginning and end of the call (the time between frames also holds what the application draws
    /// and reads back, and the frames of a browser do not wait for the GPU at all); in browsers without timestamp
    /// queries from when the browser tells that the work is done, which comes late while the page's thread is busy.
    ///
    pub(crate) fn take_call_times(&self) -> Vec<CallTime> {
        // The times arrive when the device is polled
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.gpu.device.poll(w::PollType::Poll);
        self.call_times
            .lock()
            .map(|mut times| std::mem::take(&mut times.finished))
            .unwrap_or_default()
    }

    /// Forgets the times of the calls so far, also of those the GPU has not finished yet.
    pub(crate) fn restart_call_times(&self) {
        if let Ok(mut times) = self.call_times.lock() {
            times.round += 1;
            times.finished.clear();
        }
    }
}

///
/// A buffer with the bytes, written through the queue. Not mapped at creation: browsers refuse that for some sizes on
/// some GPUs, and wgpu panics then.
///
fn filled_buffer(gpu: &GpuDevice, label: &str, bytes: &[u8], usage: w::BufferUsages) -> w::Buffer {
    // Copies need a multiple of four bytes, and a binding at least one word
    let size = (bytes.len() as u64).div_ceil(4).max(1) * 4;
    let buffer = gpu.device.create_buffer(&w::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | w::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !bytes.is_empty() {
        let mut padded = bytes.to_vec();
        padded.resize(size as usize, 0);
        gpu.queue.write_buffer(&buffer, 0, &padded);
    }
    buffer
}

/// The scopes of wgpu that catch the errors of some work: out of memory, validation and internal errors.
struct ErrorScopes([w::ErrorScopeGuard; 3]);

impl ErrorScopes {
    fn push(device: &w::Device) -> Self {
        Self([
            device.push_error_scope(w::ErrorFilter::OutOfMemory),
            device.push_error_scope(w::ErrorFilter::Validation),
            device.push_error_scope(w::ErrorFilter::Internal),
        ])
    }

    /// Ends the scopes, innermost first, and hands what they caught to the watch.
    fn pop_into(self, watch: &ErrorWatch) {
        let mut pending = watch.pending.lock().unwrap_or_else(|e| e.into_inner());
        for scope in self.0.into_iter().rev() {
            pending.push(Box::pin(scope.pop()));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
type ErrorFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Option<w::Error>> + Send>>;
#[cfg(target_arch = "wasm32")]
type ErrorFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Option<w::Error>>>>;

///
/// What the error scopes of the tracer caught. Natively wgpu knows at once, a browser answers a little later, so the
/// answers are polled without waiting.
///
#[derive(Default)]
struct ErrorWatch {
    pending: std::sync::Mutex<Vec<ErrorFuture>>,
    first: std::sync::Mutex<Option<String>>,
}

impl std::fmt::Debug for ErrorWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErrorWatch")
            .field("first", &self.first)
            .finish_non_exhaustive()
    }
}

impl ErrorWatch {
    /// The first error caught, after looking at the answers there are.
    fn check(&self) -> Option<String> {
        let mut first = self.first.lock().unwrap_or_else(|e| e.into_inner());
        if first.is_none() {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            pending.retain_mut(|future| match future.as_mut().poll(&mut context) {
                std::task::Poll::Ready(Some(error)) => {
                    first.get_or_insert_with(|| error.to_string());
                    false
                }
                std::task::Poll::Ready(None) => false,
                std::task::Poll::Pending => true,
            });
        }
        first.clone()
    }
}

/// Submits work after waiting for the oldest submission when [MAX_SUBMISSIONS_IN_FLIGHT] may still run.
fn submit(
    gpu: &GpuDevice,
    in_flight: &mut std::collections::VecDeque<w::SubmissionIndex>,
    commands: w::CommandBuffer,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        while in_flight.len() >= MAX_SUBMISSIONS_IN_FLIGHT {
            let oldest = in_flight.pop_front().unwrap();
            // An error (a lost device) shows when the work is read back
            let _ = gpu.device.poll(w::PollType::Wait {
                submission_index: Some(oldest),
                timeout: None,
            });
        }
        in_flight.push_back(gpu.queue.submit([commands]));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = in_flight;
        gpu.queue.submit([commands]);
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
                &self.gpu,
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
        let features = render_image.has_features();
        let mut scratch = match self.scratch.take() {
            Some(s) if s.width == width && s.height == height && s.features == features => s,
            _ if features => GpuRenderImage::with_features(&self.gpu, width, height),
            _ => GpuRenderImage::new(&self.gpu, width, height),
        };
        scratch.reset();
        // Go on with the samples of the render image, so every call adds new ones
        scratch.samples = render_image.sample_count();
        let mut result = self.trace_gpu(scene, camera, &mut scratch, settings);
        if result.is_ok() {
            let sums = scratch.read_sums();
            // The GPU is done after the read, so its errors are known: a pipeline that failed to compile leaves zeros
            match self.gpu_error() {
                Some(error) => result = Err(RtError::Gpu(format!("the GPU failed: {error}"))),
                None => scratch.add_to(render_image, &sums, settings.samples_per_call),
            }
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
    /// The next tile of the sample in progress, see [ComputeTracer::trace_gpu_tiles]; 0 between samples
    next_tile: u32,
    sums: w::Buffer,
    /// Three words per pixel with the features for a denoiser instead of one.
    features: bool,
    resolve: Option<Resolve>,
    denoise: Option<Box<denoise_gpu::GpuDenoise>>,
}

impl GpuRenderImage {
    /// A black render image with the given size.
    pub fn new(gpu: &GpuDevice, width: u32, height: u32) -> Self {
        Self::create(gpu, width, height, false)
    }

    ///
    /// A black render image that also collects the features for denoising, like [RenderImage::with_features]. Costs 32
    /// more bytes per pixel.
    ///
    pub fn with_features(gpu: &GpuDevice, width: u32, height: u32) -> Self {
        Self::create(gpu, width, height, true)
    }

    /// Whether the image collects features.
    pub fn has_features(&self) -> bool {
        self.features
    }

    fn create(gpu: &GpuDevice, width: u32, height: u32, features: bool) -> Self {
        let words = if features { 3 } else { 1 };
        let sums = gpu.device.create_buffer(&w::BufferDescriptor {
            label: Some("render image"),
            size: (width.max(1) as u64) * (height.max(1) as u64) * 16 * words,
            usage: w::BufferUsages::STORAGE | w::BufferUsages::COPY_SRC | w::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            gpu: gpu.clone(),
            width,
            height,
            samples: 0,
            next_tile: 0,
            sums,
            features,
            resolve: None,
            denoise: None,
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

    ///
    /// Whether [ComputeTracer::trace_gpu_tiles] traced only some tiles of the next sample so far. Its tiles then hold
    /// a sample more than the others and the average is wrong there until the sample is complete.
    ///
    pub fn sample_in_progress(&self) -> bool {
        self.next_tile > 0
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
        self.next_tile = 0;
    }

    /// Changes the size and forgets all samples.
    pub fn resize(&mut self, width: u32, height: u32) {
        *self = Self::create(&self.gpu, width, height, self.features);
    }

    /// A render image of the CPU for the sums read back, with features if collected.
    fn empty_render_image(&self) -> RenderImage {
        if self.features {
            RenderImage::with_features(self.width, self.height)
        } else {
            RenderImage::new(self.width, self.height)
        }
    }

    /// Adds sums read back from this image to a render image.
    fn add_to(&self, render_image: &mut RenderImage, sums: &[[f32; 4]], samples: u32) {
        if self.features {
            let features: Vec<[f32; FEATURE_WORDS]> = sums
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| {
                    [
                        p[1][0], p[1][1], p[1][2], p[2][0], p[2][1], p[2][2], p[1][3], p[0][3],
                    ]
                })
                .collect();
            render_image.accumulate_features(&features);
            let colors: Vec<[f32; 3]> = sums
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| [p[0][0], p[0][1], p[0][2]])
                .collect();
            render_image.accumulate(&colors, samples);
        } else {
            let colors: Vec<[f32; 3]> = sums.iter().map(|p| [p[0], p[1], p[2]]).collect();
            render_image.accumulate(&colors, samples);
        }
    }

    ///
    /// Copies the samples into a [RenderImage], waiting for the GPU. Not possible in a browser, use
    /// [GpuRenderImage::read_async] there.
    ///
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(&self) -> RenderImage {
        let mut render_image = self.empty_render_image();
        self.add_to(&mut render_image, &self.read_sums(), self.samples);
        render_image
    }

    /// Copies the samples into a [RenderImage] once the GPU is done, also in a browser.
    pub async fn read_async(&self) -> RenderImage {
        let sums = self.start_read().await;
        let mut render_image = self.empty_render_image();
        self.add_to(&mut render_image, &sums, self.samples);
        render_image
    }

    fn read_sums(&self) -> Vec<[f32; 4]> {
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
            words: (self.width * self.height) as usize * if self.features { 3 } else { 1 },
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
            if self.features { 3 } else { 1 },
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
        copy_rows_to_texture(
            &mut encoder,
            &resolve.averages,
            pitch,
            self.width,
            self.height,
            &target,
        );
        self.gpu.queue.submit([encoder.finish()]);
    }

    ///
    /// Like [GpuRenderImage::write_to_three_d_texture], but denoised on the GPU by the filter of `denoiser`.
    ///
    /// # Panics
    /// If the image collects no features (see [GpuRenderImage::with_features]), or as
    /// [GpuRenderImage::write_to_three_d_texture].
    ///
    pub fn write_denoised_to_three_d_texture(
        &mut self,
        texture: &three_d::Texture2D,
        denoiser: &crate::AtrousDenoiser,
    ) {
        assert!(self.features, "the render image collects no features");
        assert!(
            texture.width() == self.width && texture.height() == self.height,
            "the texture has another size than the render image"
        );
        let target = texture
            .as_wgpu()
            .expect("the texture is not from the wgpu backend");
        let denoise = self.denoise.get_or_insert_with(|| {
            Box::new(denoise_gpu::GpuDenoise::new(
                &self.gpu.device,
                self.width,
                self.height,
            ))
        });
        denoise.run(&self.gpu, &self.sums, self.samples, denoiser, &target);
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

/// Copies rows of RGBA floats with `pitch` pixels each into a texture, in the order of its memory: three-d keeps the
/// bottom row of an image first there, as OpenGL does.
fn copy_rows_to_texture(
    encoder: &mut w::CommandEncoder,
    buffer: &w::Buffer,
    pitch: u32,
    width: u32,
    height: u32,
    target: &w::Texture,
) {
    encoder.copy_buffer_to_texture(
        w::TexelCopyBufferInfo {
            buffer,
            layout: w::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch * 16),
                rows_per_image: Some(height),
            },
        },
        w::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: w::Origin3d::ZERO,
            aspect: w::TextureAspect::All,
        },
        w::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
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
    /// Number of vec4 to read.
    words: usize,
    state: std::sync::Arc<std::sync::Mutex<MapState>>,
}

impl PendingSums {
    fn finish(&self) -> Vec<[f32; 4]> {
        let ok = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .done
            .unwrap_or(false);
        if !ok {
            // The device was lost: no samples
            return vec![[0.0; 4]; self.words];
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
            .take(self.words)
            .copied()
            .collect()
    }
}

impl std::future::Future for PendingSums {
    type Output = Vec<[f32; 4]>;

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
    /// Four vec4 per light and the tables of their profiles, written into the buffer of lights and photons with every
    /// call
    lights: Words,
    /// The profiles of luminaires and the words where their tables start
    profiles: Vec<(std::sync::Arc<crate::IesProfile>, u32)>,
    texels: w::Buffer,
}

impl SceneBuffers {
    /// Bytes on the GPU.
    fn bytes(&self) -> u64 {
        [
            &self.nodes,
            &self.triangles,
            &self.vertices,
            &self.instances,
            &self.materials,
            &self.texels,
        ]
        .iter()
        .map(|b| b.size())
        .sum()
    }
}

/// Words of a buffer, floats by their bits.
#[derive(Default, Debug)]
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
    fn new(gpu: &GpuDevice, scene: &RtScene, accel: &CpuAccel) -> Self {
        let device = &gpu.device;
        let limits = device.limits();
        let max_bytes = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
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
            let first_vertex = (vertices.0.len() / 12) as u32;
            for &t in &bvh.indices {
                let [a, b, c] = mesh.triangles[t as usize];
                triangles.u(first_vertex + a);
                triangles.u(first_vertex + b);
                triangles.u(first_vertex + c);
                triangles.u(t);
            }
            for (i, p) in mesh.positions.iter().enumerate() {
                let uv = mesh.uvs.as_ref().map_or(vec2(0.0, 0.0), |uvs| uvs[i]);
                vertices.v3(*p, uv.x);
                let normal = mesh
                    .normals
                    .as_ref()
                    .map_or(vec3(0.0, 0.0, 0.0), |normals| normals[i]);
                vertices.v3(normal, uv.y);
                match &mesh.tangents {
                    // w is never zero for a tangent, zero means none
                    Some(tangents) => {
                        let t = tangents[i];
                        vertices.v3(t.truncate(), if t.w < 0.0 { -1.0 } else { 1.0 })
                    }
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
            instances.u(if material.is_cut_out_everywhere() {
                0
            } else if material.cuts_out_by_texture() {
                2
            } else {
                1
            });
        }

        let mut materials = Words::default();
        for m in scene.materials() {
            materials.v3(m.base_color, m.alpha);
            materials.f(m.metallic);
            materials.f(m.roughness);
            materials.f(m.specular);
            materials.f(m.transmission);
            materials.v3(m.emission, m.index_of_refraction);
            materials.v3(
                vec3(
                    if m.thin_walled { 1.0 } else { 0.0 },
                    m.normal_scale,
                    m.alpha_cutout.unwrap_or(-1.0),
                ),
                m.medium_priority as f32,
            );
            let id = |t: Option<TextureId>| t.map_or(NONE, |t| t.0);
            materials.u(id(m.base_color_texture));
            materials.u(id(m.metallic_roughness_texture));
            materials.u(id(m.normal_texture));
            materials.u(id(m.emission_texture));
            materials.u(id(m.transmission_texture));
            materials.u(NONE);
            materials.u(NONE);
            materials.u(NONE);
        }
        // The media inside solids as entries after the materials: absorption and anisotropy, scattering
        let count = scene.materials().len();
        for (i, m) in scene.materials().iter().enumerate() {
            let Some(medium) = m
                .medium
                .filter(|md| !md.is_clear() && m.transmission > 0.0 && !m.thin_walled)
            else {
                continue;
            };
            let entry = materials.0.len() as u32 / MATERIAL_WORDS;
            materials.0[i * MATERIAL_WORDS as usize + 21] = entry;
            materials.v3(medium.absorption, medium.anisotropy);
            materials.v3(medium.scattering, 0.0);
            (0..4 * 4).for_each(|_| materials.f(0.0));
        }
        debug_assert!(materials.0.len() >= count * MATERIAL_WORDS as usize);

        // The lights, then the tables of their profiles, one per distinct profile
        let mut profiles: Vec<(std::sync::Arc<crate::IesProfile>, u32)> = Vec::new();
        let mut table_word = 16 * scene.lights().len() as u32;
        for light in scene.lights() {
            if let RtLight::Luminaire { profile, .. } = light {
                if !profiles
                    .iter()
                    .any(|(p, _)| std::sync::Arc::ptr_eq(p, profile))
                {
                    profiles.push((profile.clone(), table_word));
                    table_word += profile.table().0.len() as u32;
                }
            }
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
                    lights.v3(Vec3::zero(), 0.0);
                    lights.v3(Vec3::zero(), 0.0);
                }
                RtLight::Point {
                    position,
                    color,
                    intensity,
                } => {
                    lights.v3(*position, 1.0);
                    lights.v3(*color * *intensity, 0.0);
                    lights.v3(Vec3::zero(), 0.0);
                    lights.v3(Vec3::zero(), 0.0);
                }
                RtLight::Luminaire {
                    position,
                    down,
                    front,
                    color,
                    profile,
                    scale,
                } => {
                    // A point light with the word of its table plus one and the rows of the table
                    let (table, rows) = profile_table(&profiles, profile);
                    lights.v3(*position, 1.0);
                    lights.v3(*color * *scale, 0.0);
                    lights.v3(*down, (table + 1) as f32);
                    lights.v3(*front, rows as f32);
                }
                RtLight::Rect {
                    corner,
                    edge_u,
                    edge_v,
                    color,
                    luminance,
                    two_sided,
                } => {
                    lights.v3(*corner, 2.0);
                    lights.v3(*color * *luminance, 0.0);
                    lights.v3(*edge_u, if *two_sided { 1.0 } else { 0.0 });
                    lights.v3(*edge_v, 0.0);
                }
                RtLight::Sphere {
                    center,
                    radius,
                    color,
                    luminance,
                } => {
                    lights.v3(*center, 3.0);
                    lights.v3(*color * *luminance, 0.0);
                    lights.v3(vec3(*radius, 0.0, 0.0), 0.0);
                    lights.v3(Vec3::zero(), 0.0);
                }
            }
        }
        for (profile, _) in &profiles {
            profile.table().0.iter().for_each(|v| lights.f(*v));
        }

        let storage = |label, words: &Words| {
            filled_buffer(gpu, label, &words.bytes(), w::BufferUsages::STORAGE)
        };
        let texels = texel_words(scene.textures(), max_bytes);
        Self {
            revision: scene.revision(),
            nodes: storage("ray tracing nodes", &nodes),
            triangles: storage("ray tracing triangles", &triangles),
            vertices: storage("ray tracing vertices", &vertices),
            instances: storage("ray tracing instances", &instances),
            materials: storage("ray tracing materials", &materials),
            lights,
            profiles,
            texels: storage("ray tracing textures", &texels),
        }
    }
}

/// Words of a material in trace.wgsl.
const MATERIAL_WORDS: u32 = 24;

/// Cells of the hashed photon grid, PHOTON_CELLS in trace.wgsl.
const PHOTON_CELLS: u32 = 65536;
/// Words of a photon, PHOTON_WORDS in trace.wgsl.
const PHOTON_WORDS: u32 = 12;

///
/// Where the caustic photons of a call live in the buffer of lights and photons, in words (layout in trace.wgsl).
///
struct CausticPlan {
    source_list: Vec<crate::photons::CausticSource>,
    sphere_list: Vec<(Vec3, f32)>,
    per_source: u32,
    center: Vec3,
    extent: f32,
    counter: u32,
    counts: u32,
    sources: u32,
    spheres: u32,
    raw: u32,
    words: u32,
}

impl CausticPlan {
    /// The plan if caustics are asked for and the scene has solid glass and lights that cast them.
    fn new(
        scene: &RtScene,
        accel: &CpuAccel,
        settings: &TraceSettings,
        light_words: u32,
        max_binding: u64,
    ) -> Option<Self> {
        if settings.caustic_photons == 0 {
            return None;
        }
        let sphere_list = crate::photons::glass_spheres(scene);
        let source_list = crate::photons::caustic_sources(scene);
        let (center, extent) = crate::photons::scene_box(accel)?;
        if sphere_list.is_empty() || source_list.is_empty() {
            return None;
        }
        let counter = light_words.next_multiple_of(4);
        let counts = counter + 4;
        let sources = counts + 3 * PHOTON_CELLS;
        let spheres = sources + 16 * source_list.len() as u32;
        let raw = spheres + 4 * sphere_list.len() as u32;
        // Shot and sorted photons, as many as the largest buffer the device binds takes
        let room = (max_binding / 4).saturating_sub(raw as u64) / (2 * PHOTON_WORDS as u64);
        let photons = (settings.caustic_photons as u64).min(room);
        let per_source = (photons / source_list.len() as u64).max(1) as u32;
        let words = raw + 2 * PHOTON_WORDS * per_source * source_list.len() as u32;
        Some(Self {
            source_list,
            sphere_list,
            per_source,
            center,
            extent,
            counter,
            counts,
            sources,
            spheres,
            raw,
            words,
        })
    }

    fn capacity(&self) -> u32 {
        self.per_source * self.source_list.len() as u32
    }

    /// The sources and the glass spheres, written at `sources`.
    fn emitter_bytes(&self, profiles: &[(std::sync::Arc<crate::IesProfile>, u32)]) -> Vec<u8> {
        use crate::photons::CausticSource;
        let mut words = Words::default();
        for source in &self.source_list {
            // Four vec4 each: kind and position or direction, light, the profile's down and front as for lights
            match source {
                CausticSource::Parallel {
                    direction,
                    light,
                    cos_radius,
                } => {
                    words.v3(*direction, 0.0);
                    words.v3(*light, *cos_radius);
                    words.v3(Vec3::zero(), 0.0);
                    words.v3(Vec3::zero(), 0.0);
                }
                CausticSource::Point {
                    position,
                    light,
                    profile,
                } => {
                    words.v3(*position, 1.0);
                    words.v3(*light, 1.0);
                    match profile {
                        Some((profile, down, front)) => {
                            let (table, rows) = profile_table(profiles, profile);
                            words.v3(*down, (table + 1) as f32);
                            words.v3(*front, rows as f32);
                        }
                        None => {
                            words.v3(Vec3::zero(), 0.0);
                            words.v3(Vec3::zero(), 0.0);
                        }
                    }
                }
            }
        }
        for (center, radius) in &self.sphere_list {
            words.v3(*center, *radius);
        }
        words.bytes()
    }
}

/// Word of a profile's table in the buffer of lights and the number of its rows.
fn profile_table(
    profiles: &[(std::sync::Arc<crate::IesProfile>, u32)],
    profile: &std::sync::Arc<crate::IesProfile>,
) -> (u32, u32) {
    let word = profiles
        .iter()
        .find(|(p, _)| std::sync::Arc::ptr_eq(p, profile))
        .map_or(0, |(_, w)| *w);
    (word, profile.table().1 as u32)
}

/// Texture id meaning no texture, as in trace.wgsl.
const NONE: u32 = u32::MAX;

///
/// All textures in one buffer (layout in trace.wgsl). Textures are halved in size until they fit into `max_bytes`, the
/// largest buffer the device binds.
///
fn texel_words(textures: &[RtTexture], max_bytes: u64) -> Words {
    let mut textures: Vec<std::borrow::Cow<'_, RtTexture>> =
        textures.iter().map(std::borrow::Cow::Borrowed).collect();
    let size = |textures: &[std::borrow::Cow<'_, RtTexture>]| -> u64 {
        4 * (4
            + 4 * textures.len() as u64
            + textures.iter().map(|t| t.texels.len() as u64).sum::<u64>())
    };
    while size(&textures) > max_bytes {
        // Halve the largest texture
        let Some(largest) = (0..textures.len()).max_by_key(|&i| textures[i].texels.len()) else {
            break;
        };
        if textures[largest].texels.len() <= 1 {
            break;
        }
        textures[largest] = std::borrow::Cow::Owned(halved(&textures[largest]));
    }
    let mut words = Words::default();
    words.u(textures.len() as u32);
    (0..3).for_each(|_| words.u(0));
    let mut first = 4 + 4 * textures.len() as u32;
    for t in &textures {
        words.u(first);
        words.u(t.width);
        words.u(t.height);
        let wrap = |w: RtWrapping| match w {
            RtWrapping::Repeat => 0,
            RtWrapping::MirroredRepeat => 1,
            RtWrapping::ClampToEdge => 2,
        };
        words.u(u32::from(t.linear)
            | (u32::from(t.nearest) << 1)
            | (wrap(t.wrap_u) << 2)
            | (wrap(t.wrap_v) << 4));
        first += t.texels.len() as u32;
    }
    for t in &textures {
        t.texels
            .iter()
            .for_each(|&texel| words.u(u32::from_le_bytes(texel)));
    }
    words
}

/// The texture with half the width and height, each texel the average of four.
fn halved(t: &RtTexture) -> RtTexture {
    let (w, h) = ((t.width / 2).max(1), (t.height / 2).max(1));
    let mut texels = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (2 * x + dx).min(t.width - 1);
                let sy = (2 * y + dy).min(t.height - 1);
                let texel = t.texels[(sy * t.width + sx) as usize];
                (0..4).for_each(|c| sum[c] += texel[c] as u32);
            }
            texels.push(sum.map(|s| ((s + 2) / 4) as u8));
        }
    }
    RtTexture {
        width: w,
        height: h,
        texels,
        ..t.clone()
    }
}

/// The uniform block `Params` of trace.wgsl.
fn params_bytes(
    scene: &RtScene,
    camera: &Camera,
    width: u32,
    height: u32,
    settings: &TraceSettings,
    caustics: Option<&CausticPlan>,
    camera_inside: &crate::nesting::Inside,
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
    let mut screen_to_direction = (camera.projection() * view)
        .invert()
        .unwrap_or(Mat4::identity());
    if let Some(region) = &settings.view_region {
        screen_to_direction = screen_to_direction * region.ndc_to_view();
    }
    p.m4(&screen_to_direction);
    let at = |u: f32, v: f32| {
        let (u, v) = match &settings.view_region {
            Some(region) => region.to_view(u, v),
            None => (u, v),
        };
        camera.position_at_uv_coordinates((u, v))
    };
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
    match crate::lens::Lens::of(camera, settings.depth_of_field) {
        Some(lens) => {
            p.v3(lens.u, lens.focus_distance);
            p.v3(lens.v, 1.0);
            p.v3(lens.view, 0.0);
        }
        None => (0..12).for_each(|_| p.f(0.0)),
    }
    match caustics {
        Some(plan) => {
            p.f(0.0);
            p.f(1.0);
            p.f(plan.extent);
            p.f(0.0);
            p.v3(plan.center, 0.0);
            p.u(plan.counter);
            p.u(plan.counts);
            p.u(plan.sources);
            p.u(plan.spheres);
            p.u(plan.source_list.len() as u32);
            p.u(plan.sphere_list.len() as u32);
            p.u(plan.per_source);
            p.u(plan.raw);
        }
        None => (0..16).for_each(|_| p.f(0.0)),
    }
    match scene.fog() {
        Some(fog) => {
            p.v3(fog.medium.absorption, fog.medium.anisotropy);
            p.v3(fog.medium.scattering, fog.height);
            p.f(if fog.scale_height.is_finite() {
                1.0 / fog.scale_height
            } else {
                0.0
            });
            p.f(1.0);
        }
        None => (0..10).for_each(|_| p.f(0.0)),
    }
    p.f(if scene.has_media() { 1.0 } else { 0.0 });
    p.f(0.0);
    // The solids the camera is in
    let mut nest = [NONE; 4];
    for (slot, solid) in nest.iter_mut().zip(camera_inside.solids()) {
        *slot = solid.instance;
    }
    nest.iter().for_each(|&i| p.u(i));
    p.bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_shrink_to_fit_the_buffer() {
        let large = RtTexture::new(64, 32, vec![[200, 100, 50, 255]; 64 * 32]);
        let small = RtTexture::new(2, 2, vec![[0, 0, 0, 255]; 4]);
        let all = [large, small];
        let fits = texel_words(&all, 1 << 20);
        assert_eq!(fits.0.len(), 4 + 8 + 64 * 32 + 4);
        // Room for the header and a quarter of the large texture
        let shrunk = texel_words(&all, 4 * (4 + 8 + 32 * 16 + 4));
        assert_eq!(shrunk.0.len(), 4 + 8 + 32 * 16 + 4);
        assert_eq!(&shrunk.0[4..7], &[12, 32, 16]);
        assert_eq!(shrunk.0[12], u32::from_le_bytes([200, 100, 50, 255]));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn every_pass_is_timed_also_in_calls_with_many_passes() {
        let Ok(context) = three_d::Context::new_headless_wgpu() else {
            return;
        };
        let Some(tracer) = ComputeTracer::from_context(&context) else {
            return;
        };
        if !tracer.times_from_timestamps() {
            return;
        }
        let mut tracer = tracer.with_tile_size(8);
        let mut scene = RtScene::new();
        scene.set_environment(RtEnvironment::Uniform {
            color: vec3(1.0, 1.0, 1.0),
            luminance: 1000.0,
        });
        let camera = Camera::new_perspective(
            three_d_asset::Viewport::new_at_origo(40, 24),
            vec3(0.0, 0.0, 4.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            degrees(45.0),
            0.1,
            100.0,
        );
        let mut image = GpuRenderImage::new(tracer.gpu(), 40, 24);
        // 15 tiles: a call of one sample, each pass timed, and one of four samples, timed in groups of two passes
        tracer
            .trace_gpu(&scene, &camera, &mut image, &TraceSettings::default())
            .unwrap();
        let four = TraceSettings {
            samples_per_call: 4,
            ..Default::default()
        };
        tracer
            .trace_gpu(&scene, &camera, &mut image, &four)
            .unwrap();
        let _ = tracer.gpu.device.poll(w::PollType::wait_indefinitely());
        let times = tracer.take_call_times();
        assert_eq!(
            times.iter().map(|time| time.tiles).collect::<Vec<_>>(),
            [15, 60]
        );
        assert!(times.iter().all(|time| time.ms > 0.0), "{times:?}");
        assert_eq!(tracer.gpu_error(), None);
    }

    #[test]
    fn errors_of_the_gpu_are_caught() {
        let Ok(context) = three_d::Context::new_headless_wgpu() else {
            return;
        };
        let gpu = GpuDevice::from_context(&context).unwrap();
        let watch = ErrorWatch::default();
        let scopes = ErrorScopes::push(&gpu.device);
        assert!(watch.check().is_none());
        // Mapped for reading and bound as storage at once is not allowed
        let _ = gpu.device.create_buffer(&w::BufferDescriptor {
            label: None,
            size: 16,
            usage: w::BufferUsages::MAP_READ | w::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        scopes.pop_into(&watch);
        let error = watch.check().expect("an error");
        assert!(error.to_lowercase().contains("usage"), "{error}");
        // And the context does not see it
        assert!(context.error_check().is_ok());
    }

    /// wgpu reports invalid shaders only to an error handler, so the shaders are checked here on every platform.
    #[test]
    fn shaders_are_valid() {
        for (name, source) in [
            ("trace.wgsl", include_str!("compute/trace.wgsl")),
            ("resolve.wgsl", include_str!("compute/resolve.wgsl")),
            ("denoise.wgsl", include_str!("compute/denoise.wgsl")),
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
