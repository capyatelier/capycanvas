//! Transfer jobs bridge the serial editor owner and a background file worker.
//! Jobs never retain a session pointer. File descriptors/URLs stay host-owned.
use super::*;
use layer_core::{Project, ProjectLimits};
use layer_host::Renderer;
use layer_render::{CanvasRenderer, EffectValidationRequest};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{CloseDecision, DocumentLocation, DocumentRequest, HostRequestKind, UiSession};
use std::{
    fs::File,
    io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom, Write},
    mem::ManuallyDrop,
    os::fd::FromRawFd,
    sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

#[path = "project_color.rs"]
mod color;
pub use color::*;
#[path = "project_source.rs"]
mod source;
#[path = "project_inspection.rs"]
mod inspection;
#[path = "project_export.rs"]
mod export;
pub use export::*;
#[path = "project_preferences.rs"]
mod preferences;
pub use preferences::*;

struct Environment {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    viewport: [u32; 2],
    brush: layer_core::BrushSnapshot,
    new_options: layer_ui::NewDocumentOptions,
    photo_policy: layer_ui::PhotoOpenPolicy,
    place: Option<(layer_core::LayerId, u32)>,
    working_space: layer_core::color::RgbSpace,
}
enum Payload {
    Color(Box<color::Task>),
    Source(Box<source::Task>),
    Info(layer_color::DocumentInfo),
    Inspection(Box<inspection::Task>),
    Save {
        snapshot: Option<Project>,
        project: Option<Project>,
    },
    Open {
        environment: Option<Environment>,
        candidate: Option<Box<UiSession<Renderer>>>,
        photo: bool,
        pending_photo: Option<(String, layer_core::color::source::SourceImage)>,
    },
    Placed {
        source: Option<layer_core::color::source::SourceImage>,
        name: String,
        target: layer_core::LayerId,
        request: u32,
        device: wgpu::Device,
    },
    Export(Box<export::Task>),
    Retired {
        _session: Box<UiSession<Renderer>>,
    },
}
struct State {
    payload: Payload,
    error: Option<String>,
}
pub struct CapyProjectTask {
    state: Mutex<State>,
    phase: AtomicU8,
    control: layer_render_wgpu::snapshot::CaptureControl,
    epoch: u64,
    revision: u64,
    save_request: Option<u32>,
}
impl CapyProjectTask {
    fn new(payload: Payload, epoch: u64, revision: u64, save_request: Option<u32>) -> *mut Self {
        Box::into_raw(Box::new(Self {
            state: Mutex::new(State {
                payload,
                error: None,
            }),
            phase: AtomicU8::new(0),
            control: Default::default(),
            epoch,
            revision,
            save_request,
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
        // Dispatch workers have a small stack. Recursive WGSL translation can
        // exceed it even for built-in shaders in debug builds. Join a scoped
        // worker so the borrowed descriptor/job remain valid until completion.
        let result = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("capy-project".into())
                .stack_size(8 * 1024 * 1024)
                .spawn_scoped(scope, || self.perform_inner(work))
                .map_err(|e| e.to_string())?
                .join()
                .map_err(|_| "Document worker failed".to_string())
        });
        match result {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_task(
    app: *mut CapyApple,
    opening: u32,
) -> *mut CapyProjectTask {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return std::ptr::null_mut();
    };
    app.perform(|app| {
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
            }
        } else if opening == 2 {
            Payload::Save {
                snapshot: Some(session.capture_project_recovery()?),
                project: None,
            }
        } else if opening == 4 {
            Payload::Color(Box::new(color::Task::capture(session)?))
        } else if opening == 5 {
            Payload::Info(layer_color::DocumentInfo::capture(session.engine().document()))
        } else if opening == 6 {
            Payload::Source(Box::new(source::Task::capture(session)?))
        } else if opening == 7 {
            Payload::Inspection(Box::new(inspection::Task::capture(session)?))
        } else if opening == 1 || opening == 3 {
            session.require_document_idle()?;
            let place = if opening == 3 {
                let request = session.state().requests.iter().find(|r| matches!(r.kind,
                    HostRequestKind::Document { request: DocumentRequest::Place | DocumentRequest::Paste }))
                    .ok_or("No image import is pending")?;
                Some((session.engine().document().active_target(), request.id))
            } else { None };
            let gpu = session
                .engine()
                .backend()
                .0
                .as_ref()
                .ok_or("Wait for the canvas to finish starting")?;
            Payload::Open {
                environment: Some(Environment {
                    adapter: gpu.adapter().clone(),
                    device: gpu.device().clone(),
                    queue: gpu.queue().clone(),
                    viewport: session.state().camera.viewport,
                    brush: session.engine().configured_brush().clone(),
                    new_options: session.state().settings.new_document.defaults,
                    photo_policy: session.state().settings.photo_open,
                    working_space: session.engine().document().color.space,
                    place,
                }),
                candidate: None,
                photo: false,
                pending_photo: None,
            }
        } else {
            return Err("Unknown project task kind".into());
        };
        Ok(CapyProjectTask::new(
            payload,
            epoch,
            session.engine().document().revision,
            save_request,
        ))
    })
    .unwrap_or(std::ptr::null_mut())
}

struct Stream<'a> {
    file: ManuallyDrop<File>,
    task: &'a CapyProjectTask,
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
                || app.host.session.capture_project_recovery().is_err(),
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
impl Stream<'_> {
    fn cancelled(&self) -> std::io::Result<()> {
        self.task.check_cancelled().map_err(std::io::Error::other)
    }
}
impl Read for Stream<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.cancelled()?;
        self.file.read(bytes)
    }
}
impl Seek for Stream<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.cancelled()?;
        self.file.seek(position)
    }
}
impl Write for Stream<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.cancelled()?;
        self.file.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// # Safety
