//! Transfer jobs bridge the serial editor owner and a background file worker.
//! Jobs never retain a session pointer. File descriptors/URLs stay host-owned.
use super::*;
use layer_core::authored::ArtworkCapture;
use layer_core::package::codec::PreparedPackage;
use layer_host::{Renderer, clipboard::ClipTask, export::ExportTask, open::OpenEnvironment, tasks::{ColorTask, SourceTask}, window::OpenAdoption};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{CloseDecision, DocumentLocation, DocumentRequest, HostRequestKind, PixelClip, UiSession};
use std::{
    fs::File,
    io::{Cursor, Read, Write, Seek, SeekFrom},
    mem::ManuallyDrop,
    os::fd::FromRawFd,
    sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

#[path = "project_clipboard.rs"]
mod clipboard;
pub use clipboard::*;
#[path = "project_color.rs"]
mod color;
pub use color::*;
#[path = "project_preferences.rs"]
mod preferences;
pub use preferences::*;
#[path = "project_proof.rs"]
mod proof;
pub use proof::*;

enum Payload {
    Package(layer_ui::PackageView),
    Color(Box<ColorTask>),
    Source(Box<SourceTask>),
    Info(layer_color::DocumentInfo),
    Lookup {
        resource: Option<std::sync::Arc<layer_core::Lut3d>>,
        request: u32,
    },
    Proof(Box<proof::Task>),
    Save {
        snapshot: Option<ArtworkCapture>,
        project: Option<ArtworkCapture>,
        gpu: Option<layer_render_wgpu::snapshot::SnapshotGpu>,
    },
    Open {
        environment: Option<OpenEnvironment>,
        candidate: Option<Box<UiSession<Renderer>>>,
        source: layer_ui::ImportSource,
        imported: Option<layer_ui::ImportedDocument>,
    },
    Placed {
        images: layer_ui::ImageImportBatch,
        context: layer_ui::ImagePlacementContext,
        request: u32,
        device: wgpu::Device,
        environment: Option<OpenEnvironment>,
        clip: Option<PixelClip>,
        color: layer_core::color::DocumentColor,
        mode: Option<layer_ui::PasteMode>,
    },
    Export(Box<ExportTask>),
    Clip {
        task: Option<Box<ClipTask>>,
        clip: Option<Box<PixelClip>>,
        publication: Option<u64>,
        request: u32,
        operation:DocumentRequest,
    },
    Retired {
        _renderer: Option<Box<WgpuRasterizer>>,
    },
}
struct State {
    payload: Payload,
    error: Option<String>,
    expectation: Option<layer_ui::DestinationExpectation>,
    fingerprint: Option<layer_ui::DestinationFingerprint>,
}
pub struct CapyProjectTask {
    state: Mutex<State>,
    phase: AtomicU8,
    control: layer_render_wgpu::snapshot::CaptureControl,
    epoch: u64,
    revision: u64,
    save_request: Option<u32>,
    localization: std::sync::Arc<layer_ui::Localizer>,
}
impl CapyProjectTask {
    fn new(payload: Payload, epoch: u64, revision: u64, save_request: Option<u32>, localization: std::sync::Arc<layer_ui::Localizer>) -> *mut Self {
        Box::into_raw(Box::new(Self {
            state: Mutex::new(State {
                payload,
                error: None,
                expectation: None,
                fingerprint: None,
            }),
            phase: AtomicU8::new(0),
            control: Default::default(),
            epoch,
            revision,
            save_request,
            localization,
        }))
    }
    fn check_cancelled(&self) -> Result<(), String> {
        if self.phase.load(Ordering::Acquire) == 1 {
            Err("Document operation cancelled".into())
        } else {
            Ok(())
        }
    }
    fn perform(&self, work: impl FnOnce(&mut Payload) -> Result<(), String> + Send) -> i32 {
        match on_large_stack("capy-project", || self.perform_inner(work)) {
            Ok(status) => status,
            Err(error) => {
                self.state.lock().unwrap_or_else(|e| e.into_inner()).error = Some(error);
                -1
            }
        }
    }
    fn perform_inner(&self, work: impl FnOnce(&mut Payload) -> Result<(), String>) -> i32 {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.error = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.check_cancelled()?;
            work(&mut state.payload)
        }));
        state.error = match result {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e),
            Err(_) => Some("Document operation failed".into()),
        };
        if state.error.is_some() { -1 } else { 0 }
    }
}

