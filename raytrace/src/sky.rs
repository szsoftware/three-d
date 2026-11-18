//!
//! A daylight sky after Preetham, Shirley and Smits (1999) with the sun from [SunPosition].
//!

use crate::{GeoLocation, LocalDateTime, SceneOrientation, SunPosition};
use std::f32::consts::PI;
use three_d_asset::prelude::*;

/// Angular radius of the sun disc in radians (0.2666°).
const SUN_ANGULAR_RADIUS: f32 = 0.004_653;

/// Illuminance of the sun outside the atmosphere in lux.
const SOLAR_ILLUMINANCE_OUTSIDE: f32 = 128_000.0;

/// Elevation in degrees below which the sky has gone dark (end of civil twilight).
const TWILIGHT_END: f32 = -6.0;

///
/// A clear daylight sky with the sun, for a place, date and time or a given sun position.
///
/// The sky luminance follows the analytic model of Preetham et al., the sun is a disc of 0.53° whose illuminance and
/// color follow the air mass it shines through (Meinel's approximation), and below the horizon the ground reflects the
/// light falling on it. Light of the sun is only added by the tracers when they sample the sun on purpose, so
/// [Sky::radiance] does not contain the sun disc.
///
#[derive(Clone, Debug)]
pub struct Sky {
    sun: SunPosition,
    orientation: SceneOrientation,
    turbidity: f32,
    ground_albedo: Vec3,
    // Derived values
    sun_direction: Vec3,
    sun_illuminance: f32,
    sun_color: Vec3,
    perez: [[f32; 5]; 3],
    zenith: [f32; 3],
    norm: [f32; 3],
    sky_scale: f32,
    ground_luminance: Vec3,
}

impl Sky {
    ///
    /// The sky over a place at a local date and time, with a turbidity of 3 (clear) and grass-like ground.
    ///
    pub fn new(location: GeoLocation, time: LocalDateTime, orientation: SceneOrientation) -> Self {
        Self::from_sun_position(SunPosition::at(location, time), orientation)
    }

    /// The sky for a given sun position.
    pub fn from_sun_position(sun: SunPosition, orientation: SceneOrientation) -> Self {
        let mut sky = Self {
            sun,
            orientation,
            turbidity: 3.0,
            ground_albedo: vec3(0.2, 0.2, 0.2),
            sun_direction: vec3(0.0, 1.0, 0.0),
            sun_illuminance: 0.0,
            sun_color: vec3(1.0, 1.0, 1.0),
            perez: [[0.0; 5]; 3],
            zenith: [0.0; 3],
            norm: [1.0; 3],
            sky_scale: 1.0,
            ground_luminance: vec3(0.0, 0.0, 0.0),
        };
        sky.update();
        sky
    }

    ///
    /// Haze of the air, from 2 (very clear) to about 10 (hazy). Values below 2 are raised to 2, where the model ends.
    ///
    pub fn with_turbidity(mut self, turbidity: f32) -> Self {
        self.turbidity = turbidity.clamp(2.0, 12.0);
        self.update();
        self
    }

    /// Linear albedo of the ground beyond the scene, seen below the horizon.
    pub fn with_ground_albedo(mut self, albedo: Vec3) -> Self {
        self.ground_albedo = albedo;
        self.update();
        self
    }

    /// Where the sun stands.
    pub fn sun_position(&self) -> SunPosition {
        self.sun
    }

    /// Unit vector from the scene towards the sun.
    pub fn sun_direction(&self) -> Vec3 {
        self.sun_direction
    }

    /// Illuminance in lux of the sun on a surface facing it, 0 when the sun is below the horizon.
    pub fn sun_illuminance(&self) -> f32 {
        self.sun_illuminance
    }

    /// Linear color of the sunlight, with a luminance of one.
    pub fn sun_color(&self) -> Vec3 {
        self.sun_color
    }

    /// Cosine of the angular radius of the sun disc.
    pub fn sun_cos_radius(&self) -> f32 {
        SUN_ANGULAR_RADIUS.cos()
    }

    ///
    /// Luminance in cd/m² (linear sRGB) of the sky in a direction, without the sun disc. Below the horizon this is the
    /// ground.
    ///
    pub fn radiance(&self, direction: Vec3) -> Vec3 {
        let up = self.orientation.up.normalize();
        let d = direction.normalize();
        let cos_theta = d.dot(up);
        if cos_theta <= 0.0 {
            return self.ground_luminance;
        }
        self.sky_luminance(d, cos_theta.max(0.01))
    }

