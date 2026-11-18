//!
//! Tracers add samples of the scene to a render image. Every tracer uses the same scene, render image and material model.
//!

use crate::sampling::*;
use crate::{
    AccelBuilder, Camera, CpuAccel, CpuBvhBuilder, Hit, RenderImage, RtEnvironment, RtError,
    RtLight, RtMaterial, RtScene,
};
use std::f32::consts::PI;
use three_d_asset::prelude::*;
use three_d_asset::ProjectionType;

///
/// Which kind of hardware a tracer runs on.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TracerKind {
    /// On the CPU, the reference for all others.
    CpuReference,
    /// In compute shaders of the wgpu backend, with the hierarchies in storage buffers.
    Compute,
    /// With the ray queries of the wgpu backend on hardware that traces rays.
    RayQuery,
}

///
/// What the next call of [Tracer::trace] does.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceSettings {
    /// Samples added to every pixel per call.
    pub samples_per_call: u32,
    /// Most surfaces a path bounces off before it ends.
    pub max_bounces: u32,
    /// Changes the random numbers, the same seed gives the same image.
    pub seed: u32,
    /// Brightest value a single sample may add after the first bounce, in cd/m². Removes the rare very bright
    /// pixels (fireflies) at the cost of a little energy. `None` keeps every sample as it is.
    pub indirect_clamp: Option<f32>,
}

impl Default for TraceSettings {
    fn default() -> Self {
        Self {
            samples_per_call: 1,
            max_bounces: 8,
            seed: 0,
            indirect_clamp: None,
        }
    }
}

///
/// Adds samples of a scene, seen by a camera, to a render image.
///
/// Implementations: [CpuReferenceTracer] on any platform, `ComputeTracer` in compute shaders of the wgpu backend
/// (feature `wgpu`), a ray query tracer later. The shading is the same in all of them, they differ in how rays find the
/// triangles they hit.
///
pub trait Tracer {
    /// What this tracer runs on.
    fn kind(&self) -> TracerKind;

    /// Builds what the scene needs ahead of time. [Tracer::trace] does it too when needed.
    fn prepare(&mut self, scene: &RtScene) -> Result<(), RtError>;

    ///
    /// Adds [TraceSettings::samples_per_call] samples to every pixel of the render image. The render image's samples have to be from
    /// the same scene and camera, reset it when they change.
    ///
    fn trace(
        &mut self,
        scene: &RtScene,
        camera: &Camera,
        render_image: &mut RenderImage,
        settings: &TraceSettings,
    ) -> Result<(), RtError>;
}

///
/// A path tracer on the CPU, on all threads where there are threads.
///
/// It is the reference for the GPU tracers: unbiased apart from what is listed here, deterministic for a seed, and
/// written for clarity over speed. Paths sample the sun, directional and point lights at every diffuse or glossy
/// bounce and the sky by the material. Not included: caustics (light focused by glass or mirrors onto a surface) and
/// light through solid glass, which is approximated by letting shadow rays through transmissive surfaces.
///
#[derive(Debug, Default)]
pub struct CpuReferenceTracer {
    builder: CpuBvhBuilder,
    accel: Option<CpuAccel>,
    threads: Option<usize>,
}

impl CpuReferenceTracer {
    /// A tracer that uses all threads.
    pub fn new() -> Self {
        Self::default()
    }

    /// A tracer with a fixed number of threads, the image does not depend on it.
    pub fn with_threads(threads: usize) -> Self {
        Self {
            threads: Some(threads.max(1)),
            ..Self::default()
        }
    }

    /// The acceleration structure, after [Tracer::prepare].
    pub fn accel(&self) -> Option<&CpuAccel> {
        self.accel.as_ref()
    }

    fn thread_count(&self) -> usize {
        if cfg!(target_arch = "wasm32") {
            return 1;
        }
        self.threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        })
    }
}

impl Tracer for CpuReferenceTracer {
    fn kind(&self) -> TracerKind {
        TracerKind::CpuReference
    }

