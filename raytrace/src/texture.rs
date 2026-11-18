//!
//! Textures of materials, sampled by the tracers.
//!

use three_d_asset::prelude::*;
use three_d_asset::{Interpolation, Texture2D, TextureData, Wrapping};

/// Identifies a texture of an [RtScene](crate::RtScene).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub u32);

///
/// How texture coordinates outside of 0 to 1 are mapped into the texture.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RtWrapping {
    /// The texture repeats.
    #[default]
    Repeat,
    /// The texture repeats, every other copy mirrored.
    MirroredRepeat,
    /// The border texels continue.
    ClampToEdge,
}

impl From<Wrapping> for RtWrapping {
    fn from(w: Wrapping) -> Self {
        match w {
            Wrapping::Repeat => Self::Repeat,
            Wrapping::MirroredRepeat => Self::MirroredRepeat,
            Wrapping::ClampToEdge => Self::ClampToEdge,
        }
    }
}

///
/// A texture with four bytes per texel. The first row is at texture coordinate v = 0, as in three-d.
///
/// Colors (the base color and emission textures) are sRGB unless [RtTexture::linear] is set, all other textures hold
/// linear values. Textures with floats are stored clamped to 0 to 1 and marked linear.
///
#[derive(Clone, Debug, PartialEq)]
pub struct RtTexture {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Red, green, blue and alpha of every texel, row by row.
    pub texels: Vec<[u8; 4]>,
    /// The values are linear, also in color textures.
    pub linear: bool,
    /// Wrapping along u.
    pub wrap_u: RtWrapping,
    /// Wrapping along v.
    pub wrap_v: RtWrapping,
    /// Takes the nearest texel instead of interpolating between four.
    pub nearest: bool,
}

impl RtTexture {
    /// A texture from texels in sRGB, repeating and interpolated.
    pub fn new(width: u32, height: u32, texels: Vec<[u8; 4]>) -> Self {
        assert_eq!(
            texels.len(),
            (width * height) as usize,
            "one texel per pixel"
        );
        Self {
            width,
            height,
            texels,
            linear: false,
            wrap_u: RtWrapping::Repeat,
            wrap_v: RtWrapping::Repeat,
            nearest: false,
        }
    }

    ///
    /// Copies a texture of three-d (`CpuTexture` in three-d). Red alone becomes gray, red and green get blue zero.
    ///
    pub fn from_cpu_texture(texture: &Texture2D) -> Self {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let (texels, linear): (Vec<[u8; 4]>, bool) = match &texture.data {
            TextureData::RU8(d) => (d.iter().map(|&r| [r, r, r, 255]).collect(), false),
            TextureData::RgU8(d) => (d.iter().map(|&[r, g]| [r, g, 0, 255]).collect(), false),
            TextureData::RgbU8(d) => (d.iter().map(|&[r, g, b]| [r, g, b, 255]).collect(), false),
            TextureData::RgbaU8(d) => (d.clone(), false),
            TextureData::RF16(d) => (
                d.iter()
                    .map(|r| {
                        let r = byte(r.to_f32());
                        [r, r, r, 255]
                    })
                    .collect(),
                true,
            ),
            TextureData::RgF16(d) => (
                d.iter()
                    .map(|[r, g]| [byte(r.to_f32()), byte(g.to_f32()), 0, 255])
                    .collect(),
                true,
            ),
            TextureData::RgbF16(d) => (
                d.iter()
                    .map(|[r, g, b]| [byte(r.to_f32()), byte(g.to_f32()), byte(b.to_f32()), 255])
                    .collect(),
                true,
            ),
            TextureData::RgbaF16(d) => (
                d.iter()
                    .map(|[r, g, b, a]| {
                        [
                            byte(r.to_f32()),
                            byte(g.to_f32()),
                            byte(b.to_f32()),
                            byte(a.to_f32()),
                        ]
                    })
                    .collect(),
                true,
            ),
            TextureData::RF32(d) => (
                d.iter()
                    .map(|&r| {
                        let r = byte(r);
                        [r, r, r, 255]
                    })
                    .collect(),
                true,
            ),
            TextureData::RgF32(d) => (
                d.iter().map(|&[r, g]| [byte(r), byte(g), 0, 255]).collect(),
                true,
            ),
            TextureData::RgbF32(d) => (
                d.iter()
                    .map(|&[r, g, b]| [byte(r), byte(g), byte(b), 255])
                    .collect(),
                true,
            ),
            TextureData::RgbaF32(d) => (
                d.iter()
                    .map(|&[r, g, b, a]| [byte(r), byte(g), byte(b), byte(a)])
                    .collect(),
                true,
            ),
        };
        let (width, height) = if texels.len() == (texture.width * texture.height) as usize {
            (texture.width, texture.height)
        } else {
            // Data that does not fit the size: one white texel instead of reading out of range
            return Self::new(1, 1, vec![[255; 4]]);
        };
        Self {
            width,
            height,
            texels,
            linear,
            wrap_u: texture.wrap_s.into(),
            wrap_v: texture.wrap_t.into(),
            nearest: texture.mag_filter == Interpolation::Nearest,
        }
    }

