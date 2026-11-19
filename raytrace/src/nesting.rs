//!
//! Solids inside solids: which medium a ray is in where closed meshes of glass, water, ice and smoke overlap, and
//! what happens at their surfaces. Where solids overlap, the one with the higher [RtMaterial::medium_priority] fills
//! the overlap, at equal priority the one entered last; the surfaces of the others there are not seen (nested
//! dielectrics, after Schmidt and Budge, "Simple nested dielectrics in ray traced images", 2002).
//!

use crate::{RtMaterial, RtMedium};

/// Solids a ray can be in at once; more are dropped, the lowest first.
pub(crate) const MAX_NESTED: usize = 4;

/// A solid a ray is in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Solid {
    pub instance: u32,
    pub priority: u32,
    pub medium: Option<RtMedium>,
    /// Index of refraction; a mesh that only holds a medium (index 1) takes the one of the solid it is in, so light
    /// passes it unbent.
    pub ior: f32,
}

impl Solid {
    /// The solid of an instance with the material, if the material makes one: transmissive and not thin walled.
    pub fn of(instance: u32, m: &RtMaterial) -> Option<Self> {
        (m.transmission > 0.0 && !m.thin_walled).then_some(Self {
            instance,
            priority: m.medium_priority,
            medium: m.medium,
            ior: m.index_of_refraction,
        })
    }
}

/// What a surface of a solid is for a ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Boundary {
    /// Whether light sees it: reflects, refracts and takes its color. Surfaces inside a solid of higher priority are
    /// not seen.
    pub seen: bool,
    /// Index of refraction after over before.
    pub eta: f32,
}

/// The solids a ray is in, in the order it entered them; none is the air with the scene's fog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Inside {
    solids: [Option<Solid>; MAX_NESTED],
}

impl Inside {
    /// In no solid.
    pub const AIR: Self = Self {
        solids: [None; MAX_NESTED],
    };

    /// Whether the ray is in no solid.
    pub fn is_air(&self) -> bool {
        self.solids[0].is_none()
    }

    /// The solids, in the order they were entered.
    pub fn solids(&self) -> impl Iterator<Item = &Solid> {
        self.solids.iter().flatten()
    }

    /// The solid whose medium the ray is in: the highest priority, at equal priority the one entered last.
    pub fn top(&self) -> Option<Solid> {
        let mut top: Option<Solid> = None;
        for s in self.solids() {
            if top.is_none_or(|t| s.priority >= t.priority) {
                top = Some(*s);
            }
        }
        top
    }

    /// Index of refraction where the ray is.
    pub fn ior(&self) -> f32 {
        self.top().map_or(1.0, |s| s.ior)
    }

    /// The medium where the ray is, `None` in the air and in clear solids.
    pub fn medium(&self) -> Option<RtMedium> {
        self.top().and_then(|s| s.medium)
    }

    pub fn contains(&self, instance: u32) -> bool {
        self.solids().any(|s| s.instance == instance)
    }

