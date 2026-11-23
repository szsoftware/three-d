use crate::thread::ThreadBound;
use crate::*;
use std::sync::Arc;
use std::time::Duration;
use three_d::{Context, Texture2D};
use three_d_raytrace::{FinalRender, PreviewSettings, RaytracePreview};

///
/// The device all views of a scene use: final renders, the raytraced preview and the realtime view. On the GPU where it
/// can (wgpu: Vulkan, Metal, DX12; WebGPU in the browser), else the CPU tracer with the reason in
/// [Renderer::cpu_reason]. It belongs to the thread that created it.
///
#[derive(uniffi::Object)]
pub struct Renderer {
    pub(crate) state: ThreadBound<RendererState>,
    cpu_reason: Option<String>,
    device: String,
}

pub(crate) struct RendererState {
    pub context: Option<Context>,
    /// The engine of final renders: a small preview that owns the GPU tracer (and its slow fallback)
    engine: Option<RaytracePreview>,
    pub cpu_threads: usize,
}

#[uniffi::export]
impl Renderer {
    ///
    /// A renderer without a window: wgpu if `prefer_gpu` and an adapter is there, else (on Linux) OpenGL through EGL
    /// for the realtime view with the CPU tracer, else only the CPU tracer for final renders. In the browser the
    /// application creates the context and hands it over with `Renderer::with_context` from Rust.
    ///
    #[uniffi::constructor]
    pub fn headless(prefer_gpu: bool) -> Arc<Self> {
        let mut reasons = Vec::new();
        #[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
        if prefer_gpu {
            match Context::new_headless_wgpu() {
                Ok(context) => return Self::with_context(context),
                Err(e) => reasons.push(format!("no wgpu adapter: {e}")),
            }
        }
        if !prefer_gpu {
            reasons.push("Asked for the CPU.".to_owned());
        }
        #[cfg(target_os = "linux")]
        match Context::new_headless_gl_with_screen(1, 1, true, false) {
            Ok(context) => {
                let renderer = Self::build(Some(context), true);
                return Arc::new(Self {
                    cpu_reason: Some(join(&reasons, &renderer.cpu_reason)),
                    ..renderer
                });
            }
            Err(e) => reasons.push(format!("no OpenGL either: {e}")),
        }
        let renderer = Self::build(None, false);
        Arc::new(Self {
            cpu_reason: Some(reasons.join(" ")),
            ..renderer
        })
    }

    /// Only the CPU tracer, without any graphics device: final renders work, preview and realtime view do not.
    #[uniffi::constructor]
    pub fn cpu_only() -> Arc<Self> {
        Arc::new(Self {
            cpu_reason: Some("Asked for the CPU.".to_owned()),
            ..Self::build(None, false)
        })
    }

    /// Why final renders and the preview run on the CPU, `None` on the GPU.
    pub fn cpu_reason(&self) -> Option<String> {
        self.cpu_reason.clone()
    }

    /// The tracer in words, e.g. for a tooltip: GPU (compute shaders), compiling, or CPU with its threads.
    pub fn device(&self) -> String {
        self.state
            .with(|s| match &s.engine {
                Some(e) if e.is_compiling() => "GPU, compiling the shaders".to_owned(),
                Some(e) => match e.kind() {
                    three_d_raytrace::TracerKind::CpuReference => {
                        format!("CPU, {} threads", s.cpu_threads)
                    }
                    kind => format!("GPU ({kind:?})"),
                },
                None => self.device.clone(),
            })
            .unwrap_or_else(|_| self.device.clone())
    }

    /// Whether the renderer has a graphics context, which the preview and the realtime view need.
    pub fn has_context(&self) -> bool {
        self.state.with(|s| s.context.is_some()).unwrap_or(false)
    }

    /// Threads of the CPU tracer, all cores by default.
    pub fn set_cpu_threads(&self, threads: u32) -> Result<(), RtFfiError> {
        self.state
            .with(|s| s.cpu_threads = (threads as usize).max(1))
    }