    /// The values at texture coordinates, from 0 to 1 and without decoding sRGB.
    pub fn sample(&self, uv: Vec2) -> Vec4 {
        self.sample_with(uv, |t| {
            vec4(t[0] as f32, t[1] as f32, t[2] as f32, t[3] as f32) / 255.0
        })
    }

    /// The color at texture coordinates, linear, with alpha.
    pub fn sample_color(&self, uv: Vec2) -> Vec4 {
        if self.linear {
            return self.sample(uv);
        }
        self.sample_with(uv, |t| {
            vec4(
                srgb_to_linear(t[0]),
                srgb_to_linear(t[1]),
                srgb_to_linear(t[2]),
                t[3] as f32 / 255.0,
            )
        })
    }

    fn sample_with(&self, uv: Vec2, decode: impl Fn(&[u8; 4]) -> Vec4) -> Vec4 {
        if self.width == 0 || self.height == 0 {
            return vec4(1.0, 1.0, 1.0, 1.0);
        }
        let texel = |x: i64, y: i64| {
            let x = wrap(x, self.width, self.wrap_u);
            let y = wrap(y, self.height, self.wrap_v);
            decode(&self.texels[(y * self.width + x) as usize])
        };
        let x = uv.x * self.width as f32;
        let y = uv.y * self.height as f32;
        if self.nearest {
            return texel(x.floor() as i64, y.floor() as i64);
        }
        // Texel centers are at half texels
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let top = texel(x0, y0) * (1.0 - fx) + texel(x0 + 1, y0) * fx;
        let bottom = texel(x0, y0 + 1) * (1.0 - fx) + texel(x0 + 1, y0 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// A texel index inside the texture.
fn wrap(i: i64, size: u32, wrapping: RtWrapping) -> u32 {
    let n = size as i64;
    (match wrapping {
        RtWrapping::Repeat => i.rem_euclid(n),
        RtWrapping::ClampToEdge => i.clamp(0, n - 1),
        RtWrapping::MirroredRepeat => {
            let m = i.rem_euclid(2 * n);
            if m < n {
                m
            } else {
                2 * n - 1 - m
            }
        }
    }) as u32
}

/// An sRGB byte as a linear value.
pub(crate) fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker() -> RtTexture {
        RtTexture::new(
            2,
            2,
            vec![
                [0, 0, 0, 255],
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 0],
            ],
        )
    }

    #[test]
    fn texel_centers_give_the_texel() {
        let t = checker();
        assert_eq!(t.sample(vec2(0.25, 0.25)), vec4(0.0, 0.0, 0.0, 1.0));
        assert_eq!(t.sample(vec2(0.75, 0.25)), vec4(1.0, 0.0, 0.0, 1.0));
        // Row 0 is at v = 0
        assert_eq!(t.sample(vec2(0.25, 0.75)), vec4(0.0, 1.0, 0.0, 1.0));
        assert_eq!(t.sample(vec2(0.75, 0.75)), vec4(0.0, 0.0, 1.0, 0.0));
    }

    #[test]
    fn interpolates_and_wraps() {
        let t = checker();
        // Between the two texels of the first row
        let p = t.sample(vec2(0.5, 0.25));
        assert!((p.x - 0.5).abs() < 1e-6, "{p:?}");
        // Repeat: the left border mixes with the right column
        let p = t.sample(vec2(0.0, 0.25));
        assert!((p.x - 0.5).abs() < 1e-6, "{p:?}");
        // Clamp: the left border is the left texel
        let clamped = RtTexture {
            wrap_u: RtWrapping::ClampToEdge,
            ..checker()
        };
        assert_eq!(clamped.sample(vec2(0.0, 0.25)).x, 0.0);
        assert_eq!(wrap(-1, 4, RtWrapping::MirroredRepeat), 0);
        assert_eq!(wrap(4, 4, RtWrapping::MirroredRepeat), 3);
        assert_eq!(wrap(-1, 4, RtWrapping::Repeat), 3);
    }

    #[test]
    fn colors_are_decoded() {
        let t = RtTexture {
            nearest: true,
            ..RtTexture::new(1, 1, vec![[188, 188, 188, 128]])
        };
        let c = t.sample_color(vec2(0.5, 0.5));
        assert!((c.x - 0.5029).abs() < 1e-3, "{c:?}");
        assert!((c.w - 128.0 / 255.0).abs() < 1e-6);
    }
}
