# three-d-raytrace

Path tracing for three-d scenes, part of the fork. Step 1 of the plan: the scene description, a two level bounding
volume hierarchy and a reference path tracer on the CPU. GPU tracers on the wgpu backend come next and use the same
scene, film and material model.

```rust
use three_d_raytrace::*;

let mut scene = RtScene::new();
scene.set_environment(RtEnvironment::Sky(Sky::new(
    GeoLocation::new(52.52, 13.405),               // Berlin
    LocalDateTime::new(2026, 6, 21, 17, 0, 2.0),   // 17:00 summer time
    SceneOrientation::default(),                   // Y up, north along -Z
)));
let mesh = scene.add_mesh(&CpuMesh::cube())?;
let material = scene.add_material(CpuMaterial::default()); // or an RtMaterial
scene.add_instance(mesh, material, Mat4::identity())?;

let camera = Camera::new_perspective(/* as for the raster view */);
let mut film = Film::new(960, 600);
let mut tracer = CpuReferenceTracer::new();
while film.sample_count() < 256 {
    tracer.trace(&scene, &camera, &mut film, &TraceSettings { samples_per_call: 8, ..Default::default() })?;
}
film.save_png("garden.png", &ToneMapping { exposure_ev100: 14.0, ..Default::default() })?;
film.save_exr("garden.exr")?; // linear, cd/m²
```

## Parts

| Part | Type | Exchangeable |
| --- | --- | --- |
| Scene | `RtScene`, built from `CpuMesh`, `CpuMaterial`, `CpuModel` or `RtMesh`, `RtMaterial` | no, shared by all tracers |
| Acceleration structure | `AccelBuilder`, now `CpuBvhBuilder` → `CpuAccel` | yes: GPU buffers, wgpu `Blas`/`Tlas` later |
| Tracer | `Tracer`, now `CpuReferenceTracer` | yes: compute shader and ray query tracers later |
| Film | `Film` with `ToneMapping`, PNG and OpenEXR export | no |
| Sky and sun | `Sky` (Preetham), `SunPosition` (NOAA), `GeoLocation`, `LocalDateTime`, `SceneOrientation` | no |

Units are photometric: luminance in cd/m², illuminance in lux, intensity in cd. The exposure is given as EV100 like on
a camera, about 15 for a sunny day and 7 for a lit room.

## With three-d

| Feature | Gives |
| --- | --- |
| `three-d` | `Film::to_three_d_texture` and `Film::write_to_three_d_texture`: the film as a three-d `Texture2D` (32 bit floats, upright), on any backend, to show it while samples are added |
| `wgpu` | `RaytraceSupport::of(&context)`: `Cpu` (OpenGL, WebGL2), `Compute` (WebGPU, Vulkan, Metal, DirectX 12) or `HardwareRayQuery`; `GpuDevice::from_context` with the device and queue for GPU tracers |

Textures are shared with wgpu through `Texture2D::as_wgpu` in three-d, which keeps the order of three-d's work and the
tracer's on the queue.

## Material model

`RtMaterial` is the metallic roughness model of `CpuMaterial` (GGX with height correlated Smith masking, Schlick's
Fresnel term, Lambert for the diffuse part) plus a few settings for tracing: `specular` (strength of the reflection of
non-metals, 0 is purely diffuse), `thin_walled` (glass panes modelled as one face, the default) and
`alpha_cutout`. Textures follow in step 4 of the plan.

## What the reference tracer does and does not do

- Paths sample the sun disc, directional and point lights at every diffuse or glossy bounce, the sky by the material,
  and end by Russian roulette.
- Shadow rays pass through transmissive surfaces with their transmission and Fresnel term, so sunlight falls through
  windows into rooms.
- Not included: caustics (light focused by glass or mirrors onto a surface) and refraction on the way to a light.
- Deterministic: the same seed gives the same image with any number of threads.

## Tests

`cargo test -p three-d-raytrace` checks the hierarchy against testing every triangle, the sun position against solar
noon at the solstices and equinox, the material model for energy conservation, and the tracer against values known
from physics: a white sphere disappears in a uniform environment (white furnace), a white ground under the sky
reflects E/π, a point light falls off with the square of the distance, and a pane of glass lets 96 % of the light
through. A reference image of the garden scene is compared in blocks of 4 × 4 pixels, write it again with
`THREE_D_WRITE_REFERENCE=1 cargo test --release -p three-d-raytrace`.

The example renders the same scene: `cargo run --release -p three-d-raytrace --example garden -- 256 960 600`.
