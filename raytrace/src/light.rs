//!
//! Sampling and hitting the area lights.
//!

use crate::sampling::{orthonormal_basis, uniform_cone};
use crate::RtLight;
use std::f32::consts::PI;
use three_d_asset::prelude::*;

/// A point on an area light seen from somewhere.
pub(crate) struct LightSample {
    /// Unit direction towards the light.
    pub direction: Vec3,
    pub distance: f32,
    /// Luminance arriving from the light.
    pub radiance: Vec3,
    /// Probability density per solid angle.
    pub pdf: f32,
}

/// A ray hitting an area light.
pub(crate) struct LightHit {
    pub distance: f32,
    pub radiance: Vec3,
}

/// A point on the light `light` seen from `p`, for two random numbers. `None` for lights without area.
pub(crate) fn sample_area_light(light: &RtLight, p: Vec3, u1: f32, u2: f32) -> Option<LightSample> {
    match light {
        RtLight::Rect {
            corner,
            edge_u,
            edge_v,
            color,
            luminance,
            two_sided,
        } => {
            let normal = edge_u.cross(*edge_v);
            let area = normal.magnitude();
            if area <= 0.0 {
                return None;
            }
            let normal = normal / area;
            let q = *corner + *edge_u * u1 + *edge_v * u2;
            let d = q - p;
            let d2 = d.magnitude2();
            if d2 <= 0.0 {
                return None;
            }
            let distance = d2.sqrt();
            let direction = d / distance;
            let cos = -normal.dot(direction);
            let cos = if *two_sided { cos.abs() } else { cos };
            if cos <= 1e-6 {
                return None;
            }
            Some(LightSample {
                direction,
                distance,
                radiance: *color * *luminance,
                pdf: d2 / (area * cos),
            })
        }
        RtLight::Sphere {
            center,
            radius,
            color,
            luminance,
        } => {
            let d = *center - p;
            let d2 = d.magnitude2();
            let r2 = radius * radius;
            if d2 <= r2 {
                return None;
            }
            let axis = d / d2.sqrt();
            let cos_max = (1.0 - r2 / d2).max(0.0).sqrt();
            let (t, b) = orthonormal_basis(axis);
            let c = uniform_cone(u1, u2, cos_max);
            let direction = (t * c.x + b * c.y + axis * c.z).normalize();
            let distance = sphere_distance(*center, *radius, p, direction).unwrap_or(d2.sqrt());
            Some(LightSample {
                direction,
                distance,
                radiance: *color * *luminance,
                pdf: cone_pdf(cos_max),
            })
        }
        _ => None,
    }
}

/// Where the ray from `origin` along the unit `direction` hits the light, if before `max_distance`.
pub(crate) fn hit_area_light(
    light: &RtLight,
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
) -> Option<LightHit> {
    let (distance, radiance) = match light {
        RtLight::Rect {
            corner,
            edge_u,
            edge_v,
            color,
            luminance,
            two_sided,
        } => {
            let normal = edge_u.cross(*edge_v);
            let n2 = normal.magnitude2();
            let facing = normal.dot(direction);
            if n2 <= 0.0 || facing == 0.0 || (!*two_sided && facing > 0.0) {
                return None;
            }
            let t = normal.dot(*corner - origin) / facing;
            if t <= 1e-4 {
                return None;
            }
            let q = origin + direction * t - *corner;
            let a = q.cross(*edge_v).dot(normal) / n2;
            let b = edge_u.cross(q).dot(normal) / n2;
            if !(0.0..=1.0).contains(&a) || !(0.0..=1.0).contains(&b) {
                return None;
            }
            (t, *color * *luminance)
        }
        RtLight::Sphere {
            center,
            radius,
            color,
            luminance,
        } => {
            if (origin - *center).magnitude2() <= radius * radius {
                return None;
            }
            (
                sphere_distance(*center, *radius, origin, direction)?,
                *color * *luminance,
            )
        }
        _ => return None,
    };
    (distance < max_distance).then_some(LightHit { distance, radiance })
}

/// The density [sample_area_light] gives the direction from `p` towards the light, hit at `distance`.
pub(crate) fn area_light_pdf(light: &RtLight, p: Vec3, direction: Vec3, distance: f32) -> f32 {
    match light {
        RtLight::Rect { edge_u, edge_v, .. } => {
            let normal = edge_u.cross(*edge_v);
            let area = normal.magnitude();
            let cos = (normal.dot(direction) / area).abs();
            if area <= 0.0 || cos <= 1e-6 {
                return 0.0;
            }
            distance * distance / (area * cos)
        }
        RtLight::Sphere { center, radius, .. } => {
            let d2 = (*center - p).magnitude2();
            let r2 = radius * radius;
            if d2 <= r2 {
                return 0.0;
            }
            cone_pdf((1.0 - r2 / d2).max(0.0).sqrt())
        }
        _ => 0.0,
    }
}

