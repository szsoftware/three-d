//!
//! A bounding volume hierarchy over boxes, built with the surface area heuristic.
//!
//! The nodes have a flat layout of 32 bytes each, the same layout a GPU tracer reads from a storage buffer.
//!

use three_d_asset::prelude::*;

/// The number of bins the surface area heuristic tries per axis.
const BINS: usize = 12;

///
/// An axis aligned box.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    pub const EMPTY: Self = Self {
        min: Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY),
        max: Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
    };

    pub fn grow(&mut self, p: Vec3) {
        self.min = vec3(
            self.min.x.min(p.x),
            self.min.y.min(p.y),
            self.min.z.min(p.z),
        );
        self.max = vec3(
            self.max.x.max(p.x),
            self.max.y.max(p.y),
            self.max.z.max(p.z),
        );
    }

    pub fn union(&mut self, other: &Bounds) {
        self.grow(other.min);
        self.grow(other.max);
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }

    pub fn area(&self) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let d = self.max - self.min;
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }

    pub fn center(&self) -> Vec3 {
        0.5 * (self.min + self.max)
    }

    /// The bounds after transforming all eight corners.
    pub fn transformed(&self, m: &Mat4) -> Bounds {
        let mut b = Bounds::EMPTY;
        if self.is_empty() {
            return b;
        }
        for i in 0..8 {
            let p = vec3(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            );
            b.grow((m * p.extend(1.0)).truncate());
        }
        b
    }
}

///
/// One node of a [Bvh]. A leaf has `count > 0` and holds the primitives `indices[first..first + count]`,
/// an inner node has `count == 0` and its children at `first` and `first + 1`.
///
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct BvhNode {
    /// Minimum corner of the box around everything below this node.
    pub min: [f32; 3],
    /// First primitive (leaf) or first child (inner node).
    pub first: u32,
    /// Maximum corner of the box around everything below this node.
    pub max: [f32; 3],
    /// Number of primitives, 0 for an inner node.
    pub count: u32,
}

///
/// A bounding volume hierarchy over a list of boxes. The root is node 0.
///
#[derive(Clone, Debug, Default)]
pub struct Bvh {
    /// The nodes, root first.
    pub nodes: Vec<BvhNode>,
    /// Primitive indices in the order the leaves refer to them.
    pub indices: Vec<u32>,
}

impl Bvh {
    ///
    /// Builds the hierarchy over primitives with the given boxes. Leaves hold at most `max_leaf_size` primitives
    /// unless they can not be split any further.
    ///
    pub(crate) fn build(bounds: &[Bounds], max_leaf_size: usize) -> Self {
        let n = bounds.len();
        let mut indices: Vec<u32> = (0..n as u32).collect();
        let centers: Vec<Vec3> = bounds.iter().map(|b| b.center()).collect();
        let mut nodes = Vec::with_capacity(2 * n.max(1));
        nodes.push(BvhNode {
            min: [0.0; 3],
            first: 0,
            max: [0.0; 3],
            count: n as u32,
        });
        if n == 0 {
            // An empty box that no ray hits
            nodes[0].min = [f32::INFINITY; 3];
            nodes[0].max = [f32::NEG_INFINITY; 3];
            return Self { nodes, indices };
        }

        // (node, start, end) still to process
        let mut todo = vec![(0usize, 0usize, n)];
        while let Some((node, start, end)) = todo.pop() {
            let mut node_bounds = Bounds::EMPTY;
            let mut center_bounds = Bounds::EMPTY;
            for &i in &indices[start..end] {
                node_bounds.union(&bounds[i as usize]);
                center_bounds.grow(centers[i as usize]);
            }
            nodes[node].min = node_bounds.min.into();
            nodes[node].max = node_bounds.max.into();
            let count = end - start;

            let split = if count > 1 {
                find_split(&indices[start..end], bounds, &centers, &center_bounds)
            } else {
                None
            };
            let leaf_cost = count as f32;
            let split = match split {
                // Splitting has to pay for the extra box test (cost 1 relative to a primitive test)
                Some((axis, position, cost))
                    if cost / node_bounds.area().max(1e-30) + 1.0 < leaf_cost =>
                {
                    Some((axis, position))
                }
                _ if count > max_leaf_size => {
                    // Too many primitives for a leaf: split at the middle of the longest axis of the centers
                    let d = center_bounds.max - center_bounds.min;
                    let axis = if d.x >= d.y && d.x >= d.z {
                        0
                    } else if d.y >= d.z {
                        1
                    } else {
                        2
                    };
                    if d[axis] > 0.0 {
                        Some((axis, center_bounds.center()[axis]))
                    } else {
                        None
                    }
                }
                _ => None,
            };

            let mut made_children = false;
            if let Some((axis, position)) = split {
                // Partition the indices in place
                let slice = &mut indices[start..end];
                let mut left = 0;
                for i in 0..slice.len() {
                    if centers[slice[i] as usize][axis] < position {
                        slice.swap(i, left);
                        left += 1;
                    }
                }
                if left > 0 && left < count {
                    let first_child = nodes.len();
                    for _ in 0..2 {
                        nodes.push(BvhNode {
                            min: [0.0; 3],
                            first: 0,
                            max: [0.0; 3],
                            count: 0,
                        });
                    }
                    nodes[node].first = first_child as u32;
                    nodes[node].count = 0;
                    todo.push((first_child + 1, start + left, end));
                    todo.push((first_child, start, start + left));
                    made_children = true;
                }
            }
            if !made_children {
                nodes[node].first = start as u32;
                nodes[node].count = count as u32;
            }
        }
        Self { nodes, indices }
    }

