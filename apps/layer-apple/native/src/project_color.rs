//! Worker preparation and atomic owner publication of shared color/history edits.
use super::*;

/// # Safety
/// File worker only. Choice is optional DocumentColorChange/ColorProfile JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_edit_work(task: *const CapyProjectTask, choice: *const c_char, copy: bool) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    let choice = unsafe { read_title(choice) }.map(str::to_owned);
    task.perform(|payload| {
        let choice = choice?;
        match payload {
            Payload::Color(color) => color.work(serde_json::from_str(&choice).map_err(|e| e.to_string())?, copy, task.control.clone()),
            Payload::Source(source) if !copy => source.work(serde_json::from_str(&choice).map_err(|e| e.to_string())?, || task.control.is_cancelled()),
            _ => Err("Not an editable color/source task".into()),
        }
    })
}
/// # Safety
/// Owner only, after edit_work. Shared validation constructs the source candidate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_candidate(app: *mut CapyApple, task: *const CapyProjectTask) -> i32 {
    let (Some(app), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else { return -1; };
    app.perform(|app| {
        task.check_cancelled()?;
        let mut state = task.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = &state.error { return Err(error.clone()); }
        match &mut state.payload {
            Payload::Source(source) => source.prepare(&app.host, task.control.is_cancelled()),
            Payload::Color(_) => Ok(()),
            _ => Err("Not an editable color/source task".into()),
        }
    }).map_or(-1, |_| 0)
}
/// # Safety
/// Worker only, after project_candidate; renders complete source comparisons.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_compare(task: *const CapyProjectTask) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    task.perform(|payload| match payload {
        Payload::Source(source) => source.compare(task.control.clone()),
        Payload::Export(export) => export.compare(task.control.clone()),
        Payload::Color(_) => Ok(()),
        _ => Err("Not an editable color/source task".into()),
    })
}
/// # Safety
/// File worker only. Returns owned metadata JSON. Parsing profiles stays off owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_details(task: *const CapyProjectTask) -> *mut c_char {
    let Some(task) = (unsafe { task.as_ref() }) else { return std::ptr::null_mut(); };
    let mut json = String::new();
    let result = task.perform(|payload| {
        json = match payload {
            Payload::Info(info) => serde_json::to_string(&info.describe()?),
            Payload::Inspection(inspection) => serde_json::to_string(&inspection.histogram(task)?),
            Payload::Source(source) => serde_json::to_string(&source.details()?),
            Payload::Export(export) => serde_json::to_string(&export.details()?),
            Payload::Color(color) => serde_json::to_string(&color.details()),
            _ => return Err("No document details are available".into()),
        }.map_err(|e| e.to_string())?;
        Ok(())
    });
    if result < 0 { std::ptr::null_mut() } else { CString::new(json).unwrap().into_raw() }
}
#[repr(C)]
pub struct CapyProjectPreview {
    pub width: u32, pub height: u32, pub pixels: *const u8, pub count: usize,
}
/// # Safety
/// Worker only, after successful preparation. Borrowed pixels remain readable
/// until the next mutating job call/free; copy them before returning to the UI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_preview(task: *const CapyProjectTask, after: bool, output: *mut CapyProjectPreview) -> i32 {
    unsafe { capy_project_preview_at(task, u32::from(after), output) }
}
/// # Safety
/// Same worker ownership as capy_project_preview; index 2 is the encoded SDR base.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_preview_at(task: *const CapyProjectTask, index: u32, output: *mut CapyProjectPreview) -> i32 {
    let (Some(task), Some(output)) = (unsafe { task.as_ref() }, unsafe { output.as_mut() }) else { return -1; };
    let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
    let previews = match &state.payload { Payload::Color(c) => c.previews(), Payload::Source(s) => s.previews(), Payload::Export(e) => &e.previews, _ => return -1 };
    let Some(preview) = previews.get(index as usize) else { return -1; };
    *output = CapyProjectPreview { width: preview.extent[0], height: preview.extent[1], pixels: preview.pixels.as_ptr(), count: preview.pixels.len() };
    0
}
