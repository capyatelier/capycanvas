//! Apple host ABI. The same library serves UIKit and AppKit. Rust owns the
//! shared session; Swift owns UI and serial execution. No callbacks into Swift.
mod metal;
mod document_tabs;
pub use document_tabs::*;
mod session;
pub use session::*;
mod storage;
pub use storage::*;

/// SDR viewing contract shared by canvas, UI values and image transports.
/// Core Animation/ColorSync maps tagged P3 to the current screen, including sRGB.
const DISPLAY_SPACE: layer_core::color::RgbSpace = layer_core::color::RgbSpace::DisplayP3;
mod project;
pub use project::*;
mod previews;
mod scopes;
pub use previews::*;
pub use scopes::*;
mod workspaces;
mod stroke_recording;
pub use stroke_recording::*;
#[cfg(test)]
mod tests;
use layer_host::{NativeHost, PointerBatch};
use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn panic_diagnostic(payload: Box<dyn std::any::Any + Send>) -> String {
    let detail = payload.downcast_ref::<String>().map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied()).unwrap_or("Unknown panic payload");
    format!("Native operation panicked: {detail}")
}

pub struct CapyApple {
    metal: metal::MetalHost,
    host: NativeHost,
    window: document_tabs::Window,
    session_disk: Option<std::sync::Arc<std::sync::Mutex<session::WindowDisk>>>,
    session_capture_sequence: u64,
    error: Option<CString>,
    chrome_facts: layer_ui::ChromeFacts,
    dismissed_contacts: std::collections::BTreeSet<u64>,
    workspaces: Option<layer_workspace::WorkspaceController<layer_workspace::StoreWorker>>,
    language: layer_ui::LanguageTransition,
    published_language: Option<u64>,
    scopes: scopes::Cache,
}
impl CapyApple {
    fn perform<T>(&mut self, work: impl FnOnce(&mut Self) -> Result<T, String>) -> Option<T> {
        self.error = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.metal.observe_failure(&mut self.host, false);
            work(self)
        })).unwrap_or_else(|payload| Err(panic_diagnostic(payload)));
        match result {
            Ok(value) => Some(value),
            Err(message) => {
                self.error = CString::new(message.replace('\0', " ")).ok();
                None
            }
        }
    }
    fn gpu_operation<T>(&mut self, work: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        let result = catch_unwind(AssertUnwindSafe(|| work(self)))
            .unwrap_or_else(|payload| Err(panic_diagnostic(payload)));
        if let Err(message) = &result {
            self.metal.stop(&mut self.host, message.clone());
            self.dismissed_contacts.clear();
        }
        result
    }
}
#[cfg(test)]
fn fixture_localization() -> std::sync::Arc<layer_ui::Localizer> {
    layer_ui::Localizer::shared(layer_ui::UiLanguage::English)
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_create(platform: u32) -> *mut CapyApple {
    apple_launch_localized(platform, "", fixture_localization()).unwrap_or(std::ptr::null_mut())
}

#[derive(serde::Deserialize)]
struct ControlRequest<T> {
    language: layer_ui::UiLanguage,
    request: T,
}
fn control_request<T: serde::de::DeserializeOwned>(source: &str) -> Result<(std::sync::Arc<layer_ui::Localizer>, T), String> {
    let request: ControlRequest<T> = serde_json::from_str(source).map_err(|e| e.to_string())?;
    let localization = layer_ui::Localizer::prepared(request.language).ok_or("apple_language_not_prepared")?;
    Ok((localization, request.request))
}
#[derive(serde::Deserialize)]
struct AppleLaunch {
    saved: String,
    preferred_languages: Vec<String>,
}
/// # Safety
/// JSON must be valid NUL-terminated UTF-8 for this call. A non-null bootstrap
/// output and error must be writable and their results released with capy_apple_string_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_launch(platform: u32, json: *const c_char, bootstrap: *mut *mut c_char, error: *mut *mut c_char) -> *mut CapyApple {
    if !bootstrap.is_null() { unsafe { *bootstrap = std::ptr::null_mut() }; }
    if !error.is_null() { unsafe { *error = std::ptr::null_mut() }; }
    let result = catch_unwind(|| {
        if json.is_null() { return Err("Missing launch configuration".into()); }
        let source = (unsafe { CStr::from_ptr(json) }).to_str().map_err(|e| e.to_string())?;
        let launch = serde_json::from_str::<AppleLaunch>(source).map_err(|e| e.to_string())?;
        let tags: Vec<&str> = launch.preferred_languages.iter().map(String::as_str).collect();
        let localization = layer_ui::launch_localization(&launch.saved, &tags);
        layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        if !bootstrap.is_null() {
            let view = serde_json::to_string(&layer_ui::bootstrap_view(&localization)).map_err(|e| e.to_string())?;
            let view = CString::new(view).map_err(|e| e.to_string())?;
            unsafe { *bootstrap = view.into_raw() };
        }
        apple_launch_localized(platform, &launch.saved, localization)
    }).unwrap_or_else(|payload| Err(panic_diagnostic(payload)));
    match result {
        Ok(app) => app,
        Err(message) => {
            if !error.is_null() {
                unsafe { *error = CString::new(format!("Session launch: {message}").replace('\0', " ")).unwrap().into_raw() };
            }
            std::ptr::null_mut()
        }
    }
}
/// Dispatch workers have small stacks; debug builds of the shared session exceed them.
pub(crate) fn on_large_stack<T: Send>(name: &str, work: impl FnOnce() -> T + Send) -> Result<T, String> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name(name.into())
            .stack_size(8 * 1024 * 1024)
            .spawn_scoped(scope, work)
            .map_err(|e| e.to_string())?
            .join()
            .map_err(panic_diagnostic)
    })
}
fn apple_launch_localized(platform: u32, saved: &str, localization: std::sync::Arc<layer_ui::Localizer>) -> Result<*mut CapyApple, String> {
    on_large_stack("capy-launch", move || apple_launch_on_stack(platform, saved, localization).map(|app| app as usize))?
        .map(|app| app as *mut CapyApple)
}
fn apple_launch_on_stack(platform: u32, saved: &str, localization: std::sync::Arc<layer_ui::Localizer>) -> Result<*mut CapyApple, String> {
    let platform = match platform {
        0 => layer_ui::Platform::Ios,
        1 => layer_ui::Platform::Mac,
        _ => return Err(format!("Unknown Apple platform {platform}")),
    };
    let mut host = NativeHost::launch_localized(platform, saved, localization)?;
    host.ui_color = layer_host::UiColor::Tagged(DISPLAY_SPACE);
    host.dispatch(layer_ui::UiAction::RestoreWorkspace {
        workspace: Box::new(layer_ui::WorkspaceState::for_platform(platform)),
    })?;
    host.session.set_document_replacement(false);
    let window = document_tabs::Window::localized(host.session.localization());
    Ok(Box::into_raw(Box::new(CapyApple {
        language: layer_ui::LanguageTransition::new(host.session.localization().clone()),
        published_language: None,
        scopes: Default::default(),
        host,
        window,
        session_disk: None,
        session_capture_sequence: 0,
        metal: metal::MetalHost::default(),
        error: None,
        chrome_facts: Default::default(),
        dismissed_contacts: Default::default(),
        workspaces: None,
    })))
}
pub struct CapyLanguageTask {
    request: layer_ui::LanguageRequest,
    prepared: Option<std::sync::Arc<layer_ui::Localizer>>,
}
/// # Safety
/// Serial owner only. JSON is an ordered array of preferred system-language tags.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_language_request(app: *mut CapyApple, json: *const c_char) -> *mut CapyLanguageTask {
    let Some(app) = (unsafe { app.as_mut() }) else { return std::ptr::null_mut(); };
    app.perform(|a| {
        if json.is_null() { return Err("Missing preferred languages".into()); }
        let tags: Vec<String> = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
        Ok(a.language.request(a.host.session.state().settings.language, &tags)
            .map(|request| Box::into_raw(Box::new(CapyLanguageTask { request, prepared: None }))))
    }).flatten().unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// Preparation worker only; task has no concurrent caller or borrowed editor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_language_prepare(task: *mut CapyLanguageTask) {
    if let Some(task) = unsafe { task.as_mut() } {
        layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        task.prepared = Some(layer_ui::Localizer::shared(task.request.language));
    }
}
/// # Safety
/// Serial owner only after worker preparation; task remains owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_language_prepared(app: *mut CapyApple, task: *mut CapyLanguageTask) -> bool {
    let (Some(app), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_mut() }) else { return false; };
    task.prepared.take().is_some_and(|localization| app.language.prepared(task.request, localization))
}
/// # Safety
/// Serial owner only. Native input capture/composition is supplied by the UI owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_language_publish(app: *mut CapyApple, input_busy: bool) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else { return -1; };
    app.perform(|a| {
        let busy = input_busy || a.host.session.localization_input_busy() || a.chrome_facts.held || a.chrome_facts.dragging;
        let Some(localization) = a.language.publish(busy) else { return Ok(if a.language.pending() { 0 } else { -1 }); };
        a.window.set_localization(localization.clone());
        if let Some(workspaces) = &mut a.workspaces { workspaces.set_localization(localization.clone()); }
        Ok(i32::from(a.host.set_localization(localization)))
    }).unwrap_or(-1)
}
/// # Safety
/// Task must be released exactly once, after every preparation/owner call ends.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_language_free(task: *mut CapyLanguageTask) {
    if !task.is_null() { unsafe { drop(Box::from_raw(task)); } }
}
/// # Safety
/// Handle must come from create, with no outstanding calls or borrowed strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_destroy(app: *mut CapyApple) {
    if !app.is_null() {
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}
/// # Safety
/// Handle must remain alive throughout this call and use of the borrowed result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_error(app: *const CapyApple) -> *const c_char {
    unsafe { app.as_ref() }
        .and_then(|a| a.error.as_ref())
        .map_or(std::ptr::null(), |e| e.as_ptr())
}
/// # Safety
/// Text must be an unreleased owned result returned by this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_string_free(text: *mut c_char) {
    if !text.is_null() {
        unsafe {
            drop(CString::from_raw(text));
        }
    }
}
/// # Safety
/// `json` is a NUL-terminated UTF-8 numeric request, valid for this call.
/// This stateless operation has no session, GPU or file access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_numeric(json: *const c_char) -> *mut c_char {
    unsafe { stateless_json(json, "Missing numeric request", |source| {
        let (localization, request) = control_request::<layer_ui::NumericRequest>(source)?;
        serde_json::to_value(request.resolve().map_err(|reason| reason.message(&localization))?).map_err(|e| e.to_string())
    }) }
}
/// Shared tagged color forms, display previews and sampled gradient ramps.
/// # Safety
/// `json` is a NUL-terminated UTF-8 color request, valid for this call.
/// This stateless operation has no session, GPU or file access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_color_ui(json: *const c_char) -> *mut c_char {
    unsafe { stateless_json(json, "Missing color request", |source| {
        let (localization, request) = control_request(source)?;
        layer_ui::color_ui_localized(request, &localization)
    }) }
}
/// # Safety
/// `json` is a NUL-terminated UTF-8 toolbar presentation request, valid for this call.
/// This stateless operation has no session, GPU or file access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_toolbar_ui(json: *const c_char) -> *mut c_char {
    unsafe { stateless_json(json, "Missing toolbar request", |source| {
        let (localization, request) = control_request(source)?;
        layer_ui::toolbar_ui(request, &localization)
    }) }
}
/// # Safety
/// `json` is readable NUL-terminated UTF-8 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_native_caption(json: *const c_char) -> *mut c_char {
    unsafe { stateless_json(json, "Missing caption request", |source| {
        let (localization, request) = control_request::<layer_ui::NativeCaption>(source)?;
        Ok(serde_json::json!({"text": request.message(&localization)}))
    }) }
}
/// # Safety
/// `json` is readable NUL-terminated UTF-8 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_numeric_labels(json: *const c_char) -> *mut c_char {
    unsafe { stateless_json(json, "Missing numeric label request", |source| {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request { label: String }
        let (localization, request) = control_request::<Request>(source)?;
        serde_json::to_value(layer_ui::NumericLabels::new(&request.label, &localization)).map_err(|e| e.to_string())
    }) }
}
/// # Safety
/// `json` is readable NUL-terminated UTF-8 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_appearance(json: *const c_char) -> *mut c_char {
    unsafe { stateless_json(json, "Missing document options", |source| {
        let (localization, options) = control_request::<layer_ui::NewDocumentOptions>(source)?;
        serde_json::to_value(options.appearance(&localization)).map_err(|e| e.to_string())
    }) }
}
unsafe fn stateless_json(json: *const c_char, missing: &str,
    resolve: impl FnOnce(&str) -> Result<serde_json::Value, String> + std::panic::UnwindSafe) -> *mut c_char {
    catch_unwind(|| {
        let result = (|| {
            if json.is_null() {
                return Err(missing.to_string());
            }
            let source = unsafe { CStr::from_ptr(json) }
                .to_str()
                .map_err(|e| e.to_string())?;
            resolve(source)
        })();
        let response = result.unwrap_or_else(|error| serde_json::json!({"error": error}));
        CString::new(response.to_string())
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut())
    })
    .unwrap_or(std::ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_color_hit(x: f32, y: f32, size: f32, shape: u32) -> u32 {
    let Some(shape) = color_shape(shape) else {
        return 0;
    };
    match layer_ui::ColorWheelGeometry::new(size).and_then(|g| g.hit_shape([x, y], shape)) {
        None => 0,
        Some(layer_ui::ColorWheelPart::Hue) => 1,
        Some(layer_ui::ColorWheelPart::Field) => 2,
    }
}
fn color_shape(shape: u32) -> Option<layer_ui::ColorShape> {
    match shape {
        0 => Some(layer_ui::ColorShape::Circle),
        1 => Some(layer_ui::ColorShape::Square),
        2 => Some(layer_ui::ColorShape::Triangle),
        _ => None,
    }
}
/// Shared dial hit testing: 0 misses, 1 center, 2 brightness, 3 color.
#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_parameter_hit(x:f32,y:f32,size:f32,hdr:bool)->u32 {
    if hdr { u32::from(layer_ui::HdrIntensityArc::new(size).is_some_and(|a|a.contains([x,y]))) }
    else {layer_ui::proof_panel::sdr_dial_hit(size,[x,y]).filter(|h| *h < 3).map_or(0, |h| u32::from(h) + 1)}
}
/// # Safety
/// Caller supplies exactly edge²*4 writable bytes. This is a disposable UI texture.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_proof_texture(edge:u32,bytes:*mut u8,count:usize)->bool {
    if edge==0 || edge>512 || bytes.is_null() || count!=(edge as usize).pow(2)*4 {return false;}
    let texture=layer_ui::proof_panel::sdr_direction_texture(edge);
    unsafe {std::ptr::copy_nonoverlapping(texture.as_ptr(),bytes,count)};
    true
}
/// # Safety
/// Borrowed JSON and side²*4 Float32 output components, exclusively writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_hdr_field(side:u32,request:*const c_char,pixels:*mut f32,count:usize,operation:u32)->bool {
    if side==0 || side>2048 || pixels.is_null() || count!=(side as usize).pow(2)*4 || request.is_null(){return false;}
    let Ok(source)=unsafe{CStr::from_ptr(request)}.to_str() else{return false;};
    let Ok(request)=serde_json::from_str::<layer_ui::color_management::PickerField>(source) else{return false;};
    let pixels=unsafe{std::slice::from_raw_parts_mut(pixels.cast::<[f32;4]>(),count/4)};
    match operation {0=>request.render(side,pixels),1=>request.render_base(side,pixels),2=>request.map(pixels),_=>Err("Unknown field operation".into())}.is_ok()
}
/// Stateless display-encoded wheel field. No editor, GPU or file access.
/// # Safety
/// `rgba` must point to `count` writable bytes exclusively borrowed for this call.
/// `rgb_space` is a NUL-terminated shared RGB-space name valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_color_field(
    side: u32,
    hue: f32,
    shape: u32,
    rgb_space: *const c_char,
    guide: bool,
    rgba: *mut u8,
    count: usize,
) -> i32 {
    let length = (side as usize)
        .checked_mul(side as usize)
        .and_then(|n| n.checked_mul(4));
    if side == 0
        || !hue.is_finite()
        || rgba.is_null()
        || length != Some(count)
        || count > isize::MAX as usize
    {
        return 0;
    }
    if rgb_space.is_null() { return 0; }
    let Ok(name) = unsafe { CStr::from_ptr(rgb_space) }.to_str() else { return 0; };
    let Ok(space) = serde_json::from_value(serde_json::Value::String(name.into())) else { return 0; };
    let Some(shape) = color_shape(shape) else { return 0; };
    let pixels = unsafe { std::slice::from_raw_parts_mut(rgba, count) };
    i32::from(if guide {
        layer_ui::render_hue_guide_in(side, shape, space, DISPLAY_SPACE, pixels)
    } else {
        layer_ui::render_color_field(side, shape, hue, space, DISPLAY_SPACE, pixels)
    })
}
/// # Safety
/// Valid handle; json must be a NUL-terminated UTF-8 string except for request 7.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_request(
    app: *mut CapyApple,
    request: u32,
    json: *const c_char,
) -> *mut c_char {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return std::ptr::null_mut();
    };
    app.perform(|a| {
        if request == 7 {
            let Some(bytes) = a.host.take_layout_update_bytes().map_err(|e| e.to_string())? else {
                return Ok(None);
            };
            let mut extension = serde_json::json!({
                "display_status": a.metal.display_status(&a.host),
                "document_tabs": a.window.view(&a.host, a.host.logical[0]),
            });
            let generation = a.host.localization_generation();
            if a.published_language != Some(generation) {
                extension["language_generation"] = generation.into();
                extension["bootstrap"] = serde_json::to_value(a.host.bootstrap_view()).map_err(|e| e.to_string())?;
                extension["catalog"] = a.host.query(serde_json::json!({"type":"catalog"}))?;
                if let Some(workspaces) = &a.workspaces {
                    extension["workspace_view"] = serde_json::to_value(&workspaces.view).map_err(|e| e.to_string())?;
                }
                a.published_language = Some(generation);
            }
            let extension = serde_json::to_vec(&extension)
            .map_err(|e| e.to_string())?;
            return CString::new(layer_host::extend_update(bytes, &extension))
                .map(|snapshot| Some(snapshot.into_raw()))
                .map_err(|e| e.to_string());
        }
        let value = {
            if json.is_null() {
                return Err("Missing request JSON".into());
            }
            serde_json::from_str(
                unsafe { CStr::from_ptr(json) }
                    .to_str()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?
        };
        let result = match request {
            0 => {
                a.host
                    .dispatch(serde_json::from_value(value).map_err(|e| e.to_string())?)?;
                Some(serde_json::Value::Null)
            }
            1 => {
                let input: layer_ui::UiInput =
                    serde_json::from_value(value).map_err(|e| e.to_string())?;
                let reply = a.host.input(input.clone())?;
                match input {
                    layer_ui::UiInput::Chrome { facts, .. } => a.chrome_facts = facts,
                    layer_ui::UiInput::Blur => a.dismissed_contacts.clear(),
                    _ => {}
                }
                Some(serde_json::to_value(reply).map_err(|e| e.to_string())?)
            }
            2 => Some(match value.get("type").and_then(serde_json::Value::as_str) {
                Some("display_headroom") => {
                    let headroom=value["value"].as_f64().ok_or("Missing display headroom")? as f32;
                    a.metal.set_headroom(&mut a.host,headroom)?;
                    serde_json::Value::Null
                },
                Some("bootstrap") => serde_json::to_value(a.host.bootstrap_view()).map_err(|e| e.to_string())?,
                Some("document_tabs") => a.tabs_request(value)?,
                Some("touch_policy") => {
                    let tap_ms = value["tap_ms"].as_u64().and_then(|ms| u32::try_from(ms).ok()).ok_or("Missing touch timing")?;
                    let slop = value["slop"].as_f64().ok_or("Missing touch slop")? as f32;
                    a.host.session.set_touch_policy(layer_ui::TouchPolicy { tap_ms, slop })?;
                    serde_json::Value::Null
                }
                Some("screen_report") => {
                    use layer_color::screen::{Chromaticities, ScreenReport};
                    use layer_core::color::{RgbSpace, hdr::REFERENCE_WHITE_NITS};
                    let name = value["name"].as_str().filter(|n| !n.is_empty()).map(str::to_owned);
                    let gamut = if value["wide"].as_bool() == Some(true) { RgbSpace::DisplayP3 } else { RgbSpace::Srgb };
                    let headroom = value["headroom"].as_f64().map(|h| h as f32).filter(|h| h.is_finite() && *h >= 1.);
                    let capable = headroom.map(|h| h > 1.);
                    let report = ScreenReport::managed(name, Chromaticities::of(gamut), capable == Some(true),
                        headroom.map(|h| h * REFERENCE_WHITE_NITS), capable);
                    if a.host.session.set_screen_report(report) {
                        a.host.dirty = true;
                        a.host.invalidate_snapshot();
                    }
                    serde_json::Value::Null
                }
                _ => a.host.query(value)?,
            }),
            6 => Some(a.workspace(value)?),
            _ => return Err("Unknown Apple host request".into()),
        };
        result
            .map(|r| {
                CString::new(r.to_string())
                    .map(CString::into_raw)
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .flatten()
    .unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// Valid exclusively owned handle, called on the serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_scroll(
    app: *mut CapyApple,
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    scale: f32,
    zoom: u32,
    horizontal: u32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        a.host
            .scroll([x, y], [dx, dy], scale, zoom != 0, horizontal != 0)
    })
    .map_or(-1, |_| 0)
}

/// # Safety
/// Valid exclusively owned handle, called on the serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_gesture(
    app: *mut CapyApple,
    x: f32,
    y: f32,
    scale: f32,
    rotation: f32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| a.host.gesture([x, y], scale, rotation))
        .map_or(-1, |_| 0)
}

