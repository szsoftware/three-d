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
            ))
        })??;
        Ok(Arc::new(PreviewJob {
            renderer: self,
            scene,
            state: ThreadBound::new(PreviewJobState {
                preview: state,
                camera,
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
}

#[uniffi::export]
impl FinalJob {
    /// Traces for about the step time; true when the image is complete.
    pub fn step(&self) -> Result<bool, RtFfiError> {
        let done = self.render.with(|render| {
            self.renderer.state.with(|s| {
                self.scene.with_rt(|scene| match &mut s.engine {
                    Some(engine) => engine.trace_final(scene, render),
                    None => render.trace_cpu(scene, s.cpu_threads),
                })
            })
        })??;
        Ok(done?)
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
    renderer: Arc<Renderer>,
    scene: Arc<Scene>,
    state: ThreadBound<PreviewJobState>,
}

struct PreviewJobState {
    preview: PreviewState,
    camera: CameraSpec,
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
    pub fn new(context: &Context, width: u32, height: u32, look: Look, threads: usize) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let mut preview = RaytracePreview::new(context, width, height);
        preview.set_settings(PreviewSettings {
            tone_mapping: look.tone_mapping(),
            max_bounces: look.max_bounces,
            gpu_caustic_photons: look.caustic_photons,
            cpu_caustic_photons: look.caustic_photons / 10,
            cpu_threads: threads,
            ..*preview.settings()
        });
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
        self.preview
            .render_to_color(&self.target.as_color_target(None));
        let bytes = self
            .readback
            .read(&self.target.as_color_target(None), self.width, self.height);
        Frame {
            rgba: NativeBytes::of(bytes),
            width: self.width,
            height: self.height,
            samples: self.preview.sample_count(),
        }
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
            let camera = s.camera;
            s.preview.update(&self.scene, &camera)
        })?
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
