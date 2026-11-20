use crate::render::{rgba_texture, PreviewState, Readback};
use crate::thread::ThreadBound;
use crate::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use three_d::*;
use three_d_raytrace::{RtEnvironment, RtLight, RtMaterial, RtScene};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// How mouse and keys move the camera.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum Navigation {
    /// Turns around `target` with the left button, zooms with the wheel between the distances (three-d's OrbitControl).
    Orbit {
        /// The point to turn around.
        target: V3,
        /// Closest distance.
        min_distance: f32,
        /// Farthest distance.
        max_distance: f32,
    },
    /// Walks at eye height: W, A, S, D or the arrows, looking around with the left button; meters per second.
    Walk {
        /// Height of the eyes over the floor at y = 0.
        eye_height: f32,
        /// Meters per second.
        speed: f32,
    },
    /// Flies freely (three-d's FlyControl): left button looks, right button moves, wheel moves forward.
    Fly {
        /// Meters per wheel step.
        speed: f32,
    },
}

/// A pointer or key event of the application, pixels from the top left as in Compose.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum InputEvent {
    /// A button went down: 0 left, 1 right, 2 middle.
    Press {
        /// x in pixels from the left.
        x: f32,
        /// y in pixels from the top.
        y: f32,
        /// 0 left, 1 right, 2 middle.
        button: u32,
    },
    /// A button went up.
    Release {
        /// x in pixels from the left.
        x: f32,
        /// y in pixels from the top.
        y: f32,
        /// 0 left, 1 right, 2 middle.
        button: u32,
    },
    /// The pointer moved, with the button held if any.
    Move {
        /// x in pixels from the left.
        x: f32,
        /// y in pixels from the top.
        y: f32,
        /// Pixels moved right since the last move.
        dx: f32,
        /// Pixels moved down since the last move.
        dy: f32,
        /// The button held: 0 left, 1 right, 2 middle, `None` for none.
        button: Option<u32>,
    },
    /// The wheel turned; positive away from the user.
    Wheel {
        /// x in pixels from the left.
        x: f32,
        /// y in pixels from the top.
        y: f32,
        /// Steps.
        delta: f32,
    },
    /// A key went down or up: "W", "A", "S", "D", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Shift".
    Key {
        /// The key.
        key: String,
        /// Down or up.
        down: bool,
    },
}

/// What a pick hit.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Hit {
    /// The instance id in the scene.
    pub instance: u32,
    /// Where it was hit, in world coordinates.
    pub position: V3,
}

/// Raster settings of the realtime view.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct RealtimeSpec {
    /// Size of the sun's shadow map, 0 for no shadows.
    #[uniffi(default = 2048)]
    pub sun_shadow_map: u32,
    /// Hands over to the raytraced preview after the camera stood still this long, `None` never.
    pub raytrace_when_still_ms: Option<u32>,
    /// Exposure and tone curve, the same as the raytracer's.
    pub look: Look,
}

/// The realtime view of a scene: three-d's rasterizer at frame rate, see [Renderer::scene_view].
#[derive(uniffi::Object)]
pub struct SceneView {
    scene: Arc<Scene>,
    state: ThreadBound<ViewState>,
}

struct Group {
    instances: Vec<u32>,
    gm: Gm<InstancedMesh, PhysicalMaterial>,
}

struct ViewState {
    context: Context,
    camera: Camera,
    navigation: Navigation,
    orbit: OrbitControl,
    fly: FlyControl,
    keys: HashSet<String>,
    width: u32,
    height: u32,
    color: Texture2D,
    depth: DepthTexture2D,
    readback: Readback,
    groups: Vec<Group>,
    lights: Lights,
    revision: u64,
    instance_revision: u64,
    selection: HashSet<u32>,
    realtime: RealtimeSpec,
    last_change: Instant,
    last_frame: Instant,
    raytrace: Option<PreviewState>,
    cpu_threads: usize,
    pick: Option<(f32, f32)>,
    picked: Option<Option<Hit>>,
}

#[derive(Default)]
struct Lights {
    ambient: Option<AmbientLight>,
    directional: Vec<DirectionalLight>,
    point: Vec<PointLight>,
    spot: Vec<SpotLight>,
    background: Srgba,
}

