//!
//! Path tracing for [three-d](https://github.com/asny/three-d) scenes.
//!
//! The crate is built from parts with their own interfaces, so that tracers for other hardware can replace each other
//! without changing the scene or how the result is shown:
//!
//! - [RtScene] holds meshes, materials, instances, lights and the environment. It is built from the same CPU data as the
//!   raster view ([CpuMesh], [CpuMaterial], [CpuModel]) and knows nothing about any backend.
//! - [AccelBuilder] builds the acceleration structure, here a two level bounding volume hierarchy on the CPU
//!   ([CpuBvhBuilder]): one hierarchy per mesh and one over the instances, so instanced vegetation stays small.
//! - [Tracer] adds samples to a [RenderImage]. The [CpuReferenceTracer] runs on any platform without a GPU and is the reference
//!   the GPU tracers of the wgpu backend are compared against. With the `wgpu` feature, `ComputeTracer` traces in compute
//!   shaders into a `GpuRenderImage` that stays on the GPU.
//! - [RenderImage] collects the samples and turns them into an image with a [ToneMapping].
//! - With the `three-d` feature, [RenderImage::to_three_d_texture] shows the render image with three-d on any backend. With the `wgpu`
//!   feature, [RaytraceSupport] tells what a context can run and [GpuDevice] hands its device to GPU tracers.
//! - [Sky] is a daylight sky with the sun at the position for a place, date and time ([GeoLocation], [LocalDateTime]).
//!
//! Units are photometric: luminance in cd/m², illuminance in lux and luminous intensity in cd. The exposure of the
//! [ToneMapping] is given as EV100 like on a camera, about 15 for a sunny day and 7 for a lit room.
//!

#![warn(missing_docs)]

mod accel;
mod bvh;
#[cfg(feature = "wgpu")]
mod compute;
mod denoise;
#[cfg(feature = "three-d")]
mod display;
mod error;
#[cfg(feature = "wgpu")]
mod gpu;
mod ies;
mod lens;
mod light;
mod medium;
mod photons;
#[cfg(feature = "three-d")]
mod preview;
#[cfg(all(feature = "three-d", not(target_arch = "wasm32")))]
mod preview_worker;
mod priority;
mod render_image;
mod sampling;
mod scene;
mod sky;
mod sun;
mod texture;
mod tracer;

pub use accel::*;
pub use bvh::{Bvh, BvhNode};
#[cfg(feature = "wgpu")]
pub use compute::*;
pub use denoise::*;
pub use error::*;
#[cfg(feature = "wgpu")]
pub use gpu::*;
pub use ies::IesProfile;
pub use lens::DepthOfField;
pub use medium::{RtFog, RtMedium};
#[cfg(feature = "three-d")]
pub use preview::*;
pub use render_image::*;
pub use scene::*;
pub use sky::*;
pub use sun::*;
pub use texture::*;
pub use tracer::*;

pub use three_d_asset::prelude::*;
pub use three_d_asset::{
    Camera, Model as CpuModel, PbrMaterial as CpuMaterial, Texture2D as CpuTexture, TextureData,
    TriMesh as CpuMesh, Viewport,
};
