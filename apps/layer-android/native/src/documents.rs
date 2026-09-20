//! File workers own transfer jobs, never the live editor. Only adoption and
//! checkpoint acknowledgement return to the render Looper.
use crate::android::{app, error, fail, read};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jint, jlong},
};
use layer_core::{Project, ProjectLimits};
use layer_host::Renderer;
use layer_render::{CanvasRenderer, EffectValidationRequest};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{DocumentLocation, DocumentRequest, HostRequestKind, UiSession};
use std::{
    fs::File,
    io::{BufWriter, Write},
    os::fd::FromRawFd,
    time::{Duration, Instant},
};

struct Environment {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    viewport: [u32; 2],
    brush: layer_core::BrushSnapshot,
    cache: String,
    new_options: layer_ui::NewDocumentOptions,
    photo_policy: layer_ui::PhotoOpenPolicy,
    source_name: String,
    working_space: layer_core::color::RgbSpace,
    pending_photo: Option<layer_core::color::source::SourceImage>,
}
enum Payload {
    Save(Option<Project>),
    Export {
        gpu: layer_render_wgpu::snapshot::SnapshotGpu,
        snapshot: Option<layer_ui::DocumentExport>,
        recipe: layer_ui::ExportRecipe,
        control: layer_render_wgpu::snapshot::CaptureControl,
    },
    Open {
        environment: Option<Environment>,
        candidate: Option<Box<UiSession<Renderer>>>,
    },
    Placed {
        source: Option<layer_core::color::source::SourceImage>,
        name: String,
    },
    Retired {
        _session: Box<UiSession<Renderer>>,
    },
}
struct Task {
    epoch: u64,
    revision: u64,
    request: u32,
    recovered: bool,
    source: layer_ui::ImportSource,
    place: Option<layer_core::LayerId>,
    gpu_generation: u64,
    open_control: layer_render_wgpu::snapshot::CaptureControl,
    payload: Payload,
}
unsafe fn task<'a>(handle: jlong) -> &'a mut Task {
    unsafe { &mut *(handle as *mut Task) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    location: JString,
    epoch: jlong,
    revision: jlong,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let session = &mut a.host.session;
        let request = session
            .state()
            .requests
            .iter()
            .find_map(|r| match &r.kind {
                HostRequestKind::Document { request } if r.id == id as u32 => Some(request.clone()),
                _ => None,
            })
            .ok_or("The document request is no longer active")?;
        let place = matches!(request, DocumentRequest::Place | DocumentRequest::Paste)
            .then_some(session.engine().document().active_target());
        let payload = match request {
            DocumentRequest::Save { .. } => {
                let location: DocumentLocation =
                    serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
                Payload::Save(Some(session.capture_project_save(id as u32, location)?))
            }
            DocumentRequest::Open
            | DocumentRequest::New
            | DocumentRequest::Place
            | DocumentRequest::Paste => {
                session.require_document_idle()?;
                if session.state().document_file.epoch != epoch as u64
                    || session.engine().document().revision != revision as u64
                {
                    return Err("The document changed; review those changes before opening".into());
                }
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
                        cache: a.cache_directory.clone(),
                        new_options: session.state().settings.new_document.defaults,
                        photo_policy: session.state().settings.photo_open,
                        pending_photo: None,
                        working_space: session.engine().document().color.space,
                        source_name: serde_json::from_str::<Option<DocumentLocation>>(&read(
                            &mut env, &location,
                        )?)
                        .map_err(error)?
                        .map(|location| location.name)
                        .unwrap_or_else(|| "Photo".into()),
                    }),
                    candidate: None,
                }
            }
            _ => return Err("This request does not transfer a project".into()),
        };
        Ok(Box::into_raw(Box::new(Task {
            epoch: session.state().document_file.epoch,
            revision: session.engine().document().revision,
            request: id as u32,
            recovered: false,
            source: layer_ui::ImportSource::Master,
            place,
            gpu_generation: a.gpu_generation,
            open_control: Default::default(),
            payload,
        })) as jlong)
    })();
    match result {
        Ok(value) => value,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}