    fn prepare(&mut self, scene: &RtScene) -> Result<(), RtError> {
        match &mut self.accel {
            Some(accel) if accel.is_up_to_date(scene) => {}
            Some(accel) => self.builder.refit_instances(accel, scene)?,
            None => self.accel = Some(self.builder.build(scene)?),
        }
        Ok(())
    }

    fn trace(
        &mut self,
        scene: &RtScene,
        camera: &Camera,
        render_image: &mut RenderImage,
        settings: &TraceSettings,
    ) -> Result<(), RtError> {
        render_image.check_not_empty()?;
        self.prepare(scene)?;
        let accel = self.accel.as_ref().expect("prepared");
        let context = PathContext {
            scene,
            accel,
            rays: CameraRays::new(camera),
            settings: *settings,
            width: render_image.width(),
            height: render_image.height(),
            first_sample: render_image.sample_count(),
        };
        let width = render_image.width() as usize;
        let threads = self
            .thread_count()
            .min(render_image.height() as usize)
            .max(1);
        let rows = render_image.rows_mut();
        if threads == 1 {
            for (y, row) in rows.chunks_mut(width).enumerate() {
                context.render_row(y as u32, row);
            }
        } else {
            // Rows are handed out one at a time, so threads stay busy where the scene is uneven
            let next = std::sync::atomic::AtomicUsize::new(0);
            let row_slices: Vec<std::sync::Mutex<&mut [[f32; 3]]>> =
                rows.chunks_mut(width).map(std::sync::Mutex::new).collect();
            std::thread::scope(|s| {
                for _ in 0..threads {
                    s.spawn(|| loop {
                        let y = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if y >= row_slices.len() {
                            break;
                        }
                        let mut row = row_slices[y].lock().unwrap();
                        context.render_row(y as u32, &mut row);
                    });
                }
            });
        }
        render_image.add_samples(settings.samples_per_call);
        Ok(())
    }
}

/// Primary rays of a camera.
struct CameraRays {
    camera: Camera,
    /// For perspective cameras: from normalized device coordinates to a direction in world space.
    screen_to_direction: Option<Mat4>,
}

impl CameraRays {
    fn new(camera: &Camera) -> Self {
        let screen_to_direction = match camera.projection_type() {
            ProjectionType::Perspective { .. } => {
                let mut view = camera.view();
                view[3] = vec4(0.0, 0.0, 0.0, 1.0);
                (camera.projection() * view).invert()
            }
            _ => None,
        };
        Self {
            camera: camera.clone(),
            screen_to_direction,
        }
    }

    /// Origin and unit direction for uv coordinates with v up.
    fn ray(&self, u: f32, v: f32) -> (Vec3, Vec3) {
        match self.screen_to_direction {
            Some(m) => (
                self.camera.position(),
                (m * vec4(2.0 * u - 1.0, 2.0 * v - 1.0, 0.0, 1.0))
                    .truncate()
                    .normalize(),
            ),
            None => (
                self.camera.position_at_uv_coordinates((u, v)),
                self.camera.view_direction_at_uv_coordinates((u, v)),
            ),
        }
    }
}

/// Everything one call of trace shares between threads.
struct PathContext<'a> {
    scene: &'a RtScene,
    accel: &'a CpuAccel,
    rays: CameraRays,
    settings: TraceSettings,
    width: u32,
    height: u32,
    first_sample: u32,
}

/// The surface at a hit, in world space.
struct Surface {
    position: Vec3,
    /// Face normal on the side the ray came from.
    geometric_normal: Vec3,
    /// Interpolated normal on the same side.
    shading_normal: Vec3,
    /// Whether the ray hit the front of the triangle (counter clockwise seen from the ray).
    front_face: bool,
    /// The material with its textures applied at the hit.
    material: RtMaterial,
}