    /// Luminance of the sun disc in cd/m², for rays that see the sun directly.
    pub fn sun_radiance(&self) -> Vec3 {
        let solid_angle = 2.0 * PI * (1.0 - SUN_ANGULAR_RADIUS.cos());
        self.sun_color * (self.sun_illuminance / solid_angle)
    }

    #[allow(clippy::needless_range_loop)]
    fn sky_luminance(&self, d: Vec3, cos_theta: f32) -> Vec3 {
        let cos_gamma = d.dot(self.sun_direction).clamp(-1.0, 1.0);
        let gamma = cos_gamma.acos();
        let mut xyy = [0.0f32; 3];
        for c in 0..3 {
            let [a, b, cc, dd, e] = self.perez[c];
            let f = (1.0 + a * (b / cos_theta).exp())
                * (1.0 + cc * (dd * gamma).exp() + e * cos_gamma * cos_gamma);
            xyy[c] = self.zenith[c] * f / self.norm[c];
        }
        xyy_to_linear_srgb(xyy[0].max(0.0) * self.sky_scale, xyy[1], xyy[2])
    }

    fn update(&mut self) {
        let t = self.turbidity;
        self.sun_direction = self.sun.direction(&self.orientation);

        // Below the horizon the sky keeps the colors of sunset and fades out until the end of civil twilight
        let elevation = self.sun.elevation as f32;
        let theta_s = (90.0 - elevation.max(0.1)).to_radians();
        self.sky_scale = ((elevation - TWILIGHT_END) / -TWILIGHT_END).clamp(0.0, 1.0);

        self.perez = [
            [
                0.1787 * t - 1.4630,
                -0.3554 * t + 0.4275,
                -0.0227 * t + 5.3251,
                0.1206 * t - 2.5771,
                -0.0670 * t + 0.3703,
            ],
            [
                -0.0193 * t - 0.2592,
                -0.0665 * t + 0.0008,
                -0.0004 * t + 0.2125,
                -0.0641 * t - 0.8989,
                -0.0033 * t + 0.0452,
            ],
            [
                -0.0167 * t - 0.2608,
                -0.0950 * t + 0.0092,
                -0.0079 * t + 0.2102,
                -0.0441 * t - 1.6537,
                -0.0109 * t + 0.0529,
            ],
        ];
        let chi = (4.0 / 9.0 - t / 120.0) * (PI - 2.0 * theta_s);
        // kcd/m² in the paper
        let zenith_y = ((4.0453 * t - 4.9710) * chi.tan() - 0.2155 * t + 2.4192).max(0.0) * 1000.0;
        let (s, s2, s3) = (theta_s, theta_s * theta_s, theta_s * theta_s * theta_s);
        let zenith_x = t * t * (0.00166 * s3 - 0.00375 * s2 + 0.00209 * s)
            + t * (-0.02903 * s3 + 0.06377 * s2 - 0.03202 * s + 0.00394)
            + (0.11693 * s3 - 0.21196 * s2 + 0.06052 * s + 0.25886);
        let zenith_yc = t * t * (0.00275 * s3 - 0.00610 * s2 + 0.00317 * s)
            + t * (-0.04214 * s3 + 0.08970 * s2 - 0.04153 * s + 0.00516)
            + (0.15346 * s3 - 0.26756 * s2 + 0.06670 * s + 0.26688);
        self.zenith = [zenith_y, zenith_x, zenith_yc];
        for c in 0..3 {
            let [a, b, cc, dd, e] = self.perez[c];
            let cos_s = theta_s.cos();
            self.norm[c] =
                (1.0 + a * b.exp()) * (1.0 + cc * (dd * theta_s).exp() + e * cos_s * cos_s);
        }

        // Sunlight through the air mass of its path (Kasten and Young), extinction stronger for blue
        if elevation > 0.0 {
            let zenith_deg = 90.0 - elevation;
            let air_mass = 1.0
                / (zenith_deg.to_radians().cos() + 0.50572 * (96.07995 - zenith_deg).powf(-1.6364));
            self.sun_illuminance = SOLAR_ILLUMINANCE_OUTSIDE * 0.7f32.powf(air_mass.powf(0.678));
            let haze = t / 3.0;
            let color = vec3(
                (-0.06 * haze * air_mass).exp(),
                (-0.11 * haze * air_mass).exp(),
                (-0.25 * haze * air_mass).exp(),
            );
            self.sun_color = color / crate::sampling::luminance(color).max(1e-6);
        } else {
            self.sun_illuminance = 0.0;
            self.sun_color = vec3(1.0, 1.0, 1.0);
        }

        // Light falling on the ground from the sun and the sky, reflected diffusely
        let up = self.orientation.up.normalize();
        let (t1, t2) = crate::sampling::orthonormal_basis(up);
        let (rings, segments) = (16, 32);
        let mut sky_irradiance = vec3(0.0, 0.0, 0.0);
        for i in 0..rings {
            // Equal steps in cos²θ make each ring weigh the same with the cosine
            let u = (i as f32 + 0.5) / rings as f32;
            let cos_t = (1.0 - u).sqrt();
            let sin_t = u.sqrt();
            for j in 0..segments {
                let phi = 2.0 * PI * (j as f32 + 0.5) / segments as f32;
                let d = up * cos_t + (t1 * phi.cos() + t2 * phi.sin()) * sin_t;
                sky_irradiance += self.sky_luminance(d, cos_t.max(0.01));
            }
        }
        sky_irradiance *= PI / (rings * segments) as f32;
        let sun_irradiance =
            self.sun_color * self.sun_illuminance * self.sun_direction.dot(up).max(0.0);
        let albedo = self.ground_albedo;
        let e = sky_irradiance + sun_irradiance;
        self.ground_luminance = vec3(albedo.x * e.x, albedo.y * e.y, albedo.z * e.z) / PI;
    }
}