#[uniffi::export]
impl Renderer {
    /// A realtime view of `width` x `height` pixels, on the same device as the raytracer.
    pub fn scene_view(
        self: Arc<Self>,
        scene: Arc<Scene>,
        camera: CameraSpec,
        width: u32,
        height: u32,
        realtime: RealtimeSpec,
    ) -> Result<Arc<SceneView>, RtFfiError> {
        let (context, threads) = self.state.with(|s| (s.context.clone(), s.cpu_threads))?;
        let context =
            context.ok_or_else(|| invalid("a realtime view needs a renderer with a context"))?;
        let (width, height) = (width.max(1), height.max(1));
        let cam = view_camera(&camera, width, height);
        let target: Vec3 = camera.target.into();
        let state = ViewState {
            color: rgba_texture(&context, width, height),
            depth: depth_texture(&context, width, height),
            orbit: OrbitControl::new(target, 0.5, 500.0),
            fly: FlyControl::new(0.5),
            navigation: Navigation::Orbit {
                target: camera.target,
                min_distance: 0.5,
                max_distance: 500.0,
            },
            keys: HashSet::new(),
            camera: cam,
            width,
            height,
            readback: Readback::new(),
            groups: Vec::new(),
            lights: Lights::default(),
            revision: u64::MAX,
            instance_revision: u64::MAX,
            selection: HashSet::new(),
            realtime,
            last_change: Instant::now(),
            last_frame: Instant::now(),
            raytrace: None,
            cpu_threads: threads,
            pick: None,
            picked: None,
            context,
        };
        Ok(Arc::new(SceneView {
            scene,
            state: ThreadBound::new(state),
        }))
    }
}

#[uniffi::export]
impl SceneView {
    /// Sets the camera, e.g. from a saved view.
    pub fn set_camera(&self, camera: CameraSpec) -> Result<(), RtFfiError> {
        self.state.with(|s| {
            s.camera = view_camera(&camera, s.width, s.height);
            s.changed();
        })
    }

    /// The camera as mouse and keys moved it.
    pub fn camera(&self) -> Result<CameraSpec, RtFfiError> {
        self.state.with(|s| CameraSpec::from_camera(&s.camera))
    }

    /// How mouse and keys move the camera.
    pub fn set_navigation(&self, navigation: Navigation) -> Result<(), RtFfiError> {
        self.state.with(|s| {
            match navigation {
                Navigation::Orbit {
                    target,
                    min_distance,
                    max_distance,
                } => {
                    s.orbit = OrbitControl::new(target.into(), min_distance, max_distance);
                }
                Navigation::Fly { speed } => s.fly = FlyControl::new(speed),
                Navigation::Walk { eye_height, .. } => {
                    let eye = s.camera.position();
                    let target = s.camera.target();
                    let up = s.camera.up();
                    let lift = vec3(0.0, eye_height - eye.y, 0.0);
                    s.camera.set_view(eye + lift, target + lift, up);
                    s.changed();
                }
            }
            s.navigation = navigation;
        })
    }

    /// Raster settings and the exposure.
    pub fn set_realtime(&self, realtime: RealtimeSpec) -> Result<(), RtFfiError> {
        self.state.with(|s| {
            s.realtime = realtime;
            s.revision = u64::MAX;
            s.raytrace = None;
        })
    }

    /// Hands an event to the camera control; true if it moves the camera.
    pub fn input(&self, event: InputEvent) -> Result<bool, RtFfiError> {
        self.state.with(|s| s.input(event))
    }

    /// A new size in pixels.
    pub fn resize(&self, width: u32, height: u32) -> Result<(), RtFfiError> {
        self.state.with(|s| {
            let (width, height) = (width.max(1), height.max(1));
            if (width, height) != (s.width, s.height) {
                (s.width, s.height) = (width, height);
                s.camera.set_viewport(Viewport::new_at_origo(width, height));
                s.color = rgba_texture(&s.context, width, height);
                s.depth = depth_texture(&s.context, width, height);
                if let Some(r) = &mut s.raytrace {
                    r.resize(width, height);
                }
                s.changed();
            }
        })
    }

    /// Instances drawn as selected (lighter), e.g. after a click.
    pub fn set_selection(&self, instances: Vec<u32>) -> Result<(), RtFfiError> {
        self.state.with(|s| {
            s.selection = instances.into_iter().collect();
            s.revision = u64::MAX;
        })
    }

    /// Asks what is under the pixel; the answer comes with [SceneView::take_pick] after one of the next frames. Works in the browser too.
    pub fn request_pick(&self, x: f32, y: f32) -> Result<(), RtFfiError> {
        self.state.with(|s| {
            s.pick = Some((x, y));
            s.picked = None;
        })
    }