impl PathContext<'_> {
    fn render_row(&self, y: u32, row: &mut [[f32; 3]]) {
        for (x, sum) in row.iter_mut().enumerate() {
            for s in 0..self.settings.samples_per_call {
                let sample = self.first_sample + s;
                let mut rng = Rng::for_pixel(self.settings.seed, x as u32, y, sample);
                let u = (x as f32 + rng.next_f32()) / self.width as f32;
                let v = 1.0 - (y as f32 + rng.next_f32()) / self.height as f32;
                let (origin, direction) = self.rays.ray(u, v);
                let l = self.radiance(origin, direction, &mut rng);
                // A broken sample must not spoil the pixel for good
                if l.x.is_finite() && l.y.is_finite() && l.z.is_finite() {
                    sum[0] += l.x;
                    sum[1] += l.y;
                    sum[2] += l.z;
                }
            }
        }
    }

    /// Luminance arriving along the ray, by following one path.
    fn radiance(&self, mut origin: Vec3, mut direction: Vec3, rng: &mut Rng) -> Vec3 {
        let mut result = vec3(0.0, 0.0, 0.0);
        let mut throughput = vec3(1.0, 1.0, 1.0);
        // Lights that can not be hit by chance (the sun disc) are seen directly only after camera rays and mirrors
        let mut last_was_delta = true;
        let mut bounce = 0;
        loop {
            let Some(hit) = self
                .accel
                .intersect(self.scene, origin, direction, f32::INFINITY)
            else {
                result += self.add(
                    bounce,
                    mul(throughput, self.environment(direction, last_was_delta)),
                );
                break;
            };
            let surface = self.surface(&hit, direction);
            let m = &surface.material;
            result += self.add(bounce, mul(throughput, m.emission));
            if bounce >= self.settings.max_bounces {
                break;
            }
            let v = -direction;
            let ng = surface.geometric_normal;
            let mut n = surface.shading_normal;
            if n.dot(v) <= 0.0 {
                n = ng;
            }

            if m.transmission > 0.0 && rng.next_f32() < m.transmission {
                // Smooth glass: reflect or pass, chosen by the Fresnel term
                let eta = if m.thin_walled || surface.front_face {
                    m.index_of_refraction
                } else {
                    1.0 / m.index_of_refraction
                };
                let cos_i = n.dot(v).clamp(0.0, 1.0);
                if rng.next_f32() < fresnel_dielectric(cos_i, eta) {
                    direction = reflect(v, n);
                    origin = offset(surface.position, ng);
                } else {
                    throughput = mul(throughput, m.base_color);
                    if !m.thin_walled {
                        direction = refract(v, n, 1.0 / eta).unwrap_or_else(|| reflect(v, n));
                    }
                    origin = offset(surface.position, -ng);
                }
                last_was_delta = true;
            } else {
                // Diffuse and glossy reflection: light sources directly, then a new direction from the material
                let p = offset(surface.position, ng);
                result += self.add(
                    bounce,
                    mul(throughput, self.direct_light(p, v, n, ng, m, rng)),
                );

                let Some((l, weight)) = sample_material(m, v, n, rng) else {
                    break;
                };
                if l.dot(ng) <= 0.0 {
                    break;
                }
                throughput = mul(throughput, weight);
                direction = l;
                origin = p;
                last_was_delta = false;
            }

            bounce += 1;
            if bounce >= 3 {
                // Russian roulette ends dark paths early without changing the expected value
                let q = throughput
                    .x
                    .max(throughput.y)
                    .max(throughput.z)
                    .clamp(0.05, 0.95);
                if rng.next_f32() > q {
                    break;
                }
                throughput /= q;
            }
        }
        result
    }

    /// Contribution clamped after the first bounce if asked for.
    fn add(&self, bounce: u32, contribution: Vec3) -> Vec3 {
        match self.settings.indirect_clamp {
            Some(max) if bounce > 0 => {
                let m = contribution.x.max(contribution.y).max(contribution.z);
                if m > max {
                    contribution * (max / m)
                } else {
                    contribution
                }
            }
            _ => contribution,
        }
    }

    fn environment(&self, direction: Vec3, include_sun: bool) -> Vec3 {
        match self.scene.environment() {
            RtEnvironment::Uniform { color, luminance } => *color * *luminance,
            RtEnvironment::Sky(sky) => {
                let mut l = sky.radiance(direction);
                if include_sun && direction.dot(sky.sun_direction()) >= sky.sun_cos_radius() {
                    l += sky.sun_radiance();
                }
                l
            }
        }
    }

    fn surface(&self, hit: &Hit, direction: Vec3) -> Surface {
        let instance = &self.scene.instances()[hit.instance as usize];
        let mesh = &self.scene.meshes()[instance.mesh.0 as usize];
        let material = &self.scene.materials()[instance.material.0 as usize];
        let [a, b, c] = mesh.triangles[hit.triangle as usize];
        let (p0, p1, p2) = (
            mesh.positions[a as usize],
            mesh.positions[b as usize],
            mesh.positions[c as usize],
        );
        let w = 1.0 - hit.u - hit.v;
        let local = p0 * w + p1 * hit.u + p2 * hit.v;
        let position = (instance.transform * local.extend(1.0)).truncate();
        // Normals transform with the inverse transpose
        let normal_matrix = instance.inverse.transpose();
        let to_world = |n: Vec3| (normal_matrix * n.extend(0.0)).truncate().normalize();
        let face = to_world((p1 - p0).cross(p2 - p0));
        let mut smooth = match &mesh.normals {
            Some(normals) => {
                let n = normals[a as usize] * w
                    + normals[b as usize] * hit.u
                    + normals[c as usize] * hit.v;
                if n.magnitude2() > 0.0 {
                    let n = to_world(n);
                    // Vertex normals of the other side keep the face orientation
                    if n.dot(face) < 0.0 {
                        -n
                    } else {
                        n
                    }
                } else {
                    face
                }
            }
            None => face,
        };
        let (material, mapped_normal) = if material.has_textures() {
            material.at(
                self.scene.textures(),
                mesh.uv_at(hit.triangle, hit.u, hit.v),
            )
        } else {
            (material.clone(), None)
        };
        if let Some(normal) = mapped_normal {
            if let Some((t, b)) =
                tangent_frame(mesh, [a, b, c], (hit.u, hit.v), smooth, &instance.transform)
            {
                let n = t * normal.x + b * normal.y + smooth * normal.z;
                if n.magnitude2() > 0.0 {
                    smooth = n.normalize();
                }
            }
        }
        let front_face = face.dot(direction) < 0.0;
        let side = if front_face { 1.0 } else { -1.0 };
        Surface {
            position,
            geometric_normal: face * side,
            shading_normal: smooth * side,
            front_face,
            material,
        }
    }

    /// Light from the sun and the light sources reaching the point and reflected towards `v`.
    fn direct_light(
        &self,
        p: Vec3,
        v: Vec3,
        n: Vec3,
        ng: Vec3,
        m: &RtMaterial,
        rng: &mut Rng,
    ) -> Vec3 {
        let mut result = vec3(0.0, 0.0, 0.0);
        if let RtEnvironment::Sky(sky) = self.scene.environment() {
            if sky.sun_illuminance() > 0.0 {
                let (t, b) = orthonormal_basis(sky.sun_direction());
                let c = uniform_cone(rng.next_f32(), rng.next_f32(), sky.sun_cos_radius());
                let l = (t * c.x + b * c.y + sky.sun_direction() * c.z).normalize();
                result += self.light_from(
                    p,
                    v,
                    n,
                    ng,
                    m,
                    l,
                    f32::INFINITY,
                    sky.sun_color() * sky.sun_illuminance(),
                );
            }
        }
        for light in self.scene.lights() {
            match light {
                RtLight::Directional {
                    direction,
                    color,
                    illuminance,
                } => {
                    let l = -direction.normalize();
                    result +=
                        self.light_from(p, v, n, ng, m, l, f32::INFINITY, *color * *illuminance);
                }
                RtLight::Point {
                    position,
                    color,
                    intensity,
                } => {
                    let to_light = *position - p;
                    let d2 = to_light.magnitude2();
                    if d2 > 0.0 {
                        let d = d2.sqrt();
                        let l = to_light / d;
                        result += self.light_from(
                            p,
                            v,
                            n,
                            ng,
                            m,
                            l,
                            d * (1.0 - 1e-4),
                            *color * (*intensity / d2),
                        );
                    }
                }
            }
        }
        result
    }

    /// Reflected luminance for illuminance `e` (lux, on a surface facing the light) from direction `l`.
    #[allow(clippy::too_many_arguments)]
    fn light_from(
        &self,
        p: Vec3,
        v: Vec3,
        n: Vec3,
        ng: Vec3,
        m: &RtMaterial,
        l: Vec3,
        distance: f32,
        e: Vec3,
    ) -> Vec3 {
        let cos = n.dot(l);
        if cos <= 0.0 || ng.dot(l) <= 0.0 {
            return vec3(0.0, 0.0, 0.0);
        }
        let f = evaluate_material(m, v, l, n);
        if f == vec3(0.0, 0.0, 0.0) {
            return f;
        }
        let transmittance = self.transmittance(p, l, distance);
        mul(mul(f, e), transmittance) * cos
    }

    /// How much light passes along a shadow ray: nothing through opaque surfaces, some through glass.
    fn transmittance(&self, origin: Vec3, direction: Vec3, distance: f32) -> Vec3 {
        let mut t = vec3(1.0, 1.0, 1.0);
        self.accel
            .for_each_hit(self.scene, origin, direction, distance, |hit| {
                let instance = &self.scene.instances()[hit.instance as usize];
                let m = &self.scene.materials()[instance.material.0 as usize];
                if m.transmission <= 0.0 {
                    t = vec3(0.0, 0.0, 0.0);
                    return true;
                }
                let s = self.surface(hit, direction);
                let m = &s.material;
                let cos_i = s.geometric_normal.dot(-direction).clamp(0.0, 1.0);
                let f = fresnel_dielectric(cos_i, m.index_of_refraction);
                t = mul(t, m.base_color * (m.transmission * (1.0 - f)));
                t.x.max(t.y).max(t.z) <= 0.0
            });
        t
    }
}

