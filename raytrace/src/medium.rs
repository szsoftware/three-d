//!
//! Participating media: light absorbed and scattered on its way through tinted glass, water, smoke and haze.
//!

use crate::{vec3, InnerSpace, Vec3};

///
/// A homogeneous medium: how much light it absorbs and scatters per meter, and where the scattered light goes.
///
/// Inside a solid ([RtMaterial::medium](crate::RtMaterial::medium) with `thin_walled` false) it colors glass and water
/// with depth or fills a closed mesh with smoke or fog; as [RtFog] it is the haze over the whole scene.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RtMedium {
    /// Absorption coefficient per meter for red, green and blue.
    pub absorption: Vec3,
    /// Scattering coefficient per meter for red, green and blue.
    pub scattering: Vec3,
    /// Mean cosine of the scattering angle (Henyey-Greenstein): 0 scatters evenly, towards 1 forward as in haze and
    /// clouds, below 0 backward.
    pub anisotropy: f32,
}

impl RtMedium {
    ///
    /// A medium that only absorbs, as glTF's `KHR_materials_volume` describes it: white light keeps the color after
    /// the distance in meters.
    ///
    pub fn absorbing(color: Vec3, distance: f32) -> Self {
        let a = |c: f32| -c.clamp(1e-6, 1.0).ln() / distance.max(1e-6);
        Self {
            absorption: vec3(a(color.x), a(color.y), a(color.z)),
            scattering: vec3(0.0, 0.0, 0.0),
            anisotropy: 0.0,
        }
    }

    ///
    /// A white, scattering medium such as fog or smoke in which half the light along a line of sight is left after
    /// `half_distance` meters. `albedo` is the part of the light that is scattered instead of absorbed.
    ///
    pub fn fog(half_distance: f32, albedo: f32, anisotropy: f32) -> Self {
        let extinction = std::f32::consts::LN_2 / half_distance.max(1e-6);
        let albedo = albedo.clamp(0.0, 1.0);
        Self {
            absorption: vec3(1.0, 1.0, 1.0) * extinction * (1.0 - albedo),
            scattering: vec3(1.0, 1.0, 1.0) * extinction * albedo,
            anisotropy: anisotropy.clamp(-0.99, 0.99),
        }
    }

    /// Light taken out of a ray per meter, by absorption and scattering.
    pub fn extinction(&self) -> Vec3 {
        self.absorption + self.scattering
    }

    /// Whether the medium changes light at all.
    pub fn is_clear(&self) -> bool {
        self.extinction() == vec3(0.0, 0.0, 0.0)
    }
}

///
/// Haze or fog over the whole scene, thinning out with height, so the sky stays visible: the medium at
/// `height` (meters, y up), with its density falling by e every `scale_height` meters above it and rising below it.
/// The air of a clear day has a scale height of about 1200 m for haze; ground fog has a few meters.
///
/// Outside of closed meshes with their own [RtMaterial::medium](crate::RtMaterial::medium). The camera must be in the
/// fog, not in such a mesh.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RtFog {
    /// The medium at `height`.
    pub medium: RtMedium,
    /// Height in meters at which the medium has its coefficients.
    pub height: f32,
    /// Meters over which the density falls by e upwards, `f32::INFINITY` for the same everywhere.
    pub scale_height: f32,
}

impl RtFog {
    /// The density relative to the medium at a height.
    pub(crate) fn density(&self, y: f32) -> f32 {
        if self.scale_height.is_finite() {
            (-(y - self.height) / self.scale_height).exp().min(1e12)
        } else {
            1.0
        }
    }

    ///
    /// The density integrated along the ray from 0 to `t` in meters ("optical depth" per unit coefficient). Infinite
    /// where the ray goes on forever without the density falling.
    ///
    pub(crate) fn depth(&self, origin: Vec3, direction: Vec3, t: f32) -> f32 {
        let rho = self.density(origin.y);
        let k = direction.y / self.scale_height;
        if k.abs() < 1e-7 {
            return rho * t;
        }
        if t.is_infinite() {
            return if k > 0.0 { rho / k } else { f32::INFINITY };
        }
        // ∫ ρ e^(-k s) ds = ρ (1 - e^(-k t)) / k
        rho * -(-k * t).exp_m1() / k
    }

    ///
    /// The distance along the ray at which the integrated density reaches `depth`, `None` if it never does.
    ///
    pub(crate) fn distance_to_depth(
        &self,
        origin: Vec3,
        direction: Vec3,
        depth: f32,
    ) -> Option<f32> {
        let rho = self.density(origin.y);
        if rho <= 0.0 {
            return None;
        }
        let k = direction.y / self.scale_height;
        if k.abs() < 1e-7 {
            return Some(depth / rho);
        }
        // ρ (1 - e^(-k t)) / k = depth
        let x = 1.0 - depth * k / rho;
        if x <= 0.0 {
            return None;
        }
        Some(-x.ln() / k)
    }
}

