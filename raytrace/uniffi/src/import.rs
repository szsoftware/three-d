use crate::*;
use std::sync::Arc;
use three_d_asset::Geometry;
use three_d_raytrace::{MaterialId, RtMaterial};

/// One file of a model: the model itself or a file it refers to by name (an OBJ's MTL, textures, a glTF's .bin).
#[derive(uniffi::Record)]
pub struct ModelFile {
    /// The name as the model refers to it, e.g. `chair.mtl` or `textures/wood.jpg`; the model file's name tells the format
    /// by its extension: `.obj`, `.gltf` or `.glb`.
    pub name: String,
    /// The bytes.
    pub bytes: Arc<Upload>,
}

/// A part of an imported model: a mesh with its material and where it sits in the model.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ModelPart {
    /// The name in the file.
    pub name: String,
    /// Mesh id in the scene.
    pub mesh: u32,
    /// Material id in the scene.
    pub material: u32,
    /// From the part to the model, 16 values column major.
    pub transform: Vec<f32>,
}

/// An imported model: its parts are in the scene as meshes and materials, not yet placed.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ImportedModel {
    /// The parts.
    pub parts: Vec<ModelPart>,
    /// Smallest corner of the model's bounding box, in model units.
    pub min: V3,
    /// Largest corner.
    pub max: V3,
}

#[uniffi::export]
impl Scene {
    ///
    /// Reads a model (OBJ with MTL, glTF, GLB) with the files it refers to and adds its meshes, materials and textures.
    /// The parts are not placed: place them with `add_instance`, e.g. all parts of a chair with the chair's transform
    /// times the part's own. The files are read, not taken, so one texture file may serve several models.
    ///
    pub fn import_model(
        &self,
        main: String,
        files: Vec<ModelFile>,
    ) -> Result<ImportedModel, RtFfiError> {
        let mut raw = three_d_asset::io::RawAssets::new();
        for f in &files {
            let bytes = f.bytes.read(|b| b.to_vec())?;
            raw.insert(f.name.as_str(), bytes);
        }
        let model: three_d_asset::Model =
            raw.deserialize(main.as_str())
                .map_err(|e| RtFfiError::Import {
                    why: format!("{main}: {e}"),
                })?;
        self.with_rt_mut(|scene| -> Result<ImportedModel, RtFfiError> {
            let materials: Vec<MaterialId> = model
                .materials
                .iter()
                .map(|m| scene.add_cpu_material(m))
                .collect();
            let mut default_material = None;
            let mut parts = Vec::new();
            let (mut min, mut max) = (
                Vec3::new(f32::MAX, f32::MAX, f32::MAX),
                Vec3::new(f32::MIN, f32::MIN, f32::MIN),
            );
            for primitive in &model.geometries {
                if let Geometry::Triangles(mesh) = &primitive.geometry {
                    let aabb = mesh.compute_aabb();
                    for corner in [aabb.min(), aabb.max()] {
                        let q = primitive.transformation
                            * three_d_raytrace::vec4(corner.x, corner.y, corner.z, 1.0);
                        let p = vec3(q.x, q.y, q.z);
                        min = vec3(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
                        max = vec3(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
                    }
                    let id = scene.add_mesh(mesh)?;
                    let material = match primitive.material_index.and_then(|i| materials.get(i)) {
                        Some(m) => *m,
                        None => *default_material
                            .get_or_insert_with(|| scene.add_material(RtMaterial::default())),
                    };
                    let m: &[f32; 16] = primitive.transformation.as_ref();
                    parts.push(ModelPart {
                        name: primitive.name.clone(),
                        mesh: id.0,
                        material: material.0,
                        transform: m.to_vec(),
                    });
                }
            }
            if parts.is_empty() {
                return Err(RtFfiError::Import {
                    why: format!("{main} has no triangles"),
                });
            }
            Ok(ImportedModel {
                parts,
                min: min.into(),
                max: max.into(),
            })
        })
    }
}

use three_d_raytrace::{vec3, Vec3};