    ///
    /// Visits the leaves a ray passes through, nearer boxes first. `visit` gets each primitive index together with the
    /// current maximum distance, which it can shorten to skip boxes further away, and returns `true` to stop.
    ///
    pub(crate) fn traverse(
        &self,
        origin: Vec3,
        direction: Vec3,
        t_max: &mut f32,
        mut visit: impl FnMut(u32, &mut f32) -> bool,
    ) {
        if self.indices.is_empty() {
            return;
        }
        let inv = vec3(1.0 / direction.x, 1.0 / direction.y, 1.0 / direction.z);
        let mut stack = [0u32; 64];
        let mut size = 0;
        let root = &self.nodes[0];
        if slab(root, origin, inv, *t_max).is_none() {
            return;
        }
        let mut current = 0u32;
        loop {
            let node = &self.nodes[current as usize];
            if node.count > 0 {
                for i in node.first..node.first + node.count {
                    if visit(self.indices[i as usize], t_max) {
                        return;
                    }
                }
            } else {
                let a = node.first;
                let b = a + 1;
                let ta = slab(&self.nodes[a as usize], origin, inv, *t_max);
                let tb = slab(&self.nodes[b as usize], origin, inv, *t_max);
                match (ta, tb) {
                    (Some(ta), Some(tb)) => {
                        let (near, far) = if ta <= tb { (a, b) } else { (b, a) };
                        if size < stack.len() {
                            stack[size] = far;
                            size += 1;
                        }
                        current = near;
                        continue;
                    }
                    (Some(_), None) => {
                        current = a;
                        continue;
                    }
                    (None, Some(_)) => {
                        current = b;
                        continue;
                    }
                    (None, None) => {}
                }
            }
            // Next node from the stack that is still in reach
            loop {
                if size == 0 {
                    return;
                }
                size -= 1;
                let candidate = stack[size];
                if slab(&self.nodes[candidate as usize], origin, inv, *t_max).is_some() {
                    current = candidate;
                    break;
                }
            }
        }
    }
}

/// The distance at which the ray enters the box of the node, if it does before `t_max`.
#[inline]
fn slab(node: &BvhNode, origin: Vec3, inv: Vec3, t_max: f32) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = t_max;
    for axis in 0..3 {
        let a = (node.min[axis] - origin[axis]) * inv[axis];
        let b = (node.max[axis] - origin[axis]) * inv[axis];
        // min/max ignore NaN (0 * infinity), which keeps rays in a box face
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    if t0 <= t1 {
        Some(t0)
    } else {
        None
    }
}

