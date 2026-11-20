# three-d-raytrace-uniffi

Scenes for `three-d-raytrace` built and rendered from Kotlin (or any language uniffi generates bindings for). The
surface is small and flat on purpose; how a scene *reads* in Kotlin is the job of a DSL on top of the generated
bindings (`raytraceScene { box(4.4 x 0.02 x 3.4) of gravel at vec(0, 1.cm, 0) }`).

| Object | What it does |
| --- | --- |
| `Scene` | meshes, textures, materials, instances, lights, sky and haze by id, like `RtScene`; stays changeable |
| `Upload`, `MeshUpload` | bytes of a fixed size that Kotlin writes into through `NativeBytes` (no uniffi copy): meshes, files, textures, many transforms |
| `Scene::import_model` | OBJ with MTL, glTF, GLB with the files they refer to; parts are added, not placed |
| `Renderer` | the device of all views: wgpu where it can, else OpenGL (Linux) with the CPU tracer, else the CPU tracer alone |
| `FinalJob` | a final render in print size, `step()` per frame, PNG or RGBA as `NativeBytes` |
| `PreviewJob` | the raytraced preview that gets sharper while nothing changes |
| `SceneView` | the realtime view: three-d's rasterizer, orbit/walk/fly, pick, hand-over to the raytraced preview when still |

`NativeBytes { address, length }` is the record the cargo-uniffi Gradle plugin turns into `asByteBuffer()` (JVM,
Android) and `asUint8Array()` (web).

Objects that hold a context (`Renderer`, `FinalJob`, `PreviewJob`, `SceneView`) belong to the thread that created the
renderer; another thread gets `RtFfiError::WrongThread`. Use a dispatcher with one thread in Kotlin.

In the browser the application creates the context from its canvas and hands it over with `Renderer::with_context`
from its own Rust crate; `Renderer::headless` is for desktop and Android.

```sh
cargo test -p three-d-raytrace-uniffi --release      # with VK_ICD_FILENAMES set to lavapipe in a container
```
