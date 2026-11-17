use crate::core::texture::*;

///
/// A depth texture cube map that can be rendered into and read from. See also [RenderTarget] and [DepthTarget].
///
pub struct DepthTextureCubeMap {
    context: Context,
    id: backend::TextureId,
    width: u32,
    height: u32,
}

impl DepthTextureCubeMap {
    ///
    /// Creates a new depth texture cube map.
    ///
    pub fn new<T: DepthTextureDataType>(
        context: &Context,
        width: u32,
        height: u32,
        wrap_s: Wrapping,
        wrap_t: Wrapping,
        wrap_r: Wrapping,
    ) -> Self {
        let id = create_depth_texture::<T>(
            context,
            backend::TextureKind::CubeMap,
            width,
            height,
            1,
            wrap_s,
            wrap_t,
            Some(wrap_r),
        );
        Self {
            context: context.clone(),
            id,
            width,
            height,
        }
    }

    ///
    /// Returns a [DepthTarget] which can be used to clear, write to and read from the given side of this texture.
    /// Combine this together with a [ColorTarget] with [RenderTarget::new] to be able to write to both a depth and color target at the same time.
    ///
    pub fn as_depth_target(&self, side: CubeMapSide) -> DepthTarget<'_> {
        DepthTarget::new_texture_cube_map(&self.context, self, side)
    }

    /// The width of this texture.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height of this texture.
    pub fn height(&self) -> u32 {
        self.height
    }

    pub(in crate::core) fn id(&self) -> backend::TextureId {
        self.id
    }
}

impl Drop for DepthTextureCubeMap {
    fn drop(&mut self) {
        self.context.backend().delete_texture(self.id);
    }
}
