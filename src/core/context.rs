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
    /// **Note:** The wgpu backend is work in progress. It supports the functionality of the [core](crate::core) module except
    /// reading pixels in the browser, which panics with a message. The OpenGL/WebGL backend is not affected.
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
    /// **Note:** The wgpu backend is work in progress, see [Context::from_wgpu].
    ///
    #[cfg(all(feature = "wgpu", not(target_arch = "wasm32")))]
    #[cfg_attr(docsrs, doc(cfg(feature = "wgpu")))]
    pub fn new_headless_wgpu() -> Result<Self, CoreError> {
        Ok(Self::from_backend(backend::Backend::Wgpu(Box::new(
            backend::wgpu::WgpuBackend::new_headless()?,
        ))))
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