    /// The answer to the last [SceneView::request_pick]: `None` while it is not there yet, `Some(None)` for nothing hit.
    pub fn take_pick(&self) -> Result<Option<Option<Hit>>, RtFfiError> {
        self.state.with(|s| s.picked.take())
    }

    /// Draws a frame: the rasterizer while things move, the raytraced preview after the camera stood still.
    pub fn next_frame(&self) -> Result<Frame, RtFfiError> {
        self.state.with(|s| s.frame(&self.scene))?
    }
}

#[uniffi::export]
impl SceneView {
    /// What is under the pixel (from the top left), right away: on the GPU where the platform can wait for it, in the
    /// browser with a ray through the scene on the CPU. Exported on every target, so that the common API of the bindings
    /// has it.
    pub fn pick(&self, x: f32, y: f32) -> Result<Option<Hit>, RtFfiError> {
        self.state.with(|s| {
            s.sync(&self.scene);
            #[cfg(not(target_arch = "wasm32"))]
            {
                s.pick_now(x, y)
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(self
                    .scene
                    .with_rt(|rt| cpu_pick(rt, &s.camera, x, s.height as f32 - y)))
            }
        })?
    }
}

impl ViewState {
    fn changed(&mut self) {
        self.last_change = Instant::now();
    }

    fn input(&mut self, event: InputEvent) -> bool {
        let point = |x: f32, y: f32| PhysicalPoint {
            x,
            y: self.height as f32 - y,
        };
        let button = |b: u32| match b {
            1 => MouseButton::Right,
            2 => MouseButton::Middle,
            _ => MouseButton::Left,
        };
        let modifiers = Modifiers::default();
        let e = match event {
            InputEvent::Press { x, y, button: b } => Event::MousePress {
                button: button(b),
                position: point(x, y),
                modifiers,
                handled: false,
            },
            InputEvent::Release { x, y, button: b } => Event::MouseRelease {
                button: button(b),
                position: point(x, y),
                modifiers,
                handled: false,
            },
            InputEvent::Move {
                x,
                y,
                dx,
                dy,
                button: b,
            } => Event::MouseMotion {
                button: b.map(button),
                delta: (dx, dy),
                position: point(x, y),
                modifiers,
                handled: false,
            },
            InputEvent::Wheel { x, y, delta } => Event::MouseWheel {
                delta: (0.0, delta),
                position: point(x, y),
                modifiers,
                handled: false,
            },
            InputEvent::Key { key, down } => {
                if down {
                    self.keys.insert(key);
                } else {
                    self.keys.remove(&key);
                }
                return matches!(self.navigation, Navigation::Walk { .. }) && !self.keys.is_empty();
            }
        };
        let mut events = [e];
        let moved = match self.navigation {
            Navigation::Orbit { .. } => self.orbit.handle_events(&mut self.camera, &mut events),
            Navigation::Fly { .. } => self.fly.handle_events(&mut self.camera, &mut events),
            Navigation::Walk { .. } => match &events[0] {
                Event::MouseMotion {
                    delta,
                    button: Some(MouseButton::Left),
                    ..
                } => {
                    // Looking around: turn around the vertical axis and tilt, the eyes stay where they are
                    let (eye, target, up) = (
                        self.camera.position(),
                        self.camera.target(),
                        self.camera.up(),
                    );
                    let dir = (target - eye).normalize();
                    let yaw = -delta.0 * 0.003;
                    let (sin, cos) = yaw.sin_cos();
                    let turned = vec3(dir.x * cos - dir.z * sin, 0.0, dir.x * sin + dir.z * cos);
                    let pitch: f32 = (dir.y - delta.1 * 0.003).clamp(-0.95, 0.95);
                    let flat = turned.normalize() * (1.0 - pitch * pitch).sqrt();
                    self.camera
                        .set_view(eye, eye + vec3(flat.x, pitch, flat.z), up);
                    true
                }
                _ => false,
            },
        };
        if moved {
            self.changed();
        }
        moved
    }