/// # Safety
/// Called on the session owner. Return a job to a worker; never use the session
/// from that worker. The caller must eventually free the job after all calls.
/// placement is optional JSON for kind 3: a surface point or a layer-row hit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_task(
    app: *mut CapyApple,
    opening: u32,
    placement: *const c_char,
) -> *mut CapyProjectTask {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return std::ptr::null_mut();
    };
    app.perform(|app| {
        if opening != 3 && !placement.is_null() { return Err("Only image placement accepts a drop target".into()); }
        let admission = app.window.documents.admission(&app.host.session.retained_document_tiles());
        let options = app.host.renderer_options(app.metal.cache.clone());
        let session = &mut app.host.session;
        let epoch = session.state().document_file.epoch;
        let mut save_request = None;
        let payload = if opening == 0 {
            let id = session
                .state()
                .requests
                .iter()
                .find_map(|r| match r.kind {
                    HostRequestKind::Document {
                        request: DocumentRequest::Save { .. },
                    } => Some(r.id),
                    _ => None,
                })
                .ok_or("No save request is pending")?;
            save_request = Some(id);
            Payload::Save {
                snapshot: Some(session.capture_project_save(
                    id,
                    DocumentLocation {
                        uri: "apple:pending".into(),
                        name: session.state().document_file.title().into(),
                    },
                )?),
                project: None,
                gpu: session.engine().backend().0.as_ref().map(|renderer| renderer.snapshot_gpu()),
            }
        } else if opening == 4 {
            Payload::Color(Box::new(ColorTask::capture(session, None, DISPLAY_SPACE)?))
        } else if opening == 5 {
            Payload::Info(layer_color::DocumentInfo::capture(session.engine().document()))
        } else if opening == 6 {
            Payload::Source(Box::new(SourceTask::capture(session, None, DISPLAY_SPACE)?))
        } else if opening == 7 {
            let request = session.state().requests.iter().find(|r| matches!(r.kind,
                HostRequestKind::Document { request: DocumentRequest::ImportLookup { .. } }))
                .ok_or("No color lookup import is pending")?.id;
            Payload::Lookup { resource: None, request }
        } else if opening == 8 {
            let request = session.state().requests.iter().find(|r| matches!(r.kind,
                HostRequestKind::Document { request: DocumentRequest::Copy { .. } }))
                .ok_or("No copy is pending")?.id;
            Payload::Clip { task: Some(Box::new(ClipTask::capture(session, request)?)), clip: None, publication: None, request, operation:session.document_request(request)?.clone() }
        } else if opening == 3 {
            #[derive(Default, serde::Deserialize)]
            struct Placement { screen: Option<layer_core::Point>, layer: Option<Row>, nonce: Option<String> }
            #[derive(serde::Deserialize)]
            struct Row { target: u64, fraction: f32 }
            let placement: Placement = if placement.is_null() { Default::default() } else {
                serde_json::from_str(unsafe { read_title(placement) }?).map_err(|e| e.to_string())?
            };
            if placement.screen.is_some() && placement.layer.is_some() { return Err("Choose one image drop target".into()); }
            let destination = placement.layer.map(|row| {
                let position = session.image_layer_drop_hint(row.target, row.fraction)
                    .ok_or("Images cannot be placed at that layer position")?;
                Ok::<_, String>(layer_ui::ImageLayerDestination { target: layer_ui::occurrence_handle(row.target)?, position })
            }).transpose()?;
            let context = session.image_placement_context(placement.screen, destination)?;
            let request = session.state().requests.iter().find(|r| matches!(r.kind,
                HostRequestKind::Document { request: DocumentRequest::Place | DocumentRequest::Paste { .. } }))
                .ok_or("No image import is pending")?.id;
            let device = session.engine().backend().0.as_ref()
                .ok_or("Wait for the canvas to finish starting")?.device().clone();
            Payload::Placed {
                images: layer_ui::ImageImportBatch::new(session.state().settings.photo_open,
                    session.engine().document().composition().color.space, Default::default()),
                context, request, device,
                color: session.engine().document().composition().color,
                mode: match session.document_request(request)? { DocumentRequest::Paste { mode } => Some(*mode), _ => None },
                environment: session.pasting_new_image().then(|| OpenEnvironment::capture(session, admission, options)).transpose()?,
                clip: placement.nonce.map(|nonce| app.window.documents.clip.capture(&nonce, session.localization())).transpose()?,
            }
        } else if opening == 1 {
            session.require_document_idle()?;
            Payload::Open {
                environment: Some(OpenEnvironment::capture(session, admission, options)?),
                candidate: None,
                source: layer_ui::ImportSource::Master,
                imported: None,
            }
        } else {
            return Err("Unknown project task kind".into());
        };
        let task = CapyProjectTask::new(
            payload,
            epoch,
            session.engine().document().revision,
            save_request,
            session.localization().clone(),
        );
        if opening == 0 {
            unsafe { &*task }.state.lock().unwrap_or_else(|e| e.into_inner()).expectation = session.save_destination_expectation();
        }
        Ok(task)
    })
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// Session owner only. Used as a barrier before a host decides whether to close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_ready(app: *mut CapyApple) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|app| {
        if app.host.session.state().filter_load.pending {
            return Ok(1);
        }
        app.host.session.require_document_idle()?;
        Ok(0)
    })
    .unwrap_or(-1)
}

