//!
//! The scene description, independent of any backend.
//!

use crate::{RtError, Sky};
use three_d_asset::prelude::*;
use three_d_asset::{Geometry, Model, PbrMaterial, TriMesh};

/// Identifies a mesh of an [RtScene].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshId(pub u32);

/// Identifies a material of an [RtScene].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u32);

/// Identifies an instance of an [RtScene].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InstanceId(pub u32);

///
/// Triangles in their own coordinate system, shared by all instances that use them.
///
#[derive(Clone, Debug)]
pub struct RtMesh {
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Vertex normals for smooth shading, the face normal is used without them.
    pub normals: Option<Vec<Vec3>>,
    /// Texture coordinates, kept for textured materials.
    pub uvs: Option<Vec<Vec2>>,
    /// Three vertex indices per triangle.
    pub triangles: Vec<[u32; 3]>,
}

impl RtMesh {
    ///
    /// Copies the triangles of a [TriMesh](three_d_asset::TriMesh) (`CpuMesh` in three-d).
    ///
    pub fn from_cpu_mesh(mesh: &TriMesh) -> Result<Self, RtError> {
        let positions = mesh.positions.to_f32();
        let count = positions.len();
        let triangles: Vec<[u32; 3]> = match mesh.indices.to_u32() {
            Some(indices) => {
                if !indices.len().is_multiple_of(3) {
                    return Err(RtError::InvalidMesh(format!(
                        "{} indices is not a multiple of 3",
                        indices.len()
                    )));
                }
                indices.as_chunks::<3>().0.to_vec()
            }
            None => {
                if !count.is_multiple_of(3) {
                    return Err(RtError::InvalidMesh(format!(
                        "{count} positions without indices is not a multiple of 3"
                    )));
                }
                (0..count as u32 / 3)
                    .map(|i| [3 * i, 3 * i + 1, 3 * i + 2])
                    .collect()
            }
        };
        if let Some(bad) = triangles.iter().flatten().find(|&&i| i as usize >= count) {
            return Err(RtError::InvalidMesh(format!(
                "index {bad} is out of range for {count} positions"
            )));
        }
        let check = |len: Option<usize>, what: &str| match len {
            Some(len) if len != count => Err(RtError::InvalidMesh(format!(
                "{len} {what} for {count} positions"
            ))),
            _ => Ok(()),
        };
        check(mesh.normals.as_ref().map(|n| n.len()), "normals")?;
        check(mesh.uvs.as_ref().map(|n| n.len()), "uvs")?;
        Ok(Self {
            positions,
            normals: mesh.normals.clone(),
            uvs: mesh.uvs.clone(),
            triangles,
        })
    }
}

///
/// A physically based material, the same model as [PbrMaterial](three_d_asset::PbrMaterial) (`CpuMaterial` in
/// three-d) plus a few settings that only matter when tracing. Colors are linear sRGB.
///
#[derive(Clone, Debug, PartialEq)]
pub struct RtMaterial {
    /// Base color, the diffuse color of non-metals and the reflection color of metals.
    pub base_color: Vec3,
    /// Opacity, only used together with [RtMaterial::alpha_cutout].
    pub alpha: f32,
    /// 0 for non-metals, 1 for metals.
    pub metallic: f32,
    /// Microfacet roughness from 0 (mirror) to 1 (matte).
    pub roughness: f32,
    /// Strength of the specular reflection of non-metals, 1 is the usual 4 % at normal incidence and 0 is a purely
    /// diffuse surface.
    pub specular: f32,
    /// Emitted luminance in cd/m².
    pub emission: Vec3,
    /// Part of the light that passes through (glass), from 0 to 1.
    pub transmission: f32,
    /// Index of refraction, 1.5 for glass.
    pub index_of_refraction: f32,
    /// Surfaces with an [RtMaterial::alpha] below this value are not there at all, as for leaves.
    pub alpha_cutout: Option<f32>,
    /// Transmissive surfaces are thin sheets (window panes modelled as one face) instead of the boundary of a solid.
    pub thin_walled: bool,
}