fn cone_pdf(cos_max: f32) -> f32 {
    1.0 / (2.0 * PI * (1.0 - cos_max).max(1e-7))
}

/// The distance to the near side of a sphere seen from outside.
fn sphere_distance(center: Vec3, radius: f32, origin: Vec3, direction: Vec3) -> Option<f32> {
    let oc = origin - center;
    let b = oc.dot(direction);
    let c = oc.magnitude2() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t > 1e-4).then_some(t)
}

/// Weight of one of two sampling strategies by the power heuristic.
pub(crate) fn power_heuristic(pdf: f32, other: f32) -> f32 {
    let (a, b) = (pdf * pdf, other * other);
    if a + b <= 0.0 {
        0.0
    } else {
        a / (a + b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> RtLight {
        RtLight::Rect {
            corner: vec3(-1.0, 2.0, -0.5),
            edge_u: vec3(0.0, 0.0, 1.0),
            edge_v: vec3(2.0, 0.0, 0.0),
            color: vec3(1.0, 1.0, 1.0),
            luminance: 10.0,
            two_sided: false,
        }
    }

    /// Integrating the inverse density over the samples gives the solid angle the light covers.
    fn solid_angle(light: &RtLight, p: Vec3) -> f32 {
        let n = 64;
        let mut sum = 0.0;
        for i in 0..n {
            for j in 0..n {
                let (u1, u2) = ((i as f32 + 0.5) / n as f32, (j as f32 + 0.5) / n as f32);
                if let Some(s) = sample_area_light(light, p, u1, u2) {
                    sum += 1.0 / s.pdf;
                    // The sample can be hit again with the same density
                    let hit = hit_area_light(light, p, s.direction, f32::INFINITY).unwrap();
                    assert!((hit.distance - s.distance).abs() < 1e-3 * s.distance);
                    let pdf = area_light_pdf(light, p, s.direction, hit.distance);
                    assert!((pdf / s.pdf - 1.0).abs() < 1e-3, "{pdf} {}", s.pdf);
                }
            }
        }
        sum / (n * n) as f32
    }

    #[test]
    fn densities_cover_the_solid_angle() {
        // A sphere of radius 1 at distance 2: 2π(1 - cos 30°)
        let ball = RtLight::Sphere {
            center: vec3(0.0, 2.0, 0.0),
            radius: 1.0,
            color: vec3(1.0, 1.0, 1.0),
            luminance: 1.0,
        };
        let expected = 2.0 * PI * (1.0 - 30f32.to_radians().cos());
        let got = solid_angle(&ball, vec3(0.0, 0.0, 0.0));
        assert!((got / expected - 1.0).abs() < 1e-3, "{got} vs {expected}");
        // A small panel far away: area / d²
        let far = RtLight::Rect {
            corner: vec3(-0.05, 10.0, -0.05),
            edge_u: vec3(0.1, 0.0, 0.0),
            edge_v: vec3(0.0, 0.0, 0.1),
            color: vec3(1.0, 1.0, 1.0),
            luminance: 1.0,
            two_sided: false,
        };
        let got = solid_angle(&far, vec3(0.0, 0.0, 0.0));
        assert!((got / 1e-4 - 1.0).abs() < 1e-3, "{got}");
        solid_angle(&panel(), vec3(0.3, 3.0, 0.2));
    }

    #[test]
    fn one_sided_panels_shine_to_one_side() {
        // edge_u × edge_v = z × 2x points up: the panel shines upwards and is dark from below
        let below = vec3(0.0, 0.0, 0.0);
        assert!(sample_area_light(&panel(), below, 0.5, 0.5).is_none());
        assert!(hit_area_light(&panel(), below, vec3(0.0, 1.0, 0.0), f32::INFINITY).is_none());
        let above = vec3(0.0, 3.0, 0.0);
        assert!(sample_area_light(&panel(), above, 0.5, 0.5).is_some());
        let hit = hit_area_light(&panel(), above, vec3(0.0, -1.0, 0.0), f32::INFINITY).unwrap();
        assert!((hit.distance - 1.0).abs() < 1e-5);
        assert!(hit_area_light(&panel(), above, vec3(0.0, -1.0, 0.0), 0.5).is_none());
    }
}
