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

/// Storage buffers the compute tracer binds at once: nodes, triangles, vertices, instances, materials, lights, sums and
/// textures. WebGPU guarantees 8.
const STORAGE_BUFFERS_NEEDED: u32 = 8;

impl RaytraceSupport {
    ///
    /// What the context can do. Applications fall back to the raster view or the [CpuReferenceTracer](crate::CpuReferenceTracer)
    /// when the GPU tracers are not available.
    ///
    pub fn of(context: &Context) -> Self {
        let Some((device, _)) = context.as_wgpu() else {
            return Self::Cpu;
        };
        if driver_problem(context).is_some() {
            return Self::Cpu;
        }
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
/// Why the GPU tracers must not run on the device of the context, though it offers compute shaders: drivers known to
/// crash when they compile or run the tracer. `None` for every other device and for contexts without wgpu.
///
/// No driver is known to crash at the moment. Intel graphics of the 7th generation (Ivy Bridge, Haswell, Bay Trail, for
/// example HD Graphics 4000) crashed in Mesa's shader compiler for these chips (crocus and hasvk) until the tracer
/// shader was made smaller: it needed more scratch memory than Ivy Bridge has and jumps further than Haswell can.
///
pub fn driver_problem(context: &Context) -> Option<String> {
    let (device, _) = context.as_wgpu()?;
    let info = device.adapter_info();
    crashes_with_the_tracer(info.vendor, &info.name, &info.driver, &info.driver_info).then(|| {
        format!(
            "The driver of this graphics card ({}) crashes with the ray tracer; ray tracing runs on the CPU.",
            info.name
        )
    })
}

/// Devices whose drivers crash with the tracer, by what the adapter tells. None at the moment.
fn crashes_with_the_tracer(_vendor: u32, _name: &str, _driver: &str, _driver_info: &str) -> bool {
    false
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
