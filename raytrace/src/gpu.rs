//!
//! What a context of three-d offers to tracers that run on the GPU.
//!

use three_d::Context;

///
/// The fastest kind of tracer a three-d context can run.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RaytraceSupport {
    /// Only the CPU: the OpenGL backend, or wgpu without compute shaders (WebGL2 in the browser).
    Cpu,
    /// Compute shaders with storage buffers on the wgpu backend: WebGPU in the browser, Vulkan, Metal, DirectX 12.
    Compute,
    /// Ray queries on hardware that traces rays. The device must have been created with
    /// [wgpu::Features::EXPERIMENTAL_RAY_QUERY](three_d::wgpu::Features::EXPERIMENTAL_RAY_QUERY), which three-d's own
    /// constructors do not ask for yet.
    HardwareRayQuery,
}

/// Storage buffers the compute tracer binds at once: nodes, triangles, vertices, instances, materials, lights and sums.
const STORAGE_BUFFERS_NEEDED: u32 = 7;

impl RaytraceSupport {
    ///
    /// What the context can do. Applications fall back to the raster view or the [CpuReferenceTracer](crate::CpuReferenceTracer)
    /// when the GPU tracers are not available.
    ///
    pub fn of(context: &Context) -> Self {
        let Some((device, _)) = context.as_wgpu() else {
            return Self::Cpu;
        };
        let limits = device.limits();
        // Without compute shaders (WebGL2, OpenGL before 4.3) the compute limits are zero
        if limits.max_compute_workgroups_per_dimension == 0
            || limits.max_compute_invocations_per_workgroup < 64
            || limits.max_storage_buffers_per_shader_stage < STORAGE_BUFFERS_NEEDED
        {
            return Self::Cpu;
        }
        if device
            .features()
            .contains(three_d::wgpu::Features::EXPERIMENTAL_RAY_QUERY)
        {
            Self::HardwareRayQuery
        } else {
            Self::Compute
        }
    }
}

///
/// The wgpu device and queue of a three-d context, which a GPU tracer uses next to three-d.
///
/// Textures of three-d are shared with [Texture2D::as_wgpu](three_d::Texture2D::as_wgpu), which also orders the work of
/// three-d and the tracer on the queue.
///
#[derive(Clone, Debug)]
pub struct GpuDevice {
    /// The device.
    pub device: three_d::wgpu::Device,
    /// The queue.
    pub queue: three_d::wgpu::Queue,
}

impl GpuDevice {
    /// The device of a context with the wgpu backend, `None` with another backend.
    pub fn from_context(context: &Context) -> Option<Self> {
        context.as_wgpu().map(|(device, queue)| Self {
            device: device.clone(),
            queue: queue.clone(),
        })
    }
}
