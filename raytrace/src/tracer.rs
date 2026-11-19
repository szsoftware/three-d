//!
//! Tracers add samples of the scene to a render image. Every tracer uses the same scene, render image and material model.
//!

use crate::ies::luminaire_angles;
use crate::lens::Lens;
use crate::light::*;
use crate::medium::*;
use crate::nesting::{Inside, Solid};
use crate::photons::*;
use crate::render_image::FEATURE_WORDS;
use crate::sampling::*;
use crate::{
    AccelBuilder, Camera, CpuAccel, CpuBvhBuilder, DepthOfField, Hit, RenderImage, RtEnvironment,
    RtError, RtLight, RtMaterial, RtScene,
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
    ///
    /// Photons shot per call from the sun, directional and point lights at solid glass (transmissive and not thin
    /// walled), for caustics: the light such glass bends onto surfaces. 0 leaves caustics out and lets shadow rays
    /// pass solid glass straight, which is faster but shows neither the focused light nor the true shadow.
    /// The `ComputeTracer` keeps at most as many as fit into the largest buffer the device binds.
    ///
    pub caustic_photons: u32,
    ///
    /// Radius in scene units within which photons light a point, for the first call. It shrinks with every call
    /// (progressive photon mapping), so caustics get sharp and the image converges to the right result.
    ///
    pub caustic_radius: f32,
    /// Blur in front of and behind the focus distance of a perspective camera, `None` for a pinhole camera that
    /// shows everything sharp.
    pub depth_of_field: Option<DepthOfField>,
}