/// Worker only. fd is an open, exclusively accessed descriptor at offset zero
/// and stays host-owned. The host fsyncs and atomically replaces on success.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_write(task: *const CapyProjectTask, fd: i32) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else {
        return -1;
    };
    task.perform(|payload| {
        if fd < 0 {
            return Err("Missing project output".into());
        }
        let stream = Stream {
            file: ManuallyDrop::new(unsafe { File::from_raw_fd(fd) }),
            task,
        };
        match payload {
            Payload::Save { snapshot, project } => {
                if let Some(snapshot) = snapshot.take() {
                    *project = Some(snapshot.pruned()?);
                }
                project
                    .as_ref()
                    .ok_or("Missing project snapshot")?
                    .write(stream)
            }
            Payload::Color(color) => color.write_copy(stream),
            Payload::Export(export) => export.write(stream, task.control.clone()),
            _ => Err("Not a write task".into()),
        }
    })
}

enum Input<'a> {
    New(Option<layer_ui::NewDocumentOptions>),
    File(i32),
    Bytes(&'a [u8]),
    Assume(layer_core::color::ColorProfile),
}
enum Decoded {
    Project(Project),
    Photo(layer_core::color::source::SourceImage),
}
fn decode(mut input: impl BufRead + Seek, limits: ProjectLimits, place: bool) -> Result<Decoded, String> {
    if input.fill_buf().map_err(|e| e.to_string())?.starts_with(b"CAPY") {
        if place { return Err("Choose a PNG, TIFF or JPEG image to place".into()); }
        Project::read(input, limits).map(Decoded::Project)
    } else {
        layer_color::photo::read_photo(input, Default::default()).map(Decoded::Photo)
    }
}
/// # Safety
/// Worker only. Read/validate/prepare before adoption. fd == -1 uses captured
/// New defaults. Other fds remain caller-owned; name is NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_read(task: *const CapyProjectTask, fd: i32, name: *const c_char) -> i32 {
    let input = match fd { -1 => Ok(Input::New(None)), n if n >= 0 => Ok(Input::File(n)),
        _ => Err("Missing project input".into()) };
    unsafe { prepare_project(task, input, read_title(name)) }
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
/// # Safety
/// Task remains alive; returns owned JSON interpretation, or JSON null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_profile(task: *const CapyProjectTask) -> *mut c_char {
    let Some(task) = (unsafe { task.as_ref() }) else { return std::ptr::null_mut(); };
    let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
    let source = match &state.payload {
        Payload::Open { pending_photo: Some((_, source)), .. } => Some(&source.interpretation),
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
        let input = input?;
        let mut name = name?.to_owned();
        let Payload::Open { environment, candidate, photo, pending_photo } = payload else {
            return Err("Not an open task".into());
        };
        // Validate an explicit assumption before consuming the resumable job.
        let assumed = if let Input::Assume(profile) = &input {
            let (source_name, source) = pending_photo.as_ref().ok_or("No image interpretation is pending")?;
            name = source_name.clone();
            Some(layer_color::assume_source_profile(source.clone(), profile.clone())?)
        } else { None };
        let context = environment.take().ok_or("This open task has already run")?;
        let limits = ProjectLimits {
            dimension: context.device.limits().max_texture_dimension_2d.min(ProjectLimits::default().dimension),
            ..Default::default()
        };
        let decoded = match input {
            Input::New(options) => Decoded::Project(options.unwrap_or(context.new_options).project()?),
            Input::File(fd) => decode(BufReader::new(Stream {
                file: ManuallyDrop::new(unsafe { File::from_raw_fd(fd) }), task,
            }), limits, context.place.is_some())?,
            Input::Bytes(bytes) => decode(Cursor::new(bytes), limits, context.place.is_some())?,
            Input::Assume(_) => Decoded::Photo(assumed.unwrap()),
        };
        task.check_cancelled()?;
        let project = match decoded {
            Decoded::Project(project) => project,
            Decoded::Photo(source) => {
                *photo = true;
                if source.interpretation.profile_assumed
                    && context.photo_policy.missing_profile == layer_ui::MissingProfilePolicy::Ask {
                    *pending_photo = Some((name, source));
                    *environment = Some(context);
                    return Ok(());
                }
                pending_photo.take();
                let name: String = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem)
                    .chars().filter(|c| !c.is_control()).take(128).collect();
                let name = if name.trim().is_empty() { "Photo" } else { name.trim() };
                if let Some((target, request)) = context.place {
                    layer_color::WorkingDecoder::new(&source.interpretation, context.working_space, Default::default())?;
                    *payload = Payload::Placed { source: Some(source), name: name.into(), target, request, device: context.device };
                    return Ok(());
                }
                let depth = context.photo_policy.editing_depth(source.interpretation.depth);
                layer_color::photo_project(source, name, depth)?
            }
        };
        if context.place.is_some() { return Err("Choose an image to place".into()); }
        project.validate(limits)?;
        let environment = context;
        task.check_cancelled()?;
        let mut gpu = WgpuRasterizer::from_wgpu_native_staged(
            environment.adapter, environment.device, environment.queue, project.document.color,
        ).map_err(|e| e.to_string())?;
        gpu.configure_ui_previews(crate::DISPLAY_SPACE).map_err(|e| e.to_string())?;
        gpu.finish_startup_cache();
        let mut programs = Vec::new();
        for effect in project
            .document
            .layers
            .iter()
            .filter_map(|l| l.effect.as_ref())
        {
            if !programs.contains(&effect.program) {
                programs.push(effect.program.clone());
            }
        }
        let mut validating = !programs.is_empty();
        if validating {
            gpu.request_effect_validation(EffectValidationRequest {
                request_id: 1,
                namespace: programs.clone(),
                programs,
            })
            .map_err(|e| e.to_string())?;
        }
        // Start deferred compilation before waiting for validation. The worker
        // publishes only a ready native SDR canvas, including prepared opens.
        gpu.prepare_startup(&project.document, &environment.brush, false).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            task.check_cancelled()?;
            gpu.device().poll(wgpu::PollType::Poll).map_err(|e| e.to_string())?;
            if validating && let Some(result) = gpu.take_effect_validation() {
                result.result?;
                validating = false;
            }
            let ready = gpu.poll_startup().map_err(|e| e.to_string())?;
            if !validating && ready.canvas_ready && ready.brush_ready { break; }
            if Instant::now() >= deadline { return Err("Project canvas preparation timed out".into()); }
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut prepared =
            UiSession::from_project(Renderer(Some(gpu)), project, None, environment.viewport)?;
        prepared.frame(0, 0)?;
        task.check_cancelled()?;
        *candidate = Some(Box::new(prepared));
        Ok(())
    })
}

