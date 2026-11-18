use crate::Context;
use crate::SurfaceSettings;
use crate::WindowError;
use std::sync::Arc;
use winit::window::Window;

///
/// Creates a context that uses the wgpu backend and has a screen which is presented on the given window or canvas.
///
#[cfg(feature = "wgpu")]
mod wgpu_context {
    use super::*;
    use crate::core::backend::{
        wgpu::{AdapterPreference, ScreenDesc, WgpuBackend},
        Backend,
    };
    use crate::{BackendPreference, CoreError, HardwareAcceleration};

    fn error(error: impl std::fmt::Display) -> WindowError {
        WindowError::ThreeDError(CoreError::ContextCreation(error.to_string()))
    }

    ///
    /// The surface is created by the given function, after it is known that there is an adapter and a device.
    /// In a browser, a canvas that was once used for WebGPU cannot be used for WebGL anymore.
    ///
    pub(super) async fn new(
        instance: ::wgpu::Instance,
        create_surface: impl FnOnce(&::wgpu::Instance) -> Result<::wgpu::Surface<'static>, WindowError>,
        (width, height): (u32, u32),
        settings: SurfaceSettings,
        preference: BackendPreference,
    ) -> Result<Context, WindowError> {
        let adapter_preference = match settings.hardware_acceleration {
            HardwareAcceleration::Off => AdapterPreference::Software,
            HardwareAcceleration::Required => AdapterPreference::Hardware,
            // With the choice between the backends, OpenGL on hardware is better than wgpu in software
            HardwareAcceleration::Preferred if preference == BackendPreference::Auto => {
                AdapterPreference::HardwareNotGl
            }
            HardwareAcceleration::Preferred => AdapterPreference::Any,
        };
        // In a browser the adapter is asked for before the surface exists, see above
        #[cfg(not(target_arch = "wasm32"))]
        let (surface, (backend, adapter)) = {
            let surface = create_surface(&instance)?;
            let backend =
                WgpuBackend::request(&instance, adapter_preference, Some(&surface)).await?;
            (surface, backend)
        };
        #[cfg(target_arch = "wasm32")]
        let (surface, (backend, adapter)) = {
            let backend = WgpuBackend::request(&instance, adapter_preference, None).await?;
            (create_surface(&instance)?, backend)
        };
        backend.set_surface(
            surface,
            &adapter,
            ScreenDesc {
                width,
                height,
                depth: settings.depth_buffer > 0,
                multisample: settings.multisamples > 0,
                vsync: settings.vsync,
            },
        )?;
        Ok(Context::from_backend(Backend::Wgpu(Box::new(backend))))
    }

    pub(super) fn surface_error(e: ::wgpu::CreateSurfaceError) -> WindowError {
        error(e)
    }
}

#[cfg(not(feature = "wgpu"))]
mod wgpu_context {
    use super::*;

    pub(super) fn not_compiled() -> WindowError {
        WindowError::ThreeDError(crate::CoreError::ContextCreation(
            "the wgpu backend is asked for, but the crate is not compiled with the `wgpu` feature"
                .to_string(),
        ))
    }
}

#[cfg(target_arch = "wasm32")]
mod inner {
    use crate::HardwareAcceleration;
    use serde::{Deserialize, Serialize};
    use wasm_bindgen::JsCast;
    use web_sys::HtmlCanvasElement;
    use winit::platform::web::WindowExtWebSys;

    use super::*;
    #[allow(non_snake_case)]
    #[derive(Serialize, Deserialize)]
    struct ContextOpt {
        pub antialias: bool,
        pub depth: bool,
        pub stencil: bool,
        pub willReadFrequently: bool,
        pub alpha: bool,
    }

    /// A context used for rendering
    pub struct WindowedContext {
        pub(super) context: Context,
        canvas: HtmlCanvasElement,
    }

    impl WindowedContext {
        /// Creates a new context from a [winit] window.
        pub fn from_winit_window(
            window: &Window,
            settings: SurfaceSettings,
        ) -> Result<Self, WindowError> {
            let canvas = window.canvas().ok_or(WindowError::CanvasMissing)?;
            Self::from_canvas(canvas, settings)
        }