/// CIE xyY to linear sRGB (D65).
fn xyy_to_linear_srgb(luminance: f32, x: f32, y: f32) -> Vec3 {
    if y <= 0.0 || luminance <= 0.0 {
        return vec3(0.0, 0.0, 0.0);
    }
    let cx = x / y * luminance;
    let cz = (1.0 - x - y) / y * luminance;
    let cy = luminance;
    vec3(
        (3.2406 * cx - 1.5372 * cy - 0.4986 * cz).max(0.0),
        (-0.9689 * cx + 1.8758 * cy + 0.0415 * cz).max(0.0),
        (0.0557 * cx - 0.2040 * cy + 1.0570 * cz).max(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampling::luminance;

    fn sky_at(elevation: f64) -> Sky {
        Sky::from_sun_position(
            SunPosition {
                azimuth: 180.0,
                elevation,
            },
            SceneOrientation::default(),
        )
    }

    #[test]
    fn clear_day_has_plausible_values() {
        let sky = sky_at(60.0);
        // Direct sun on a clear day: around 90 to 110 klux
        assert!(sky.sun_illuminance() > 80_000.0 && sky.sun_illuminance() < 120_000.0);
        // Zenith of a clear sky: a few kcd/m²
        let zenith = luminance(sky.radiance(vec3(0.0, 1.0, 0.0)));
        assert!(zenith > 2_000.0 && zenith < 15_000.0, "{zenith}");
        // Blue sky away from the sun
        let north = sky.radiance(vec3(0.0, 0.5, -1.0));
        assert!(north.z > north.x, "{north:?}");
        // Brighter near the sun than away from it
        let near_sun = sky.radiance(sky.sun_direction() + vec3(0.05, 0.0, 0.0));
        assert!(luminance(near_sun) > luminance(north));
    }

    #[test]
    fn low_sun_is_weaker_and_redder() {
        let high = sky_at(60.0);
        let low = sky_at(5.0);
        assert!(low.sun_illuminance() < 0.5 * high.sun_illuminance());
        assert!(low.sun_color().x / low.sun_color().z > high.sun_color().x / high.sun_color().z);
    }

    #[test]
    fn night_is_dark() {
        let sky = sky_at(-20.0);
        assert_eq!(sky.sun_illuminance(), 0.0);
        assert_eq!(sky.radiance(vec3(0.0, 1.0, 0.0)), vec3(0.0, 0.0, 0.0));
    }

    #[test]
    fn ground_reflects_sun_and_sky() {
        let sky = sky_at(60.0);
        let ground = luminance(sky.radiance(vec3(0.0, -1.0, 0.0)));
        // 20 % of roughly 100 klux, divided by π
        assert!(ground > 4_000.0 && ground < 12_000.0, "{ground}");
    }
}