/// # Safety
/// Owner only. Submit queued input without a drawable, then validate a committed
/// snapshot. An active stroke can retain its preceding committed raster. This
/// is preparation only: the caller must capture a project job, wait for its
/// file worker, and atomically publish recovery before acknowledging durability.
/// Returns 1 while queued input or a non-capturable canvas operation remains.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_prepare_recovery(app: *mut CapyApple, now: u64) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|app| {
        app.metal.observe_failure(&mut app.host, true);
        if app.host.session.engine().backend().0.is_some() {
            // A failed GPU is suspended with its surviving CPU state. Recovery
            // still proceeds; the shared snapshot/worker validates the pixels.
            let _ = app.gpu_operation(|app| app.host.prepare_canvas_frame(now, now, true));
        }
        let renderer_ready = app.host.session.engine().backend().0.is_none()
            || (app.host.startup.canvas_ready
                && !app.host.session.engine().has_pending_document_edits());
        Ok(i32::from(
            !renderer_ready || app.host.session.engine().has_pending_input()
                || app.host.session.capture_artwork().is_err(),
        ))
    })
    .unwrap_or(-1)
}

/// # Safety
/// The task must remain alive. Compare the UI's approved document with the
/// actual owner capture so intervening edits cannot bypass an unsaved prompt.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_matches(
    task: *const CapyProjectTask,
    epoch: u64,
    revision: u64,
) -> i32 {
    unsafe { task.as_ref() }.map_or(0, |t| i32::from(t.epoch == epoch && t.revision == revision))
}
fn host_file(fd: i32) -> ManuallyDrop<File> {
    ManuallyDrop::new(unsafe { File::from_raw_fd(fd) })
}

/// # Safety
/// Worker only. fd is an open, exclusively accessed descriptor at offset zero
/// and stays host-owned. The host fsyncs and atomically replaces on success.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_write(task: *const CapyProjectTask, fd: i32) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else {
        return -1;
    };
    let mut fingerprint = None;
    let result = task.perform(|payload| {
        if fd < 0 {
            return Err("Missing project output".into());
        }
        let file = host_file(fd);
        let stream = layer_core::Cancellable { inner: &*file, cancelled: || task.check_cancelled().is_err() };
        let localization = &task.localization;
        match payload {
            Payload::Save { snapshot, project, gpu } => {
                if let Some(snapshot) = snapshot.take() {
                    *project = Some(snapshot);
                }
                let capture = project.as_ref().ok_or("Missing project snapshot")?;
                let preview = gpu.as_ref().and_then(|gpu| gpu.package_preview(capture, task.control.cancellation_flag()));
                let package = PreparedPackage::prepare(capture, preview, task.control.cancellation_flag())?;
                let mut writer = layer_ui::FingerprintWriter::new(stream);
                package.write(&mut writer, task.control.cancellation_flag())?;
                fingerprint = Some(writer.finish());
                Ok(())
            }
            Payload::Package(view) => view.copy_original(&mut {stream}, task.control.cancellation_flag()),
            Payload::Color(color) => color.write_copy(stream, task.control.is_cancelled()),
            Payload::Export(export) => export.write(stream, task.control.clone()).map_err(|reason| reason.message(localization)),
            _ => Err("Not a write task".into()),
        }
    });
    if result == 0 { task.state.lock().unwrap_or_else(|e| e.into_inner()).fingerprint = fingerprint; }
    result
}