    ///
    /// A final render of `width` x `height` pixels with `samples` per pixel, e.g. A4 at 300 dpi (3508 x 2480). Call
    /// [FinalJob::step] until it returns true, between frames of the application.
    ///
    #[allow(clippy::too_many_arguments)]
    pub fn final_render(
        self: Arc<Self>,
        scene: Arc<Scene>,
        camera: CameraSpec,
        width: u32,
        height: u32,
        samples: u32,
        look: Look,
        step_ms: u32,
    ) -> Result<Arc<FinalJob>, RtFfiError> {
        let (width, height) = (width.max(1), height.max(1));
        let render = FinalRender::new(&camera.camera(width, height), width, height, samples.max(1))
            .with_settings(look.settings(&camera))
            .with_tone_mapping(look.tone_mapping())
            .with_frame_time(Duration::from_millis(step_ms.max(1) as u64));
        let render = if look.denoise {
            render
        } else {
            render.with_denoiser(None)
        };
        // Created here so that it belongs to the renderer's thread
        self.state.with(|_| ())?;
        Ok(Arc::new(FinalJob {
            renderer: self,
            scene,
            render: ThreadBound::new(render),
            source: PhotoSource::Renderer,
        }))
    }

    /// A raytraced preview of `width` x `height` pixels that gets sharper while scene and camera stay the same.
    pub fn preview(
        self: Arc<Self>,
        scene: Arc<Scene>,
        camera: CameraSpec,
        width: u32,
        height: u32,
        look: Look,
    ) -> Result<Arc<PreviewJob>, RtFfiError> {
        let state = self.state.with(|s| -> Result<PreviewState, RtFfiError> {
            let context = s
                .context
                .clone()
                .ok_or_else(|| invalid("a preview needs a renderer with a context"))?;
            Ok(PreviewState::new(
                &context,
                width,
                height,
                look,
                s.cpu_threads,
                PreviewOptions::default(),
            ))
        })??;
        Ok(Arc::new(PreviewJob {
            renderer: self,
            scene,
            state: ThreadBound::new(PreviewJobState {
                preview: state,
                camera,
                photo: false,
            }),
        }))
    }
}

/// The renderer of a web page. Exported on every target, so that the common API of the bindings has it; outside the
/// browser it answers with an error.
#[uniffi::export]
impl Renderer {
    ///
    /// A renderer on the canvas with the id `canvas_id` in the page: WebGPU where the browser has it on the graphics card
    /// and `prefer_gpu` is set, WebGL2 otherwise. The canvas only gives the device; the views hand their pixels to Kotlin
    /// (Compose draws them), so it may stay hidden. Asynchronous, because the browser cannot wait for WebGPU otherwise.
    /// Outside the browser `RtFfiError::Gpu` ("only in the browser"): use [Renderer::headless] there.
    ///
    #[uniffi::constructor]
    pub async fn from_canvas(canvas_id: String, prefer_gpu: bool) -> Result<Arc<Self>, RtFfiError> {
        #[cfg(target_arch = "wasm32")]
        {
            cargo_uniffi_support::LocalExecutor::new()
                .run(move || async move {
                    use wasm_bindgen::JsCast;
                    let canvas = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.get_element_by_id(&canvas_id))
                        .ok_or_else(|| invalid(format!("no element with the id {canvas_id:?}")))?
                        .dyn_into::<web_sys::HtmlCanvasElement>()
                        .map_err(|_| invalid(format!("the element {canvas_id:?} is no canvas")))?;
                    let preference = if prefer_gpu {
                        three_d::BackendPreference::Auto
                    } else {
                        three_d::BackendPreference::Gl
                    };
                    let context = three_d::WindowedContext::from_canvas_async(
                        canvas,
                        three_d::SurfaceSettings::default(),
                        preference,
                    )
                    .await
                    .map_err(|e| RtFfiError::Gpu { why: e.to_string() })?;
                    Ok(Self::with_context((*context).clone()))
                })
                .await
                .map_err(|e| RtFfiError::Gpu { why: e.to_string() })?
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (canvas_id, prefer_gpu);
            Err(RtFfiError::Gpu {
                why: "Renderer.fromCanvas works only in the browser, use Renderer.headless"
                    .to_owned(),
            })
        }
    }
}

impl Renderer {
    /// A renderer on the context of the application, for example the one of the browser's canvas or of the POC.
    pub fn with_context(context: Context) -> Arc<Self> {
        Arc::new(Self::build(Some(context), true))
    }

