//!
//! Functionality for rendering to the screen or into textures.
//!

mod clear_state;
#[doc(inline)]
pub use clear_state::*;

mod color_target;
#[doc(inline)]
pub use color_target::*;

mod depth_target;
#[doc(inline)]
pub use depth_target::*;

mod multisample;
#[doc(inline)]
pub use multisample::*;

mod color_target_multisample;
#[doc(inline)]
pub use color_target_multisample::*;

mod depth_target_multisample;
#[doc(inline)]
pub use depth_target_multisample::*;

use crate::core::*;

use crate::context::Framebuffer;
///
/// Adds additional functionality to clear, read from and write to the screen (see [RenderTarget::screen]) or a color texture and
/// a depth texture at the same time (see [RenderTarget::new]).
/// If you only want to perform an operation on either a color texture or depth texture, see [ColorTarget] and [DepthTarget] respectively.
/// A render target purely adds functionality, so it can be created each time it is needed, the actual data is saved in the textures.
///
pub struct RenderTarget<'a> {
    id: Option<backend::FramebufferId>,
    color: Option<ColorTarget<'a>>,
    depth: Option<DepthTarget<'a>>,
    pub(crate) context: Context,
    width: u32,
    height: u32,
}

impl<'a> RenderTarget<'a> {
    ///
    /// Returns the screen render target for this context.
    /// Write to this render target to draw something on the screen.
    ///
    pub fn screen(context: &Context, width: u32, height: u32) -> Self {
        Self {
            context: context.clone(),
            id: None,
            color: None,
            depth: None,
            width,
            height,
        }
    }

    ///
    /// Constructs a new render target that enables rendering into the given [ColorTarget] and [DepthTarget].
    ///
    pub fn new(color: ColorTarget<'a>, depth: DepthTarget<'a>) -> Self {
        let width = color.width();
        let height = color.height();
        Self {
            context: color.context.clone(),
            id: Some(new_framebuffer(&color.context)),
            color: Some(color),
            depth: Some(depth),
            width,
            height,
        }
    }

    /// The width of this target.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height of this target.
    pub fn height(&self) -> u32 {
        self.height
    }

    ///
    /// Clears the color and depth of this render target as defined by the given clear state.
    ///
    pub fn clear(&self, clear_state: ClearState) -> &Self {
        self.clear_partially(self.scissor_box(), clear_state)
    }

    ///
    /// Clears the color and depth of the part of this render target that is inside the given scissor box.
    ///
    pub fn clear_partially(&self, scissor_box: ScissorBox, clear_state: ClearState) -> &Self {
        self.context
            .backend()
            .clear(&self.pass_target(), scissor_box, clear_state);
        self
    }

    ///
    /// Writes whatever rendered in the `render` closure into this render target.
    ///
    pub fn write<E: std::error::Error>(
        &self,
        render: impl FnOnce() -> Result<(), E>,
    ) -> Result<&Self, E> {
        self.write_partially(self.scissor_box(), render)
    }

    ///
    /// Writes whatever rendered in the `render` closure into the part of this render target defined by the scissor box.
    ///
    pub fn write_partially<E: std::error::Error>(
        &self,
        scissor_box: ScissorBox,
        render: impl FnOnce() -> Result<(), E>,
    ) -> Result<&Self, E> {
        self.context
            .backend()
            .begin_pass(&self.pass_target(), scissor_box, backend::LoadOp::Load);
        let result = render();
        self.context.backend().end_pass();
        result?;
        if let Some(ref color) = self.color {
            color.generate_mip_maps();
        }
        Ok(self)
    }

    ///
    /// Returns the colors of the pixels in this render target.
    /// The number of channels per pixel and the data format for each channel returned from this function is specified by the generic parameter `T`.
    ///
    /// **Note:**
    /// The base type of the generic parameter `T` must match the base type of the render target, for example if the render targets base type is `u8`, the base type of `T` must also be `u8`.
    ///
    /// **Web:**
    /// The generic parameter `T` is limited to:
    /// - Unsigned byte RGBA (Specify `T` as either `Vec4<u8>` or `[u8; 4]`) which works with any render target using `u8` as its base type.
    /// - 32-bit float RGBA (Specify `T` as either `Vec4<f32>` or `[f32; 4]`) which works with any render target using `f16` or `f32` as its base type.
    ///
    pub fn read_color<T: TextureDataType>(&self) -> Vec<T> {
        self.read_color_partially(self.scissor_box())
    }