/// # Safety
/// File worker immediately before atomic publication. The host retains a
/// coordinated read descriptor for the current destination, or -1 if missing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_destination_matches(task: *const CapyProjectTask, fd: i32, uri: *const c_char) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    let expectation = task.state.lock().unwrap_or_else(|e| e.into_inner()).expectation.clone();
    let Some(expectation) = expectation else { return 0; };
    let result = (|| -> Result<(),String> {
        if expectation.location.uri != unsafe { read_title(uri) }? { return Ok(()); }
        let observed = if fd < 0 { None } else { Some(layer_ui::DestinationFingerprint::read(&*host_file(fd))?) };
        if expectation.matches(observed.as_ref()) { Ok(()) }
        else { Err("The saved drawing changed outside Capy Canvas. Save a copy to keep both versions.".into()) }
    })();
    match result {
        Ok(()) => 0,
        Err(error) => { task.state.lock().unwrap_or_else(|e| e.into_inner()).error = Some(error); -1 }
    }
}

/// # Safety
/// Worker only. The task and host-owned descriptor remain alive through writing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_export_preview(task: *const CapyProjectTask, fd: i32) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    task.perform(|payload| {
        if fd < 0 { return Err("Missing preview output".into()); }
        let file = host_file(fd);
        let Payload::Package(view) = payload else { return Err("No retained drawing preview".into()); };
        view.export_preview(&mut &*file, task.control.cancellation_flag())
    })
}

enum Input<'a> {
    New(Option<layer_ui::NewDocumentOptions>),
    File(i32),
    Bytes(&'a [u8]),
    Assume(layer_core::color::ColorProfile),
}

