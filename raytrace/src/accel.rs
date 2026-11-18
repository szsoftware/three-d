//!
//! Acceleration structures: one hierarchy per mesh (bottom level) and one over the instances (top level).
//!

use crate::bvh::{Bounds, Bvh};
use crate::{RtError, RtScene};
use three_d_asset::prelude::*;

///
/// Builds the acceleration structure of a scene for one kind of tracer.
///
/// The CPU implementation is [CpuBvhBuilder]. A GPU tracer brings its own, for example one that uploads the same
/// hierarchies into storage buffers or one that uses the hardware structures of wgpu.
///
pub trait AccelBuilder {
    /// The acceleration structure this builder makes.
    type Accel;

    /// Builds everything from scratch.
    fn build(&mut self, scene: &RtScene) -> Result<Self::Accel, RtError>;

    ///
    /// Brings the structure up to date after instances were added or moved. Only meshes that are new are built,
    /// the top level is built again.
    ///
    fn refit_instances(&mut self, accel: &mut Self::Accel, scene: &RtScene) -> Result<(), RtError>;
}

///
/// Builds a [CpuAccel] with the surface area heuristic.
///
#[derive(Clone, Copy, Debug)]
pub struct CpuBvhBuilder {
    /// Most triangles in one leaf of a mesh hierarchy.
    pub max_triangles_per_leaf: usize,
}

impl Default for CpuBvhBuilder {
    fn default() -> Self {
        Self {
            max_triangles_per_leaf: 4,
        }
    }
}

///
/// A two level bounding volume hierarchy in memory, traced on the CPU.
///
#[derive(Clone, Debug, Default)]
pub struct CpuAccel {
    meshes: Vec<Bvh>,
    mesh_bounds: Vec<Bounds>,
    instances: Bvh,
    instance_revision: u64,
    scene: u64,
}

///
/// Where a ray hits a triangle.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// Distance along the ray, in units of the ray direction's length.
    pub t: f32,
    /// Index of the instance.
    pub instance: u32,
    /// Index of the triangle in the instance's mesh.
    pub triangle: u32,
    /// Barycentric coordinate of the second vertex.
    pub u: f32,
    /// Barycentric coordinate of the third vertex.
    pub v: f32,
}

impl AccelBuilder for CpuBvhBuilder {
    type Accel = CpuAccel;

    fn build(&mut self, scene: &RtScene) -> Result<CpuAccel, RtError> {
        let mut accel = CpuAccel::default();
        self.refit_instances(&mut accel, scene)?;
        Ok(accel)
    }

    fn refit_instances(&mut self, accel: &mut CpuAccel, scene: &RtScene) -> Result<(), RtError> {
        if accel.scene != scene.id() {
            // Another scene: nothing can be kept
            *accel = CpuAccel {
                scene: scene.id(),
                ..Default::default()
            };
        }
        // Meshes never change once added, so only new ones are built
        for mesh in &scene.meshes()[accel.meshes.len()..] {
            let bounds: Vec<Bounds> = mesh
                .triangles
                .iter()
                .map(|t| {
                    let mut b = Bounds::EMPTY;
                    for &i in t {
                        b.grow(mesh.positions[i as usize]);
                    }
                    b
                })
                .collect();
            let bvh = Bvh::build(&bounds, self.max_triangles_per_leaf.max(1));
            let mut all = Bounds::EMPTY;
            bounds.iter().for_each(|b| all.union(b));
            accel.meshes.push(bvh);
            accel.mesh_bounds.push(all);
        }
        let bounds: Vec<Bounds> = scene
            .instances()
            .iter()
            .map(|i| accel.mesh_bounds[i.mesh.0 as usize].transformed(&i.transform))
            .collect();
        accel.instances = Bvh::build(&bounds, 1);
        accel.instance_revision = scene.instance_revision();
        Ok(())
    }
}

impl CpuAccel {
    /// Whether the structure matches the instances of the scene.
    pub fn is_up_to_date(&self, scene: &RtScene) -> bool {
        self.scene == scene.id()
            && self.meshes.len() == scene.meshes().len()
            && self.instance_revision == scene.instance_revision()
    }

    /// The hierarchy of each mesh, in the order of the meshes.
    pub fn mesh_hierarchies(&self) -> &[Bvh] {
        &self.meshes
    }

    /// The hierarchy over the instances.
    pub fn instance_hierarchy(&self) -> &Bvh {
        &self.instances
    }

    ///
    /// The closest triangle the ray hits before `t_max`, skipping surfaces cut out by their material.
    ///
    pub fn intersect(
        &self,
        scene: &RtScene,
        origin: Vec3,
        direction: Vec3,
        t_max: f32,
    ) -> Option<Hit> {
        let mut best = None;
        let mut t = t_max;
        self.walk(scene, origin, direction, &mut t, |hit, t_max| {
            *t_max = hit.t;
            best = Some(*hit);
            false
        });
        best
    }

