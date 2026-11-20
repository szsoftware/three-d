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

On the CPU the preview traces on a thread of its own (`PreviewSettings::cpu_in_background`, not in the browser), so
the frame rate of the application does not depend on the tracer; `update` shows the newest image. In the browser,
which has one thread, `update` traces row after row for at most `PreviewSettings::cpu_frame_time` (12 ms) and shows a
sample when it is complete, at an eighth of the resolution (`cpu_downscale` 8), so the page stays responsive. Natively
the CPU traces with one thread less than the machine has (`PreviewSettings::cpu_threads`), so the application keeps a
core for its frames, and the threads of the preview run at nice 10 on Linux and Android. `driver_problem`
names devices whose drivers crash with the tracer (none known at the moment); `RaytraceSupport::of` reports `Cpu` for
them and the preview says why.

On the GPU the preview watches the GPU's own time of every update while it traces (`PreviewSettings::gpu_frame_time`,
40 ms by default), from timestamp queries where the device has them (also in Chrome), in browsers without them from
when the browser tells that the work is done, and otherwise from the time between updates. After three slower updates
in a row it halves the resolution, down to a quarter, and then goes on with the CPU, with the reason in
`availability`. One five times slower (under 5 fps) goes at once to the resolution that would be fast enough, or to the
CPU. Weak GPUs such as the Intel HD 4000 trace no faster than four CPU cores (about 0.5 million samples per second on
the T430) but hold up every frame of the application while they do; the CPU traces beside the frames.
`gpu_downscale` sets the starting resolution, `RaytracePreview::gpu_downscale` tells the current one. The texture is
then smaller than the viewport; call `resize` only when the viewport changes, it starts again (and does nothing for
the same size). A new `gpu_frame_time` starts from `gpu_downscale` again, also back from the CPU when the GPU was too
slow; `None` keeps the full resolution, for a switch such as "full resolution, fewer frames".

While camera and scene stand still, a GPU at a lower resolution goes on at the full one after 16 samples
(`gpu_sharp_when_still`, on by default): `ComputeTracer::trace_gpu_tiles` spreads a sample over several updates, as many
tiles in each as fit into `gpu_frame_time`, and the preview shows the image once it has 4 samples, so the finished
image is sharp. A move of the camera goes back to the lower resolution.

The shaders of the GPU compile on another thread at nice 19, which takes about 8 s on the HD 4000; the CPU traces
meanwhile and the preview goes on with the GPU in the first `update` after (`is_compiling`, `wait_for_gpu`). With wgpu
on OpenGL no other thread may use the context while a frame is drawn (wgpu gives up after a second with "Could not
lock adapter context"), so there the shaders compile in the `update` after the CPU shows its first image, and that
update takes as long. On Intel graphics of the 7th generation over OpenGL the preview takes the CPU right away
(`PreviewAvailability::CpuOnly` says why): the GPU tracer is no faster there, and the image would stand still while
it compiles. In the browser wgpu can only compile synchronously,
which holds the page (in headless Chromium with SwiftShader for minutes); there `ComputeTracer::from_context_async`
first lets the browser compile the same pipelines with `createComputePipelineAsync` in the background, and wgpu then
finds them in the browser's cache (12 s in the background, then 1 ms, in the same Chromium). It needs the browser's
`GPUDevice`, which three-d keeps when it creates the device (`Context::webgpu_device`). When the samples move from the
CPU to the GPU, the CPU's image is kept: if the preview goes back to the CPU for the same view, it goes on from there.

Errors of the GPU in the work of the compute tracer (an invalid shader, too little memory) are caught with wgpu's error
scopes instead of three-d's error handler: `ComputeTracer::gpu_error` returns the first one, `trace_gpu` returns it as
`RtError::Gpu` from then on, and the preview goes on with the CPU. Browsers report them a frame or two later. In the
browser the vendor and architecture of the adapter come from WebGPU's `GPUAdapterInfo` (`Context::wgpu_adapter_info`),
so Intel graphics of the 7th generation (architecture "gen-7") are recognised as natively.

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

## Denoising

```rust
let mut image = RenderImage::with_features(width, height); // or GpuRenderImage::with_features
tracer.trace(&scene, &camera, &mut image, &settings)?;
let smooth = AtrousDenoiser::default().denoise(&image)?;   // on the CPU, also for GPU images after read()
// On the GPU, without leaving it:
// gpu_image.write_denoised_to_three_d_texture(&texture, &AtrousDenoiser::default());
```

`AtrousDenoiser` is an edge avoiding à-trous wavelet filter like the spatial part of SVGF. With features the render
image also collects, per pixel, the albedo, normal and distance of what the camera sees first and the squared
luminance of the samples (32 more bytes per pixel). The filter blurs the light per surface (color divided by albedo)
in five passes of growing reach, only between pixels with similar normals and distances and only as far as the
variance says the differences are noise, then multiplies the albedo back so textures stay sharp. With more samples
the variance falls and the filter steps back by itself. `RaytracePreview` denoises by default
(`PreviewSettings::denoiser`). In the garden at 8 samples the difference to 512 samples drops from 6.3 to 3.6 (RMSE of
8 bit sRGB); denoising 960 × 600 takes about 0.7 s on four cores.

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

- Paths sample the sun disc, directional, point and area lights at every diffuse or glossy bounce, the sky by the
  material, and end by Russian roulette.
- Area lights (`RtLight::Rect` for panels and windows, `RtLight::Sphere` for bulbs, both with soft shadows) are sampled
  directly and also found by the material's own directions; both are weighted by multiple importance sampling (power
  heuristic), so small bright lights and glossy surfaces both stay calm. `RtLight::sphere_with_flux` and
  `rect_with_flux` take lumen. Area lights are seen but cast no shadows.