    fn build(context: Option<Context>, gpu_engine: bool) -> Self {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let (engine, cpu_reason, device) = match &context {
            Some(context) if gpu_engine => {
                let engine = RaytracePreview::new(context, 64, 64);
                let reason = match engine.availability() {
                    three_d_raytrace::PreviewAvailability::Gpu => None,
                    three_d_raytrace::PreviewAvailability::CpuOnly(why) => Some(why.clone()),
                };
                let device = format!("{:?}", engine.kind());
                (Some(engine), reason, device)
            }
            _ => (None, None, format!("CPU, {threads} threads")),
        };
        Self {
            state: ThreadBound::new(RendererState {
                context,
                engine,
                cpu_threads: threads,
            }),
            cpu_reason,
            device,
        }
    }

    /// The context, for views of other crates on the same device.
    pub fn context(&self) -> Result<Option<Context>, RtFfiError> {
        self.state.with(|s| s.context.clone())
    }
}

#[cfg(target_os = "linux")]
fn join(reasons: &[String], last: &Option<String>) -> String {
    let mut all = reasons.to_vec();
    all.extend(last.clone());
    all.join(" ")
}

/// A final render in progress, see [Renderer::final_render].
#[derive(uniffi::Object)]
pub struct FinalJob {
    renderer: Arc<Renderer>,
    scene: Arc<Scene>,
    render: ThreadBound<FinalRender>,
    /// The preview whose GPU traces the photo, when it goes on with that preview's samples
    source: PhotoSource,
}

impl FinalJob {
    pub(crate) fn new(
        renderer: Arc<Renderer>,
        scene: Arc<Scene>,
        render: FinalRender,
        source: PhotoSource,
    ) -> Arc<Self> {
        Arc::new(Self {
            renderer,
            scene,
            render: ThreadBound::new(render),
            source,
        })
    }
}

/// Which engine traces a final render.
pub(crate) enum PhotoSource {
    /// The renderer's own
    Renderer,
    /// The engine of a raytraced preview, whose image the photo goes on with
    Preview(Arc<PreviewJob>),
    /// The raytraced preview of a realtime view
    View(Arc<crate::view::SceneView>),
}

#[uniffi::export]
impl FinalJob {
    /// Traces for about the step time; true when the image is complete.
    pub fn step(&self) -> Result<bool, RtFfiError> {
        let on_renderer = |render: &mut FinalRender| -> Result<bool, RtFfiError> {
            let done = self.renderer.state.with(|s| {
                self.scene.with_rt(|scene| match &mut s.engine {
                    Some(engine) => engine.trace_final(scene, render),
                    None => render.trace_cpu(scene, s.cpu_threads),
                })
            })?;
            Ok(done?)
        };
        self.render.with(|render| match &self.source {
            PhotoSource::Renderer => on_renderer(render),
            PhotoSource::Preview(job) => {
                let done = job.state.with(|s| {
                    let done = self
                        .scene
                        .with_rt(|scene| s.preview.preview.trace_final(scene, render));
                    s.photo = matches!(done, Ok(false));
                    done
                })?;
                Ok(done?)
            }
            PhotoSource::View(view) => {
                match view.trace_photo(&self.scene, render)? {
                    Some(done) => Ok(done),
                    // The view's preview is gone (a new look): the renderer goes on
                    None => on_renderer(render),
                }
            }
        })?
    }

    /// Whether the photo went on with the samples of a preview, see [PreviewJob::photo] and [SceneView::photo].
    pub fn continues_preview(&self) -> bool {
        self.render.with(|r| r.continues_preview()).unwrap_or(false)
    }

    /// From 0 to 1.
    pub fn progress(&self) -> f32 {
        self.render.with(|r| r.progress()).unwrap_or(0.0)
    }