///
/// Tangent and bitangent in world space around the normal `n`, as three-d uses them for normal textures: from the
/// tangents of the mesh, or else with the tangent along u and the bitangent towards decreasing v, as three-d computes
/// missing tangents.
///
fn tangent_frame(
    mesh: &crate::RtMesh,
    [a, b, c]: [u32; 3],
    (u, v): (f32, f32),
    n: Vec3,
    transform: &Mat4,
) -> Option<(Vec3, Vec3)> {
    let to_world = |d: Vec3| (transform * d.extend(0.0)).truncate();
    let (tangent, w) = match (&mesh.tangents, &mesh.uvs) {
        (Some(tangents), _) => {
            let t = tangents[a as usize] * (1.0 - u - v)
                + tangents[b as usize] * u
                + tangents[c as usize] * v;
            let w = if tangents[a as usize].w < 0.0 {
                -1.0
            } else {
                1.0
            };
            (to_world(t.truncate()), w)
        }
        (None, Some(uvs)) => {
            let (p0, p1, p2) = (
                mesh.positions[a as usize],
                mesh.positions[b as usize],
                mesh.positions[c as usize],
            );
            let (t0, t1, t2) = (uvs[a as usize], uvs[b as usize], uvs[c as usize]);
            let (dp1, dp2) = (p1 - p0, p2 - p0);
            let (du1, du2) = (t1 - t0, t2 - t0);
            let det = du1.x * du2.y - du2.x * du1.y;
            if det.abs() < 1e-20 {
                return None;
            }
            let along_u = to_world((dp1 * du2.y - dp2 * du1.y) / det);
            let along_v = to_world((dp2 * du1.x - dp1 * du2.x) / det);
            let w = if n.cross(along_u).dot(along_v) < 0.0 {
                1.0
            } else {
                -1.0
            };
            (along_u, w)
        }
        (None, None) => return None,
    };
    let t = tangent - n * n.dot(tangent);
    if t.magnitude2() <= 0.0 {
        return None;
    }
    let t = t.normalize();
    Some((t, n.cross(t) * w))
}

