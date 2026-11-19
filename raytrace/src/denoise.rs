//!
//! Denoising of render images with few samples.
//!

use crate::sampling::luminance;
use crate::{RenderImage, RtError};
use three_d_asset::prelude::*;

///
/// Turns a noisy render image into a smooth one. The result holds the averages as one sample.
///
pub trait Denoiser {
    /// The denoised image. The render image needs features, see [RenderImage::with_features].
    fn denoise(&mut self, image: &RenderImage) -> Result<RenderImage, RtError>;
}

///
/// An edge avoiding à-trous wavelet filter like the spatial part of SVGF (Schied et al. 2017): the light reaching each
/// surface (the color divided by the albedo) is blurred over growing distances, but only between pixels with similar
/// normals and distances, and only as far as the variance of the samples says the differences are noise. Textures stay
/// sharp because the albedo is multiplied back afterwards, and the filter gets weaker as samples are added.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtrousDenoiser {
    /// Passes with the step doubled each time; five reach 2 × (1 + 2 + 4 + 8 + 16) = 62 pixels.
    pub iterations: u32,
    /// How many standard deviations of the noise a luminance difference may be to still blur.
    pub sigma_luminance: f32,
    /// Exponent of the cosine between normals; higher keeps edges between faces sharper.
    pub sigma_normal: f32,
    /// How many times the change of distance between neighbouring pixels a difference may be to still blur.
    pub sigma_distance: f32,
}

impl Default for AtrousDenoiser {
    fn default() -> Self {
        Self {
            iterations: 5,
            sigma_luminance: 4.0,
            sigma_normal: 128.0,
            sigma_distance: 1.0,
        }
    }
}

/// B3 spline weights of the 5 × 5 kernel.
const KERNEL: [f32; 5] = [1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0];

/// The smallest albedo the light is divided by.
const MIN_ALBEDO: f32 = 0.02;

/// The per pixel inputs of the filter.
pub(crate) struct Guide {
    pub width: usize,
    pub height: usize,
    pub albedo: Vec<Vec3>,
    pub normal: Vec<Vec3>,
    pub distance: Vec<f32>,
    /// How much the distance changes towards the neighbours.
    pub gradient: Vec<f32>,
}

impl AtrousDenoiser {
    /// Light per surface, its variance and the guide.
    pub(crate) fn prepare(image: &RenderImage) -> Result<(Vec<Vec3>, Vec<f32>, Guide), RtError> {
        image.check_not_empty()?;
        if !image.has_features() {
            return Err(RtError::NoFeatures);
        }
        let (width, height) = (image.width() as usize, image.height() as usize);
        let n = image.sample_count().max(1) as f32;
        let count = width * height;
        let mut light = Vec::with_capacity(count);
        let mut variance = Vec::with_capacity(count);
        let mut albedo = Vec::with_capacity(count);
        let mut normal = Vec::with_capacity(count);
        let mut distance = Vec::with_capacity(count);
        for y in 0..height as u32 {
            for x in 0..width as u32 {
                let c = image.pixel(x, y);
                let f = image.features(x, y).expect("features");
                let a = vec3(
                    f.albedo.x.max(MIN_ALBEDO),
                    f.albedo.y.max(MIN_ALBEDO),
                    f.albedo.z.max(MIN_ALBEDO),
                );
                light.push(vec3(c.x / a.x, c.y / a.y, c.z / a.z));
                // Variance of the average from the samples' own spread, in units of the light per surface
                let lum = luminance(c);
                let v = if n > 1.0 {
                    (f.luminance_squared - lum * lum).max(0.0) / (n - 1.0)
                } else {
                    0.0
                };
                variance.push(v / luminance(a).max(MIN_ALBEDO).powi(2));
                albedo.push(a);
                normal.push(if f.normal.magnitude2() > 0.0 {
                    f.normal.normalize()
                } else {
                    f.normal
                });
                distance.push(f.distance);
            }
        }
        let gradient = distance_gradient(width, height, &distance);
        let guide = Guide {
            width,
            height,
            albedo,
            normal,
            distance,
            gradient,
        };
        if n < 4.0 {
            // Too few samples for their spread: take the spread among similar neighbours instead
            variance = spatial_variance(&light, &guide, n);
        }
        Ok((light, variance, guide))
    }

