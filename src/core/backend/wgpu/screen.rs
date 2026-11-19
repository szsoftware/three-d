//!
//! The screen of the wgpu backend.
//!
//! The screen is a set of textures like every other render target, so everything that works on a render target works on the screen
//! with the same conventions, including reading from it. If the backend has a surface (the part of a window that is drawn to),
//! [WgpuBackend::present] draws the color texture of the screen to it, upside down, since the first row of a surface is the top row.
//!

// Only the window module gives a context a screen
#![cfg_attr(not(feature = "window"), allow(dead_code, unused_imports))]

use super::*;

///
/// How the screen is set up.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScreenDesc {
    /// The size in physical pixels.
    pub width: u32,
    pub height: u32,
    /// Whether the screen has a depth buffer.
    pub depth: bool,
    /// Whether the screen is multisampled (4 samples, the only number every WebGPU implementation knows apart from 1).
    pub multisample: bool,
    /// Whether presenting waits for the display.
    pub vsync: bool,
}

pub(super) struct Screen {
    desc: ScreenDesc,
    /// The color with one sample, which is presented and read from.
    color: w::Texture,
    multisample_color: Option<w::Texture>,
    depth: Option<w::Texture>,
    surface: Option<(w::Surface<'static>, w::SurfaceConfiguration)>,
}

impl Screen {
    /// The color texture that is rendered into.
    pub(super) fn color(&self) -> &w::Texture {
        self.multisample_color.as_ref().unwrap_or(&self.color)
    }

    pub(super) fn depth(&self) -> Option<&w::Texture> {
        self.depth.as_ref()
    }
}

/// Which adapter is asked for, see [WgpuBackend::request].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdapterPreference {
    /// Only a hardware adapter.
    Hardware,
    /// Only a hardware adapter of an API other than OpenGL. For when the OpenGL backend is the alternative, which is better than wgpu on top of OpenGL.
    HardwareNotGl,
    /// Whatever there is.
    Any,
    /// The software adapter.
    Software,
}

