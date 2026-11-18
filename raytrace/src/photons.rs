//!
//! Caustic photons: light from the sun and the directional and point lights that reached a surface through solid
//! glass, found by shooting from the lights and gathered where camera paths meet surfaces.
//!

use crate::sampling::{orthonormal_basis, uniform_cone, Rng};
use crate::{RtEnvironment, RtLight, RtScene};
use std::collections::HashMap;
use std::f32::consts::PI;
use three_d_asset::prelude::*;

/// A packet of light that came through glass and landed on a surface.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Photon {
    pub position: Vec3,
    /// Unit direction the light travelled in.
    pub direction: Vec3,
    /// Luminous flux in lm.
    pub power: Vec3,
}

/// The photons of one pass in a grid of cells as large as the gather radius.
pub(crate) struct PhotonMap {
    pub radius: f32,
    cells: HashMap<[i32; 3], Vec<Photon>>,
    min: Vec3,
    max: Vec3,
}

impl PhotonMap {
    pub(crate) fn new(photons: Vec<Photon>, radius: f32) -> Self {
        let mut cells: HashMap<[i32; 3], Vec<Photon>> = HashMap::new();
        let mut min = vec3(f32::MAX, f32::MAX, f32::MAX);
        let mut max = vec3(f32::MIN, f32::MIN, f32::MIN);
        for p in photons {
            min = vec3(
                min.x.min(p.position.x),
                min.y.min(p.position.y),
                min.z.min(p.position.z),
            );
            max = vec3(
                max.x.max(p.position.x),
                max.y.max(p.position.y),
                max.z.max(p.position.z),
            );
            cells.entry(cell(p.position, radius)).or_default().push(p);
        }
        Self {
            radius,
            cells,
            min,
            max,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    ///
    /// Calls `visit` with every photon within the radius of `p` that lies near the plane with normal `n` and arrived
    /// from its side, and the area of the gather disc.
    ///
    pub(crate) fn gather(&self, p: Vec3, n: Vec3, mut visit: impl FnMut(&Photon)) {
        let r = self.radius;
        if p.x < self.min.x - r
            || p.y < self.min.y - r
            || p.z < self.min.z - r
            || p.x > self.max.x + r
            || p.y > self.max.y + r
            || p.z > self.max.z + r
        {
            return;
        }
        let c = cell(p, r);
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let Some(photons) = self.cells.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) else {
                        continue;
                    };
                    for photon in photons {
                        let d = photon.position - p;
                        // Within the disc, close to the surface's plane and coming from its front
                        if d.magnitude2() <= r * r
                            && d.dot(n).abs() <= 0.25 * r
                            && photon.direction.dot(n) < 0.0
                        {
                            visit(photon);
                        }
                    }
                }
            }
        }
    }

    /// Area of the gather disc.
    pub(crate) fn area(&self) -> f32 {
        PI * self.radius * self.radius
    }
}

fn cell(p: Vec3, size: f32) -> [i32; 3] {
    [
        (p.x / size).floor() as i32,
        (p.y / size).floor() as i32,
        (p.z / size).floor() as i32,
    ]
}

/// Whether light through a material of this kind is left to the photons: glass that bends it.
pub(crate) fn bends_light(m: &crate::RtMaterial) -> bool {
    m.transmission > 0.0 && !m.thin_walled
}

/// Bounding spheres of the instances of solid glass in world space.
pub(crate) fn glass_spheres(scene: &RtScene) -> Vec<(Vec3, f32)> {
    scene
        .instances()
        .iter()
        .filter(|i| bends_light(&scene.materials()[i.material.0 as usize]))
        .filter_map(|i| {
            let mesh = &scene.meshes()[i.mesh.0 as usize];
            let mut min = vec3(f32::MAX, f32::MAX, f32::MAX);
            let mut max = vec3(f32::MIN, f32::MIN, f32::MIN);
            for p in &mesh.positions {
                let w = (i.transform * p.extend(1.0)).truncate();
                min = vec3(min.x.min(w.x), min.y.min(w.y), min.z.min(w.z));
                max = vec3(max.x.max(w.x), max.y.max(w.y), max.z.max(w.z));
            }
            (min.x <= max.x).then(|| {
                let center = (min + max) * 0.5;
                (center, (max - center).magnitude() * 1.001)
            })
        })
        .collect()
}

