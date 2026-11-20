//!
//! A progressive preview in a three-d application: traced a few samples per frame, finer while camera and scene stand
//! still, on the GPU where the context can and on the CPU otherwise.
//!

use crate::{
    Camera, CpuReferenceTracer, Denoiser, RenderImage, RtScene, ToneMapping, ToneOperator,
    TraceSettings, Tracer, TracerKind,
};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use three_d::{
    ColorTarget, Context, DepthTest, Effect, EffectMaterialId, Interpolation, Light, Program,
    RenderStates, RenderTarget, Texture2D, Viewer, Wrapping,
};
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

///
/// What the preview of a context runs on, with a reason to show the user, for example in a tooltip.
///
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewAvailability {
    /// Compute shaders on the GPU.
    Gpu,
    /// Only the CPU, much slower and at a lower resolution; the text says why.
    CpuOnly(String),
}

///
/// Why the preview started its image again, see [RaytracePreview::last_restart]. Every restart forgets the samples, so
/// an application that shows the preview should only cause one when the image really changes.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewRestart {
    /// The first image of the preview.
    First,
    /// The camera moved or turned, or its field of view or planes changed.
    Camera,
    /// The camera's viewport has another size; a window that was resized, but also one whose size the application
    /// passes on while it is minimized or before its layout settled.
    Viewport,
    /// Another scene, or the scene changed: another [RtScene] for the same content counts, as does every change that
    /// raises [RtScene::revision].
    Scene,
    /// [RaytracePreview::resize] with another size.
    Resize,
    /// [RaytracePreview::set_settings] with settings that change the image, or [RaytracePreview::restart].
    Settings,
    /// The preview went on with another engine: on the GPU once its shaders compiled, at a lower GPU resolution, or
    /// on the CPU.
    Engine,
}

impl PreviewAvailability {
    /// What a context can run.
    pub fn of(#[allow(unused_variables)] context: &Context) -> Self {
        #[cfg(feature = "wgpu")]
        {
            use crate::RaytraceSupport;
            if let Some(problem) = crate::driver_problem(context) {
                return Self::CpuOnly(problem);
            }
            if RaytraceSupport::of(context) >= RaytraceSupport::Compute {
                if let Some(problem) = context
                    .wgpu_adapter_info()
                    .and_then(|info| crate::gpu::preview_problem(&info))
                {
                    return Self::CpuOnly(problem);
                }
                return Self::Gpu;
            }
            if context.as_wgpu().is_some() {
                return Self::CpuOnly(
                    "The graphics card or driver has no compute shaders (needs WebGPU, Vulkan, Metal, DirectX 12 \
                     or OpenGL 4.3); ray tracing runs on the CPU."
                        .to_owned(),
                );
            }
        }
        if cfg!(target_arch = "wasm32") {
            return Self::CpuOnly(
                "In the browser WebGL2 has no compute shaders and this application does not use WebGPU; ray tracing \
                 runs on the CPU in a single thread, at a low resolution and in parts of every frame."
                    .to_owned(),
            );
        }
        Self::CpuOnly(
            "The OpenGL backend has no ray tracing on the GPU (needs the wgpu backend with WebGPU, Vulkan, Metal \
             or DirectX 12); ray tracing runs on the CPU."
                .to_owned(),
        )
    }
}

///
/// Settings of a [RaytracePreview], which may change at any time.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewSettings {
    /// Samples added per call of [RaytracePreview::update] on the GPU.
    pub gpu_samples_per_frame: u32,
    /// The GPU traces at width and height divided by this, at first; see `gpu_frame_time`.
    pub gpu_downscale: u32,
    ///
    /// Longest time the GPU may trace for one update: its own time, from timestamps at the beginning and end of the
    /// work (in browsers from when the browser says it is done), or, on devices without timestamp queries, the time
    /// between two updates. When three updates in a row take longer, the preview halves the GPU's resolution (down to a
    /// quarter) and, if that is not enough, goes on with the CPU, which traces beside the frames. One five times longer
    /// goes at once to the resolution that would be fast enough, or to the CPU (in browsers it also waits for three in a
    /// row); [RaytracePreview::availability] then says why. The first update after the GPU starts does not count, it
    /// builds the scene's structures and uses the pipelines for the first time. Weak GPUs such as Intel HD 4000 trace
    /// no faster than four CPU cores and would hold up every frame. `None` keeps the GPU at `gpu_downscale`, also when
    /// set later ([RaytracePreview::set_settings]). What the application draws and reads back in a frame does not
    /// count where the GPU's time is measured.
    ///
    pub gpu_frame_time: Option<std::time::Duration>,
    ///
    /// When the GPU traces at a lower resolution (`gpu_downscale`, or lowered by `gpu_frame_time`) and camera and
    /// scene stand still for 16 of its samples, it goes on at the full resolution: a sample spread over several
    /// updates, as many tiles in each as fit into `gpu_frame_time`, and shown once it has 4 samples; until then the
    /// image of the lower resolution stays. Only where the GPU's time is measured (timestamp queries, browsers) and
    /// with a `gpu_frame_time`. On by default.
    ///
    pub gpu_sharp_when_still: bool,
    /// The CPU traces at width and height divided by this, 4 by default and 8 in browsers.
    pub cpu_downscale: u32,
    ///
    /// Longest time an update traces on the CPU when it traces in the update (in browsers, or without
    /// `cpu_in_background`): a sample then takes several updates, row after row, and the image changes when it is
    /// complete, so the frames go on. `None` traces a whole sample in every update. 12 ms in browsers by default, else
    /// `None`.
    ///
    pub cpu_frame_time: Option<std::time::Duration>,
    ///
    /// The CPU traces on a thread of its own, one sample after the other, and [RaytracePreview::update] shows the
    /// newest image, so the frames do not wait for the tracer. Otherwise every update traces one sample and waits.
    /// Browsers have no threads, there the update always traces.
    ///
    pub cpu_in_background: bool,
    ///
    /// Threads the CPU traces with: by default one less than the machine has, at least one, so the application keeps
    /// a core for its frames. The threads that trace in the background run at a low priority on Linux and Android
    /// (nice 10). Browsers trace in one thread.
    ///
    pub cpu_threads: usize,
    /// No more samples are added after this many, the image stays as it is.
    pub max_samples: u32,
    /// Most bounces of a path.
    pub max_bounces: u32,
    ///
    /// Photons per sample for caustics of solid glass on the GPU, see [TraceSettings::caustic_photons]. They cost
    /// time only in scenes with such glass; 0 lets light pass the glass straight.
    ///
    pub gpu_caustic_photons: u32,
    /// The same on the CPU, fewer since every photon there takes as long as a few pixels.
    pub cpu_caustic_photons: u32,
    /// Blur in front of and behind the focus distance, `None` for everything sharp.
    pub depth_of_field: Option<crate::DepthOfField>,
    /// Exposure and curve of [RaytracePreview::render].
    pub tone_mapping: ToneMapping,
    /// Smooths the noise of the first samples with this filter, `None` to show the samples as they are.
    pub denoiser: Option<crate::AtrousDenoiser>,
}