    /// Walking with the keys held, at the time since the last frame.
    fn walk(&mut self, seconds: f32) {
        let Navigation::Walk { eye_height, speed } = self.navigation else {
            return;
        };
        if self.keys.is_empty() {
            return;
        }
        let (eye, target, up) = (
            self.camera.position(),
            self.camera.target(),
            self.camera.up(),
        );
        let forward = vec3(target.x - eye.x, 0.0, target.z - eye.z).normalize();
        let right = forward.cross(vec3(0.0, 1.0, 0.0));
        let held = |k: &[&str]| k.iter().any(|k| self.keys.contains(*k));
        let mut step = vec3(0.0, 0.0, 0.0);
        if held(&["W", "ArrowUp"]) {
            step += forward;
        }
        if held(&["S", "ArrowDown"]) {
            step -= forward;
        }
        if held(&["D", "ArrowRight"]) {
            step += right;
        }
        if held(&["A", "ArrowLeft"]) {
            step -= right;
        }
        if step.magnitude2() == 0.0 {
            return;
        }
        let run = if held(&["Shift"]) { 3.0 } else { 1.0 };
        let mut v = step.normalize() * speed * run * seconds.min(0.1);
        v.y = eye_height - eye.y;
        self.camera.set_view(eye + v, target + v, up);
        self.changed();
    }

    /// Brings the three-d objects up to the scene: everything anew when meshes, materials or lights changed, only
    /// the instance data when instances moved.
    fn sync(&mut self, scene: &Scene) {
        scene.with_rt(|rt| {
            let scale = self.realtime.look.tone_mapping().exposure_scale();
            if rt.revision() != self.revision {
                self.groups = build_groups(&self.context, rt, &self.selection, scale);
                self.lights = build_lights(&self.context, rt, scale);
                self.revision = rt.revision();
                self.instance_revision = rt.instance_revision();
                if self.realtime.sun_shadow_map > 0 {
                    let geometries: Vec<&InstancedMesh> =
                        self.groups.iter().map(|g| &g.gm.geometry).collect();
                    for light in &mut self.lights.directional {
                        let _ = light.generate_shadow_map(
                            self.realtime.sun_shadow_map,
                            geometries.iter().copied(),
                        );
                    }
                }
                self.changed();
            } else if rt.instance_revision() != self.instance_revision {
                for g in &mut self.groups {
                    g.gm.geometry.set_instances(&instances_of(rt, &g.instances));
                }
                self.instance_revision = rt.instance_revision();
                self.changed();
            }
        });
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn pick_now(&mut self, x: f32, y: f32) -> Result<Option<Hit>, RtFfiError> {
        let pixel = PhysicalPoint {
            x,
            y: self.height as f32 - y,
        };
        let geometries: Vec<&InstancedMesh> = self.groups.iter().map(|g| &g.gm.geometry).collect();
        let hit =
            three_d::renderer::pick(&self.context, &self.camera, pixel, geometries, Cull::Back)
                .map_err(|e| RtFfiError::Gpu { why: e.to_string() })?;
        Ok(hit.and_then(|h| self.hit(h.geometry_id, h.instance_id, h.position)))
    }

    fn hit(&self, geometry: u32, instance: u32, position: Vec3) -> Option<Hit> {
        let g = self.groups.get(geometry as usize)?;
        let id = *g.instances.get(instance as usize)?;
        Some(Hit {
            instance: id,
            position: position.into(),
        })
    }

    fn frame(&mut self, scene: &Scene) -> Result<Frame, RtFfiError> {
        let now = Instant::now();
        let seconds = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        self.walk(seconds);
        self.sync(scene);
        if let Some((x, y)) = self.pick.take() {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.picked = Some(self.pick_now(x, y)?);
            }
            #[cfg(target_arch = "wasm32")]
            {
                // The browser cannot wait for the GPU: the ray goes through the scene on the CPU instead
                self.picked =
                    Some(scene.with_rt(|rt| cpu_pick(rt, &self.camera, x, self.height as f32 - y)));
            }
        }

        // Still long enough: the raytraced preview takes over and gets sharper
        let still = now.duration_since(self.last_change).as_millis() as u32;
        if let Some(after) = self.realtime.raytrace_when_still_ms {
            if still >= after {
                let camera = CameraSpec::from_camera(&self.camera);
                let (w, h, look, threads) = (
                    self.width,
                    self.height,
                    self.realtime.look,
                    self.cpu_threads,
                );
                let context = self.context.clone();
                let preview = self
                    .raytrace
                    .get_or_insert_with(|| PreviewState::new(&context, w, h, look, threads));
                preview.update(scene, &camera)?;
                if preview.preview.sample_count() > 0 {
                    return Ok(preview.read());
                }
            } else {
                // Moving again: the preview starts over when the camera stands still next time
                if let Some(p) = &mut self.raytrace {
                    p.preview.restart();
                }
            }
        }

        let target = RenderTarget::new(
            self.color.as_color_target(None),
            self.depth.as_depth_target(),
        );
        let b = self.lights.background;
        target.clear(ClearState::color_and_depth(
            b.r as f32 / 255.0,
            b.g as f32 / 255.0,
            b.b as f32 / 255.0,
            1.0,
            1.0,
        ));
        let mut lights: Vec<&dyn three_d::Light> = Vec::new();
        lights.extend(self.lights.ambient.iter().map(|l| l as &dyn three_d::Light));
        lights.extend(
            self.lights
                .directional
                .iter()
                .map(|l| l as &dyn three_d::Light),
        );
        lights.extend(self.lights.point.iter().map(|l| l as &dyn three_d::Light));
        lights.extend(self.lights.spot.iter().map(|l| l as &dyn three_d::Light));
        self.camera.tone_mapping = match self.realtime.look.tone {
            ToneCurve::Aces => three_d::ToneMapping::Aces,
            ToneCurve::Reinhard => three_d::ToneMapping::Reinhard,
            ToneCurve::Clamp => three_d::ToneMapping::None,
        };
        target.render(&self.camera, self.groups.iter().map(|g| &g.gm), &lights);
        let (w, h) = (self.width, self.height);
        let bytes = self.readback.read(&self.color.as_color_target(None), w, h);
        Ok(Frame {
            rgba: NativeBytes::of(bytes),
            width: w,
            height: h,
            samples: 0,
        })
    }
}

