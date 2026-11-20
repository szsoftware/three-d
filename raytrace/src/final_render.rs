//!
//! An image larger than the screen, for print or export, traced in bands beside the frames of an application: each
//! update traces only as much as fits into a frame, and the bands are denoised and tone mapped one after the other.
//!

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::{
    AtrousDenoiser, Camera, CpuReferenceTracer, Denoiser, RenderImage, RtError, RtScene,
    ToneMapping, TraceSettings, ViewRegion,
};
use std::time::Duration;

/// Width and height of A4 in landscape at 300 dots per inch.
pub const A4_LANDSCAPE_300_DPI: (u32, u32) = (3508, 2480);
/// Width and height of A3 in landscape at 300 dots per inch.
pub const A3_LANDSCAPE_300_DPI: (u32, u32) = (4961, 3508);

/// Most pixels of a band: its sums with the features for the denoiser (48 bytes a pixel) stay below the 128 MB a
/// storage buffer may have everywhere, browsers included.
const BAND_PIXELS: u32 = 2_000_000;

///
/// An image of a scene for print or export, larger than the screen: for example A4 at 300 dpi
/// ([A4_LANDSCAPE_300_DPI]). It is traced in bands of rows, a sample of a band spread over several updates on the GPU
/// so that each stays within [FinalRender::with_frame_time], then denoised and tone mapped band by band. The result is
/// 8 bit sRGB, rows from the top ([FinalRender::rgba8], [FinalRender::png]).
///
/// Call [RaytracePreview::trace_final](crate::RaytracePreview::trace_final) every frame instead of
/// [RaytracePreview::update](crate::RaytracePreview::update) until it returns `true`; it uses the preview's GPU, or
/// the CPU where there is none. [RaytracePreview::render](crate::RaytracePreview::render) shows the last image of the
/// preview meanwhile. [FinalRender::png] needs the feature `export` (a default feature).
///
/// Outside of browsers a final render is [Send] and [Sync], so it can live in an object shared between threads. With
/// [FinalRender::trace_cpu] it can also be traced on a thread of its own, with a clone of the scene; the GPU path
/// stays on the thread of the context.
///
/// ```ignore
/// let (width, height) = A4_LANDSCAPE_300_DPI;
/// let mut print = FinalRender::new(&camera, width, height, 256)
///     .with_settings(demo.settings)
///     .with_tone_mapping(demo.tone_mapping);
/// // every frame, until done
/// if preview.trace_final(&scene, &mut print)? {
///     let png = print.png().unwrap();
/// }
/// ```
///
pub struct FinalRender {
    camera: Camera,
    width: u32,
    height: u32,
    samples: u32,
    settings: TraceSettings,
    tone_mapping: ToneMapping,
    denoiser: Option<AtrousDenoiser>,
    frame_time: Duration,
    band_pixels: u32,
    /// Index of the band in progress, the number of bands when done
    band: u32,
    work: Option<Work>,
    pixels: Vec<[u8; 4]>,
    /// Pixel samples traced in the bands before the one in progress, and in all bands
    traced: u64,
    total: u64,
    started: Option<Instant>,
    cpu: Option<CpuReferenceTracer>,
}

/// The band in progress.
enum Work {
    #[cfg(feature = "wgpu")]
    Gpu {
        image: crate::GpuRenderImage,
        /// The GPU's milliseconds per tile
        tile_ms: Option<f64>,
    },
    /// The band's colors on their way back from the GPU
    #[cfg(feature = "wgpu")]
    Reading(Reading),
    Cpu {
        image: RenderImage,
        /// The row the next call starts at in the sample in progress
        next_row: u32,
        rows_per_call: u32,
    },
}

///
/// The colors of a band on their way back from the GPU, read with wgpu itself rather than three-d's futures, so that
/// [FinalRender] is [Send] and [Sync] outside of browsers.
///
#[cfg(feature = "wgpu")]
struct Reading {
    /// To poll outside of browsers
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    device: three_d::wgpu::Device,
    buffer: three_d::wgpu::Buffer,
    /// Whether the buffer is mapped, once wgpu calls back
    mapped: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
    width: u32,
    rows: u32,
    bytes_per_row: u32,
}