- The first 16 random numbers of every pixel sample come from an Owen scrambled Sobol sequence (Burley 2020), the rest
  from PCG. In the garden this lowers the noise at equal samples by about a quarter, so about 1.7 times fewer samples
  give the same image.
- Shadow rays pass through transmissive surfaces with their transmission and Fresnel term, so sunlight falls through
  windows into rooms. Light that reaches a surface through glass is counted once: the sun disc and area lights seen
  through glass after a diffuse bounce no longer add to what the shadow rays found (before, sunlight through a thin
  window was counted twice by the paths that happened to hit the sun disc).
- Caustics of solid glass (`thin_walled: false`) with `TraceSettings::caustic_photons`: every call shoots that many
  photons from the sun, directional and point lights, aimed at the bounding spheres of the solid glass, follows them
  through it like the camera paths and keeps them where they land. Points of the camera paths on diffuse or glossy
  surfaces gather the photons within `caustic_radius`, which shrinks with every call (progressive photon mapping),
  so focused light gets sharp and the image converges. Shadow rays of these lights then stop at solid glass, so the
  shadow of a glass ball is dark around its bright focus. Area lights and the sky reach surfaces behind glass by the
  paths themselves. The compute tracer does the same in compute passes per sample (shoot, count per cell of a hashed
  grid, find where cells start, sort) and gathers in the trace pass. `RaytracePreview` shoots 200 000 photons per
  sample on the GPU and 20 000 on the CPU (`PreviewSettings::gpu_caustic_photons`, `cpu_caustic_photons`). A glass
  face lying exactly on another surface sends its light below it; leave a millimetre between them.
- Not included: caustics of mirrors and of the light of area lights and the sky.
- Luminaires with measured light distributions: `IesProfile::parse` reads IES files (LM-63, type C photometry,
  `TILT=NONE`, all symmetries of the C-planes) into a table of 1° vertically and 5° horizontally.
  `RtLight::luminaire(position, down, front, Arc<IesProfile>)` places one like a point light with hard shadows,
  `luminaire_with_flux` scales it to a lumen value, `IesProfile::flux` integrates what it sends out. On CPU and GPU,
  also for caustic photons; the tables share the buffer of the lights on the GPU.
- Depth of field with `TraceSettings::depth_of_field` (also `PreviewSettings::depth_of_field`), on CPU and GPU:
  `DepthOfField { f_number, focus_distance }` behaves like a photo camera with the same field of view on a full frame
  sensor (24 mm high), so f/2.8 at 45° is a 29 mm lens with an aperture of 10 mm. Scenes have to be in meters.
  `DepthOfField::focused_on(&camera, point, f_number)` focuses on a point. Rays start on the aperture (concentric
  mapping of two Sobol dimensions) and meet at the focus plane; orthographic cameras stay sharp.
