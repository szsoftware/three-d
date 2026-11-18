//!
//! Showing a render image with three-d, on any of its backends.
//!

use crate::RenderImage;
use three_d::{Context, Interpolation, Texture2D, Wrapping};

impl RenderImage {
    ///
    /// A three-d texture with the linear average of every pixel in cd/m² (32 bit floats), for example to show it with a
    /// tone mapping of three-d or to combine it with the raster view. The bottom row of the image is at texture
    /// coordinate v = 0, as three-d expects.
    ///
    pub fn to_three_d_texture(&self, context: &Context) -> Texture2D {
        let texture = Texture2D::new_empty::<[f32; 4]>(
            context,
            self.width(),
            self.height(),
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        self.write_to_three_d_texture(&texture);
        texture
    }

    ///
    /// Writes the render image into a texture made by [RenderImage::to_three_d_texture], to show the image while samples are added.
    ///
    /// # Panics
    /// If the texture does not have the size of the render image.
    ///
    pub fn write_to_three_d_texture(&self, texture: &Texture2D) {
        assert!(
            texture.width() == self.width() && texture.height() == self.height(),
            "the texture has another size than the render_image"
        );
        texture.fill(&self.bottom_up_rgba());
    }

    /// The average of every pixel with alpha one, bottom row first.
    pub(crate) fn bottom_up_rgba(&self) -> Vec<[f32; 4]> {
        (0..self.height())
            .rev()
            .flat_map(|y| (0..self.width()).map(move |x| (x, y)))
            .map(|(x, y)| {
                let p = self.pixel(x, y);
                [p.x, p.y, p.z, 1.0]
            })
            .collect()
    }
}