/// # Safety
/// Valid handle and retained CAMetalLayer; layer outlives attach through detach.
/// Cache directory is null or a NUL-terminated UTF-8 path valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_attach(
    app: *mut CapyApple,
    layer: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
    cache_directory: *const c_char,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if layer.is_null() {
            return Err("Missing Metal layer".into());
        }
        let cache = (!cache_directory.is_null())
            .then(|| unsafe { project::read_title(cache_directory) }.map(std::path::PathBuf::from))
            .transpose()?;
        a.host.resize(width, height, scale)?;
        a.gpu_operation(|a| unsafe { a.metal.attach(&mut a.host, layer, cache) })
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle. Call once the canvas is ready.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_finish_startup_cache(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if let Some(gpu) = a.host.session.renderer_mut().0.as_mut() {
            gpu.finish_startup_cache();
        }
        Ok(())
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_resize(
    app: *mut CapyApple,
    width: u32,
    height: u32,
    scale: f32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| a.host.resize(width, height, scale))
        .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle. Request presentation after native exposure
/// without changing document, camera or history.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_redraw(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.host.dirty = true;
    0
}
/// # Safety
/// Valid exclusively owned handle; detach before releasing its native layer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_detach(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        a.host.input(layer_ui::UiInput::Blur)?;
        a.dismissed_contacts.clear();
        a.metal.detach();
        Ok(())
    })
    .map_or(-1, |_| 0)
}

