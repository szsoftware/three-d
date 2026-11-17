# The wgpu backend

This fork of `three-d` can render with [wgpu](https://wgpu.rs) (Vulkan, Metal, DirectX 12 and WebGPU) in addition to OpenGL/WebGL.
OpenGL/WebGL stays the default and behaves exactly like upstream `three-d`. Nothing changes for existing code, also not if the `wgpu` feature is enabled.

## Enable it

```toml
[dependencies]
three-d = { git = "https://github.com/szsoftware/three-d", features = ["wgpu"] }
```

Without the feature, wgpu is not compiled and the dependencies are the same as upstream.

## Choose the backend

| What you want | How |
| :-- | :-- |
| A window with OpenGL/WebGL (as before) | `Window::new(settings)` |
| A window with wgpu if possible, otherwise OpenGL | `Window::new_with_backend(settings, BackendPreference::Auto)` |
| A window with wgpu, or an error | `Window::new_with_backend(settings, BackendPreference::Wgpu)` |
| WebGPU in the browser with fallback to WebGL2 | `Window::new_async(settings, BackendPreference::Auto).await` |
| Your own winit window | `WindowedContext::from_winit_window_with_backend` or `from_winit_window_async` |
| Your own canvas (web) | `WindowedContext::from_canvas_async` |
| No window (tests, server) | `Context::new_headless_wgpu()` |
| Your own wgpu device | `Context::from_wgpu(device, queue)` |

`BackendPreference::Auto` takes wgpu if there is a hardware adapter and OpenGL/WebGL otherwise.
A browser cannot wait for WebGPU, so only `Window::new_async` can give you WebGPU there. `Window::new_with_backend` with `Auto` means WebGL2 in a browser.

`context.as_gl()` and `context.as_wgpu()` tell which backend a context uses and give access to the low-level API of that backend.

```rust
let window = Window::new_with_backend(WindowSettings::default(), BackendPreference::Auto)?;
let context = window.gl(); // the name is historic, the context can use wgpu
// From here on everything is the same for both backends
```

See the example `wgpu_window` (`cargo run --release --features wgpu --example wgpu_window`), which also runs in the browser.

## What works

Everything in the `core` and `renderer` modules, the window and the GUI:
buffers, all kinds of textures, mipmaps, render targets including multisampling, materials, lights, shadows, effects, deferred rendering, picking, text and `GUI` (egui).
Your own shaders (`Program::from_source`, `Material`, `Effect`, `Geometry`) stay in GLSL. The backend translates them, you do not have to change them.

The images are the same as with OpenGL within a few steps of 255 per channel. The repository tests this for the functionality of every example on software renderers for OpenGL and Vulkan.

## What is different with wgpu

- **Reading pixels in the browser.** WebGPU cannot wait for the GPU. `read_color`, `read_depth`, `pick` and `ray_intersect` panic with a message there.
  Use `RenderTarget::read_color_async`, `ColorTarget::read_async`, `pick_async` and `ray_intersect_async` instead. They work with every backend on every platform, so code that uses them runs everywhere. On the desktop the blocking functions work with wgpu, too.
- **Low-level OpenGL.** Dereferencing a `Context` to the `glow` context, the `context` module, `Program::use_raw_texture`, `RenderTarget::from_framebuffer` and `GUI::from_gl_context` need OpenGL and panic with a message on a wgpu context. Use `context.as_gl()` to check.
- **Multisampling** is off or 4 samples, since that is all WebGPU guarantees. Any other number above 1 means 4.
- **Depth and stencil of the screen.** `SurfaceSettings::depth_buffer` is off (0) or on, there is no stencil buffer.
- **GUI.** egui paint callbacks are not supported, since they draw with the API of a specific backend.
- **Float textures** with 32 bits are filtered linearly only if the adapter supports it (`FLOAT32_FILTERABLE`), otherwise with the nearest texel.
- **Errors.** For a context created by `three-d`, validation errors are collected and returned by `Context::error_check` (at most 64 between two checks). For `Context::from_wgpu` they go to the error handler of your device.
- **Custom GLSL.** The translation handles what the shaders of `three-d` and its examples use. A construct it does not know gives a `CoreError::ShaderCompilation` that names it, when the program is created. Known limits: no arrays of samplers, no arrays as inputs or outputs of a shader stage, and a function with a sampler parameter can only be called with the name of a sampler. Shaders are written without a version line, as for OpenGL.
- **Not there yet:** rendering into a texture view of another wgpu application, shaders written in WGSL, compute shaders.

## How it works

- All graphics calls of `core` go through one internal interface (`src/core/backend.rs`) with an implementation for OpenGL (`backend/gl.rs`, the upstream code) and one for wgpu (`backend/wgpu.rs`).
- Shaders are translated from the GLSL dialect of `three-d` to GLSL 4.50 and then to a naga module (`backend/wgpu/glsl.rs`). In the browser the module is handed over as WGSL.
- The wgpu backend keeps the conventions of OpenGL (y up, depth from -1 to 1 in clip space, first texture row at the bottom), so viewports, scissor boxes, `gl_FragCoord` and UV coordinates mean the same with both backends.
- Draw calls on the same render target are collected into one render pass and all passes of a frame into one command buffer.
- The screen is a set of textures like every other render target. Presenting draws it to the surface of the window.

## Tested where

- Automatically on every pull request: Linux with software renderers (lavapipe for Vulkan, llvmpipe for OpenGL), image comparison of OpenGL against wgpu; build on Windows, macOS and for the web; API compatibility with upstream (`cargo semver-checks`).
- By hand: a real window on Linux (X11), Chromium with WebGPU and the fallback to WebGL2.
- Not tested yet: real GPUs, Metal, DirectX 12, Safari, Android.