    /// Milliseconds since the first step.
    pub fn elapsed_ms(&self) -> u64 {
        self.render
            .with(|r| r.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    /// An estimate of the milliseconds left, `None` before there is one.
    pub fn remaining_ms(&self) -> Option<u64> {
        self.render
            .with(|r| r.remaining().map(|d| d.as_millis() as u64))
            .ok()
            .flatten()
    }

    /// Whether the image is complete.
    pub fn is_done(&self) -> bool {
        self.render.with(|r| r.is_done()).unwrap_or(false)
    }

    /// Width and height in pixels.
    pub fn size(&self) -> Vec<u32> {
        self.render
            .with(|r| vec![r.width(), r.height()])
            .unwrap_or_default()
    }

    /// The complete image as RGBA8, rows top first, when it is done; close it in Kotlin when it is used.
    pub fn rgba(&self) -> Result<Option<Arc<OwnedBytes>>, RtFfiError> {
        self.render
            .with(|r| r.rgba8().map(|pixels| OwnedBytes::new(pixels.to_vec())))
    }

    /// The complete image as PNG file when it is done; close it in Kotlin when it is used.
    pub fn png(&self) -> Result<Option<Arc<OwnedBytes>>, RtFfiError> {
        self.render.with(|r| r.png().map(OwnedBytes::new))
    }
}

/// A preview frame: RGBA8, rows top first, in sRGB.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Frame {
    /// The pixels, valid for the next two frames (a ring of three).
    pub rgba: NativeBytes,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Samples per pixel so far, 0 for a raster frame.
    pub samples: u32,
}

/// A raytraced preview, see [Renderer::preview].
#[derive(uniffi::Object)]
pub struct PreviewJob {
    pub(crate) renderer: Arc<Renderer>,
    scene: Arc<Scene>,
    pub(crate) state: ThreadBound<PreviewJobState>,
}

pub(crate) struct PreviewJobState {
    preview: PreviewState,
    camera: CameraSpec,
    /// A photo traces on this preview's engine: the preview waits
    photo: bool,
}

/// A raytrace preview with the texture it is drawn into in sRGB and a ring of three buffers to read it into.
pub(crate) struct PreviewState {
    pub preview: RaytracePreview,
    context: Context,
    target: Texture2D,
    readback: Readback,
    pub width: u32,
    pub height: u32,
}

impl PreviewState {
    pub fn new(
        context: &Context,
        width: u32,
        height: u32,
        look: Look,
        threads: usize,
        options: PreviewOptions,
    ) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let mut preview = match options.tracer {
            TracerChoice::Cpu => RaytracePreview::new_on_cpu(context, width, height),
            TracerChoice::Auto | TracerChoice::Gpu => RaytracePreview::new(context, width, height),
        };
        let settings = options.apply(
            PreviewSettings {
                tone_mapping: look.tone_mapping(),
                max_bounces: look.max_bounces,
                gpu_caustic_photons: look.caustic_photons,
                cpu_caustic_photons: look.caustic_photons / 10,
                cpu_threads: threads,
                ..*preview.settings()
            },
            threads,
        );
        preview.set_settings(settings);
        Self {
            preview,
            context: context.clone(),
            target: rgba_texture(context, width, height),
            readback: Readback::new(),
            width,
            height,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) != (self.width, self.height) {
            self.preview.resize(width, height);
            self.target = rgba_texture(&self.context, width, height);
            (self.width, self.height) = (width, height);
        }
    }

    /// Traces for one frame time; the frame if the image changed.
    pub fn update(
        &mut self,
        scene: &Scene,
        camera: &CameraSpec,
    ) -> Result<Option<Frame>, RtFfiError> {
        let camera = camera.camera(self.width, self.height);
        let changed = scene.with_rt(|s| self.preview.update(s, &camera))?;
        Ok(changed.then(|| self.read()))
    }

    /// The current image as a frame.
    pub fn read(&mut self) -> Frame {
        self.read_with(None)
            .expect("without an overlay nothing can fail")
    }

    /// [PreviewState::read] with egui drawn over the preview's image, as the view shows it.
    pub fn read_with(
        &mut self,
        overlay: Option<&three_d_egui_uniffi::EguiOverlay>,
    ) -> Result<Frame, RtFfiError> {
        self.preview
            .render_to_color(&self.target.as_color_target(None));
        if let Some(overlay) = overlay {
            let viewport = three_d::Viewport::new_at_origo(self.width, self.height);
            overlay.paint_color(&self.context, &self.target.as_color_target(None), viewport)?;
        }
        let bytes = self
            .readback
            .read(&self.target.as_color_target(None), self.width, self.height);
        Ok(Frame {
            rgba: NativeBytes::of(bytes),
            width: self.width,
            height: self.height,
            samples: self.preview.sample_count(),
        })
    }
}

#[uniffi::export]
impl PreviewJob {
    /// A new camera; the preview starts again.
    pub fn set_camera(&self, camera: CameraSpec) -> Result<(), RtFfiError> {
        self.state.with(|s| s.camera = camera)
    }

