use crate::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use three_d_asset::{Indices, Positions};
use three_d_raytrace::{
    CpuMesh, InstanceId, MaterialId, MeshId, RtEnvironment, RtScene, RtTexture, Vec2, Vec3,
};

///
/// A scene: meshes, textures, materials, instances, lights and the environment, referred to by ids as in `RtScene`.
/// It stays changeable after it is built: transforms, materials of instances, materials, lights. A preview or a
/// realtime view of it notices changes by itself.
///
#[derive(uniffi::Object)]
pub struct Scene {
    inner: Mutex<SceneState>,
}

pub(crate) struct SceneState {
    pub scene: RtScene,
    builtins: HashMap<(Builtin, u32), u32>,
}

#[uniffi::export]
impl Scene {
    /// An empty scene: no light, a black environment.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Self::from_rt(RtScene::new())
    }
}

/// One of the demo scenes of three-d-raytrace (garden, pergola, aquarium, caustics, downlights) with its camera and look.
#[uniffi::export]
pub fn demo_scene(name: String) -> Result<DemoInfo, RtFfiError> {
    let demo = three_d_raytrace::scenes::all()
        .into_iter()
        .find(|d| d.name == name)
        .ok_or_else(|| RtFfiError::UnknownId {
            what: format!("demo scene {name}"),
        })?;
    Ok(DemoInfo {
        name: demo.name.to_owned(),
        title: demo.title.to_owned(),
        description: demo.description.to_owned(),
        camera: CameraSpec::from_camera(&demo.camera(16, 9)),
        look: Look::from_demo(&demo.tone_mapping, &demo.settings),
        scene: Scene::from_rt(demo.scene),
    })
}

#[uniffi::export]
impl Scene {
    /// One of the meshes every scene has, added once per kind and subdivisions and shared after that.
    pub fn builtin_mesh(&self, kind: Builtin, subdivisions: u32) -> u32 {
        let mut s = self.lock();
        if let Some(&id) = s.builtins.get(&(kind, subdivisions)) {
            return id;
        }
        let n = subdivisions.max(3);
        let mesh = match kind {
            Builtin::Cube => CpuMesh::cube(),
            Builtin::Sphere => CpuMesh::sphere(n),
            Builtin::Square => CpuMesh::square(),
            Builtin::Cylinder => CpuMesh::cylinder(n),
        };
        let id = s
            .scene
            .add_mesh(&mesh)
            .expect("the builtin meshes are valid")
            .0;
        s.builtins.insert((kind, subdivisions), id);
        id
    }

    ///
    /// A mesh from arrays: 3 positions per vertex, 3 indices per triangle, optional normals (3 per vertex) and texture
    /// coordinates (2 per vertex). uniffi copies the arrays; for large meshes use [Scene::add_uploaded_mesh].
    ///
    pub fn add_mesh(
        &self,
        positions: Vec<f32>,
        indices: Vec<u32>,
        normals: Option<Vec<f32>>,
        uvs: Option<Vec<f32>>,
    ) -> Result<u32, RtFfiError> {
        let mesh = cpu_mesh(positions, indices, normals, uvs)?;
        Ok(self.lock().scene.add_mesh(&mesh)?.0)
    }

    /// A mesh Kotlin wrote into a [MeshUpload]; the upload is empty afterwards.
    pub fn add_uploaded_mesh(&self, upload: Arc<MeshUpload>) -> Result<u32, RtFfiError> {
        let d = upload.take()?;
        self.add_mesh(d.positions, d.indices, d.normals, d.uvs)
    }

    ///
    /// A texture from a PNG or JPEG file in `file` (decoded here), or from raw RGBA8 pixels if `width` and `height` are
    /// given. `srgb` for colors, false for normal and roughness maps. `repeat` lets it wrap around.
    ///
    pub fn add_texture(
        &self,
        file: Arc<Upload>,
        srgb: bool,
        repeat: bool,
    ) -> Result<u32, RtFfiError> {
        let mut texture = file.read(decode_texture)??;
        texture.linear = !srgb;
        if !repeat {
            texture.wrap_u = three_d_raytrace::RtWrapping::ClampToEdge;
            texture.wrap_v = three_d_raytrace::RtWrapping::ClampToEdge;
        }
        Ok(self.lock().scene.add_texture(texture).0)
    }

