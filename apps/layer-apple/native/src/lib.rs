//! Apple host ABI. The same library serves UIKit and AppKit. Rust owns the
//! shared session; Swift owns UI and serial execution. No callbacks into Swift.
mod handles;
pub use handles::{capy_apple_string_copy,capy_apple_string_free,capy_apple_string_length};
mod metal;
mod frame_rate;
pub use frame_rate::*;
mod document_tabs;
pub use document_tabs::*;
mod session;
pub use session::*;
mod papers;
pub use papers::*;
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

pub struct CapyApple {
    metal: metal::MetalHost,
    host: NativeHost,
    window: document_tabs::Window,
    session_disk: Option<std::sync::Arc<std::sync::Mutex<session::WindowDisk>>>,
    session_capture_sequence: u64,
    paper_directory: Option<std::path::PathBuf>,
    error: Option<CString>,
    chrome_facts: layer_ui::ChromeFacts,
    dismissed_contacts: std::collections::BTreeSet<u64>,
    workspaces: Option<layer_workspace::WorkspaceController<layer_workspace::StoreWorker>>,
    language: layer_ui::LanguageTransition,
    published_language: Option<u64>,
    scopes: scopes::Cache,
}
impl AsMut<NativeHost> for CapyApple {
    fn as_mut(&mut self) -> &mut NativeHost { &mut self.host }
}
impl CapyApple {
    fn perform<T>(&mut self, work: impl FnOnce(&mut Self) -> Result<T, String>) -> Option<T> {
        self.error = None;
        let result = layer_host::contain(self, |a| {
            a.metal.observe_failure(&mut a.host, false);
            work(a)
        });
        match result {
            Ok(value) => Some(value),
            Err(message) => {
                self.error = CString::new(message.replace('\0', " ")).ok();
                None
            }
        }
    }
    fn gpu_operation<T>(&mut self, work: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        let result = work(self);
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
pub extern "C" fn capy_apple_create(platform: u32) -> u64 {
    apple_launch_localized(platform, "", fixture_localization()).unwrap_or(0)
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
    paper_directory: Option<std::path::PathBuf>,
    screen: Option<layer_ui::ScreenInfo>,
}
pub struct CapyAppleLaunch {
    settings: layer_ui::Settings,
    localization: std::sync::Arc<layer_ui::Localizer>,
    paper: layer_host::papers::PreparedPaper,
    directory: Option<std::path::PathBuf>,
    screen: Option<layer_ui::ScreenInfo>,
}
impl CapyAppleLaunch {
    fn prepare(saved: &str, localization: std::sync::Arc<layer_ui::Localizer>, directory: Option<std::path::PathBuf>, screen: Option<layer_ui::ScreenInfo>) -> Self {
        let settings = layer_ui::Settings::restore_localized(saved, &localization);
        let material = layer_ui::paper_library::PaperMaterial::capture(&settings.new_document, None);
        let paper = layer_host::papers::resolve(&material, directory.as_deref(), &localization);
        Self { settings, localization, paper, directory, screen }
    }
}
/// # Safety
/// File worker only; json is readable NUL-terminated UTF-8; error is writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_prepare_launch(json: *const c_char, error: *mut u64) -> u64 {
    if !error.is_null() { unsafe { *error = 0 }; }
    let result = catch_unwind(|| {
        if json.is_null() { return Err("Missing launch configuration".into()); }
        let source = (unsafe { CStr::from_ptr(json) }).to_str().map_err(|e| e.to_string())?;
        let launch = serde_json::from_str::<AppleLaunch>(source).map_err(|e| e.to_string())?;
        let tags: Vec<&str> = launch.preferred_languages.iter().map(String::as_str).collect();
        let localization = layer_ui::launch_localization(&launch.saved, &tags);
        layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        Ok(handles::store(CapyAppleLaunch::prepare(&launch.saved, localization, launch.paper_directory, launch.screen)))
    }).unwrap_or_else(|payload| Err(layer_host::panic_diagnostic(&*payload)));
    result.unwrap_or_else(|message: String| {
        if !error.is_null() { unsafe { *error = CString::new(format!("Session launch: {message}").replace('\0', " ")).map_or(0,handles::text) }; }
        0
    })
}
/// # Safety
/// Prepared launch remains live through the exclusive owner call. Writable outputs contain owned text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_launch(platform: u32, prepared: u64, bootstrap: *mut u64, error: *mut u64) -> u64 {
    if !bootstrap.is_null() { unsafe { *bootstrap = 0 }; }
    if !error.is_null() { unsafe { *error = 0 }; }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let prepared = handles::shared::<CapyAppleLaunch>(prepared).ok_or("Missing launch configuration")?;
        if !bootstrap.is_null() {
            let view = serde_json::to_string(&layer_ui::bootstrap_view(&prepared.localization)).map_err(|e| e.to_string())?;
            let view = CString::new(view).map_err(|e| e.to_string())?;
            unsafe { *bootstrap = handles::text(view) };
        }
        apple_launch_prepared(platform, &prepared)
    })).unwrap_or_else(|payload| Err(layer_host::panic_diagnostic(&*payload)));
    match result {
        Ok(app) => app,
        Err(message) => {
            if !error.is_null() { unsafe { *error = CString::new(format!("Session launch: {message}").replace('\0', " ")).map_or(0,handles::text) }; }
            0
        }
    }
}
/// # Safety
/// Prepared launch is owned exactly once; owner readers have finished. Drop on the file worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_launch_free(prepared: u64) {
    handles::free::<CapyAppleLaunch>(prepared);
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
            .map_err(|payload| layer_host::panic_diagnostic(&*payload))
    })
}
#[cfg(test)]
fn apple_launch_localized(platform: u32, saved: &str, localization: std::sync::Arc<layer_ui::Localizer>) -> Result<u64, String> {
    let prepared = CapyAppleLaunch::prepare(saved, localization, None, None);
    apple_launch_prepared(platform, &prepared)
}
fn apple_launch_prepared(platform: u32, prepared: &CapyAppleLaunch) -> Result<u64, String> {
    let platform = match platform {
        0 => layer_ui::Platform::Ios,
        1 => layer_ui::Platform::Mac,
        _ => return Err(format!("Unknown Apple platform {platform}")),
    };
    let mut host = NativeHost::launch_prepared(platform, prepared.settings.clone(), prepared.localization.clone(), prepared.paper.clone(), prepared.screen)?;
    host.ui_color = layer_host::UiColor::Tagged(DISPLAY_SPACE);
    host.session.set_document_replacement(false);
    let window = document_tabs::Window::localized(host.session.localization());
    handles::store_app(CapyApple {
        language: layer_ui::LanguageTransition::new(host.session.localization().clone()),
        published_language: None,
        scopes: Default::default(),
        host,
        window,
        session_disk: None,
        session_capture_sequence: 0,
        paper_directory: prepared.directory.clone(),
        metal: metal::MetalHost::default(),
        error: None,
        chrome_facts: Default::default(),
        dismissed_contacts: Default::default(),
        workspaces: None,
    })
}
pub struct CapyLanguageTask {
    request: layer_ui::LanguageRequest,
    prepared: Option<std::sync::Arc<layer_ui::Localizer>>,
}
/// # Safety
/// Serial owner only. JSON is an ordered array of preferred system-language tags.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_language_request(app: u64, json: *const c_char) -> u64 {
    let Some(mut lease_app)=handles::app(app) else { return 0; };
    let app=&mut *lease_app;
    app.perform(|a| {
        if json.is_null() { return Err("Missing preferred languages".into()); }
        let tags: Vec<String> = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
        Ok(a.language.request(a.host.session.state().settings.language, &tags)
            .map(|request| handles::mutable(CapyLanguageTask { request, prepared: None })))
    }).flatten().unwrap_or(0)
}
/// # Safety
/// Preparation worker only; task has no concurrent caller or borrowed editor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_language_prepare(task: u64) {
    if let Some(mut lease_task)=handles::borrow::<CapyLanguageTask>(task) {let task=&mut *lease_task;
        layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        task.prepared = Some(layer_ui::Localizer::shared(task.request.language));
    }
}
/// # Safety
/// Serial owner only after worker preparation; task remains owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_language_prepared(app: u64, task: u64) -> bool {
    let (Some(mut lease_app),Some(mut lease_task))=(handles::app(app),handles::borrow::<CapyLanguageTask>(task)) else { return false; };
    let (app,task)=(&mut *lease_app,&mut *lease_task);
    task.prepared.take().is_some_and(|localization| app.language.prepared(task.request, localization))
}
/// # Safety
/// Serial owner only. Native input capture/composition is supplied by the UI owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_language_publish(app: u64, input_busy: bool) -> i32 {
    let Some(mut lease_app)=handles::app(app) else { return -1; };
    let app=&mut *lease_app;
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
pub unsafe extern "C" fn capy_language_free(task: u64) {
    handles::free::<CapyLanguageTask>(task);
}
/// # Safety
/// Call on the serial owner after detaching the native surface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_destroy(app: u64) {
    handles::free_app(app);
}
/// # Safety
/// Call on the serial owner. The returned text is owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_error(app: u64) -> u64 {
    handles::app(app)
        .and_then(|a| a.error.clone())
        .map_or(0, handles::text)
}
/// # Safety
/// `json` is a NUL-terminated UTF-8 numeric request, valid for this call.
/// This stateless operation has no session, GPU or file access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_numeric(json: *const c_char) -> u64 {
    unsafe { stateless_json(json, "Missing numeric request", |source| {
        let (localization, request) = control_request::<layer_ui::NumericRequest>(source)?;
        serde_json::to_value(request.resolve().map_err(|reason| reason.message(&localization))?).map_err(|e| e.to_string())
    }) }
}
/// # Safety
/// `key` is readable NUL-terminated UTF-8 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_control_takes_key(key: *const c_char, command: bool, alt: bool, keyboard_focus: bool) -> bool {
    let key = unsafe { std::ffi::CStr::from_ptr(key) }.to_str().unwrap_or("");
    layer_ui::control_takes_key(key, layer_ui::Modifiers { command, alt, shift: false }, keyboard_focus)
}
/// # Safety
/// Strings are readable NUL-terminated UTF-8 for this call; `latin` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_shortcut_key_is(key: *const c_char, latin: *const c_char, expected: *const c_char) -> bool {
    let key = unsafe { std::ffi::CStr::from_ptr(key) }.to_str().unwrap_or("");
    let latin = (!latin.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(latin) }.to_str().unwrap_or(""))
        .filter(|latin| !latin.is_empty());
    let expected = unsafe { std::ffi::CStr::from_ptr(expected) }.to_str().unwrap_or("");
    layer_ui::KeyChord::new(layer_ui::shortcut_key(key, latin), layer_ui::Modifiers::default()).key.eq_ignore_ascii_case(expected)
}
/// Shared tagged color forms, display previews and sampled gradient ramps.
/// # Safety
/// `json` is a NUL-terminated UTF-8 color request, valid for this call.
/// This stateless operation has no session, GPU or file access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_color_ui(json: *const c_char) -> u64 {
    unsafe { stateless_json(json, "Missing color request", |source| {
        let (localization, request) = control_request(source)?;
        layer_ui::color_ui_localized(request, &localization)
    }) }
}
/// # Safety
/// `json` is a NUL-terminated UTF-8 toolbar presentation request, valid for this call.
/// This stateless operation has no session, GPU or file access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_toolbar_ui(json: *const c_char) -> u64 {
    unsafe { stateless_json(json, "Missing toolbar request", |source| {
        let request: ControlRequest<layer_ui::ToolbarUiRequest> = serde_json::from_str(source).map_err(|e| e.to_string())?;
        match request.request {
            layer_ui::ToolbarUiRequest::Metrics => Ok(layer_ui::toolbar_metrics()),
            toolbar => {
                let localization = layer_ui::Localizer::prepared(request.language).ok_or("apple_language_not_prepared")?;
                layer_ui::toolbar_ui(toolbar, &localization)
            }
        }
    }) }
}
/// # Safety
/// `json` is readable NUL-terminated UTF-8 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_drawer_frame(json: *const c_char) -> u64 {
    unsafe { stateless_json(json, "Missing drawer frame", layer_ui::drawer_frame) }
}
/// # Safety
/// `json` is readable NUL-terminated UTF-8 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_settings_search_key(json: *const c_char) -> u64 {
    #[derive(serde::Deserialize)]
    struct Request { key: String, modifiers: layer_ui::Modifiers, editing: bool, capturing: bool }
    unsafe { stateless_json(json, "Missing settings key", |source| {
        let request: Request = serde_json::from_str(source).map_err(|e| e.to_string())?;
        Ok(serde_json::json!(layer_ui::settings_search_key(&request.key, request.modifiers, request.editing, request.capturing)))
    }) }
}
/// # Safety
/// `json` is readable NUL-terminated UTF-8 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_bubble_ui(json: *const c_char) -> u64 {
    unsafe { stateless_json(json, "Missing bubble request", |source| {
        layer_host::bubbles::query(serde_json::from_str(source).map_err(|e| e.to_string())?)
    }) }
}
/// # Safety
/// `json` is readable NUL-terminated UTF-8 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_native_caption(json: *const c_char) -> u64 {
    unsafe { stateless_json(json, "Missing caption request", |source| {
        let (localization, request) = control_request::<layer_ui::NativeCaption>(source)?;
        Ok(serde_json::json!({"text": request.message(&localization)}))
    }) }
}
/// # Safety
/// `json` is readable NUL-terminated UTF-8 for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_numeric_labels(json: *const c_char) -> u64 {
    unsafe { stateless_json(json, "Missing numeric label request", |source| {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request { label: String }
        let (localization, request) = control_request::<Request>(source)?;
        serde_json::to_value(layer_ui::NumericLabels::new(&request.label, &localization)).map_err(|e| e.to_string())
    }) }
}
unsafe fn stateless_json(json: *const c_char, missing: &str,
    resolve: impl FnOnce(&str) -> Result<serde_json::Value, String> + std::panic::UnwindSafe) -> u64 {
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
            .map(handles::text)
            .unwrap_or(0)
    })
    .unwrap_or(0)
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
    app: u64,
    request: u32,
    json: *const c_char,
) -> u64 {
    let Some(mut lease_app)=handles::app(app) else {
        return 0;
    };
    let app=&mut *lease_app;
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
                .map(|snapshot| Some(handles::text(snapshot)))
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
                    .map(handles::text)
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .flatten()
    .unwrap_or(0)
}
/// # Safety
/// Valid exclusively owned handle, called on the serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_scroll(
    app: u64,
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    scale: f32,
    zoom: u32,
    horizontal: u32,
) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
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
    app: u64,
    x: f32,
    y: f32,
    scale: f32,
    rotation: f32,
    began: u32,
) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
    app.perform(|a| a.host.gesture([x, y], scale, rotation, began != 0))
        .map_or(-1, |_| 0)
}