    ///
    /// Calls `visit` for every triangle the ray hits before `t_max`, in no particular order, until it returns `true`.
    ///
    pub fn for_each_hit(
        &self,
        scene: &RtScene,
        origin: Vec3,
        direction: Vec3,
        t_max: f32,
        mut visit: impl FnMut(&Hit) -> bool,
    ) {
        let mut t = t_max;
        self.walk(scene, origin, direction, &mut t, |hit, _| visit(hit));
    }

    fn walk(
        &self,
        scene: &RtScene,
        origin: Vec3,
        direction: Vec3,
        t_max: &mut f32,
        mut on_hit: impl FnMut(&Hit, &mut f32) -> bool,
    ) {
        if self.instances.nodes.is_empty() {
            return;
        }
        let instances = scene.instances();
        let materials = scene.materials();
        let meshes = scene.meshes();
        self.instances
            .traverse(origin, direction, t_max, |instance_index, t_max| {
                let instance = &instances[instance_index as usize];
                let material = &materials[instance.material.0 as usize];
                if material.is_cut_out_everywhere() {
                    return false;
                }
                let cut_by_texture = material.cuts_out_by_texture();
                let mesh = &meshes[instance.mesh.0 as usize];
                // The same t on the ray in mesh coordinates, the direction is not normalised again
                let o = (instance.inverse * origin.extend(1.0)).truncate();
                let d = (instance.inverse * direction.extend(0.0)).truncate();
                let mut stop = false;
                self.meshes[instance.mesh.0 as usize].traverse(o, d, t_max, |triangle, t_max| {
                    let [a, b, c] = mesh.triangles[triangle as usize];
                    if let Some((t, u, v)) = intersect_triangle(
                        o,
                        d,
                        mesh.positions[a as usize],
                        mesh.positions[b as usize],
                        mesh.positions[c as usize],
                    ) {
                        if t < *t_max
                            && !(cut_by_texture
                                && material
                                    .is_cut_out_at(scene.textures(), mesh.uv_at(triangle, u, v)))
                        {
                            let hit = Hit {
                                t,
                                instance: instance_index,
                                triangle,
                                u,
                                v,
                            };
                            stop = on_hit(&hit, t_max);
                        }
                    }
                    stop
                });
                stop
            });
    }
}