/// One thread less than the machine has, at least one.
fn default_cpu_threads() -> usize {
    if cfg!(target_arch = "wasm32") {
        return 1;
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .saturating_sub(1)
        .max(1)
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self {
            gpu_samples_per_frame: 1,
            gpu_downscale: 1,
            gpu_frame_time: Some(std::time::Duration::from_millis(40)),
            gpu_sharp_when_still: true,
            cpu_downscale: if cfg!(target_arch = "wasm32") { 8 } else { 4 },
            cpu_frame_time: if cfg!(target_arch = "wasm32") {
                Some(std::time::Duration::from_millis(12))
            } else {
                None
            },
            cpu_in_background: true,
            cpu_threads: default_cpu_threads(),
            max_samples: 1024,
            max_bounces: 6,
            gpu_caustic_photons: 200_000,
            cpu_caustic_photons: 20_000,
            depth_of_field: None,
            tone_mapping: ToneMapping::default(),
            denoiser: Some(crate::AtrousDenoiser::default()),
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum Engine {
    #[cfg(feature = "wgpu")]
    Gpu {
        tracer: Box<crate::ComputeTracer>,
        image: crate::GpuRenderImage,
    },
    Cpu {
        tracer: CpuReferenceTracer,
        image: RenderImage,
        /// The sample traced in parts with [PreviewSettings::cpu_frame_time], and the next row of it
        pending: RenderImage,
        next_row: u32,
    },
    #[cfg(not(target_arch = "wasm32"))]
    CpuWorker {
        worker: crate::preview_worker::CpuWorker,
        width: u32,
        height: u32,
        /// Samples of the image in the texture.
        shown: u32,
        /// The scene handed to the worker, kept while id and revision stay the same.
        scene: Option<std::sync::Arc<RtScene>>,
    },
}

///
/// A ray traced view of an [RtScene] that gets finer every frame while camera and scene stay the same, for example
/// as an entry next to the raster examples:
///
/// ```ignore
/// let mut preview = RaytracePreview::new(&context, width, height);
/// // every frame
/// preview.update(&scene, &camera)?;
/// preview.render(&target);
/// ```
///
/// The camera is the camera of three-d (`&*camera` for a `three_d::Camera`), the preview starts again when it moves or
/// the scene changes.
///
pub struct RaytracePreview {
    context: Context,
    availability: PreviewAvailability,
    engine: Engine,
    texture: Texture2D,
    width: u32,
    height: u32,
    /// What the samples were traced for: camera matrices and viewport, scene and revision
    traced_for: Option<TracedFor>,
    /// Why the next image starts again, when it is not a change of camera or scene
    restart_cause: Option<PreviewRestart>,
    /// Why the image started again the last time, and how many times it did
    last_restart: Option<PreviewRestart>,
    restarts: u64,
    /// The GPU's time of the newest measured call, see [RaytracePreview::gpu_time]
    gpu_time: Option<std::time::Duration>,
    settings: PreviewSettings,
    /// What the GPU's resolution is divided by now
    gpu_scale: u32,
    pace: Pace,
    /// The compute tracer while its shaders compile on another thread, or in the background of the browser
    #[cfg(feature = "wgpu")]
    compiling: Option<Compiling>,
    /// The CPU engine with its texture and what it traced for, while the GPU traces: back when the preview goes to
    /// the CPU again, so its samples are not lost
    #[cfg(feature = "wgpu")]
    parked: Option<(Engine, Texture2D, Option<TracedFor>)>,
    /// The GPU's image at the full resolution while camera and scene stand still, see
    /// [PreviewSettings::gpu_sharp_when_still]
    #[cfg(feature = "wgpu")]
    sharp: Option<Sharp>,
    /// A tile at the full resolution took too long for [PreviewSettings::gpu_frame_time], until the image starts again
    #[cfg(feature = "wgpu")]
    sharp_too_slow: bool,
    /// The compute tracer while the CPU traces because the GPU was too slow for [PreviewSettings::gpu_frame_time]:
    /// back to the GPU with it when the limit changes
    #[cfg(feature = "wgpu")]
    slow_tracer: GpuTracer,
}

/// The GPU's image at the full resolution, see [PreviewSettings::gpu_sharp_when_still].
#[cfg(feature = "wgpu")]
struct Sharp {
    image: crate::GpuRenderImage,
    /// The texture of the full resolution, or, while that is shown as [RaytracePreview::texture], the texture of the
    /// lower resolution, back when the camera moves
    texture: Texture2D,
    shown: bool,
    /// The GPU's milliseconds per tile, see [next_tile_ms]
    tile_ms: Option<f64>,
}

/// Samples of the GPU's image at the lower resolution before the GPU goes on at the full one.
#[cfg(feature = "wgpu")]
const SHARP_AFTER: u32 = 16;
/// Samples of the image at the full resolution before it is shown instead.
#[cfg(feature = "wgpu")]
const SHARP_SHOWN_AFTER: u32 = 4;
/// Share of [PreviewSettings::gpu_frame_time] the tiles of an update are planned for: tiles of the sky take a tenth of
/// tiles with much of the scene, so the next ones may take longer than the ones before.
#[cfg(feature = "wgpu")]
const SHARP_SHARE: f64 = 0.75;

/// Tiles of the full resolution for the next update: as many as fit into the share of the limit, at least one.
#[cfg(feature = "wgpu")]
fn sharp_tiles(tile_ms: Option<f64>, limit: std::time::Duration, tiles: u32) -> u32 {
    match tile_ms {
        None => 1,
        Some(ms) => {
            let fit = limit.as_secs_f64() * 1000.0 * SHARP_SHARE / ms.max(1e-3);
            (fit.min(u32::MAX as f64) as u32).clamp(1, tiles.max(1))
        }
    }
}

/// The milliseconds per tile after a measured call: up at once to a slower call, down slowly after faster ones.
#[cfg(feature = "wgpu")]
fn next_tile_ms(tile_ms: Option<f64>, call: crate::compute::CallTime) -> Option<f64> {
    let ms = call.ms / call.tiles.max(1) as f64;
    Some(match tile_ms {
        Some(old) if old > ms => 0.8 * old + 0.2 * ms,
        _ => ms,
    })
}

/// Camera matrices and viewport, scene and revision.
type TracedFor = (Vec<f32>, u64, u64);

/// The compute tracer of the preview, while it is made.
#[cfg(feature = "wgpu")]
struct Compiling {
    #[cfg(not(target_arch = "wasm32"))]
    receiver: Option<std::sync::mpsc::Receiver<Box<crate::ComputeTracer>>>,
    /// On OpenGL: made on the thread of the context once the CPU shows an image, with the updates waited so far.
    /// wgpu's OpenGL backend locks the one context for every call, so a thread that compiles holds up the frames and
    /// wgpu gives up after a second.
    #[cfg(not(target_arch = "wasm32"))]
    deferred: Option<(
        crate::GpuDevice,
        Option<three_d::wgpu::AdapterInfo>,
        std::cell::Cell<u32>,
    )>,
    #[cfg(target_arch = "wasm32")]
    slot: std::rc::Rc<std::cell::RefCell<Option<GpuTracer>>>,
}

#[cfg(feature = "wgpu")]
impl Compiling {
    /// Makes the tracer for the context: natively on a thread, in a browser with the browser's background compiling.
    fn start(context: &Context) -> Option<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let gpu = crate::GpuDevice::from_context(context)?;
            let info = context.wgpu_adapter_info();
            if info
                .as_ref()
                .is_some_and(|i| i.backend == three_d::wgpu::Backend::Gl)
            {
                return Some(Self {
                    receiver: None,
                    deferred: Some((gpu, info, std::cell::Cell::new(0))),
                });
            }
            let (send, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                crate::priority::lower_this_thread(crate::priority::COMPILING);
                let _ = send.send(Box::new(crate::ComputeTracer::for_adapter(&gpu, info)));
            });
            Some(Self {
                receiver: Some(receiver),
                deferred: None,
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
            let sink = slot.clone();
            let context = context.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let tracer = crate::ComputeTracer::from_context_async(&context).await;
                *sink.borrow_mut() = Some(tracer.map(Box::new));
            });
            Some(Self { slot })
        }
    }

    ///
    /// The tracer once it is made, `Some(None)` if that failed, `None` while it is not done. On OpenGL it is made here
    /// when `shown` (the CPU shows an image), or after some updates without.
    ///
    fn poll(&self, #[allow(unused_variables)] shown: bool) -> Option<GpuTracer> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::sync::mpsc::TryRecvError;
            if let Some((gpu, info, updates)) = &self.deferred {
                updates.set(updates.get() + 1);
                if !shown && updates.get() < DEFERRED_UPDATES {
                    return None;
                }
                return Some(Some(Box::new(crate::ComputeTracer::for_adapter(
                    gpu,
                    info.clone(),
                ))));
            }
            let receiver = self.receiver.as_ref()?;
            match receiver.try_recv() {
                Ok(tracer) => Some(Some(tracer)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(None),
            }
        }
        #[cfg(target_arch = "wasm32")]
        self.slot.borrow_mut().take()
    }

    /// Waits for the tracer; a browser can not wait, there it is the tracer if it is already made.
    fn wait(self) -> GpuTracer {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(receiver) = &self.receiver {
            return receiver.recv().ok();
        }
        self.poll(true).flatten()
    }
}

