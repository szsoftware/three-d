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

///
/// The power preference with which the browser offers a WebGPU adapter on the graphics card, `None` if it offers only
/// software ones (SwiftShader, llvmpipe) or no WebGPU at all. The adapter without a preference comes first; browsers
/// that know several adapters can give a software one there (Chrome with `--enable-unsafe-webgpu` on a laptop whose
/// graphics card Chrome does not list as safe), and one on the graphics card for "low-power" or "high-performance".
/// wgpu does not pass on whether an adapter is a fallback, so the browser is asked directly.
///
pub(crate) async fn hardware_power_preference() -> Option<wgpu::PowerPreference> {
    for preference in [
        wgpu::PowerPreference::None,
        wgpu::PowerPreference::LowPower,
        wgpu::PowerPreference::HighPerformance,
    ] {
        match adapter_is_software(preference).await {
            Some(false) => return Some(preference),
            Some(true) => {}
            None => return None,
        }
    }
    None
}

/// Whether the adapter the browser gives for the preference is a software one, `None` if it gives none.
async fn adapter_is_software(preference: wgpu::PowerPreference) -> Option<bool> {
    use wasm_bindgen::JsCast;
    let get = |object: &JsValue, key: &str| Reflect::get(object, &JsValue::from_str(key)).ok();
    let navigator = get(&js_sys::global(), "navigator")?;
    let gpu = get(&navigator, "gpu").filter(|g| g.is_object())?;
    let request = get(&gpu, "requestAdapter").and_then(|f| f.dyn_into::<Function>().ok())?;
    let options = js_sys::Object::new();
    let name = match preference {
        wgpu::PowerPreference::LowPower => Some("low-power"),
        wgpu::PowerPreference::HighPerformance => Some("high-performance"),
        wgpu::PowerPreference::None => None,
    };
    if let Some(name) = name {
        Reflect::set(
            &options,
            &JsValue::from_str("powerPreference"),
            &JsValue::from_str(name),
        )
        .ok()?;
    }
    let promise = request
        .call1(&gpu, &options)
        .ok()?
        .dyn_into::<js_sys::Promise>()
        .ok()?;
    let adapter = wasm_bindgen_futures::JsFuture::from(promise).await.ok()?;
    if !adapter.is_object() {
        return None;
    }
    let info = get(&adapter, "info").filter(|i| i.is_object());
    let flag = |object: Option<&JsValue>| {
        object
            .and_then(|o| get(o, "isFallbackAdapter"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    let text = |key: &str| {
        info.as_ref()
            .and_then(|i| get(i, key))
            .and_then(|a| a.as_string())
            .unwrap_or_default()
            .to_lowercase()
    };
    let names = format!(
        "{} {} {}",
        text("vendor"),
        text("architecture"),
        text("description")
    );
    Some(
        flag(info.as_ref())
            || flag(Some(&adapter))
            || ["swiftshader", "llvmpipe", "lavapipe", "softpipe"]
                .iter()
                .any(|software| names.contains(software)),
    )
}