/// A light that casts caustics: the sun or a directional light (direction the light travels, illuminance, cosine of
/// the half angle of the disc it comes from) or a point light (position, intensity).
pub(crate) enum CausticSource {
    Parallel {
        direction: Vec3,
        light: Vec3,
        cos_radius: f32,
    },
    Point {
        position: Vec3,
        light: Vec3,
    },
}

/// The lights whose caustics come from photons; area lights and the sky are found by the paths themselves.
pub(crate) fn caustic_sources(scene: &RtScene) -> Vec<CausticSource> {
    let mut sources = Vec::new();
    if let RtEnvironment::Sky(sky) = scene.environment() {
        if sky.sun_illuminance() > 0.0 {
            sources.push(CausticSource::Parallel {
                direction: -sky.sun_direction(),
                light: sky.sun_color() * sky.sun_illuminance(),
                cos_radius: sky.sun_cos_radius(),
            });
        }
    }
    for light in scene.lights() {
        match light {
            RtLight::Directional {
                direction,
                color,
                illuminance,
            } => sources.push(CausticSource::Parallel {
                direction: direction.normalize(),
                light: *color * *illuminance,
                cos_radius: 1.0,
            }),
            RtLight::Point {
                position,
                color,
                intensity,
            } => sources.push(CausticSource::Point {
                position: *position,
                light: *color * *intensity,
            }),
            _ => {}
        }
    }
    sources
}

/// Where a photon starts, where it goes and its flux in lm.
pub(crate) struct Emission {
    pub origin: Vec3,
    pub direction: Vec3,
    pub power: Vec3,
}

impl CausticSource {
    ///
    /// One of `count` photons aimed at the glass: at a sphere chosen by its size seen from the light, uniformly over
    /// it. Where spheres overlap, the photon's flux is shared by all that contain it. Photons of parallel light start
    /// on the disc of the sphere through its center, the caller moves them outside the scene.
    ///
    pub(crate) fn emit(
        &self,
        spheres: &[(Vec3, f32)],
        count: u32,
        rng: &mut Rng,
    ) -> Option<Emission> {
        match *self {
            CausticSource::Parallel {
                direction,
                light,
                cos_radius,
            } => {
                let d = if cos_radius < 1.0 {
                    let (t, b) = orthonormal_basis(direction);
                    let c = uniform_cone(rng.next_f32(), rng.next_f32(), cos_radius);
                    (t * c.x + b * c.y + direction * c.z).normalize()
                } else {
                    direction
                };
                let areas: Vec<f32> = spheres.iter().map(|(_, r)| PI * r * r).collect();
                let (center, radius) = spheres[pick(&areas, rng.next_f32())?];
                let (t, b) = orthonormal_basis(d);
                let r = radius * rng.next_f32().sqrt();
                let phi = 2.0 * PI * rng.next_f32();
                let point = center + t * (r * phi.cos()) + b * (r * phi.sin());
                let covering = spheres
                    .iter()
                    .filter(|(c, r)| {
                        let to = *c - point;
                        (to - d * to.dot(d)).magnitude2() <= r * r
                    })
                    .count()
                    .max(1);
                let total: f32 = areas.iter().sum();
                Some(Emission {
                    origin: point,
                    direction: d,
                    power: light * (total / (count as f32 * covering as f32)),
                })
            }
            CausticSource::Point { position, light } => {
                let cones: Vec<(Vec3, f32)> = spheres
                    .iter()
                    .map(|(c, r)| {
                        let to = *c - position;
                        let distance = to.magnitude();
                        if distance <= *r {
                            (vec3(0.0, 0.0, 1.0), -1.0)
                        } else {
                            let sin = r / distance;
                            (to / distance, (1.0 - sin * sin).max(0.0).sqrt())
                        }
                    })
                    .collect();
                let solid_angles: Vec<f32> =
                    cones.iter().map(|(_, c)| 2.0 * PI * (1.0 - c)).collect();
                let (axis, cos_max) = cones[pick(&solid_angles, rng.next_f32())?];
                let (t, b) = orthonormal_basis(axis);
                let c = uniform_cone(rng.next_f32(), rng.next_f32(), cos_max);
                let d = (t * c.x + b * c.y + axis * c.z).normalize();
                let covering = cones
                    .iter()
                    .filter(|(axis, cos_max)| d.dot(*axis) >= *cos_max)
                    .count()
                    .max(1);
                let total: f32 = solid_angles.iter().sum();
                Some(Emission {
                    origin: position,
                    direction: d,
                    power: light * (total / (count as f32 * covering as f32)),
                })
            }
        }
    }
}