/// # Safety
/// Worker only, after all clipboard images and profile choices have been read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_finish_images(task: *const CapyProjectTask) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    task.perform(|payload| {
        let Payload::Placed { images, environment, clip, color, mode, .. } = payload else { return Err("Not an image import".into()); };
        let Some(environment) = environment.take() else {
            if *mode != Some(layer_ui::PasteMode::Into) && let Some(clip) = clip {
                clip.convert_layers(*color, layer_color::photo::PhotoMemoryBudget::current().encode_bytes, || task.control.is_cancelled())?;
            }
            return Ok(());
        };
        let document = match clip.take() {
            Some(clip) => clip.document(&environment.localization)?,
            None => layer_ui::clipboard_document(images.take_sources(task.control.is_cancelled())?, environment.photo_policy, &environment.localization)?,
        };
        let candidate = environment.prepare(document, || task.check_cancelled().is_err())?;
        *payload = Payload::Open { environment: None, candidate: Some(candidate), source: layer_ui::ImportSource::Photo, imported: None };
        Ok(())
    })
}
/// Shared decoder capabilities drive native picker and clipboard preferences.
#[unsafe(no_mangle)]
pub extern "C" fn capy_photo_formats() -> *mut c_char {
    CString::new(serde_json::to_string(&layer_color::photo::formats().collect::<Vec<_>>()).unwrap())
        .unwrap().into_raw()
}
/// # Safety
/// Worker only. Read/validate/prepare before adoption. fd == -1 uses captured
/// New defaults. Other fds remain caller-owned; name is NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_read(task: *const CapyProjectTask, fd: i32, name: *const c_char) -> i32 {
    let fingerprint = if fd >= 0 {
        let file = host_file(fd);
        layer_ui::DestinationFingerprint::read(&*file).and_then(|fingerprint| {
            (&*file).seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            Ok(Some(fingerprint))
        })
    } else { Ok(None) };
    let input = fingerprint.as_ref().map_err(Clone::clone).and_then(|_| match fd {
        -1 => Ok(Input::New(None)), n if n >= 0 => Ok(Input::File(n)), _ => Err("Missing project input".into()),
    });
    let result = unsafe { prepare_project(task, input, read_title(name)) };
    if result == 0 && let Some(task) = (unsafe { task.as_ref() }) {
        task.state.lock().unwrap_or_else(|e| e.into_inner()).fingerprint = fingerprint.ok().flatten();
    }
    result
}
/// # Safety
/// Worker only; bytes and UTF-8 name remain readable until this call returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_read_bytes(task: *const CapyProjectTask, bytes: *const u8, count: usize, name: *const c_char) -> i32 {
    let input = if bytes.is_null() || count == 0 || count > isize::MAX as usize {
        Err("The clipboard contains no image data".into())
    } else { Ok(Input::Bytes(unsafe { std::slice::from_raw_parts(bytes, count) })) };
    unsafe { prepare_project(task, input, read_title(name)) }
}
/// # Safety
/// Worker only; options is NUL-terminated NewDocumentOptions JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_new(task: *const CapyProjectTask, options: *const c_char) -> i32 {
    let options = unsafe { read_title(options) }.and_then(|text| {
        serde_json::from_str(text).map(|value| Input::New(Some(value))).map_err(|e| e.to_string())
    });
    unsafe { prepare_project(task, options, Ok("Untitled")) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_package_summary(task:*const CapyProjectTask)->*mut c_char {
    let Some(task) = (unsafe {task.as_ref()}) else{return std::ptr::null_mut();};
    let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
    let summary = match &state.payload {
        Payload::Package(view) => serde_json::to_string(&view.summary(&task.localization)),
        _ => Ok("null".into()),
    };
    summary.ok().and_then(|value| CString::new(value).ok()).map_or(std::ptr::null_mut(),CString::into_raw)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_package_preview(task:*const CapyProjectTask,output:*mut CapyProjectPreview)->i32 {
    let (Some(task),Some(output)) = (unsafe {task.as_ref()},unsafe {output.as_mut()}) else{return -1;};
    let state = task.state.lock().unwrap_or_else(|e|e.into_inner());
    let Payload::Package(view) = &state.payload else{return -1;};
    if let Some(preview) = view.preview() {
        let [width,height] = preview.size();
        *output = CapyProjectPreview {width,height,pixels:preview.pixels().as_ptr(),count:preview.pixels().len()};
    } else {*output = CapyProjectPreview {width:0,height:0,pixels:std::ptr::null(),count:0};}
    0
}

/// # Safety
/// Task remains alive; returns owned JSON interpretation, or JSON null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_profile(task: *const CapyProjectTask) -> *mut c_char {
    let Some(task) = (unsafe { task.as_ref() }) else { return std::ptr::null_mut(); };
    let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
    let source = match &state.payload {
        Payload::Open { imported: Some(imported), environment: Some(environment), .. } =>
            imported.interpretation_required(environment.photo_policy).map(|s| &s.interpretation),
        Payload::Placed { images, .. } => images.pending_source().map(|s| &s.interpretation),
        _ => None,
    };
    CString::new(serde_json::to_string(&source).unwrap()).unwrap().into_raw()
}
/// # Safety
/// Worker only; profile is NUL-terminated ColorProfile JSON. Invalid choices
/// retain the pending source for retry without rereading or altering the file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_assume_profile(task: *const CapyProjectTask, profile: *const c_char) -> i32 {
    let input = unsafe { read_title(profile) }.and_then(|text| {
        serde_json::from_str(text).map(Input::Assume).map_err(|e| e.to_string())
    });
    unsafe { prepare_project(task, input, Ok("Photo")) }
}
unsafe fn prepare_project(task: *const CapyProjectTask, input: Result<Input<'_>, String>, name: Result<&str, String>) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    task.perform(|payload| {
        if let Payload::Lookup { resource, .. } = payload {
            let mut bytes = Vec::new();
            match input? {
                Input::File(fd) => { (&*host_file(fd)).take(layer_core::Lut3d::MAX_TEXT_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?; }
                Input::Bytes(data) => bytes.extend_from_slice(&data[..data.len().min(layer_core::Lut3d::MAX_TEXT_BYTES + 1)]),
                _ => return Err("Choose a color lookup table".into()),
            }
            task.check_cancelled()?;
            *resource = Some(std::sync::Arc::new(layer_core::Lut3d::parse_cube_named(&bytes, name?)?));
            return Ok(());
        }
        if let Payload::Placed { images, mode, .. } = payload {
            let interpreting = matches!(&input, Ok(Input::Assume(_)));
            let result = (|| {
                let name = name?;
                let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
                match input? {
                    Input::File(fd) => {
                        let file = host_file(fd);
                        images.read_candidate(layer_core::Cancellable { inner: &*file, cancelled: || task.check_cancelled().is_err() },
                            stem, task.control.cancellation_flag())
                    }
                    Input::Bytes(bytes) => images.read_candidate(Cursor::new(bytes), stem, task.control.cancellation_flag()),
                    Input::Assume(profile) => images.interpret(profile, task.control.is_cancelled()),
                    Input::New(_) => Err("Choose images to place".into()),
                }
            })();
            if result.is_err() && !interpreting && mode.is_none() { images.invalidate(); }
            return result;
        }
        let input = input?;
        let name = name?;
        let Payload::Open { environment, candidate, source, imported } = payload else {
            return Err("Not an open task".into());
        };
        let context = environment.as_ref().ok_or("This open task has already run")?;
        match input {
            Input::New(options) => *imported = Some(layer_ui::ImportedDocument::new(
                options.unwrap_or(context.new_options).project(&context.localization)?,
                layer_ui::ImportSource::Master,
            )),
            Input::File(fd) => {
                let file = host_file(fd);
                let outcome = context.read(layer_core::Cancellable { inner: &*file, cancelled: || task.check_cancelled().is_err() },
                    layer_ui::ImportIntent::Open, name, task.control.cancellation_flag())?;
                match outcome {
                    layer_ui::ImportOutcome::Editable(value) => *imported = Some(value),
                    layer_ui::ImportOutcome::Package(value) => {*payload = Payload::Package(layer_ui::PackageView::new(value)?);return Ok(());}
                }
            }
            Input::Bytes(bytes) => match context.read(Cursor::new(bytes),
                layer_ui::ImportIntent::Open,name,task.control.cancellation_flag())? {
                layer_ui::ImportOutcome::Editable(value) => *imported = Some(value),
                layer_ui::ImportOutcome::Package(value) => {*payload = Payload::Package(layer_ui::PackageView::new(value)?);return Ok(());}
            },
            Input::Assume(profile) => imported.as_mut().ok_or("No image interpretation is pending")?.interpret(profile)?,
        }
        task.check_cancelled()?;
        let ready = imported.as_ref().ok_or("Document preparation is incomplete")?;
        if ready.interpretation_required(context.photo_policy).is_some() { return Ok(()); }
        *source = ready.source;
        let imported = imported.take().unwrap();
        let environment = environment.take().unwrap();
        match environment.prepare(imported.project.clone(), || task.check_cancelled().is_err()) {
            Ok(prepared) => *candidate = Some(prepared),
            Err(reason) => {
                task.check_cancelled()?;
                let Some(outcome) = imported.preserve_unsupported(reason.clone()) else { return Err(reason); };
                *payload = Payload::Package(layer_ui::PackageView::new(outcome)?);
            }
        }
        Ok(())
    })
}

