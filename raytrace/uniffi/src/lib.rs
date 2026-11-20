//!
//! Scenes for [three-d-raytrace](three_d_raytrace) built and rendered from Kotlin, or any other language uniffi
//! generates bindings for. The surface is small and flat on purpose: a [Scene] that hands out ids like
//! [RtScene](three_d_raytrace::RtScene), records for materials, lights, the sky and the camera, buffers that Kotlin
//! writes into without a copy ([Upload], [MeshUpload]) and a [Renderer] for final renders, a raytraced preview and a
//! realtime view. How a scene reads in Kotlin is the business of a DSL on top of the generated bindings.
//!
//! Large data never goes through uniffi's serialization: [Upload] and [MeshUpload] allocate their bytes once and hand
//! out [NativeBytes], which the cargo-uniffi Gradle plugin turns into a `ByteBuffer` (JVM, Android) or a view of the
//! WebAssembly memory (web) that Kotlin writes into.
//!
//! Objects that hold a three-d context ([Renderer], [FinalJob], [PreviewJob], [SceneView]) belong to the thread that
//! created the renderer: a call from another thread returns [RtFfiError::WrongThread]. In Kotlin they run on a
//! dispatcher with one thread; in the browser there is only one.
//!

uniffi::setup_scaffolding!();

mod bytes;
mod error;
mod import;
mod render;
mod scene;
mod thread;
mod values;
mod view;

pub use bytes::*;
pub use error::*;
pub use import::*;
pub use render::*;
pub use scene::*;
pub use values::*;
pub use view::*;