/// Henyey-Greenstein phase function per steradian, for the cosine between the direction the light travels before and
/// after scattering.
pub(crate) fn phase(g: f32, cos: f32) -> f32 {
    let d = 1.0 + g * g - 2.0 * g * cos;
    (1.0 - g * g) / (4.0 * std::f32::consts::PI * d * d.sqrt())
}

/// A direction after scattering from `forward` (the direction the light traveled), drawn from the phase function, which
/// is also its density.
pub(crate) fn sample_phase(g: f32, forward: Vec3, u1: f32, u2: f32) -> Vec3 {
    let cos = if g.abs() < 1e-3 {
        1.0 - 2.0 * u1
    } else {
        let s = (1.0 - g * g) / (1.0 - g + 2.0 * g * u1);
        ((1.0 + g * g - s * s) / (2.0 * g)).clamp(-1.0, 1.0)
    };
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    let phi = 2.0 * std::f32::consts::PI * u2;
    let (t, b) = crate::sampling::orthonormal_basis(forward);
    (t * (sin * phi.cos()) + b * (sin * phi.sin()) + forward * cos).normalize()
}

/// Componentwise e^(-x).
pub(crate) fn exp_neg(x: Vec3) -> Vec3 {
    vec3((-x.x).exp(), (-x.y).exp(), (-x.z).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phase_function_integrates_to_one_and_samples_itself() {
        for g in [-0.5, 0.0, 0.3, 0.9] {
            let n = 20_000;
            let integral: f32 = (0..n)
                .map(|i| {
                    let cos = -1.0 + 2.0 * (i as f32 + 0.5) / n as f32;
                    phase(g, cos) * 2.0 * std::f32::consts::PI * 2.0 / n as f32
                })
                .sum();
            assert!((integral - 1.0).abs() < 1e-3, "{g}: {integral}");
            // The mean cosine of the samples is g
            let forward = vec3(0.0, 0.0, 1.0);
            let m = 200;
            let mean: f32 = (0..m * m)
                .map(|i| {
                    let (u1, u2) = (
                        ((i / m) as f32 + 0.5) / m as f32,
                        ((i % m) as f32 + 0.5) / m as f32,
                    );
                    sample_phase(g, forward, u1, u2).dot(forward)
                })
                .sum::<f32>()
                / (m * m) as f32;
            assert!((mean - g).abs() < 2e-3, "{g}: {mean}");
        }
    }

    #[test]
    fn fog_depth_and_distance_are_inverse() {
        let fog = RtFog {
            medium: RtMedium::fog(100.0, 0.9, 0.0),
            height: 0.0,
            scale_height: 50.0,
        };
        let origin = vec3(0.0, 10.0, 0.0);
        for direction in [
            vec3(0.0, 1.0, 0.0),
            vec3(1.0, -0.3, 0.0).normalize(),
            vec3(1.0, 0.0, 0.0),
        ] {
            let t = 37.0;
            let depth = fog.depth(origin, direction, t);
            let back = fog.distance_to_depth(origin, direction, depth).unwrap();
            assert!((back - t).abs() < 1e-3 * t, "{direction:?}: {back}");
            // Numerically
            let steps = 10_000;
            let sum: f32 = (0..steps)
                .map(|i| {
                    fog.density((origin + direction * ((i as f32 + 0.5) * t / steps as f32)).y) * t
                        / steps as f32
                })
                .sum();
            assert!(
                (sum - depth).abs() < 1e-3 * depth,
                "{direction:?}: {sum} {depth}"
            );
        }
        // Upwards the fog thins out to a finite depth
        let up = fog.depth(origin, vec3(0.0, 1.0, 0.0), f32::INFINITY);
        assert!((up - fog.density(10.0) * 50.0).abs() < 1e-3);
        assert!(fog
            .distance_to_depth(origin, vec3(0.0, 1.0, 0.0), up * 1.01)
            .is_none());
    }

    #[test]
    fn an_absorbing_medium_keeps_the_color_after_the_distance() {
        let m = RtMedium::absorbing(vec3(0.5, 0.25, 1.0), 2.0);
        let left = exp_neg(m.extinction() * 2.0);
        assert!((left - vec3(0.5, 0.25, 1.0)).magnitude() < 1e-5, "{left:?}");
    }
}