/// # Safety
/// Session owner only, after a successful worker read. Failure preserves the
/// original editor. Success retains the retired editor in the task for worker
/// destruction, avoiding document/resource teardown on the drawing queue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_prepare_adopt(app:*mut CapyApple,task:*const CapyProjectTask,now:u64)->i32 {
    let (Some(a),Some(t))=(unsafe{app.as_mut()},unsafe{task.as_ref()}) else{return -1};
    let opening=matches!(t.state.lock().unwrap_or_else(|e|e.into_inner()).payload,Payload::Open{..});
    if !opening {return 0;}
    if a.host.session.state().document_file.epoch!=t.epoch || a.host.session.engine().document().revision!=t.revision {return 0;}
    let ready=unsafe{capy_apple_prepare_recovery(app,now)};
    if ready!=0 {return ready;}
    a.perform(|a|Ok(i32::from(a.host.session.retained_document_tiles().try_blobs()?.is_none()))).unwrap_or(-1)
}
/// # Safety
/// Serial owner, after worker preparation and outgoing backing completion.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_adopt(
    app: *mut CapyApple,
    task: *const CapyProjectTask,
    title: *const c_char,
    uri: *const c_char,
) -> i32 {
    let (Some(app), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else {
        return -1;
    };
    app.perform(|app| {
        task.check_cancelled()?;
        let title = unsafe { read_title(title) }?;
        let uri = unsafe { read_title(uri) }?;
        let location = (!uri.is_empty()).then(|| DocumentLocation {
            uri: uri.into(),
            name: title.into(),
        });
        let mut state = task.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        if let Payload::Color(color) = &mut state.payload {
            return color.adopt(&mut app.host, task.control.is_cancelled(), || unsafe { capy_project_begin_commit(task) } >= 0);
        }
        if let Payload::Source(source) = &mut state.payload {
            return source.adopt(&mut app.host, task.control.is_cancelled(), || unsafe { capy_project_begin_commit(task) } >= 0);
        }
        if let Payload::Clip { clip, publication, request, .. } = &mut state.payload {
            return clipboard::adopt_clip(app, task, clip, publication.ok_or("The copy was not published")?, *request);
        }
        if let Payload::Lookup { resource, request } = &mut state.payload {
            let session = &mut app.host.session;
            let previous = session.state().revision;
            let result = session.apply_lookup(*request, resource.take().ok_or("The color lookup table is not prepared")?);
            let change = session.complete_document_request(*request, result)?;
            app.host.apply_change(previous, change);
            return Ok(());
        }
        if let Payload::Placed { images, context, request, device, clip, .. } = &mut state.payload {
            let session = &mut app.host.session;
            session.validate_image_placement(context)?;
            if session.renderer_mut().0.as_ref().map(|gpu| gpu.device()) != Some(device)
                || !session.state().requests.iter().any(|r| r.id == *request) {
                return Err("The canvas or import request changed; try again".into());
            }
            if unsafe { capy_project_begin_commit(task) } < 0 { return Err("Document operation cancelled".into()); }
            let previous = session.state().revision;
            let mode = match session.document_request(*request)? { DocumentRequest::Paste { mode } => Some(*mode), _ => None };
            if let (Some(clip), Some(mode)) = (clip.as_ref(), mode) { session.paste_clip(clip, mode)?; } else {
                let sources = images.take_sources(task.control.is_cancelled())?;
                match mode {
                    Some(mode) => session.paste_layer_sources(sources, mode, context)?,
                    None => session.place_layer_sources(sources, context.center, context.destination)?,
                }
            }
            let mut change = session.complete_document_request(*request, Ok(true))?;
            change.canvas_wake = true;
            change.regions |= layer_ui::regions::ALL;
            app.host.apply_change(previous, change);
            return Ok(());
        }
        let Payload::Open { candidate, source, .. } = &mut state.payload else {
            return Err("Not an open task".into());
        };
        let location = source.adoption_location(location);
        let fingerprint_location = location.clone();
        let open = OpenAdoption { epoch: task.epoch, revision: task.revision, location };
        let retired = app.window.adopt(&mut app.host, candidate, open,
            || unsafe { capy_project_begin_commit(task) } >= 0, Box::new)?;
        app.document_retired();
        if let (Some(location), Some(fingerprint)) = (fingerprint_location, state.fingerprint.clone()) {
            app.host.session.record_destination_fingerprint(&location, fingerprint)?;
        }
        state.payload=Payload::Retired {_renderer:retired};
        Ok(())
    })
    .map_or(-1, |_| 0)
}

/// # Safety
/// Session owner only, after the host has durably committed a successful save.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_saved(
    app: *mut CapyApple,
    task: *const CapyProjectTask,
    title: *const c_char,
    uri: *const c_char,
) -> i32 {
    let (Some(app), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else {
        return -1;
    };
    app.perform(|app| {
        task.check_cancelled()?;
        let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = &state.error {
            return Err(e.clone());
        }
        if !matches!(
            state.payload,
            Payload::Save {
                project: Some(_),
                ..
            }
        ) {
            return Err("Save did not complete".into());
        }
        let session = &mut app.host.session;
        if session.state().document_file.epoch != task.epoch {
            return Err("The save belongs to a different document".into());
        }
        let id = task.save_request.ok_or("Not a save task")?;
        let location = DocumentLocation {
                uri: unsafe { read_title(uri) }?.into(),
                name: unsafe { read_title(title) }?.into(),
            };
        session.retarget_project_save(id, location.clone())?;
        session.complete_document_request(id, Ok(true))?;
        if let Some(fingerprint) = state.fingerprint.clone() { session.record_destination_fingerprint(&location, fingerprint)?; }
        Ok(())
    })
    .map_or(-1, |_| 0)
}
pub(crate) unsafe fn read_title<'a>(title: *const c_char) -> Result<&'a str, String> {
    if title.is_null() {
        return Err("Missing document title".into());
    }
    unsafe { CStr::from_ptr(title) }
        .to_str()
        .map_err(|_| "Invalid document title".into())
}
/// # Safety
/// Task must stay alive for the call. Safe to cancel concurrently with a worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_cancel(task: *const CapyProjectTask) {
    if let Some(task) = unsafe { task.as_ref() } {
        if task.phase.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            task.control.cancel();
        }
    }
}
/// # Safety
/// Call just before atomic publication. Cancellation wins before this point;
/// after it, publication finishes and cancellation cannot retract the save.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_begin_commit(task: *const CapyProjectTask) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else {
        return -1;
    };
    match task
        .phase
        .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
    {
        Ok(_) | Err(2) => 0,
        Err(_) => -1,
    }
}
/// # Safety
/// Call after the worker finishes. Returns an owned string, or NULL on success.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_error(task: *const CapyProjectTask) -> *mut c_char {
    let Some(task) = (unsafe { task.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
    state
        .error
        .as_ref()
        .and_then(|s| CString::new(s.replace('\0', " ")).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}
/// # Safety
/// No outstanding calls. Free on a worker: retired documents can be large.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_free(task: *mut CapyProjectTask) {
    if !task.is_null() {
        drop(unsafe { Box::from_raw(task) });
    }
}

/// # Safety
/// Session owner only. Complete a native picker/worker request after its effect.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_complete(
    app: *mut CapyApple,
    id: u32,
    succeeded: u32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|app| {
        app.host
            .session
            .complete_document_request(id, Ok(succeeded != 0))
            .map(|_| ())
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// Session owner only. 0 requests close, 1 saves, 2 discards, 3 cancels,
/// 4 clears a close authorization when an application termination is cancelled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_close(
    app: *mut CapyApple,
    id: u32,
    decision: u32,
) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|app| {
        let session = &mut app.host.session;
        match decision {
            0 => {
                session.request_document_close()?;
            }
            1..=3 => {
                session.respond_document_close(
                    id,
                    match decision {
                        1 => CloseDecision::Save,
                        2 => CloseDecision::Discard,
                        _ => CloseDecision::Cancel,
                    },
                )?;
            }
            4 => session.reset_document_close(),
            _ => return Err("Unknown close decision".into()),
        }
        Ok(())
    })
    .map_or(-1, |_| 0)
}