impl WgpuBackend {
    ///
    /// Asks for an adapter and a device and creates a backend with the device.
    /// With a surface, the adapter is one that can draw to the surface.
    ///
    pub async fn request(
        instance: &w::Instance,
        preference: AdapterPreference,
        surface: Option<&w::Surface<'static>>,
    ) -> Result<(Self, w::Adapter), CoreError> {
        let adapter = instance
            .request_adapter(&w::RequestAdapterOptions {
                power_preference: w::PowerPreference::default(),
                force_fallback_adapter: preference == AdapterPreference::Software,
                compatible_surface: surface,
            })
            .await
            .map_err(|e| CoreError::ContextCreation(e.to_string()));
        #[cfg(all(feature = "wgpu-gl", not(target_arch = "wasm32")))]
        let adapter = Self::prefer_hardware(instance, preference, surface, adapter).await;
        let adapter = adapter?;
        let info = adapter.get_info();
        let hardware = info.device_type != w::DeviceType::Cpu;
        let allowed = match preference {
            AdapterPreference::Hardware => hardware,
            AdapterPreference::HardwareNotGl => hardware && info.backend != w::Backend::Gl,
            AdapterPreference::Any | AdapterPreference::Software => true,
        };
        if !allowed {
            return Err(CoreError::ContextCreation(
                "there is no hardware accelerated wgpu adapter".to_string(),
            ));
        }
        let (device, queue) = adapter
            .request_device(&w::DeviceDescriptor {
                label: Some("three-d"),
                // Optional features are used when the adapter has them
                required_features: adapter.features() & w::Features::FLOAT32_FILTERABLE,
                ..Default::default()
            })
            .await
            .map_err(|e| CoreError::ContextCreation(e.to_string()))?;
        // The device belongs to this backend, so errors can be collected for Context::error_check instead of the default, which is to panic
        let errors = Arc::new(Mutex::new(Vec::new()));
        let sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |error: w::Error| {
            let mut errors = sink.lock().unwrap_or_else(|e| e.into_inner());
            // An application that never checks for errors must not collect them forever
            if errors.len() < MAX_ERRORS {
                errors.push(error.to_string());
            }
        }));
        // wgpu tells when the device is lost (on OpenGL only with the patch of wgpu-hal described in docs/wgpu-backend.md), it stays lost
        let lost = Arc::new(reset::DeviceLost::default());
        let sink = lost.clone();
        device.set_device_lost_callback(move |_, message| {
            sink.lose(if message.is_empty() {
                "wgpu lost the device".to_string()
            } else {
                format!("wgpu lost the device ({message})")
            })
        });
        let mut backend = Self::new(device, queue);
        backend.errors = Some(errors);
        backend.lost = lost;
        backend.float32_renderable = adapter
            .get_texture_format_features(w::TextureFormat::Rgba32Float)
            .allowed_usages
            .contains(w::TextureUsages::RENDER_ATTACHMENT);
        Ok((backend, adapter))
    }

    ///
    /// wgpu asks Vulkan, Metal and DirectX 12 first and takes what they offer, even if that is a software adapter (as on a graphics
    /// card whose Vulkan driver wgpu does not accept). With OpenGL compiled in, a hardware adapter of OpenGL is better than that.
    ///
    #[cfg(all(feature = "wgpu-gl", not(target_arch = "wasm32")))]
    async fn prefer_hardware(
        instance: &w::Instance,
        preference: AdapterPreference,
        surface: Option<&w::Surface<'static>>,
        chosen: Result<w::Adapter, CoreError>,
    ) -> Result<w::Adapter, CoreError> {
        let is_hardware =
            |adapter: &w::Adapter| adapter.get_info().device_type != w::DeviceType::Cpu;
        let wanted = match preference {
            AdapterPreference::Hardware | AdapterPreference::Any => true,
            AdapterPreference::HardwareNotGl | AdapterPreference::Software => false,
        };
        if !wanted || chosen.as_ref().is_ok_and(is_hardware) {
            return chosen;
        }
        let hardware = instance
            .enumerate_adapters(w::Backends::all())
            .await
            .into_iter()
            .find(|adapter| {
                is_hardware(adapter)
                    && surface.is_none_or(|surface| adapter.is_surface_supported(surface))
            });
        match hardware {
            Some(adapter) => Ok(adapter),
            None => chosen,
        }
    }

    ///
    /// Gives the backend a screen that is presented on the given surface. The adapter is the one the backend was created with.
    ///
    pub fn set_surface(
        &self,
        surface: w::Surface<'static>,
        adapter: &w::Adapter,
        desc: ScreenDesc,
    ) -> Result<(), CoreError> {
        let capabilities = surface.get_capabilities(adapter);
        let format = *capabilities.formats.first().ok_or_else(|| {
            CoreError::ContextCreation("the wgpu adapter cannot draw to the window".to_string())
        })?;
        // The shaders of three-d encode the colors themselves, as for the default framebuffer of OpenGL,
        // so the surface is written to through a view without sRGB encoding
        let view_format = format.remove_srgb_suffix();
        let configuration = w::SurfaceConfiguration {
            usage: w::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: desc.width.max(1),
            height: desc.height.max(1),
            present_mode: present_mode(desc.vsync),
            desired_maximum_frame_latency: 2,
            alpha_mode: if capabilities
                .alpha_modes
                .contains(&w::CompositeAlphaMode::Opaque)
            {
                w::CompositeAlphaMode::Opaque
            } else {
                w::CompositeAlphaMode::Auto
            },
            view_formats: if view_format == format {
                Vec::new()
            } else {
                vec![view_format]
            },
        };
        surface.configure(&self.device, &configuration);
        self.set_screen(desc);
        self.lock().screen.as_mut().unwrap().surface = Some((surface, configuration));
        Ok(())
    }

    ///
    /// Creates the textures of the screen. Keeps the surface if there is one, but does not change its size, see [WgpuBackend::resize_screen].
    ///
    pub fn set_screen(&self, desc: ScreenDesc) {
        let texture = |format, multisample: bool| {
            self.device.create_texture(&w::TextureDescriptor {
                label: Some("three-d screen"),
                size: w::Extent3d {
                    width: desc.width.max(1),
                    height: desc.height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: if multisample { 4 } else { 1 },
                dimension: w::TextureDimension::D2,
                format,
                usage: if multisample {
                    self.multisample_usage()
                } else {
                    w::TextureUsages::TEXTURE_BINDING
                        | w::TextureUsages::COPY_SRC
                        | w::TextureUsages::RENDER_ATTACHMENT
                },
                view_formats: &[],
            })
        };
        let state = &mut *self.lock();
        // Recorded commands may still render to the old textures
        self.flush(state);
        let surface = state.screen.take().and_then(|screen| screen.surface);
        state.screen = Some(Screen {
            desc,
            color: texture(w::TextureFormat::Rgba8Unorm, false),
            multisample_color: desc
                .multisample
                .then(|| texture(w::TextureFormat::Rgba8Unorm, true)),
            depth: desc
                .depth
                .then(|| texture(w::TextureFormat::Depth32Float, desc.multisample)),
            surface,
        });
    }

    /// Whether there is a screen without a surface.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn has_offscreen_screen(&self) -> bool {
        self.lock()
            .screen
            .as_ref()
            .is_some_and(|screen| screen.surface.is_none())
    }

    ///
    /// Changes the size of the screen and its surface.
    ///
    pub fn resize_screen(&self, width: u32, height: u32) {
        let desc = {
            let state = &mut *self.lock();
            let screen = state.screen.as_mut().expect(NO_SCREEN);
            if (screen.desc.width, screen.desc.height) == (width, height) {
                return;
            }
            if let Some((surface, configuration)) = &mut screen.surface {
                configuration.width = width.max(1);
                configuration.height = height.max(1);
                surface.configure(&self.device, configuration);
            }
            ScreenDesc {
                width,
                height,
                ..screen.desc
            }
        };
        self.set_screen(desc);
    }

    ///
    /// Changes whether presenting waits for the display.
    ///
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn set_vsync(&self, vsync: bool) {
        let state = &mut *self.lock();
        let screen = state.screen.as_mut().expect(NO_SCREEN);
        screen.desc.vsync = vsync;
        if let Some((surface, configuration)) = &mut screen.surface {
            configuration.present_mode = present_mode(vsync);
            surface.configure(&self.device, configuration);
        }
    }

    ///
    /// Shows what was rendered to the screen on the surface. Without a surface, only submits what is recorded.
    /// The frame is left out if the surface has to be set up again, for example after the window changed its size.
    ///
    pub fn present(&self) -> Result<(), CoreError> {
        let state = &mut *self.lock();
        let screen = state.screen.as_ref().expect(NO_SCREEN);
        let Some((surface, configuration)) = &screen.surface else {
            self.flush(state);
            return self.lost.error();
        };
        let frame = match surface.get_current_texture() {
            w::CurrentSurfaceTexture::Success(frame)
            | w::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            w::CurrentSurfaceTexture::Outdated | w::CurrentSurfaceTexture::Lost => {
                surface.configure(&self.device, configuration);
                self.flush(state);
                return Ok(());
            }
            w::CurrentSurfaceTexture::Timeout | w::CurrentSurfaceTexture::Occluded => {
                self.flush(state);
                return Ok(());
            }
            w::CurrentSurfaceTexture::Validation => {
                return Err(CoreError::ContextError(
                    "the surface of the window is invalid".to_string(),
                ));
            }
        };
        let format = configuration.format.remove_srgb_suffix();
        let destination = frame.texture.create_view(&w::TextureViewDescriptor {
            format: Some(format),
            ..Default::default()
        });
        self.present_to(state, &destination, format);
        self.flush(state);
        frame.present();
        Ok(())
    }

    ///
    /// Records the pass that draws the color of the screen to the given view the way a surface expects it, with the top row first.
    ///
    pub(super) fn present_to(
        &self,
        state: &mut State,
        destination: &w::TextureView,
        format: w::TextureFormat,
    ) {
        let screen = state.screen.as_ref().expect(NO_SCREEN);
        let color = Slice {
            texture: screen.color.clone(),
            mip_level: 0,
            layer: 0,
        };
        if let Some(multisample_color) = &screen.multisample_color {
            let multisample_color = Slice {
                texture: multisample_color.clone(),
                mip_level: 0,
                layer: 0,
            };
            self.resolve_color(state, &multisample_color, &color);
        }
        self.builtin_pass(
            state,
            builtin::Kind::Present,
            &color.view(),
            destination,
            format,
            None,
        );
    }
}

