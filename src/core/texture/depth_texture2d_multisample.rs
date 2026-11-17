use crate::core::texture::*;

pub struct DepthTexture2DMultisample {
    context: Context,
    id: backend::RenderbufferId,
    width: u32,
    height: u32,
    number_of_samples: u32,
}

impl DepthTexture2DMultisample {
    pub fn new<T: DepthTextureDataType>(
        context: &Context,
        width: u32,
        height: u32,
        number_of_samples: u32,
    ) -> Self {
        // Neither filtering, nor mipmap levels, nor clamping makes sense for multisampled textures.
        let id = context.backend().create_renderbuffer(
            backend::TextureFormat::Depth(T::depth_format()),
            width,
            height,
            number_of_samples,
        );
        Self {
            context: context.clone(),
            id,
            width,
            height,
            number_of_samples,
        }
    }

    /// The width of this texture.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height of this texture.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The number of samples per fragment.
    pub fn number_of_samples(&self) -> u32 {
        self.number_of_samples
    }

    pub(in crate::core) fn id(&self) -> backend::RenderbufferId {
        self.id
    }
}

impl Drop for DepthTexture2DMultisample {
    fn drop(&mut self) {
        self.context.backend().delete_renderbuffer(self.id);
    }
}