    /// One pass with the given step, for row `y` of the image.
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        step: i32,
        light: &[Vec3],
        variance: &[f32],
        blurred_variance: &[f32],
        guide: &Guide,
        y: usize,
        out_light: &mut [Vec3],
        out_variance: &mut [f32],
    ) {
        let (w, h) = (guide.width as i32, guide.height as i32);
        for x in 0..guide.width {
            let p = y * guide.width + x;
            let lp = luminance(light[p]);
            let np = guide.normal[p];
            let zp = guide.distance[p];
            let gp = guide.gradient[p];
            let sigma_l = self.sigma_luminance * blurred_variance[p].max(0.0).sqrt() + 1e-10;
            let mut sum_w = 0.0;
            let mut sum_l = vec3(0.0, 0.0, 0.0);
            let mut sum_v = 0.0;
            for (j, ky) in KERNEL.iter().enumerate() {
                let qy = y as i32 + (j as i32 - 2) * step;
                if qy < 0 || qy >= h {
                    continue;
                }
                for (i, kx) in KERNEL.iter().enumerate() {
                    let qx = x as i32 + (i as i32 - 2) * step;
                    if qx < 0 || qx >= w {
                        continue;
                    }
                    let q = qy as usize * guide.width + qx as usize;
                    let mut weight = kx * ky;
                    if q != p {
                        let cos = np.dot(guide.normal[q]).max(0.0);
                        let wn = if self.sigma_normal.fract() == 0.0 {
                            cos.powi(self.sigma_normal as i32)
                        } else {
                            cos.powf(self.sigma_normal)
                        };
                        let offset = (((i as i32 - 2).pow(2) + (j as i32 - 2).pow(2)) as f32)
                            .sqrt()
                            * step as f32;
                        let wz = (-(zp - guide.distance[q]).abs()
                            / (self.sigma_distance * gp * offset + 1e-4 * zp + 1e-6))
                            .exp();
                        let wl = (-(lp - luminance(light[q])).abs() / sigma_l).exp();
                        weight *= wn * wz * wl;
                    }
                    sum_w += weight;
                    sum_l += light[q] * weight;
                    sum_v += variance[q] * weight * weight;
                }
            }
            out_light[x] = sum_l / sum_w;
            out_variance[x] = sum_v / (sum_w * sum_w);
        }
    }

    /// Filters the light per surface in place.
    pub(crate) fn filter(&self, light: &mut Vec<Vec3>, variance: &mut Vec<f32>, guide: &Guide) {
        let width = guide.width;
        for i in 0..self.iterations.min(16) {
            let step = 1i32 << i;
            let blurred = blur3(variance, guide.width, guide.height);
            let mut next_light = vec![vec3(0.0, 0.0, 0.0); light.len()];
            let mut next_variance = vec![0.0; variance.len()];
            par_rows(
                &mut next_light,
                &mut next_variance,
                width,
                |y, out_l, out_v| {
                    self.pass(step, light, variance, &blurred, guide, y, out_l, out_v)
                },
            );
            *light = next_light;
            *variance = next_variance;
        }
    }
}

impl Denoiser for AtrousDenoiser {
    fn denoise(&mut self, image: &RenderImage) -> Result<RenderImage, RtError> {
        let (mut light, mut variance, guide) = Self::prepare(image)?;
        self.filter(&mut light, &mut variance, &guide);
        let pixels = light
            .iter()
            .zip(&guide.albedo)
            .map(|(l, a)| [l.x * a.x, l.y * a.y, l.z * a.z])
            .collect();
        Ok(RenderImage::from_averages(
            image.width(),
            image.height(),
            pixels,
        ))
    }
}