/// # Safety
/// Serial owner only. Retire GPU resources while retaining the CPU session;
/// attach the retained native layer again to restart rendering.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_suspend_renderer(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else { return -1; };
    app.perform(|a| {
        a.metal.stop(&mut a.host, "Canvas stopped. Save the drawing or restart the canvas to continue.".into());
        a.dismissed_contacts.clear();
        Ok(())
    }).map_or(-1, |_| 0)
}

/// # Safety
/// Serial owner only. Nonblocking health check, including when no frame is due.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_poll_renderer(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else { return -1; };
    app.perform(|a| {
        a.gpu_operation(|a| {
            a.metal.observe_failure(&mut a.host, true);
            let changed=a.metal.poll_color(&mut a.host)? | a.metal.screen_tick(&mut a.host);
            if changed {a.host.invalidate_snapshot();}
            Ok(i32::from(changed || a.host.session.rendering_suspended()))
        })
    }).unwrap_or(-1)
}

/// # Safety
/// Debug fixtures and unit tests only; affects this editor's device, never the system GPU.
#[cfg(any(test, debug_assertions))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_test_gpu_fault(app: *mut CapyApple, validation: u32) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else { return -1; };
    app.perform(|a| {
        let device = a.host.session.engine().backend().0.as_ref().ok_or("No test GPU")?.device();
        if validation == 1 {
            let _invalid = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("isolated invalid buffer"), size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: false,
            });
        } else if validation == 0 { device.destroy(); }
        else { return Err("Unknown GPU fault".into()); }
        Ok(())
    }).map_or(-1, |_| 0)
}
/// # Safety
/// Records must contain count initialized doubles, alive for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_pointer(
    app: *mut CapyApple,
    id: u64,
    tool: u32,
    button: u32,
    records: *const f64,
    count: usize,
    predicted: u32,
    view_revision: u64,
) -> i32 {
    unsafe {
        apple_pointer(
            app,
            id,
            tool,
            button,
            records,
            count,
            predicted,
            view_revision,
            std::ptr::null(),
            0,
        )
    }
}
/// # Safety
/// Records contain `count` doubles and updates contain `count / 9 * 2` u64s.
/// Both arrays must remain alive for this call. Updates are token/expecting pairs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_pointer_updates(
    app: *mut CapyApple,
    id: u64,
    tool: u32,
    button: u32,
    records: *const f64,
    count: usize,
    updates: *const u64,
    correction: u32,
    view_revision: u64,
) -> i32 {
    if updates.is_null() {
        return -1;
    }
    unsafe {
        apple_pointer(
            app,
            id,
            tool,
            button,
            records,
            count,
            0,
            view_revision,
            updates,
            correction,
        )
    }
}
unsafe fn apple_pointer(
    app: *mut CapyApple,
    id: u64,
    tool: u32,
    button: u32,
    records: *const f64,
    count: usize,
    predicted: u32,
    view_revision: u64,
    updates: *const u64,
    correction: u32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if records.is_null()
            || count == 0
            || count % 9 != 0
            || count > 8192 * 9
            || tool > 3
            || button > 2
            || predicted > 1
            || correction > 1
        {
            return Err("Invalid Apple pointer batch".into());
        }
        let records = unsafe { std::slice::from_raw_parts(records, count) };
        if !records.iter().all(|v| v.is_finite())
            || records
                .chunks_exact(9)
                .any(|r| r[7] < 0. || r[8] < 0. || r[8] > 4. || r[8].fract() != 0.)
        {
            return Err("Invalid Apple pointer sample".into());
        }
        if !a.host.accepts_pointer_input(view_revision) {
            return Ok(());
        }
        if a.dismissed_contacts.contains(&id) {
            if correction == 0 && predicted == 0 && records.chunks_exact(9).any(|r| r[8] >= 3.) {
                a.dismissed_contacts.remove(&id);
            }
            return Ok(());
        }
        let updates = if updates.is_null() {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(updates, count / 9 * 2) }
        };
        if updates
            .chunks_exact(2)
            .any(|u| u[1] > 1 || (u[1] != 0 && u[0] == 0) || (correction != 0 && u[0] == 0))
        {
            return Err("Invalid Apple input estimates".into());
        }
        if correction == 0 && predicted == 0 && records[8] == 1. {
            let viewport = a.host.logical;
            let physical = a.host.session.state().camera.viewport;
            let position = [
                records[0] as f32 * viewport[0] / physical[0].max(1) as f32,
                records[1] as f32 * viewport[1] / physical[1].max(1) as f32,
            ];
            let reply = a.host.input(layer_ui::UiInput::Chrome {
                event: layer_ui::ChromeEvent::Contact {
                    position,
                    canvas: true,
                },
                facts: a.chrome_facts,
                viewport,
            })?;
            if reply.handled {
                if !records.chunks_exact(9).any(|r| r[8] >= 3.) {
                    a.dismissed_contacts.insert(id);
                }
                return Ok(());
            }
        }
        a.host.pointer_batch_updates(
            PointerBatch {
                id,
                tool: tool as u8,
                button: button as u8,
                records,
                predicted: predicted != 0,
                view_revision,
                barrel_twist: false,
            },
            updates,
            correction != 0,
        )
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid handle; costs, if non-NULL, must point to five writable u64 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_frame(
    app: *mut CapyApple,
    now: u64,
    presentation: u64,
    costs: *mut u64,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if presentation < now {
            return Err("Presentation precedes observation time".into());
        }
        if a.host.session.rendering_suspended() { return Ok(0); }
        let (again, timing) = a.gpu_operation(|a| a.metal.frame(&mut a.host, now, presentation))?;
        if !costs.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(timing.as_ptr(), costs, 5);
            }
        }
        Ok(i32::from(again))
    })
    .unwrap_or(-1)
}
/// # Safety
/// Valid handle on its serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_canvas_bar_hold(app: *const CapyApple) -> u32 {
    unsafe { app.as_ref() }.map_or(0, |a| a.host.session.canvas_bar_hold())
}