/// Updates on OpenGL before the shaders are compiled on the context's thread even without an image of the CPU.
#[cfg(all(feature = "wgpu", not(target_arch = "wasm32")))]
const DEFERRED_UPDATES: u32 = 60;

/// The compute tracer, if the preview makes one.
#[cfg(feature = "wgpu")]
type GpuTracer = Option<Box<crate::ComputeTracer>>;
#[cfg(not(feature = "wgpu"))]
type GpuTracer = Option<std::convert::Infallible>;

/// Times of the GPU's work, or of the frames, while the GPU traces.
#[derive(Default)]
#[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
struct Pace {
    #[cfg(not(target_arch = "wasm32"))]
    last: Option<std::time::Instant>,
    /// Whether the times of the tracer's calls before this engine were dropped
    started: bool,
    /// Times measured since the engine was made: of the GPU's calls, or between frames without timestamps
    frames: u32,
    slow: u32,
}

#[cfg(feature = "wgpu")]
impl Pace {
    ///
    /// A frame that starts `now`, outside of browsers; see [Pace::step]. The first frame of an engine has no time
    /// before it; the time after it, with the first trace, is the warm-up.
    ///
    #[cfg(not(target_arch = "wasm32"))]
    fn frame(
        &mut self,
        now: std::time::Instant,
        limit: std::time::Duration,
        tracing: bool,
        gpu_scale: u32,
    ) -> Option<Result<u32, String>> {
        let last = self.last.replace(now)?;
        self.step(now - last, limit, true, tracing, gpu_scale)
    }

    ///
    /// One measured time: between two frames, in the browser of one call of the GPU. Returns the GPU's new divisor
    /// of width and height when the preview gets lighter, or why it goes on with the CPU. Without `at_once` a far
    /// too slow time also waits for [SLOW_FRAMES] in a row.
    ///
    fn step(
        &mut self,
        interval: std::time::Duration,
        limit: std::time::Duration,
        at_once: bool,
        tracing: bool,
        gpu_scale: u32,
    ) -> Option<Result<u32, String>> {
        self.frames += 1;
        // Only while tracing: a finished image costs the GPU nothing. The first time after a new engine has the scene's
        // structures and the first use of the pipelines in it.
        if !tracing || self.frames <= WARM_UP_FRAMES || interval <= limit {
            self.slow = 0;
            return None;
        }
        self.slow += 1;
        // Far too slow: at once to the resolution that would be fast enough, the time goes with the pixels
        let far_too_slow = interval > limit * FAR_TOO_SLOW;
        if self.slow < SLOW_FRAMES && !(far_too_slow && at_once) {
            return None;
        }
        let mut scale = gpu_scale * 2;
        if far_too_slow {
            let pixels = interval.as_secs_f32() / limit.as_secs_f32();
            while scale <= MAX_GPU_DOWNSCALE
                && pixels * (gpu_scale as f32 / scale as f32).powi(2) > 1.0
            {
                scale *= 2;
            }
        }
        Some(if scale <= MAX_GPU_DOWNSCALE {
            Ok(scale)
        } else {
            Err(format!(
                "The GPU needs {} ms per frame at 1/{} of the resolution, too slow even at 1/{}, so ray tracing \
                 runs on the CPU beside the frames.",
                interval.as_millis(),
                gpu_scale,
                MAX_GPU_DOWNSCALE
            ))
        })
    }
}

/// Times measured after a new engine that do not count: the first has the scene's structures and the first use of the
/// pipelines.
#[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
const WARM_UP_FRAMES: u32 = 1;
/// Slow frames in a row before the preview gets lighter.
#[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
const SLOW_FRAMES: u32 = 3;
/// A frame this many times slower than the limit makes the preview lighter at once.
#[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
const FAR_TOO_SLOW: u32 = 5;
/// The lowest GPU resolution before the CPU, as a divisor of width and height.
#[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
const MAX_GPU_DOWNSCALE: u32 = 4;

impl RaytracePreview {
    ///
    /// A preview of the given size, on the GPU where the context can, see [PreviewAvailability::of]. The shaders of
    /// the GPU compile on another thread, which takes seconds on some drivers; the CPU traces until they are done, see
    /// [RaytracePreview::is_compiling].
    ///
    pub fn new(context: &Context, width: u32, height: u32) -> Self {
        Self::with_availability(context, width, height, PreviewAvailability::of(context))
    }

    /// A preview on the CPU, whatever the context could do.
    pub fn new_on_cpu(context: &Context, width: u32, height: u32) -> Self {
        Self::with_availability(
            context,
            width,
            height,
            PreviewAvailability::CpuOnly("Asked for the CPU.".to_owned()),
        )
    }

    fn with_availability(
        context: &Context,
        width: u32,
        height: u32,
        availability: PreviewAvailability,
    ) -> Self {
        let settings = PreviewSettings::default();
        let gpu_scale = settings.gpu_downscale.max(1);
        #[cfg(feature = "wgpu")]
        let compiling = if availability == PreviewAvailability::Gpu {
            // The shaders take seconds to compile on some drivers; the CPU traces meanwhile
            Compiling::start(context)
        } else {
            None
        };
        let (engine, texture) = Self::engine(context, None, width, height, &settings, gpu_scale);
        Self {
            context: context.clone(),
            availability,
            engine,
            texture,
            width,
            height,
            traced_for: None,
            restart_cause: None,
            last_restart: None,
            restarts: 0,
            gpu_time: None,
            settings,
            gpu_scale,
            pace: Pace::default(),
            #[cfg(feature = "wgpu")]
            compiling,
            #[cfg(feature = "wgpu")]
            parked: None,
            #[cfg(feature = "wgpu")]
            sharp: None,
            #[cfg(feature = "wgpu")]
            sharp_too_slow: false,
            #[cfg(feature = "wgpu")]
            slow_tracer: None,
        }
    }

    /// On the GPU with the tracer, else on the CPU.
    fn engine(
        context: &Context,
        #[allow(unused_variables)] tracer: GpuTracer,
        width: u32,
        height: u32,
        settings: &PreviewSettings,
        #[allow(unused_variables)] gpu_scale: u32,
    ) -> (Engine, Texture2D) {
        #[cfg(feature = "wgpu")]
        if let Some(tracer) = tracer {
            let (w, h) = ((width / gpu_scale).max(1), (height / gpu_scale).max(1));
            let image = crate::GpuRenderImage::with_features(tracer.gpu(), w, h);
            let texture = empty_texture(context, w, h);
            return (Engine::Gpu { tracer, image }, texture);
        }
        let scale = settings.cpu_downscale.max(1);
        let (w, h) = ((width / scale).max(1), (height / scale).max(1));
        let texture = empty_texture(context, w, h);
        #[cfg(not(target_arch = "wasm32"))]
        if settings.cpu_in_background {
            return (
                Engine::CpuWorker {
                    worker: crate::preview_worker::CpuWorker::new(w, h),
                    width: w,
                    height: h,
                    shown: 0,
                    scene: None,
                },
                texture,
            );
        }
        (
            Engine::Cpu {
                tracer: CpuReferenceTracer::with_threads(settings.cpu_threads),
                image: RenderImage::with_features(w, h),
                pending: RenderImage::with_features(w, h),
                next_row: 0,
            },
            texture,
        )
    }

