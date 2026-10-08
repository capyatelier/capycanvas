//! Pixel copies: frozen on the serial owner, composed and encoded on a file
//! worker. The application keeps the clip; the pasteboard carries its PNG and nonce.
use super::*;

/// # Safety
/// The task stays alive. Returns owned progress text for a large copy, or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_clip_progress(task: *const CapyProjectTask) -> *mut c_char {
    let Some(task) = (unsafe { task.as_ref() }) else { return std::ptr::null_mut() };
    let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
    let Payload::Clip { task: Some(clip), operation, .. } = &state.payload else { return std::ptr::null_mut() };
    let details = clip.capture_details();
    if !details.large { return std::ptr::null_mut(); }
    CString::new(operation.title(&task.localization).to_string()).map_or(std::ptr::null_mut(), CString::into_raw)
}

/// # Safety
/// Worker only. Composes and encodes the copy; `nonce` is NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_clip_run(task: *const CapyProjectTask, nonce: *const c_char) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1 };
    let nonce = unsafe { read_title(nonce) }.map(str::to_owned);
    task.perform(|payload| {
        let nonce = nonce?;
        let Payload::Clip { task: capture, clip, .. } = payload else { return Err("Not a copy task".into()) };
        let capture = capture.take().ok_or("This copy has already run")?;
        *clip = Some(Box::new(capture.run(nonce, task.control.clone())?));
        Ok(())
    })
}

/// # Safety
/// Worker only, after `capy_project_clip_run`. The PNG stays borrowed from the
/// task until it is adopted or freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_clip_png(task: *const CapyProjectTask, bytes: *mut *const u8, count: *mut usize) -> i32 {
    let (Some(task), Some(bytes), Some(count)) = (unsafe { task.as_ref() }, unsafe { bytes.as_mut() }, unsafe { count.as_mut() }) else { return -1 };
    let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
    let Payload::Clip { clip: Some(clip), .. } = &state.payload else { return -1 };
    *bytes = clip.png.as_ptr();
    *count = clip.png.len();
    0
}

/// # Safety
/// The prepared task stays alive; called on the pasteboard thread after writing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_clip_published(task: *const CapyProjectTask) {
    let Some(task) = (unsafe { task.as_ref() }) else { return };
    let mut state = task.state.lock().unwrap_or_else(|e| e.into_inner());
    if let Payload::Clip { publication, clip: Some(_), .. } = &mut state.payload {
        publication.get_or_insert_with(layer_ui::RetainedClipboard::publication);
    }
}

pub(super) fn adopt_clip(app: &mut CapyApple, task: &CapyProjectTask, clip: &mut Option<Box<PixelClip>>, publication: u64, request: u32) -> Result<(), String> {
    let clip = clip.take().ok_or("The copy did not finish")?;
    if unsafe { capy_project_begin_commit(task) } < 0 { return Err("Document operation cancelled".into()); }
    app.window.documents.clip.set_published(*clip, publication);
    let previous = app.host.session.state().revision;
    let mut change = app.host.session.complete_document_request(request, Ok(true))?;
    change.canvas_wake = true;
    app.host.apply_change(previous, change);
    Ok(())
}

/// # Safety
/// Session owner only. Returns the nonce of the retained copy, or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_clip_nonce(app: *mut CapyApple) -> *mut c_char {
    let Some(app) = (unsafe { app.as_mut() }) else { return std::ptr::null_mut() };
    app.window.documents.clip.get()
        .and_then(|clip| CString::new(clip.nonce.as_str()).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}
