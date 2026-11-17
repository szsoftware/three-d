use crate::core::texture::*;
///
/// A 3D color texture.
///
pub struct Texture3D {
    context: Context,
    id: backend::TextureId,
    width: u32,
    height: u32,
    depth: u32,
    number_of_mip_maps: u32,
    data_byte_size: usize,
}

impl Texture3D {
    ///
    /// Construcs a new 3D texture with the given data.
    ///
    /// **Note:** Mip maps will not be generated for RGB16F and RGB32F format, even if `mip_map_filter` is specified.
    ///
    pub fn new(context: &Context, cpu_texture: &CpuTexture3D) -> Self {
        match cpu_texture.data {
            TextureData::RU8(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RgU8(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RgbU8(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RgbaU8(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RF16(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RgF16(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RgbF16(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RgbaF16(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RF32(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RgF32(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RgbF32(ref data) => Self::new_with_data(context, cpu_texture, data),
            TextureData::RgbaF32(ref data) => Self::new_with_data(context, cpu_texture, data),
        }
    }

    fn new_with_data<T: TextureDataType>(
        context: &Context,
        cpu_texture: &CpuTexture3D,
        data: &[T],
    ) -> Self {
        let texture = Self::new_empty::<T>(
            context,
            cpu_texture.width,
            cpu_texture.height,
            cpu_texture.depth,
            cpu_texture.min_filter,
            cpu_texture.mag_filter,
            cpu_texture.mipmap,
            cpu_texture.wrap_s,
            cpu_texture.wrap_t,
            cpu_texture.wrap_r,
        );
        texture.fill(data);
        texture
    }

    ///
    /// Creates a new empty 3D color texture.
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
        wrap_r: Wrapping,
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
            wrap_r,
            true,
            |_| {},
        )
    }

    ///
    /// Fills this texture with the given data and generate mip maps if specified at construction.
    ///
    /// # Panic
    /// Will panic if the length of the data does not correspond to the width, height, depth and format specified at construction.
    /// It is therefore necessary to create a new texture if the texture size or format has changed.
    ///
    pub fn fill<T: TextureDataType>(&self, data: &[T]) {
        check_data_length::<T>(
            self.width,
            self.height,
            self.depth,
            self.data_byte_size,
            data.len(),
        );
        self.context.backend().write_texture(
            self.id,
            backend::TextureKind::D3,
            backend::TextureRegion::All,
            self.width,
            self.height,
            self.depth,
            backend::ColorFormat::of::<T>(),
            to_byte_slice(data),
        );
        self.generate_mip_maps();
    }

    /// The width of this texture.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height of this texture.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The depth of this texture.
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// The number of mip maps of this texture.
    pub fn number_of_mip_maps(&self) -> u32 {
        self.number_of_mip_maps
    }

    fn generate_mip_maps(&self) {
        if self.number_of_mip_maps > 1 {
            self.context
                .backend()
                .generate_mipmaps(self.id, backend::TextureKind::D3);
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
        wrap_r: Wrapping,
        callback: impl FnOnce(&Self),
    ) -> Self {
        Self::new_internal::<T>(
            context, width, height, depth, min_filter, mag_filter, mipmap, wrap_s, wrap_t, wrap_r,
            false, callback,
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
        wrap_r: Wrapping,
        allocate: bool,
        callback: impl FnOnce(&Self),
    ) -> Self {
        let number_of_mip_maps =
            calculate_number_of_mip_maps::<T>(mipmap, width, height, Some(depth));
        let id = create_texture::<T>(
            context,
            backend::TextureKind::D3,
            width,
            height,
            depth,
            number_of_mip_maps,
            min_filter,
            mag_filter,
            mipmap,
            wrap_s,
            wrap_t,
            Some(wrap_r),
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

impl Drop for Texture3D {
    fn drop(&mut self) {
        self.context.backend().delete_texture(self.id);
    }
}
