//!
//! Compiling the tracer in a browser without stopping the page: WebGPU's `createComputePipelineAsync` compiles in the
//! background, wgpu has only the synchronous `createComputePipeline`, which on slow drivers holds the page for seconds.
//! The browser keeps pipelines it made in a cache by their descriptors, so the pipelines of the same shader, layouts
//! and constants that wgpu asks for afterwards are found there while the compiled ones are alive.
//!

use super::{Slot, MEDIA_CONSTANTS, PHOTON_ENTRIES, PHOTON_SLOTS, TRACE_SLOTS};
use js_sys::{Array, Function, Promise};
use wasm_bindgen::{JsCast, JsValue};

const COMPILE: &str = "
    const layoutOf = spec => device.createPipelineLayout({
        bindGroupLayouts: [device.createBindGroupLayout({
            entries: spec.map(([binding, type, hasDynamicOffset, minBindingSize]) => {
                const buffer = { type, hasDynamicOffset };
                if (minBindingSize > 0) buffer.minBindingSize = minBindingSize;
                return { binding, visibility: GPUShaderStage.COMPUTE, buffer };
            }),
        })],
    });
    const module = device.createShaderModule({ code });
    const traceLayout = layoutOf(trace);
    const photonLayout = layoutOf(photons);
    const stage = (entryPoint, constants) => constants ? { module, entryPoint, constants } : { module, entryPoint };
    const jobs = [device.createComputePipelineAsync({ layout: traceLayout, compute: stage('trace') })];
    if (media) jobs.push(device.createComputePipelineAsync({ layout: traceLayout, compute: stage('trace', media) }));
    for (const entry of entries) jobs.push(device.createComputePipelineAsync({ layout: photonLayout, compute: stage(entry) }));
    return Promise.all(jobs);
";

/// The bindings as WebGPU's layout entries are made from them: binding, type, dynamic offset, minimal size.
fn spec(slots: &[(u32, Slot)]) -> Array {
    slots
        .iter()
        .map(|&(binding, slot)| {
            let (ty, dynamic, min_size) = match slot {
                Slot::Uniform { dynamic, min_size } => ("uniform", dynamic, min_size),
                Slot::Storage { read_only: true } => ("read-only-storage", false, 0),
                Slot::Storage { read_only: false } => ("storage", false, 0),
            };
            Array::of4(
                &JsValue::from(binding),
                &JsValue::from_str(ty),
                &JsValue::from_bool(dynamic),
                &JsValue::from_f64(min_size as f64),
            )
        })
        .collect()
}

///
/// Compiles the pipelines of the tracer on the browser's device, with the one for participating media if `media`.
/// Resolves to the pipelines, which have to be kept until wgpu made its own, or to the browser's error.
///
pub(super) async fn compile(device: &JsValue, media: bool) -> Result<JsValue, JsValue> {
    let function = Function::new_with_args("device, code, trace, photons, entries, media", COMPILE);
    let media = if media {
        let constants = js_sys::Object::new();
        for (name, value) in MEDIA_CONSTANTS {
            js_sys::Reflect::set(
                &constants,
                &JsValue::from_str(name),
                &JsValue::from_f64(value),
            )?;
        }
        constants.into()
    } else {
        JsValue::NULL
    };
    let entries: Array = PHOTON_ENTRIES
        .iter()
        .map(|e| JsValue::from_str(e))
        .collect();
    let arguments: Array = [
        device.clone(),
        JsValue::from_str(include_str!("trace.wgsl")),
        spec(&TRACE_SLOTS).into(),
        spec(&PHOTON_SLOTS).into(),
        entries.into(),
        media,
    ]
    .into_iter()
    .collect();
    let promise: Promise = function.apply(&JsValue::NULL, &arguments)?.dyn_into()?;
    wasm_bindgen_futures::JsFuture::from(promise).await
}
