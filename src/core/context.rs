use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

#[doc(hidden)]
pub use crate::context::HasContext;

///
/// Contains the low-level OpenGL/WebGL graphics context as well as other "global" variables.
/// Implements Deref with the low-level graphics context as target, so you can call low-level functionality
/// directly on this struct. Use the [context](crate::context) module to get access to low-level constants and structs.
///
#[derive(Clone)]
pub struct Context {
    backend: Arc<backend::Backend>,
    /// A cache of programs to avoid recompiling a [Program] every frame.
    pub programs: Arc<RwLock<HashMap<Vec<u8>, Program>>>,
}

impl Context {
    ///
    /// Creates a new mid-level context, used in this [core](crate::core) module, from a low-level OpenGL/WebGL context from the [context](crate::context) module.
    /// This should only be called directly if you are creating a low-level context yourself (ie. not using the features in the [window](crate::window) module).
    /// Since the content in the [context](crate::context) module is just a re-export of [glow](https://crates.io/crates/glow),
    /// you can also call this method with a reference counter to a glow context created using glow and not the re-export in [context](crate::context).
    ///
    pub fn from_gl_context(context: Arc<crate::context::Context>) -> Result<Self, CoreError> {
        Ok(Self::from_backend(backend::Backend::Gl(
            backend::gl::GlBackend::new(context)?,
        )))
    }

