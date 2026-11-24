# three-d-raytrace-uniffi

Scenes for `three-d-raytrace` built and rendered from Kotlin (or any language uniffi generates bindings for). The
surface is small and flat on purpose; how a scene *reads* in Kotlin is the job of a DSL on top of the generated
bindings (`raytraceScene { box(4.4 x 0.02 x 3.4) of gravel at vec(0, 1.cm, 0) }`).

| Object | What it does |
| --- | --- |
| `Scene` | meshes, textures, materials, instances, lights, sky and haze by id, like `RtScene`; stays changeable |
| `UploadBytes` | bytes Kotlin writes into (`copyFrom`, `asFloatBuffer`, `asFloat32Array`, no uniffi copy): meshes, files, textures, many transforms; `resize` before each upload keeps the memory |
| `Scene::import_model` | OBJ with MTL, glTF, GLB with the files they refer to; parts are added, not placed |
| `Renderer` | the device of all views: wgpu where it can, else OpenGL (Linux) with the CPU tracer, else the CPU tracer alone |
| `FinalJob` | a final render in print size, `step()` per frame, PNG or RGBA as `OwnedBytes` (close them in Kotlin) |
| `PreviewJob` | the raytraced preview that gets sharper while nothing changes |
| `SceneView` | the realtime view: three-d's rasterizer, orbit/walk/fly, pick, hand-over to the raytraced preview when still |

`NativeBytes`, `OwnedBytes` and `UploadBytes` come from `cargo-uniffi-support`, the Rust side of the cargo-uniffi Gradle
plugin (a git dependency on its private repository; CI reads it with the secret `CARGO_UNIFFI_SUPPORT_TOKEN`). The plugin's
common API gives them `asByteBuffer()`, `asFloatBuffer()` (JVM, Android), `asUint8Array()`, `asFloat32Array()` (web) and
`copyFrom(…)`. On the web every new large upload grows the WebAssembly memory for good: reuse one `UploadBytes` per kind.

Objects that hold a context (`Renderer`, `FinalJob`, `PreviewJob`, `SceneView`) belong to the thread that created the
renderer; another thread gets `RtFfiError::WrongThread`. Use the plugin's `UniffiOwnerThread` in Kotlin.

The crate has the plugin's log bridge: `UniffiLog.install()` once at the start of the app, and the records of three-d,
wgpu and this crate go to SLF4J or `System.Logger` (JVM), Logcat (Android) or the console (web). The level comes from
`rustLog` in the plugin's settings or `-PcargoUniffi.rustLog=debug`. The app installs no Rust logger of its own.

In the browser `Renderer.fromCanvas(canvasId, preferGpu)` (async) takes the device of a canvas in the page: WebGPU on
the graphics card, WebGL2 otherwise. The canvas may stay hidden, the views hand their pixels to Kotlin. An application
crate with its own context hands it over with `Renderer::with_context` from Rust. `Renderer::headless` is for desktop and
Android.

The raytraced preview of a `SceneView` decides by itself between GPU and CPU and lowers the GPU's resolution when it
is slow. For tests, benchmarks and demonstrations `setPreviewOptions(PreviewOptions(tracer, gpuDownscale,
cpuDownscale, denoise, cpuThreads))` fixes it: `TracerChoice.GPU` never goes to the CPU, `gpuDownscale = 1u` keeps the full
resolution. `previewCapabilities()` says what the device can (GPU with the reason why not, ray query, timestamp
queries, the downscales, the logical cores for `cpuThreads`), to show impossible switches as disabled with the reason as a tooltip.

`SceneView.setOverlay(overlay)` puts egui over the view: an `EguiOverlay` of `three-d-egui-uniffi`, whose tree Kotlin
describes (see that crate). The view draws it over every frame, the rasterizer's and the raytraced preview's, before the
pixels are read, and hands it the events of `input` first: a click on a panel is egui's and leaves the camera alone (ask
`overlay.covers(x, y)` before picking), a drag that starts beside the panels turns the camera. Characters for text
fields go to `overlay.input(UiInput.Text(...))` directly. Hovering over the panel does not restart the raytraced preview.

An application that renders with three-d itself (a window, JOGL, a canvas, a framebuffer of a UI toolkit) can show a
`SceneView` inside its own frame instead of reading it back: make the renderer with `Renderer::with_context(context)` on
the application's context (OpenGL or wgpu, no device of its own) on its thread, and draw each frame with
`SceneView::render_into(&target, viewport)` from Rust or `renderToScreen(screenWidth, screenHeight, x, y, width,
height)` from Kotlin. Rasterizer, raytraced preview and egui overlay are drawn into that part of the target; the scene,
camera, input and options go on through the bindings. `Renderer.headless` with `nextFrame()` stays the way for photos,
tests and views without a host.

### In the application crate

A dependency on this crate is not enough: the linker drops everything the application's cdylib does not use, and with
it the uniffi exports, so the bindings of this crate (`three_d_raytrace_uniffi.api`) never appear. Re-export the crate
in the application's `lib.rs`:

```rust
uniffi::setup_scaffolding!();
pub use three_d_raytrace_uniffi;
```

```sh
cargo test -p three-d-raytrace-uniffi --release      # with VK_ICD_FILENAMES set to lavapipe in a container
```