/// # Safety
/// Valid handle and retained CAMetalLayer; layer outlives attach through detach.
/// Cache directory is null or a NUL-terminated UTF-8 path valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_attach(
    app: u64,
    layer: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
    app.perform(|a| {
        if layer.is_null() {
            return Err("Missing Metal layer".into());
        }
        a.host.resize(width, height, scale)?;
        a.host.reset_canvas_frame_rate();
        a.gpu_operation(|a| unsafe { a.metal.attach(&mut a.host, layer) })
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle. Call once the canvas is ready.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_finish_startup_catalog(app: u64) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
    app.perform(|a| {
        if let Some(gpu) = a.host.session.renderer_mut().0.as_mut() {
            gpu.finish_startup_catalog();
        }
        Ok(())
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_resize(
    app: u64,
    width: u32,
    height: u32,
    scale: f32,
) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
    app.perform(|a| a.host.resize(width, height, scale))
        .map_or(-1, |_| 0)
}
/// # Safety
/// Valid exclusively owned handle. Request presentation after native exposure
/// without changing document, camera or history.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_redraw(app: u64) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
    app.host.dirty = true;
    0
}
/// # Safety
/// Valid exclusively owned handle; detach before releasing its native layer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_detach(app: u64) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
    app.perform(|a| {
        a.host.input(layer_ui::UiInput::Blur)?;
        a.dismissed_contacts.clear();
        a.metal.detach();
        a.host.reset_canvas_frame_rate();
        Ok(())
    })
    .map_or(-1, |_| 0)
}

/// # Safety
/// Serial owner only. Retire GPU resources while retaining the CPU session;
/// attach the retained native layer again to restart rendering.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_suspend_renderer(app: u64) -> i32 {
    let Some(mut lease_app)=handles::app(app) else { return -1; };
    let app=&mut *lease_app;
    app.perform(|a| {
        a.metal.stop(&mut a.host, "Canvas stopped. Save the drawing or restart the canvas to continue.".into());
        a.dismissed_contacts.clear();
        Ok(())
    }).map_or(-1, |_| 0)
}