/// # Safety
/// Session owner only, after a successful worker read. Failure preserves the
/// original editor. Success retains the retired editor in the task for worker
/// destruction, avoiding document/resource teardown on the drawing queue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_adopt(
    app: *mut CapyApple,
    task: *const CapyProjectTask,
    title: *const c_char,
    uri: *const c_char,
) -> i32 {
    unsafe { adopt_project(app, task, title, uri, false) }
}

/// # Safety
/// Owner only after a successful worker read. Recovery retains unsaved status
/// and never treats the private archive as the user's save destination.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_recover(
    app: *mut CapyApple,
    task: *const CapyProjectTask,
) -> i32 {
    unsafe { adopt_project(app, task, c"Untitled".as_ptr(), c"".as_ptr(), true) }
}

unsafe fn adopt_project(
    app: *mut CapyApple,
    task: *const CapyProjectTask,
    title: *const c_char,
    uri: *const c_char,
    recovered: bool,
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
            if recovered { return Err("A color change is not a recovery drawing".into()); }
            return color.adopt(app, task);
        }
        if let Payload::Source(source) = &mut state.payload {
            if recovered { return Err("A source edit is not a recovery drawing".into()); }
            return source.adopt(app, task);
        }
        if let Payload::Placed { source, name, target, request, device } = &mut state.payload {
            if recovered { return Err("An image import is not a recovery drawing".into()); }
            let session = &mut app.host.session;
            if session.state().document_file.epoch != task.epoch
                || session.engine().document().revision != task.revision
                || session.engine().document().active_target() != *target
                || session.renderer_mut().0.as_ref().map(|gpu| gpu.device()) != Some(device)
                || !session.state().requests.iter().any(|r| r.id == *request) {
                return Err("The drawing or selected layer changed while importing; try again".into());
            }
            if unsafe { capy_project_begin_commit(task) } < 0 { return Err("Document operation cancelled".into()); }
            let previous = session.state().revision;
            session.import_layer_source(name, source.as_ref().ok_or("Image already placed")?.clone())?;
            source.take();
            let mut change = session.complete_document_request(*request, Ok(true))?;
            change.canvas_wake = true;
            app.host.apply_change(previous, change);
            return Ok(());
        }
        let Payload::Open { candidate, photo, .. } = &mut state.payload else {
            return Err("Not an open task".into());
        };
        if *photo && recovered { return Err("Recovery requires a native drawing".into()); }
        // Opening a photo never grants Save permission to overwrite its source.
        let location = if *photo { None } else { location };
        if candidate.as_ref().and_then(|s| s.engine().backend().0.as_ref()).map(|gpu| gpu.device())
            != app.host.session.engine().backend().0.as_ref().map(|gpu| gpu.device()) {
            return Err("The canvas changed while preparing this drawing; open it again".into());
        }
        let prepared = candidate
            .take()
            .ok_or("Project preparation is incomplete")?;
        if unsafe { capy_project_begin_commit(task) } < 0 {
            *candidate = Some(prepared);
            return Err("Document operation cancelled".into());
        }
        let result = if recovered {
            app.host
                .session
                .adopt_recovered_project(prepared, task.epoch, task.revision)
        } else {
            app.host
                .session
                .adopt_project(prepared, task.epoch, task.revision, location)
        };
        match result {
            Ok(retired) => state.payload = Payload::Retired { _session: retired },
            Err((error, prepared)) => {
                *candidate = Some(prepared);
                return Err(error);
            }
        }
        app.host.document_adopted();
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
        session.retarget_project_save(
            id,
            DocumentLocation {
                uri: unsafe { read_title(uri) }?.into(),
                name: unsafe { read_title(title) }?.into(),
            },
        )?;
        session.complete_document_request(id, Ok(true))?;
        Ok(())
    })
    .map_or(-1, |_| 0)
}
unsafe fn read_title<'a>(title: *const c_char) -> Result<&'a str, String> {
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
        app.host.prepare_canvas_frame(now, now, true)?;
        if !app.host.startup.canvas_ready || app.host.session.engine().has_pending_document_edits()
        {
            return Ok(0);
        }
        let session = &mut app.host.session;
        let epoch = session.state().document_file.epoch;
        let revision = session.engine().document().revision;
        let export = export::Task::capture(session, id)?;
        *output = CapyProjectTask::new(
            Payload::Export(Box::new(export)),
            epoch,
            revision,
            None,
        );
        Ok(1)
    })
    .unwrap_or(-1)
}