/// How much the distance changes to the next pixel, the smaller of the two sides, the larger of the two axes.
fn distance_gradient(width: usize, height: usize, distance: &[f32]) -> Vec<f32> {
    let at = |x: usize, y: usize| distance[y * width + x];
    let one_axis = |a: Option<f32>, p: f32, b: Option<f32>| match (a, b) {
        (Some(a), Some(b)) => (p - a).abs().min((b - p).abs()),
        (Some(a), None) => (p - a).abs(),
        (None, Some(b)) => (b - p).abs(),
        (None, None) => 0.0,
    };
    let mut g = Vec::with_capacity(distance.len());
    for y in 0..height {
        for x in 0..width {
            let p = at(x, y);
            let gx = one_axis(
                (x > 0).then(|| at(x - 1, y)),
                p,
                (x + 1 < width).then(|| at(x + 1, y)),
            );
            let gy = one_axis(
                (y > 0).then(|| at(x, y - 1)),
                p,
                (y + 1 < height).then(|| at(x, y + 1)),
            );
            g.push(gx.max(gy));
        }
    }
    g
}

/// The variance of the average of `n` samples estimated from the 5 × 5 neighbours on the same surface.
fn spatial_variance(light: &[Vec3], guide: &Guide, n: f32) -> Vec<f32> {
    let (w, h) = (guide.width as i32, guide.height as i32);
    let mut result = Vec::with_capacity(light.len());
    for y in 0..h {
        for x in 0..w {
            let p = (y * w + x) as usize;
            let (mut sum, mut sum2, mut count) = (0.0, 0.0, 0.0);
            for qy in (y - 2).max(0)..=(y + 2).min(h - 1) {
                for qx in (x - 2).max(0)..=(x + 2).min(w - 1) {
                    let q = (qy * w + qx) as usize;
                    if guide.normal[p].dot(guide.normal[q]) < 0.9
                        || (guide.distance[p] - guide.distance[q]).abs()
                            > 0.05 * guide.distance[p].max(1e-6)
                    {
                        continue;
                    }
                    let l = luminance(light[q]);
                    sum += l;
                    sum2 += l * l;
                    count += 1.0;
                }
            }
            let mean = sum / count;
            result.push((sum2 / count - mean * mean).max(0.0) / n);
        }
    }
    result
}

/// A 3 × 3 Gaussian blur.
fn blur3(values: &[f32], width: usize, height: usize) -> Vec<f32> {
    const K: [f32; 3] = [0.25, 0.5, 0.25];
    let (w, h) = (width as i32, height as i32);
    let mut result = Vec::with_capacity(values.len());
    for y in 0..h {
        for x in 0..w {
            let (mut sum, mut weight) = (0.0, 0.0);
            for (j, ky) in K.iter().enumerate() {
                for (i, kx) in K.iter().enumerate() {
                    let (qx, qy) = (x + i as i32 - 1, y + j as i32 - 1);
                    if qx >= 0 && qx < w && qy >= 0 && qy < h {
                        sum += values[(qy * w + qx) as usize] * kx * ky;
                        weight += kx * ky;
                    }
                }
            }
            result.push(sum / weight);
        }
    }
    result
}

/// Runs `f` for every row, on all cores.
fn par_rows(
    light: &mut [Vec3],
    variance: &mut [f32],
    width: usize,
    f: impl Fn(usize, &mut [Vec3], &mut [f32]) + Sync,
) {
    let height = light.len() / width;
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(height)
        .max(1);
    if threads == 1 {
        // Also where there are no threads, as in the browser
        for (y, (l, v)) in light
            .chunks_mut(width)
            .zip(variance.chunks_mut(width))
            .enumerate()
        {
            f(y, l, v);
        }
        return;
    }
    let rows_per_thread = height.div_ceil(threads);
    std::thread::scope(|s| {
        for (chunk, (l, v)) in light
            .chunks_mut(rows_per_thread * width)
            .zip(variance.chunks_mut(rows_per_thread * width))
            .enumerate()
        {
            let f = &f;
            s.spawn(move || {
                for (r, (l, v)) in l.chunks_mut(width).zip(v.chunks_mut(width)).enumerate() {
                    f(chunk * rows_per_thread + r, l, v);
                }
            });
        }
    });
}