fn present_mode(vsync: bool) -> w::PresentMode {
    if vsync {
        w::PresentMode::AutoVsync
    } else {
        w::PresentMode::AutoNoVsync
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::*;

    const WIDTH: u32 = 40;
    const HEIGHT: u32 = 24;

    /// A context with a screen but without a surface.
    fn context(multisample: bool) -> Option<Context> {
        let context = match Context::new_headless_wgpu() {
            Ok(context) => context,
            Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
                panic!("no wgpu adapter: {error}")
            }
            Err(error) => {
                eprintln!("skipped, no wgpu adapter: {error}");
                return None;
            }
        };
        context.wgpu_backend().set_screen(ScreenDesc {
            width: WIDTH,
            height: HEIGHT,
            depth: true,
            multisample,
            vsync: true,
        });
        Some(context)
    }

    /// Two spheres, the one in front is not symmetric in the image, so a flipped image is noticed.
    fn render(context: &Context, target: &RenderTarget, width: u32, height: u32) {
        let camera = Camera::new_perspective(
            Viewport::new_at_origo(width, height),
            vec3(0.0, 0.0, 5.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            degrees(45.0),
            0.1,
            10.0,
        );
        let sphere = |color, position| {
            let mut sphere = Gm::new(
                Mesh::new(context, &CpuMesh::sphere(16)),
                ColorMaterial {
                    color,
                    ..Default::default()
                },
            );
            sphere.set_transformation(Mat4::from_translation(position));
            sphere
        };
        target
            .clear(ClearState::color_and_depth(0.2, 0.4, 0.6, 1.0, 1.0))
            .render(
                &camera,
                [
                    &sphere(Srgba::RED, vec3(0.5, 0.8, 1.0)),
                    &sphere(Srgba::GREEN, vec3(0.0, 0.0, 0.0)),
                ],
                &[],
            );
    }

    fn render_to_texture(context: &Context) -> (Vec<[u8; 4]>, Vec<f32>) {
        let color = Texture2D::new_empty::<[u8; 4]>(
            context,
            WIDTH,
            HEIGHT,
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        let depth = DepthTexture2D::new::<f32>(
            context,
            WIDTH,
            HEIGHT,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        let target = RenderTarget::new(color.as_color_target(None), depth.as_depth_target());
        render(context, &target, WIDTH, HEIGHT);
        (target.read_color(), target.read_depth())
    }

    #[test]
    fn screen_is_a_render_target() {
        let Some(context) = context(false) else {
            return;
        };
        let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
        render(&context, &screen, WIDTH, HEIGHT);
        let (color, depth) = render_to_texture(&context);
        assert_eq!(screen.read_color::<[u8; 4]>(), color);
        assert_eq!(screen.read_depth(), depth);
        assert!(color.contains(&[255, 0, 0, 255]) && color.contains(&[0, 255, 0, 255]));
        context.error_check().unwrap();
        crate::core::backend::wgpu::compare_tests::release(&context);
    }

    #[test]
    fn multisampled_screen() {
        let Some(context) = context(true) else {
            return;
        };
        let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
        render(&context, &screen, WIDTH, HEIGHT);
        let actual = screen.read_color::<[u8; 4]>();
        let (expected, _) = render_to_texture(&context);
        // The edges of the spheres are smooth, everything else is the same
        let different = actual.iter().zip(&expected).filter(|(a, e)| a != e).count();
        assert!(different > 0, "the screen is not multisampled");
        assert!(different < actual.len() / 4, "{different} pixels differ");
        assert!(actual.contains(&[255, 0, 0, 255]) && actual.contains(&[0, 255, 0, 255]));
        // The depth of a pixel is the depth of one of its samples
        let depth = screen.read_depth();
        assert_eq!(depth[0], 1.0);
        assert!(depth[(HEIGHT / 2 * WIDTH + WIDTH / 2) as usize] < 1.0);
        context.error_check().unwrap();
        crate::core::backend::wgpu::compare_tests::release(&context);
    }

    #[test]
    fn present_draws_the_top_row_first() {
        for multisample in [false, true] {
            let Some(context) = context(multisample) else {
                return;
            };
            let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
            render(&context, &screen, WIDTH, HEIGHT);
            let expected = screen.read_color::<[u8; 4]>();
            let backend = context.wgpu_backend();
            let presented = {
                let state = &mut *backend.lock();
                // What a surface would get, in a format with another order of the channels
                let format = w::TextureFormat::Bgra8Unorm;
                let surface = backend.temporary_texture(format, WIDTH, HEIGHT);
                backend.present_to(state, &surface.view(), format);
                let rect = ScissorBox::new_at_origo(WIDTH, HEIGHT);
                backend.read_texture(state, &surface, rect, 4)
            };
            // The first row of a surface is the top row, which is also the first row that is read from a render target
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let [r, g, b, _] = expected[(y * WIDTH + x) as usize];
                    let index = ((y * WIDTH + x) * 4) as usize;
                    assert_eq!(presented[index..index + 4], [b, g, r, 255], "at {x}, {y}");
                }
            }
            // Without a surface, presenting only submits
            backend.present().unwrap();
            context.error_check().unwrap();
            crate::core::backend::wgpu::compare_tests::release(&context);
        }
    }

    #[test]
    fn screen_changes_its_size() {
        let Some(context) = context(true) else {
            return;
        };
        let backend = context.wgpu_backend();
        backend.resize_screen(WIDTH * 2, HEIGHT + 3);
        let screen = RenderTarget::screen(&context, WIDTH * 2, HEIGHT + 3);
        render(&context, &screen, WIDTH * 2, HEIGHT + 3);
        let color = screen.read_color::<[u8; 4]>();
        assert_eq!(color.len() as u32, WIDTH * 2 * (HEIGHT + 3));
        assert!(color.contains(&[255, 0, 0, 255]));
        // A window that is minimized has no size
        backend.resize_screen(0, 0);
        backend.present().unwrap();
        context.error_check().unwrap();
        crate::core::backend::wgpu::compare_tests::release(&context);
    }

    #[test]
    fn offscreen_screen_of_a_headless_context() {
        for multisample in [false, true] {
            let context =
                match Context::new_headless_wgpu_with_screen(WIDTH, HEIGHT, true, multisample) {
                    Ok(context) => context,
                    Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
                        panic!("no wgpu adapter: {error}")
                    }
                    Err(error) => {
                        eprintln!("skipped, no wgpu adapter: {error}");
                        return;
                    }
                };
            // The same into a texture is what the screen has to show
            let texture = Texture2D::new_empty::<[u8; 4]>(
                &context,
                WIDTH,
                HEIGHT,
                Interpolation::Nearest,
                Interpolation::Nearest,
                None,
                Wrapping::ClampToEdge,
                Wrapping::ClampToEdge,
            );
            let depth_texture = DepthTexture2D::new::<f32>(
                &context,
                WIDTH,
                HEIGHT,
                Wrapping::ClampToEdge,
                Wrapping::ClampToEdge,
            );
            let target = RenderTarget::new(
                texture.as_color_target(None),
                depth_texture.as_depth_target(),
            );
            render(&context, &target, WIDTH, HEIGHT);
            let expected = target.read_color::<[u8; 4]>();

            let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
            render(&context, &screen, WIDTH, HEIGHT);
            let actual = screen.read_color::<[u8; 4]>();
            assert_eq!(actual.len() as u32, WIDTH * HEIGHT);
            if multisample {
                let different = actual.iter().zip(&expected).filter(|(a, e)| a != e).count();
                assert!(different < actual.len() / 4, "{different} pixels differ");
            } else {
                assert_eq!(actual, expected);
            }

            // The first row is the top row: the red sphere is up and to the right of the center
            let red: Vec<u32> = (0..WIDTH * HEIGHT)
                .filter(|i| actual[*i as usize] == [255, 0, 0, 255])
                .collect();
            assert!(!red.is_empty());
            let row = red.iter().map(|i| i / WIDTH).sum::<u32>() / red.len() as u32;
            let column = red.iter().map(|i| i % WIDTH).sum::<u32>() / red.len() as u32;
            assert!(
                row < HEIGHT / 2 && column > WIDTH / 2,
                "row {row}, column {column}"
            );
            // A part of the screen is addressed from the bottom, as everywhere
            let corner = screen.read_color_partially::<[u8; 4]>(ScissorBox {
                x: 0,
                y: HEIGHT as i32 - 2,
                width: 3,
                height: 2,
            });
            assert_eq!(corner[..3], actual[..3]);
            assert_eq!(corner[3..], actual[WIDTH as usize..WIDTH as usize + 3]);

            // A frame that is not read, then another size
            render(&context, &screen, WIDTH, HEIGHT);
            context.submit_offscreen_screen().unwrap();
            context
                .resize_offscreen_screen(WIDTH * 2, HEIGHT + 3)
                .unwrap();
            let screen = RenderTarget::screen(&context, WIDTH * 2, HEIGHT + 3);
            render(&context, &screen, WIDTH * 2, HEIGHT + 3);
            let color = screen.read_color::<[u8; 4]>();
            assert_eq!(color.len() as u32, WIDTH * 2 * (HEIGHT + 3));
            assert!(color.contains(&[255, 0, 0, 255]));
            assert!(screen.read_depth().iter().any(|d| *d < 1.0));
            context.error_check().unwrap();
            crate::core::backend::wgpu::compare_tests::release(&context);
        }
    }

    #[test]
    fn context_without_screen_has_no_offscreen_screen() {
        let Ok(context) = Context::new_headless_wgpu() else {
            return;
        };
        assert!(context.resize_offscreen_screen(WIDTH, HEIGHT).is_err());
        assert!(context.submit_offscreen_screen().is_err());
    }

    #[test]
    fn context_is_freed_without_cached_programs() {
        let Ok(context) = Context::new_headless_wgpu_with_screen(WIDTH, HEIGHT, true, false) else {
            return;
        };
        let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
        render(&context, &screen, WIDTH, HEIGHT);
        drop(screen);
        // The programs in the cache hold the context, so it would never be freed
        assert!(!context.programs.read().unwrap().is_empty());
        assert!(context.backend_references() > 1);
        context.programs.write().unwrap().clear();
        assert_eq!(context.backend_references(), 1);
    }

    #[test]
    fn a_lost_device_is_reported() {
        let Ok(context) = Context::new_headless_wgpu_with_screen(WIDTH, HEIGHT, true, false) else {
            return;
        };
        let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
        render(&context, &screen, WIDTH, HEIGHT);
        assert!(!screen.read_color::<[u8; 4]>().is_empty());
        assert!(!context.is_lost());
        context.error_check().unwrap();
        // What a reset of the graphics card does to wgpu, on OpenGL with the patched wgpu-hal
        context.as_wgpu().unwrap().0.destroy();
        assert!(context.is_lost());
        let error = context.error_check().unwrap_err().to_string();
        assert!(error.contains("graphics device is lost"), "{error}");
        // Still lost, and every frame says so
        assert!(context.error_check().is_err());
        assert!(context.submit_offscreen_screen().is_err());
        // Pixels would not be the ones that were drawn
        render(&context, &screen, WIDTH, HEIGHT);
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            screen.read_color::<[u8; 4]>()
        }));
        let message = read.unwrap_err();
        let message = message
            .downcast_ref::<String>()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(message.contains("graphics device is lost"), "{message}");
        drop(screen);
        context.programs.write().unwrap().clear();
    }

    #[test]
    #[should_panic(expected = "the context has no screen")]
    fn context_without_window_has_no_screen() {
        let Ok(context) = Context::new_headless_wgpu() else {
            panic!("the context has no screen (skipped, no wgpu adapter)");
        };
        RenderTarget::screen(&context, WIDTH, HEIGHT).clear(ClearState::color(0.0, 0.0, 0.0, 1.0));
    }
}