    fn with(&self, mut solid: Solid) -> Self {
        if solid.ior == 1.0 {
            solid.ior = self.ior();
        }
        let mut solids: Vec<Solid> = self.solids().copied().collect();
        if solids.len() == MAX_NESTED {
            // The oldest of the lowest priority goes
            let lowest = solids
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| s.priority)
                .map(|(i, _)| i)
                .expect("full");
            solids.remove(lowest);
        }
        solids.push(solid);
        Self::from_list(&solids)
    }

    fn without(&self, instance: u32) -> Self {
        let solids: Vec<Solid> = self
            .solids()
            .copied()
            .filter(|s| s.instance != instance)
            .collect();
        Self::from_list(&solids)
    }

    fn from_list(list: &[Solid]) -> Self {
        let mut solids = [None; MAX_NESTED];
        for (slot, s) in solids.iter_mut().zip(list) {
            *slot = Some(*s);
        }
        Self { solids }
    }

    ///
    /// Crosses a surface of the solid, into it when the ray hits the front: where the ray is after, and what the
    /// surface is. Entering a solid the ray is in already, or leaving one it is not in (a surface touching another,
    /// such as a pane of glass on the floor), is a surface not seen.
    ///
    pub fn cross(&self, solid: Solid, front_face: bool) -> (Self, Boundary) {
        let unseen = Boundary {
            seen: false,
            eta: 1.0,
        };
        let entered = self.contains(solid.instance);
        if front_face == entered {
            return (*self, unseen);
        }
        let after = if front_face {
            self.with(solid)
        } else {
            self.without(solid.instance)
        };
        let seen = if front_face {
            after.top().map(|s| s.instance) == Some(solid.instance)
        } else {
            self.top().map(|s| s.instance) == Some(solid.instance)
        };
        let eta = if seen { after.ior() / self.ior() } else { 1.0 };
        (after, Boundary { seen, eta })
    }

    ///
    /// The solids that contain a point, from the signed boundary hits of a ray from it to infinity: `hits` gives the
    /// solid and whether the ray hit its front. A solid the ray leaves more often than it enters contains the point.
    ///
    pub fn of_point(hits: impl IntoIterator<Item = (Solid, bool)>) -> Self {
        let mut counts: Vec<(Solid, i32)> = Vec::new();
        for (solid, front) in hits {
            let sign = if front { -1 } else { 1 };
            match counts
                .iter_mut()
                .find(|(s, _)| s.instance == solid.instance)
            {
                Some((_, n)) => *n += sign,
                None => counts.push((solid, sign)),
            }
        }
        let mut containing: Vec<Solid> = counts
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(s, _)| s)
            .collect();
        // The order of entering is not known: lower priorities first, so ties go to the later instance
        containing.sort_by_key(|s| (s.priority, s.instance));
        containing
            .into_iter()
            .fold(Self::AIR, |inside, solid| inside.with(solid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(instance: u32, priority: u32, ior: f32) -> Solid {
        Solid {
            instance,
            priority,
            medium: None,
            ior,
        }
    }

    #[test]
    fn ice_in_water_in_a_glass() {
        let (glass, water, ice) = (solid(0, 3, 1.5), solid(1, 1, 1.33), solid(2, 2, 1.31));
        // Into the wall of the glass, then the water's surface inside the wall is not seen
        let (a, b) = Inside::AIR.cross(glass, true);
        assert!(b.seen && (b.eta - 1.5).abs() < 1e-6);
        let (a, b) = a.cross(water, true);
        assert!(!b.seen);
        // Out of the wall into the water
        let (a, b) = a.cross(glass, false);
        assert!(b.seen && (b.eta - 1.33 / 1.5).abs() < 1e-6, "{b:?}");
        assert_eq!(a.top(), Some(water));
        // Into the ice and out of it again
        let (a, b) = a.cross(ice, true);
        assert!(b.seen && (b.eta - 1.31 / 1.33).abs() < 1e-6);
        let (a, b) = a.cross(water, false);
        assert!(!b.seen, "the water's surface in the ice");
        let (a, b) = a.cross(ice, false);
        assert!(
            b.seen && (b.eta - 1.0 / 1.31).abs() < 1e-6,
            "ice above the water"
        );
        assert!(a.is_air());
    }

    #[test]
    fn a_medium_without_refraction_takes_the_index_around_it() {
        let water = solid(0, 0, 1.33);
        let smoke = solid(1, 0, 1.0);
        let (a, _) = Inside::AIR.cross(water, true);
        let (a, b) = a.cross(smoke, true);
        assert!(b.seen && b.eta == 1.0);
        let (_, b) = a.cross(smoke, false);
        assert!(b.seen && b.eta == 1.0);
    }

    #[test]
    fn touching_surfaces_are_not_seen() {
        let pane = solid(0, 0, 1.5);
        let (a, b) = Inside::AIR.cross(pane, false);
        assert!(!b.seen && a.is_air());
        let (a, _) = a.cross(pane, true);
        let (a2, b) = a.cross(pane, true);
        assert!(!b.seen && a2 == a);
    }

    #[test]
    fn a_point_in_smoke_in_a_room() {
        let (room, smoke) = (solid(0, 0, 1.0), solid(1, 5, 1.0));
        // Out of the smoke, then out of the room: both contain the point
        let inside = Inside::of_point([(smoke, false), (room, false)]);
        assert_eq!(inside.top().map(|s| s.instance), Some(1));
        // Into a solid and out again: not in it
        let outside = Inside::of_point([(smoke, true), (smoke, false)]);
        assert!(outside.is_air());
    }
}