        /// Creates a new context from a canvas.
        pub fn from_canvas(
            canvas: HtmlCanvasElement,
            settings: SurfaceSettings,
        ) -> Result<Self, WindowError> {
            // get webgl context and verify extensions
            let webgl_context = canvas
                .get_context_with_context_options(
                    "webgl2",
                    &serde_wasm_bindgen::to_value(&ContextOpt {
                        antialias: settings.multisamples > 0,
                        depth: settings.depth_buffer > 0,
                        stencil: settings.stencil_buffer > 0,
                        willReadFrequently: match settings.hardware_acceleration {
                            HardwareAcceleration::Required => false,
                            HardwareAcceleration::Preferred => false,
                            HardwareAcceleration::Off => true,
                        },
                        alpha: false,
                    })
                    .unwrap(),
                )
                .map_err(|e| WindowError::WebGL2NotSupported(format!(": {:?}", e)))?
                .ok_or(WindowError::WebGL2NotSupported("".to_string()))?
                .dyn_into::<web_sys::WebGl2RenderingContext>()
                .map_err(|e| WindowError::WebGL2NotSupported(format!(": {:?}", e)))?;
            webgl_context
                .get_extension("EXT_color_buffer_float")
                .map_err(|e| WindowError::ColorBufferFloatNotSupported(format!("{:?}", e)))?;
            webgl_context
                .get_extension("OES_texture_float_linear")
                .map_err(|e| WindowError::OESTextureFloatNotSupported(format!(": {:?}", e)))?;
            webgl_context
                .get_extension("OES_texture_half_float_linear")
                .map_err(|e| WindowError::OESTextureFloatNotSupported(format!(": {:?}", e)))?;

            Ok(Self {
                context: Context::from_gl_context(Arc::new(
                    crate::context::Context::from_webgl2_context(webgl_context),
                ))?,
                canvas,
            })
        }

        ///
        /// Creates a new context from a [winit] window which uses the preferred backend, see [BackendPreference](crate::BackendPreference).
        /// Asynchronous, because a browser cannot wait for WebGPU in any other way.
        ///
        pub async fn from_winit_window_async(
            window: &Window,
            settings: SurfaceSettings,
            preference: crate::BackendPreference,
        ) -> Result<Self, WindowError> {
            let canvas = window.canvas().ok_or(WindowError::CanvasMissing)?;
            Self::from_canvas_async(canvas, settings, preference).await
        }

        ///
        /// Creates a new context from a canvas which uses the preferred backend, see [BackendPreference](crate::BackendPreference).
        /// With [BackendPreference::Auto](crate::BackendPreference::Auto), WebGPU is used if the browser supports it and WebGL2 otherwise.
        ///
        pub async fn from_canvas_async(
            canvas: HtmlCanvasElement,
            settings: SurfaceSettings,
            preference: crate::BackendPreference,
        ) -> Result<Self, WindowError> {
            use crate::BackendPreference;
            if preference == BackendPreference::Gl {
                return Self::from_canvas(canvas, settings);
            }
            #[cfg(feature = "wgpu")]
            {
                let instance =
                    ::wgpu::Instance::new(::wgpu::InstanceDescriptor::new_without_display_handle());
                let target = canvas.clone();
                let context = wgpu_context::new(
                    instance,
                    |instance| {
                        instance
                            .create_surface(::wgpu::SurfaceTarget::Canvas(target))
                            .map_err(wgpu_context::surface_error)
                    },
                    (canvas.width(), canvas.height()),
                    settings,
                    preference,
                )
                .await;
                match context {
                    Ok(context) => Ok(Self { context, canvas }),
                    Err(_) if preference == BackendPreference::Auto => {
                        Self::from_canvas(canvas, settings)
                    }
                    Err(error) => Err(error),
                }
            }
            #[cfg(not(feature = "wgpu"))]
            {
                if preference == BackendPreference::Auto {
                    Self::from_canvas(canvas, settings)
                } else {
                    Err(wgpu_context::not_compiled())
                }
            }
        }

        /// Resizes the context
        pub fn resize(&self, physical_size: winit::dpi::PhysicalSize<u32>) {
            self.canvas.set_width(physical_size.width as u32);
            self.canvas.set_height(physical_size.height as u32);
            #[cfg(feature = "wgpu")]
            if self.context.as_wgpu().is_some() {
                self.context
                    .wgpu_backend()
                    .resize_screen(physical_size.width, physical_size.height);
            }
        }

        /// Make this context current. Needed when using multiple windows (contexts) on native.
        pub fn make_current(&self) -> Result<(), WindowError> {
            Ok(())
        }