#[cfg(feature = "wgpu")]
impl Reading {
    /// Copies the texture into a buffer and asks for it to be mapped.
    fn start(gpu: &crate::GpuDevice, texture: &three_d::wgpu::Texture) -> Self {
        use three_d::wgpu as w;
        let (width, rows) = (texture.width(), texture.height());
        let bytes_per_row = (width * 4).next_multiple_of(w::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = gpu.device.create_buffer(&w::BufferDescriptor {
            label: Some("read final render band"),
            size: bytes_per_row as u64 * rows as u64,
            usage: w::BufferUsages::COPY_DST | w::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&w::CommandEncoderDescriptor {
                label: Some("read final render band"),
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            w::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: w::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(rows),
                },
            },
            w::Extent3d {
                width,
                height: rows,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let mapped = std::sync::Arc::new(std::sync::Mutex::new(None));
        let shared = mapped.clone();
        buffer.slice(..).map_async(w::MapMode::Read, move |result| {
            *shared.lock().unwrap_or_else(|e| e.into_inner()) = Some(result.is_ok());
        });
        Self {
            device: gpu.device.clone(),
            buffer,
            mapped,
            width,
            rows,
            bytes_per_row,
        }
    }

    /// The colors, top row first, once the GPU is done; browsers map without being asked.
    fn poll(&self) -> Result<Option<Vec<[u8; 4]>>, RtError> {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.device.poll(three_d::wgpu::PollType::Poll);
        let mapped = *self.mapped.lock().unwrap_or_else(|e| e.into_inner());
        match mapped {
            None => Ok(None),
            Some(false) => Err(RtError::Gpu(
                "the final render could not be read from the GPU".to_owned(),
            )),
            Some(true) => {
                let mut colors = Vec::with_capacity((self.width * self.rows) as usize);
                {
                    let bytes = self.buffer.slice(..).get_mapped_range();
                    // three-d draws into textures with v = 0 at wgpu's first row: the top is the last row
                    for row in bytes.chunks(self.bytes_per_row as usize).rev() {
                        let row = &row[..(self.width * 4) as usize];
                        colors.extend(row.as_chunks::<4>().0.iter().copied());
                    }
                }
                self.buffer.unmap();
                Ok(Some(colors))
            }
        }
    }
}

/// Rows of a band: those traced, and those of them kept (the others give the denoiser what lies beyond the band).
struct Band {
    traced: std::ops::Range<u32>,
    kept: std::ops::Range<u32>,
}

impl FinalRender {
    ///
    /// An image of this size and number of samples per pixel, seen by the camera; the camera's viewport becomes the
    /// image's size, so its view widens or narrows to the image's aspect.
    ///
    pub fn new(camera: &Camera, width: u32, height: u32, samples: u32) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let mut camera = camera.clone();
        camera.set_viewport(three_d_asset::Viewport::new_at_origo(width, height));
        let mut render = Self {
            camera,
            width,
            height,
            samples: samples.max(1),
            settings: TraceSettings::default(),
            tone_mapping: ToneMapping::default(),
            denoiser: Some(AtrousDenoiser::default()),
            frame_time: Duration::from_millis(40),
            band_pixels: BAND_PIXELS,
            band: 0,
            work: None,
            pixels: Vec::new(),
            traced: 0,
            total: 0,
            started: None,
            cpu: None,
        };
        render.count();
        render
    }

    /// Bounces, caustic photons, depth of field and seed; samples per call and view region are the renderer's.
    pub fn with_settings(mut self, settings: TraceSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Exposure and curve of the 8 bit colors.
    pub fn with_tone_mapping(mut self, tone_mapping: ToneMapping) -> Self {
        self.tone_mapping = tone_mapping;
        self
    }

    /// The filter for the noise, `None` for the samples as they are. [AtrousDenoiser::default] by default.
    pub fn with_denoiser(mut self, denoiser: Option<AtrousDenoiser>) -> Self {
        self.denoiser = denoiser;
        self.count();
        self
    }

    ///
    /// Longest time an update traces: on the GPU its own time for the tiles of an update (where it is measured), on
    /// the CPU the time of the rows. 40 ms by default.
    ///
    pub fn with_frame_time(mut self, frame_time: Duration) -> Self {
        self.frame_time = frame_time;
        self
    }

    /// Most pixels of a band, smaller for less memory on the GPU.
    pub fn with_band_pixels(mut self, pixels: u32) -> Self {
        self.band_pixels = pixels.max(1);
        self.count();
        self
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Samples per pixel.
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// The camera of the whole image.
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// How much is done, from 0 to 1.
    pub fn progress(&self) -> f32 {
        if self.is_done() {
            return 1.0;
        }
        let current = match &self.work {
            #[cfg(feature = "wgpu")]
            Some(Work::Gpu { image, .. }) => {
                image.sample_count() as u64 * image.width() as u64 * image.height() as u64
            }
            Some(Work::Cpu { image, .. }) => {
                image.sample_count() as u64 * image.width() as u64 * image.height() as u64
            }
            #[cfg(feature = "wgpu")]
            Some(Work::Reading(_)) => {
                let band = self.band_of(self.band);
                (band.traced.end - band.traced.start) as u64
                    * self.width as u64
                    * self.samples as u64
            }
            None => 0,
        };
        ((self.traced + current) as f64 / self.total.max(1) as f64).min(0.999) as f32
    }

    /// Time since the first trace.
    pub fn elapsed(&self) -> Duration {
        self.started.map(|s| s.elapsed()).unwrap_or_default()
    }

    /// Time still needed at the pace so far, `None` before there is a pace.
    pub fn remaining(&self) -> Option<Duration> {
        let progress = self.progress() as f64;
        if self.is_done() {
            return Some(Duration::ZERO);
        }
        (progress > 0.001).then(|| self.elapsed().mul_f64((1.0 - progress) / progress))
    }

    /// Whether the image is complete.
    pub fn is_done(&self) -> bool {
        self.band >= self.bands()
    }

    /// The 8 bit sRGB colors, rows from the top, once the image is complete.
    pub fn rgba8(&self) -> Option<&[[u8; 4]]> {
        self.is_done().then_some(self.pixels.as_slice())
    }

    /// The image as PNG, once complete. Only with the feature `export`, which is on by default.
    #[cfg(feature = "export")]
    pub fn png(&self) -> Option<Vec<u8>> {
        use image::ImageEncoder;
        let pixels = self.rgba8()?;
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                pixels.as_flattened(),
                self.width,
                self.height,
                image::ExtendedColorType::Rgba8,
            )
            .ok()?;
        Some(bytes)
    }

    /// Rows kept from each band.
    fn band_rows(&self) -> u32 {
        (self.band_pixels / self.width)
            .saturating_sub(2 * self.margin())
            .max(16)
    }

    /// Rows above and below a band for the denoiser: as far as its filter reaches.
    fn margin(&self) -> u32 {
        self.denoiser
            .map_or(0, |d| 2 * ((1u32 << d.iterations.min(10)) - 1))
    }

    fn bands(&self) -> u32 {
        self.height.div_ceil(self.band_rows())
    }

    fn band_of(&self, band: u32) -> Band {
        let rows = self.band_rows();
        let kept = band * rows..((band + 1) * rows).min(self.height);
        let traced =
            kept.start.saturating_sub(self.margin())..(kept.end + self.margin()).min(self.height);
        Band { traced, kept }
    }

    fn count(&mut self) {
        self.total = (0..self.bands())
            .map(|b| {
                let band = self.band_of(b);
                (band.traced.end - band.traced.start) as u64
                    * self.width as u64
                    * self.samples as u64
            })
            .sum();
    }

    /// The settings of the band in progress.
    fn band_settings(&self, band: &Band) -> TraceSettings {
        TraceSettings {
            samples_per_call: 1,
            view_region: Some(ViewRegion::rows(band.traced.clone(), self.height)),
            // Other random numbers in every band, whose pixels have the same coordinates
            seed: self.settings.seed ^ self.band.wrapping_mul(0x9e37_79b9),
            ..self.settings
        }
    }

    /// Puts the band's colors in place and goes on with the next band.
    fn finish_band(&mut self, colors: &[[u8; 4]]) {
        let band = self.band_of(self.band);
        if self.pixels.is_empty() {
            self.pixels = vec![[0, 0, 0, 255]; self.width as usize * self.height as usize];
        }
        let w = self.width as usize;
        let skip = (band.kept.start - band.traced.start) as usize;
        let rows = (band.kept.end - band.kept.start) as usize;
        let from = &colors[skip * w..(skip + rows) * w];
        self.pixels[band.kept.start as usize * w..band.kept.end as usize * w].copy_from_slice(from);
        self.traced +=
            (band.traced.end - band.traced.start) as u64 * self.width as u64 * self.samples as u64;
        self.band += 1;
        self.work = None;
    }

    ///
    /// Traces on the CPU for at most [FinalRender::with_frame_time], with a tracer of its own of `threads` threads.
    /// Returns whether the image is complete.
    ///
    pub fn trace_cpu(&mut self, scene: &RtScene, threads: usize) -> Result<bool, RtError> {
        if self.is_done() {
            return Ok(true);
        }
        let start = Instant::now();
        self.started.get_or_insert(start);
        let band = self.band_of(self.band);
        let settings = self.band_settings(&band);
        let rows = band.traced.end - band.traced.start;
        let tracer = self
            .cpu
            .get_or_insert_with(|| CpuReferenceTracer::with_threads(threads));
        let threads = tracer.thread_count() as u32;
        if !matches!(self.work, None | Some(Work::Cpu { .. })) {
            // The band began on the GPU, which is gone: again on the CPU
            self.work = None;
        }
        let work = self.work.get_or_insert_with(|| Work::Cpu {
            image: RenderImage::with_features(self.width, rows),
            next_row: 0,
            rows_per_call: 2 * threads,
        });
        let (image, next_row, rows_per_call) = match work {
            Work::Cpu {
                image,
                next_row,
                rows_per_call,
            } => (image, next_row, rows_per_call),
            #[cfg(feature = "wgpu")]
            _ => unreachable!("made for the CPU"),
        };
        while image.sample_count() < self.samples {
            // Some rows for every thread at once, as many as fit into the frame time
            let row = *next_row;
            let end = (row + *rows_per_call).min(rows);
            let call = Instant::now();
            tracer.trace_rows(
                scene,
                &self.camera,
                image,
                &settings,
                image.sample_count(),
                row..end,
            )?;
            let ms = call.elapsed().as_secs_f64() * 1000.0 / (end - row) as f64;
            let fit = (self.frame_time.as_secs_f64() * 1000.0 / ms.max(0.001)) as u32;
            *rows_per_call = fit.clamp(threads, rows.max(threads));
            *next_row = end;
            if *next_row >= rows {
                image.add_samples(1);
                *next_row = 0;
            }
            if start.elapsed() >= self.frame_time {
                return Ok(false);
            }
        }
        let colors = match self.denoiser {
            Some(mut denoiser) => denoiser.denoise(image)?.to_srgb8(&self.tone_mapping),
            None => image.to_srgb8(&self.tone_mapping),
        };
        self.finish_band(&colors);
        Ok(self.is_done())
    }

    ///
    /// Traces on the GPU with the compute tracer: as many tiles as fit into [FinalRender::with_frame_time] where the
    /// GPU's time is measured, else one. Returns whether the image is complete.
    ///
    #[cfg(feature = "wgpu")]
    pub fn trace_gpu(
        &mut self,
        context: &three_d::Context,
        tracer: &mut crate::ComputeTracer,
        scene: &RtScene,
    ) -> Result<bool, RtError> {
        if self.is_done() {
            return Ok(true);
        }
        self.started.get_or_insert_with(Instant::now);
        let band = self.band_of(self.band);
        let settings = self.band_settings(&band);
        let rows = band.traced.end - band.traced.start;
        if matches!(self.work, Some(Work::Cpu { .. })) {
            // The band began on the CPU: again on the GPU, which has come
            self.work = None;
        }
        if self.work.is_none() {
            // Times of the calls before are not of this band's tiles
            tracer.take_call_times();
            self.work = Some(Work::Gpu {
                image: if self.denoiser.is_some() {
                    crate::GpuRenderImage::with_features(tracer.gpu(), self.width, rows)
                } else {
                    crate::GpuRenderImage::new(tracer.gpu(), self.width, rows)
                },
                tile_ms: None,
            });
        }
        match self.work.as_mut().expect("made") {
            Work::Gpu { image, tile_ms } => {
                for call in tracer.take_call_times() {
                    *tile_ms = crate::preview::next_tile_ms(*tile_ms, call);
                }
                if image.sample_count() < self.samples {
                    if tracer.unfinished_calls() > crate::gpu::frames_ahead(tracer.adapter_info()) {
                        return Ok(false);
                    }
                    let tiles = crate::preview::sharp_tiles(
                        *tile_ms,
                        self.frame_time,
                        tracer.tiles_per_sample(image.width(), image.height()),
                    );
                    tracer.trace_gpu_tiles(scene, &self.camera, image, &settings, tiles)?;
                    return Ok(false);
                }
                // Denoised and tone mapped on the GPU, then read back
                let linear = crate::preview::empty_texture(context, self.width, rows);
                match &self.denoiser {
                    Some(denoiser) => image.write_denoised_to_three_d_texture(&linear, denoiser),
                    None => image.write_to_three_d_texture(&linear),
                }
                let colors = three_d::Texture2D::new_empty::<[u8; 4]>(
                    context,
                    self.width,
                    rows,
                    three_d::Interpolation::Nearest,
                    three_d::Interpolation::Nearest,
                    None,
                    three_d::Wrapping::ClampToEdge,
                    three_d::Wrapping::ClampToEdge,
                );
                let target = colors.as_color_target(None);
                target.apply_screen_effect(
                    &crate::preview::DisplayEffect {
                        texture: &linear,
                        tone_mapping: self.tone_mapping,
                    },
                    three_d::Camera::new_2d(target.viewport()),
                    &[],
                    None,
                    None,
                );
                // as_wgpu orders three-d's work before the copy
                let texture = colors.as_wgpu().expect("a texture of wgpu");
                self.work = Some(Work::Reading(Reading::start(tracer.gpu(), &texture)));
                Ok(false)
            }
            Work::Reading(reading) => match reading.poll()? {
                Some(colors) => {
                    self.finish_band(&colors);
                    Ok(self.is_done())
                }
                None => Ok(false),
            },
            Work::Cpu { .. } => unreachable!("dropped above"),
        }
    }
}

/// A final render can be traced on another thread than the one that made it, outside of browsers.
#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(dead_code)]
fn final_render_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<FinalRender>();
}