/// # Safety
/// Valid handle on its serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_shader_input(app: *const CapyApple) {
    if let Some(gpu) = unsafe { app.as_ref() }.and_then(|a| a.host.session.engine().backend().0.as_ref()) {
        gpu.shader_input();
    }
}

/// # Safety
/// Valid handle, read on its serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_camera_revision(app: *const CapyApple) -> u64 {
    unsafe { app.as_ref() }.map_or(0, |a| a.host.session.state().camera.revision)
}

/// # Safety
/// Valid handle on its serial owner. Timing is opt-in and disabled by default.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_gpu_timing(app: *mut CapyApple, enabled: u32) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if enabled > 1 {
            return Err("Invalid GPU timing flag".into());
        }
        a.metal.set_timing_enabled(enabled != 0);
        Ok(0)
    })
    .unwrap_or(-1)
}

/// # Safety
/// Valid serial-owned handle, writable stats and samples (capacity <= 256).
/// A zero-capacity call may pass NULL samples. Never waits for GPU completion.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_take_gpu_timing(
    app: *mut CapyApple,
    samples: *mut layer_render_wgpu::GpuFrameSample,
    capacity: usize,
    stats: *mut layer_render_wgpu::GpuFrameTimingStats,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|a| {
        if capacity > 256 || stats.is_null() || (capacity > 0 && samples.is_null()) {
            return Err("Invalid GPU timing output buffer".into());
        }
        let output = if capacity == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(samples, capacity) }
        };
        let (count, status) = a.metal.take_timing(&mut a.host, output)?;
        unsafe {
            *stats = status;
        }
        Ok(count as i32)
    })
    .unwrap_or(-1)
}