/// Möller-Trumbore: distance and barycentric coordinates of the hit, if the ray hits the triangle in front of it.
#[inline]
fn intersect_triangle(o: Vec3, d: Vec3, p0: Vec3, p1: Vec3, p2: Vec3) -> Option<(f32, f32, f32)> {
    let e1 = p1 - p0;
    let e2 = p2 - p0;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-20 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - p0;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    if t > 0.0 {
        Some((t, u, v))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampling::Rng;
    use crate::{MeshId, RtMaterial, RtMesh};

    fn random_mesh(rng: &mut Rng, triangles: usize) -> RtMesh {
        let mut positions = Vec::new();
        for _ in 0..triangles {
            let c =
                vec3(rng.next_f32(), rng.next_f32(), rng.next_f32()) * 4.0 - vec3(2.0, 2.0, 2.0);
            for _ in 0..3 {
                positions.push(
                    c + (vec3(rng.next_f32(), rng.next_f32(), rng.next_f32())
                        - vec3(0.5, 0.5, 0.5))
                        * 0.6,
                );
            }
        }
        RtMesh {
            triangles: (0..triangles as u32)
                .map(|i| [3 * i, 3 * i + 1, 3 * i + 2])
                .collect(),
            positions,
            normals: None,
            uvs: None,
            tangents: None,
        }
    }

    /// The closest hit by testing every triangle of every instance.
    fn brute_force(scene: &RtScene, o: Vec3, d: Vec3) -> Option<(f32, u32, u32)> {
        let mut best: Option<(f32, u32, u32)> = None;
        for (ii, instance) in scene.instances().iter().enumerate() {
            let mesh = &scene.meshes()[instance.mesh.0 as usize];
            for (ti, [a, b, c]) in mesh.triangles.iter().enumerate() {
                let w = |i: &u32| {
                    (instance.transform * mesh.positions[*i as usize].extend(1.0)).truncate()
                };
                if let Some((t, _, _)) = intersect_triangle(o, d, w(a), w(b), w(c)) {
                    if best.is_none_or(|(bt, _, _)| t < bt) {
                        best = Some((t, ii as u32, ti as u32));
                    }
                }
            }
        }
        best
    }

    #[test]
    fn two_levels_find_the_same_hits_as_testing_everything() {
        let mut rng = Rng::new(42, 1);
        let mut scene = RtScene::new();
        let material = scene.add_material(RtMaterial::default());
        let meshes: Vec<MeshId> = (0..3)
            .map(|_| scene.add_rt_mesh(random_mesh(&mut rng, 200)))
            .collect();
        for i in 0..12 {
            let transform = Mat4::from_translation(vec3(i as f32 * 1.5 - 9.0, (i % 3) as f32, 0.0))
                * Mat4::from_angle_y(Rad(i as f32))
                * Mat4::from_nonuniform_scale(1.0, 0.5 + 0.1 * i as f32, 1.0);
            scene
                .add_instance(meshes[i % 3], material, transform)
                .unwrap();
        }
        let accel = CpuBvhBuilder::default().build(&scene).unwrap();
        let mut hits = 0;
        for _ in 0..2000 {
            let o = vec3(
                rng.next_f32() * 24.0 - 12.0,
                rng.next_f32() * 8.0 - 4.0,
                8.0,
            );
            let target = vec3(
                rng.next_f32() * 24.0 - 12.0,
                rng.next_f32() * 6.0 - 3.0,
                0.0,
            );
            let d = (target - o).normalize();
            let expected = brute_force(&scene, o, d);
            let got = accel.intersect(&scene, o, d, f32::INFINITY);
            match (expected, got) {
                (None, None) => {}
                (Some((t, ii, ti)), Some(hit)) => {
                    hits += 1;
                    assert!((t - hit.t).abs() < 1e-3 * t.max(1.0), "{t} vs {}", hit.t);
                    if (t - hit.t).abs() < 1e-6 {
                        assert_eq!((ii, ti), (hit.instance, hit.triangle));
                    }
                }
                (e, g) => panic!("expected {e:?}, got {g:?}"),
            }
        }
        assert!(hits > 500, "only {hits} rays hit anything");
    }

    #[test]
    fn moving_an_instance_moves_its_hits() {
        let mut scene = RtScene::new();
        let material = scene.add_material(RtMaterial::default());
        let mesh = scene.add_mesh(&three_d_asset::TriMesh::cube()).unwrap();
        let id = scene
            .add_instance(mesh, material, Mat4::identity())
            .unwrap();
        let mut builder = CpuBvhBuilder::default();
        let mut accel = builder.build(&scene).unwrap();
        let o = vec3(0.0, 0.0, 10.0);
        let d = vec3(0.0, 0.0, -1.0);
        assert!((accel.intersect(&scene, o, d, f32::INFINITY).unwrap().t - 9.0).abs() < 1e-5);
        scene
            .set_transform(id, Mat4::from_translation(vec3(0.0, 0.0, 3.0)))
            .unwrap();
        assert!(!accel.is_up_to_date(&scene));
        builder.refit_instances(&mut accel, &scene).unwrap();
        assert!(accel.is_up_to_date(&scene));
        assert!((accel.intersect(&scene, o, d, f32::INFINITY).unwrap().t - 6.0).abs() < 1e-5);
    }

    #[test]
    fn another_scene_is_built_again() {
        // Two scenes with the same number of changes and meshes must not share a structure
        let scene_at = |z: f32| {
            let mut scene = RtScene::new();
            let material = scene.add_material(RtMaterial::default());
            let mut mesh = three_d_asset::TriMesh::cube();
            mesh.transform(Mat4::from_translation(vec3(0.0, 0.0, z)))
                .unwrap();
            let mesh = scene.add_mesh(&mesh).unwrap();
            scene
                .add_instance(mesh, material, Mat4::identity())
                .unwrap();
            scene
        };
        let (a, b) = (scene_at(0.0), scene_at(3.0));
        let mut builder = CpuBvhBuilder::default();
        let mut accel = builder.build(&a).unwrap();
        assert!(!accel.is_up_to_date(&b));
        builder.refit_instances(&mut accel, &b).unwrap();
        let o = vec3(0.0, 0.0, 10.0);
        let d = vec3(0.0, 0.0, -1.0);
        assert!((accel.intersect(&b, o, d, f32::INFINITY).unwrap().t - 6.0).abs() < 1e-5);
    }

    #[test]
    fn cut_out_material_is_not_hit() {
        let mut scene = RtScene::new();
        let leaf = scene.add_material(RtMaterial {
            alpha: 0.2,
            alpha_cutout: Some(0.5),
            ..Default::default()
        });
        let mesh = scene.add_mesh(&three_d_asset::TriMesh::cube()).unwrap();
        scene.add_instance(mesh, leaf, Mat4::identity()).unwrap();
        let accel = CpuBvhBuilder::default().build(&scene).unwrap();
        assert!(accel
            .intersect(
                &scene,
                vec3(0.0, 0.0, 10.0),
                vec3(0.0, 0.0, -1.0),
                f32::INFINITY
            )
            .is_none());
    }
}