/// Moves a point off a surface along its normal, relative to the size of its coordinates.
fn offset(p: Vec3, n: Vec3) -> Vec3 {
    let scale = 1.0f32.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
    p + n * (1e-4 * scale)
}

fn reflect(v: Vec3, n: Vec3) -> Vec3 {
    2.0 * v.dot(n) * n - v
}

/// Direction after refraction for `eta` = n_from / n_to, none on total internal reflection.
fn refract(v: Vec3, n: Vec3, eta: f32) -> Option<Vec3> {
    let cos_i = v.dot(n);
    let sin_t2 = eta * eta * (1.0 - cos_i * cos_i);
    if sin_t2 >= 1.0 {
        return None;
    }
    Some((-v * eta + n * (eta * cos_i - (1.0 - sin_t2).sqrt())).normalize())
}

fn mul(a: Vec3, b: Vec3) -> Vec3 {
    vec3(a.x * b.x, a.y * b.y, a.z * b.z)
}

/// The material's roughness as GGX alpha, kept away from zero.
fn alpha(m: &RtMaterial) -> f32 {
    let r = m.roughness.max(0.03);
    r * r
}

fn specular_f0(m: &RtMaterial) -> Vec3 {
    let dielectric = 0.04 * m.specular;
    vec3(dielectric, dielectric, dielectric) * (1.0 - m.metallic) + m.base_color * m.metallic
}