- Participating media (volumes), on CPU and GPU. `RtMedium { absorption, scattering, anisotropy }` per meter, with a
  Henyey-Greenstein phase function. `RtMaterial::medium` fills a closed mesh of solid glass (`thin_walled: false`):
  `RtMedium::absorbing(color, distance)` colors glass and water with depth as glTF's `KHR_materials_volume` does,
  scattering media make cloudy water. `RtMaterial::medium_boundary(medium)` is an invisible boundary for smoke and fog
  in a closed mesh; passing it costs no bounce and casts no caustics. `RtScene::set_fog(RtFog { medium, height,
  scale_height })` puts haze over the whole scene that thins out upwards, so the sky stays visible and sunlight
  arrives; `RtMedium::fog(half_distance, albedo, anisotropy)` sets it up from the distance at which half the light is
  left. Paths draw where they scatter from the extinction of a random color channel, sample the lights there with the
  phase function and go on in a direction drawn from it; shadow rays sum the lengths inside solids from their signed
  boundary hits, which needs no order of the hits. Purely absorbing media are applied without noise. Photons are
  attenuated along their way, the light they lose by scattering reaches surfaces by the paths. Media inside media:
  where closed meshes overlap, the one with the higher `RtMaterial::medium_priority` fills the space (glass over ice
  over water), the surfaces of the others are not seen and refraction goes by the index on both sides; paths keep up to
  four solids, shadow rays take the surfaces in their order (on the GPU up to eight per shadow ray, beyond that the
  signed sums; on WARP, the software adapter of DirectX 12, always the sums, because its compiler computes zeros with
  the ordered code, see `ComputeTracer::orders_shadow_crossings`). The camera may be inside a solid (smoke, under water): the solids containing it are found from a ray to
  infinity. A surface touching the solid a path just left (a pane of glass lying on the floor) is hit and not jumped
  over. Not included: media whose density varies other than the fog with height, nesting for the photons of caustics
  on the GPU. `examples/nested_media.rs` renders a glass tank of water with ice cubes and a pane on the floor. The compute tracer builds a second pipeline for
  scenes with media (the override constant `MEDIA` in trace.wgsl), so the tracer of scenes without stays as small as
  before for the compilers of small GPUs. On Intel graphics of the 7th generation (Ivy Bridge, Haswell, Bay Trail) that
  pipeline is too large for Mesa's compilers, so it is not built there: `ComputeTracer::media_problem` says why,
  `trace_gpu` returns `RtError::Gpu` for scenes with media and `RaytracePreview` goes on with the CPU. `examples/volumes.rs` renders a pergola in ground haze with a water basin
  and a ball of smoke.
- Deterministic: the same seed gives the same image with any number of threads.

## Tests

`cargo test -p three-d-raytrace` checks the hierarchy against testing every triangle, the sun position against solar
noon at the solstices and equinox, the material model for energy conservation, and the tracer against values known
from physics: a white sphere disappears in a uniform environment (white furnace), a white ground under the sky
reflects E/π, a point light falls off with the square of the distance, and a pane of glass lets 96 % of the light
through. `tests/caustics.rs` checks on CPU and GPU that photons through a glass slab light the floor as brightly as the straight
shadow rays and that a glass ball gathers the light of its shadow into a bright focus without making light.
`tests/ies.rs` checks on CPU and GPU that the floor under a luminaire gets the profile's intensity in the front
and side planes within 1 %. `tests/depth_of_field.rs` checks on CPU and GPU that an edge in the focus plane stays sharp and an edge behind it is
blurred by the thin lens's circle of confusion, without losing light. A reference image of the garden scene is compared in blocks of 4 × 4 pixels, write it again with
`THREE_D_WRITE_REFERENCE=1 cargo test --release -p three-d-raytrace`.

With the `wgpu` feature, `tests/gpu.rs` runs the same physical checks with the compute tracer and compares its garden
image with the CPU tracer's in blocks of 6 × 6 pixels; set `THREE_D_REQUIRE_WGPU=1` to fail instead of skipping without
an adapter.

The examples render the same scene: `cargo run --release -p three-d-raytrace --example garden -- 256 960 600` on the
CPU and `cargo run --release -p three-d-raytrace --features wgpu --example garden_gpu -- 256 960 600` with the compute
tracer.
`cargo run --release -p three-d-raytrace --example caustics -- 64 640 400` renders a glass ball and a glass block on a
terrace in the sun, without and with caustics and with depth of field (64 samples at 640 × 400 take 5 s and 23 s with 200 000 photons per
sample on four cores).
