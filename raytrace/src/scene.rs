//!
//! The scene description, independent of any backend.
//!

use crate::{RtError, RtTexture, Sky, TextureId};
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
    /// Texture coordinates for textured materials.
    pub uvs: Option<Vec<Vec2>>,
    /// Tangents for normal textures, the bitangent is cross(normal, tangent) · w as in three-d. Without them the tangents
    /// follow the texture coordinates of each triangle.
    pub tangents: Option<Vec<Vec4>>,
    /// Three vertex indices per triangle.
    pub triangles: Vec<[u32; 3]>,
}

impl RtMesh {
    /// Texture coordinates at barycentric coordinates `u`, `v` of a triangle, zero without texture coordinates.
    pub(crate) fn uv_at(&self, triangle: u32, u: f32, v: f32) -> Vec2 {
        match &self.uvs {
            Some(uvs) => {
                let [a, b, c] = self.triangles[triangle as usize];
                uvs[a as usize] * (1.0 - u - v) + uvs[b as usize] * u + uvs[c as usize] * v
            }
            None => vec2(0.0, 0.0),
        }
    }

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
        check(mesh.tangents.as_ref().map(|n| n.len()), "tangents")?;
        Ok(Self {
            positions,
            normals: mesh.normals.clone(),
            uvs: mesh.uvs.clone(),
            tangents: mesh.tangents.clone(),
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
    /// Multiplies the base color (sRGB) and the alpha.
    pub base_color_texture: Option<TextureId>,
    /// Multiplies the roughness (green) and the metallic value (blue), as in glTF.
    pub metallic_roughness_texture: Option<TextureId>,
    /// Normals in the tangent space of the texture coordinates, as in glTF.
    pub normal_texture: Option<TextureId>,
    /// Strength of the [RtMaterial::normal_texture] across the surface, 1 as painted.
    pub normal_scale: f32,
    /// Multiplies the emission (sRGB).
    pub emission_texture: Option<TextureId>,
    /// Multiplies the transmission (red).
    pub transmission_texture: Option<TextureId>,
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
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            emission_texture: None,
            transmission_texture: None,
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

impl RtMaterial {
    ///
    /// Whether the material uses any texture. Textures of a [PbrMaterial] are only taken over by
    /// [RtScene::add_cpu_material], which adds them to the scene.
    ///
    pub fn has_textures(&self) -> bool {
        self.base_color_texture.is_some()
            || self.metallic_roughness_texture.is_some()
            || self.normal_texture.is_some()
            || self.emission_texture.is_some()
            || self.transmission_texture.is_some()
    }

    /// Whether every part is cut out, without looking at textures.
    pub(crate) fn is_cut_out_everywhere(&self) -> bool {
        self.alpha_cutout.is_some_and(|c| self.alpha < c)
    }

    /// Whether some parts may be cut out by the base color texture, so rays have to test the alpha of every hit.
    pub(crate) fn cuts_out_by_texture(&self) -> bool {
        self.alpha_cutout.is_some() && self.base_color_texture.is_some()
    }

    /// The alpha at texture coordinates.
    pub(crate) fn alpha_at(&self, textures: &[RtTexture], uv: Vec2) -> f32 {
        match self
            .base_color_texture
            .and_then(|t| textures.get(t.0 as usize))
        {
            Some(t) => self.alpha * t.sample(uv).w,
            None => self.alpha,
        }
    }

    /// Whether the surface is cut out at texture coordinates.
    pub(crate) fn is_cut_out_at(&self, textures: &[RtTexture], uv: Vec2) -> bool {
        self.alpha_cutout
            .is_some_and(|c| self.alpha_at(textures, uv) < c)
    }

    ///
    /// The material at texture coordinates, with the textures applied and removed, and the normal of the normal texture
    /// in tangent space (z up) if there is one.
    ///
    pub(crate) fn at(&self, textures: &[RtTexture], uv: Vec2) -> (RtMaterial, Option<Vec3>) {
        let texture = |t: Option<TextureId>| t.and_then(|t| textures.get(t.0 as usize));
        let mut m = self.clone();
        if let Some(t) = texture(self.base_color_texture) {
            let c = t.sample_color(uv);
            m.base_color = vec3(
                m.base_color.x * c.x,
                m.base_color.y * c.y,
                m.base_color.z * c.z,
            );
            m.alpha *= c.w;
        }
        if let Some(t) = texture(self.metallic_roughness_texture) {
            let s = t.sample(uv);
            m.roughness *= s.y;
            m.metallic *= s.z;
        }
        if let Some(t) = texture(self.emission_texture) {
            let c = t.sample_color(uv);
            m.emission = vec3(m.emission.x * c.x, m.emission.y * c.y, m.emission.z * c.z);
        }
        if let Some(t) = texture(self.transmission_texture) {
            m.transmission *= t.sample(uv).x;
        }
        let normal = texture(self.normal_texture).map(|t| {
            let s = t.sample(uv);
            let n = vec3(
                (2.0 * s.x - 1.0) * self.normal_scale,
                (2.0 * s.y - 1.0) * self.normal_scale,
                2.0 * s.z - 1.0,
            );
            n.normalize()
        });
        m.base_color_texture = None;
        m.metallic_roughness_texture = None;
        m.normal_texture = None;
        m.emission_texture = None;
        m.transmission_texture = None;
        (m, normal)
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
            normal_scale: m.normal_scale,
            ..Default::default()
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
#[derive(Clone, Debug)]
pub struct RtScene {
    /// Tells scenes apart for the acceleration structures, a clone shares its meshes and keeps it
    id: u64,
    meshes: Vec<RtMesh>,
    textures: Vec<RtTexture>,
    materials: Vec<RtMaterial>,
    instances: Vec<RtInstance>,
    lights: Vec<RtLight>,
    environment: RtEnvironment,
    instance_revision: u64,
    revision: u64,
}

impl Default for RtScene {
    fn default() -> Self {
        Self {
            id: next_revision(),
            meshes: Vec::new(),
            textures: Vec::new(),
            materials: Vec::new(),
            instances: Vec::new(),
            lights: Vec::new(),
            environment: RtEnvironment::default(),
            instance_revision: 0,
            revision: 0,
        }
    }
}

/// Revisions and ids count across all scenes.
fn next_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl RtScene {
    /// An empty scene without light.
    pub fn new() -> Self {
        Self::default()
    }

    /// Meshes are only added, so a structure built for a scene with this id can be brought up to date.
    pub(crate) fn id(&self) -> u64 {
        self.id
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
            .map(|m| self.add_cpu_material(m))
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
        self.revision = next_revision();
        MeshId(self.meshes.len() as u32 - 1)
    }

    /// Adds a texture for materials.
    pub fn add_texture(&mut self, texture: RtTexture) -> TextureId {
        self.textures.push(texture);
        self.revision = next_revision();
        TextureId(self.textures.len() as u32 - 1)
    }

    ///
    /// Adds a material of three-d (`CpuMaterial`) with its textures: base color, metallic and roughness (also from the
    /// texture combined with occlusion), normals, emission and transmission. Occlusion textures are not needed, tracing
    /// finds the occlusion itself.
    ///
    pub fn add_cpu_material(&mut self, material: &PbrMaterial) -> MaterialId {
        let mut m = RtMaterial::from(material);
        let mut add = |t: &Option<three_d_asset::Texture2D>| {
            t.as_ref()
                .map(|t| self.add_texture(RtTexture::from_cpu_texture(t)))
        };
        m.base_color_texture = add(&material.albedo_texture);
        m.metallic_roughness_texture = match &material.metallic_roughness_texture {
            Some(_) => add(&material.metallic_roughness_texture),
            None => add(&material.occlusion_metallic_roughness_texture),
        };
        m.normal_texture = add(&material.normal_texture);
        m.emission_texture = add(&material.emissive_texture);
        m.transmission_texture = add(&material.transmission_texture);
        self.add_material(m)
    }

    ///
    /// Adds a material, either an [RtMaterial] or a [PbrMaterial](three_d_asset::PbrMaterial) (`CpuMaterial` in three-d).
    /// The textures of a `PbrMaterial` are left out here, [RtScene::add_cpu_material] takes them over.
    ///
    pub fn add_material(&mut self, material: impl Into<RtMaterial>) -> MaterialId {
        self.materials.push(material.into());
        self.revision = next_revision();
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
        self.revision = next_revision();
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
        self.revision = next_revision();
        Ok(())
    }

    /// Adds a light.
    pub fn add_light(&mut self, light: RtLight) {
        self.lights.push(light);
        self.revision = next_revision();
    }

    /// Removes all lights added with [RtScene::add_light].
    pub fn clear_lights(&mut self) {
        self.lights.clear();
        self.revision = next_revision();
    }

    /// Sets the light from outside the scene, for example a [Sky].
    pub fn set_environment(&mut self, environment: RtEnvironment) {
        self.environment = environment;
        self.revision = next_revision();
    }

    /// The meshes.
    pub fn meshes(&self) -> &[RtMesh] {
        &self.meshes
    }

    /// The textures.
    pub fn textures(&self) -> &[RtTexture] {
        &self.textures
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
    /// Grows with every change of the scene, and no two scenes share a revision once they were changed, so a tracer
    /// notices when it is given another scene. A render image showing the scene has to be reset when it changes.
    ///
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Grows when instances are added or moved, the top level of the acceleration structure is rebuilt then.
    pub fn instance_revision(&self) -> u64 {
        self.instance_revision
    }

    fn touch_instances(&mut self) {
        self.instance_revision = next_revision();
        self.revision = next_revision();
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