/// Reflectance at grazing angles: one, unless the specular strength of a non-metal is turned down.
fn specular_f90(m: &RtMaterial) -> f32 {
    m.specular.clamp(0.0, 1.0) * (1.0 - m.metallic) + m.metallic
}

/// How likely the glossy part is sampled, from how much it reflects.
fn specular_probability(m: &RtMaterial, n_dot_v: f32) -> f32 {
    let f = luminance(fresnel_schlick(specular_f0(m), specular_f90(m), n_dot_v));
    let d = luminance(m.base_color) * (1.0 - m.metallic);
    if f <= 0.0 {
        0.0
    } else if d <= 0.0 {
        1.0
    } else {
        (f / (f + d)).clamp(0.1, 0.9)
    }
}

/// The BRDF of the opaque part: Lambert plus GGX with height correlated Smith masking and Schlick's Fresnel term.
pub(crate) fn evaluate_material(m: &RtMaterial, v: Vec3, l: Vec3, n: Vec3) -> Vec3 {
    let n_dot_l = n.dot(l);
    let n_dot_v = n.dot(v);
    if n_dot_l <= 0.0 || n_dot_v <= 0.0 {
        return vec3(0.0, 0.0, 0.0);
    }
    let h = (v + l).normalize();
    let n_dot_h = n.dot(h).max(0.0);
    let v_dot_h = v.dot(h).max(0.0);
    let a = alpha(m);
    let f = fresnel_schlick(specular_f0(m), specular_f90(m), v_dot_h);
    let specular = f * (ggx_d(n_dot_h, a) * ggx_visibility(n_dot_l, n_dot_v, a));
    // The diffuse part gets what the glossy part leaves at this viewing angle, which keeps the sum below one
    let kd = (vec3(1.0, 1.0, 1.0) - fresnel_schlick(specular_f0(m), specular_f90(m), n_dot_v))
        * (1.0 - m.metallic);
    mul(kd, m.base_color) / PI + specular
}

/// Probability density of sampling `l` with [sample_material].
fn material_pdf(m: &RtMaterial, v: Vec3, l: Vec3, n: Vec3) -> f32 {
    let n_dot_l = n.dot(l);
    let n_dot_v = n.dot(v);
    if n_dot_l <= 0.0 || n_dot_v <= 0.0 {
        return 0.0;
    }
    let p_spec = specular_probability(m, n_dot_v);
    let a = alpha(m);
    let h = (v + l).normalize();
    let spec_pdf = ggx_g1(n_dot_v, a) * ggx_d(n.dot(h).max(0.0), a) / (4.0 * n_dot_v);
    p_spec * spec_pdf + (1.0 - p_spec) * n_dot_l / PI
}