    pub(crate) fn from_backend(backend: backend::Backend) -> Self {
        Self {
            backend: Arc::new(backend),
            programs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    ///
    /// Returns the low-level OpenGL/WebGL context if this context uses the OpenGL/WebGL backend, otherwise `None`.
    /// Dereferencing a [Context] gives the same low-level context but panics if another backend is used.
    ///
    pub fn as_gl(&self) -> Option<&Arc<crate::context::Context>> {
        match &*self.backend {
            backend::Backend::Gl(gl) => Some(&gl.context),
            #[cfg(any(test, feature = "wgpu"))]
            _ => None,
        }
    }

    ///
    /// Creates a new context which uses the wgpu backend with the given device and queue, for example if another part of the application also uses wgpu.
    ///
    /// The wgpu backend supports the functionality of the [core](crate::core) and [renderer](crate::renderer) modules.
    /// In a browser, pixels can only be read with the asynchronous functions like [RenderTarget::read_color_async].
    /// Functionality that hands out raw OpenGL objects panics with a message. The OpenGL/WebGL backend is not affected by any of this.
    /// A context created with this function has no screen, render into textures instead.
    /// Errors of a device that is given to this function are reported to the error handler of that device, not by [Context::error_check].
    ///
    #[cfg(feature = "wgpu")]
    #[cfg_attr(docsrs, doc(cfg(feature = "wgpu")))]
    pub fn from_wgpu(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self::from_backend(backend::Backend::Wgpu(Box::new(
            backend::wgpu::WgpuBackend::new(device, queue),
        )))
    }

    ///
    /// Creates a new context which uses the wgpu backend without a window, for example for tests or for rendering on a server.
    /// Render into textures by using [Texture2D::as_color_target] and read the result with [RenderTarget::read_color].
    ///
    /// See [Context::from_wgpu] for what is different with the wgpu backend.
    ///
    #[cfg(all(feature = "wgpu", not(target_arch = "wasm32")))]
    #[cfg_attr(docsrs, doc(cfg(feature = "wgpu")))]
    pub fn new_headless_wgpu() -> Result<Self, CoreError> {
        Ok(Self::from_backend(backend::Backend::Wgpu(Box::new(
            backend::wgpu::WgpuBackend::new_headless()?,
        ))))
    }

    ///
    /// Creates a new context which uses the wgpu backend without a window, like [Context::new_headless_wgpu], but with a screen
    /// that is not shown anywhere. This is for applications that show the pixels themselves, for example inside a user interface toolkit.
    ///
    /// Everything that renders to [RenderTarget::screen] works as it does with a window. Read the image with
    /// `RenderTarget::screen(&context, width, height).read_color()`, the first row that is returned is the top row of the image.
    /// The screen has the given size in pixels, a depth buffer if `depth` is set and four samples per pixel if `multisample` is set.
    ///
    /// Use [Context::resize_offscreen_screen] when the size changes and call [Context::submit_offscreen_screen] at the end of a frame that is not read.
    ///
    /// **Note:** The programs in [Context::programs] keep the context and with it the graphics device alive. An application that
    /// creates and drops contexts while it runs should empty that cache before it drops a context: `context.programs.write().unwrap().clear()`.
    ///
    #[cfg(all(feature = "wgpu", not(target_arch = "wasm32")))]
    #[cfg_attr(docsrs, doc(cfg(feature = "wgpu")))]
    pub fn new_headless_wgpu_with_screen(
        width: u32,
        height: u32,
        depth: bool,
        multisample: bool,
    ) -> Result<Self, CoreError> {
        Self::headless_wgpu_with_screen(
            width,
            height,
            depth,
            multisample,
            backend::wgpu::AdapterPreference::Any,
        )
    }

    ///
    /// Creates a new context which uses the OpenGL backend without a window, with a screen that is not shown anywhere.
    /// This is the same as [Context::new_headless_wgpu_with_screen] with OpenGL, for graphics cards that have a good OpenGL driver
    /// but no Vulkan driver wgpu can use. It works on Linux, where EGL can create a context without a window.
    ///
    /// The context is current on the thread that calls this function, and all rendering has to happen on that thread.
    /// A graphics device that renders in hardware is preferred.
    ///
    /// Everything that renders to [RenderTarget::screen] works as it does with a window. Read the image with
    /// `RenderTarget::screen(&context, width, height).read_color()`, the first row that is returned is the top row of the image.
    /// The screen has the given size in pixels, a depth buffer if `depth` is set and four samples per pixel if `multisample` is set.
    /// Use [Context::resize_offscreen_screen] when the size changes.
    ///
    #[cfg(all(feature = "headless-gl", target_os = "linux"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "headless-gl")))]
    pub fn new_headless_gl_with_screen(
        width: u32,
        height: u32,
        depth: bool,
        multisample: bool,
    ) -> Result<Self, CoreError> {
        Self::headless_gl_with_screen(width, height, depth, multisample, false)
    }

    #[cfg(all(feature = "headless-gl", target_os = "linux"))]
    fn headless_gl_with_screen(
        width: u32,
        height: u32,
        depth: bool,
        multisample: bool,
        hardware_only: bool,
    ) -> Result<Self, CoreError> {
        let (backend, _) = backend::gl_headless::new(hardware_only)?;
        backend.set_offscreen_screen(width, height, depth, multisample)?;
        Ok(Self::from_backend(backend::Backend::Gl(backend)))
    }

    #[cfg(all(feature = "wgpu", not(target_arch = "wasm32")))]
    fn headless_wgpu_with_screen(
        width: u32,
        height: u32,
        depth: bool,
        multisample: bool,
        preference: backend::wgpu::AdapterPreference,
    ) -> Result<Self, CoreError> {
        let context = Self::from_backend(backend::Backend::Wgpu(Box::new(
            backend::wgpu::WgpuBackend::new_headless_with(preference)?,
        )));
        context
            .wgpu_backend()
            .set_screen(backend::wgpu::ScreenDesc {
                width,
                height,
                depth,
                multisample,
                vsync: false,
            });
        Ok(context)
    }

    ///
    /// Creates a new context without a window, with a screen that is not shown anywhere, and chooses the backend by what the system has:
    /// 1. wgpu with a hardware adapter of Vulkan, Metal or DirectX 12 (with the `wgpu` feature),
    /// 2. OpenGL with a hardware device (with the `headless-gl` feature, on Linux),
    /// 3. wgpu with a hardware adapter that draws with OpenGL (with the `wgpu-gl` feature),
    /// 4. wgpu with a software adapter,
    /// 5. OpenGL with a software device.
    ///
    /// See [Context::new_headless_wgpu_with_screen] and [Context::new_headless_gl_with_screen] for how to use it,
    /// [Context::as_wgpu] and [Context::as_gl] tell which backend was chosen.
    ///
    #[cfg(any(
        all(feature = "wgpu", not(target_arch = "wasm32")),
        all(feature = "headless-gl", target_os = "linux")
    ))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "wgpu", feature = "headless-gl"))))]
    pub fn new_headless_with_screen(
        width: u32,
        height: u32,
        depth: bool,
        multisample: bool,
    ) -> Result<Self, CoreError> {
        let mut result = Err(CoreError::ContextCreation(
            "no backend without a window is compiled in".to_string(),
        ));
        // 0 and 1: hardware, 2: software
        for step in 0..3 {
            #[cfg(all(feature = "wgpu", not(target_arch = "wasm32")))]
            {
                use backend::wgpu::AdapterPreference;
                let preference = match step {
                    // With the `wgpu-gl` feature, wgpu has adapters that draw with OpenGL. The OpenGL backend does that with less effort
                    0 => AdapterPreference::HardwareNotGl,
                    1 => AdapterPreference::Hardware,
                    _ => AdapterPreference::Any,
                };
                result =
                    Self::headless_wgpu_with_screen(width, height, depth, multisample, preference);
                if result.is_ok() {
                    return result;
                }
            }
            #[cfg(all(feature = "headless-gl", target_os = "linux"))]
            if step != 1 {
                result =
                    Self::headless_gl_with_screen(width, height, depth, multisample, step == 0);
                if result.is_ok() {
                    return result;
                }
            }
        }
        result
    }

    ///
    /// Changes the size of the screen of a context created with [Context::new_headless_wgpu_with_screen],
    /// [Context::new_headless_gl_with_screen] or [Context::new_headless_with_screen].
    /// The content of the screen is lost. Returns an error for every other context, the screen of a window follows the window.
    ///
    #[cfg(any(
        all(feature = "wgpu", not(target_arch = "wasm32")),
        all(feature = "headless-gl", target_os = "linux")
    ))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "wgpu", feature = "headless-gl"))))]
    pub fn resize_offscreen_screen(&self, width: u32, height: u32) -> Result<(), CoreError> {
        match &*self.backend {
            #[cfg(all(feature = "wgpu", not(target_arch = "wasm32")))]
            backend::Backend::Wgpu(wgpu) if wgpu.has_offscreen_screen() => {
                wgpu.resize_screen(width, height);
                Ok(())
            }
            #[cfg(all(feature = "headless-gl", target_os = "linux"))]
            backend::Backend::Gl(gl) if gl.resize_offscreen_screen(width, height)? => Ok(()),
            _ => Err(Self::no_offscreen_screen()),
        }
    }

    ///
    /// Ends a frame of a context with an offscreen screen (see [Context::resize_offscreen_screen]): hands everything that was drawn
    /// since the last call to the graphics card, like a window does when it shows a frame. Reading from the screen does the same,
    /// so this is only necessary for frames that are not read. Returns an error for every other context.
    ///
    #[cfg(any(
        all(feature = "wgpu", not(target_arch = "wasm32")),
        all(feature = "headless-gl", target_os = "linux")
    ))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "wgpu", feature = "headless-gl"))))]
    pub fn submit_offscreen_screen(&self) -> Result<(), CoreError> {
        match &*self.backend {
            #[cfg(all(feature = "wgpu", not(target_arch = "wasm32")))]
            backend::Backend::Wgpu(wgpu) if wgpu.has_offscreen_screen() => wgpu.present(),
            #[cfg(all(feature = "headless-gl", target_os = "linux"))]
            backend::Backend::Gl(gl) if gl.has_offscreen_screen() => {
                use crate::context::HasContext;
                unsafe { gl.context.flush() };
                gl.lost.error()
            }
            _ => Err(Self::no_offscreen_screen()),
        }
    }

    #[cfg(any(
        all(feature = "wgpu", not(target_arch = "wasm32")),
        all(feature = "headless-gl", target_os = "linux")
    ))]
    fn no_offscreen_screen() -> CoreError {
        CoreError::ContextError(
            "the context has no offscreen screen, create it with Context::new_headless_with_screen"
                .to_string(),
        )
    }

    ///
    /// Returns the wgpu device and queue if this context uses the wgpu backend, otherwise `None`.
    ///
    #[cfg(feature = "wgpu")]
    #[cfg_attr(docsrs, doc(cfg(feature = "wgpu")))]
    pub fn as_wgpu(&self) -> Option<(&wgpu::Device, &wgpu::Queue)> {
        match &*self.backend {
            backend::Backend::Wgpu(wgpu) => Some((&wgpu.device, &wgpu.queue)),
            _ => None,
        }
    }

    ///
    /// What the wgpu adapter of this context is, `None` with another backend. In the browser wgpu only passes on the
    /// description as name; the browser's vendor and architecture of the adapter are added as `driver` and
    /// `driver_info` (for example "intel" and "gen-7"), when the context created the device itself.
    ///
    #[cfg(feature = "wgpu")]
    #[cfg_attr(docsrs, doc(cfg(feature = "wgpu")))]
    pub fn wgpu_adapter_info(&self) -> Option<wgpu::AdapterInfo> {
        let backend::Backend::Wgpu(wgpu) = &*self.backend else {
            return None;
        };
        #[allow(unused_mut)]
        let mut info = wgpu.device.adapter_info();
        #[cfg(target_arch = "wasm32")]
        if let Some(browser) = &wgpu.browser {
            info.driver = browser.info("vendor");
            info.driver_info = browser.info("architecture");
            if info.name.is_empty() {
                info.name = browser.info("description");
            }
        }
        Some(info)
    }

    ///
    /// The browser's `GPUDevice` behind the wgpu device of this context, for WebGPU calls wgpu does not have, such as
    /// `createComputePipelineAsync`. `None` with another backend or when the context did not create the device itself.
    ///
    #[cfg(all(feature = "wgpu", target_arch = "wasm32"))]
    #[cfg_attr(docsrs, doc(cfg(all(feature = "wgpu", target_arch = "wasm32"))))]
    pub fn webgpu_device(&self) -> Option<wasm_bindgen::JsValue> {
        match &*self.backend {
            backend::Backend::Wgpu(wgpu) => wgpu.browser.as_ref().map(|b| b.device.clone()),
            _ => None,
        }
    }

    /// The backend which executes all graphics calls of the [core](crate::core) module.
    pub(crate) fn backend(&self) -> &dyn backend::BackendApi {
        self.backend.api()
    }

    /// The GL backend. Used for the functionality that hands raw GL calls to the user.
    pub(crate) fn gl_backend(&self) -> &backend::gl::GlBackend {
        match &*self.backend {
            backend::Backend::Gl(gl) => gl,
            #[cfg(any(test, feature = "wgpu"))]
            _ => panic!("{}", NOT_GL),
        }
    }

    #[cfg(feature = "wgpu")]
    #[allow(dead_code)]
    pub(crate) fn wgpu_backend(&self) -> &backend::wgpu::WgpuBackend {
        match &*self.backend {
            backend::Backend::Wgpu(wgpu) => wgpu,
            _ => panic!("this functionality needs the wgpu backend"),
        }
    }

    /// The number of contexts, including the copies inside of textures, programs and so on, that keep the backend alive.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn backend_references(&self) -> usize {
        Arc::strong_count(&self.backend)
    }

    #[cfg(test)]
    pub(crate) fn null_backend(&self) -> &backend::null::NullBackend {
        match &*self.backend {
            backend::Backend::Null(null) => null,
            _ => panic!("not the null backend"),
        }
    }

    ///
    /// Set the scissor test for this context (see [ScissorBox]).
    ///
    pub fn set_scissor(&self, scissor_box: ScissorBox) {
        self.backend().set_scissor(scissor_box);
    }

    ///
    /// Set the viewport for this context (See [Viewport]).
    ///
    pub fn set_viewport(&self, viewport: Viewport) {
        self.backend().set_viewport(viewport);
    }

    ///
    /// Set the face culling for this context (see [Cull]).
    ///
    pub fn set_cull(&self, cull: Cull) {
        self.backend().set_cull(cull);
    }

    ///
    /// Set the write mask for this context (see [WriteMask]).
    ///
    pub fn set_write_mask(&self, write_mask: WriteMask) {
        self.backend().set_write_mask(write_mask);
    }

    ///
    /// Set the depth test for this context (see [DepthTest]).
    ///
    pub fn set_depth_test(&self, depth_test: DepthTest) {
        self.backend().set_depth_test(depth_test);
    }

    ///
    /// Set the blend state for this context (see [Blend]).
    ///
    pub fn set_blend(&self, blend: Blend) {
        self.backend().set_blend(blend);
    }

    ///
    /// Set the render states for this context (see [RenderStates]).
    ///
    pub fn set_render_states(&self, render_states: RenderStates) {
        self.backend().set_render_states(render_states);
    }

    ///
    /// Returns an error if an GPU-side error has happened while rendering which can be used to check for errors while developing.
    /// Can also be used in production to handle unexpected rendering errors, but do not call it too often to avoid performance problems.
    ///
    pub fn error_check(&self) -> Result<(), CoreError> {
        self.backend().error_check()
    }

    ///
    /// Returns whether the graphics device is lost, for example because the graphics card was reset after a draw or compute call
    /// took too long. What was drawn or computed since may be wrong, and nothing works anymore: create a new context.
    /// Once lost, the context stays lost. [Context::error_check] then returns an error, and functions that read pixels panic
    /// instead of returning pixels that are wrong.
    ///
    /// This is cheap, so it can be called after every frame, and after reading results that were computed with the device directly
    /// (see [Context::as_wgpu]).
    ///
    /// A device is noticed as lost
    /// - with the wgpu backend on Vulkan, Metal and DirectX 12, for devices created by this crate,
    /// - with the wgpu backend on top of OpenGL (the `wgpu-gl` feature, with EGL on Linux and Android) only if the application uses
    ///   the patched wgpu-hal of this repository, see "Lost devices" in `docs/wgpu-backend.md`,
    /// - with the OpenGL backend in a context without a window ([Context::new_headless_gl_with_screen]),
    /// - with every other OpenGL context only if it was created with the reset notification strategy `LOSE_CONTEXT_ON_RESET`
    ///   (`GL_KHR_robustness`), and then only after [Context::error_check] returned the error.
    ///
    /// On OpenGL this needs a driver that supports `EGL_EXT_create_context_robustness` or EGL 1.5 and `GL_KHR_robustness`
    /// or `GL_ARB_robustness`, as Mesa does. With the OpenGL backend, call it on the thread where the context is used.
    ///
    pub fn is_lost(&self) -> bool {
        self.backend().is_lost()
    }
}

const NOT_GL: &str =
    "this functionality needs the OpenGL/WebGL backend, but the context uses another backend";

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("Context");
        d.field("programs", &self.programs.read().unwrap().len());
        d.finish()
    }
}

impl std::ops::Deref for Context {
    type Target = Arc<crate::context::Context>;
    fn deref(&self) -> &Self::Target {
        self.as_gl().expect(NOT_GL)
    }
}