    /// Raw RGBA8 pixels, rows top first, `width * height * 4` bytes in `pixels`.
    pub fn add_texture_rgba(
        &self,
        width: u32,
        height: u32,
        pixels: Arc<Upload>,
        srgb: bool,
    ) -> Result<u32, RtFfiError> {
        let bytes = pixels.take()?;
        if bytes.len() != width as usize * height as usize * 4 {
            return Err(invalid(format!(
                "{} bytes for {width} x {height} RGBA8 pixels",
                bytes.len()
            )));
        }
        let mut texture = RtTexture::new(width, height, bytes.as_chunks::<4>().0.to_vec());
        texture.linear = !srgb;
        Ok(self.lock().scene.add_texture(texture).0)
    }

    /// A material, its id for instances.
    pub fn add_material(&self, material: Material) -> Result<u32, RtFfiError> {
        let mut s = self.lock();
        let m = material.to_rt(s.scene.textures().len() as u32)?;
        Ok(s.scene.add_material(m).0)
    }

    /// Changes a material for every instance made of it.
    pub fn set_material(&self, id: u32, material: Material) -> Result<(), RtFfiError> {
        let mut s = self.lock();
        let m = material.to_rt(s.scene.textures().len() as u32)?;
        Ok(s.scene.set_material(MaterialId(id), m)?)
    }

    /// The material as it is, for example one that came with an imported model.
    pub fn material(&self, id: u32) -> Result<Material, RtFfiError> {
        let s = self.lock();
        let m = s
            .scene
            .materials()
            .get(id as usize)
            .ok_or_else(|| RtFfiError::UnknownId {
                what: format!("material {id}"),
            })?;
        Ok(Material::from_rt(m))
    }

    /// Places a mesh with a material; `transform` is 16 values, column major.
    pub fn add_instance(
        &self,
        mesh: u32,
        material: u32,
        transform: Vec<f32>,
    ) -> Result<u32, RtFfiError> {
        let t = mat4(&transform)?;
        Ok(self
            .lock()
            .scene
            .add_instance(MeshId(mesh), MaterialId(material), t)?
            .0)
    }

    /// Many instances of one mesh: 16 `f32` per transform in `transforms`. Returns the id of the first, the others follow.
    pub fn add_instances(
        &self,
        mesh: u32,
        material: u32,
        transforms: Arc<Upload>,
    ) -> Result<u32, RtFfiError> {
        let values = transforms.take_f32()?;
        if values.len() % 16 != 0 || values.is_empty() {
            return Err(invalid(format!(
                "{} values are not a whole number of transforms",
                values.len()
            )));
        }
        let mut s = self.lock();
        let first = s.scene.instances().len() as u32;
        for chunk in values.as_chunks::<16>().0 {
            s.scene
                .add_instance(MeshId(mesh), MaterialId(material), mat4(chunk)?)?;
        }
        Ok(first)
    }

    /// Moves an instance; only the top level of the acceleration structure is rebuilt.
    pub fn set_transform(&self, instance: u32, transform: Vec<f32>) -> Result<(), RtFfiError> {
        let t = mat4(&transform)?;
        Ok(self.lock().scene.set_transform(InstanceId(instance), t)?)
    }

    /// Gives an instance another material.
    pub fn set_instance_material(&self, instance: u32, material: u32) -> Result<(), RtFfiError> {
        Ok(self
            .lock()
            .scene
            .set_instance_material(InstanceId(instance), MaterialId(material))?)
    }

    /// Adds a light.
    pub fn add_light(&self, light: Light) {
        self.lock().scene.add_light(light.into());
    }

    /// Removes all lights, the sky stays.
    pub fn clear_lights(&self) {
        self.lock().scene.clear_lights();
    }

    /// A daylight sky with the sun for a place and time.
    pub fn set_sky(&self, sky: SkySpec) {
        self.lock()
            .scene
            .set_environment(RtEnvironment::Sky((&sky).into()));
    }

    /// The same luminance from every direction, e.g. a white studio or black for a night scene.
    pub fn set_uniform_environment(&self, color: V3, luminance: f32) {
        self.lock().scene.set_environment(RtEnvironment::Uniform {
            color: color.into(),
            luminance,
        });
    }