    /// What the preview runs on, with the reason when it is the CPU.
    pub fn availability(&self) -> &PreviewAvailability {
        &self.availability
    }

    /// The kind of tracer in use.
    pub fn kind(&self) -> TracerKind {
        match &self.engine {
            #[cfg(feature = "wgpu")]
            Engine::Gpu { .. } => TracerKind::Compute,
            Engine::Cpu { .. } => TracerKind::CpuReference,
            #[cfg(not(target_arch = "wasm32"))]
            Engine::CpuWorker { .. } => TracerKind::CpuReference,
        }
    }

    fn on_cpu(&self) -> bool {
        self.kind() == TracerKind::CpuReference
    }

    /// The settings.
    pub fn settings(&self) -> &PreviewSettings {
        &self.settings
    }

    ///
    /// Changes the settings. A new CPU resolution starts the preview again. A new
    /// [PreviewSettings::gpu_frame_time] goes back to [PreviewSettings::gpu_downscale] and watches the GPU's time
    /// anew, also from the CPU when the preview went there because the GPU was too slow: `None` keeps the GPU at the
    /// full resolution (with a `gpu_downscale` of 1) from then on.
    ///
    pub fn set_settings(&mut self, settings: PreviewSettings) {
        #[allow(unused_mut)]
        let mut rebuild = ((settings.cpu_downscale != self.settings.cpu_downscale
            || settings.cpu_in_background != self.settings.cpu_in_background)
            && self.on_cpu())
            || (settings.gpu_downscale != self.settings.gpu_downscale && !self.on_cpu());
        if settings.gpu_downscale != self.settings.gpu_downscale {
            self.gpu_scale = settings.gpu_downscale.max(1);
        }
        #[cfg(feature = "wgpu")]
        if settings.cpu_downscale != self.settings.cpu_downscale
            || settings.cpu_in_background != self.settings.cpu_in_background
        {
            self.parked = None;
        }
        #[cfg(feature = "wgpu")]
        if !settings.gpu_sharp_when_still || settings.gpu_frame_time.is_none() {
            self.leave_sharp();
        }
        let restart = settings.max_bounces != self.settings.max_bounces
            || settings.cpu_caustic_photons != self.settings.cpu_caustic_photons
            || settings.gpu_caustic_photons != self.settings.gpu_caustic_photons
            || settings.depth_of_field != self.settings.depth_of_field;
        #[cfg(feature = "wgpu")]
        let new_limit = settings.gpu_frame_time != self.settings.gpu_frame_time;
        self.settings = settings;
        #[cfg(feature = "wgpu")]
        if new_limit {
            self.sharp_too_slow = false;
            if let Some(tracer) = self.slow_tracer.take() {
                // Back from the CPU, which waits with its samples
                self.availability = PreviewAvailability::Gpu;
                self.gpu_scale = settings.gpu_downscale.max(1);
                self.restart_cause = Some(PreviewRestart::Settings);
                self.rebuild_with(Some(tracer), true);
                return;
            }
            if !self.on_cpu() && self.gpu_scale != settings.gpu_downscale.max(1) {
                self.gpu_scale = settings.gpu_downscale.max(1);
                rebuild = true;
            }
            self.pace = Pace::default();
        }
        if rebuild || restart {
            self.restart_cause = Some(PreviewRestart::Settings);
        }
        if rebuild {
            self.rebuild();
        } else if restart {
            self.restart();
        }
    }

