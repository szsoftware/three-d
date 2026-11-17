use crate::core::texture::*;

///
/// An array of 2D depth textures that can be rendered into and read from. See also [RenderTarget] and [DepthTarget].
///
pub struct DepthTexture2DArray {
    context: Context,
    id: backend::TextureId,
    width: u32,
    height: u32,
    depth: u32,
}

impl DepthTexture2DArray {
    ///
    /// Creates a new array of depth textures.
    ///
    pub fn new<T: DepthTextureDataType>(
        context: &Context,
        width: u32,
        height: u32,
        depth: u32,
        wrap_s: Wrapping,
        wrap_t: Wrapping,
    ) -> Self {
        let id = create_depth_texture::<T>(
            context,
            backend::TextureKind::D2Array,
            width,
            height,
            depth,
            wrap_s,
            wrap_t,
            None,
        );
        Self {
            context: context.clone(),
            id,
            width,
            height,
            depth,
        }
    }

    ///
    /// Returns a [DepthTarget] which can be used to clear, write to and read from the given layer of this texture.
    /// Combine this together with a [ColorTarget] with [RenderTarget::new] to be able to write to both a depth and color target at the same time.
    ///
    pub fn as_depth_target(&self, layer: u32) -> DepthTarget<'_> {
        DepthTarget::new_texture_2d_array(&self.context, self, layer)
    }

    /// The width of this texture.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height of this texture.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The number of layers.
    pub fn depth(&self) -> u32 {
        self.depth
    }

    pub(in crate::core) fn id(&self) -> backend::TextureId {
        self.id
    }
}

impl Drop for DepthTexture2DArray {
    fn drop(&mut self) {
        self.context.backend().delete_texture(self.id);
    }
}