    ///
    /// Returns the colors of the pixels in this render target inside the given scissor box.
    /// The number of channels per pixel and the data format for each channel returned from this function is specified by the generic parameter `T`.
    ///
    /// **Note:**
    /// The base type of the generic parameter `T` must match the base type of the render target, for example if the render targets base type is `u8`, the base type of `T` must also be `u8`.
    ///
    /// **Web:**
    /// The generic parameter `T` is limited to:
    /// - Unsigned byte RGBA (Specify `T` as either `Vec4<u8>` or `[u8; 4]`) which works with any render target using `u8` as its base type.
    /// - 32-bit float RGBA (Specify `T` as either `Vec4<f32>` or `[f32; 4]`) which works with any render target using `f16` or `f32` as its base type.
    ///
    pub fn read_color_partially<T: TextureDataType>(&self, scissor_box: ScissorBox) -> Vec<T> {
        if self.id.is_some() && self.color.is_none() {
            panic!("Cannot read color from a render target without a color target");
        }
        // On web, the read format needs to be RGBA and f16 is not supported (see https://webglfundamentals.org/webgl/lessons/webgl-readpixels.html).
        #[cfg(target_arch = "wasm32")]
        if T::size() != 4
            || !(T::primitive_type() == backend::PrimitiveType::U8
                || T::primitive_type() == backend::PrimitiveType::F32)
        {
            panic!("Only the texture data types `Vec4<T>` and `[T; 4]` where `T` is either `u8` or `f32` are supported when reading color from a render target on web.");
        }

        let bytes = self.context.backend().read_pixels(
            &self.pass_target(),
            scissor_box,
            backend::ReadFormat::Color(backend::ColorFormat::of::<T>()),
        );
        let mut pixels = from_byte_slice(&bytes).to_vec();
        flip_y(
            &mut pixels,
            scissor_box.width as usize,
            scissor_box.height as usize,
        );
        pixels
    }

    ///
    /// Returns the colors of the pixels in this render target like [RenderTarget::read_color], but as a future.
    ///
    /// This is the only way to read pixels with the wgpu backend in a browser (WebGPU), which cannot wait for the GPU.
    /// It works with every backend and on every platform, so it can be used in code that should run everywhere.
    /// Where the pixels can be waited for, the future is ready at once.
    ///
    /// The future contains the pixels of the render target at the time this function is called, even if the render target changes
    /// or is dropped before the future is awaited. The notes of [RenderTarget::read_color] about the generic parameter `T` apply,
    /// the limits on web only for the OpenGL/WebGL backend.
    ///
    pub fn read_color_async<T: TextureDataType>(
        &self,
    ) -> impl std::future::Future<Output = Vec<T>> + 'static {
        self.read_color_partially_async(self.scissor_box())
    }

    ///
    /// Returns the colors of the pixels in this render target inside the given scissor box like [RenderTarget::read_color_partially],
    /// but as a future, see [RenderTarget::read_color_async].
    ///
    pub fn read_color_partially_async<T: TextureDataType>(
        &self,
        scissor_box: ScissorBox,
    ) -> impl std::future::Future<Output = Vec<T>> + 'static {
        if self.id.is_some() && self.color.is_none() {
            panic!("Cannot read color from a render target without a color target");
        }
        #[cfg(target_arch = "wasm32")]
        if self.context.as_gl().is_some()
            && (T::size() != 4
                || !(T::primitive_type() == backend::PrimitiveType::U8
                    || T::primitive_type() == backend::PrimitiveType::F32))
        {
            panic!("Only the texture data types `Vec4<T>` and `[T; 4]` where `T` is either `u8` or `f32` are supported when reading color from a render target on web.");
        }
        let bytes = self.context.backend().read_pixels_async(
            &self.pass_target(),
            scissor_box,
            backend::ReadFormat::Color(backend::ColorFormat::of::<T>()),
        );
        async move {
            let mut pixels = from_byte_slice(&bytes.await).to_vec();
            flip_y(
                &mut pixels,
                scissor_box.width as usize,
                scissor_box.height as usize,
            );
            pixels
        }
    }

    ///
    /// Returns the depth values in this render target.
    ///
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_depth(&self) -> Vec<f32> {
        self.read_depth_partially(self.scissor_box())
    }

    ///
    /// Returns the depth values in this render target inside the given scissor box.
    ///
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_depth_partially(&self, scissor_box: ScissorBox) -> Vec<f32> {
        if self.id.is_some() && self.depth.is_none() {
            panic!("cannot read depth from a render target without a depth target");
        }
        let pixels = self.context.backend().read_pixels(
            &self.pass_target(),
            scissor_box,
            backend::ReadFormat::Depth,
        );
        from_byte_slice(&pixels).to_vec()
    }

    ///
    /// Creates a [RenderTarget] with the given low-level [Framebuffer]. Should only be used if the [Framebuffer] is used for something else, ie. to be able
    /// to combine this crate with functionality of another crate. Also see [Self::into_framebuffer].
    ///
    pub fn from_framebuffer(
        context: &Context,
        width: u32,
        height: u32,
        framebuffer: Framebuffer,
    ) -> Self {
        Self {
            id: Some(backend::FramebufferId::Gl(framebuffer)),
            color: None,
            depth: None,
            context: context.clone(),
            width,
            height,
        }
    }

    ///
    /// Transforms this [RenderTarget] into a low-level [Framebuffer]. Should only be used if the [Framebuffer] is used for something else, ie. to be able
    /// to combine this crate with functionality of another crate. Also see [Self::from_framebuffer].
    ///
    pub fn into_framebuffer(mut self) -> Option<Framebuffer> {
        self.id.take().map(|id| id.gl())
    }

    pub(in crate::core) fn blit_to(&self, target: &RenderTarget) {
        let target_is_screen = target.color.is_none() && target.depth.is_none();
        let mask = if self.color.is_some() && (target.color.is_some() || target_is_screen) {
            backend::CopyMask {
                color: true,
                depth: self.depth.is_some() && (target.depth.is_some() || target_is_screen),
            }
        } else if self.depth.is_some() && (target.depth.is_some() || target_is_screen) {
            backend::CopyMask {
                color: false,
                depth: true,
            }
        } else {
            unreachable!()
        };
        self.context
            .backend()
            .copy_target(&self.pass_target(), &target.pass_target(), mask);
    }

    fn new_color(color: ColorTarget<'a>) -> Self {
        let width = color.width();
        let height = color.height();
        Self {
            context: color.context.clone(),
            id: Some(new_framebuffer(&color.context)),
            color: Some(color),
            depth: None,
            width,
            height,
        }
    }

    fn new_depth(depth: DepthTarget<'a>) -> Self {
        let width = depth.width();
        let height = depth.height();
        Self {
            context: depth.context.clone(),
            id: Some(new_framebuffer(&depth.context)),
            depth: Some(depth),
            color: None,
            width,
            height,
        }
    }

    fn pass_target(&self) -> backend::PassTarget {
        backend::PassTarget {
            framebuffer: self.id,
            color: self.color.as_ref().map(|color| color.attachment()),
            depth: self.depth.as_ref().map(|depth| depth.attachment()),
            width: self.width,
            height: self.height,
        }
    }
}