fn view_camera(spec: &CameraSpec, width: u32, height: u32) -> Camera {
    Camera::new_perspective(
        Viewport::new_at_origo(width, height),
        spec.eye.into(),
        spec.target.into(),
        spec.up.into(),
        degrees(spec.fov_degrees),
        spec.near,
        spec.far,
    )
}

fn depth_texture(context: &Context, width: u32, height: u32) -> DepthTexture2D {
    DepthTexture2D::new::<f32>(
        context,
        width,
        height,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    )
}

fn instances_of(rt: &RtScene, ids: &[u32]) -> Instances {
    Instances {
        transformations: ids
            .iter()
            .map(|&i| rt.instances()[i as usize].transform)
            .collect(),
        ..Default::default()
    }
}

/// One instanced mesh per mesh and material: a hundred trees of one kind are one draw call.
fn build_groups(
    context: &Context,
    rt: &RtScene,
    selection: &HashSet<u32>,
    scale: f32,
) -> Vec<Group> {
    let mut by_key: HashMap<(u32, u32, bool), Vec<u32>> = HashMap::new();
    let mut order = Vec::new();
    for (i, inst) in rt.instances().iter().enumerate() {
        let key = (
            inst.mesh.0,
            inst.material.0,
            selection.contains(&(i as u32)),
        );
        by_key
            .entry(key)
            .or_insert_with(|| {
                order.push(key);
                Vec::new()
            })
            .push(i as u32);
    }
    let meshes: Vec<CpuMesh> = rt.meshes().iter().map(cpu_mesh_of).collect();
    order
        .into_iter()
        .map(|key| {
            let ids = by_key.remove(&key).unwrap();
            let mesh = &meshes[key.0 as usize];
            let mut cpu = cpu_material_of(rt, &rt.materials()[key.1 as usize], key.2, scale);
            if mesh.uvs.is_none() {
                // Without texture coordinates the shader has nothing to read textures with: on wgpu the pipeline
                // would fail to build and the whole frame stays empty
                cpu.albedo_texture = None;
                cpu.metallic_roughness_texture = None;
                cpu.normal_texture = None;
                cpu.occlusion_texture = None;
                cpu.emissive_texture = None;
            } else if mesh.tangents.is_none() {
                cpu.normal_texture = None;
            }
            let material = if cpu.albedo.a < 255 {
                PhysicalMaterial::new_transparent(context, &cpu)
            } else {
                PhysicalMaterial::new_opaque(context, &cpu)
            };
            let mesh = InstancedMesh::new(context, &instances_of(rt, &ids), mesh);
            Group {
                instances: ids,
                gm: Gm::new(mesh, material),
            }
        })
        .collect()
}

