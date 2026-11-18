# three-d-raytrace

Path tracing for three-d scenes, part of the fork: the scene description, a two level bounding volume hierarchy, a
reference path tracer on the CPU and the same path tracer in compute shaders of three-d's wgpu backend. Both use the
same scene, render image and material model.

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
let mut render_image = RenderImage::new(960, 600);
let mut tracer = CpuReferenceTracer::new();
while render_image.sample_count() < 256 {
    tracer.trace(&scene, &camera, &mut render_image, &TraceSettings { samples_per_call: 8, ..Default::default() })?;
}
render_image.save_png("garden.png", &ToneMapping { exposure_ev100: 14.0, ..Default::default() })?;
render_image.save_exr("garden.exr")?; // linear, cd/m²
```

## Parts

| Part | Type | Exchangeable |
| --- | --- | --- |
| Scene | `RtScene`, built from `CpuMesh`, `CpuMaterial`, `CpuModel` or `RtMesh`, `RtMaterial` | no, shared by all tracers |
| Acceleration structure | `AccelBuilder`, now `CpuBvhBuilder` → `CpuAccel` | yes: GPU buffers, wgpu `Blas`/`Tlas` later |
| Tracer | `Tracer`: `CpuReferenceTracer`, `ComputeTracer` (feature `wgpu`) | yes: a ray query tracer later |
| Render image | `RenderImage` with `ToneMapping`, PNG and OpenEXR export | no |
| Sky and sun | `Sky` (Preetham), `SunPosition` (NOAA), `GeoLocation`, `LocalDateTime`, `SceneOrientation` | no |

Units are photometric: luminance in cd/m², illuminance in lux, intensity in cd. The exposure is given as EV100 like on
a camera, about 15 for a sunny day and 7 for a lit room.

## With three-d

| Feature | Gives |
| --- | --- |
| `three-d` | `RenderImage::to_three_d_texture` and `RenderImage::write_to_three_d_texture`: the render image as a three-d `Texture2D` (32 bit floats, upright), on any backend, to show it while samples are added |
| `wgpu` | `RaytraceSupport::of(&context)`: `Cpu` (OpenGL, WebGL2), `Compute` (WebGPU, Vulkan, Metal, DirectX 12) or `HardwareRayQuery`; `GpuDevice::from_context` with the device and queue; `ComputeTracer` and `GpuRenderImage` |

| `three-d` | `RaytracePreview`: a view that gets finer every frame while camera and scene stay the same, on the GPU where there are compute shaders and otherwise on the CPU at a lower resolution; `PreviewAvailability::of(&context)` says which and why |

```rust
let mut preview = RaytracePreview::new(&context, width, height);
// Every frame; starts again by itself when the camera or the scene changed
preview.update(&scene, &camera)?;
preview.render(&target); // exposed, tone mapped and sRGB encoded
```

Textures are shared with wgpu through `Texture2D::as_wgpu` in three-d, which keeps the order of three-d's work and the
tracer's on the queue.

## On the GPU

```rust
let mut tracer = ComputeTracer::from_context(&context).expect("compute shaders");
let mut image = GpuRenderImage::new(tracer.gpu(), width, height);
let texture = image.to_three_d_texture(&context);
// Every frame while the camera stands still
tracer.trace_gpu(&scene, &camera, &mut image, &TraceSettings::default())?;
image.write_to_three_d_texture(&texture); // average per pixel, on the GPU
// When the camera or the scene changes
image.reset();
// For export: image.read() natively, image.read_async().await in the browser
```

`ComputeTracer` runs the same paths as the CPU tracer in a WGSL compute shader. The hierarchies are built on the CPU
and uploaded as storage buffers, again only when the scene changed. The image is traced in tiles (256 × 256 pixels by
default, `with_tile_size`), one dispatch per tile and sample, so no dispatch runs long enough for the system to reset
the GPU. `ComputeTracer` also implements `Tracer` and then reads every call back into a `RenderImage`, which a browser
can not wait for. Its random numbers differ from the CPU tracer's, so the images agree in their averages, not in their
noise.

## Material model

`RtMaterial` is the metallic roughness model of `CpuMaterial` (GGX with height correlated Smith masking, Schlick's
Fresnel term, Lambert for the diffuse part) plus a few settings for tracing: `specular` (strength of the reflection of
non-metals, 0 is purely diffuse), `thin_walled` (glass panes modelled as one face, the default) and
`alpha_cutout`.

Textures are added to the scene with `add_texture` (an `RtTexture` with four bytes per texel, the first row at v = 0
as in three-d) and referred to by the material: `base_color_texture` (sRGB color and alpha), `metallic_roughness_texture`
(glTF channels: roughness green, metallic blue), `normal_texture` with `normal_scale`, `emission_texture` and
`transmission_texture` (red). `RtScene::add_cpu_material` and `add_cpu_model` take them over from a `CpuMaterial`;
occlusion textures are not needed, tracing finds the occlusion itself. Normal textures use the tangents of the mesh,
or the direction of u and v as three-d computes missing tangents. With `alpha_cutout`, every hit tests the alpha of the
base color texture, so leaves on a few quads cast the shadows of their outline. Textures are interpolated between four
texels (or the nearest with `nearest`) and repeat, mirror or clamp as given; there are no mip maps, the samples per
pixel average fine detail. Textures with floats are stored clamped to 0 to 1.

The compute tracer keeps all textures in one storage buffer. If they do not fit into the largest buffer the device
binds (128 MB guaranteed by WebGPU), the largest ones are halved in size until they do.

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

With the `wgpu` feature, `tests/gpu.rs` runs the same physical checks with the compute tracer and compares its garden
image with the CPU tracer's in blocks of 6 × 6 pixels; set `THREE_D_REQUIRE_WGPU=1` to fail instead of skipping without
an adapter.

The examples render the same scene: `cargo run --release -p three-d-raytrace --example garden -- 256 960 600` on the
CPU and `cargo run --release -p three-d-raytrace --features wgpu --example garden_gpu -- 256 960 600` with the compute
tracer.