///
/// A new direction from the material and the weight f·cos/pdf, sampling the glossy part by its visible normals and the
/// diffuse part by the cosine, combined by their probabilities.
///
pub(crate) fn sample_material(
    m: &RtMaterial,
    v: Vec3,
    n: Vec3,
    rng: &mut Rng,
) -> Option<(Vec3, Vec3)> {
    let n_dot_v = n.dot(v);
    if n_dot_v <= 0.0 {
        return None;
    }
    let (t, b) = orthonormal_basis(n);
    let p_spec = specular_probability(m, n_dot_v);
    let (u0, u1, u2) = (rng.next_f32(), rng.next_f32(), rng.next_f32());
    let l = if u0 < p_spec {
        let v_local = vec3(v.dot(t), v.dot(b), n_dot_v);
        let h = ggx_sample_visible_normal(v_local, alpha(m), u1, u2);
        let h = t * h.x + b * h.y + n * h.z;
        reflect(v, h)
    } else {
        let c = cosine_hemisphere(u1, u2);
        t * c.x + b * c.y + n * c.z
    };
    let n_dot_l = n.dot(l);
    if n_dot_l <= 0.0 {
        return None;
    }
    let pdf = material_pdf(m, v, l, n);
    if pdf <= 0.0 {
        return None;
    }
    Some((l, evaluate_material(m, v, l, n) * (n_dot_l / pdf)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reflected energy for light from `v`, estimated by sampling the material.
    fn albedo(m: &RtMaterial, cos_v: f32) -> f32 {
        let n = vec3(0.0, 0.0, 1.0);
        let v = vec3((1.0 - cos_v * cos_v).sqrt(), 0.0, cos_v);
        let mut rng = Rng::new(3, 9);
        let count = 200_000;
        let mut sum = 0.0;
        for _ in 0..count {
            if let Some((_, w)) = sample_material(m, v, n, &mut rng) {
                sum += luminance(w);
            }
        }
        sum / count as f32
    }

    #[test]
    fn white_lambert_reflects_everything() {
        let m = RtMaterial::diffuse(vec3(1.0, 1.0, 1.0));
        for cos_v in [1.0, 0.5, 0.1] {
            let a = albedo(&m, cos_v);
            assert!((a - 1.0).abs() < 0.01, "{cos_v}: {a}");
        }
    }

    #[test]
    fn no_material_creates_energy() {
        for (metallic, roughness) in [(0.0, 0.5), (1.0, 0.1), (1.0, 0.9), (0.0, 0.05), (0.5, 0.3)] {
            let m = RtMaterial {
                base_color: vec3(1.0, 1.0, 1.0),
                metallic,
                roughness,
                ..Default::default()
            };
            for cos_v in [1.0, 0.5, 0.2] {
                let a = albedo(&m, cos_v);
                assert!(
                    a < 1.02,
                    "metallic {metallic} roughness {roughness} at {cos_v}: {a}"
                );
            }
        }
    }

    #[test]
    fn sampling_matches_evaluation() {
        // The estimate of the reflected light from a uniform sky must not depend on how the directions are chosen:
        // compare sampling the material with uniform hemisphere sampling.
        let m = RtMaterial {
            base_color: vec3(0.7, 0.5, 0.3),
            metallic: 0.3,
            roughness: 0.4,
            ..Default::default()
        };
        let n = vec3(0.0, 0.0, 1.0);
        let v = vec3(0.6, 0.0, 0.8);
        let by_material = albedo(&m, 0.8);
        let mut rng = Rng::new(5, 5);
        let count = 400_000;
        let mut sum = 0.0;
        for _ in 0..count {
            let u1 = rng.next_f32();
            let u2 = rng.next_f32();
            let z = u1;
            let r = (1.0 - z * z).sqrt();
            let phi = 2.0 * PI * u2;
            let l = vec3(r * phi.cos(), r * phi.sin(), z);
            sum += luminance(evaluate_material(&m, v, l, n)) * z * 2.0 * PI;
        }
        let uniform = sum / count as f32;
        assert!(
            (by_material - uniform).abs() < 0.01,
            "{by_material} vs {uniform}"
        );
    }

    #[test]
    fn refraction_bends_towards_the_normal() {
        let n = vec3(0.0, 1.0, 0.0);
        let v = vec3(1.0, 1.0, 0.0).normalize();
        let t = refract(v, n, 1.0 / 1.5).unwrap();
        assert!(t.y < 0.0);
        // Snell: sin(out) = sin(in) / 1.5
        assert!((t.x.abs() - (0.5f32.sqrt() / 1.5)).abs() < 1e-5);
    }
}