impl Default for TraceSettings {
    fn default() -> Self {
        Self {
            samples_per_call: 1,
            max_bounces: 8,
            seed: 0,
            indirect_clamp: None,
            caustic_photons: 0,
            caustic_radius: 0.02,
            depth_of_field: None,
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
/// bounce and the sky by the material. Caustics through solid glass come from photons when
/// [TraceSettings::caustic_photons] is set, which makes the result consistent rather than unbiased; without them,
/// light through solid glass is approximated by letting shadow rays through. Not included: caustics of mirrors.
///
#[derive(Debug, Default)]
pub struct CpuReferenceTracer {
    builder: CpuBvhBuilder,
    accel: Option<CpuAccel>,
    threads: Option<usize>,
    /// The threads trace at a low priority, see [CpuReferenceTracer::with_low_priority]
    low_priority: bool,
    /// The photons of the sample traced in bands, see [CpuReferenceTracer::trace_rows]
    band_photons: Option<std::sync::Arc<PhotonMap>>,
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

    ///
    /// The tracer's threads run at a low priority, so that an application on the same cores goes first: nice 10 on
    /// Linux and Android, unchanged elsewhere. The thread that calls [Tracer::trace] keeps its priority, it only waits.
    ///
    pub fn with_low_priority(mut self) -> Self {
        self.low_priority = true;
        self
    }

    /// Changes the number of threads, `None` for all.
    pub fn set_threads(&mut self, threads: Option<usize>) {
        self.threads = threads.map(|t| t.max(1));
    }

    /// The number of threads it traces with, 1 in browsers.
    pub fn threads(&self) -> usize {
        self.thread_count()
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
        let first_sample = render_image.sample_count();
        let rows = 0..render_image.height();
        self.render(
            scene,
            camera,
            render_image,
            settings,
            first_sample,
            rows,
            true,
        )?;
        render_image.add_samples(settings.samples_per_call);
        Ok(())
    }
}

impl CpuReferenceTracer {
    ///
    /// Traces the rows of one sample, the `sample`th, into the render image without counting the sample, so a sample
    /// can be traced in parts over several calls ([TraceSettings::samples_per_call] is ignored). The image is complete
    /// once all rows are traced, then [RenderImage::add_samples] counts it. Caustic photons are shot when the first
    /// row is traced and kept for the other rows.
    ///
    #[cfg_attr(not(feature = "three-d"), allow(dead_code))]
    pub(crate) fn trace_rows(
        &mut self,
        scene: &RtScene,
        camera: &Camera,
        render_image: &mut RenderImage,
        settings: &TraceSettings,
        sample: u32,
        rows: std::ops::Range<u32>,
    ) -> Result<(), RtError> {
        render_image.check_not_empty()?;
        let settings = TraceSettings {
            samples_per_call: 1,
            ..*settings
        };
        let shoot = rows.start == 0;
        self.render(scene, camera, render_image, &settings, sample, rows, shoot)
    }

    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        scene: &RtScene,
        camera: &Camera,
        render_image: &mut RenderImage,
        settings: &TraceSettings,
        first_sample: u32,
        rows: std::ops::Range<u32>,
        shoot: bool,
    ) -> Result<(), RtError> {
        self.prepare(scene)?;
        let accel = self.accel.as_ref().expect("prepared");
        let mut context = PathContext {
            scene,
            accel,
            rays: CameraRays::new(camera, settings.depth_of_field),
            settings: *settings,
            width: render_image.width(),
            height: render_image.height(),
            first_sample,
            caustics: None,
            camera_inside: Inside::AIR,
        };
        context.camera_inside = context.inside_at(camera.position());
        if settings.caustic_photons > 0 {
            if shoot {
                let map = context.shoot_photons(first_sample);
                self.band_photons = (!map.is_empty()).then(|| std::sync::Arc::new(map));
            }
            context.caustics = self.band_photons.clone();
        }
        let context = context;
        let width = render_image.width() as usize;
        let rows = rows.start.min(render_image.height())..rows.end.min(render_image.height());
        let threads = self.thread_count().min(rows.len()).max(1);
        let (sums, features) = render_image.rows_and_features_mut();
        let mut row_slices: Vec<RowSlice> = match features {
            Some(f) => sums
                .chunks_mut(width)
                .zip(f.chunks_mut(width))
                .map(|(r, f)| (r, Some(f)))
                .collect(),
            None => sums.chunks_mut(width).map(|r| (r, None)).collect(),
        };
        let first_row = rows.start as usize;
        let mut row_slices: Vec<RowSlice> =
            row_slices.drain(first_row..rows.end as usize).collect();
        if threads == 1 {
            for (i, (row, features)) in row_slices.iter_mut().enumerate() {
                context.render_row((first_row + i) as u32, row, features.as_deref_mut());
            }
        } else {
            // Rows are handed out one at a time, so threads stay busy where the scene is uneven
            let next = std::sync::atomic::AtomicUsize::new(0);
            let row_slices: Vec<std::sync::Mutex<_>> =
                row_slices.into_iter().map(std::sync::Mutex::new).collect();
            let low_priority = self.low_priority;
            std::thread::scope(|s| {
                for _ in 0..threads {
                    s.spawn(|| {
                        if low_priority {
                            crate::priority::lower_this_thread(crate::priority::TRACING);
                        }
                        loop {
                            let y = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            if y >= row_slices.len() {
                                break;
                            }
                            let mut guard = row_slices[y].lock().unwrap();
                            let (row, features) = &mut *guard;
                            context.render_row(
                                (first_row + y) as u32,
                                row,
                                features.as_deref_mut(),
                            );
                        }
                    });
                }
            });
        }
        Ok(())
    }
}

/// Intensity of a luminaire's profile towards `direction` (from the luminaire).
pub(crate) fn profile_intensity(
    profile: &crate::IesProfile,
    down: Vec3,
    front: Vec3,
    direction: Vec3,
) -> f32 {
    let (vertical, horizontal) = luminaire_angles(direction, down, front);
    profile.intensity(vertical, horizontal)
}

/// Primary rays of a camera.
struct CameraRays {
    camera: Camera,
    /// For perspective cameras: from normalized device coordinates to a direction in world space.
    screen_to_direction: Option<Mat4>,
    lens: Option<Lens>,
}

impl CameraRays {
    fn new(camera: &Camera, depth_of_field: Option<DepthOfField>) -> Self {
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
            lens: Lens::of(camera, depth_of_field),
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

/// The sums of one row and, if collected, its feature sums.
type RowSlice<'a> = (&'a mut [[f32; 3]], Option<&'a mut [[f32; FEATURE_WORDS]]>);

/// What a camera ray hits first, for the features of a denoiser.
struct FirstHit {
    albedo: Vec3,
    normal: Vec3,
    distance: f32,
}

/// The distance given to the sky in the features.
pub(crate) const SKY_DISTANCE: f32 = 1e9;

impl FirstHit {
    fn sky(direction: Vec3) -> Self {
        Self {
            albedo: vec3(1.0, 1.0, 1.0),
            normal: -direction,
            distance: SKY_DISTANCE,
        }
    }

    fn at(surface: &Surface, distance: f32) -> Self {
        let m = &surface.material;
        Self {
            albedo: feature_albedo(m),
            normal: surface.shading_normal,
            distance,
        }
    }
}

/// The albedo a denoiser divides by: one where the surface shows what lies behind or in front of it.
pub(crate) fn feature_albedo(m: &RtMaterial) -> Vec3 {
    if m.transmission > 0.0 || (m.metallic > 0.5 && m.roughness < 0.2) {
        vec3(1.0, 1.0, 1.0)
    } else {
        m.base_color
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
    /// Photons through glass of this call, if caustics are on and photons landed.
    caustics: Option<std::sync::Arc<PhotonMap>>,
    /// The solids the camera is in, such as smoke or water
    camera_inside: Inside,
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

/// What scatters light towards `v`: a surface or a medium.
enum Scatterer<'a> {
    Surface {
        m: &'a RtMaterial,
        v: Vec3,
        n: Vec3,
        ng: Vec3,
    },
    Medium {
        anisotropy: f32,
        v: Vec3,
    },
}

impl Scatterer<'_> {
    /// Reflected luminance per luminance arriving from `l`, with the cosine of surfaces.
    fn evaluate(&self, l: Vec3) -> Vec3 {
        match self {
            Scatterer::Surface { m, v, n, ng } => {
                let cos = n.dot(l);
                if cos <= 0.0 || ng.dot(l) <= 0.0 {
                    return vec3(0.0, 0.0, 0.0);
                }
                evaluate_material(m, *v, l, *n) * cos
            }
            Scatterer::Medium { anisotropy, v } => {
                let p = phase(*anisotropy, -l.dot(*v));
                vec3(p, p, p)
            }
        }
    }

    /// Density of the directions the path draws itself.
    fn pdf(&self, l: Vec3) -> f32 {
        match self {
            Scatterer::Surface { m, v, n, .. } => material_pdf(m, *v, l, *n),
            Scatterer::Medium { anisotropy, v } => phase(*anisotropy, -l.dot(*v)),
        }
    }
}

/// A surface of a solid along a shadow ray.
struct ShadowCrossing {
    t: f32,
    solid: Solid,
    front_face: bool,
    cos_i: f32,
    /// Base color times transmission of the material at the hit.
    color: Vec3,
}

impl PathContext<'_> {
    fn render_row(
        &self,
        y: u32,
        row: &mut [[f32; 3]],
        mut features: Option<&mut [[f32; FEATURE_WORDS]]>,
    ) {
        for (x, sum) in row.iter_mut().enumerate() {
            for s in 0..self.settings.samples_per_call {
                let sample = self.first_sample + s;
                let mut rng = Rng::for_pixel(self.settings.seed, x as u32, y, sample);
                let u = (x as f32 + rng.next_f32()) / self.width as f32;
                let v = 1.0 - (y as f32 + rng.next_f32()) / self.height as f32;
                let (mut origin, mut direction) = self.rays.ray(u, v);
                if let Some(lens) = &self.rays.lens {
                    (origin, direction) =
                        lens.ray(origin, direction, rng.next_f32(), rng.next_f32());
                }
                let mut first = FirstHit::sky(direction);
                let mut l = self.radiance(origin, direction, &mut rng, &mut first);
                // A broken sample must not spoil the pixel for good
                if !(l.x.is_finite() && l.y.is_finite() && l.z.is_finite()) {
                    l = vec3(0.0, 0.0, 0.0);
                }
                sum[0] += l.x;
                sum[1] += l.y;
                sum[2] += l.z;
                if let Some(features) = features.as_deref_mut() {
                    let f = &mut features[x];
                    f[0] += first.albedo.x;
                    f[1] += first.albedo.y;
                    f[2] += first.albedo.z;
                    f[3] += first.normal.x;
                    f[4] += first.normal.y;
                    f[5] += first.normal.z;
                    f[6] += first.distance;
                    let lum = luminance(l);
                    f[7] += lum * lum;
                }
            }
        }
    }

    /// Luminance arriving along the ray, by following one path.
    fn radiance(
        &self,
        mut origin: Vec3,
        mut direction: Vec3,
        rng: &mut Rng,
        first: &mut FirstHit,
    ) -> Vec3 {
        let mut result = vec3(0.0, 0.0, 0.0);
        let mut throughput = vec3(1.0, 1.0, 1.0);
        // Lights that can not be hit by chance (the sun disc) are seen directly only after camera rays and mirrors
        let mut last_was_delta = true;
        // Density of the last direction drawn from a material, for weighting area lights hit by chance
        let mut last_pdf = 0.0;
        // What happened since the last diffuse or glossy bounce, to count light only once: either the light sampled
        // there directly (straight through thin glass, and through solid glass without caustics) or the photons
        // through glass, or the light the path hits now
        let mut diffuse_seen = false;
        let mut diffuse_origin = origin;
        let mut glass_reflected = false;
        let mut glass_bent = false;
        let caustics = self.settings.caustic_photons > 0;
        let media = self.scene.has_media();
        let mut inside = self.camera_inside;
        // The solid just left and how close to the exit its surfaces do not count, for surfaces touching it
        let mut skip = None;
        // Boundaries of media passed, which do not count as bounces
        let mut invisible = false;
        let mut passed = 0;
        let mut bounce = 0;
        loop {
            let light_hit = self.nearest_area_light(origin, direction);
            let max_distance = light_hit
                .as_ref()
                .map_or(f32::INFINITY, |(_, h)| h.distance);
            let hit = self.accel.intersect_skipping(
                self.scene,
                origin,
                direction,
                max_distance,
                skip.take(),
            );
            if media {
                let end = hit.as_ref().map_or(max_distance, |h| h.t);
                let (scattered, weight) = self.sample_medium(inside, origin, direction, end, rng);
                throughput = mul(throughput, weight);
                if let Some((t, anisotropy)) = scattered {
                    if bounce >= self.settings.max_bounces {
                        break;
                    }
                    // Light scattered in the medium: the light sources directly, then a new direction
                    if bounce == 0 && passed == 0 {
                        *first = FirstHit {
                            albedo: vec3(1.0, 1.0, 1.0),
                            normal: -direction,
                            distance: t,
                        };
                    }
                    let p = origin + direction * t;
                    let v = -direction;
                    let scatterer = Scatterer::Medium { anisotropy, v };
                    result += self.add(
                        bounce,
                        mul(throughput, self.direct_light(p, &scatterer, inside, rng)),
                    );
                    let l = sample_phase(anisotropy, direction, rng.next_f32(), rng.next_f32());
                    last_pdf = scatterer.pdf(l);
                    direction = l;
                    origin = p;
                    last_was_delta = false;
                    diffuse_seen = true;
                    diffuse_origin = p;
                    glass_reflected = false;
                    glass_bent = false;
                    bounce += 1;
                    if bounce >= 3 && !self.roulette(&mut throughput, rng) {
                        break;
                    }
                    continue;
                }
                if throughput == vec3(0.0, 0.0, 0.0) {
                    break;
                }
            }
            let Some(hit) = hit else {
                if let Some((light, light_hit)) = light_hit {
                    // Area lights end the path; their light was also sampled directly, so both share it
                    let weight = if !last_was_delta {
                        power_heuristic(
                            last_pdf,
                            area_light_pdf(light, origin, direction, light_hit.distance),
                        )
                    } else if !diffuse_seen || glass_reflected {
                        // Seen in glass from the camera, or by a way the light was not sampled
                        1.0
                    } else if !glass_bent {
                        // Straight through thin glass, the direction drawn at the last diffuse bounce
                        let at = origin + direction * light_hit.distance;
                        power_heuristic(
                            last_pdf,
                            area_light_pdf(
                                light,
                                diffuse_origin,
                                direction,
                                (at - diffuse_origin).magnitude(),
                            ),
                        )
                    } else {
                        // Through solid glass the light was sampled straight already
                        0.0
                    };
                    result += self.add(bounce, mul(throughput, light_hit.radiance) * weight);
                    break;
                }
                result += self.add(
                    bounce,
                    mul(
                        throughput,
                        self.environment(
                            direction,
                            last_was_delta && (!diffuse_seen || (glass_reflected && !caustics)),
                        ),
                    ),
                );
                break;
            };
            let surface = self.surface(&hit, direction);
            let m = &surface.material;
            let solid = Solid::of(hit.instance, m);
            if let Some(solid) = solid {
                let (after, boundary) = inside.cross(solid, surface.front_face);
                if !boundary.seen {
                    // Inside a solid of higher priority, or touching another: not there for light
                    inside = after;
                    (origin, skip) = past(&surface, hit.instance, direction);
                    passed += 1;
                    if passed > 64 {
                        break;
                    }
                    continue;
                }
            }
            if bounce == 0 && passed == 0 {
                *first = FirstHit::at(&surface, hit.t);
            }
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
                // Smooth glass: reflect or pass, chosen by the Fresnel term; solids from the medium around them
                let (after, eta) = match solid {
                    Some(solid) => {
                        let (after, boundary) = inside.cross(solid, surface.front_face);
                        (after, boundary.eta)
                    }
                    None => (inside, m.index_of_refraction),
                };
                let cos_i = n.dot(v).clamp(0.0, 1.0);
                let refracted = if m.thin_walled {
                    Some(direction)
                } else {
                    refract(v, n, 1.0 / eta)
                };
                match refracted {
                    Some(refracted) if rng.next_f32() >= fresnel_dielectric(cos_i, eta) => {
                        // The boundary of a medium is no bounce, it is not seen
                        invisible = media && !m.thin_walled && eta == 1.0;
                        throughput = mul(throughput, m.base_color);
                        if m.thin_walled {
                            origin = offset(surface.position, -ng);
                        } else {
                            direction = refracted;
                            glass_bent |= eta != 1.0;
                            inside = after;
                            (origin, skip) = past(&surface, hit.instance, direction);
                        }
                    }
                    _ => {
                        // Reflected, also all of it inside the glass
                        direction = reflect(v, n);
                        origin = offset(surface.position, ng);
                        glass_reflected = true;
                        glass_bent = true;
                    }
                }
                last_was_delta = true;
            } else {
                // Diffuse and glossy reflection: light sources directly, then a new direction from the material
                let p = offset(surface.position, ng);
                let scatterer = Scatterer::Surface { m, v, n, ng };
                result += self.add(
                    bounce,
                    mul(throughput, self.direct_light(p, &scatterer, inside, rng)),
                );
                if let Some(map) = &self.caustics {
                    let mut gathered = vec3(0.0, 0.0, 0.0);
                    map.gather(surface.position, ng, |photon| {
                        gathered +=
                            mul(evaluate_material(m, v, -photon.direction, n), photon.power);
                    });
                    result += self.add(bounce, mul(throughput, gathered / map.area()));
                }

                let Some((l, weight, pdf)) = sample_material(m, v, n, rng) else {
                    break;
                };
                last_pdf = pdf;
                if l.dot(ng) <= 0.0 {
                    break;
                }
                throughput = mul(throughput, weight);
                direction = l;
                origin = p;
                last_was_delta = false;
                diffuse_seen = true;
                diffuse_origin = p;
                glass_reflected = false;
                glass_bent = false;
            }

            if std::mem::take(&mut invisible) {
                passed += 1;
                if passed > 64 {
                    break;
                }
                continue;
            }
            bounce += 1;
            if bounce >= 3 && !self.roulette(&mut throughput, rng) {
                break;
            }
        }
        result
    }

    /// Russian roulette ends dark paths early without changing the expected value. Returns whether the path goes on.
    fn roulette(&self, throughput: &mut Vec3, rng: &mut Rng) -> bool {
        let q = throughput
            .x
            .max(throughput.y)
            .max(throughput.z)
            .clamp(0.05, 0.95);
        if rng.next_f32() > q {
            return false;
        }
        *throughput /= q;
        true
    }

    /// The medium the ray is in and how its density goes along the ray: fog, or homogeneous inside solids.
    fn medium_at(&self, inside: Inside) -> Option<(RtMedium, Option<&RtFog>)> {
        if inside.is_air() {
            self.scene.fog().map(|f| (f.medium, Some(f)))
        } else {
            inside.medium().filter(|m| !m.is_clear()).map(|m| (m, None))
        }
    }

    /// The solids that contain a point, see [solids_containing].
    fn inside_at(&self, p: Vec3) -> Inside {
        solids_containing(self.scene, self.accel, p)
    }

    ///
    /// Where light along the ray scatters in the medium before `end`, chosen by the extinction of a random color
    /// channel, and the weight of the path: the scattering there or the light passing to `end`, over the mean density
    /// of the three channels. Returns the distance and the anisotropy if it scatters.
    ///
    fn sample_medium(
        &self,
        inside: Inside,
        origin: Vec3,
        direction: Vec3,
        end: f32,
        rng: &mut Rng,
    ) -> (Option<(f32, f32)>, Vec3) {
        let one = vec3(1.0, 1.0, 1.0);
        let Some((medium, fog)) = self.medium_at(inside) else {
            return (None, one);
        };
        let sigma = medium.extinction();
        let depth_at = |t: f32| match fog {
            Some(fog) => fog.depth(origin, direction, t),
            None => t,
        };
        let transmittance = |d: f32| {
            if d.is_infinite() {
                let open = |s: f32| if s > 0.0 { 0.0 } else { 1.0 };
                vec3(open(sigma.x), open(sigma.y), open(sigma.z))
            } else {
                exp_neg(sigma * d)
            }
        };
        if medium.scattering == vec3(0.0, 0.0, 0.0) {
            // Only absorbing, as tinted glass: the light left, without noise
            return (None, transmittance(depth_at(end)));
        }
        let channel = ((rng.next_f32() * 3.0) as usize).min(2);
        let depth = -(1.0 - rng.next_f32()).ln() / sigma[channel];
        let t = match fog {
            Some(fog) => fog.distance_to_depth(origin, direction, depth),
            None => Some(depth),
        }
        .filter(|t| t.is_finite() && *t < end);
        let mean = |v: Vec3| (v.x + v.y + v.z) / 3.0;
        match t {
            Some(t) => {
                let tr = exp_neg(sigma * depth_at(t));
                // The density at t cancels between scattering and the density of t
                let pdf = mean(mul(sigma, tr));
                if pdf <= 0.0 {
                    return (None, vec3(0.0, 0.0, 0.0));
                }
                (
                    Some((t, medium.anisotropy)),
                    mul(medium.scattering, tr) / pdf,
                )
            }
            None => {
                let tr = transmittance(depth_at(end));
                let p = mean(tr);
                if p <= 0.0 {
                    return (None, vec3(0.0, 0.0, 0.0));
                }
                (None, tr / p)
            }
        }
    }

    ///
    /// The photons of one call, the `pass`th: from the lights that cast caustics through solid glass to the first
    /// surface behind it that is not glass. Light that only passed thin glass is left out, shadow rays find it.
    ///
    fn shoot_photons(&self, pass: u32) -> PhotonMap {
        let radius = gather_radius(self.settings.caustic_radius, pass);
        let spheres = glass_spheres(self.scene);
        let sources = caustic_sources(self.scene);
        let scene_box = scene_box(self.accel);
        if spheres.is_empty() || sources.is_empty() || scene_box.is_none() {
            return PhotonMap::new(Vec::new(), radius);
        }
        let (center, extent) = scene_box.expect("a box");
        let per_source = (self.settings.caustic_photons / sources.len() as u32).max(1);
        let mut photons = Vec::new();
        for (s, source) in sources.iter().enumerate() {
            let mut rng = Rng::new(
                self.settings.seed ^ pass.wrapping_mul(0x9e37_79b9),
                0x5eed_0000_0000 ^ ((s as u64) << 32) ^ pass as u64,
            );
            for _ in 0..per_source {
                let Some(emission) = source.emit(&spheres, per_source, &mut rng) else {
                    break;
                };
                // Parallel light starts outside the scene
                let origin = if matches!(source, CausticSource::Parallel { .. }) {
                    let point = emission.origin;
                    point - emission.direction * ((point - center).magnitude() + extent)
                } else {
                    emission.origin
                };
                self.trace_photon(
                    origin,
                    emission.direction,
                    emission.power,
                    &mut rng,
                    &mut photons,
                );
            }
        }
        PhotonMap::new(photons, radius)
    }

    /// Follows a photon through glass and keeps it where it lands, if the glass bent it.
    fn trace_photon(
        &self,
        mut origin: Vec3,
        mut direction: Vec3,
        mut power: Vec3,
        rng: &mut Rng,
        photons: &mut Vec<Photon>,
    ) {
        let mut bent = false;
        let media = self.scene.has_media();
        let mut inside = Inside::AIR;
        let mut skip = None;
        for _ in 0..self.settings.max_bounces.max(1) + 72 {
            let Some(hit) = self.accel.intersect_skipping(
                self.scene,
                origin,
                direction,
                f32::INFINITY,
                skip.take(),
            ) else {
                return;
            };
            if media {
                // Light scattered out of the photon reaches the surfaces by the paths through the medium
                if let Some((medium, fog)) = self.medium_at(inside) {
                    let depth = fog.map_or(hit.t, |f| f.depth(origin, direction, hit.t));
                    power = mul(power, exp_neg(medium.extinction() * depth));
                }
            }
            let surface = self.surface(&hit, direction);
            let m = &surface.material;
            let solid = Solid::of(hit.instance, m);
            let (after, boundary) = match solid {
                Some(solid) => {
                    let (after, boundary) = inside.cross(solid, surface.front_face);
                    (after, Some(boundary))
                }
                None => (inside, None),
            };
            if boundary.is_some_and(|b| !b.seen) {
                inside = after;
                (origin, skip) = past(&surface, hit.instance, direction);
                continue;
            }
            let v = -direction;
            let ng = surface.geometric_normal;
            let mut n = surface.shading_normal;
            if n.dot(v) <= 0.0 {
                n = ng;
            }
            if m.transmission > 0.0 && rng.next_f32() < m.transmission {
                // The same choices as the paths from the camera
                let eta = boundary.map_or(m.index_of_refraction, |b| b.eta);
                let cos_i = n.dot(v).clamp(0.0, 1.0);
                let refracted = if m.thin_walled {
                    Some(direction)
                } else {
                    refract(v, n, 1.0 / eta)
                };
                match refracted {
                    Some(refracted) if rng.next_f32() >= fresnel_dielectric(cos_i, eta) => {
                        power = mul(power, m.base_color);
                        if m.thin_walled {
                            origin = offset(surface.position, -ng);
                        } else {
                            direction = refracted;
                            bent |= eta != 1.0;
                            inside = after;
                            (origin, skip) = past(&surface, hit.instance, direction);
                        }
                    }
                    _ => {
                        direction = reflect(v, n);
                        origin = offset(surface.position, ng);
                        bent = true;
                    }
                }
            } else {
                if bent {
                    photons.push(Photon {
                        position: surface.position,
                        direction,
                        power,
                    });
                }
                return;
            }
        }
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

    /// Light from the sun and the light sources reaching the point and scattered towards the viewer.
    fn direct_light(&self, p: Vec3, scatterer: &Scatterer, inside: Inside, rng: &mut Rng) -> Vec3 {
        let mut result = vec3(0.0, 0.0, 0.0);
        let photons = self.settings.caustic_photons > 0;
        if let RtEnvironment::Sky(sky) = self.scene.environment() {
            if sky.sun_illuminance() > 0.0 {
                let (t, b) = orthonormal_basis(sky.sun_direction());
                let c = uniform_cone(rng.next_f32(), rng.next_f32(), sky.sun_cos_radius());
                let l = (t * c.x + b * c.y + sky.sun_direction() * c.z).normalize();
                result += self.light_from(
                    p,
                    scatterer,
                    inside,
                    l,
                    f32::INFINITY,
                    sky.sun_color() * sky.sun_illuminance(),
                    photons,
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
                    result += self.light_from(
                        p,
                        scatterer,
                        inside,
                        l,
                        f32::INFINITY,
                        *color * *illuminance,
                        photons,
                    );
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
                            scatterer,
                            inside,
                            l,
                            d * (1.0 - 1e-4),
                            *color * (*intensity / d2),
                            photons,
                        );
                    }
                }
                RtLight::Luminaire {
                    position,
                    down,
                    front,
                    color,
                    profile,
                    scale,
                } => {
                    let to_light = *position - p;
                    let d2 = to_light.magnitude2();
                    if d2 > 0.0 {
                        let d = d2.sqrt();
                        let l = to_light / d;
                        result += self.light_from(
                            p,
                            scatterer,
                            inside,
                            l,
                            d * (1.0 - 1e-4),
                            *color * (*scale * profile_intensity(profile, *down, *front, -l) / d2),
                            photons,
                        );
                    }
                }
                RtLight::Rect { .. } | RtLight::Sphere { .. } => {
                    let (u1, u2) = (rng.next_f32(), rng.next_f32());
                    if let Some(s) = sample_area_light(light, p, u1, u2) {
                        // Shared with the material's own samples that hit the light
                        let weight = power_heuristic(s.pdf, scatterer.pdf(s.direction)) / s.pdf;
                        result += self.light_from(
                            p,
                            scatterer,
                            inside,
                            s.direction,
                            s.distance * (1.0 - 1e-4),
                            s.radiance * weight,
                            false,
                        );
                    }
                }
            }
        }
        result
    }

    /// The nearest area light along a ray, ignoring the scene.
    fn nearest_area_light(&self, origin: Vec3, direction: Vec3) -> Option<(&RtLight, LightHit)> {
        let mut nearest: Option<(&RtLight, LightHit)> = None;
        for light in self.scene.lights().iter().filter(|l| l.is_area()) {
            let max = nearest.as_ref().map_or(f32::INFINITY, |(_, h)| h.distance);
            if let Some(hit) = hit_area_light(light, origin, direction, max) {
                nearest = Some((light, hit));
            }
        }
        nearest
    }

    /// Reflected luminance for illuminance `e` (lux, on a surface facing the light) from direction `l`.
    #[allow(clippy::too_many_arguments)]
    fn light_from(
        &self,
        p: Vec3,
        scatterer: &Scatterer,
        inside: Inside,
        l: Vec3,
        distance: f32,
        e: Vec3,
        photons: bool,
    ) -> Vec3 {
        let f = scatterer.evaluate(l);
        if f == vec3(0.0, 0.0, 0.0) {
            return f;
        }
        let transmittance = self.transmittance(p, l, distance, photons, inside);
        mul(mul(f, e), transmittance)
    }

    /// How much light passes along a shadow ray: nothing through opaque surfaces, some through glass.
    ///
    /// With `photons` the light comes as photons through solid glass, which then casts a full shadow.
    ///
    /// The ray starts `inside` a solid or in the air; media along it absorb and scatter light out of it.
    ///
    fn transmittance(
        &self,
        origin: Vec3,
        direction: Vec3,
        distance: f32,
        photons: bool,
        inside: Inside,
    ) -> Vec3 {
        let mut t = vec3(1.0, 1.0, 1.0);
        let fog = self.scene.fog();
        let fog_depth = |d: f32| fog.map_or(0.0, |f| f.depth(origin, direction, d));
        // Surfaces of solids in the order along the ray, which counts where solids overlap
        let mut crossings: Vec<ShadowCrossing> = Vec::new();
        self.accel
            .for_each_hit(self.scene, origin, direction, distance, |hit| {
                let instance = &self.scene.instances()[hit.instance as usize];
                let m = &self.scene.materials()[instance.material.0 as usize];
                if m.transmission <= 0.0 || (photons && bends_light(m)) {
                    t = vec3(0.0, 0.0, 0.0);
                    return true;
                }
                let s = self.surface(hit, direction);
                let m = &s.material;
                let cos_i = s.geometric_normal.dot(-direction).clamp(0.0, 1.0);
                let color = m.base_color * m.transmission;
                match Solid::of(hit.instance, m) {
                    Some(solid) => crossings.push(ShadowCrossing {
                        t: hit.t,
                        solid,
                        front_face: s.front_face,
                        cos_i,
                        color,
                    }),
                    None => {
                        t = mul(
                            t,
                            color * (1.0 - fresnel_dielectric(cos_i, m.index_of_refraction)),
                        )
                    }
                }
                t.x.max(t.y).max(t.z) <= 0.0
            });
        if t == vec3(0.0, 0.0, 0.0) {
            return t;
        }
        crossings.sort_by(|a, b| a.t.total_cmp(&b.t));
        let mut depth = vec3(0.0, 0.0, 0.0);
        // The light lost between two distances where the ray is
        let segment = |inside: &Inside, from: f32, to: f32| -> Vec3 {
            if inside.is_air() {
                fog.map_or(vec3(0.0, 0.0, 0.0), |f| {
                    f.medium.extinction() * (fog_depth(to) - fog_depth(from)).max(0.0)
                })
            } else {
                inside.medium().map_or(vec3(0.0, 0.0, 0.0), |m| {
                    m.extinction() * (to - from).max(0.0)
                })
            }
        };
        let mut inside = inside;
        let mut from = 0.0;
        for c in &crossings {
            depth += segment(&inside, from, c.t);
            let (after, boundary) = inside.cross(c.solid, c.front_face);
            if boundary.seen {
                // A shadow ray goes straight, so the reflection is the same both ways through a surface
                let eta = if boundary.eta < 1.0 {
                    1.0 / boundary.eta
                } else {
                    boundary.eta
                };
                t = mul(t, c.color * (1.0 - fresnel_dielectric(c.cos_i, eta)));
            }
            inside = after;
            from = c.t;
        }
        if distance.is_infinite() {
            // Open meshes and fog that does not thin out: the medium does not end
            let open = |s: f32| if s > 0.0 { 0.0 } else { 1.0 };
            let sigma = if inside.is_air() {
                match fog {
                    Some(f) if fog_depth(f32::INFINITY).is_infinite() => f.medium.extinction(),
                    _ => vec3(0.0, 0.0, 0.0),
                }
            } else {
                inside
                    .medium()
                    .map_or(vec3(0.0, 0.0, 0.0), |m| m.extinction())
            };
            t = mul(t, vec3(open(sigma.x), open(sigma.y), open(sigma.z)));
        }
        depth += segment(&inside, from, distance);
        if !(depth.x.is_finite() && depth.y.is_finite() && depth.z.is_finite()) {
            let keep = |d: f32| if d.is_finite() { 1.0 } else { 0.0 };
            depth = vec3(
                keep(depth.x) * depth.x,
                keep(depth.y) * depth.y,
                keep(depth.z) * depth.z,
            );
        }
        mul(t, exp_neg(depth))
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

///
/// The solids that contain a point, such as the camera in smoke or under water: from the surfaces of solids a ray from
/// it to infinity hits.
///
pub(crate) fn solids_containing(scene: &RtScene, accel: &CpuAccel, p: Vec3) -> Inside {
    let direction = vec3(0.267, 0.873, 0.408).normalize();
    let mut hits = Vec::new();
    accel.for_each_hit(scene, p, direction, f32::INFINITY, |hit| {
        let instance = &scene.instances()[hit.instance as usize];
        let m = &scene.materials()[instance.material.0 as usize];
        if let Some(solid) = Solid::of(hit.instance, m) {
            let mesh = &scene.meshes()[instance.mesh.0 as usize];
            let [a, b, c] = mesh.triangles[hit.triangle as usize];
            let (p0, p1, p2) = (
                mesh.positions[a as usize],
                mesh.positions[b as usize],
                mesh.positions[c as usize],
            );
            let face =
                (instance.inverse.transpose() * (p1 - p0).cross(p2 - p0).extend(0.0)).truncate();
            hits.push((hit.t, solid, face.dot(direction) < 0.0));
        }
        false
    });
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    Inside::of_point(hits.into_iter().map(|(_, s, front)| (s, front)))
}

///
/// Where a ray goes on after it passed a surface of a solid, with the solid's surfaces skipped close to that point: a
/// little before the surface on the new way, so that another surface touching it there (a pane of glass lying on the
/// floor) is hit and not jumped over.
///
fn past(surface: &Surface, instance: u32, direction: Vec3) -> (Vec3, Option<(u32, f32)>) {
    let p = surface.position;
    let gap = 1e-4 * 1.0f32.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
    (p - direction * gap, Some((instance, 2.0 * gap)))
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
/// A new direction from the material, the weight f·cos/pdf and the pdf, sampling the glossy part by its visible normals and the
/// diffuse part by the cosine, combined by their probabilities.
///
pub(crate) fn sample_material(
    m: &RtMaterial,
    v: Vec3,
    n: Vec3,
    rng: &mut Rng,
) -> Option<(Vec3, Vec3, f32)> {
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
    Some((l, evaluate_material(m, v, l, n) * (n_dot_l / pdf), pdf))
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
            if let Some((_, w, _)) = sample_material(m, v, n, &mut rng) {
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