impl Default for RtMaterial {
    fn default() -> Self {
        Self {
            base_color: vec3(0.8, 0.8, 0.8),
            alpha: 1.0,
            metallic: 0.0,
            roughness: 0.5,
            specular: 1.0,
            emission: vec3(0.0, 0.0, 0.0),
            transmission: 0.0,
            index_of_refraction: 1.5,
            alpha_cutout: None,
            thin_walled: true,
        }
    }
}

impl RtMaterial {
    ///
    /// The luminance in cd/m² given to the full white emissive color of a [PbrMaterial], about a bright lamp shade.
    ///
    pub const EMISSIVE_LUMINANCE: f32 = 1000.0;

    /// A matte, purely diffuse surface.
    pub fn diffuse(color: Vec3) -> Self {
        Self {
            base_color: color,
            roughness: 1.0,
            specular: 0.0,
            ..Default::default()
        }
    }

    /// A clear, thin pane of glass.
    pub fn glass() -> Self {
        Self {
            base_color: vec3(1.0, 1.0, 1.0),
            roughness: 0.0,
            transmission: 1.0,
            ..Default::default()
        }
    }
}

impl From<&PbrMaterial> for RtMaterial {
    fn from(m: &PbrMaterial) -> Self {
        let albedo = m.albedo.to_linear_srgb();
        let emissive = m.emissive.to_linear_srgb().truncate();
        Self {
            base_color: albedo.truncate(),
            alpha: albedo.w,
            metallic: m.metallic.clamp(0.0, 1.0),
            roughness: m.roughness.clamp(0.0, 1.0),
            specular: 1.0,
            emission: emissive * Self::EMISSIVE_LUMINANCE,
            transmission: m.transmission.clamp(0.0, 1.0),
            index_of_refraction: m.index_of_refraction.max(1.0),
            alpha_cutout: m.alpha_cutout,
            thin_walled: true,
        }
    }
}

impl From<PbrMaterial> for RtMaterial {
    fn from(m: PbrMaterial) -> Self {
        Self::from(&m)
    }
}

///
/// A mesh placed in the scene with a material.
///
#[derive(Clone, Debug)]
pub struct RtInstance {
    /// The mesh.
    pub mesh: MeshId,
    /// The material.
    pub material: MaterialId,
    /// From mesh to world coordinates.
    pub transform: Mat4,
    pub(crate) inverse: Mat4,
}

///
/// A light source in addition to the sky and emissive surfaces.
///
#[derive(Clone, Debug, PartialEq)]
pub enum RtLight {
    /// Parallel light from far away, with hard shadows.
    Directional {
        /// The direction the light travels in.
        direction: Vec3,
        /// Linear color, scaled by the illuminance.
        color: Vec3,
        /// Illuminance in lux on a surface facing the light.
        illuminance: f32,
    },
    /// Light from one point in all directions, with hard shadows.
    Point {
        /// Position in world coordinates.
        position: Vec3,
        /// Linear color, scaled by the intensity.
        color: Vec3,
        /// Luminous intensity in cd, about 80 for a 1000 lm bulb.
        intensity: f32,
    },
}

///
/// The light coming from everywhere a ray leaves the scene.
///
#[derive(Clone, Debug)]
pub enum RtEnvironment {
    /// The same luminance from every direction.
    Uniform {
        /// Linear color, scaled by the luminance.
        color: Vec3,
        /// Luminance in cd/m².
        luminance: f32,
    },
    /// A daylight sky with the sun.
    Sky(Sky),
}

impl Default for RtEnvironment {
    fn default() -> Self {
        Self::Uniform {
            color: vec3(1.0, 1.0, 1.0),
            luminance: 0.0,
        }
    }
}

///
/// Everything a tracer needs: meshes, materials, instances, lights and the environment.
///
/// Meshes and materials are added once and referred to by id. Instances place a mesh with a material, many instances
/// can share a mesh, which keeps vegetation and repeated parts small. Changing a transform only rebuilds the top level
/// of the acceleration structure.
///
#[derive(Clone, Debug, Default)]
pub struct RtScene {
    meshes: Vec<RtMesh>,
    materials: Vec<RtMaterial>,
    instances: Vec<RtInstance>,
    lights: Vec<RtLight>,
    environment: RtEnvironment,
    instance_revision: u64,
    revision: u64,
}