// Configure before starting the worker; cancellation subsequently touches only
// the shared atomic control, never the task borrowed by that worker.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectOpenControl(
    _: JNIEnv, _: JClass, handle: jlong, control: jlong,
) {
    unsafe { task(handle) }.open_control = crate::inspection::control(control);
}
fn check_open(t: &Task) -> Result<(), String> {
    if t.open_control.is_cancelled() { Err("Opening cancelled".into()) } else { Ok(()) }
}
fn prepare(t: &mut Task, input: Option<File>, width: u32, height: u32) -> Result<(), String> {
    check_open(t)?;
    let control = t.open_control.clone();
    let Payload::Open {
        environment,
        candidate,
    } = &mut t.payload
    else {
        return Err("Not an open request".into());
    };
    let mut e = environment.take().ok_or("Project worker already ran")?;
    let limits = ProjectLimits {
        dimension: e
            .device
            .limits()
            .max_texture_dimension_2d
            .min(ProjectLimits::default().dimension),
        ..Default::default()
    };
    let project = match input {
        Some(file) => {
            let imported = layer_ui::read_import(file,
                if t.recovered { layer_ui::ImportIntent::Recovery } else if t.place.is_some() { layer_ui::ImportIntent::Place } else { layer_ui::ImportIntent::Open },
                e.photo_policy, &e.source_name, limits, Default::default(), control.cancellation_flag())?;
            t.source = imported.source;
            if let Some(source) = imported.interpretation_required(e.photo_policy) {
                e.source_name = imported.project.document.layers[0].name.to_string();
                e.pending_photo = Some(source.clone());
                *environment = Some(e);
                return Ok(());
            }
            if imported.source == layer_ui::ImportSource::Photo { e.source_name = imported.project.document.layers[0].name.to_string(); }
            imported.project
        }
        None => {
            if let Some(source) = e.pending_photo.take() {
                if e.photo_policy.needs_interpretation(&source) {
                    return Err("Choose an image interpretation before opening".into());
                }
                e.photo_policy.photo_project(source, &e.source_name)?
            } else {
                layer_ui::NewDocumentOptions {
                    extent: [width, height],
                    ..e.new_options
                }
                .project()?
            }
        }
    };
    if control.is_cancelled() { return Err("Opening cancelled".into()); }
    if t.place.is_some() {
        let source = project
            .document
            .layers
            .iter()
            .find_map(|l| l.source.as_ref())
            .ok_or("The selected file is not a photo")?;
        layer_color::WorkingDecoder::new(
            &source.interpretation,
            e.working_space,
            Default::default(),
        )?;
        let name = e
            .source_name
            .chars()
            .filter(|c| !c.is_control())
            .take(128)
            .collect::<String>();
        t.payload = Payload::Placed {
            source: Some((**source).clone()),
            name: if name.is_empty() {
                "Image".into()
            } else {
                name
            },
        };
        return Ok(());
    }
    let mut gpu = WgpuRasterizer::from_wgpu_native_staged_cached(
        e.adapter,
        e.device,
        e.queue,
        std::path::Path::new(&e.cache),
        project.document.color,
    )
    .map_err(error)?;
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
        .map_err(error)?;
    }
    // Preparing startup starts the native compiler. Waiting for validation
    // before this call leaves all embedded shader jobs permanently queued.
    gpu.prepare_startup(&project.document, &e.brush, false)
        .map_err(error)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if control.is_cancelled() { return Err("Opening cancelled".into()); }
        gpu.device().poll(wgpu::PollType::Poll).map_err(error)?;
        if validating && let Some(result) = gpu.take_effect_validation() {
            result.result?;
            validating = false;
        }
        let ready = gpu.poll_startup().map_err(error)?;
        if !validating && ready.canvas_ready && ready.brush_ready {
            break;
        }
        if Instant::now() > deadline {
            return Err("Project canvas preparation timed out".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut next = UiSession::from_project(Renderer(Some(gpu)), project, None, e.viewport)?;
    next.frame(0, 0)?;
    *candidate = Some(Box::new(next));
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectProfilePrompt(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jni::sys::jstring {
    let source = match &unsafe { task(handle) }.payload {
        Payload::Open {
            environment: Some(e),
            ..
        } => e.pending_photo.as_ref().map(|s| &s.interpretation),
        _ => None,
    };
    crate::android::string(&mut env, serde_json::to_string(&source).map_err(error))
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectAssumeProfile(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    value: JString,
) {
    let result = (|| {
        let profile = serde_json::from_str(&read(&mut env, &value)?).map_err(error)?;
        let Payload::Open {
            environment: Some(e),
            ..
        } = &mut unsafe { task(handle) }.payload
        else {
            return Err("Image interpretation is no longer pending".into());
        };
        let source = e
            .pending_photo
            .as_ref()
            .ok_or("Image interpretation is no longer pending")?;
        e.pending_photo = Some(layer_color::assume_source_profile(source.clone(), profile)?);
        Ok(())
    })();
    fail(&mut env, result);
}

/// Configure a private New task before its worker runs. No live state changes.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectOptions(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    options: JString,
) {
    let result = (|| {
        let options: layer_ui::NewDocumentOptions =
            serde_json::from_str(&read(&mut env, &options)?).map_err(error)?;
        options.validate()?;
        let Payload::Open {
            environment: Some(environment),
            ..
        } = &mut unsafe { task(handle) }.payload
        else {
            return Err("New drawing task is no longer configurable".into());
        };
        environment.new_options = options;
        Ok(())
    })();
    fail(&mut env, result);
}

/// The worker exclusively owns the job and detached descriptor for this call.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    fd: jint,
    width: jint,
    height: jint,
) {
    let input = (fd >= 0).then(|| unsafe { File::from_raw_fd(fd) });
    let t = unsafe { task(handle) };
    // JVM worker stacks are small; shader translation gets a bounded native stack.
    let result = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("capy-project".into())
            .stack_size(8 * 1024 * 1024)
            .spawn_scoped(scope, move || match &mut t.payload {
                Payload::Save(project) => {
                    let project = project.take().ok_or("Save already encoded")?.pruned()?;
                    let mut out = BufWriter::new(input.ok_or("Missing project output")?);
                    project.write(&mut out)?;
                    out.flush().map_err(error)?;
                    out.get_ref().sync_all().map_err(error)
                }
                Payload::Export {
                    gpu,
                    snapshot,
                    recipe,
                    control,
                } => {
                    recipe.validate()?;
                    let snapshot = snapshot.take().ok_or("Export already encoded")?;
                    let extent = recipe.size.extent([
                        snapshot.project.document.width,
                        snapshot.project.document.height,
                    ])?;
                    let resolution =
                        recipe.output_resolution(snapshot.project.document.resolution)?;
                    let mut renderer = gpu
                        .capture(
                            snapshot.project,
                            snapshot.background,
                            snapshot.time,
                            Default::default(),
                            control.clone(),
                        )
                        .map_err(error)?;
                    renderer.set_output_extent(extent)?;
                    renderer.set_output_resolution(resolution)?;
                    let target = recipe.interpretation();
                    let mut out = BufWriter::new(input.ok_or("Missing export output")?);
                    match recipe.format {
                        layer_ui::ExportFormat::Exr => renderer.write_exr(&mut out),
                        layer_ui::ExportFormat::PngHdr | layer_ui::ExportFormat::PngHdrMapped => renderer.write_hdr_png(&mut out, recipe.format.maps_hdr_range()),
                        layer_ui::ExportFormat::JpegHdr | layer_ui::ExportFormat::JpegHdrMapped | layer_ui::ExportFormat::AvifHdr | layer_ui::ExportFormat::AvifHdrMapped => renderer.write_gainmap(
                            &mut out, recipe.format.gainmap().unwrap(), recipe.jpeg_quality,
                            recipe.background.matte(), recipe.format.maps_hdr_range(),
                        ),
                        layer_ui::ExportFormat::Png => renderer.write_png(
                            &mut out,
                            &target,
                            recipe.encoding,
                            recipe.background.matte(),
                        ),
                        layer_ui::ExportFormat::Tiff => renderer.write_tiff(
                            &mut out,
                            &target,
                            recipe.encoding,
                            recipe.background.matte(),
                        ),
                        layer_ui::ExportFormat::Jpeg => renderer.write_jpeg(
                            &mut out,
                            &target,
                            recipe.encoding,
                            recipe
                                .background
                                .matte()
                                .ok_or("Choose a JPEG background")?,
                            recipe.jpeg_quality,
                        ),
                    }?;
                    out.flush().map_err(error)?;
                    out.get_ref().sync_all().map_err(error)
                }
                Payload::Open { .. } => {
                    prepare(t, input, width.max(0) as u32, height.max(0) as u32)
                }
                Payload::Retired { .. } | Payload::Placed { .. } => {
                    Err("Project already prepared or adopted".into())
                }
            })
            .map_err(error)?
            .join()
            .map_err(|_| "Project worker failed".to_string())?
    });
    fail(&mut env, result);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectAdopt(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    transfer: jlong,
    location: JString,
) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let t = unsafe { task(transfer) };
        check_open(t)?;
        let location: Option<DocumentLocation> =
            serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
        if let Some(target) = t.place {
            let s = &mut a.host.session;
            if s.state().document_file.epoch != t.epoch
                || s.engine().document().revision != t.revision
                || s.engine().document().active_target() != target
                || a.gpu_generation != t.gpu_generation
            {
                return Err(
                    "The document or selected layer changed while importing; try again".into(),
                );
            }
            let Payload::Placed { source, name } = &mut t.payload else {
                return Err("Image is not prepared".into());
            };
            let previous = s.state().revision;
            if !s.state().requests.iter().any(|r| r.id == t.request && matches!(r.kind,
                HostRequestKind::Document { request: DocumentRequest::Place | DocumentRequest::Paste })) {
                return Err("Image import is no longer active".into());
            }
            s.place_layer_source(name, source.as_ref().ok_or("Image already placed")?.clone(), None)?;
            source.take();
            let mut change = s.complete_document_request(t.request, Ok(true))?;
            change.canvas_wake = true;
            change.regions |= 255;
            a.host.apply_change(previous, change);
            return Ok(());
        }
        // Importing a photo never grants Save permission to overwrite it.
        let location = t.source.adoption_location(location);
        let Payload::Open { candidate, .. } = &mut t.payload else {
            return Err("Not an open request".into());
        };
        if t.gpu_generation != a.gpu_generation {
            return Err("The canvas changed while preparing this drawing; open it again".into());
        }
        let next = candidate.take().ok_or("Project is not prepared")?;
        let result = if t.recovered {
            a.host
                .session
                .adopt_recovered_project(next, t.epoch, t.revision)
        } else {
            a.host
                .session
                .adopt_project(next, t.epoch, t.revision, location)
        };
        match result {
            Ok(retired) => t.payload = Payload::Retired { _session: retired },
            Err((error, next)) => {
                *candidate = Some(next);
                return Err(error);
            }
        }
        if !t.recovered {
            a.host
                .session
                .complete_document_request(t.request, Ok(true))?;
        }
        a.host.document_adopted();
        a.project_adopted();
        Ok(())
    })();
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectFree(_: JNIEnv, _: JClass, handle: jlong) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut Task) });
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentComplete(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    success: jboolean,
    message: JString,
) {
    let result = (|| {
        let message: Option<String> =
            serde_json::from_str(&read(&mut env, &message)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let previous = a.host.session.state().revision;
        let change = a
            .host
            .session
            .complete_document_request(id as u32, message.map_or(Ok(success != 0), Err))?;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentClose(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    decision: JString,
) {
    let result = (|| {
        let decision = serde_json::from_str(&read(&mut env, &decision)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let previous = a.host.session.state().revision;
        let change = a.host.session.respond_document_close(id as u32, decision)?;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectExportTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    now: jlong,
    cancel: jlong,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        a.host.session.require_document_idle()?;
        if !a.host.session.state().requests.iter().any(|r| {
            r.id == id as u32
                && matches!(
                    r.kind,
                    HostRequestKind::Document {
                        request: DocumentRequest::Export { .. }
                    }
                )
        }) {
            return Err("Export is no longer active".into());
        }
        a.host.prepare_canvas_frame(now as u64, now as u64, true)?;
        if !a.host.startup.canvas_ready || a.host.session.engine().has_pending_document_edits() {
            return Ok(0);
        }
        let epoch = a.host.session.state().document_file.epoch;
        let revision = a.host.session.engine().document().revision;
        let snapshot = a.host.session.capture_project_export(id as u32)?;
        let gpu = a
            .host
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Canvas is unavailable")?
            .snapshot_gpu();
        Ok(Box::into_raw(Box::new(Task {
            epoch,
            revision,
            request: id as u32,
            recovered: false,
            source: layer_ui::ImportSource::Master,
            place: None,
            gpu_generation: a.gpu_generation,
            open_control: Default::default(),
            payload: Payload::Export {
                gpu,
                snapshot: Some(snapshot),
                recipe: layer_ui::ExportRecipe::web_share(),
                control: crate::inspection::control(cancel),
            },
        })) as jlong)
    })();
    match result {
        Ok(value) => value,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}

/// Configure the private output copy before the worker starts.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectExportOptions(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    value: JString,
) {
    let result = (|| {
        let selected: layer_ui::ExportRecipe =
            serde_json::from_str(&read(&mut env, &value)?).map_err(error)?;
        selected.validate()?;
        let Payload::Export {
            recipe,
            snapshot: Some(snapshot),
            ..
        } = &mut unsafe { task(handle) }.payload
        else {
            return Err("Export task is no longer configurable".into());
        };
        layer_color::WorkingEncoder::new(
            snapshot.project.document.color.space,
            &selected.interpretation(),
            selected.encoding,
        )?;
        *recipe = selected;
        Ok(())
    })();
    fail(&mut env, result);
}

/// Capture on the render owner; recovery reads/writes and candidate preparation
/// still belong to the file worker. No manual-save checkpoint is acknowledged.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectRecoveryTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    opening: jboolean,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let session = &a.host.session;
        // Background recovery waits for a committed snapshot boundary. Active
        // placements and region operations are normal deferrals, not failures
        // that should put a modal dialog over an in-progress canvas gesture.
        if opening == 0 && session.recovery_document().busy {
            return Ok(0);
        }
        let payload = if opening != 0 {
            session.require_document_idle()?;
            if session.state().document_file.modified || session.state().document_file.busy {
                return Err("Recovery requires an unchanged, idle drawing".to_string());
            }
            let gpu = session
                .engine()
                .backend()
                .0
                .as_ref()
                .ok_or("Wait for the canvas")?;
            Payload::Open {
                environment: Some(Environment {
                    adapter: gpu.adapter().clone(),
                    device: gpu.device().clone(),
                    queue: gpu.queue().clone(),
                    viewport: session.state().camera.viewport,
                    brush: session.engine().configured_brush().clone(),
                    cache: a.cache_directory.clone(),
                    new_options: session.state().settings.new_document.defaults,
                    photo_policy: session.state().settings.photo_open,
                    source_name: "Recovered drawing".into(),
                    working_space: session.engine().document().color.space,
                    pending_photo: None,
                }),
                candidate: None,
            }
        } else {
            Payload::Save(Some(session.capture_project_recovery()?))
        };
        Ok(Box::into_raw(Box::new(Task {
            epoch: session.state().document_file.epoch,
            revision: session.engine().document().revision,
            request: 0,
            recovered: true,
            source: layer_ui::ImportSource::Master,
            place: None,
            gpu_generation: a.gpu_generation,
            open_control: Default::default(),
            payload,
        })) as jlong)
    })();
    match result {
        Ok(task) => task,
        Err(error) => {
            fail(&mut env, Err(error));
            0
        }
    }
}

/// File worker only. The shared Unix writer fsyncs the complete sibling file,
/// renames it, then fsyncs the parent. Encoding failure retains the prior copy.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectPublish(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    path: JString,
) {
    let result = (|| {
        let path = read(&mut env, &path)?;
        let Payload::Save(project) = &mut unsafe { task(handle) }.payload else {
            return Err("Not a recovery save".into());
        };
        let project = project.take().ok_or("Recovery already encoded")?.pruned()?;
        layer_core::atomic_write(std::path::Path::new(&path), |output| project.write(output))
    })();
    fail(&mut env, result);
}