impl Drop for RenderTarget<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.context.backend().delete_framebuffer(id);
        }
    }
}

fn size_with_mip(size: u32, mip: Option<u32>) -> u32 {
    if let Some(mip) = mip {
        size / 2u32.pow(mip)
    } else {
        size
    }
}

fn new_framebuffer(context: &Context) -> backend::FramebufferId {
    context.backend().create_framebuffer()
}

#[cfg(debug_assertions)]
fn multisample_sanity_check(context: &Context, number_of_samples: u32) {
    let max_samples: u32 = context.backend().max_samples();
    if number_of_samples > max_samples {
        panic!("number_of_samples ({}) for multisample target is larger than supported number of samples: {}", number_of_samples, max_samples);
    }
    if (number_of_samples != 0) && number_of_samples & (number_of_samples - 1) != 0 {
        panic!("number_of_samples ({}) for multisample target must be a power of 2 (and larger than 0).", number_of_samples);
    }
}

macro_rules! impl_render_target_core_extensions_body {
    () => {
        ///
        /// Returns the scissor box that encloses the entire target.
        ///
        pub fn scissor_box(&self) -> ScissorBox {
            ScissorBox::new_at_origo(self.width(), self.height())
        }

        ///
        /// Returns the viewport that encloses the entire target.
        ///
        pub fn viewport(&self) -> Viewport {
            Viewport::new_at_origo(self.width(), self.height())
        }
    };
}

macro_rules! impl_render_target_core_extensions {
    // 2 generic arguments with bounds
    ($name:ident < $a:ident : $ta:tt , $b:ident : $tb:tt >) => {
        impl<$a: $ta, $b: $tb> $name<$a, $b> {
            impl_render_target_core_extensions_body!();
        }
    };
    // 1 generic argument with bound
    ($name:ident < $a:ident : $ta:tt >) => {
        impl<$a: $ta> $name<$a> {
            impl_render_target_core_extensions_body!();
        }
    };
    // 1 liftetime argument
    ($name:ident < $lt:lifetime >) => {
        impl<$lt> $name<$lt> {
            impl_render_target_core_extensions_body!();
        }
    };
    // without any arguments
    ($name:ty) => {
        impl $name {
            impl_render_target_core_extensions_body!();
        }
    };
}

impl_render_target_core_extensions!(RenderTarget<'a>);
impl_render_target_core_extensions!(ColorTarget<'a>);
impl_render_target_core_extensions!(DepthTarget<'a>);
impl_render_target_core_extensions!(
    RenderTargetMultisample<C: TextureDataType, D: DepthTextureDataType>
);
impl_render_target_core_extensions!(ColorTargetMultisample<C: TextureDataType>);
impl_render_target_core_extensions!(DepthTargetMultisample<D: DepthTextureDataType>);