    /// Haze over the scene, `None` for clear air.
    pub fn set_fog(&self, fog: Option<Fog>) {
        self.lock().scene.set_fog(fog.map(Into::into));
    }

    /// Numbers of meshes, materials, textures, instances and lights.
    pub fn counts(&self) -> SceneCounts {
        let s = self.lock();
        SceneCounts {
            meshes: s.scene.meshes().len() as u32,
            materials: s.scene.materials().len() as u32,
            textures: s.scene.textures().len() as u32,
            instances: s.scene.instances().len() as u32,
            lights: s.scene.lights().len() as u32,
        }
    }
}

impl Scene {
    /// A scene from an `RtScene` built in Rust.
    pub fn from_rt(scene: RtScene) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(SceneState {
                scene,
                builtins: HashMap::new(),
            }),
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, SceneState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reads the `RtScene`, for example to render it from Rust.
    pub fn with_rt<R>(&self, f: impl FnOnce(&RtScene) -> R) -> R {
        f(&self.lock().scene)
    }

    pub(crate) fn with_rt_mut<R>(&self, f: impl FnOnce(&mut RtScene) -> R) -> R {
        f(&mut self.lock().scene)
    }
}

/// What a scene holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct SceneCounts {
    /// Meshes.
    pub meshes: u32,
    /// Materials.
    pub materials: u32,
    /// Textures.
    pub textures: u32,
    /// Instances.
    pub instances: u32,
    /// Lights besides the sky.
    pub lights: u32,
}

/// A demo scene with the camera and look that fit it.
#[derive(uniffi::Record)]
pub struct DemoInfo {
    /// Short name without spaces.
    pub name: String,
    /// Name to show.
    pub title: String,
    /// One or two sentences.
    pub description: String,
    /// The start camera.
    pub camera: CameraSpec,
    /// Exposure, bounces and caustic photons.
    pub look: Look,
    /// The scene.
    pub scene: Arc<Scene>,
}

/// The names of the demo scenes, in the order to show them.
#[uniffi::export]
pub fn demo_scene_names() -> Vec<String> {
    three_d_raytrace::scenes::all()
        .into_iter()
        .map(|d| d.name.to_owned())
        .collect()
}

/// An IES profile of a luminaire, shared by all luminaires of the kind.
#[derive(Debug, uniffi::Object)]
pub struct Ies {
    pub(crate) profile: Arc<three_d_raytrace::IesProfile>,
}

#[uniffi::export]
impl Ies {
    /// Lumen the profile sends out as measured.
    pub fn flux(&self) -> f32 {
        self.profile.flux()
    }
}

/// Reads an IES file (LM-63).
#[uniffi::export]
pub fn parse_ies(text: String) -> Result<Arc<Ies>, RtFfiError> {
    Ok(Arc::new(Ies {
        profile: Arc::new(three_d_raytrace::IesProfile::parse(&text)?),
    }))
}

pub(crate) fn cpu_mesh(
    positions: Vec<f32>,
    indices: Vec<u32>,
    normals: Option<Vec<f32>>,
    uvs: Option<Vec<f32>>,
) -> Result<CpuMesh, RtFfiError> {
    if !positions.len().is_multiple_of(3) || !indices.len().is_multiple_of(3) {
        return Err(invalid(format!(
            "{} position values and {} indices are not whole vertices and triangles",
            positions.len(),
            indices.len()
        )));
    }
    let v3 = |values: Vec<f32>| -> Vec<Vec3> {
        values
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| Vec3::new(c[0], c[1], c[2]))
            .collect()
    };
    Ok(CpuMesh {
        positions: Positions::F32(v3(positions)),
        indices: Indices::U32(indices),
        normals: normals.map(v3),
        uvs: uvs.map(|values| {
            values
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| Vec2::new(c[0], c[1]))
                .collect()
        }),
        ..Default::default()
    })
}

fn decode_texture(bytes: &[u8]) -> Result<RtTexture, RtFfiError> {
    let image =
        image::load_from_memory(bytes).map_err(|e| RtFfiError::Import { why: e.to_string() })?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(RtTexture::new(
        width,
        height,
        rgba.pixels().map(|p| p.0).collect(),
    ))
}