impl RtScene {
    /// An empty scene without light.
    pub fn new() -> Self {
        Self::default()
    }

    ///
    /// A scene with all primitives of a model (`CpuModel` in three-d), each one an instance with its material.
    /// Point clouds are skipped.
    ///
    pub fn from_cpu_model(model: &Model) -> Result<Self, RtError> {
        let mut scene = Self::new();
        scene.add_cpu_model(model, Mat4::identity())?;
        Ok(scene)
    }

    ///
    /// Adds all primitives of a model with an extra transformation and returns the new instances.
    /// Point clouds are skipped, primitives without a material get the default material.
    ///
    pub fn add_cpu_model(
        &mut self,
        model: &Model,
        transform: Mat4,
    ) -> Result<Vec<InstanceId>, RtError> {
        let materials: Vec<MaterialId> = model
            .materials
            .iter()
            .map(|m| self.add_material(m))
            .collect();
        let mut default_material = None;
        let mut instances = Vec::new();
        for primitive in &model.geometries {
            if let Geometry::Triangles(mesh) = &primitive.geometry {
                let mesh = self.add_mesh(mesh)?;
                let material = match primitive.material_index.and_then(|i| materials.get(i)) {
                    Some(m) => *m,
                    None => *default_material
                        .get_or_insert_with(|| self.add_material(RtMaterial::default())),
                };
                instances.push(self.add_instance(
                    mesh,
                    material,
                    transform * primitive.transformation,
                )?);
            }
        }
        Ok(instances)
    }

    /// Adds a mesh (`CpuMesh` in three-d).
    pub fn add_mesh(&mut self, mesh: &TriMesh) -> Result<MeshId, RtError> {
        let mesh = RtMesh::from_cpu_mesh(mesh)?;
        Ok(self.add_rt_mesh(mesh))
    }

    /// Adds a mesh that is already in the tracer's own format.
    pub fn add_rt_mesh(&mut self, mesh: RtMesh) -> MeshId {
        self.meshes.push(mesh);
        self.revision += 1;
        MeshId(self.meshes.len() as u32 - 1)
    }

    /// Adds a material, either an [RtMaterial] or a [PbrMaterial](three_d_asset::PbrMaterial) (`CpuMaterial` in three-d).
    pub fn add_material(&mut self, material: impl Into<RtMaterial>) -> MaterialId {
        self.materials.push(material.into());
        self.revision += 1;
        MaterialId(self.materials.len() as u32 - 1)
    }

    /// Places a mesh with a material.
    pub fn add_instance(
        &mut self,
        mesh: MeshId,
        material: MaterialId,
        transform: Mat4,
    ) -> Result<InstanceId, RtError> {
        if mesh.0 as usize >= self.meshes.len() {
            return Err(RtError::UnknownMesh(mesh.0));
        }
        if material.0 as usize >= self.materials.len() {
            return Err(RtError::UnknownMaterial(material.0));
        }
        self.instances.push(RtInstance {
            mesh,
            material,
            transform,
            inverse: transform.invert().unwrap_or(Mat4::identity()),
        });
        self.touch_instances();
        Ok(InstanceId(self.instances.len() as u32 - 1))
    }

    /// Moves an instance.
    pub fn set_transform(&mut self, id: InstanceId, transform: Mat4) -> Result<(), RtError> {
        let instance = self
            .instances
            .get_mut(id.0 as usize)
            .ok_or(RtError::UnknownInstance(id.0))?;
        instance.transform = transform;
        instance.inverse = transform.invert().unwrap_or(Mat4::identity());
        self.touch_instances();
        Ok(())
    }

    /// Gives an instance another material.
    pub fn set_instance_material(
        &mut self,
        id: InstanceId,
        material: MaterialId,
    ) -> Result<(), RtError> {
        if material.0 as usize >= self.materials.len() {
            return Err(RtError::UnknownMaterial(material.0));
        }
        self.instances
            .get_mut(id.0 as usize)
            .ok_or(RtError::UnknownInstance(id.0))?
            .material = material;
        self.revision += 1;
        Ok(())
    }