    ///
    /// Changes the size of the preview, the size of the viewport it is traced for, and starts again; nothing happens
    /// for the same size, or for a width or height of 0 (a minimized window). Call it when the viewport changes, not
    /// when the size of [RaytracePreview::texture] differs: the texture is smaller on the CPU and on slow GPUs.
    ///
    pub fn resize(&mut self, width: u32, height: u32) {
        if (width, height) != (self.width, self.height) && width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self.restart_cause = Some(PreviewRestart::Resize);
            #[cfg(feature = "wgpu")]
            {
                self.parked = None;
            }
            self.rebuild();
        }
    }

    /// Makes the engine again for the size, settings and availability, with the compute tracer kept, and starts again.
    fn rebuild(&mut self) {
        #[allow(unused_variables)]
        let tracer: GpuTracer = match std::mem::replace(&mut self.engine, placeholder_engine()) {
            #[cfg(feature = "wgpu")]
            Engine::Gpu { tracer, .. } if self.availability == PreviewAvailability::Gpu => {
                Some(tracer)
            }
            _ => None,
        };
        // The engine is the placeholder now, which must not be parked over the CPU's samples
        self.rebuild_with(tracer, false);
    }

    /// Makes the engine with the tracer, on the CPU without one, and starts again. From the CPU to the GPU, with
    /// `park`, the CPU engine waits with its samples.
    fn rebuild_with(&mut self, tracer: GpuTracer, #[allow(unused_variables)] park: bool) {
        // The new engine's texture replaces the texture shown, whichever resolution it had
        #[cfg(feature = "wgpu")]
        {
            self.sharp = None;
            self.sharp_too_slow = false;
        }
        // Back to the CPU: with the samples it had
        #[cfg(feature = "wgpu")]
        if tracer.is_none() {
            if let Some((engine, texture, traced_for)) = self.parked.take() {
                self.pace = Pace::default();
                self.gpu_time = None;
                self.engine = engine;
                self.texture = texture;
                self.traced_for = traced_for;
                #[cfg(not(target_arch = "wasm32"))]
                if let Engine::CpuWorker { worker, .. } = &self.engine {
                    worker.adjust(
                        self.settings.max_samples,
                        self.settings.denoiser,
                        self.settings.cpu_threads,
                    );
                }
                return;
            }
        }
        #[cfg(feature = "wgpu")]
        let start = crate::compute::timing::now();
        let (engine, texture) = Self::engine(
            &self.context,
            tracer,
            self.width,
            self.height,
            &self.settings,
            self.gpu_scale,
        );
        #[cfg(feature = "wgpu")]
        if let (true, Engine::Gpu { tracer, image }) = (crate::compute::timing::enabled(), &engine)
        {
            use crate::compute::timing;
            timing::log(&format!(
                "preview made its GPU image {}x{} in {:.0} ms on the CPU",
                image.width(),
                image.height(),
                timing::now() - start
            ));
            tracer.gpu().queue.submit([]);
            timing::when_gpu_done(
                &tracer.gpu().queue,
                start,
                "the preview's GPU image".to_owned(),
            );
        }
        self.pace = Pace::default();
        self.gpu_time = None;
        let old_engine = std::mem::replace(&mut self.engine, engine);
        let old_texture = std::mem::replace(&mut self.texture, texture);
        let old_traced_for = self.traced_for.take();
        self.restart_cause.get_or_insert(PreviewRestart::Engine);
        // From the CPU to the GPU: the CPU's samples wait, the worker stops after the sample it traces
        #[cfg(feature = "wgpu")]
        if park && !self.on_cpu() && !matches!(old_engine, Engine::Gpu { .. }) {
            #[cfg(not(target_arch = "wasm32"))]
            if let Engine::CpuWorker { worker, shown, .. } = &old_engine {
                worker.adjust(*shown, self.settings.denoiser, self.settings.cpu_threads);
            }
            self.parked = Some((old_engine, old_texture, old_traced_for));
        }
        #[cfg(not(feature = "wgpu"))]
        let _ = (old_engine, old_texture, old_traced_for);
    }

    ///
    /// Whether the shaders of the GPU still compile on another thread, which takes seconds on some drivers. The CPU
    /// traces meanwhile, and the preview goes on with the GPU in the first [RaytracePreview::update] after.
    ///
    /// On wgpu's OpenGL backend no other thread can use the context while a frame is drawn, so there the shaders
    /// compile in the [RaytracePreview::update] after the CPU shows its first image (that update takes as long).
    ///
    pub fn is_compiling(&self) -> bool {
        #[cfg(feature = "wgpu")]
        return self.compiling.is_some();
        #[cfg(not(feature = "wgpu"))]
        false
    }

    ///
    /// Waits until the shaders of the GPU are compiled and goes on with the GPU, see [RaytracePreview::is_compiling].
    /// A browser can not wait: there it goes on with the GPU only if the shaders are already compiled.
    ///
    pub fn wait_for_gpu(&mut self) {
        #[cfg(feature = "wgpu")]
        if let Some(compiling) = self.compiling.take() {
            #[cfg(target_arch = "wasm32")]
            if compiling.slot.borrow().is_none() {
                self.compiling = Some(compiling);
                return;
            }
            self.take_gpu(compiling.wait());
        }
    }

    /// Goes on with the GPU once its shaders are compiled.
    #[cfg(feature = "wgpu")]
    fn poll_compiling(&mut self) {
        let shown = self.sample_count() > 0;
        if let Some(tracer) = self.compiling.as_ref().and_then(|c| c.poll(shown)) {
            self.compiling = None;
            self.take_gpu(tracer);
        }
    }

    #[cfg(feature = "wgpu")]
    fn take_gpu(&mut self, tracer: Option<Box<crate::ComputeTracer>>) {
        if tracer.is_none() {
            self.availability = PreviewAvailability::CpuOnly(
                "The ray tracing shaders failed to compile for the GPU; ray tracing runs on the CPU.".to_owned(),
            );
        }
        if self.availability == PreviewAvailability::Gpu {
            self.rebuild_with(tracer, true);
        }
    }

    /// Why the image starts again for the key, see [PreviewRestart].
    fn restart_reason(&self, key: &TracedFor) -> PreviewRestart {
        match (&self.traced_for, self.restart_cause) {
            (_, Some(cause)) => cause,
            (None, None) if self.restarts == 0 => PreviewRestart::First,
            (None, None) => PreviewRestart::Settings,
            (Some((camera, id, revision)), None) => {
                // The last two numbers of the camera's key are the viewport's size
                let n = camera.len();
                if n != key.0.len() || camera[n - 2..] != key.0[n - 2..] {
                    PreviewRestart::Viewport
                } else if *id != key.1 || *revision != key.2 {
                    PreviewRestart::Scene
                } else {
                    PreviewRestart::Camera
                }
            }
        }
    }

    ///
    /// Why the image started again the last time, `None` before the first [RaytracePreview::update]. For finding out
    /// what in an application starts the preview again, for example a camera or scene made anew in every frame, or a
    /// size passed on while the window is minimized; see [RaytracePreview::restarts].
    ///
    pub fn last_restart(&self) -> Option<PreviewRestart> {
        self.last_restart
    }

    /// How many times the image started, the first one included.
    pub fn restarts(&self) -> u64 {
        self.restarts
    }

    /// Forgets all samples.
    pub fn restart(&mut self) {
        self.restart_cause.get_or_insert(PreviewRestart::Settings);
        self.traced_for = None;
        #[cfg(feature = "wgpu")]
        {
            self.parked = None;
        }
    }

    /// Samples every pixel of the image shown has.
    pub fn sample_count(&self) -> u32 {
        #[cfg(feature = "wgpu")]
        if let Some(Sharp {
            image, shown: true, ..
        }) = &self.sharp
        {
            return image.sample_count();
        }
        match &self.engine {
            #[cfg(feature = "wgpu")]
            Engine::Gpu { image, .. } => image.sample_count(),
            Engine::Cpu { image, .. } => image.sample_count(),
            #[cfg(not(target_arch = "wasm32"))]
            Engine::CpuWorker { shown, .. } => *shown,
        }
    }

    /// Whether [PreviewSettings::max_samples] is reached, so the image does not change any more.
    pub fn is_done(&self) -> bool {
        #[cfg(feature = "wgpu")]
        let (traced, sharp_next) = match &self.sharp {
            Some(sharp) => (sharp.image.sample_count(), false),
            None => (self.sample_count(), self.wants_sharp()),
        };
        #[cfg(not(feature = "wgpu"))]
        let (traced, sharp_next) = (self.sample_count(), false);
        self.traced_for.is_some() && traced >= self.settings.max_samples && !sharp_next
    }

    ///
    /// Whether the GPU goes on at the full resolution in the next update, see [PreviewSettings::gpu_sharp_when_still]:
    /// it traces at a lower one, and the image has enough samples since camera and scene stand still.
    ///
    #[cfg(feature = "wgpu")]
    fn wants_sharp(&self) -> bool {
        let Engine::Gpu { tracer, image } = &self.engine else {
            return false;
        };
        self.sharp.is_none()
            && !self.sharp_too_slow
            && self.settings.gpu_sharp_when_still
            && self.settings.gpu_frame_time.is_some()
            && tracer.measures_call_times()
            && self.traced_for.is_some()
            && (image.width() < self.width || image.height() < self.height)
            && image.sample_count() >= SHARP_AFTER.min(self.settings.max_samples).max(1)
    }

    /// The image at the full resolution, traced from the next update on.
    #[cfg(feature = "wgpu")]
    fn start_sharp(&mut self) {
        let Engine::Gpu { tracer, image } = &self.engine else {
            return;
        };
        if crate::compute::timing::enabled() {
            crate::compute::timing::log(&format!(
                "preview goes on at {}x{} after {} samples at {}x{}",
                self.width,
                self.height,
                image.sample_count(),
                image.width(),
                image.height()
            ));
        }
        // The calls at the lower resolution do not tell the time of the tiles
        tracer.restart_call_times();
        self.sharp = Some(Sharp {
            image: crate::GpuRenderImage::with_features(tracer.gpu(), self.width, self.height),
            texture: empty_texture(&self.context, self.width, self.height),
            shown: false,
            tile_ms: None,
        });
    }

    /// Back to the image at the lower resolution, which is shown again.
    #[cfg(feature = "wgpu")]
    fn leave_sharp(&mut self) {
        if let Some(mut sharp) = self.sharp.take() {
            if crate::compute::timing::enabled() {
                crate::compute::timing::log(&format!(
                    "preview leaves the full resolution after {} samples there, {:?} ms per tile",
                    sharp.image.sample_count(),
                    sharp.tile_ms
                ));
            }
            if sharp.shown {
                std::mem::swap(&mut self.texture, &mut sharp.texture);
            }
            // The lower resolution starts counting slow calls anew
            self.pace.slow = 0;
            if let Engine::Gpu { tracer, .. } = &self.engine {
                // Calls of the full resolution do not count for the lower one
                tracer.restart_call_times();
            }
        }
    }

    ///
    /// Adds samples for the scene seen by the camera, after starting again if either changed, and writes the average
    /// into [RaytracePreview::texture]. Returns whether the image changed. With [PreviewSettings::cpu_in_background]
    /// the samples are added on another thread and this shows the newest image; the scene is copied for that thread
    /// whenever it changed.
    ///
    pub fn update(&mut self, scene: &RtScene, camera: &Camera) -> Result<bool, crate::RtError> {
        // Nothing to see, for example in a minimized window: the image waits
        if camera.viewport().width == 0 || camera.viewport().height == 0 {
            return Ok(false);
        }
        #[cfg(feature = "wgpu")]
        self.poll_compiling();
        let key = (camera_key(camera), scene.id(), scene.revision());
        let changed = self.traced_for.as_ref() != Some(&key);
        let reason = changed.then(|| self.restart_reason(&key));
        #[cfg(feature = "wgpu")]
        if changed {
            self.leave_sharp();
            self.sharp_too_slow = false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Engine::CpuWorker {
            worker,
            width,
            height,
            shown,
            scene: handed,
        } = &mut self.engine
        {
            if changed {
                let same_scene = handed
                    .as_ref()
                    .is_some_and(|s| s.id() == scene.id() && s.revision() == scene.revision());
                if !same_scene {
                    *handed = Some(std::sync::Arc::new(scene.clone()));
                }
                let mut small = camera.clone();
                small.set_viewport(three_d_asset::Viewport::new_at_origo(*width, *height));
                worker.start(crate::preview_worker::Job {
                    scene: handed.clone().expect("handed over"),
                    camera: small,
                    max_bounces: self.settings.max_bounces,
                    caustic_photons: self.settings.cpu_caustic_photons,
                    depth_of_field: self.settings.depth_of_field,
                    max_samples: self.settings.max_samples,
                    denoiser: self.settings.denoiser,
                    threads: self.settings.cpu_threads,
                });
                *shown = 0;
                self.restart_cause = None;
                self.last_restart = reason;
                self.restarts += 1;
                self.traced_for = Some(key);
            } else {
                worker.adjust(
                    self.settings.max_samples,
                    self.settings.denoiser,
                    self.settings.cpu_threads,
                );
            }
            return Ok(match worker.take()? {
                Some(finished) => {
                    self.texture.fill(&finished.rgba);
                    *shown = finished.samples;
                    true
                }
                None => false,
            });
        }
        #[cfg(feature = "wgpu")]
        if matches!(self.engine, Engine::Gpu { .. }) && self.context.is_lost() {
            // A lost device does nothing and tells that it is done at once
            self.availability = PreviewAvailability::CpuOnly(
                "The graphics device was lost (a driver crash or reset); ray tracing runs on the CPU.".to_owned(),
            );
            self.rebuild();
            return self.update(scene, camera);
        }
        #[cfg(feature = "wgpu")]
        if let Engine::Gpu { tracer, .. } = &self.engine {
            if scene.has_media() {
                if let Some(why) = tracer.media_problem() {
                    // For good: the scene may get its media back any time
                    self.availability = PreviewAvailability::CpuOnly(why);
                    self.rebuild();
                    return self.update(scene, camera);
                }
            }
        }
        #[cfg(feature = "wgpu")]
        if matches!(self.engine, Engine::Gpu { .. }) && self.pace_gpu() {
            // Lighter now: the next update starts on the new engine
            return self.update(scene, camera);
        }
        if changed {
            match &mut self.engine {
                #[cfg(feature = "wgpu")]
                Engine::Gpu { image, .. } => image.reset(),
                Engine::Cpu {
                    image,
                    pending,
                    next_row,
                    ..
                } => {
                    image.reset();
                    pending.reset();
                    *next_row = 0;
                }
                #[cfg(not(target_arch = "wasm32"))]
                Engine::CpuWorker { .. } => unreachable!("handled above"),
            }
            self.restart_cause = None;
            self.last_restart = reason;
            self.restarts += 1;
            self.traced_for = Some(key);
        }
        #[cfg(feature = "wgpu")]
        if self.wants_sharp() {
            self.start_sharp();
        }
        if !changed && self.is_done() {
            return Ok(false);
        }
        let settings = TraceSettings {
            max_bounces: self.settings.max_bounces,
            caustic_photons: self.settings.cpu_caustic_photons,
            depth_of_field: self.settings.depth_of_field,
            ..Default::default()
        };
        #[cfg(feature = "wgpu")]
        if let (Some(sharp), Engine::Gpu { tracer, .. }, Some(limit)) = (
            &mut self.sharp,
            &mut self.engine,
            self.settings.gpu_frame_time,
        ) {
            if sharp
                .tile_ms
                .is_some_and(|ms| ms > 2000.0 * limit.as_secs_f64())
            {
                // One tile alone takes far too long: the lower resolution until the image starts again
                if crate::compute::timing::enabled() {
                    crate::compute::timing::log(
                        "preview: a tile at the full resolution takes too long",
                    );
                }
                self.leave_sharp();
                self.sharp_too_slow = true;
                return self.update(scene, camera);
            }
            if tracer.unfinished_calls() > crate::gpu::frames_ahead(tracer.adapter_info()) {
                return Ok(false);
            }
            let settings = TraceSettings {
                caustic_photons: self.settings.gpu_caustic_photons,
                ..settings
            };
            let tiles = sharp_tiles(
                sharp.tile_ms,
                limit,
                tracer.tiles_per_sample(sharp.image.width(), sharp.image.height()),
            );
            let mut full = camera.clone();
            full.set_viewport(three_d_asset::Viewport::new_at_origo(
                sharp.image.width(),
                sharp.image.height(),
            ));
            match tracer.trace_gpu_tiles(scene, &full, &mut sharp.image, &settings, tiles) {
                Ok(true) => {}
                Ok(false) => return Ok(false),
                Err(crate::RtError::Gpu(why)) => {
                    self.availability = PreviewAvailability::CpuOnly(format!(
                        "The GPU failed to trace ({why}); ray tracing runs on the CPU."
                    ));
                    self.rebuild();
                    return self.update(scene, camera);
                }
                Err(error) => return Err(error),
            }
            if sharp.image.sample_count() < SHARP_SHOWN_AFTER.min(self.settings.max_samples) {
                return Ok(false);
            }
            let target = if sharp.shown {
                &self.texture
            } else {
                &sharp.texture
            };
            match &self.settings.denoiser {
                Some(denoiser) => sharp
                    .image
                    .write_denoised_to_three_d_texture(target, denoiser),
                None => sharp.image.write_to_three_d_texture(target),
            }
            if !sharp.shown {
                std::mem::swap(&mut self.texture, &mut sharp.texture);
                sharp.shown = true;
                if crate::compute::timing::enabled() {
                    crate::compute::timing::log(&format!(
                        "preview shows the full resolution, {:?} ms per tile, {tiles} tile(s) per update",
                        sharp.tile_ms
                    ));
                }
            }
            return Ok(true);
        }
        match &mut self.engine {
            #[cfg(feature = "wgpu")]
            Engine::Gpu { tracer, image } => {
                // In a browser: at most one frame ahead of the GPU, on Intel graphics of the 7th generation not even that
                if tracer.unfinished_calls() > crate::gpu::frames_ahead(tracer.adapter_info()) {
                    return Ok(false);
                }
                let settings = TraceSettings {
                    samples_per_call: self.settings.gpu_samples_per_frame.max(1),
                    caustic_photons: self.settings.gpu_caustic_photons,
                    ..settings
                };
                let mut small = camera.clone();
                small.set_viewport(three_d_asset::Viewport::new_at_origo(
                    image.width(),
                    image.height(),
                ));
                match tracer.trace_gpu(scene, &small, image, &settings) {
                    Ok(()) => {}
                    // On the CPU from now on, with the samples it had
                    Err(crate::RtError::Gpu(why)) => {
                        self.availability = PreviewAvailability::CpuOnly(format!(
                            "The GPU failed to trace ({why}); ray tracing runs on the CPU."
                        ));
                        self.rebuild();
                        return self.update(scene, camera);
                    }
                    Err(error) => return Err(error),
                }
                match &self.settings.denoiser {
                    Some(denoiser) => {
                        image.write_denoised_to_three_d_texture(&self.texture, denoiser)
                    }
                    None => image.write_to_three_d_texture(&self.texture),
                }
            }
            Engine::Cpu {
                tracer,
                image,
                pending,
                next_row,
            } => {
                if tracer.threads() != self.settings.cpu_threads.max(1)
                    && !cfg!(target_arch = "wasm32")
                {
                    tracer.set_threads(Some(self.settings.cpu_threads));
                }
                // The camera of the reduced resolution
                let mut small = camera.clone();
                small.set_viewport(three_d_asset::Viewport::new_at_origo(
                    image.width(),
                    image.height(),
                ));
                match self.settings.cpu_frame_time {
                    Some(budget) => {
                        // Row after row until the time is up; the image changes when the sample is complete
                        let start = Instant::now();
                        let height = image.height();
                        loop {
                            let row = *next_row;
                            tracer.trace_rows(
                                scene,
                                &small,
                                pending,
                                &settings,
                                image.sample_count(),
                                row..row + 1,
                            )?;
                            *next_row += 1;
                            if *next_row >= height {
                                break;
                            }
                            if start.elapsed() >= budget {
                                return Ok(false);
                            }
                        }
                        image.add_image(pending);
                        image.add_samples(1);
                        pending.reset();
                        *next_row = 0;
                    }
                    None => tracer.trace(scene, &small, image, &settings)?,
                }
                match self.settings.denoiser {
                    Some(mut denoiser) => denoiser
                        .denoise(image)?
                        .write_to_three_d_texture(&self.texture),
                    None => image.write_to_three_d_texture(&self.texture),
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Engine::CpuWorker { .. } => unreachable!("handled above"),
        }
        Ok(true)
    }

    ///
    /// Watches the GPU's time of every call (without timestamp queries the time between frames) while the GPU traces
    /// and makes the preview lighter after [SLOW_FRAMES] slow ones in a row: half the resolution, down to a quarter,
    /// then the CPU. One [FAR_TOO_SLOW] times slower than the limit goes at once to the resolution that would be fast
    /// enough, or to the CPU; in the browser it also waits for [SLOW_FRAMES] in a row. Returns whether it did.
    ///
    #[cfg(feature = "wgpu")]
    fn pace_gpu(&mut self) -> bool {
        let limit = self.settings.gpu_frame_time;
        let tracing = !self.is_done();
        let Engine::Gpu { tracer, .. } = &self.engine else {
            return false;
        };
        if let Some(sharp) = &mut self.sharp {
            // At the full resolution the tiles of an update follow the time, the resolution stays
            for call in tracer.take_call_times() {
                self.gpu_time = Some(std::time::Duration::from_secs_f64(call.ms / 1000.0));
                sharp.tile_ms = next_tile_ms(sharp.tile_ms, call);
            }
            return false;
        }
        // How the time was measured, for the reason when the preview goes to the CPU
        let measured = if tracer.times_from_timestamps() {
            "the GPU's own time, from timestamps"
        } else if tracer.measures_call_times() {
            "until the browser told that the work was done"
        } else {
            "the time between frames"
        };
        let timed = crate::compute::timing::enabled();
        let decision = if tracer.measures_call_times() {
            // The GPU's own time of every call
            if !self.pace.started {
                // Calls for the engine before, at another resolution, do not count
                tracer.restart_call_times();
                self.pace.started = true;
                return false;
            }
            // In the browser not at once on one slow call: a hidden page stops its frames, and the browser may tell
            // late about the at most two calls it still had
            let at_once = !cfg!(target_arch = "wasm32");
            let mut decision = None;
            for call in tracer.take_call_times() {
                let busy = std::time::Duration::from_secs_f64(call.ms / 1000.0);
                self.gpu_time = Some(busy);
                if timed && limit.is_some_and(|limit| busy > limit) {
                    crate::compute::timing::log(&format!(
                        "preview: a call of {} tile(s) took {:.1} ms at 1/{} ({measured}), more than the limit",
                        call.tiles, call.ms, self.gpu_scale
                    ));
                }
                if let (None, Some(limit)) = (&decision, limit) {
                    decision = self
                        .pace
                        .step(busy, limit, at_once, tracing, self.gpu_scale);
                }
            }
            decision
        } else {
            // Without timestamps: the time between frames
            #[cfg(not(target_arch = "wasm32"))]
            {
                limit.and_then(|limit| {
                    self.pace
                        .frame(std::time::Instant::now(), limit, tracing, self.gpu_scale)
                })
            }
            #[cfg(target_arch = "wasm32")]
            None
        };
        if let (true, Some(decision)) = (timed, &decision) {
            crate::compute::timing::log(&format!("preview: {decision:?} ({measured})"));
        }
        match decision {
            None => return false,
            Some(Ok(scale)) => self.gpu_scale = scale,
            Some(Err(why)) => {
                self.availability =
                    PreviewAvailability::CpuOnly(format!("{why} Measured: {measured}."));
                // Kept for another limit, see set_settings
                if let Engine::Gpu { tracer, .. } =
                    std::mem::replace(&mut self.engine, placeholder_engine())
                {
                    self.slow_tracer = Some(tracer);
                }
                self.rebuild_with(None, false);
                return true;
            }
        }
        self.rebuild();
        true
    }

    ///
    /// What the width and height of the GPU's image on screen are divided by now: raised by the preview on slow GPUs,
    /// 1 again while the image at the full resolution is shown (see [PreviewSettings::gpu_sharp_when_still]).
    ///
    pub fn gpu_downscale(&self) -> u32 {
        #[cfg(feature = "wgpu")]
        if self.sharp.as_ref().is_some_and(|sharp| sharp.shown) {
            return 1;
        }
        self.gpu_scale
    }

    ///
    /// The GPU's own time for the newest update it traced and measured: from timestamps at the beginning and end of the
    /// work, in browsers from when the browser said it was done. `None` on the CPU, before the first measured update,
    /// and on devices without timestamp queries. Unlike the time between frames it does not hold what the application
    /// draws and reads back; [PreviewSettings::gpu_frame_time] is the limit for it.
    ///
    pub fn gpu_time(&self) -> Option<std::time::Duration> {
        if self.on_cpu() {
            return None;
        }
        self.gpu_time
    }

    ///
    /// The average luminance of every pixel in cd/m² (linear, 32 bit floats), upright as three-d expects. It is
    /// smaller than the preview by [PreviewSettings::cpu_downscale] on the CPU and by
    /// [RaytracePreview::gpu_downscale] on the GPU.
    ///
    pub fn texture(&self) -> &Texture2D {
        &self.texture
    }

    /// Draws the image into the whole target, exposed and tone mapped with [PreviewSettings::tone_mapping] and in sRGB.
    pub fn render(&self, target: &RenderTarget) {
        let viewer = three_d::Camera::new_2d(target.viewport());
        target.apply_screen_effect(&self.effect(), &viewer, &[], None, None);
    }

    /// Draws the image into a color target, for example a texture, see [RaytracePreview::render].
    pub fn render_to_color(&self, target: &ColorTarget) {
        let viewer = three_d::Camera::new_2d(target.viewport());
        target.apply_screen_effect(&self.effect(), &viewer, &[], None, None);
    }

    fn effect(&self) -> DisplayEffect<'_> {
        DisplayEffect {
            texture: &self.texture,
            tone_mapping: self.settings.tone_mapping,
        }
    }
}

fn empty_texture(context: &Context, width: u32, height: u32) -> Texture2D {
    // 32 bit floats can not be interpolated everywhere (WebGPU), so the nearest texel is taken
    Texture2D::new_empty::<[f32; 4]>(
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

/// Everything about a camera that changes the image.
fn camera_key(camera: &Camera) -> Vec<f32> {
    let (view, projection) = (camera.view(), camera.projection());
    let view: &[f32; 16] = view.as_ref();
    let projection: &[f32; 16] = projection.as_ref();
    let viewport = camera.viewport();
    view.iter()
        .chain(projection.iter())
        .copied()
        .chain([viewport.width as f32, viewport.height as f32])
        .collect()
}

/// Draws the linear image with exposure, a tone curve and sRGB encoding.
struct DisplayEffect<'a> {
    texture: &'a Texture2D,
    tone_mapping: ToneMapping,
}

impl Effect for DisplayEffect<'_> {
    fn fragment_shader_source(
        &self,
        _lights: &[&dyn Light],
        _color_texture: Option<three_d::ColorTexture>,
        _depth_texture: Option<three_d::DepthTexture>,
    ) -> String {
        // The same curves as ToneMapping::apply and RenderImage::to_srgb8
        "
        uniform sampler2D radiance;
        uniform float exposure;
        uniform int toneOperator;

        in vec2 uvs;
        layout (location = 0) out vec4 outColor;

        vec3 aces(vec3 x) {
            return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
        }

        float encode(float c) {
            return c <= 0.0031308 ? 12.92 * c : 1.055 * pow(c, 1.0 / 2.4) - 0.055;
        }

        void main() {
            vec3 c = max(texture(radiance, uvs).rgb * exposure, vec3(0.0));
            if (toneOperator == 0) {
                c = min(c, vec3(1.0));
            } else if (toneOperator == 1) {
                c = c / (vec3(1.0) + c);
            } else {
                c = aces(c);
            }
            outColor = vec4(encode(c.r), encode(c.g), encode(c.b), 1.0);
        }
        "
        .to_owned()
    }

    fn id(
        &self,
        _color_texture: Option<three_d::ColorTexture>,
        _depth_texture: Option<three_d::DepthTexture>,
    ) -> EffectMaterialId {
        // In the range three-d leaves to other crates
        EffectMaterialId(0x4a52)
    }

    fn use_uniforms(
        &self,
        program: &Program,
        _viewer: &dyn Viewer,
        _lights: &[&dyn Light],
        _color_texture: Option<three_d::ColorTexture>,
        _depth_texture: Option<three_d::DepthTexture>,
    ) {
        program.use_texture("radiance", self.texture);
        program.use_uniform("exposure", self.tone_mapping.exposure_scale());
        program.use_uniform(
            "toneOperator",
            match self.tone_mapping.operator {
                ToneOperator::Clamp => 0i32,
                ToneOperator::Reinhard => 1,
                ToneOperator::Aces => 2,
            },
        );
    }

    fn render_states(&self) -> RenderStates {
        RenderStates {
            depth_test: DepthTest::Always,
            ..Default::default()
        }
    }
}

/// An engine that is replaced at once.
fn placeholder_engine() -> Engine {
    Engine::Cpu {
        tracer: CpuReferenceTracer::new(),
        image: RenderImage::new(1, 1),
        pending: RenderImage::new(1, 1),
        next_row: 0,
    }
}

#[cfg(all(test, feature = "wgpu", not(target_arch = "wasm32")))]
mod tests {
    use super::{next_tile_ms, sharp_tiles, Pace};
    use crate::compute::CallTime;
    use std::time::{Duration, Instant};

    const LIMIT: Duration = Duration::from_millis(40);

    #[test]
    fn the_first_frame_of_an_engine_does_not_count() {
        let start = Instant::now();
        let mut pace = Pace::default();
        // No time before the first frame; the time after it has the scene's structures and the pipelines' first use
        assert_eq!(pace.frame(start, LIMIT, true, 1), None);
        let mut now = start + Duration::from_millis(1182);
        assert_eq!(pace.frame(now, LIMIT, true, 1), None);
        for _ in 0..10 {
            now += Duration::from_millis(20);
            assert_eq!(pace.frame(now, LIMIT, true, 1), None);
        }
        // A far too slow frame after that counts at once: 300 ms are 7.5 times the limit, a quarter of the pixels
        now += Duration::from_millis(300);
        assert_eq!(pace.frame(now, LIMIT, true, 1), Some(Ok(4)));
    }

    #[test]
    fn three_slow_frames_halve_the_resolution() {
        let mut pace = Pace::default();
        assert_eq!(pace.step(LIMIT * 3, LIMIT, true, true, 1), None);
        let slow = LIMIT * 2;
        assert_eq!(pace.step(slow, LIMIT, true, true, 1), None);
        assert_eq!(pace.step(slow, LIMIT, true, true, 1), None);
        // A fast frame in between starts the count again
        assert_eq!(pace.step(LIMIT / 2, LIMIT, true, true, 1), None);
        assert_eq!(pace.step(slow, LIMIT, true, true, 1), None);
        assert_eq!(pace.step(slow, LIMIT, true, true, 1), None);
        assert_eq!(pace.step(slow, LIMIT, true, true, 1), Some(Ok(2)));
    }

    #[test]
    fn in_the_browser_a_far_too_slow_call_waits_for_three_in_a_row() {
        let mut pace = Pace::default();
        let far = LIMIT * 8;
        assert_eq!(pace.step(far, LIMIT, false, true, 1), None);
        assert_eq!(pace.step(far, LIMIT, false, true, 1), None);
        assert_eq!(pace.step(far, LIMIT, false, true, 1), None);
        assert_eq!(pace.step(far, LIMIT, false, true, 1), Some(Ok(4)));
    }

    #[test]
    fn too_slow_at_a_quarter_goes_to_the_cpu_and_a_finished_image_never() {
        let mut pace = Pace::default();
        let far = LIMIT * 30;
        assert_eq!(pace.step(far, LIMIT, true, true, 1), None);
        assert!(matches!(
            pace.step(far, LIMIT, true, true, 1),
            Some(Err(why)) if why.contains("1/4")
        ));
        let mut pace = Pace::default();
        for _ in 0..5 {
            assert_eq!(pace.step(far, LIMIT, true, false, 1), None);
        }
    }

    #[test]
    fn the_tiles_of_an_update_at_the_full_resolution_fit_into_the_limit() {
        // Unknown at first: one tile
        assert_eq!(sharp_tiles(None, LIMIT, 24), 1);
        // A share of 30 of the 40 ms
        assert_eq!(sharp_tiles(Some(10.0), LIMIT, 24), 3);
        assert_eq!(sharp_tiles(Some(2.0), LIMIT, 24), 15);
        // Never more than the sample has, never none
        assert_eq!(sharp_tiles(Some(0.1), LIMIT, 24), 24);
        assert_eq!(sharp_tiles(Some(100.0), LIMIT, 24), 1);
        assert_eq!(sharp_tiles(Some(0.0), LIMIT, 24), 24);
    }

    #[test]
    fn a_slower_call_counts_at_once_a_faster_one_slowly() {
        let call = |ms, tiles| CallTime { ms, tiles };
        // Milliseconds per tile of the call
        let ms = next_tile_ms(None, call(30.0, 3)).unwrap();
        assert_eq!(ms, 10.0);
        // Tiles with much of the scene after tiles of the sky
        let ms = next_tile_ms(Some(ms), call(40.0, 2)).unwrap();
        assert_eq!(ms, 20.0);
        // Back to the sky: a fifth of the way at a time
        let ms = next_tile_ms(Some(ms), call(10.0, 5)).unwrap();
        assert!((ms - 16.4).abs() < 1e-9);
    }
}
