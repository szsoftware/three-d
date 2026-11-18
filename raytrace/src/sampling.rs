//!
//! Random numbers and sampling helpers shared by the tracers.
//!

use std::f32::consts::PI;
use three_d_asset::prelude::*;

///
/// Random numbers for one pixel sample. The first [SOBOL_DIMENSIONS] numbers come from an Owen scrambled Sobol
/// sequence over the samples of the pixel, in pairs shuffled against each other (Burley 2020), so the samples of a
/// pixel spread evenly over the pixel, the lights and the first bounces. The numbers after that, and all numbers of
/// generators made by [Rng::new], come from a small PCG32 generator. Each pixel sample gets its own stream, so images
/// do not depend on how the work is split between threads.
///
#[derive(Clone, Debug)]
pub(crate) struct Rng {
    state: u64,
    inc: u64,
    /// Scrambling of the Sobol sequence for this pixel, `None` for plain random numbers.
    pixel: Option<u32>,
    /// The sample of the pixel, the index into the Sobol sequence.
    index: u32,
    /// How many numbers were drawn.
    dimension: u32,
}

/// How many numbers of a pixel sample come from the Sobol sequence.
pub(crate) const SOBOL_DIMENSIONS: u32 = 16;

impl Rng {
    pub fn new(seed: u32, stream: u64) -> Self {
        let mut rng = Self {
            state: 0,
            inc: (stream << 1) | 1,
            pixel: None,
            index: 0,
            dimension: 0,
        };
        rng.next_pcg();
        rng.state = rng.state.wrapping_add(seed as u64 ^ 0x853c_49e6_748f_ea9b);
        rng.next_pcg();
        rng
    }

    /// The numbers for one sample of one pixel.
    pub fn for_pixel(seed: u32, x: u32, y: u32, sample: u32) -> Self {
        let stream = ((y as u64) << 40) ^ ((x as u64) << 20) ^ (sample as u64);
        let mut rng = Self::new(seed ^ sample.wrapping_mul(0x9e37_79b9), stream);
        rng.pixel = Some(hash(seed ^ hash(x ^ hash(y))));
        rng.index = sample;
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let d = self.dimension;
        self.dimension = d.saturating_add(1);
        match self.pixel {
            Some(pixel) if d < SOBOL_DIMENSIONS => {
                sobol_pair(self.index, hash(pixel ^ hash(d / 2)))[(d % 2) as usize]
            }
            _ => self.next_pcg(),
        }
    }