fn cpu_mesh_of(m: &three_d_raytrace::RtMesh) -> CpuMesh {
    let mut mesh = CpuMesh {
        positions: Positions::F32(m.positions.clone()),
        indices: Indices::U32(m.triangles.iter().flatten().copied().collect()),
        normals: m.normals.clone(),
        uvs: m.uvs.clone(),
        tangents: m.tangents.clone(),
        ..Default::default()
    };
    if mesh.normals.is_none() {
        mesh.compute_normals();
    }
    // Normal textures need tangents; the raytracer follows the texture coordinates of each triangle without them
    if mesh.tangents.is_none() && mesh.uvs.is_some() {
        mesh.compute_tangents();
    }
    mesh
}

fn srgba(linear: Vec3, alpha: f32) -> Srgba {
    let c = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    Srgba::new(
        c(linear.x),
        c(linear.y),
        c(linear.z),
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn cpu_texture(rt: &RtScene, id: Option<three_d_raytrace::TextureId>) -> Option<CpuTexture> {
    let t = &rt.textures()[id?.0 as usize];
    let wrap = |w: three_d_raytrace::RtWrapping| match w {
        three_d_raytrace::RtWrapping::Repeat => Wrapping::Repeat,
        three_d_raytrace::RtWrapping::MirroredRepeat => Wrapping::MirroredRepeat,
        three_d_raytrace::RtWrapping::ClampToEdge => Wrapping::ClampToEdge,
    };
    Some(CpuTexture {
        width: t.width,
        height: t.height,
        data: TextureData::RgbaU8(t.texels.clone()),
        wrap_s: wrap(t.wrap_u),
        wrap_t: wrap(t.wrap_v),
        ..Default::default()
    })
}

/// The raytracer's material for the rasterizer: glass becomes see-through without bending light, media are left out.
fn cpu_material_of(rt: &RtScene, m: &RtMaterial, selected: bool, scale: f32) -> CpuMaterial {
    let alpha = if m.transmission > 0.0 {
        1.0 - 0.8 * m.transmission
    } else {
        1.0
    };
    let base = if selected {
        m.base_color * 0.5 + vec3(0.35, 0.45, 0.6)
    } else {
        m.base_color
    };
    // A boundary of smoke or fog without a surface of its own: almost invisible
    let alpha = if m.medium.is_some() && m.index_of_refraction == 1.0 {
        0.15
    } else {
        alpha
    };
    CpuMaterial {
        albedo: srgba(base, alpha),
        albedo_texture: cpu_texture(rt, m.base_color_texture),
        metallic: m.metallic,
        roughness: m.roughness.max(0.05),
        metallic_roughness_texture: cpu_texture(rt, m.metallic_roughness_texture),
        normal_scale: m.normal_scale,
        normal_texture: cpu_texture(rt, m.normal_texture),
        emissive: srgba(m.emission * scale, 1.0),
        emissive_texture: cpu_texture(rt, m.emission_texture),
        alpha_cutout: m.alpha_cutout,
        ..Default::default()
    }
}

/// The raytracer's lights for the rasterizer, in the same exposure: intensities are photometric values times the
/// exposure scale, so that a surface in the realtime view is about as bright as in the raytraced image.
fn build_lights(context: &Context, rt: &RtScene, scale: f32) -> Lights {
    let mut lights = Lights::default();
    let color = |c: Vec3| -> (f32, Srgba) {
        let m = c.x.max(c.y).max(c.z).max(1e-6);
        (m, srgba(c / m, 1.0))
    };
    let inverse_square = Attenuation {
        constant: 0.0,
        linear: 0.0,
        quadratic: 1.0,
    };
    match rt.environment() {
        RtEnvironment::Sky(sky) => {
            // The sun as a directional light with shadows, the sky as ambient light of its average luminance
            let (m, c) = color(sky.sun_color());
            lights.directional.push(DirectionalLight::new(
                context,
                sky.sun_illuminance() * m * scale,
                c,
                -sky.sun_direction(),
            ));
            let mut sum = vec3(0.0, 0.0, 0.0);
            let n = 64;
            for i in 0..n {
                let phi = i as f32 / n as f32 * std::f32::consts::TAU;
                for elevation in [0.15f32, 0.5, 0.9] {
                    let d = vec3(
                        phi.cos() * (1.0 - elevation * elevation).sqrt(),
                        elevation,
                        phi.sin() * (1.0 - elevation * elevation).sqrt(),
                    );
                    sum += sky.radiance(d);
                }
            }
            let average = sum / (3 * n) as f32;
            let (m, c) = color(average);
            lights.ambient = Some(AmbientLight::new(context, m * scale, c));
            let horizon = sky.radiance(vec3(0.0, 0.2, -1.0).normalize()) * scale;
            lights.background = srgba(aces(horizon), 1.0);
        }
        RtEnvironment::Uniform {
            color: c,
            luminance,
        } => {
            let (m, col) = color(*c * *luminance);
            lights.ambient = Some(AmbientLight::new(context, m * scale, col));
            lights.background = srgba(aces(*c * *luminance * scale), 1.0);
        }
    }
    for light in rt.lights() {
        match light {
            RtLight::Directional {
                direction,
                color: c,
                illuminance,
            } => {
                let (m, c) = color(*c);
                lights.directional.push(DirectionalLight::new(
                    context,
                    illuminance * m * scale,
                    c,
                    *direction,
                ));
            }
            RtLight::Point {
                position,
                color: c,
                intensity,
            } => {
                let (m, c) = color(*c);
                lights.point.push(PointLight::new(
                    context,
                    intensity * m * scale,
                    c,
                    *position,
                    inverse_square,
                ));
            }
            RtLight::Sphere {
                center,
                radius,
                color: c,
                luminance,
            } => {
                // Its flux spread evenly: 4π²r²L lumen, divided by 4π for candela
                let intensity = std::f32::consts::PI * radius * radius * luminance;
                let (m, c) = color(*c);
                lights.point.push(PointLight::new(
                    context,
                    intensity * m * scale,
                    c,
                    *center,
                    inverse_square,
                ));
            }
            RtLight::Rect {
                corner,
                edge_u,
                edge_v,
                color: c,
                luminance,
                ..
            } => {
                // A spot from the middle of the panel with the panel's intensity straight ahead (A·L candela)
                let area = edge_u.cross(*edge_v).magnitude();
                let normal = edge_u.cross(*edge_v).normalize();
                let (m, c) = color(*c);
                lights.spot.push(SpotLight::new(
                    context,
                    area * luminance * m * scale,
                    c,
                    *corner + (*edge_u + *edge_v) * 0.5,
                    normal,
                    degrees(80.0),
                    inverse_square,
                ));
            }
            RtLight::Luminaire {
                position,
                down,
                color: c,
                profile,
                scale: s,
                ..
            } => {
                // A spot with the profile's intensity straight down and the cone where it falls to a tenth
                let peak = profile.intensity(0.0, 0.0) * s;
                let cutoff = (1..90)
                    .map(|deg| deg as f32)
                    .find(|&deg| profile.intensity(deg.to_radians(), 0.0) * s < peak * 0.1)
                    .unwrap_or(90.0);
                let (m, c) = color(*c);
                lights.spot.push(SpotLight::new(
                    context,
                    peak * m * scale,
                    c,
                    *position,
                    *down,
                    degrees(cutoff),
                    inverse_square,
                ));
            }
        }
    }
    lights
}

fn aces(x: Vec3) -> Vec3 {
    let f = |x: f32| ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0);
    vec3(f(x.x), f(x.y), f(x.z))
}

/// A pick on the CPU through the bounding boxes and triangles of the scene, for the browser, where the GPU cannot be waited for.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn cpu_pick(rt: &RtScene, camera: &Camera, x: f32, y: f32) -> Option<Hit> {
    let origin = camera.position_at_pixel(PhysicalPoint { x, y });
    let dir = camera.view_direction_at_pixel(PhysicalPoint { x, y });
    let mut best: Option<(f32, u32)> = None;
    for (i, inst) in rt.instances().iter().enumerate() {
        let mesh = &rt.meshes()[inst.mesh.0 as usize];
        let inv = inst.transform.invert()?;
        let o = (inv * origin.extend(1.0)).truncate();
        let d = (inv * dir.extend(0.0)).truncate();
        for t in &mesh.triangles {
            let [a, b, c] = t.map(|k| mesh.positions[k as usize]);
            if let Some(s) = ray_triangle(o, d, a, b, c) {
                if best.is_none_or(|(bs, _)| s < bs) {
                    best = Some((s, i as u32));
                }
            }
        }
    }
    best.map(|(s, i)| Hit {
        instance: i,
        position: (origin + dir * s).into(),
    })
}

fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let t = o - a;
    let u = t.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let s = e2.dot(q) * inv;
    (s > 0.0).then_some(s)
}
