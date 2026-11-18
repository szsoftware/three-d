//!
//! Collecting samples into an image.
//!

use crate::RtError;
use three_d_asset::prelude::*;
use three_d_asset::{Texture2D, TextureData};

///
/// How the linear luminance of the render image becomes displayable colors.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToneMapping {
    /// Exposure value at ISO 100 as on a camera: about 15 for a sunny day outside, 11 for an overcast day, 7 for a
    /// lit room.
    pub exposure_ev100: f32,
    /// The curve that compresses bright values.
    pub operator: ToneOperator,
}

impl Default for ToneMapping {
    fn default() -> Self {
        Self {
            exposure_ev100: 15.0,
            operator: ToneOperator::Aces,
        }
    }
}

///
/// Curve from exposed linear values to displayable values.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToneOperator {
    /// Cut off at one.
    Clamp,
    /// x / (1 + x).
    Reinhard,
    /// The filmic curve of the ACES reference rendering (Narkowicz's fit).
    Aces,
}

impl ToneMapping {
    /// The factor from luminance in cd/m² to exposed values, as a camera with the exposure sees it.
    pub fn exposure_scale(&self) -> f32 {
        1.0 / (1.2 * 2f32.powf(self.exposure_ev100))
    }

    /// Exposed and tone mapped linear color, each channel from 0 to 1.
    pub fn apply(&self, radiance: Vec3) -> Vec3 {
        let c = radiance * self.exposure_scale();
        let f = |x: f32| -> f32 {
            let x = x.max(0.0);
            match self.operator {
                ToneOperator::Clamp => x.min(1.0),
                ToneOperator::Reinhard => x / (1.0 + x),
                ToneOperator::Aces => {
                    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
                }
            }
        };
        vec3(f(c.x), f(c.y), f(c.z))
    }
}

///
/// Sums of samples per pixel and how many samples each pixel has. Rows go from the top of the image to the bottom.
///
/// A tracer adds the same number of samples to every pixel per call, so the average of a pixel is its sum divided by
/// [RenderImage::sample_count].
///
#[derive(Clone, Debug)]
pub struct RenderImage {
    width: u32,
    height: u32,
    sum: Vec<[f32; 3]>,
    samples: u32,
}

impl RenderImage {
    /// A black render image with the given size.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            sum: vec![[0.0; 3]; (width * height) as usize],
            samples: 0,
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Forgets all samples, for example after the camera or the scene changed.
    pub fn reset(&mut self) {
        self.sum.iter_mut().for_each(|p| *p = [0.0; 3]);
        self.samples = 0;
    }

    /// Changes the size and forgets all samples.
    pub fn resize(&mut self, width: u32, height: u32) {
        *self = Self::new(width, height);
    }

    /// Number of samples every pixel has.
    pub fn sample_count(&self) -> u32 {
        self.samples
    }

    ///
    /// Adds sums of samples that were traced elsewhere, for example read back from a GPU: one sum per pixel, rows from the top,
    /// and the number of samples each sum holds.
    ///
    /// # Panics
    /// If the number of sums is not the number of pixels.
    ///
    pub fn accumulate(&mut self, sums: &[[f32; 3]], samples: u32) {
        assert_eq!(sums.len(), self.sum.len(), "one sum per pixel");
        for (s, a) in self.sum.iter_mut().zip(sums) {
            s[0] += a[0];
            s[1] += a[1];
            s[2] += a[2];
        }
        self.samples += samples;
    }

    /// Average luminance (linear sRGB, cd/m²) of a pixel, x from the left and y from the top.
    pub fn pixel(&self, x: u32, y: u32) -> Vec3 {
        let s = self.sum[(y * self.width + x) as usize];
        if self.samples == 0 {
            vec3(0.0, 0.0, 0.0)
        } else {
            vec3(s[0], s[1], s[2]) / self.samples as f32
        }
    }

