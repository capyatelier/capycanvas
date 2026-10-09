use super::*;
use std::cell::RefCell;

thread_local! {
    static REPORT: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

#[wasm_bindgen]
pub fn configure_gpu_diagnostics(report: js_sys::Function) {
    REPORT.with(|slot| *slot.borrow_mut() = Some(report));
    std::panic::set_hook(Box::new(|info| {
        self::report("runtime", "panic", info);
        console_error_panic_hook::hook(info);
    }));
}

pub(super) fn report(role: &str, kind: &str, message: impl std::fmt::Display) {
    let message: String = message.to_string().chars().take(4096).collect();
    let event = serde_json::json!({"role": role, "kind": kind, "message": message});
    if let Ok(event) = serialize(&event) {
        REPORT.with(|slot| {
            if let Some(report) = slot.borrow().as_ref() { let _ = report.call1(&JsValue::NULL, &event); }
        });
    }
}

pub(super) fn adapter_info(adapter: &wgpu::Adapter, device: &wgpu::Device) -> serde_json::Value {
    let info = adapter.get_info();
    let browser = device.as_webgpu().and_then(|device|
        js_sys::Reflect::get(device.as_ref(), &js("adapterInfo")).ok())
        .filter(|info| !info.is_null() && !info.is_undefined()).map(|info| {
            let property = |name| js_sys::Reflect::get(&info, &js(name)).ok();
            serde_json::json!({"vendor": property("vendor").and_then(|v| v.as_string()),
                "architecture": property("architecture").and_then(|v| v.as_string()),
                "device": property("device").and_then(|v| v.as_string()),
                "description": property("description").and_then(|v| v.as_string()),
                "is_fallback_adapter": property("isFallbackAdapter").and_then(|v| v.as_bool())})
        });
    serde_json::json!({"name": info.name, "vendor": info.vendor, "device": info.device,
        "backend": format!("{:?}", info.backend), "device_type": format!("{:?}", info.device_type),
        "driver": info.driver, "driver_info": info.driver_info, "browser": browser})
}
