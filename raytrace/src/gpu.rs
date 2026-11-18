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
/// crash when they compile the tracer. `None` for every other device and for contexts without wgpu.
///
/// Intel graphics of the 7th generation (Ivy Bridge, Haswell, Bay Trail, for example HD Graphics 4000) crash in Mesa's
/// shader compiler for these chips, through OpenGL (crocus) as well as Vulkan (hasvk).
///
pub fn driver_problem(context: &Context) -> Option<String> {
    let (device, _) = context.as_wgpu()?;
    let info = device.adapter_info();
    crashes_compiling_the_tracer(info.vendor, &info.name, &info.driver, &info.driver_info).then(|| {
        format!(
            "The driver of this graphics card ({}) crashes when it compiles the ray tracer; ray tracing runs on the \
             CPU.",
            info.name
        )
    })
}

/// Intel graphics of the 7th generation or its Mesa drivers, by what the adapter tells.
fn crashes_compiling_the_tracer(vendor: u32, name: &str, driver: &str, driver_info: &str) -> bool {
    let text = format!("{name} {driver} {driver_info}").to_lowercase();
    let intel = vendor == 0x8086 || text.contains("intel");
    let gen7 = [
        "ivb",
        "hsw",
        "byt",
        "ivybridge",
        "ivy bridge",
        "haswell",
        "bay trail",
    ]
    .iter()
    .any(|t| text.contains(t));
    let gen7_driver = text.contains("crocus") || text.contains("hasvk");
    intel && (gen7 || gen7_driver)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen7_intel_is_recognized() {
        assert!(crashes_compiling_the_tracer(
            0x8086,
            "Intel(R) HD Graphics 4000 (IVB GT2)",
            "Intel open-source Mesa driver",
            "Mesa 26.0.8"
        ));
        assert!(crashes_compiling_the_tracer(
            0,
            "Mesa Intel(R) HD Graphics 4000 (IVB GT2)",
            "",
            "4.2 (Core Profile) Mesa 26.0.8"
        ));
        assert!(crashes_compiling_the_tracer(
            0x8086, "anything", "crocus", ""
        ));
        assert!(!crashes_compiling_the_tracer(
            0x8086,
            "Intel(R) UHD Graphics 620 (KBL GT2)",
            "Intel open-source Mesa driver",
            "Mesa 26.0.8"
        ));
        assert!(!crashes_compiling_the_tracer(
            0x10005,
            "llvmpipe (LLVM 21.1.8, 256 bits)",
            "llvmpipe",
            "Mesa 26.0.8"
        ));
    }
}
