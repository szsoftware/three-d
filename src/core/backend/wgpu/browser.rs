//!
//! What wgpu does not pass on from the browser's WebGPU: the `GPUDevice` itself, for calls wgpu does not have such as
//! `createComputePipelineAsync`, and the adapter's vendor and architecture, which tell for example Intel graphics of
//! the 7th generation apart.
//!

use js_sys::{Function, Reflect};
use wasm_bindgen::JsValue;

/// The browser's device and the adapter's info, as JavaScript objects.
pub(crate) struct BrowserDevice {
    pub device: JsValue,
    pub info: JsValue,
}

impl BrowserDevice {
    /// A string of the adapter's info, empty if the browser does not tell.
    pub fn info(&self, key: &str) -> String {
        Reflect::get(&self.info, &JsValue::from_str(key))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default()
    }
}

///
/// Catches the device the next `GPUAdapter.requestDevice` resolves to, by wrapping that method until
/// [DeviceCapture::finish] or until it is dropped. wgpu calls the method of the prototype, so it goes through the
/// wrapper.
///
pub(crate) struct DeviceCapture {
    sink: JsValue,
}

const INSTALL: &str = "
    const proto = globalThis.GPUAdapter && globalThis.GPUAdapter.prototype;
    if (!proto || typeof proto.requestDevice !== 'function') return null;
    const original = proto.requestDevice;
    const sink = { original, device: null, info: null };
    proto.requestDevice = function (descriptor) {
        const adapter = this;
        return original.call(this, descriptor).then(device => {
            sink.device = device;
            sink.info = adapter.info || device.adapterInfo || null;
            return device;
        });
    };
    return sink;
";

const RESTORE: &str = "
    if (sink.original) { globalThis.GPUAdapter.prototype.requestDevice = sink.original; sink.original = null; }
";

impl DeviceCapture {
    /// Starts catching, `None` without WebGPU.
    pub fn install() -> Option<Self> {
        let sink = Function::new_no_args(INSTALL).call0(&JsValue::NULL).ok()?;
        sink.is_object().then_some(Self { sink })
    }

    /// The device caught since [DeviceCapture::install], if any.
    pub fn finish(self) -> Option<BrowserDevice> {
        let get = |key: &str| Reflect::get(&self.sink, &JsValue::from_str(key)).ok();
        let device = get("device").filter(|d| d.is_object())?;
        let info = get("info")
            .filter(|i| i.is_object())
            .or_else(|| Reflect::get(&device, &JsValue::from_str("adapterInfo")).ok())
            .unwrap_or(JsValue::NULL);
        Some(BrowserDevice { device, info })
    }
}

impl Drop for DeviceCapture {
    fn drop(&mut self) {
        let _ = Function::new_with_args("sink", RESTORE).call1(&JsValue::NULL, &self.sink);
    }
}