    /// A new size in pixels.
    pub fn resize(&self, width: u32, height: u32) -> Result<(), RtFfiError> {
        self.state.with(|s| s.preview.resize(width, height))
    }

    /// Traces for one frame time and returns the frame if it changed, `None` when it is converged or waits.
    pub fn next_frame(&self) -> Result<Option<Frame>, RtFfiError> {
        let _ = &self.renderer;
        self.state.with(|s| {
            if s.photo {
                // The photo traces on this preview's engine: the image stays as it is
                return Ok(None);
            }
            let camera = s.camera;
            s.preview.update(&self.scene, &camera)
        })?
    }

    /// Everything a display of the preview needs, from the moment after the last [PreviewJob::next_frame].
    pub fn status(&self) -> Result<PreviewStatus, RtFfiError> {
        self.state.with(|s| PreviewStatus::of(&s.preview.preview))
    }

    ///
    /// A photo of the preview in its size that goes on with the samples it has, up to `samples` per pixel: no
    /// shaders to wait for, no samples traced twice. `None` where there is nothing to go on with (the preview traces on
    /// the CPU, at a lower resolution, or has no image yet): then [Renderer::final_render] with the same camera and
    /// size. Step it every frame instead of [PreviewJob::next_frame]; the preview keeps its image meanwhile.
    ///
    pub fn photo(
        self: Arc<Self>,
        samples: u32,
        step_ms: u32,
    ) -> Result<Option<Arc<FinalJob>>, RtFfiError> {
        let render = self
            .state
            .with(|s| photo_of(&s.preview.preview, samples, step_ms))?;
        Ok(render.map(|render| {
            FinalJob::new(
                self.renderer.clone(),
                self.scene.clone(),
                render,
                PhotoSource::Preview(self.clone()),
            )
        }))
    }

    /// GPU or CPU and why, for a tooltip.
    pub fn availability(&self) -> Result<String, RtFfiError> {
        self.state.with(|s| {
            format!(
                "{:?}: {:?}",
                s.preview.preview.kind(),
                s.preview.preview.availability()
            )
        })
    }
}

/// A ring of byte buffers that keep their place between frames: the bytes of a frame stay valid for the next
/// `count - 1` frames (a UI thread showing one while the next are written).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct FrameBuffers {
    buffers: Vec<Vec<u8>>,
    next: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl FrameBuffers {
    pub fn new(count: usize) -> Self {
        Self {
            buffers: vec![Vec::new(); count.max(1)],
            next: 0,
        }
    }

    pub fn next(&mut self, length: usize) -> &mut Vec<u8> {
        let i = self.next;
        self.next = (i + 1) % self.buffers.len();
        let b = &mut self.buffers[i];
        // Only a new size moves the bytes
        if b.len() != length {
            *b = vec![0; length];
        }
        b
    }
}

/// The pixels of a color target for Kotlin, valid for the next two frames.
///
/// Native: read right away into a ring of three buffers. In the browser nothing can wait for the GPU: each call starts a
/// read without waiting and gives the last finished one, so the frame shows what was drawn one or two calls before (on
/// WebGPU the pixels arrive between two animation frames). Before the first read finished the frame is transparent.
pub(crate) struct Readback {
    #[cfg(not(target_arch = "wasm32"))]
    ring: FrameBuffers,
    #[cfg(target_arch = "wasm32")]
    web: web::WebReadback,
}