    /// Average luminance of all pixels, row by row from the top.
    pub fn pixels(&self) -> Vec<Vec3> {
        (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .map(|(x, y)| self.pixel(x, y))
            .collect()
    }

    /// The linear average as a texture with 32 bit floats, first row at the top.
    pub fn to_hdr_texture(&self) -> Texture2D {
        Texture2D {
            name: "render image".to_owned(),
            data: TextureData::RgbaF32(
                self.pixels()
                    .into_iter()
                    .map(|p| [p.x, p.y, p.z, 1.0])
                    .collect(),
            ),
            width: self.width,
            height: self.height,
            ..Default::default()
        }
    }

    /// The tone mapped image as an sRGB texture with 8 bits per channel, first row at the top.
    pub fn to_ldr_texture(&self, tone_mapping: &ToneMapping) -> Texture2D {
        Texture2D {
            name: "render image".to_owned(),
            data: TextureData::RgbaU8(self.to_srgb8(tone_mapping)),
            width: self.width,
            height: self.height,
            ..Default::default()
        }
    }

    /// The tone mapped image as sRGB bytes, four per pixel, first row at the top.
    pub fn to_srgb8(&self, tone_mapping: &ToneMapping) -> Vec<[u8; 4]> {
        let encode = |c: f32| {
            let s = if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
        };
        self.pixels()
            .into_iter()
            .map(|p| {
                let c = tone_mapping.apply(p);
                [encode(c.x), encode(c.y), encode(c.z), 255]
            })
            .collect()
    }

    ///
    /// Writes the tone mapped image as PNG.
    ///
    #[cfg(feature = "export")]
    pub fn save_png(
        &self,
        path: impl AsRef<std::path::Path>,
        tone_mapping: &ToneMapping,
    ) -> Result<(), RtError> {
        self.check_not_empty()?;
        let bytes: Vec<u8> = self.to_srgb8(tone_mapping).into_iter().flatten().collect();
        image::RgbaImage::from_raw(self.width, self.height, bytes)
            .ok_or_else(|| RtError::Export("size does not match".to_owned()))?
            .save_with_format(path, image::ImageFormat::Png)
            .map_err(|e| RtError::Export(e.to_string()))
    }

    ///
    /// Writes the linear average in cd/m² as OpenEXR with 32 bit floats, for post processing elsewhere.
    ///
    #[cfg(feature = "export")]
    pub fn save_exr(&self, path: impl AsRef<std::path::Path>) -> Result<(), RtError> {
        self.check_not_empty()?;
        let data: Vec<f32> = self
            .pixels()
            .into_iter()
            .flat_map(|p| [p.x, p.y, p.z])
            .collect();
        image::Rgb32FImage::from_raw(self.width, self.height, data)
            .ok_or_else(|| RtError::Export("size does not match".to_owned()))?
            .save_with_format(path, image::ImageFormat::OpenExr)
            .map_err(|e| RtError::Export(e.to_string()))
    }

    pub(crate) fn check_not_empty(&self) -> Result<(), RtError> {
        if self.width == 0 || self.height == 0 {
            Err(RtError::EmptyImage)
        } else {
            Ok(())
        }
    }

    /// The sums of all pixels, row by row from the top, for a tracer to add to.
    pub(crate) fn rows_mut(&mut self) -> &mut [[f32; 3]] {
        &mut self.sum
    }

    pub(crate) fn add_samples(&mut self, count: u32) {
        self.samples += count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mid_grey_at_its_exposure() {
        // A surface that reflects 18 % of the light falls in the middle of the curve at the matching exposure
        let tm = ToneMapping {
            exposure_ev100: 15.0,
            operator: ToneOperator::Clamp,
        };
        let luminance_for_mid_grey = 0.18 / tm.exposure_scale();
        let c = tm.apply(vec3(1.0, 1.0, 1.0) * luminance_for_mid_grey);
        assert!((c.x - 0.18).abs() < 1e-4);
    }

    #[test]
    fn aces_stays_in_range() {
        let tm = ToneMapping::default();
        for l in [0.0, 1.0, 1e3, 1e6, 1e9] {
            let c = tm.apply(vec3(l, l, l));
            assert!(c.x >= 0.0 && c.x <= 1.0);
        }
    }

    #[test]
    fn reset_forgets_samples() {
        let mut render_image = RenderImage::new(2, 2);
        render_image.rows_mut()[3] = [4.0, 4.0, 4.0];
        render_image.add_samples(2);
        assert_eq!(render_image.pixel(1, 1), vec3(2.0, 2.0, 2.0));
        render_image.reset();
        assert_eq!(render_image.sample_count(), 0);
        assert_eq!(render_image.pixel(1, 1), vec3(0.0, 0.0, 0.0));
    }
}
