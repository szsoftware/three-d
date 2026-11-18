//!
//! A progressive preview in a three-d application: traced a few samples per frame, finer while camera and scene stand
//! still, on the GPU where the context can and on the CPU otherwise.
//!

use crate::{
    Camera, CpuReferenceTracer, Denoiser, RenderImage, RtScene, ToneMapping, ToneOperator,
    TraceSettings, Tracer, TracerKind,
};
use three_d::{
    ColorTarget, Context, DepthTest, Effect, EffectMaterialId, Interpolation, Light, Program,
    RenderStates, RenderTarget, Texture2D, Viewer, Wrapping,
};

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
    /// The CPU traces at width and height divided by this.
    pub cpu_downscale: u32,
    ///
    /// The CPU traces on a thread of its own, one sample after the other, and [RaytracePreview::update] shows the
    /// newest image, so the frames do not wait for the tracer. Otherwise every update traces one sample and waits.
    /// Browsers have no threads, there the update always traces.
    ///
    pub cpu_in_background: bool,
    /// No more samples are added after this many, the image stays as it is.
    pub max_samples: u32,
    /// Most bounces of a path.
    pub max_bounces: u32,
    ///
    /// Photons per sample for caustics of solid glass on the CPU, see [TraceSettings::caustic_photons]. They cost
    /// time only in scenes with such glass; 0 lets light pass the glass straight. The GPU has no caustics yet.
    ///
    pub cpu_caustic_photons: u32,
    /// Exposure and curve of [RaytracePreview::render].
    pub tone_mapping: ToneMapping,
    /// Smooths the noise of the first samples with this filter, `None` to show the samples as they are.
    pub denoiser: Option<crate::AtrousDenoiser>,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self {
            gpu_samples_per_frame: 1,
            cpu_downscale: 4,
            cpu_in_background: true,
            max_samples: 1024,
            max_bounces: 6,
            cpu_caustic_photons: 20_000,
            tone_mapping: ToneMapping::default(),
            denoiser: Some(crate::AtrousDenoiser::default()),
        }
    }
}

enum Engine {
    #[cfg(feature = "wgpu")]
    Gpu {
        tracer: Box<crate::ComputeTracer>,
        image: crate::GpuRenderImage,
    },
    Cpu {
        tracer: CpuReferenceTracer,
        image: RenderImage,
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
    traced_for: Option<(Vec<f32>, u64, u64)>,
    settings: PreviewSettings,
}

impl RaytracePreview {
    /// A preview of the given size, on the GPU where the context can, see [PreviewAvailability::of].
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
        let (engine, texture) = Self::engine(context, &availability, width, height, &settings);
        Self {
            context: context.clone(),
            availability,
            engine,
            texture,
            width,
            height,
            traced_for: None,
            settings,
        }
    }

    fn engine(
        context: &Context,
        #[allow(unused_variables)] availability: &PreviewAvailability,
        width: u32,
        height: u32,
        settings: &PreviewSettings,
    ) -> (Engine, Texture2D) {
        #[cfg(feature = "wgpu")]
        if *availability == PreviewAvailability::Gpu {
            if let Some(tracer) = crate::ComputeTracer::from_context(context) {
                let image =
                    crate::GpuRenderImage::with_features(tracer.gpu(), width.max(1), height.max(1));
                let texture = empty_texture(context, width.max(1), height.max(1));
                return (
                    Engine::Gpu {
                        tracer: Box::new(tracer),
                        image,
                    },
                    texture,
                );
            }
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
                tracer: CpuReferenceTracer::new(),
                image: RenderImage::with_features(w, h),
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

    /// Changes the settings. A new CPU resolution starts the preview again.
    pub fn set_settings(&mut self, settings: PreviewSettings) {
        let rebuild = (settings.cpu_downscale != self.settings.cpu_downscale
            || settings.cpu_in_background != self.settings.cpu_in_background)
            && self.on_cpu();
        let restart = settings.max_bounces != self.settings.max_bounces
            || settings.cpu_caustic_photons != self.settings.cpu_caustic_photons;
        self.settings = settings;
        if rebuild {
            self.resize(self.width, self.height);
        } else if restart {
            self.restart();
        }
    }

    /// Changes the size and starts again.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (engine, texture) = Self::engine(
            &self.context,
            &self.availability,
            width,
            height,
            &self.settings,
        );
        self.engine = engine;
        self.texture = texture;
        self.width = width;
        self.height = height;
        self.traced_for = None;
    }

    /// Forgets all samples.
    pub fn restart(&mut self) {
        self.traced_for = None;
    }

    /// Samples every pixel has.
    pub fn sample_count(&self) -> u32 {
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
        self.traced_for.is_some() && self.sample_count() >= self.settings.max_samples
    }

    ///
    /// Adds samples for the scene seen by the camera, after starting again if either changed, and writes the average
    /// into [RaytracePreview::texture]. Returns whether the image changed. With [PreviewSettings::cpu_in_background]
    /// the samples are added on another thread and this shows the newest image; the scene is copied for that thread
    /// whenever it changed.
    ///
    pub fn update(&mut self, scene: &RtScene, camera: &Camera) -> Result<bool, crate::RtError> {
        let key = (camera_key(camera), scene.id(), scene.revision());
        let changed = self.traced_for.as_ref() != Some(&key);
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
                    max_samples: self.settings.max_samples,
                    denoiser: self.settings.denoiser,
                });
                *shown = 0;
                self.traced_for = Some(key);
            } else {
                worker.adjust(self.settings.max_samples, self.settings.denoiser);
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
        if changed {
            match &mut self.engine {
                #[cfg(feature = "wgpu")]
                Engine::Gpu { image, .. } => image.reset(),
                Engine::Cpu { image, .. } => image.reset(),
                #[cfg(not(target_arch = "wasm32"))]
                Engine::CpuWorker { .. } => unreachable!("handled above"),
            }
            self.traced_for = Some(key);
        } else if self.sample_count() >= self.settings.max_samples {
            return Ok(false);
        }
        let settings = TraceSettings {
            max_bounces: self.settings.max_bounces,
            caustic_photons: self.settings.cpu_caustic_photons,
            ..Default::default()
        };
        match &mut self.engine {
            #[cfg(feature = "wgpu")]
            Engine::Gpu { tracer, image } => {
                let settings = TraceSettings {
                    samples_per_call: self.settings.gpu_samples_per_frame.max(1),
                    ..settings
                };
                tracer.trace_gpu(scene, camera, image, &settings)?;
                match &self.settings.denoiser {
                    Some(denoiser) => {
                        image.write_denoised_to_three_d_texture(&self.texture, denoiser)
                    }
                    None => image.write_to_three_d_texture(&self.texture),
                }
            }
            Engine::Cpu { tracer, image } => {
                // The camera of the reduced resolution
                let mut small = camera.clone();
                small.set_viewport(three_d_asset::Viewport::new_at_origo(
                    image.width(),
                    image.height(),
                ));
                tracer.trace(scene, &small, image, &settings)?;
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
    /// The average luminance of every pixel in cd/m² (linear, 32 bit floats), upright as three-d expects. On the CPU it
    /// is smaller than the preview by [PreviewSettings::cpu_downscale].
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