/// The best split (axis, position, cost) by the binned surface area heuristic, if any.
#[allow(clippy::needless_range_loop)]
fn find_split(
    indices: &[u32],
    bounds: &[Bounds],
    centers: &[Vec3],
    center_bounds: &Bounds,
) -> Option<(usize, f32, f32)> {
    let mut best: Option<(usize, f32, f32)> = None;
    for axis in 0..3 {
        let lo = center_bounds.min[axis];
        let extent = center_bounds.max[axis] - lo;
        if extent <= 0.0 {
            continue;
        }
        let mut bin_bounds = [Bounds::EMPTY; BINS];
        let mut bin_count = [0usize; BINS];
        let scale = BINS as f32 / extent;
        for &i in indices {
            let b = (((centers[i as usize][axis] - lo) * scale) as usize).min(BINS - 1);
            bin_count[b] += 1;
            bin_bounds[b].union(&bounds[i as usize]);
        }
        // Sweep from the right to know the right side of every plane
        let mut right_area = [0.0f32; BINS];
        let mut right_count = [0usize; BINS];
        let mut acc = Bounds::EMPTY;
        let mut cnt = 0;
        for b in (1..BINS).rev() {
            acc.union(&bin_bounds[b]);
            cnt += bin_count[b];
            right_area[b] = acc.area();
            right_count[b] = cnt;
        }
        let mut acc = Bounds::EMPTY;
        let mut cnt = 0;
        for b in 0..BINS - 1 {
            acc.union(&bin_bounds[b]);
            cnt += bin_count[b];
            if cnt == 0 || right_count[b + 1] == 0 {
                continue;
            }
            let cost = acc.area() * cnt as f32 + right_area[b + 1] * right_count[b + 1] as f32;
            if best.is_none_or(|(_, _, c)| cost < c) {
                best = Some((axis, lo + (b + 1) as f32 / scale, cost));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_boxes(n: usize, seed: u32) -> Vec<Bounds> {
        let mut rng = crate::sampling::Rng::new(seed, 0);
        (0..n)
            .map(|_| {
                let c = vec3(rng.next_f32(), rng.next_f32(), rng.next_f32()) * 10.0;
                let s = vec3(rng.next_f32(), rng.next_f32(), rng.next_f32()) * 0.5;
                Bounds {
                    min: c - s,
                    max: c + s,
                }
            })
            .collect()
    }

    #[test]
    fn every_primitive_is_in_exactly_one_leaf() {
        let boxes = random_boxes(1000, 1);
        let bvh = Bvh::build(&boxes, 4);
        let mut seen = vec![0; boxes.len()];
        for node in &bvh.nodes {
            if node.count > 0 {
                assert!(node.count as usize <= 4);
                for i in node.first..node.first + node.count {
                    seen[bvh.indices[i as usize] as usize] += 1;
                }
            }
        }
        assert!(seen.iter().all(|&s| s == 1));
    }

    #[test]
    fn children_lie_inside_their_parent() {
        let boxes = random_boxes(500, 2);
        let bvh = Bvh::build(&boxes, 2);
        for node in &bvh.nodes {
            if node.count == 0 {
                for child in [node.first, node.first + 1] {
                    let c = &bvh.nodes[child as usize];
                    for a in 0..3 {
                        assert!(c.min[a] >= node.min[a] && c.max[a] <= node.max[a]);
                    }
                }
            }
        }
    }

    #[test]
    fn identical_boxes_end_in_one_leaf() {
        let boxes = vec![
            Bounds {
                min: vec3(0.0, 0.0, 0.0),
                max: vec3(1.0, 1.0, 1.0)
            };
            100
        ];
        let bvh = Bvh::build(&boxes, 4);
        assert_eq!(bvh.nodes.len(), 1);
        assert_eq!(bvh.nodes[0].count, 100);
    }

    #[test]
    fn empty_hierarchy_is_never_hit() {
        let bvh = Bvh::build(&[], 4);
        let mut t = f32::INFINITY;
        bvh.traverse(vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0), &mut t, |_, _| {
            panic!("visited")
        });
    }
}