    fn next_pcg(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// A number in [0, 1).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
}

/// A 32 bit hash (PCG output permutation), the same as `pcg_hash` in the compute shader.
pub(crate) fn hash(v: u32) -> u32 {
    let s = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

/// The first two Sobol dimensions at a shuffled index, both Owen scrambled, for the scrambling `seed`.
pub(crate) fn sobol_pair(index: u32, seed: u32) -> [u32; 2] {
    let i = nested_uniform_scramble(index, seed);
    let mut y = 0u32;
    let mut v = 1u32 << 31;
    let mut bits = i;
    while bits != 0 {
        if bits & 1 != 0 {
            y ^= v;
        }
        bits >>= 1;
        v ^= v >> 1;
    }
    [
        nested_uniform_scramble(i.reverse_bits(), hash(seed ^ 0x5bd1_e995)),
        nested_uniform_scramble(y, hash(seed ^ 0x68e3_1da4)),
    ]
}

/// Owen scrambling by the hash of Laine and Karras, as improved by Burley.
fn nested_uniform_scramble(x: u32, seed: u32) -> u32 {
    let mut x = x.reverse_bits();
    x = x.wrapping_add(seed);
    x ^= x.wrapping_mul(0x6c50_b47c);
    x ^= x.wrapping_mul(0xb82f_1e52);
    x ^= x.wrapping_mul(0xc7af_e638);
    x ^= x.wrapping_mul(0x8d22_f6e6);
    x.reverse_bits()
}

/// Two unit vectors that form a right handed frame with `n` (Duff et al. 2017).
pub(crate) fn orthonormal_basis(n: Vec3) -> (Vec3, Vec3) {
    let sign = 1.0f32.copysign(n.z);
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    (
        vec3(1.0 + sign * n.x * n.x * a, sign * b, -sign * n.x),
        vec3(b, sign + n.y * n.y * a, -n.y),
    )
}

/// A direction around +z with a density proportional to the cosine.
pub(crate) fn cosine_hemisphere(u1: f32, u2: f32) -> Vec3 {
    let r = u1.sqrt();
    let phi = 2.0 * PI * u2;
    vec3(r * phi.cos(), r * phi.sin(), (1.0 - u1).max(0.0).sqrt())
}

/// A point on the unit disc, uniform in area, mapping squares to rings so strata stay compact (Shirley and Chiu).
pub(crate) fn concentric_disc(u1: f32, u2: f32) -> (f32, f32) {
    let a = 2.0 * u1 - 1.0;
    let b = 2.0 * u2 - 1.0;
    if a == 0.0 && b == 0.0 {
        return (0.0, 0.0);
    }
    let (r, phi) = if a.abs() > b.abs() {
        (a, PI / 4.0 * (b / a))
    } else {
        (b, PI / 2.0 - PI / 4.0 * (a / b))
    };
    (r * phi.cos(), r * phi.sin())
}

/// A direction in the cone around +z with the given cosine of the half angle, uniform in solid angle.
pub(crate) fn uniform_cone(u1: f32, u2: f32, cos_max: f32) -> Vec3 {
    let cos_theta = 1.0 - u1 * (1.0 - cos_max);
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    let phi = 2.0 * PI * u2;
    vec3(sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta)
}

/// GGX normal distribution for the cosine between normal and half vector.
pub(crate) fn ggx_d(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    a2 / (PI * d * d)
}

/// Smith masking for one direction with the cosine to the normal.
pub(crate) fn ggx_g1(n_dot_x: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    2.0 * n_dot_x / (n_dot_x + (a2 + (1.0 - a2) * n_dot_x * n_dot_x).sqrt())
}

/// Height correlated Smith visibility, G / (4 n·l n·v).
pub(crate) fn ggx_visibility(n_dot_l: f32, n_dot_v: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let gv = n_dot_l * (n_dot_v * n_dot_v * (1.0 - a2) + a2).sqrt();
    let gl = n_dot_v * (n_dot_l * n_dot_l * (1.0 - a2) + a2).sqrt();
    0.5 / (gv + gl).max(1e-12)
}

/// A half vector from the GGX distribution of visible normals (Heitz 2018), in the frame where the normal is +z.
pub(crate) fn ggx_sample_visible_normal(v: Vec3, alpha: f32, u1: f32, u2: f32) -> Vec3 {
    let vh = vec3(alpha * v.x, alpha * v.y, v.z).normalize();
    let len_sq = vh.x * vh.x + vh.y * vh.y;
    let t1 = if len_sq > 0.0 {
        vec3(-vh.y, vh.x, 0.0) / len_sq.sqrt()
    } else {
        vec3(1.0, 0.0, 0.0)
    };
    let t2 = vh.cross(t1);
    let r = u1.sqrt();
    let phi = 2.0 * PI * u2;
    let p1 = r * phi.cos();
    let mut p2 = r * phi.sin();
    let s = 0.5 * (1.0 + vh.z);
    p2 = (1.0 - s) * (1.0 - p1 * p1).max(0.0).sqrt() + s * p2;
    let nh = p1 * t1 + p2 * t2 + (1.0 - p1 * p1 - p2 * p2).max(0.0).sqrt() * vh;
    vec3(alpha * nh.x, alpha * nh.y, nh.z.max(1e-6)).normalize()
}

/// Schlick's approximation of the Fresnel term, rising from `f0` at normal incidence to `f90` at grazing angles.
pub(crate) fn fresnel_schlick(f0: Vec3, f90: f32, cos_theta: f32) -> Vec3 {
    let m = (1.0 - cos_theta).clamp(0.0, 1.0);
    let m5 = m * m * m * m * m;
    f0 + (vec3(f90, f90, f90) - f0) * m5
}

/// Fresnel reflectance of an unpolarised ray at a dielectric boundary, `eta` = n_inside / n_outside as seen by the ray.
pub(crate) fn fresnel_dielectric(cos_i: f32, eta: f32) -> f32 {
    let cos_i = cos_i.clamp(0.0, 1.0);
    let sin_t2 = (1.0 - cos_i * cos_i) / (eta * eta);
    if sin_t2 >= 1.0 {
        return 1.0;
    }
    let cos_t = (1.0 - sin_t2).sqrt();
    let rs = (cos_i - eta * cos_t) / (cos_i + eta * cos_t);
    let rp = (eta * cos_i - cos_t) / (eta * cos_i + cos_t);
    0.5 * (rs * rs + rp * rp)
}

/// Relative luminance of a linear sRGB color.
pub(crate) fn luminance(c: Vec3) -> f32 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_uniform_enough() {
        let mut rng = Rng::new(7, 3);
        let n = 100_000;
        let mean: f32 = (0..n).map(|_| rng.next_f32()).sum::<f32>() / n as f32;
        assert!((mean - 0.5).abs() < 0.01, "mean {mean}");
    }

    #[test]
    fn basis_is_orthonormal() {
        for n in [
            vec3(0.0, 0.0, 1.0),
            vec3(0.0, 0.0, -1.0),
            vec3(1.0, 2.0, 3.0).normalize(),
            vec3(-0.3, 0.1, -0.9).normalize(),
        ] {
            let (t, b) = orthonormal_basis(n);
            assert!(t.dot(n).abs() < 1e-5 && b.dot(n).abs() < 1e-5 && t.dot(b).abs() < 1e-5);
            assert!((t.magnitude() - 1.0).abs() < 1e-5 && (b.magnitude() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn ggx_distribution_is_normalised() {
        // The projected normal distribution integrates to one over the hemisphere
        for alpha in [0.1f32, 0.5, 1.0] {
            let steps = 2000;
            let mut sum = 0.0;
            for i in 0..steps {
                let cos_t = (i as f32 + 0.5) / steps as f32;
                sum += ggx_d(cos_t, alpha) * cos_t * 2.0 * PI / steps as f32;
            }
            assert!((sum - 1.0).abs() < 0.02, "alpha {alpha}: {sum}");
        }
    }

    #[test]
    fn fresnel_of_glass_at_normal_incidence() {
        assert!((fresnel_dielectric(1.0, 1.5) - 0.04).abs() < 1e-4);
        assert_eq!(fresnel_dielectric(0.1, 1.0 / 1.5), 1.0);
    }
}

#[cfg(test)]
mod sobol_tests {
    use super::*;

    #[test]
    fn sobol_samples_fill_the_strata() {
        // 16 samples of a pixel: one in each of 16 rows, 16 columns and 4 × 4 squares
        let mut rows = [0; 16];
        let mut columns = [0; 16];
        let mut squares = [0; 16];
        for sample in 0..16 {
            let mut rng = Rng::for_pixel(7, 3, 5, sample);
            let (u, v) = (rng.next_f32(), rng.next_f32());
            rows[(v * 16.0) as usize] += 1;
            columns[(u * 16.0) as usize] += 1;
            squares[(v * 4.0) as usize * 4 + (u * 4.0) as usize] += 1;
        }
        assert_eq!(rows, [1; 16]);
        assert_eq!(columns, [1; 16]);
        assert_eq!(squares, [1; 16]);
        // Other pixels are scrambled differently
        let a = Rng::for_pixel(7, 3, 5, 0).next_u32();
        let b = Rng::for_pixel(7, 4, 5, 0).next_u32();
        assert_ne!(a, b);
    }
}
