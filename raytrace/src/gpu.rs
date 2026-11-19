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
/// Why the compute tracer can not trace participating media (`RtMedium`, `RtFog`) on a device, `None` if it can. Its
/// shader for scenes with media needs more scratch memory than Intel graphics of the 7th generation (Ivy Bridge,
/// Haswell, Bay Trail) have and jumps further than they can; Mesa's compilers for them (crocus, hasvk) crash or hang on
/// it, so it is not even built there and such scenes are traced on the CPU.
///
pub fn media_problem(info: &three_d::wgpu::AdapterInfo) -> Option<String> {
    is_intel_gen7(info.vendor, &info.name, &info.driver, &info.driver_info).then(|| {
        format!(
            "The graphics card ({}) can not trace fog, smoke and tinted glass, its shader compiler fails on them; \
             ray tracing of scenes with them runs on the CPU.",
            info.name
        )
    })
}

/// Intel graphics of the 7th generation, by what the adapter tells.
fn is_intel_gen7(vendor: u32, name: &str, driver: &str, driver_info: &str) -> bool {
    let text = format!("{name} {driver} {driver_info}").to_lowercase();
    let intel = vendor == 0x8086 || text.contains("intel");
    // Mesa's drivers for exactly these chips
    let driver = text.contains("crocus") || text.contains("hasvk");
    let chip = [
        "ivb",
        "hsw",
        "byt",
        "ivybridge",
        "ivy bridge",
        "haswell",
        "bay trail",
    ]
    .iter()
    .any(|k| text.contains(k));
    driver || (intel && chip)
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
    fn intel_gen7_is_recognised() {
        assert!(is_intel_gen7(
            0x8086,
            "Intel(R) HD Graphics 4000 (IVB GT2)",
            "Intel open-source Mesa driver",
            "Mesa 26.0.8"
        ));
        assert!(is_intel_gen7(
            0x8086,
            "Mesa Intel(R) HD Graphics 4600 (HSW GT2)",
            "",
            ""
        ));
        assert!(is_intel_gen7(
            0,
            "crocus",
            "OpenGL",
            "4.6 (Core Profile) Mesa"
        ));
        assert!(!is_intel_gen7(
            0x8086,
            "Intel(R) UHD Graphics 620 (KBL GT2)",
            "Intel open-source Mesa driver",
            "Mesa 26.0.8"
        ));
        assert!(!is_intel_gen7(
            0x10de,
            "NVIDIA GeForce RTX 4070",
            "NVIDIA",
            "560"
        ));
    }
}