    /// Changes a material.
    pub fn set_material(
        &mut self,
        id: MaterialId,
        material: impl Into<RtMaterial>,
    ) -> Result<(), RtError> {
        *self
            .materials
            .get_mut(id.0 as usize)
            .ok_or(RtError::UnknownMaterial(id.0))? = material.into();
        self.revision += 1;
        Ok(())
    }

    /// Adds a light.
    pub fn add_light(&mut self, light: RtLight) {
        self.lights.push(light);
        self.revision += 1;
    }

    /// Removes all lights added with [RtScene::add_light].
    pub fn clear_lights(&mut self) {
        self.lights.clear();
        self.revision += 1;
    }

    /// Sets the light from outside the scene, for example a [Sky].
    pub fn set_environment(&mut self, environment: RtEnvironment) {
        self.environment = environment;
        self.revision += 1;
    }

    /// The meshes.
    pub fn meshes(&self) -> &[RtMesh] {
        &self.meshes
    }

    /// The materials.
    pub fn materials(&self) -> &[RtMaterial] {
        &self.materials
    }

    /// The instances.
    pub fn instances(&self) -> &[RtInstance] {
        &self.instances
    }

    /// The lights.
    pub fn lights(&self) -> &[RtLight] {
        &self.lights
    }

    /// The environment.
    pub fn environment(&self) -> &RtEnvironment {
        &self.environment
    }

    ///
    /// Grows with every change of the scene. A render image showing the scene has to be reset when it changes.
    ///
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Grows when instances are added or moved, the top level of the acceleration structure is rebuilt then.
    pub fn instance_revision(&self) -> u64 {
        self.instance_revision
    }

    fn touch_instances(&mut self) {
        self.instance_revision += 1;
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_without_indices_is_a_triangle_list() {
        let mesh = RtMesh::from_cpu_mesh(&TriMesh::square()).unwrap();
        assert_eq!(mesh.triangles.len(), 2);
        let mut soup = TriMesh::square();
        soup.positions = three_d_asset::Positions::F32(vec![vec3(0.0, 0.0, 0.0); 6]);
        soup.indices = three_d_asset::Indices::None;
        soup.normals = None;
        soup.uvs = None;
        soup.tangents = None;
        assert_eq!(
            RtMesh::from_cpu_mesh(&soup).unwrap().triangles,
            vec![[0, 1, 2], [3, 4, 5]]
        );
    }

    #[test]
    fn broken_meshes_are_refused() {
        let mut mesh = TriMesh::square();
        mesh.indices = three_d_asset::Indices::U32(vec![0, 1, 7]);
        assert!(matches!(
            RtMesh::from_cpu_mesh(&mesh),
            Err(RtError::InvalidMesh(_))
        ));
    }

    #[test]
    fn unknown_ids_are_refused() {
        let mut scene = RtScene::new();
        let m = scene.add_material(RtMaterial::default());
        assert_eq!(
            scene
                .add_instance(MeshId(0), m, Mat4::identity())
                .unwrap_err(),
            RtError::UnknownMesh(0)
        );
    }

    #[test]
    fn cpu_material_converts_to_linear_colors() {
        let m = RtMaterial::from(&PbrMaterial {
            albedo: Srgba::new(255, 0, 0, 128),
            metallic: 0.3,
            roughness: 0.7,
            ..Default::default()
        });
        assert_eq!(m.base_color, vec3(1.0, 0.0, 0.0));
        assert!((m.alpha - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!((m.metallic, m.roughness), (0.3, 0.7));
    }

    #[test]
    fn moving_an_instance_only_touches_the_instances() {
        let mut scene = RtScene::new();
        let mesh = scene.add_mesh(&TriMesh::cube()).unwrap();
        let mat = scene.add_material(RtMaterial::default());
        let id = scene.add_instance(mesh, mat, Mat4::identity()).unwrap();
        let (r, i) = (scene.revision(), scene.instance_revision());
        scene.set_transform(id, Mat4::from_scale(2.0)).unwrap();
        assert!(scene.revision() > r && scene.instance_revision() > i);
        let i = scene.instance_revision();
        scene.add_light(RtLight::Point {
            position: vec3(0.0, 0.0, 0.0),
            color: vec3(1.0, 1.0, 1.0),
            intensity: 1.0,
        });
        assert_eq!(scene.instance_revision(), i);
    }
}
