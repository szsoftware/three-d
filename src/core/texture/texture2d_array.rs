use crate::core::texture::*;

///
/// A array of 2D color textures that can be rendered into.
///
/// **Note:** [DepthTest] is disabled if not also writing to a [DepthTarget].
/// Use a [RenderTarget] to write to both color and depth.
///
pub struct Texture2DArray {
    context: Context,
    id: backend::TextureId,
    width: u32,
    height: u32,
    depth: u32,
    number_of_mip_maps: u32,
    data_byte_size: usize,
}

impl Texture2DArray {
    ///
    /// Creates a new texture array from the given [CpuTexture]s.
    /// All of the cpu textures must contain data with the same [TextureDataType] and the same width and height.
    ///
    /// **Note:** Mip maps will not be generated for RGB16F and RGB32F format, even if `mip_map_filter` is specified.
    ///
    pub fn new(context: &Context, cpu_textures: &[&CpuTexture]) -> Self {
        let cpu_texture = cpu_textures
            .first()
            .expect("Expect at least one texture in a texture array");
        match &cpu_texture.data {
            TextureData::RU8(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures.iter().map(|t| ru8_data(t)).collect::<Vec<_>>(),
            ),
            TextureData::RgU8(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rgu8_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RgbU8(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rgbu8_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RgbaU8(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rgbau8_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RF16(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rf16_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RgF16(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rgf16_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RgbF16(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rgbf16_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RgbaF16(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rgbaf16_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RF32(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rf32_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RgF32(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rgf32_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RgbF32(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rgbf32_data(t))
                    .collect::<Vec<_>>(),
            ),
            TextureData::RgbaF32(_) => Self::new_with_data(
                context,
                cpu_texture,
                &cpu_textures
                    .iter()
                    .map(|t| rgbaf32_data(t))
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn new_with_data<T: TextureDataType>(
        context: &Context,
        cpu_texture: &CpuTexture,
        data: &[&[T]],
    ) -> Self {
        let texture = Self::new_empty::<T>(
            context,
            cpu_texture.width,
            cpu_texture.height,
            data.len() as u32,
            cpu_texture.min_filter,
            cpu_texture.mag_filter,
            cpu_texture.mipmap,
            cpu_texture.wrap_s,
            cpu_texture.wrap_t,
        );
        texture.fill(data);
        texture
    }

    ///
    /// Creates a new array of 2D textures.
    ///
    /// **Note:** Mip maps will not be generated for RGB16F and RGB32F format, even if `mip_map_filter` is specified.
    ///
    pub fn new_empty<T: TextureDataType>(
        context: &Context,
        width: u32,
        height: u32,
        depth: u32,
        min_filter: Interpolation,
        mag_filter: Interpolation,
        mipmap: Option<Mipmap>,
        wrap_s: Wrapping,
        wrap_t: Wrapping,
    ) -> Self {
        Self::new_internal::<T>(
            context,
            width,
            height,
            depth,
            min_filter,
            mag_filter,
            mipmap,
            wrap_s,
            wrap_t,
            true,
            |_| {},
        )
    }

    ///
    /// Fills the texture array with the given pixel data and generate mip maps if specified at construction.
    ///
    /// # Panic
    /// Will panic if the data does not correspond to the width, height, depth and format specified at construction.
    /// It is therefore necessary to create a new texture if the texture size or format has changed.
    ///
    pub fn fill<T: TextureDataType>(&self, data: &[&[T]]) {
        for (i, data) in data.iter().enumerate() {
            self.fill_layer(i as u32, data);
        }
    }

    ///
    /// Fills the given layer in the texture array with the given pixel data and generate mip maps if specified at construction.
    ///
    /// # Panic
    /// Will panic if the layer number is bigger than the number of layers or if the data does not correspond to the width, height and format specified at construction.
    /// It is therefore necessary to create a new texture if the texture size or format has changed.
    ///
    pub fn fill_layer<T: TextureDataType>(&self, layer: u32, data: &[T]) {
        if layer >= self.depth {
            panic!(
                "cannot fill the layer {} with data, since there are only {} layers in the texture array",
                layer, self.depth
            )
        }
        check_data_length::<T>(self.width, self.height, 1, self.data_byte_size, data.len());
        let mut data = (*data).to_owned();
        flip_y(&mut data, self.width as usize, self.height as usize);
        self.context.backend().write_texture(
            self.id,
            backend::TextureKind::D2Array,
            backend::TextureRegion::Layer(layer),
            self.width,
            self.height,
            1,
            backend::ColorFormat::of::<T>(),
            to_byte_slice(&data),
        );
        self.generate_mip_maps();
    }

    ///
    /// Returns a [ColorTarget] which can be used to clear, write to and read from the given layers and mip level of this texture.
    /// Combine this together with a [DepthTarget] with [RenderTarget::new] to be able to write to both a depth and color target at the same time.
    /// If `None` is specified as the mip level, the 0 level mip level is used and mip maps are generated after a write operation if a mip map filter is specified.
    /// Otherwise, the given mip level is used and no mip maps are generated.
    ///
    /// **Note:** [DepthTest] is disabled if not also writing to a depth texture.
    ///
    pub fn as_color_target<'a>(
        &'a self,
        layers: &'a [u32],
        mip_level: Option<u32>,
    ) -> ColorTarget<'a> {
        ColorTarget::new_texture_2d_array(&self.context, self, layers, mip_level)
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

    /// The number of mip maps of this texture.
    pub fn number_of_mip_maps(&self) -> u32 {
        self.number_of_mip_maps
    }

    pub(in crate::core) fn generate_mip_maps(&self) {
        if self.number_of_mip_maps > 1 {
            self.context
                .backend()
                .generate_mipmaps(self.id, backend::TextureKind::D2Array);
        }
    }

    pub(in crate::core) fn id(&self) -> backend::TextureId {
        self.id
    }

    ///
    /// Creates a new texture where it is up to the caller to allocate and transfer data to the GPU
    /// using low-level context calls inside the callback.
    /// This function binds the texture and sets the parameters before calling the callback and generates mip maps afterwards.
    ///
    /// # Safety
    ///
    /// This function is unsafe and should only be used in special cases,
    /// for example when you have an uncommon source of data or the data is in a special format like sRGB.
    ///
    pub unsafe fn new_unchecked<T: TextureDataType>(
        context: &Context,
        width: u32,
        height: u32,
        depth: u32,
        min_filter: Interpolation,
        mag_filter: Interpolation,
        mipmap: Option<Mipmap>,
        wrap_s: Wrapping,
        wrap_t: Wrapping,
        callback: impl FnOnce(&Self),
    ) -> Self {
        Self::new_internal::<T>(
            context, width, height, depth, min_filter, mag_filter, mipmap, wrap_s, wrap_t, false,
            callback,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_internal<T: TextureDataType>(
        context: &Context,
        width: u32,
        height: u32,
        depth: u32,
        min_filter: Interpolation,
        mag_filter: Interpolation,
        mipmap: Option<Mipmap>,
        wrap_s: Wrapping,
        wrap_t: Wrapping,
        allocate: bool,
        callback: impl FnOnce(&Self),
    ) -> Self {
        let number_of_mip_maps = calculate_number_of_mip_maps::<T>(mipmap, width, height, None);
        let id = create_texture::<T>(
            context,
            backend::TextureKind::D2Array,
            width,
            height,
            depth,
            number_of_mip_maps,
            min_filter,
            mag_filter,
            mipmap,
            wrap_s,
            wrap_t,
            None,
            allocate,
        );
        let texture = Self {
            context: context.clone(),
            id,
            width,
            height,
            depth,
            number_of_mip_maps,
            data_byte_size: std::mem::size_of::<T>(),
        };
        callback(&texture);
        texture.generate_mip_maps();
        texture
    }
}

impl Drop for Texture2DArray {
    fn drop(&mut self) {
        self.context.backend().delete_texture(self.id);
    }
}