/// # Safety
/// Serial owner only. Nonblocking health check, including when no frame is due.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_poll_renderer(app: u64) -> i32 {
    let Some(mut lease_app)=handles::app(app) else { return -1; };
    let app=&mut *lease_app;
    app.perform(|a| {
        a.gpu_operation(|a| {
            a.metal.observe_failure(&mut a.host, true);
            let changed=a.metal.poll_color(&mut a.host)? | a.metal.screen_tick(&mut a.host)
                | a.metal.poll_presentation(&mut a.host)?;
            if changed {a.host.invalidate_snapshot();}
            Ok(i32::from(changed || a.host.session.rendering_suspended()))
        })
    }).unwrap_or(-1)
}

/// # Safety
/// Debug fixtures and unit tests only; affects this editor's device, never the system GPU.
#[cfg(any(test, debug_assertions))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_test_gpu_fault(app: u64, validation: u32) -> i32 {
    let Some(mut lease_app)=handles::app(app) else { return -1; };
    let app=&mut *lease_app;
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
    app: u64,
    id: u64,
    tool: u32,
    button: u32,
    records: *const f64,
    count: usize,
    predicted: u32,
    view_revision: u64,
    device_flags: u32,
) -> i32 {
    unsafe {
        apple_pointer(
            app,
            ApplePointerInput {id,tool,button,predicted,view_revision},
            records,
            count,
            std::ptr::null(),
            0,
            device_flags,
        )
    }
}
/// # Safety
/// Records contain `count` doubles and updates contain `count / 9 * 2` u64s.
/// Both arrays must remain alive for this call. Updates are token/expecting pairs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_pointer_updates(
    app: u64,
    id: u64,
    tool: u32,
    button: u32,
    records: *const f64,
    count: usize,
    updates: *const u64,
    correction: u32,
    view_revision: u64,
    device_flags: u32,
) -> i32 {
    unsafe {
        apple_pointer(
            app,
            ApplePointerInput {id,tool,button,predicted:0,view_revision},
            records,
            if updates.is_null() {0} else {count},
            updates,
            correction,
            device_flags,
        )
    }
}
struct ApplePointerInput {id:u64,tool:u32,button:u32,predicted:u32,view_revision:u64}
unsafe fn apple_pointer(
    app: u64,
    input: ApplePointerInput,
    records: *const f64,
    count: usize,
    updates: *const u64,
    correction: u32,
    device_flags: u32,
) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
    let ApplePointerInput {id,tool,button,predicted,view_revision}=input;
    app.perform(|a| {
        if !a.host.accepts_pointer_input(view_revision) { return Ok(()); }
        if tool > 3 { return Err("Invalid Apple pointer tool".into()); }
        let invalid = records.is_null()
            || count == 0
            || !count.is_multiple_of(9)
            || count > 8192 * 9
            || button > 2
            || predicted > 1
            || correction > 1
            || device_flags & !u32::from(layer_engine::SampleFlags::INDIRECT_POINTER.0 | layer_engine::SampleFlags::BARREL_TWIST.0) != 0;
        let records = if invalid { &[][..] } else { unsafe { std::slice::from_raw_parts(records, count) } };
        let updates = if updates.is_null() || records.is_empty() {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(updates, records.len() / 9 * 2) }
        };
        let batch = PointerBatch { id, tool: tool as u8, button: button as u8, records,
            predicted: predicted != 0, view_revision, device_flags: layer_engine::SampleFlags(device_flags as u16) };
        if batch.validate(updates, correction != 0).is_err() {
            return a.host.pointer_batch_updates(batch, updates, correction != 0);
        }
        if a.dismissed_contacts.contains(&id) {
            if correction == 0 && predicted == 0 && records.as_chunks::<9>().0.iter().any(|r| r[8] >= 3.) {
                a.dismissed_contacts.remove(&id);
            }
            return Ok(());
        }
        if correction == 0 && let Some(position) = batch.contact_start() {
            let viewport = a.host.logical;
            let physical = a.host.session.state().camera.viewport;
            let position = [
                position[0] * viewport[0] / physical[0].max(1) as f32,
                position[1] * viewport[1] / physical[1].max(1) as f32,
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
                if !records.as_chunks::<9>().0.iter().any(|r| r[8] >= 3.) {
                    a.dismissed_contacts.insert(id);
                }
                return Ok(());
            }
        }
        a.host.pointer_batch_updates(batch, updates, correction != 0)
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Valid handle; costs, if non-NULL, must point to five writable u64 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_frame(
    app: u64,
    now: u64,
    presentation: u64,
    costs: *mut u64,
) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
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
pub unsafe extern "C" fn capy_apple_canvas_bar_hold(app: u64) -> u32 {
    handles::app(app).map_or(0, |a| a.host.session.canvas_bar_hold())
}

/// # Safety
/// Valid handle on its serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_shader_input(app: u64) {
    if let Some(app)=handles::app(app) && let Some(gpu)=app.host.session.engine().backend().0.as_ref() {
        gpu.shader_input();
    }
}

/// # Safety
/// Valid handle on its serial owner, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_consumed_paint_ns(app: u64) -> u64 {
    handles::app(app).map_or(0, |a| a.host.session.engine().metrics().last_consumed_paint_ns)
}

/// # Safety
/// Valid handle on its serial owner, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_submitted_canvas_updates(app: u64) -> u64 {
    handles::app(app).map_or(0,|a|a.host.session.engine().backend().0.as_ref().map_or(0,|gpu|gpu.submitted_updates()))
}

/// # Safety
/// Valid handle, read on its serial owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_camera_revision(app: u64) -> u64 {
    handles::app(app).map_or(0, |a| a.host.session.state().camera.revision)
}

/// # Safety
/// Valid handle on its serial owner. Timing is opt-in and disabled by default.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_gpu_timing(app: u64, enabled: u32) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
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
    app: u64,
    samples: *mut layer_render_wgpu::GpuFrameSample,
    capacity: usize,
    stats: *mut layer_render_wgpu::GpuFrameTimingStats,
) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
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
