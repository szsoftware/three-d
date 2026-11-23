//!
//! The OpenGL/WebGL2 backend, using [glow](https://crates.io/crates/glow).
//!
//! The code in here is the GL code that used to live directly in the public types in [core](crate::core),
//! moved as literally as possible so the sequence of GL calls is unchanged.
//!

use super::*;
use crate::context::HasContext;
use std::sync::Arc;

///
/// The GL backend: a glow context and the one vertex array object that is reused for every draw.
///
pub(crate) struct GlBackend {
    pub context: Arc<crate::context::Context>,
    vao: crate::context::VertexArray,
    /// What stands in for the default framebuffer if the context has no window, see [GlBackend::set_offscreen_screen].
    screen: std::sync::Mutex<Option<OffscreenScreen>>,
    /// A framebuffer of the application that is the screen, see [GlBackend::set_external_screen]. It wins over `screen`.
    external: std::sync::Mutex<Option<ExternalScreen>>,
    /// What has to live as long as the context if this backend created it.
    #[allow(dead_code)]
    pub(super) keep_alive: Option<KeepAlive>,
    /// Whether the graphics card was reset, which is only noticed if the context was created for it (see [reset]).
    pub(crate) lost: Arc<reset::DeviceLost>,
}

///
/// The context and display of a context that this crate created. They are only kept, never used, so they can move between
/// threads with the backend. As for every OpenGL context, the calls have to come from the thread where the context is current.
///
pub(crate) struct KeepAlive(#[allow(dead_code)] pub Box<dyn std::any::Any>);

unsafe impl Send for KeepAlive {}
unsafe impl Sync for KeepAlive {}
impl std::panic::UnwindSafe for KeepAlive {}
impl std::panic::RefUnwindSafe for KeepAlive {}

///
/// A framebuffer that is used wherever the screen is asked for.
///
struct OffscreenScreen {
    width: u32,
    height: u32,
    depth: bool,
    multisample: bool,
    framebuffer: crate::context::Framebuffer,
    renderbuffers: Vec<crate::context::Renderbuffer>,
    /// With multisampling, the framebuffer with one sample per pixel that is read from.
    resolved: Option<crate::context::Framebuffer>,
}

///
/// A framebuffer that belongs to the application and is used wherever the screen is asked for.
///
struct ExternalScreen {
    /// The framebuffer of the application, which this crate never deletes.
    framebuffer: crate::context::Framebuffer,
    width: u32,
    height: u32,
    /// If the framebuffer of the application has no depth buffer and one was asked for: a framebuffer of this crate
    /// with the color buffer of the application's framebuffer and a depth buffer.
    depth: Option<AddedDepth>,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct AddedDepth {
    framebuffer: crate::context::Framebuffer,
    renderbuffer: crate::context::Renderbuffer,
    /// The color buffer of the application's framebuffer that is attached, as GL object type and name.
    color: (u32, u32),
}

impl Drop for GlBackend {
    fn drop(&mut self) {
        // The context of the application lives on, so what this crate added to its framebuffer is deleted
        let external = self.external.get_mut().unwrap_or_else(|e| e.into_inner());
        if let Some(added) = external.take().and_then(|external| external.depth) {
            unsafe {
                self.context.delete_framebuffer(added.framebuffer);
                self.context.delete_renderbuffer(added.renderbuffer);
            }
        }
    }
}

impl GlBackend {
    pub fn new(context: Arc<crate::context::Context>) -> Result<Self, CoreError> {
        unsafe {
            if !context.version().is_embedded {
                // Enable seamless cube map textures - not available on OpenGL ES and WebGL
                context.enable(crate::context::TEXTURE_CUBE_MAP_SEAMLESS);
            }
            context.pixel_store_i32(crate::context::UNPACK_ALIGNMENT, 1);
            context.pixel_store_i32(crate::context::PACK_ALIGNMENT, 1);
        };
        let backend = unsafe {
            // Create one Vertex Array Object which is then reused all the time.
            let vao = context
                .create_vertex_array()
                .map_err(CoreError::ContextCreation)?;
            Self {
                context,
                vao,
                screen: std::sync::Mutex::new(None),
                external: std::sync::Mutex::new(None),
                keep_alive: None,
                lost: Default::default(),
            }
        };
        Ok(backend)
    }

    ///
    /// Creates a framebuffer that is used as the screen from now on, for a context without a window.
    /// The former one is deleted with its content.
    ///
    #[allow(dead_code)]
    pub fn set_offscreen_screen(
        &self,
        width: u32,
        height: u32,
        depth: bool,
        multisample: bool,
    ) -> Result<(), CoreError> {
        use crate::context as gl;
        let context = &self.context;
        let (width, height) = (width.max(1), height.max(1));
        let mut renderbuffers = Vec::new();
        let framebuffer = |samples: i32,
                           renderbuffers: &mut Vec<gl::Renderbuffer>|
         -> Result<gl::Framebuffer, CoreError> {
            unsafe {
                let framebuffer = context
                    .create_framebuffer()
                    .map_err(CoreError::ContextCreation)?;
                context.bind_framebuffer(gl::FRAMEBUFFER, Some(framebuffer));
                let mut attach = |attachment, format| -> Result<(), CoreError> {
                    let renderbuffer = context
                        .create_renderbuffer()
                        .map_err(CoreError::ContextCreation)?;
                    context.bind_renderbuffer(gl::RENDERBUFFER, Some(renderbuffer));
                    context.renderbuffer_storage_multisample(
                        gl::RENDERBUFFER,
                        samples,
                        format,
                        width as i32,
                        height as i32,
                    );
                    context.framebuffer_renderbuffer(
                        gl::FRAMEBUFFER,
                        attachment,
                        gl::RENDERBUFFER,
                        Some(renderbuffer),
                    );
                    renderbuffers.push(renderbuffer);
                    Ok(())
                };
                attach(gl::COLOR_ATTACHMENT0, gl::RGBA8)?;
                if depth {
                    attach(gl::DEPTH_ATTACHMENT, gl::DEPTH_COMPONENT32F)?;
                }
                context.bind_renderbuffer(gl::RENDERBUFFER, None);
                context.draw_buffers(&[gl::COLOR_ATTACHMENT0]);
                context.read_buffer(gl::COLOR_ATTACHMENT0);
                let status = context.check_framebuffer_status(gl::FRAMEBUFFER);
                if status != gl::FRAMEBUFFER_COMPLETE {
                    return Err(CoreError::ContextCreation(format!(
                        "the framebuffer of the offscreen screen is not complete (status {status:#x})"
                    )));
                }
                Ok(framebuffer)
            }
        };
        let created = (|| {
            let main = framebuffer(if multisample { 4 } else { 0 }, &mut renderbuffers)?;
            let resolved = if multisample {
                Some(framebuffer(0, &mut renderbuffers)?)
            } else {
                None
            };
            Ok((main, resolved))
        })();
        let former = {
            let mut screen = self.screen.lock().unwrap_or_else(|e| e.into_inner());
            match created {
                Ok((framebuffer, resolved)) => screen.replace(OffscreenScreen {
                    width,
                    height,
                    depth,
                    multisample,
                    framebuffer,
                    renderbuffers,
                    resolved,
                }),
                Err(error) => {
                    unsafe {
                        context.bind_framebuffer(gl::FRAMEBUFFER, None);
                        for renderbuffer in renderbuffers {
                            context.delete_renderbuffer(renderbuffer);
                        }
                    }
                    return Err(error);
                }
            }
        };
        unsafe {
            if let Some(former) = former {
                context.delete_framebuffer(former.framebuffer);
                if let Some(resolved) = former.resolved {
                    context.delete_framebuffer(resolved);
                }
                for renderbuffer in former.renderbuffers {
                    context.delete_renderbuffer(renderbuffer);
                }
            }
            context.bind_framebuffer(gl::FRAMEBUFFER, self.screen_framebuffer());
        }
        Ok(())
    }

    ///
    /// Changes the size of the offscreen screen. Returns whether there is one.
    ///
    #[allow(dead_code)]
    pub fn resize_offscreen_screen(&self, width: u32, height: u32) -> Result<bool, CoreError> {
        let settings = {
            let screen = self.screen.lock().unwrap_or_else(|e| e.into_inner());
            screen
                .as_ref()
                .map(|s| (s.width, s.height, s.depth, s.multisample))
        };
        match settings {
            Some((old_width, old_height, depth, multisample)) => {
                if (old_width, old_height) != (width.max(1), height.max(1)) {
                    self.set_offscreen_screen(width, height, depth, multisample)?;
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    #[allow(dead_code)]
    pub fn has_offscreen_screen(&self) -> bool {
        self.screen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    ///
    /// Makes a framebuffer of the application the screen, or with `None` the screen that was used before.
    /// See [Context::set_screen_framebuffer](crate::core::Context::set_screen_framebuffer).
    ///
    pub fn set_external_screen(
        &self,
        external: Option<(crate::context::Framebuffer, u32, u32, bool)>,
    ) -> Result<(), CoreError> {
        let result = self.replace_external_screen(external);
        // After an error, the screen is the one without the application's framebuffer
        unsafe {
            self.context
                .bind_framebuffer(crate::context::FRAMEBUFFER, self.screen_framebuffer());
        }
        result
    }

    fn replace_external_screen(
        &self,
        external: Option<(crate::context::Framebuffer, u32, u32, bool)>,
    ) -> Result<(), CoreError> {
        use crate::context as gl;
        let context = &self.context;
        let mut screen = self.external.lock().unwrap_or_else(|e| e.into_inner());
        let former = screen.take();
        let delete = |added: AddedDepth| unsafe {
            context.delete_framebuffer(added.framebuffer);
            context.delete_renderbuffer(added.renderbuffer);
        };
        let Some((framebuffer, width, height, depth)) = external else {
            if let Some(added) = former.and_then(|former| former.depth) {
                delete(added);
            }
            return Ok(());
        };
        let (width, height) = (width.max(1), height.max(1));
        let attachment = |attachment: u32, parameter: u32| unsafe {
            context.get_framebuffer_attachment_parameter_i32(gl::FRAMEBUFFER, attachment, parameter)
        };
        unsafe { context.bind_framebuffer(gl::FRAMEBUFFER, Some(framebuffer)) };
        let has_depth = attachment(gl::DEPTH_ATTACHMENT, gl::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE)
            as u32
            != gl::NONE;
        let former_size = former.as_ref().map(|former| (former.width, former.height));
        let added = former.and_then(|former| former.depth);
        if !depth || has_depth {
            if let Some(added) = added {
                delete(added);
            }
            *screen = Some(ExternalScreen {
                framebuffer,
                width,
                height,
                depth: None,
            });
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (added, former_size);
            Err(CoreError::ContextError(
                "WebGL cannot add a depth buffer to a framebuffer without one, attach a depth buffer to it"
                    .to_string(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        unsafe {
            let no_color = || {
                CoreError::ContextError(
                    "the screen framebuffer has no color buffer at GL_COLOR_ATTACHMENT0, which is needed to add a depth buffer"
                        .to_string(),
                )
            };
            let kind = attachment(
                gl::COLOR_ATTACHMENT0,
                gl::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE,
            ) as u32;
            let name = attachment(
                gl::COLOR_ATTACHMENT0,
                gl::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME,
            ) as u32;
            let name = match std::num::NonZeroU32::new(name) {
                Some(name) if kind == gl::TEXTURE || kind == gl::RENDERBUFFER => name,
                _ => {
                    if let Some(added) = added {
                        delete(added);
                    }
                    return Err(no_color());
                }
            };
            let (level, face) = if kind == gl::TEXTURE {
                (
                    attachment(
                        gl::COLOR_ATTACHMENT0,
                        gl::FRAMEBUFFER_ATTACHMENT_TEXTURE_LEVEL,
                    ),
                    attachment(
                        gl::COLOR_ATTACHMENT0,
                        gl::FRAMEBUFFER_ATTACHMENT_TEXTURE_CUBE_MAP_FACE,
                    ) as u32,
                )
            } else {
                (0, 0)
            };
            let resized = former_size != Some((width, height));
            let mut added = match added {
                // Nothing changed: the framebuffer of this crate can be used as it is
                Some(added) if !resized && added.color == (kind, name.get()) => {
                    *screen = Some(ExternalScreen {
                        framebuffer,
                        width,
                        height,
                        depth: Some(added),
                    });
                    return Ok(());
                }
                Some(added) => added,
                None => AddedDepth {
                    framebuffer: context
                        .create_framebuffer()
                        .map_err(CoreError::ContextCreation)?,
                    renderbuffer: match context.create_renderbuffer() {
                        Ok(renderbuffer) => renderbuffer,
                        Err(e) => return Err(CoreError::ContextCreation(e)),
                    },
                    color: (0, 0),
                },
            };
            if resized || added.color == (0, 0) {
                context.bind_renderbuffer(gl::RENDERBUFFER, Some(added.renderbuffer));
                context.renderbuffer_storage(
                    gl::RENDERBUFFER,
                    gl::DEPTH_COMPONENT32F,
                    width as i32,
                    height as i32,
                );
                context.bind_renderbuffer(gl::RENDERBUFFER, None);
            }
            context.bind_framebuffer(gl::FRAMEBUFFER, Some(added.framebuffer));
            if kind == gl::TEXTURE {
                context.framebuffer_texture_2d(
                    gl::FRAMEBUFFER,
                    gl::COLOR_ATTACHMENT0,
                    if face != 0 { face } else { gl::TEXTURE_2D },
                    Some(crate::context::NativeTexture(name)),
                    level,
                );
            } else {
                context.framebuffer_renderbuffer(
                    gl::FRAMEBUFFER,
                    gl::COLOR_ATTACHMENT0,
                    gl::RENDERBUFFER,
                    Some(crate::context::NativeRenderbuffer(name)),
                );
            }
            context.framebuffer_renderbuffer(
                gl::FRAMEBUFFER,
                gl::DEPTH_ATTACHMENT,
                gl::RENDERBUFFER,
                Some(added.renderbuffer),
            );
            context.draw_buffers(&[gl::COLOR_ATTACHMENT0]);
            context.read_buffer(gl::COLOR_ATTACHMENT0);
            let status = context.check_framebuffer_status(gl::FRAMEBUFFER);
            if status != gl::FRAMEBUFFER_COMPLETE {
                delete(added);
                return Err(CoreError::ContextError(format!(
                    "the screen framebuffer with an added depth buffer is not complete (status {status:#x}), \
                     its color buffer has to be a 2D texture or a renderbuffer"
                )));
            }
            added.color = (kind, name.get());
            *screen = Some(ExternalScreen {
                framebuffer,
                width,
                height,
                depth: Some(added),
            });
            Ok(())
        }
    }

    ///
    /// Sets the OpenGL state that this crate expects but does not set before each draw call, after other code changed it.
    /// See [Context::reset_gl_state](crate::core::Context::reset_gl_state).
    ///
    pub fn reset_state(&self) {
        use crate::context as gl;
        let context = &self.context;
        let embedded = context.version().is_embedded;
        unsafe {
            context.bind_framebuffer(gl::FRAMEBUFFER, self.screen_framebuffer());
            // Pixel transfer as set when the backend was created, from and to memory
            for (parameter, value) in [
                (gl::UNPACK_ALIGNMENT, 1),
                (gl::UNPACK_ROW_LENGTH, 0),
                (gl::UNPACK_IMAGE_HEIGHT, 0),
                (gl::UNPACK_SKIP_PIXELS, 0),
                (gl::UNPACK_SKIP_ROWS, 0),
                (gl::UNPACK_SKIP_IMAGES, 0),
                (gl::PACK_ALIGNMENT, 1),
                (gl::PACK_ROW_LENGTH, 0),
                (gl::PACK_SKIP_PIXELS, 0),
                (gl::PACK_SKIP_ROWS, 0),
            ] {
                context.pixel_store_i32(parameter, value);
            }
            for target in [
                gl::PIXEL_UNPACK_BUFFER,
                gl::PIXEL_PACK_BUFFER,
                gl::ARRAY_BUFFER,
                gl::UNIFORM_BUFFER,
            ] {
                context.bind_buffer(target, None);
            }
            context.bind_vertex_array(None);
            context.use_program(None);
            // A sampler object bound to a texture unit overrides the parameters this crate sets on its textures
            let units = context.get_parameter_i32(gl::MAX_COMBINED_TEXTURE_IMAGE_UNITS);
            for unit in 0..units.max(0) as u32 {
                context.bind_sampler(unit, None);
            }
            context.active_texture(gl::TEXTURE0);
            for capability in [
                gl::STENCIL_TEST,
                gl::POLYGON_OFFSET_FILL,
                gl::SAMPLE_ALPHA_TO_COVERAGE,
                gl::SAMPLE_COVERAGE,
                gl::RASTERIZER_DISCARD,
            ] {
                context.disable(capability);
            }
            context.enable(gl::DITHER);
            context.front_face(gl::CCW);
            context.depth_range_f32(0.0, 1.0);
            context.blend_color(0.0, 0.0, 0.0, 0.0);
            context.stencil_mask(!0);
            #[cfg(not(target_arch = "wasm32"))]
            context.disable(gl::PRIMITIVE_RESTART_FIXED_INDEX);
            if !embedded {
                context.disable(gl::PRIMITIVE_RESTART);
                context.disable(gl::COLOR_LOGIC_OP);
                context.disable(gl::FRAMEBUFFER_SRGB);
                context.polygon_mode(gl::FRONT_AND_BACK, gl::FILL);
                context.enable(gl::TEXTURE_CUBE_MAP_SEAMLESS);
            }
        }
    }

    /// The framebuffer that is the screen: the application's framebuffer if there is one, otherwise the offscreen screen if there is one,
    /// otherwise the default framebuffer.
    fn screen_framebuffer(&self) -> Option<crate::context::Framebuffer> {
        if let Some(external) = self
            .external
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return Some(
                external
                    .depth
                    .as_ref()
                    .map_or(external.framebuffer, |added| added.framebuffer),
            );
        }
        self.screen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|screen| screen.framebuffer)
    }

    fn framebuffer(&self, target: &PassTarget) -> Option<crate::context::Framebuffer> {
        match target.framebuffer {
            Some(framebuffer) => Some(framebuffer.gl()),
            None => self.screen_framebuffer(),
        }
    }

    ///
    /// A multisampled offscreen screen cannot be read from directly. This copies it to its framebuffer with one sample per pixel
    /// and binds that one for reading.
    ///
    fn resolve_offscreen_screen(&self, source: &PassTarget) {
        if source.framebuffer.is_some()
            || self
                .external
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
        {
            return;
        }
        let screen = self.screen.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((screen, resolved)) = screen
            .as_ref()
            .and_then(|screen| Some((screen, screen.resolved?)))
        {
            use crate::context as gl;
            let (width, height) = (screen.width as i32, screen.height as i32);
            self.set_scissor(ScissorBox::new_at_origo(screen.width, screen.height));
            unsafe {
                self.context
                    .bind_framebuffer(gl::READ_FRAMEBUFFER, Some(screen.framebuffer));
                self.context
                    .bind_framebuffer(gl::DRAW_FRAMEBUFFER, Some(resolved));
                let mut mask = gl::COLOR_BUFFER_BIT;
                if screen.depth {
                    mask |= gl::DEPTH_BUFFER_BIT;
                }
                self.context.blit_framebuffer(
                    0,
                    0,
                    width,
                    height,
                    0,
                    0,
                    width,
                    height,
                    mask,
                    gl::NEAREST,
                );
                self.context
                    .bind_framebuffer(gl::DRAW_FRAMEBUFFER, Some(screen.framebuffer));
                self.context
                    .bind_framebuffer(gl::READ_FRAMEBUFFER, Some(resolved));
            }
        }
    }

    ///
    /// Sets up the draw state for the program, calls `draw` and cleans up afterwards.
    /// This is the implementation of [Program::draw_with], which hands raw GL calls to the user.
    ///
    pub fn draw_with(
        &self,
        program: ProgramId,
        attribute_locations: &[u32],
        render_states: RenderStates,
        viewport: Viewport,
        draw: &mut dyn FnMut(),
    ) {
        self.set_viewport(viewport);
        self.set_render_states(render_states);
        self.use_program(program);

        draw();

        unsafe {
            for location in attribute_locations {
                self.context.disable_vertex_attrib_array(*location);
            }
            self.context.bind_vertex_array(None);
        }

        self.unuse_program();

        #[cfg(debug_assertions)]
        self.error_check()
            .expect("Unexpected rendering error occured")
    }

    fn use_program(&self, program: ProgramId) {
        unsafe {
            self.context.use_program(Some(program.gl()));
        }
    }

    fn unuse_program(&self) {
        unsafe {
            self.context.use_program(None);
        }
    }

    fn bind_buffer(&self, kind: BufferKind, buffer: BufferId) {
        unsafe {
            self.context
                .bind_buffer(buffer_target(kind), Some(buffer.gl()));
        }
    }

    fn bind_texture_object(&self, texture: TextureId, kind: TextureKind) {
        unsafe {
            self.context
                .bind_texture(texture_target(kind), Some(texture.gl()));
        }
    }

    fn bind_target(&self, target: &PassTarget, binding: u32) {
        unsafe {
            self.context
                .bind_framebuffer(binding, self.framebuffer(target));
        }
        if let Some(ref color) = target.color {
            self.attach_color(color);
        }
        if let Some(ref depth) = target.depth {
            self.attach_depth(depth);
        }
    }

    fn attach_color(&self, color: &ColorAttachment) {
        unsafe {
            match color {
                ColorAttachment::Texture2D { texture, mip_level } => {
                    self.context
                        .draw_buffers(&[crate::context::COLOR_ATTACHMENT0]);
                    self.context.framebuffer_texture_2d(
                        crate::context::FRAMEBUFFER,
                        crate::context::COLOR_ATTACHMENT0,
                        crate::context::TEXTURE_2D,
                        Some(texture.gl()),
                        *mip_level as i32,
                    );
                }
                ColorAttachment::Texture2DArray {
                    texture,
                    layers,
                    mip_level,
                } => {
                    self.context.draw_buffers(
                        &(0..layers.len())
                            .map(|i| crate::context::COLOR_ATTACHMENT0 + i as u32)
                            .collect::<Vec<u32>>(),
                    );
                    (0..layers.len()).for_each(|channel| {
                        self.context.framebuffer_texture_layer(
                            crate::context::DRAW_FRAMEBUFFER,
                            crate::context::COLOR_ATTACHMENT0 + channel as u32,
                            Some(texture.gl()),
                            *mip_level as i32,
                            layers[channel] as i32,
                        );
                    });
                }
                ColorAttachment::CubeMap {
                    texture,
                    sides,
                    mip_level,
                } => {
                    self.context.draw_buffers(
                        &(0..sides.len())
                            .map(|i| crate::context::COLOR_ATTACHMENT0 + i as u32)
                            .collect::<Vec<u32>>(),
                    );
                    (0..sides.len()).for_each(|channel| {
                        self.context.framebuffer_texture_2d(
                            crate::context::DRAW_FRAMEBUFFER,
                            crate::context::COLOR_ATTACHMENT0 + channel as u32,
                            cube_map_side_target(sides[channel]),
                            Some(texture.gl()),
                            *mip_level as i32,
                        );
                    });
                }
                ColorAttachment::Multisample(renderbuffer) => {
                    self.context
                        .draw_buffers(&[crate::context::COLOR_ATTACHMENT0]);
                    self.context.framebuffer_renderbuffer(
                        crate::context::FRAMEBUFFER,
                        crate::context::COLOR_ATTACHMENT0,
                        crate::context::RENDERBUFFER,
                        Some(renderbuffer.gl()),
                    );
                }
            }
        }
    }

    fn attach_depth(&self, depth: &DepthAttachment) {
        unsafe {
            match depth {
                DepthAttachment::Texture2D(texture) => {
                    self.context.framebuffer_texture_2d(
                        crate::context::FRAMEBUFFER,
                        crate::context::DEPTH_ATTACHMENT,
                        crate::context::TEXTURE_2D,
                        Some(texture.gl()),
                        0,
                    );
                }
                DepthAttachment::Texture2DArray { texture, layer } => {
                    self.context.framebuffer_texture_layer(
                        crate::context::DRAW_FRAMEBUFFER,
                        crate::context::DEPTH_ATTACHMENT,
                        Some(texture.gl()),
                        0,
                        *layer as i32,
                    );
                }
                DepthAttachment::CubeMap { texture, side } => {
                    self.context.framebuffer_texture_2d(
                        crate::context::DRAW_FRAMEBUFFER,
                        crate::context::DEPTH_ATTACHMENT,
                        cube_map_side_target(*side),
                        Some(texture.gl()),
                        0,
                    );
                }
                DepthAttachment::Multisample(renderbuffer) => {
                    self.context.framebuffer_renderbuffer(
                        crate::context::FRAMEBUFFER,
                        crate::context::DEPTH_ATTACHMENT,
                        crate::context::RENDERBUFFER,
                        Some(renderbuffer.gl()),
                    );
                }
            }
        }
    }

    fn set_texture_parameters(&self, target: u32, sampler: &SamplerDesc) {
        let context = &self.context;
        let SamplerDesc {
            min_filter,
            mag_filter,
            mipmap,
            wrap_s,
            wrap_t,
            wrap_r,
        } = *sampler;
        unsafe {
            match mipmap {
                None => context.tex_parameter_i32(
                    target,
                    crate::context::TEXTURE_MIN_FILTER,
                    interpolation_from(min_filter),
                ),
                Some(Mipmap {
                    filter: Interpolation::Nearest,
                    ..
                }) => {
                    if min_filter == Interpolation::Nearest {
                        context.tex_parameter_i32(
                            target,
                            crate::context::TEXTURE_MIN_FILTER,
                            crate::context::NEAREST_MIPMAP_NEAREST as i32,
                        );
                    } else {
                        context.tex_parameter_i32(
                            target,
                            crate::context::TEXTURE_MIN_FILTER,
                            crate::context::LINEAR_MIPMAP_NEAREST as i32,
                        )
                    }
                }
                Some(Mipmap {
                    filter: Interpolation::Linear,
                    ..
                }) => {
                    if min_filter == Interpolation::Nearest {
                        context.tex_parameter_i32(
                            target,
                            crate::context::TEXTURE_MIN_FILTER,
                            crate::context::NEAREST_MIPMAP_LINEAR as i32,
                        );
                    } else {
                        context.tex_parameter_i32(
                            target,
                            crate::context::TEXTURE_MIN_FILTER,
                            crate::context::LINEAR_MIPMAP_LINEAR as i32,
                        )
                    }
                }
                _ => panic!("Can only sample textures using 'NEAREST' or 'LINEAR' interpolation"),
            }
            if let Some(Mipmap { max_ratio, .. }) = mipmap {
                let extensions = context.supported_extensions();
                // Desktop
                if extensions.contains("GL_ARB_texture_filter_anisotropic") ||
                    extensions.contains("GL_EXT_texture_filter_anisotropic") ||
                    // Web
                    extensions.contains("EXT_texture_filter_anisotropic")
                {
                    let max_ratio = max_ratio.min(
                        context.get_parameter_i32(crate::context::MAX_TEXTURE_MAX_ANISOTROPY_EXT)
                            as u32,
                    );
                    context.tex_parameter_i32(
                        target,
                        crate::context::TEXTURE_MAX_ANISOTROPY_EXT,
                        max_ratio as i32,
                    );
                }
            }
            context.tex_parameter_i32(
                target,
                crate::context::TEXTURE_MAG_FILTER,
                interpolation_from(mag_filter),
            );
            context.tex_parameter_i32(
                target,
                crate::context::TEXTURE_WRAP_S,
                wrapping_from(wrap_s),
            );
            context.tex_parameter_i32(
                target,
                crate::context::TEXTURE_WRAP_T,
                wrapping_from(wrap_t),
            );
            if let Some(r) = wrap_r {
                context.tex_parameter_i32(target, crate::context::TEXTURE_WRAP_R, wrapping_from(r));
            }
        }
    }

    fn blend_const_from_multiplier(multiplier: BlendMultiplierType) -> u32 {
        match multiplier {
            BlendMultiplierType::Zero => crate::context::ZERO,
            BlendMultiplierType::One => crate::context::ONE,
            BlendMultiplierType::SrcColor => crate::context::SRC_COLOR,
            BlendMultiplierType::OneMinusSrcColor => crate::context::ONE_MINUS_SRC_COLOR,
            BlendMultiplierType::DstColor => crate::context::DST_COLOR,
            BlendMultiplierType::OneMinusDstColor => crate::context::ONE_MINUS_DST_COLOR,
            BlendMultiplierType::SrcAlpha => crate::context::SRC_ALPHA,
            BlendMultiplierType::OneMinusSrcAlpha => crate::context::ONE_MINUS_SRC_ALPHA,
            BlendMultiplierType::DstAlpha => crate::context::DST_ALPHA,
            BlendMultiplierType::OneMinusDstAlpha => crate::context::ONE_MINUS_DST_ALPHA,
            BlendMultiplierType::SrcAlphaSaturate => crate::context::SRC_ALPHA_SATURATE,
        }
    }

    fn blend_const_from_equation(equation: BlendEquationType) -> u32 {
        match equation {
            BlendEquationType::Add => crate::context::FUNC_ADD,
            BlendEquationType::Subtract => crate::context::FUNC_SUBTRACT,
            BlendEquationType::ReverseSubtract => crate::context::FUNC_REVERSE_SUBTRACT,
            BlendEquationType::Min => crate::context::MIN,
            BlendEquationType::Max => crate::context::MAX,
        }
    }

    fn framebuffer_check(&self) -> Result<(), CoreError> {
        unsafe {
            match self
                .context
                .check_framebuffer_status(crate::context::FRAMEBUFFER)
            {
                crate::context::FRAMEBUFFER_COMPLETE => Ok(()),
                crate::context::FRAMEBUFFER_INCOMPLETE_ATTACHMENT => Err(CoreError::ContextError(
                    "FRAMEBUFFER_INCOMPLETE_ATTACHMENT".to_string(),
                )),
                crate::context::FRAMEBUFFER_INCOMPLETE_DRAW_BUFFER => Err(CoreError::ContextError(
                    "FRAMEBUFFER_INCOMPLETE_DRAW_BUFFER".to_string(),
                )),
                crate::context::FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT => {
                    Err(CoreError::ContextError(
                        "FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT".to_string(),
                    ))
                }
                crate::context::FRAMEBUFFER_UNSUPPORTED => Err(CoreError::ContextError(
                    "FRAMEBUFFER_UNSUPPORTED".to_string(),
                )),
                crate::context::FRAMEBUFFER_UNDEFINED => {
                    Err(CoreError::ContextError("FRAMEBUFFER_UNDEFINED".to_string()))
                }
                crate::context::FRAMEBUFFER_INCOMPLETE_READ_BUFFER => Err(CoreError::ContextError(
                    "FRAMEBUFFER_INCOMPLETE_READ_BUFFER".to_string(),
                )),
                crate::context::FRAMEBUFFER_INCOMPLETE_MULTISAMPLE => Err(CoreError::ContextError(
                    "FRAMEBUFFER_INCOMPLETE_MULTISAMPLE".to_string(),
                )),
                crate::context::FRAMEBUFFER_INCOMPLETE_LAYER_TARGETS => Err(
                    CoreError::ContextError("FRAMEBUFFER_INCOMPLETE_LAYER_TARGETS".to_string()),
                ),
                _ => Err(CoreError::ContextError(
                    "Unknown framebuffer error".to_string(),
                )),
            }?;
        }
        Ok(())
    }
}

/// The number of bytes of the pixels in the rectangle.
fn read_size(rect: ScissorBox, format: ReadFormat) -> usize {
    let pixel_size = match format {
        ReadFormat::Color(format) => format.channels as usize * primitive_size(format.primitive),
        ReadFormat::Depth => 4,
    };
    rect.width as usize * rect.height as usize * pixel_size
}

///
/// Pixels that are on their way from the graphics card into memory, see [GlBackend::read_pixels_async].
/// Must be polled and dropped on the thread where the context is current.
///
#[cfg(not(target_arch = "wasm32"))]
struct PendingRead {
    context: Arc<crate::context::Context>,
    lost: Arc<reset::DeviceLost>,
    /// `None` when the pixels have been handed out.
    resources: Option<(crate::context::Buffer, crate::context::Fence)>,
    /// Where the pixels go.
    storage: Option<Box<dyn PixelStorage>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::future::Future for PendingRead {
    type Output = Box<dyn PixelStorage>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        task: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let (buffer, fence) = self
            .resources
            .expect("the pixels have already been handed out");
        let context = self.context.clone();
        unsafe {
            let status = context.client_wait_sync(fence, 0, 0);
            if status == crate::context::TIMEOUT_EXPIRED {
                // Nothing announces the fence, so the future asks to be polled again.
                // The thread makes room first, for when it is all the executor does (as in a blocking wait).
                std::thread::yield_now();
                task.waker().wake_by_ref();
                return std::task::Poll::Pending;
            }
            // Also if waiting failed: reading the buffer then waits for the card itself
            let mut storage = self
                .storage
                .take()
                .expect("the pixels have already been handed out");
            let bytes = storage.bytes();
            context.bind_buffer(crate::context::PIXEL_PACK_BUFFER, Some(buffer));
            let mapped = context.map_buffer_range(
                crate::context::PIXEL_PACK_BUFFER,
                0,
                bytes.len() as i32,
                crate::context::MAP_READ_BIT,
            );
            if !mapped.is_null() {
                std::ptr::copy_nonoverlapping(mapped, bytes.as_mut_ptr(), bytes.len());
            }
            context.unmap_buffer(crate::context::PIXEL_PACK_BUFFER);
            context.bind_buffer(crate::context::PIXEL_PACK_BUFFER, None);
            self.resources = None;
            context.delete_buffer(buffer);
            context.delete_sync(fence);
            // A lost context does nothing and reports every fence as signaled
            self.lost.panic_if_lost();
            std::task::Poll::Ready(storage)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for PendingRead {
    fn drop(&mut self) {
        if let Some((buffer, fence)) = self.resources.take() {
            unsafe {
                self.context.delete_buffer(buffer);
                self.context.delete_sync(fence);
            }
        }
    }
}

impl GlBackend {
    /// Reads the pixels in the rectangle to memory or to the buffer that is bound for it.
    fn read_pixels_to(
        &self,
        source: &PassTarget,
        rect: ScissorBox,
        format: ReadFormat,
        destination: crate::context::PixelPackData,
    ) {
        self.bind_target(source, crate::context::DRAW_FRAMEBUFFER);
        self.bind_target(source, crate::context::READ_FRAMEBUFFER);
        self.resolve_offscreen_screen(source);
        let (gl_format, gl_data_type) = match format {
            ReadFormat::Color(format) => (
                format_from_channels(format.channels),
                data_type(format.primitive),
            ),
            ReadFormat::Depth => (crate::context::DEPTH_COMPONENT, crate::context::FLOAT),
        };
        unsafe {
            self.context.read_pixels(
                rect.x,
                rect.y,
                rect.width as i32,
                rect.height as i32,
                gl_format,
                gl_data_type,
                destination,
            );
        }
        // Pixels of a context that is lost are not what was drawn
        self.lost.panic_if_lost();
    }
}

impl BackendApi for GlBackend {
    fn set_viewport(&self, viewport: Viewport) {
        unsafe {
            self.context.viewport(
                viewport.x,
                viewport.y,
                viewport.width as i32,
                viewport.height as i32,
            );
        }
    }

    fn set_scissor(&self, scissor_box: ScissorBox) {
        unsafe {
            if scissor_box.width > 0 && scissor_box.height > 0 {
                self.context.enable(crate::context::SCISSOR_TEST);
                self.context.scissor(
                    scissor_box.x,
                    scissor_box.y,
                    scissor_box.width as i32,
                    scissor_box.height as i32,
                );
            } else {
                self.context.disable(crate::context::SCISSOR_TEST);
            }
        }
    }

    fn set_cull(&self, cull: Cull) {
        unsafe {
            match cull {
                Cull::None => {
                    self.context.disable(crate::context::CULL_FACE);
                }
                Cull::Back => {
                    self.context.enable(crate::context::CULL_FACE);
                    self.context.cull_face(crate::context::BACK);
                }
                Cull::Front => {
                    self.context.enable(crate::context::CULL_FACE);
                    self.context.cull_face(crate::context::FRONT);
                }
                Cull::FrontAndBack => {
                    self.context.enable(crate::context::CULL_FACE);
                    self.context.cull_face(crate::context::FRONT_AND_BACK);
                }
            }
        }
    }

    fn set_write_mask(&self, write_mask: WriteMask) {
        unsafe {
            self.context.color_mask(
                write_mask.red,
                write_mask.green,
                write_mask.blue,
                write_mask.alpha,
            );
            self.context.depth_mask(write_mask.depth);
        }
    }

    fn set_depth_test(&self, depth_test: DepthTest) {
        unsafe {
            self.context.enable(crate::context::DEPTH_TEST);
            match depth_test {
                DepthTest::Never => {
                    self.context.depth_func(crate::context::NEVER);
                }
                DepthTest::Less => {
                    self.context.depth_func(crate::context::LESS);
                }
                DepthTest::Equal => {
                    self.context.depth_func(crate::context::EQUAL);
                }
                DepthTest::LessOrEqual => {
                    self.context.depth_func(crate::context::LEQUAL);
                }
                DepthTest::Greater => {
                    self.context.depth_func(crate::context::GREATER);
                }
                DepthTest::NotEqual => {
                    self.context.depth_func(crate::context::NOTEQUAL);
                }
                DepthTest::GreaterOrEqual => {
                    self.context.depth_func(crate::context::GEQUAL);
                }
                DepthTest::Always => {
                    self.context.depth_func(crate::context::ALWAYS);
                }
            }
        }
    }

    fn set_blend(&self, blend: Blend) {
        unsafe {
            if let Blend::Enabled {
                source_rgb_multiplier,
                source_alpha_multiplier,
                destination_rgb_multiplier,
                destination_alpha_multiplier,
                rgb_equation,
                alpha_equation,
            } = blend
            {
                self.context.enable(crate::context::BLEND);
                self.context.blend_func_separate(
                    Self::blend_const_from_multiplier(source_rgb_multiplier),
                    Self::blend_const_from_multiplier(destination_rgb_multiplier),
                    Self::blend_const_from_multiplier(source_alpha_multiplier),
                    Self::blend_const_from_multiplier(destination_alpha_multiplier),
                );
                self.context.blend_equation_separate(
                    Self::blend_const_from_equation(rgb_equation),
                    Self::blend_const_from_equation(alpha_equation),
                );
            } else {
                self.context.disable(crate::context::BLEND);
            }
        }
    }

    fn set_render_states(&self, render_states: RenderStates) {
        self.set_cull(render_states.cull);
        self.set_write_mask(render_states.write_mask);
        if !render_states.write_mask.depth && render_states.depth_test == DepthTest::Always {
            unsafe { self.context.disable(crate::context::DEPTH_TEST) }
        } else {
            self.set_depth_test(render_states.depth_test);
        }
        self.set_blend(render_states.blend);
    }

    fn error_check(&self) -> Result<(), CoreError> {
        self.lost.error()?;
        self.framebuffer_check()?;
        unsafe {
            let e = self.context.get_error();
            if e == reset::CONTEXT_LOST {
                self.lost.lose("the OpenGL context was lost");
                self.lost.error()?;
            }
            if e != crate::context::NO_ERROR {
                Err(CoreError::ContextError(
                    match e {
                        crate::context::INVALID_ENUM => "Invalid enum",
                        crate::context::INVALID_VALUE => "Invalid value",
                        crate::context::INVALID_OPERATION => "Invalid operation",
                        crate::context::INVALID_FRAMEBUFFER_OPERATION => {
                            "Invalid framebuffer operation"
                        }
                        crate::context::OUT_OF_MEMORY => "Out of memory",
                        crate::context::STACK_OVERFLOW => "Stack overflow",
                        crate::context::STACK_UNDERFLOW => "Stack underflow",
                        _ => "Unknown",
                    }
                    .to_string(),
                ))?;
            }
        }
        Ok(())
    }

    fn is_lost(&self) -> bool {
        self.lost.check().is_some()
    }

    fn max_samples(&self) -> u32 {
        unsafe {
            self.context
                .get_parameter_i32(crate::context::MAX_SAMPLES)
                .try_into()
                .unwrap()
        }
    }

    fn create_buffer(&self) -> BufferId {
        BufferId::Gl(unsafe {
            self.context
                .create_buffer()
                .expect("Failed creating buffer")
        })
    }

    fn delete_buffer(&self, buffer: BufferId) {
        unsafe {
            self.context.delete_buffer(buffer.gl());
        }
    }

    fn write_buffer(&self, buffer: BufferId, kind: BufferKind, bytes: &[u8], usage: BufferUsage) {
        self.bind_buffer(kind, buffer);
        unsafe {
            self.context.buffer_data_u8_slice(
                buffer_target(kind),
                bytes,
                match usage {
                    BufferUsage::Static => crate::context::STATIC_DRAW,
                    BufferUsage::Dynamic => crate::context::DYNAMIC_DRAW,
                },
            );
            self.context.bind_buffer(buffer_target(kind), None);
        }
    }

    fn write_buffer_subset(&self, buffer: BufferId, kind: BufferKind, offset: u32, bytes: &[u8]) {
        self.bind_buffer(kind, buffer);
        unsafe {
            self.context
                .buffer_sub_data_u8_slice(buffer_target(kind), offset as i32, bytes);
            self.context.bind_buffer(buffer_target(kind), None);
        }
    }

    fn create_texture(&self, desc: &TextureDesc) -> TextureId {
        let id = unsafe {
            self.context
                .create_texture()
                .expect("Failed creating texture")
        };
        let target = texture_target(desc.kind);
        unsafe {
            self.context.bind_texture(target, Some(id));
        }
        self.set_texture_parameters(target, &desc.sampler);
        if let Some(storage) = desc.storage {
            let internal_format = internal_format(storage.format);
            unsafe {
                match desc.kind {
                    TextureKind::D2 | TextureKind::CubeMap => self.context.tex_storage_2d(
                        target,
                        storage.mip_levels as i32,
                        internal_format,
                        storage.width as i32,
                        storage.height as i32,
                    ),
                    TextureKind::D2Array | TextureKind::D3 => self.context.tex_storage_3d(
                        target,
                        storage.mip_levels as i32,
                        internal_format,
                        storage.width as i32,
                        storage.height as i32,
                        storage.depth as i32,
                    ),
                }
            }
        }
        TextureId::Gl(id)
    }

    fn delete_texture(&self, texture: TextureId) {
        unsafe {
            self.context.delete_texture(texture.gl());
        }
    }

    fn write_texture(
        &self,
        texture: TextureId,
        kind: TextureKind,
        region: TextureRegion,
        width: u32,
        height: u32,
        depth: u32,
        format: ColorFormat,
        bytes: &[u8],
    ) {
        self.bind_texture_object(texture, kind);
        let pixels = crate::context::PixelUnpackData::Slice(Some(bytes));
        unsafe {
            match (kind, region) {
                (TextureKind::D2, _) => self.context.tex_sub_image_2d(
                    crate::context::TEXTURE_2D,
                    0,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    format_from_channels(format.channels),
                    data_type(format.primitive),
                    pixels,
                ),
                (TextureKind::CubeMap, TextureRegion::Side(side)) => self.context.tex_sub_image_2d(
                    cube_map_side_target(side),
                    0,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    format_from_channels(format.channels),
                    data_type(format.primitive),
                    pixels,
                ),
                (TextureKind::D2Array, TextureRegion::Layer(layer)) => {
                    self.context.tex_sub_image_3d(
                        crate::context::TEXTURE_2D_ARRAY,
                        0,
                        0,
                        0,
                        layer as i32,
                        width as i32,
                        height as i32,
                        1,
                        format_from_channels(format.channels),
                        data_type(format.primitive),
                        pixels,
                    )
                }
                (TextureKind::D3, _) => self.context.tex_sub_image_3d(
                    crate::context::TEXTURE_3D,
                    0,
                    0,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    depth as i32,
                    format_from_channels(format.channels),
                    data_type(format.primitive),
                    pixels,
                ),
                _ => unreachable!("invalid texture region {:?} for {:?}", region, kind),
            }
        }
    }

    fn generate_mipmaps(&self, texture: TextureId, kind: TextureKind) {
        self.bind_texture_object(texture, kind);
        unsafe {
            self.context.generate_mipmap(texture_target(kind));
        }
    }

    fn create_renderbuffer(
        &self,
        format: TextureFormat,
        width: u32,
        height: u32,
        number_of_samples: u32,
    ) -> RenderbufferId {
        let id = unsafe {
            self.context
                .create_renderbuffer()
                .expect("Failed creating render buffer")
        };
        unsafe {
            self.context
                .bind_renderbuffer(crate::context::RENDERBUFFER, Some(id));
            self.context.renderbuffer_storage_multisample(
                crate::context::RENDERBUFFER,
                number_of_samples as i32,
                internal_format(format),
                width as i32,
                height as i32,
            );
        }
        RenderbufferId::Gl(id)
    }

    fn delete_renderbuffer(&self, renderbuffer: RenderbufferId) {
        unsafe {
            self.context.delete_renderbuffer(renderbuffer.gl());
        }
    }

    fn create_framebuffer(&self) -> FramebufferId {
        FramebufferId::Gl(unsafe {
            self.context
                .create_framebuffer()
                .expect("Failed creating frame buffer")
        })
    }

    fn delete_framebuffer(&self, framebuffer: FramebufferId) {
        unsafe {
            self.context.delete_framebuffer(framebuffer.gl());
        }
    }

    fn begin_pass(&self, target: &PassTarget, scissor_box: ScissorBox, load: LoadOp) {
        match load {
            LoadOp::Load => {}
        }
        self.set_scissor(scissor_box);
        self.bind_target(target, crate::context::DRAW_FRAMEBUFFER);
    }

    fn end_pass(&self) {
        // GL renders immediately and keeps the framebuffer bound, exactly as before the backend layer existed.
    }

    fn clear(&self, target: &PassTarget, scissor_box: ScissorBox, clear_state: ClearState) {
        self.set_scissor(scissor_box);
        self.bind_target(target, crate::context::DRAW_FRAMEBUFFER);
        self.set_write_mask(WriteMask {
            red: clear_state.red.is_some(),
            green: clear_state.green.is_some(),
            blue: clear_state.blue.is_some(),
            alpha: clear_state.alpha.is_some(),
            depth: clear_state.depth.is_some(),
        });
        unsafe {
            let clear_color = clear_state.red.is_some()
                || clear_state.green.is_some()
                || clear_state.blue.is_some()
                || clear_state.alpha.is_some();
            if clear_color {
                self.context.clear_color(
                    clear_state.red.unwrap_or(0.0),
                    clear_state.green.unwrap_or(0.0),
                    clear_state.blue.unwrap_or(0.0),
                    clear_state.alpha.unwrap_or(1.0),
                );
            }
            if let Some(depth) = clear_state.depth {
                self.context.clear_depth_f32(depth);
            }
            self.context
                .clear(if clear_color && clear_state.depth.is_some() {
                    crate::context::COLOR_BUFFER_BIT | crate::context::DEPTH_BUFFER_BIT
                } else if clear_color {
                    crate::context::COLOR_BUFFER_BIT
                } else {
                    crate::context::DEPTH_BUFFER_BIT
                });
        }
    }

    fn copy_target(&self, source: &PassTarget, destination: &PassTarget, mask: CopyMask) {
        self.bind_target(source, crate::context::DRAW_FRAMEBUFFER);
        self.bind_target(destination, crate::context::DRAW_FRAMEBUFFER);
        let mask = match (mask.color, mask.depth) {
            (true, true) => crate::context::COLOR_BUFFER_BIT | crate::context::DEPTH_BUFFER_BIT,
            (true, false) => crate::context::COLOR_BUFFER_BIT,
            (false, true) => crate::context::DEPTH_BUFFER_BIT,
            (false, false) => unreachable!(),
        };
        self.set_scissor(ScissorBox::new_at_origo(
            destination.width,
            destination.height,
        ));
        unsafe {
            self.context
                .bind_framebuffer(crate::context::READ_FRAMEBUFFER, self.framebuffer(source));

            self.context.blit_framebuffer(
                0,
                0,
                source.width as i32,
                source.height as i32,
                0,
                0,
                destination.width as i32,
                destination.height as i32,
                mask,
                crate::context::NEAREST,
            );
        }
    }

    fn read_pixels(&self, source: &PassTarget, rect: ScissorBox, format: ReadFormat) -> Vec<u8> {
        let mut bytes = vec![0u8; read_size(rect, format)];
        self.read_pixels_into(source, rect, format, &mut bytes);
        bytes
    }

    fn read_pixels_into(
        &self,
        source: &PassTarget,
        rect: ScissorBox,
        format: ReadFormat,
        bytes: &mut [u8],
    ) {
        assert_eq!(bytes.len(), read_size(rect, format));
        self.read_pixels_to(
            source,
            rect,
            format,
            crate::context::PixelPackData::Slice(Some(bytes)),
        );
    }

    // In a browser the default stays, which reads at once: a future that asks to be polled again would keep the browser
    // from ever finishing the frame it waits for
    #[cfg(not(target_arch = "wasm32"))]
    fn read_pixels_async(
        &self,
        source: &PassTarget,
        rect: ScissorBox,
        format: ReadFormat,
    ) -> PixelsFuture {
        let pixels = self.read_pixels_async_into(
            source,
            rect,
            format,
            Box::new(vec![0u8; read_size(rect, format)]),
        );
        Box::pin(async move {
            *pixels
                .await
                .into_any()
                .downcast::<Vec<u8>>()
                .expect("the memory that was handed in")
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_pixels_async_into(
        &self,
        source: &PassTarget,
        rect: ScissorBox,
        format: ReadFormat,
        mut storage: Box<dyn PixelStorage>,
    ) -> StorageFuture {
        // The pixels are copied into a buffer of the graphics card, which does not wait for the card to finish drawing.
        // The fence tells when the card is done, then the buffer is read without waiting.
        let size = read_size(rect, format);
        unsafe {
            let context = &self.context;
            let buffer = context
                .create_buffer()
                .expect("Failed creating a buffer for reading pixels");
            context.bind_buffer(crate::context::PIXEL_PACK_BUFFER, Some(buffer));
            context.buffer_data_size(
                crate::context::PIXEL_PACK_BUFFER,
                size as i32,
                crate::context::STREAM_READ,
            );
            self.read_pixels_to(
                source,
                rect,
                format,
                crate::context::PixelPackData::BufferOffset(0),
            );
            context.bind_buffer(crate::context::PIXEL_PACK_BUFFER, None);
            let fence = context
                .fence_sync(crate::context::SYNC_GPU_COMMANDS_COMPLETE, 0)
                .expect("Failed creating a fence for reading pixels");
            // Otherwise the card may not start before the future is polled
            context.flush();
            assert_eq!(storage.bytes().len(), size);
            Box::pin(PendingRead {
                context: self.context.clone(),
                lost: self.lost.clone(),
                resources: Some((buffer, fence)),
                storage: Some(storage),
            })
        }
    }

    fn create_program(
        &self,
        vertex_shader_source: &str,
        fragment_shader_source: &str,
    ) -> Result<(ProgramId, ProgramReflection), CoreError> {
        let context = &self.context;
        unsafe {
            let vert_shader = context
                .create_shader(crate::context::VERTEX_SHADER)
                .expect("Failed creating vertex shader");
            let frag_shader = context
                .create_shader(crate::context::FRAGMENT_SHADER)
                .expect("Failed creating fragment shader");

            let header: &str = if context.version().is_embedded {
                "#version 300 es
                    #ifdef GL_FRAGMENT_PRECISION_HIGH
                        precision highp float;
                        precision highp int;
                        precision highp sampler2DArray;
                        precision highp sampler3D;
                    #else
                        precision mediump float;
                        precision mediump int;
                        precision mediump sampler2DArray;
                        precision mediump sampler3D;
                    #endif\n"
            } else {
                "#version 330 core\n"
            };
            let vertex_shader_source = format!("{}{}", header, vertex_shader_source);
            let fragment_shader_source = format!("{}{}", header, fragment_shader_source);

            context.shader_source(vert_shader, &vertex_shader_source);
            context.shader_source(frag_shader, &fragment_shader_source);
            context.compile_shader(vert_shader);
            context.compile_shader(frag_shader);

            let id = context.create_program().expect("Failed creating program");
            context.attach_shader(id, vert_shader);
            context.attach_shader(id, frag_shader);
            context.link_program(id);

            if !context.get_program_link_status(id) {
                let log = context.get_shader_info_log(vert_shader);
                if !log.is_empty() {
                    Err(shader_compilation_error(
                        "vertex",
                        log,
                        vertex_shader_source,
                    ))?;
                }
                let log = context.get_shader_info_log(frag_shader);
                if !log.is_empty() {
                    Err(shader_compilation_error(
                        "fragment",
                        log,
                        fragment_shader_source,
                    ))?;
                }
                let log = context.get_program_info_log(id);
                if !log.is_empty() {
                    Err(CoreError::ShaderLink(log))?;
                }
                Err(CoreError::ShaderCompilerError)?;
            }

            context.detach_shader(id, vert_shader);
            context.detach_shader(id, frag_shader);
            context.delete_shader(vert_shader);
            context.delete_shader(frag_shader);

            // Init vertex attributes
            let num_attribs = context.get_active_attributes(id);
            let mut attributes = HashMap::new();
            for i in 0..num_attribs {
                if let Some(crate::context::ActiveAttribute { name, .. }) = context
                    .get_active_attribute(id, i)
                    .filter(|a| !a.name.starts_with("gl_"))
                {
                    if let Some(location) = context.get_attrib_location(id, &name) {
                        attributes.insert(name, location);
                    }
                }
            }

            // Init uniforms
            let num_uniforms = context.get_active_uniforms(id);
            let mut uniforms = HashMap::new();
            for i in 0..num_uniforms {
                if let Some(crate::context::ActiveUniform { name, .. }) = context
                    .get_active_uniform(id, i)
                    .filter(|a| !a.name.starts_with("gl_"))
                {
                    if let Some(location) = context.get_uniform_location(id, &name) {
                        let name = name.split('[').next().unwrap().to_string();
                        uniforms.insert(name, UniformLocation::Gl(location));
                    }
                }
            }

            Ok((
                ProgramId::Gl(id),
                ProgramReflection {
                    attributes,
                    uniforms,
                },
            ))
        }
    }

    fn delete_program(&self, program: ProgramId) {
        unsafe {
            self.context.delete_program(program.gl());
        }
    }

    fn set_uniform(&self, program: ProgramId, location: &UniformLocation, value: &UniformValue) {
        self.use_program(program);
        let location = Some(location.gl());
        unsafe {
            match value {
                UniformValue::F32(type_, data) => match type_ {
                    UniformType::Value => self.context.uniform_1_f32_slice(location, data),
                    UniformType::Vec2 => self.context.uniform_2_f32_slice(location, data),
                    UniformType::Vec3 => self.context.uniform_3_f32_slice(location, data),
                    UniformType::Vec4 => self.context.uniform_4_f32_slice(location, data),
                    UniformType::Mat2 => self
                        .context
                        .uniform_matrix_2_f32_slice(location, false, data),
                    UniformType::Mat3 => self
                        .context
                        .uniform_matrix_3_f32_slice(location, false, data),
                    UniformType::Mat4 => self
                        .context
                        .uniform_matrix_4_f32_slice(location, false, data),
                },
                UniformValue::I32(type_, data) => match type_ {
                    UniformType::Value => self.context.uniform_1_i32_slice(location, data),
                    UniformType::Vec2 => self.context.uniform_2_i32_slice(location, data),
                    UniformType::Vec3 => self.context.uniform_3_i32_slice(location, data),
                    UniformType::Vec4 => self.context.uniform_4_i32_slice(location, data),
                    _ => unimplemented!(),
                },
                UniformValue::U32(type_, data) => match type_ {
                    UniformType::Value => self.context.uniform_1_u32_slice(location, data),
                    UniformType::Vec2 => self.context.uniform_2_u32_slice(location, data),
                    UniformType::Vec3 => self.context.uniform_3_u32_slice(location, data),
                    UniformType::Vec4 => self.context.uniform_4_u32_slice(location, data),
                    _ => unimplemented!(),
                },
            }
        }
        self.unuse_program();
    }

    fn bind_texture(&self, unit: u32, texture: TextureId, kind: TextureKind) {
        unsafe {
            self.context.active_texture(crate::context::TEXTURE0 + unit);
        }
        self.bind_texture_object(texture, kind);
    }

    fn uniform_block_index(&self, program: ProgramId, name: &str) -> Option<u32> {
        unsafe { self.context.get_uniform_block_index(program.gl(), name) }
    }

    fn bind_uniform_block(
        &self,
        program: ProgramId,
        block_index: u32,
        binding: u32,
        buffer: BufferId,
    ) {
        unsafe {
            self.context
                .uniform_block_binding(program.gl(), block_index, binding);
            self.context.bind_buffer_base(
                crate::context::UNIFORM_BUFFER,
                binding,
                Some(buffer.gl()),
            );
            self.context
                .bind_buffer(crate::context::UNIFORM_BUFFER, None);
        }
    }

    fn bind_vertex_attribute(
        &self,
        program: ProgramId,
        location: u32,
        buffer: BufferId,
        format: VertexFormat,
        divisor: u32,
    ) {
        self.bind_buffer(BufferKind::Vertex(format), buffer);
        self.use_program(program);
        unsafe {
            self.context.bind_vertex_array(Some(self.vao));
            self.context.enable_vertex_attrib_array(location);
            if !format.normalized && format.primitive.is_integer() {
                self.context.vertex_attrib_pointer_i32(
                    location,
                    format.components as i32,
                    data_type(format.primitive),
                    0,
                    0,
                );
            } else {
                self.context.vertex_attrib_pointer_f32(
                    location,
                    format.components as i32,
                    data_type(format.primitive),
                    format.normalized,
                    0,
                    0,
                );
            }
            self.context.vertex_attrib_divisor(location, divisor);
            self.context.bind_buffer(crate::context::ARRAY_BUFFER, None);
        }
        self.unuse_program();
    }

    fn prepare_attributeless_draw(&self) {
        unsafe { self.context.bind_vertex_array(Some(self.vao)) };
    }

    fn draw(&self, call: &DrawCall) {
        let context = &self.context;
        let mut draw = || unsafe {
            match (call.indices, call.instances) {
                (None, None) => {
                    context.draw_arrays(crate::context::TRIANGLES, 0, call.count as i32);
                }
                (None, Some(instances)) => {
                    context.draw_arrays_instanced(
                        crate::context::TRIANGLES,
                        0,
                        call.count as i32,
                        instances as i32,
                    );
                    context.bind_buffer(crate::context::ELEMENT_ARRAY_BUFFER, None);
                }
                (Some(indices), instances) => {
                    context.bind_buffer(
                        crate::context::ELEMENT_ARRAY_BUFFER,
                        Some(indices.buffer.gl()),
                    );
                    if let Some(instances) = instances {
                        context.draw_elements_instanced(
                            crate::context::TRIANGLES,
                            call.count as i32,
                            data_type(indices.index_type),
                            indices.first as i32,
                            instances as i32,
                        );
                    } else {
                        context.draw_elements(
                            crate::context::TRIANGLES,
                            call.count as i32,
                            data_type(indices.index_type),
                            indices.first as i32,
                        );
                    }
                    context.bind_buffer(crate::context::ELEMENT_ARRAY_BUFFER, None);
                }
            }
        };
        self.draw_with(
            call.program,
            call.attribute_locations,
            call.render_states,
            call.viewport,
            &mut draw,
        );
    }
}

// CONVERSIONS TO GL CONSTANTS

fn shader_compilation_error(typ: &str, log: String, source: String) -> CoreError {
    let lines: Vec<String> = source
        .lines()
        .enumerate()
        .map(|(index, l)| format!("{:0>3}: {}", index + 1, l))
        .collect();
    CoreError::ShaderCompilation(typ.to_string(), lines.join("\n"), log)
}

fn buffer_target(kind: BufferKind) -> u32 {
    match kind {
        BufferKind::Vertex(_) | BufferKind::Instance(_) => crate::context::ARRAY_BUFFER,
        BufferKind::Element => crate::context::ELEMENT_ARRAY_BUFFER,
        BufferKind::Uniform => crate::context::UNIFORM_BUFFER,
    }
}

pub(crate) fn texture_target(kind: TextureKind) -> u32 {
    match kind {
        TextureKind::D2 => crate::context::TEXTURE_2D,
        TextureKind::D2Array => crate::context::TEXTURE_2D_ARRAY,
        TextureKind::D3 => crate::context::TEXTURE_3D,
        TextureKind::CubeMap => crate::context::TEXTURE_CUBE_MAP,
    }
}

fn cube_map_side_target(side: CubeMapSide) -> u32 {
    match side {
        CubeMapSide::Right => crate::context::TEXTURE_CUBE_MAP_POSITIVE_X,
        CubeMapSide::Left => crate::context::TEXTURE_CUBE_MAP_NEGATIVE_X,
        CubeMapSide::Top => crate::context::TEXTURE_CUBE_MAP_POSITIVE_Y,
        CubeMapSide::Bottom => crate::context::TEXTURE_CUBE_MAP_NEGATIVE_Y,
        CubeMapSide::Front => crate::context::TEXTURE_CUBE_MAP_POSITIVE_Z,
        CubeMapSide::Back => crate::context::TEXTURE_CUBE_MAP_NEGATIVE_Z,
    }
}

pub(crate) fn data_type(primitive: PrimitiveType) -> u32 {
    match primitive {
        PrimitiveType::U8 => crate::context::UNSIGNED_BYTE,
        PrimitiveType::U16 => crate::context::UNSIGNED_SHORT,
        PrimitiveType::U32 => crate::context::UNSIGNED_INT,
        PrimitiveType::I8 => crate::context::BYTE,
        PrimitiveType::I16 => crate::context::SHORT,
        PrimitiveType::I32 => crate::context::INT,
        PrimitiveType::F16 => crate::context::HALF_FLOAT,
        PrimitiveType::F32 => crate::context::FLOAT,
    }
}

fn primitive_size(primitive: PrimitiveType) -> usize {
    match primitive {
        PrimitiveType::U8 | PrimitiveType::I8 => 1,
        PrimitiveType::U16 | PrimitiveType::I16 | PrimitiveType::F16 => 2,
        PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => 4,
    }
}

pub(crate) fn format_from_channels(channels: u32) -> u32 {
    match channels {
        1 => crate::context::RED,
        2 => crate::context::RG,
        3 => crate::context::RGB,
        4 => crate::context::RGBA,
        _ => unreachable!(),
    }
}

pub(crate) fn internal_format(format: TextureFormat) -> u32 {
    match format {
        TextureFormat::Depth(DepthFormat::Depth16) => crate::context::DEPTH_COMPONENT16,
        TextureFormat::Depth(DepthFormat::Depth24) => crate::context::DEPTH_COMPONENT24,
        TextureFormat::Depth(DepthFormat::Depth32F) => crate::context::DEPTH_COMPONENT32F,
        TextureFormat::Color(ColorFormat {
            primitive,
            channels,
        }) => {
            let formats = match primitive {
                PrimitiveType::U8 => [
                    crate::context::R8,
                    crate::context::RG8,
                    crate::context::RGB8,
                    crate::context::RGBA8,
                ],
                PrimitiveType::U16 => [
                    crate::context::R16UI,
                    crate::context::RG16UI,
                    crate::context::RGB16UI,
                    crate::context::RGBA16UI,
                ],
                PrimitiveType::U32 => [
                    crate::context::R32UI,
                    crate::context::RG32UI,
                    crate::context::RGB32UI,
                    crate::context::RGBA32UI,
                ],
                PrimitiveType::I8 => [
                    crate::context::R8I,
                    crate::context::RG8I,
                    crate::context::RGB8I,
                    crate::context::RGBA8I,
                ],
                PrimitiveType::I16 => [
                    crate::context::R16I,
                    crate::context::RG16I,
                    crate::context::RGB16I,
                    crate::context::RGBA16I,
                ],
                PrimitiveType::I32 => [
                    crate::context::R32I,
                    crate::context::RG32I,
                    crate::context::RGB32I,
                    crate::context::RGBA32I,
                ],
                PrimitiveType::F16 => [
                    crate::context::R16F,
                    crate::context::RG16F,
                    crate::context::RGB16F,
                    crate::context::RGBA16F,
                ],
                PrimitiveType::F32 => [
                    crate::context::R32F,
                    crate::context::RG32F,
                    crate::context::RGB32F,
                    crate::context::RGBA32F,
                ],
            };
            match channels {
                1..=4 => formats[channels as usize - 1],
                _ => unreachable!(),
            }
        }
    }
}

fn wrapping_from(wrapping: Wrapping) -> i32 {
    (match wrapping {
        Wrapping::Repeat => crate::context::REPEAT,
        Wrapping::MirroredRepeat => crate::context::MIRRORED_REPEAT,
        Wrapping::ClampToEdge => crate::context::CLAMP_TO_EDGE,
    }) as i32
}

fn interpolation_from(interpolation: Interpolation) -> i32 {
    (match interpolation {
        Interpolation::Nearest => crate::context::NEAREST,
        Interpolation::Linear => crate::context::LINEAR,
        _ => panic!("Can only sample textures using 'NEAREST' or 'LINEAR' interpolation"),
    }) as i32
}
