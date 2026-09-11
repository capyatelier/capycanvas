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
    io::{Read, Write},
    mem::ManuallyDrop,
    os::fd::FromRawFd,
    sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

struct Environment {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    viewport: [u32; 2],
}
enum Payload {
    Save {
        snapshot: Option<Project>,
        project: Option<Project>,
    },
    Open {
        environment: Option<Environment>,
        candidate: Option<Box<UiSession<Renderer>>>,
    },
    Export {
        readback: Option<layer_render_wgpu::ExportReadback>,
        image: Option<layer_render::ReadbackImage>,
    },
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
        session.require_document_idle()?;
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
        } else if opening == 1 {
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
                }),
                candidate: None,
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
/// Owner only. Drain accepted input without acquiring/presenting a drawable so
/// a backgrounded or detached surface cannot strand the last pen-up batch.
/// Returns 1 while preparation or a live interaction still prevents capture.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_recovery_flush_input(app: *mut CapyApple, now: u64) -> i32 {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    app.perform(|app| {
        if app.host.session.engine().backend().0.is_some() {
            app.host.prepare_canvas_frame(now, now, true)?;
        }
        Ok(i32::from(
            app.host.session.require_document_idle().is_err()
                || app.host.session.state().filter_load.pending,
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
            Payload::Export { readback, image } => {
                if let Some(readback) = readback.take() {
                    *image = Some(readback.finish().map_err(|e| e.to_string())?);
                }
                task.check_cancelled()?;
                image
                    .as_ref()
                    .ok_or("Missing export pixels")?
                    .write_png(stream)
            }
            _ => Err("Not a write task".into()),
        }
    })
}

/// # Safety
/// Worker only. Read/validate/prepare the complete candidate before adoption.
/// fd == -1 prepares a new blank drawing. Other fds remain caller-owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_read(task: *const CapyProjectTask, fd: i32) -> i32 {
    unsafe { prepare_project(task, fd, layer_ui::DEFAULT_DOCUMENT_EXTENT) }
}
/// # Safety
/// Worker only; the task must be an unused open task. Dimensions follow shared policy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_new(
    task: *const CapyProjectTask,
    width: u32,
    height: u32,
) -> i32 {
    unsafe { prepare_project(task, -1, [width, height]) }
}
unsafe fn prepare_project(task: *const CapyProjectTask, fd: i32, extent: [u32; 2]) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else {
        return -1;
    };
    task.perform(|payload| {
        let Payload::Open {
            environment,
            candidate,
        } = payload
        else {
            return Err("Not an open task".into());
        };
        let environment = environment.take().ok_or("This open task has already run")?;
        let limits = ProjectLimits {
            dimension: environment
                .device
                .limits()
                .max_texture_dimension_2d
                .min(ProjectLimits::default().dimension),
            ..Default::default()
        };
        let project = if fd == -1 {
            layer_ui::new_drawing(extent[0], extent[1])?
        } else if fd < -1 {
            return Err("Missing project input".into());
        } else {
            Project::read(
                Stream {
                    file: ManuallyDrop::new(unsafe { File::from_raw_fd(fd) }),
                    task,
                },
                limits,
            )?
        };
        task.check_cancelled()?;
        // This eager constructor is confined to the background candidate. The
        // interactive canvas continues on its established staged renderer.
        #[allow(deprecated)]
        let mut gpu =
            WgpuRasterizer::from_wgpu(environment.adapter, environment.device, environment.queue)
                .map_err(|e| e.to_string())?;
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
        if !programs.is_empty() {
            gpu.request_effect_validation(EffectValidationRequest {
                request_id: 1,
                namespace: programs.clone(),
                programs,
            })
            .map_err(|e| e.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                task.check_cancelled()?;
                gpu.device()
                    .poll(wgpu::PollType::Poll)
                    .map_err(|e| e.to_string())?;
                if let Some(result) = gpu.take_effect_validation() {
                    result.result?;
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("Project shader preparation timed out".into());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
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
        let Payload::Open { candidate, .. } = &mut state.payload else {
            return Err("Not an open task".into());
        };
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
        let _ = task
            .phase
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
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
            return Err("No PNG export request is pending".into());
        }
        app.host.prepare_canvas_frame(now, now, true)?;
        if !app.host.startup.canvas_ready || app.host.session.engine().has_pending_document_edits()
        {
            return Ok(0);
        }
        let session = &mut app.host.session;
        let epoch = session.state().document_file.epoch;
        let revision = session.engine().document().revision;
        let gpu = session
            .renderer_mut()
            .0
            .as_mut()
            .ok_or("Canvas is unavailable")?;
        if !gpu.export_ready() {
            return Ok(0);
        }
        let readback = gpu
            .begin_export_readback(id as u64)
            .map_err(|e| e.to_string())?;
        *output = CapyProjectTask::new(
            Payload::Export {
                readback: Some(readback),
                image: None,
            },
            epoch,
            revision,
            None,
        );
        Ok(1)
    })
    .unwrap_or(-1)
}