/// An index chosen with the probability of its weight.
fn pick(weights: &[f32], u: f32) -> Option<usize> {
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let mut left = u * total;
    for (i, w) in weights.iter().enumerate() {
        if left < *w {
            return Some(i);
        }
        left -= w;
    }
    weights.iter().rposition(|w| *w > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photons_carry_the_light_that_falls_on_the_glass() {
        // Two overlapping spheres under parallel light: the flux through both discs together
        let spheres = [(vec3(0.0, 0.0, 0.0), 1.0), (vec3(0.5, 0.0, 0.0), 1.0)];
        let source = CausticSource::Parallel {
            direction: vec3(0.0, -1.0, 0.0),
            light: vec3(1.0, 1.0, 1.0),
            cos_radius: 1.0,
        };
        let n = 200_000;
        let mut rng = Rng::new(1, 2);
        let mut flux = 0.0;
        for _ in 0..n {
            flux += source.emit(&spheres, n, &mut rng).unwrap().power.x;
        }
        // Area of the union of two unit discs half a radius apart
        let d: f32 = 0.5;
        let lens = 2.0 * (d / 2.0).acos() - d / 2.0 * (4.0 - d * d).sqrt();
        let union = 2.0 * PI - lens;
        assert!((flux - union).abs() < 0.01 * union, "{flux} {union}");

        // A point light: the intensity over the solid angle of the sphere
        let source = CausticSource::Point {
            position: vec3(0.0, 4.0, 0.0),
            light: vec3(1.0, 1.0, 1.0),
        };
        let mut flux = 0.0;
        for _ in 0..n {
            flux += source.emit(&spheres[..1], n, &mut rng).unwrap().power.x;
        }
        let omega = 2.0 * PI * (1.0 - (1.0f32 - 1.0 / 16.0).sqrt());
        assert!((flux - omega).abs() < 1e-3 * omega, "{flux} {omega}");
    }

    #[test]
    fn gather_finds_the_photons_in_the_disc() {
        let photon = |x: f32, y: f32| Photon {
            position: vec3(x, y, 0.0),
            direction: vec3(0.0, -1.0, 0.0),
            power: vec3(1.0, 1.0, 1.0),
        };
        let map = PhotonMap::new(
            vec![
                photon(0.0, 0.0),
                photon(0.09, 0.0),
                photon(0.2, 0.0),
                photon(0.0, 0.5),
            ],
            0.1,
        );
        let mut found = 0;
        map.gather(vec3(0.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), |_| found += 1);
        assert_eq!(found, 2);
        // Not from below
        let mut found = 0;
        map.gather(vec3(0.0, 0.0, 0.0), vec3(0.0, -1.0, 0.0), |_| found += 1);
        assert_eq!(found, 0);
    }
}