impl Readback {
    pub fn new() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            ring: FrameBuffers::new(3),
            #[cfg(target_arch = "wasm32")]
            web: web::WebReadback::default(),
        }
    }

    pub fn read(&mut self, target: &three_d::ColorTarget<'_>, width: u32, height: u32) -> &[u8] {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes = self.ring.next(width as usize * height as usize * 4);
            target.read_into(as_texels(bytes));
            &bytes[..]
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.web.read(target, width, height)
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    type Pixels = Vec<[u8; 4]>;
    type PendingRead = Pin<Box<dyn Future<Output = Pixels>>>;

    #[derive(Default)]
    pub struct WebReadback {
        pending: Option<(PendingRead, usize)>,
        /// Finished reads, the newest last; three are kept so that the bytes of a frame stay valid for two more calls
        shown: VecDeque<Pixels>,
        free: Vec<Pixels>,
        blank: Vec<u8>,
    }

    impl WebReadback {
        pub fn read(
            &mut self,
            target: &three_d::ColorTarget<'_>,
            width: u32,
            height: u32,
        ) -> &[u8] {
            let count = width as usize * height as usize;
            // The read of an earlier call: the browser has mapped its buffer by now, or not yet
            if let Some((future, len)) = &mut self.pending {
                let mut context = Context::from_waker(Waker::noop());
                if let Poll::Ready(pixels) = future.as_mut().poll(&mut context) {
                    if *len == count {
                        self.shown.push_back(pixels);
                        while self.shown.len() > 3 {
                            self.free.extend(self.shown.pop_front());
                        }
                    } else {
                        self.free.push(pixels);
                    }
                    self.pending = None;
                }
            }
            // One read at a time; the next starts as soon as the last one is in
            if self.pending.is_none() {
                let buffer = self.free.pop().unwrap_or_default();
                self.pending = Some((Box::pin(target.read_into_async(buffer)), count));
            }
            match self.shown.back() {
                Some(pixels) if pixels.len() == count => pixels.as_flattened(),
                _ => {
                    // Nothing of this size yet (the first frames, or right after a resize): transparent
                    self.blank.resize(count * 4, 0);
                    &self.blank
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn as_texels(bytes: &mut [u8]) -> &mut [[u8; 4]] {
    let n = bytes.len() / 4;
    // SAFETY: [u8; 4] has the alignment of u8 and the bytes are exactly n * 4 long
    unsafe { std::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut [u8; 4], n) }
}

pub(crate) fn rgba_texture(context: &Context, width: u32, height: u32) -> Texture2D {
    Texture2D::new_empty::<[u8; 4]>(
        context,
        width,
        height,
        three_d::Interpolation::Nearest,
        three_d::Interpolation::Nearest,
        None,
        three_d::Wrapping::ClampToEdge,
        three_d::Wrapping::ClampToEdge,
    )
}

/// A photo that goes on with the samples of a preview, `None` where it cannot.
pub(crate) fn photo_of(
    preview: &RaytracePreview,
    samples: u32,
    step_ms: u32,
) -> Option<FinalRender> {
    #[cfg(feature = "gpu")]
    {
        FinalRender::from_preview(preview, samples.max(1))
            .map(|r| r.with_frame_time(Duration::from_millis(step_ms.max(1) as u64)))
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = (preview, samples, step_ms);
        None
    }
}

/// Which tracer a raytraced preview of a [SceneView] uses, see [SceneView::set_preview_options].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, uniffi::Enum)]
pub enum TracerChoice {
    /// The GPU where it can and is fast enough, else the CPU (the default)
    #[default]
    Auto,
    /// The GPU only, also when it is slow: at most a quarter of the resolution, never the CPU. While the shaders
    /// compile the CPU traces (`PreviewStatus.compiling`). Only where `PreviewCapabilities.gpu` is true.
    Gpu,
    /// The CPU only
    Cpu,
}

///
/// What a raytraced preview of a [SceneView] does, set by hand for tests, benchmarks and demonstrations; the default
/// decides everything by itself. See [SceneView::set_preview_options].
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct PreviewOptions {
    /// Which tracer
    pub tracer: TracerChoice,
    ///
    /// Width and height on the GPU divided by this, fixed: 1, 2 or 4. `None` starts at the full resolution and lowers
    /// it when the GPU needs more than 40 ms per frame (then, for `Auto`, on to the CPU).
    ///
    pub gpu_downscale: Option<u32>,
    /// Width and height on the CPU divided by this, 1 to 16; `None` for 4 (8 in browsers).
    pub cpu_downscale: Option<u32>,
    /// Smooths the noise of the first samples (on by default)
    pub denoise: bool,
    ///
    /// Threads the CPU traces with, 1 to `PreviewCapabilities.cpu_cores`; changes the running preview. `None` for the
    /// renderer's `cpu_threads` (one less than the machine has, at least one). Browsers trace in one thread.
    ///
    pub cpu_threads: Option<u32>,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self::AUTOMATIC
    }
}

impl PreviewOptions {
    /// The defaults: everything automatic, with the denoiser.
    pub const AUTOMATIC: Self = Self {
        tracer: TracerChoice::Auto,
        gpu_downscale: None,
        cpu_downscale: None,
        denoise: true,
        cpu_threads: None,
    };

    /// Downscales that are not 1, 2 or 4 on the GPU or not 1 to 16 on the CPU.
    pub fn check(&self) -> Result<(), RtFfiError> {
        if let Some(n) = self.cpu_threads {
            let cores = cpu_cores();
            if !(1..=cores).contains(&n) {
                return Err(invalid(format!(
                    "cpu_threads {n}: 1 to {cores}, or None for automatic"
                )));
            }
        }
        if let Some(n) = self.gpu_downscale {
            if !GPU_DOWNSCALES.contains(&n) {
                return Err(invalid(format!(
                    "gpu_downscale {n}: 1, 2 or 4, or None for automatic"
                )));
            }
        }
        if let Some(n) = self.cpu_downscale {
            if !(1..=MAX_CPU_DOWNSCALE).contains(&n) {
                return Err(invalid(format!(
                    "cpu_downscale {n}: 1 to {MAX_CPU_DOWNSCALE}, or None for automatic"
                )));
            }
        }
        Ok(())
    }

    /// The settings with these options in place of what they decide.
    /// `threads` is what the CPU traces with for `cpu_threads: None`.
    pub fn apply(&self, settings: PreviewSettings, threads: usize) -> PreviewSettings {
        let default = PreviewSettings::default();
        let (gpu_downscale, gpu_frame_time) = match self.gpu_downscale {
            Some(n) => (n, None),
            None => (default.gpu_downscale, default.gpu_frame_time),
        };
        PreviewSettings {
            gpu_downscale,
            gpu_frame_time,
            gpu_falls_back_to_cpu: self.tracer != TracerChoice::Gpu,
            cpu_downscale: self.cpu_downscale.unwrap_or(default.cpu_downscale),
            cpu_threads: self.cpu_threads.map_or(threads, |n| n as usize),
            denoiser: if self.denoise {
                settings.denoiser.or(default.denoiser)
            } else {
                None
            },
            ..settings
        }
    }
}

/// Logical cores of the machine, 1 where it does not say (browsers).
fn cpu_cores() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

/// The fixed divisors of width and height on the GPU, see [PreviewOptions::gpu_downscale].
const GPU_DOWNSCALES: [u32; 3] = [1, 2, 4];
/// The largest divisor of width and height on the CPU, see [PreviewOptions::cpu_downscale].
const MAX_CPU_DOWNSCALE: u32 = 16;

///
/// What a raytraced preview can do on the device of a [SceneView], to show options that cannot work as disabled,
/// with the reason as a tooltip. See [SceneView::preview_capabilities].
///
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PreviewCapabilities {
    /// The GPU can trace: [TracerChoice::Gpu] and [PreviewOptions::gpu_downscale] work
    pub gpu: bool,
    /// Why not, where `gpu` is false: no compute shaders (OpenGL, WebGL2), or a driver known to crash
    pub gpu_reason: Option<String>,
    /// The GPU traces with hardware ray queries ([TracerKind::RayQuery]), not with hierarchies in storage buffers
    pub ray_query: bool,
    /// The GPU measures its own time per frame (timestamp queries): `PreviewStatus.gpu_time_ms` and the sharp image
    /// at the full resolution when still. Without, the time between two frames counts.
    pub timestamp_queries: bool,
    /// The values [PreviewOptions::gpu_downscale] takes: 1, 2 and 4
    pub gpu_downscales: Vec<u32>,
    /// The largest [PreviewOptions::cpu_downscale]
    pub max_cpu_downscale: u32,
    /// Logical cores of the machine, the most [PreviewOptions::cpu_threads]; 1 in browsers
    pub cpu_cores: u32,
}

impl PreviewCapabilities {
    pub(crate) fn of(context: &Context) -> Self {
        let gpu_reason = match three_d_raytrace::PreviewAvailability::of(context) {
            three_d_raytrace::PreviewAvailability::Gpu => None,
            three_d_raytrace::PreviewAvailability::CpuOnly(why) => Some(why),
        };
        #[cfg(feature = "gpu")]
        let (ray_query, timestamp_queries) = (
            gpu_reason.is_none()
                && three_d_raytrace::RaytraceSupport::of(context)
                    == three_d_raytrace::RaytraceSupport::HardwareRayQuery,
            context.as_wgpu().is_some_and(|(device, _)| {
                device
                    .features()
                    .contains(three_d::wgpu::Features::TIMESTAMP_QUERY)
            }),
        );
        #[cfg(not(feature = "gpu"))]
        let (ray_query, timestamp_queries) = (false, false);
        Self {
            gpu: gpu_reason.is_none(),
            gpu_reason,
            ray_query,
            timestamp_queries,
            gpu_downscales: GPU_DOWNSCALES.to_vec(),
            max_cpu_downscale: MAX_CPU_DOWNSCALE,
            cpu_cores: cpu_cores(),
        }
    }
}

/// What traces a raytraced preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TracerKind {
    /// On the CPU
    Cpu,
    /// In compute shaders on the GPU
    Compute,
    /// With the ray queries of a GPU that traces rays
    RayQuery,
}

/// Why a raytraced preview started its image again; every restart forgets the samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PreviewRestart {
    /// The first image
    First,
    /// The camera moved or turned
    Camera,
    /// The view has another size
    Viewport,
    /// The scene changed
    Scene,
    /// The preview was resized
    Resize,
    /// Settings that change the image
    Settings,
    /// Another engine: the GPU once its shaders compiled, a lower GPU resolution, or the CPU
    Engine,
}

///
/// Everything a display of a raytraced preview needs, from the same moment, so that Kotlin counts nothing itself.
///
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PreviewStatus {
    /// What traces
    pub tracer: TracerKind,
    /// Why only the CPU can trace, `None` where the GPU can
    pub cpu_reason: Option<String>,
    /// Whether the GPU's shaders still compile (the CPU traces meanwhile)
    pub compiling: bool,
    /// Samples every pixel of the image shown has
    pub samples: u32,
    /// Samples the image ends with
    pub max_samples: u32,
    /// Whether the image is done and does not change any more
    pub done: bool,
    /// Size of the image shown, in pixels (smaller than the view while the GPU or CPU traces at a lower resolution)
    pub width: u32,
    pub height: u32,
    /// Size of the preview on screen
    pub full_width: u32,
    pub full_height: u32,
    /// The GPU's time of the newest measured update, in milliseconds
    pub gpu_time_ms: Option<f32>,
    /// How many times the image started
    pub restarts: u64,
    /// Why it started the last time
    pub last_restart: Option<PreviewRestart>,
    /// Milliseconds since the image started, up to when it was done
    pub elapsed_ms: u64,
    /// Milliseconds still needed for `max_samples`, `None` before the first sample
    pub remaining_ms: Option<u64>,
}

impl PreviewStatus {
    pub(crate) fn of(preview: &RaytracePreview) -> Self {
        let s = preview.status();
        Self {
            tracer: match s.kind {
                three_d_raytrace::TracerKind::CpuReference => TracerKind::Cpu,
                three_d_raytrace::TracerKind::Compute => TracerKind::Compute,
                three_d_raytrace::TracerKind::RayQuery => TracerKind::RayQuery,
            },
            cpu_reason: match s.availability {
                three_d_raytrace::PreviewAvailability::Gpu => None,
                three_d_raytrace::PreviewAvailability::CpuOnly(why) => Some(why),
            },
            compiling: s.compiling,
            samples: s.samples,
            max_samples: s.max_samples,
            done: s.done,
            width: s.width,
            height: s.height,
            full_width: s.full_width,
            full_height: s.full_height,
            gpu_time_ms: s.gpu_time.map(|d| d.as_secs_f32() * 1000.0),
            restarts: s.restarts,
            last_restart: s.last_restart.map(|r| match r {
                three_d_raytrace::PreviewRestart::First => PreviewRestart::First,
                three_d_raytrace::PreviewRestart::Camera => PreviewRestart::Camera,
                three_d_raytrace::PreviewRestart::Viewport => PreviewRestart::Viewport,
                three_d_raytrace::PreviewRestart::Scene => PreviewRestart::Scene,
                three_d_raytrace::PreviewRestart::Resize => PreviewRestart::Resize,
                three_d_raytrace::PreviewRestart::Settings => PreviewRestart::Settings,
                three_d_raytrace::PreviewRestart::Engine => PreviewRestart::Engine,
            }),
            elapsed_ms: s.elapsed.as_millis() as u64,
            remaining_ms: s.remaining.map(|d| d.as_millis() as u64),
        }
    }
}