        /// Swap buffers - should always be called after rendering.
        pub fn swap_buffers(&self) -> Result<(), WindowError> {
            #[cfg(feature = "wgpu")]
            if self.context.as_wgpu().is_some() {
                self.context.wgpu_backend().present()?;
            }
            Ok(())
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod inner {
    use glutin::{prelude::*, surface::*};

    use super::*;
    ///
    /// A windowed graphics context, ie. a graphics context that is associated with a window.
    ///
    pub struct WindowedContext {
        pub(super) context: Context,
        /// The OpenGL surface and context, `None` with the wgpu backend, which keeps its surface itself.
        gl: Option<(
            Surface<WindowSurface>,
            glutin::context::PossiblyCurrentContext,
        )>,
    }

    impl WindowedContext {
        /// Creates a new windowed context from a [winit](https://crates.io/crates/winit) window.
        #[allow(unsafe_code)]
        pub fn from_winit_window(
            window: &Window,
            settings: SurfaceSettings,
        ) -> Result<Self, WindowError> {
            if settings.multisamples > 0 && !settings.multisamples.is_power_of_two() {
                Err(WindowError::InvalidNumberOfMSAASamples)?;
            }
            use glutin::prelude::*;
            use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
            let raw_display_handle = window.display_handle()?.as_raw();
            let raw_window_handle = window.window_handle()?.as_raw();

            // EGL is crossplatform and the official khronos way
            // but sometimes platforms/drivers may not have it, so we use back up options
            // where possible. TODO: check whether we can expose these options as
            // "features", so that users can select the relevant backend they want.

            // try egl and fallback to windows wgl. Windows is the only platform that
            // *requires* window handle to create display.
            #[cfg(target_os = "windows")]
            let preference =
                glutin::display::DisplayApiPreference::WglThenEgl(Some(raw_window_handle));
            // try egl and fallback to x11 glx
            #[cfg(target_os = "linux")]
            let preference = glutin::display::DisplayApiPreference::EglThenGlx(Box::new(
                winit::platform::x11::register_xlib_error_hook,
            ));
            #[cfg(target_os = "macos")]
            let preference = glutin::display::DisplayApiPreference::Cgl;
            #[cfg(target_os = "android")]
            let preference = glutin::display::DisplayApiPreference::Egl;

            let gl_display =
                unsafe { glutin::display::Display::new(raw_display_handle, preference)? };
            let swap_interval = if settings.vsync {
                glutin::surface::SwapInterval::Wait(std::num::NonZeroU32::new(1).unwrap())
            } else {
                glutin::surface::SwapInterval::DontWait
            };

            let hardware_acceleration = match settings.hardware_acceleration {
                crate::HardwareAcceleration::Required => Some(true),
                crate::HardwareAcceleration::Preferred => None,
                crate::HardwareAcceleration::Off => Some(false),
            };
            let config_template = glutin::config::ConfigTemplateBuilder::new()
                .prefer_hardware_accelerated(hardware_acceleration)
                .with_depth_size(settings.depth_buffer);
            // we don't know if multi sampling option is set. so, check if its more than 0.
            let config_template = if settings.multisamples > 0 {
                config_template.with_multisampling(settings.multisamples)
            } else {
                config_template
            };
            let config_template = config_template
                .with_stencil_size(settings.stencil_buffer)
                .compatible_with_native_window(raw_window_handle)
                .build();
            // finds all valid configurations supported by this display that match the
            // config_template this is where we will try to get a "fallback" config if
            // we are okay with ignoring some native options required by user like multi
            // sampling, srgb, transparency etc..
            let config = unsafe {
                gl_display
                    .find_configs(config_template)?
                    .next()
                    .ok_or(WindowError::SurfaceCreationError)?
            };

            let context_attributes =
                glutin::context::ContextAttributesBuilder::new().build(Some(raw_window_handle));
            // for surface creation.
            let (width, height): (u32, u32) = window.inner_size().into();
            let width = std::num::NonZeroU32::new(width.max(1)).unwrap();
            let height = std::num::NonZeroU32::new(height.max(1)).unwrap();
            let surface_attributes =
                glutin::surface::SurfaceAttributesBuilder::<glutin::surface::WindowSurface>::new()
                    .build(raw_window_handle, width, height);
            // start creating the gl objects
            let gl_context = unsafe { gl_display.create_context(&config, &context_attributes)? };

            let gl_surface =
                unsafe { gl_display.create_window_surface(&config, &surface_attributes)? };
            let gl_context = gl_context.make_current(&gl_surface)?;
            gl_surface.set_swap_interval(&gl_context, swap_interval)?;

            Ok(Self {
                context: Context::from_gl_context(Arc::new(unsafe {
                    crate::context::Context::from_loader_function(|s| {
                        let s = std::ffi::CString::new(s)
                            .expect("failed to construct C string from string for gl proc address");

                        gl_display.get_proc_address(&s)
                    })
                }))?,
                gl: Some((gl_surface, gl_context)),
            })
        }

        ///
        /// Creates a new windowed context from a [winit](https://crates.io/crates/winit) window which uses the preferred backend,
        /// see [BackendPreference](crate::BackendPreference). The window must not be dropped before the context.
        ///
        pub fn from_winit_window_with_backend(
            window: &Window,
            settings: SurfaceSettings,
            preference: crate::BackendPreference,
        ) -> Result<Self, WindowError> {
            #[cfg(feature = "wgpu")]
            {
                pollster::block_on(Self::from_winit_window_async(window, settings, preference))
            }
            #[cfg(not(feature = "wgpu"))]
            {
                if preference == crate::BackendPreference::Wgpu {
                    Err(wgpu_context::not_compiled())
                } else {
                    Self::from_winit_window(window, settings)
                }
            }
        }

        ///
        /// The same as [WindowedContext::from_winit_window_with_backend], but asynchronous like the function with the same name for the web,
        /// so the same code can be used on all platforms.
        ///
        #[allow(unsafe_code)]
        pub async fn from_winit_window_async(
            window: &Window,
            settings: SurfaceSettings,
            preference: crate::BackendPreference,
        ) -> Result<Self, WindowError> {
            use crate::BackendPreference;
            if preference == BackendPreference::Gl {
                return Self::from_winit_window(window, settings);
            }
            #[cfg(feature = "wgpu")]
            {
                let instance = ::wgpu::Instance::new(
                    ::wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
                );
                // Safety: the window must outlive the context, which is documented for this function and the same as for OpenGL
                let target = unsafe {
                    ::wgpu::SurfaceTargetUnsafe::from_display_and_window(window, window)?
                };
                let context = wgpu_context::new(
                    instance,
                    |instance| {
                        unsafe { instance.create_surface_unsafe(target) }
                            .map_err(wgpu_context::surface_error)
                    },
                    window.inner_size().into(),
                    settings,
                    preference,
                )
                .await;
                match context {
                    Ok(context) => Ok(Self { context, gl: None }),
                    Err(_) if preference == BackendPreference::Auto => {
                        Self::from_winit_window(window, settings)
                    }
                    Err(error) => Err(error),
                }
            }
            #[cfg(not(feature = "wgpu"))]
            {
                if preference == BackendPreference::Auto {
                    Self::from_winit_window(window, settings)
                } else {
                    Err(wgpu_context::not_compiled())
                }
            }
        }

        /// Resizes the context
        pub fn resize(&self, physical_size: winit::dpi::PhysicalSize<u32>) {
            let Some((surface, glutin_context)) = &self.gl else {
                #[cfg(feature = "wgpu")]
                self.context
                    .wgpu_backend()
                    .resize_screen(physical_size.width, physical_size.height);
                return;
            };
            let width = std::num::NonZeroU32::new(physical_size.width.max(1)).unwrap();
            let height = std::num::NonZeroU32::new(physical_size.height.max(1)).unwrap();
            surface.resize(glutin_context, width, height);
        }

        /// Make this context current. Needed when using multiple windows (contexts) on native.
        pub fn make_current(&self) -> Result<(), WindowError> {
            let Some((surface, glutin_context)) = &self.gl else {
                return Ok(());
            };
            Ok(glutin_context.make_current(surface)?)
        }

        /// Swap buffers - should always be called after rendering.
        pub fn swap_buffers(&self) -> Result<(), WindowError> {
            let Some((surface, glutin_context)) = &self.gl else {
                #[cfg(feature = "wgpu")]
                self.context.wgpu_backend().present()?;
                return Ok(());
            };
            Ok(surface.swap_buffers(glutin_context)?)
        }

        /// Enables or disabled vsync.
        pub fn set_vsync(&self, enabled: bool) -> Result<(), WindowError> {
            let Some((surface, glutin_context)) = &self.gl else {
                #[cfg(feature = "wgpu")]
                self.context.wgpu_backend().set_vsync(enabled);
                return Ok(());
            };
            let swap_interval = if enabled {
                glutin::surface::SwapInterval::Wait(std::num::NonZeroU32::new(1).unwrap())
            } else {
                glutin::surface::SwapInterval::DontWait
            };
            Ok(surface.set_swap_interval(glutin_context, swap_interval)?)
        }
    }
}

pub use inner::*;

impl std::ops::Deref for WindowedContext {
    type Target = Context;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}