/// # Safety
/// Owner only. Returns 0 while shaders/document replay prepare, 1 with a job,
/// or -1 on failure. The job contains only an independently owned GPU snapshot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_export_task(
    app: *mut CapyApple,
    id: u32,
    now: u64,
    output: *mut *mut CapyProjectTask,
) -> i32 {
    let (Some(app), Some(output)) = (unsafe { app.as_mut() }, unsafe { output.as_mut() }) else {
        return -1;
    };
    *output = std::ptr::null_mut();
    app.perform(|app| {
        app.host.session.require_document_idle()?;
        if !app.host.session.state().requests.iter().any(|r| {
            r.id == id
                && matches!(
                    r.kind,
                    HostRequestKind::Document {
                        request: DocumentRequest::Export { .. }
                    }
                )
        }) {
            return Err("No export request is pending".into());
        }
        let host = &mut app.host;
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("capy-export".into())
                .stack_size(8 * 1024 * 1024)
                .spawn_scoped(scope, || host.prepare_canvas_frame(now, now, true))
                .map_err(|e| e.to_string())?
                .join()
                .map_err(|_| "Export preparation failed".to_string())?
        })?;
        if !app.host.startup.canvas_ready || app.host.session.engine().has_pending_document_edits()
        {
            return Ok(0);
        }
        let session = &mut app.host.session;
        let epoch = session.state().document_file.epoch;
        let revision = session.engine().document().revision;
        let export = ExportTask::capture(session, id, "", DISPLAY_SPACE)?;
        *output = CapyProjectTask::new(
            Payload::Export(Box::new(export)),
            epoch,
            revision,
            None,
            session.localization().clone(),
        );
        Ok(1)
    })
    .unwrap_or(-1)
}
