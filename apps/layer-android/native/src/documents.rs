//! File workers own transfer jobs, never the live editor. Only adoption and
//! checkpoint acknowledgement return to the render Looper.
use crate::android::{app, error, fail, or_throw, read};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jint, jlong},
};
use layer_core::authored::{ArtworkCapture, SourceTarget};
use layer_core::package::codec::PreparedPackage;
use std::sync::atomic::AtomicBool;
use layer_host::{Renderer, export::ExportTask, open::OpenEnvironment, window::OpenAdoption};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{DocumentLocation, DocumentRequest, UiSession};
use std::{
    fs::File,
    io::{BufWriter, Read, Write},
    os::fd::FromRawFd,
};

struct Environment {
    open: OpenEnvironment,
    source_name: String,
    working_space: layer_core::color::RgbSpace,
    pending_import: Option<layer_ui::ImportedDocument>,
}
impl Environment {
    fn new(open: OpenEnvironment, working_space: layer_core::color::RgbSpace, source_name: String) -> Self {
        Self { open, working_space, source_name, pending_import: None }
    }
}
enum Payload {
    Lookup { resource:Option<std::sync::Arc<layer_core::Lut3d>>, name:String },
    Save(Option<ArtworkCapture>),
    Package {view: layer_ui::PackageView, localization: std::sync::Arc<layer_ui::Localizer>},
    Export {
        export: Box<ExportTask>,
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
        _renderer: Option<Box<WgpuRasterizer>>,
    },
}
struct Task {
    owner: u64,
    epoch: u64,
    revision: u64,
    request: u32,
    source: layer_ui::ImportSource,
    place: Option<SourceTarget>,
    gpu_generation: u64,
    open_control: layer_render_wgpu::snapshot::CaptureControl,
    payload: Payload,
}
impl Task {
    fn new(owner: u64, epoch: u64, revision: u64, request: u32, gpu_generation: u64, payload: Payload) -> Self {
        Self { owner, epoch, revision, request, gpu_generation, payload,
            source: layer_ui::ImportSource::Master, place: None, open_control: Default::default() }
    }
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
        let admission = a.window.documents.admission(&a.host.session.retained_document_tiles());
        let options = a.host.renderer_options(Some(a.cache_directory.clone().into()));
        let session = &mut a.host.session;
        let request = session.document_request(id as u32)
            .map_err(|_| "The document request is no longer active")?.clone();
        let place = matches!(request, DocumentRequest::Place | DocumentRequest::Paste { .. })
            .then(|| session.engine().document().active_target()).flatten();
        let payload = match request {
            DocumentRequest::ImportLookup {..} => Payload::Lookup {resource:None,
                name:serde_json::from_str::<Option<DocumentLocation>>(&read(&mut env,&location)?).map_err(error)?.map(|location|location.name).unwrap_or_default()},
            DocumentRequest::Save { .. } => {
                let location: DocumentLocation =
                    serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
                Payload::Save(Some(session.capture_project_save(id as u32, location)?))
            }
            DocumentRequest::Open
            | DocumentRequest::New
            | DocumentRequest::Place
            | DocumentRequest::Paste { .. } => {
                session.require_document_idle()?;
                if session.state().document_file.epoch != epoch as u64
                    || session.engine().document().revision != revision as u64
                {
                    return Err(layer_ui::DocumentTransportRefusal::OpenSnapshotChanged.message(session.localization()).to_string());
                }
                Payload::Open {
                    environment: Some(Environment::new(
                        OpenEnvironment::capture(session, admission, options)?,
                        session.engine().document().composition().color.space,
                        serde_json::from_str::<Option<DocumentLocation>>(&read(
                            &mut env, &location,
                        )?)
                        .map_err(error)?
                        .map(|location| location.name)
                        .unwrap_or_else(|| "Photo".into()),
                    )),
                    candidate: None,
                }
            }
            _ => return Err("This request does not transfer a project".into()),
        };
        Ok(Box::into_raw(Box::new(Task {
            place,
            ..Task::new(a.window.documents.selected(), session.state().document_file.epoch,
                session.engine().document().revision, id as u32, a.gpu_generation, payload)
        })) as jlong)
    })();
    or_throw(&mut env, result, 0)
}

// Configure before starting the worker; cancellation subsequently touches only
// the shared atomic control, never the task borrowed by that worker.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectOpenControl(
    _: JNIEnv, _: JClass, handle: jlong, control: jlong,
) {
    unsafe { crate::inspection::borrow::<Task>(handle) }.open_control = crate::inspection::control(control);
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
    let imported = match input {
        Some(file) => {
            let outcome = e.open.read(file,
                if t.place.is_some() { layer_ui::ImportIntent::Place } else { layer_ui::ImportIntent::Open },
                &e.source_name, control.cancellation_flag())?;
            let imported = match outcome {
                layer_ui::ImportOutcome::Editable(imported) => imported,
                layer_ui::ImportOutcome::Package(outcome) => {
                    if control.is_cancelled() { return Err("Opening cancelled".into()); }
                    t.payload = Payload::Package {view:layer_ui::PackageView::new(outcome)?,localization:e.open.localization};
                    return Ok(());
                }
            };
            t.source = imported.source;
            if imported.interpretation_required(e.open.photo_policy).is_some() {
                e.source_name = imported.project.scene().order().first().and_then(|h| imported.project.scene().occurrence(*h)).map_or_else(|| "Photo".into(), |o| o.name.to_string());
                e.pending_import = Some(imported);
                *environment = Some(e);
                return Ok(());
            }
            if imported.source == layer_ui::ImportSource::Photo { e.source_name = imported.project.scene().order().first().and_then(|h| imported.project.scene().occurrence(*h)).map_or_else(|| "Photo".into(), |o| o.name.to_string()); }
            imported
        }
        None => {
            if let Some(imported) = e.pending_import.take() {
                if imported.interpretation_required(e.open.photo_policy).is_some() {
                    return Err("Choose an image interpretation before opening".into());
                }
                imported
            } else {
                layer_ui::ImportedDocument::new(layer_ui::NewDocumentOptions {
                    extent: [width, height],
                    ..e.open.new_options
                }
                .project(&e.open.localization)?, layer_ui::ImportSource::Master)
            }
        }
    };
    if control.is_cancelled() { return Err("Opening cancelled".into()); }
    if t.place.is_some() {
        let source = imported.project.artwork.paint.iter().find_map(|(_,_,p)| p.original.as_ref())
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
    match e.open.prepare(imported.project.clone(), || control.is_cancelled()) {
        Ok(prepared) => *candidate = Some(prepared),
        Err(reason) => {
            if control.is_cancelled() { return Err("Opening cancelled".into()); }
            let Some(outcome) = imported.preserve_unsupported(reason.clone()) else { return Err(reason); };
            t.payload = Payload::Package {view:layer_ui::PackageView::new(outcome)?,localization:e.open.localization};
        }
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectPackagePrompt(mut env:JNIEnv,_:JClass,handle:jlong)->jni::sys::jstring {
    let summary = match &unsafe {crate::inspection::borrow_ref::<Task>(handle)}.payload {
        Payload::Package {view,localization} => serde_json::to_string(&view.summary(localization)).map_err(error),
        _ => Ok("null".into()),
    };
    crate::android::string(&mut env,summary)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectPackagePreview(mut env:JNIEnv,_:JClass,handle:jlong)->jni::sys::jbyteArray {
    let bytes = match &unsafe {crate::inspection::borrow_ref::<Task>(handle)}.payload {
        Payload::Package {view,..} => view.preview().map(|p| p.encoded().as_ref()).unwrap_or(&[]),
        _ => &[],
    };
    let result = env.byte_array_from_slice(bytes).map(|bytes| bytes.into_raw()).map_err(error);
    or_throw(&mut env,result,std::ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectPackageWrite(mut env:JNIEnv,_:JClass,handle:jlong,fd:jint,preview:jboolean) {
    let result = crate::inspection::on_worker("capy-original-copy","Drawing copy worker failed",move || {
        if fd < 0 {return Err("Missing drawing copy output".into());}
        let mut output = BufWriter::new(unsafe {File::from_raw_fd(fd)});
        let task = unsafe {crate::inspection::borrow_ref::<Task>(handle)};
        let Payload::Package {view,..} = &task.payload else{return Err("No preserved drawing to copy".into());};
        if preview != 0 { view.export_preview(&mut output, task.open_control.cancellation_flag())?; }
        else { view.copy_original(&mut output,task.open_control.cancellation_flag())?; }
        output.flush().map_err(error)?;
        output.get_ref().sync_all().map_err(error)
    });
    fail(&mut env,result);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectProfilePrompt(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jni::sys::jstring {
    let source = match &unsafe { crate::inspection::borrow::<Task>(handle) }.payload {
        Payload::Open {
            environment: Some(e),
            ..
        } => e.pending_import.as_ref().and_then(|i| i.project.artwork.paint.iter().find_map(|(_,_,p)| p.original.as_ref())).map(|s| &s.interpretation),
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
        } = &mut unsafe { crate::inspection::borrow::<Task>(handle) }.payload
        else {
            return Err("Image interpretation is no longer pending".into());
        };
        e.pending_import.as_mut().ok_or("Image interpretation is no longer pending")?.interpret(profile)
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
        let Payload::Open {
            environment: Some(environment),
            ..
        } = &mut unsafe { crate::inspection::borrow::<Task>(handle) }.payload
        else {
            return Err("New drawing task is no longer configurable".into());
        };
        options.validate().map_err(|failure| failure.message(&environment.open.localization))?;
        environment.open.new_options = options;
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
    let t = unsafe { crate::inspection::borrow::<Task>(handle) };
    let result = crate::inspection::on_worker("capy-project", "Project worker failed", move || match &mut t.payload {
        Payload::Lookup {resource,name} => {
            let mut bytes=Vec::new();
            input.ok_or("Missing lookup input")?.take(layer_core::Lut3d::MAX_TEXT_BYTES as u64+1).read_to_end(&mut bytes).map_err(error)?;
            *resource=Some(std::sync::Arc::new(layer_core::Lut3d::parse_cube_named(&bytes,name)?));
            Ok(())
        }
        Payload::Save(project) => {
            let project = project.take().ok_or("Save already encoded")?;
            let mut out = BufWriter::new(input.ok_or("Missing project output")?);
            let cancelled = AtomicBool::new(false);
            PreparedPackage::prepare(&project, None, &cancelled)?.write(&mut out, &cancelled)?;
            out.flush().map_err(error)?;
            out.get_ref().sync_all().map_err(error)
        }
        Payload::Export { export, control } => {
            let localization = &*crate::launch::active_localization()?;
            let out = input.ok_or("Missing export output")?;
            export.write(&out, control.clone()).map_err(|reason| reason.message(localization))?;
            out.sync_all().map_err(error)
        }
        Payload::Open { .. } => {
            prepare(t, input, width.max(0) as u32, height.max(0) as u32)
        }
        Payload::Retired { .. } | Payload::Placed { .. } | Payload::Package { .. } => {
            Err("Project already prepared or adopted".into())
        }
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
        let t = unsafe { crate::inspection::borrow::<Task>(transfer) };
        check_open(t)?;
        let location: Option<DocumentLocation> =
            serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
        if let Payload::Lookup {resource,..}=&mut t.payload {
            let previous=a.host.session.state().revision;
            let result=a.host.session.apply_lookup(t.request,resource.take().ok_or("Lookup is not prepared")?);
            let change=a.host.session.complete_document_request(t.request,result)?;
            a.host.apply_change(previous,change);
            return Ok(());
        }
        if let Some(target) = t.place {
            let s = &mut a.host.session;
            if s.state().document_file.epoch != t.epoch
                || s.engine().document().revision != t.revision
                || s.engine().document().active_target() != Some(target)
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
            if !matches!(s.document_request(t.request), Ok(DocumentRequest::Place | DocumentRequest::Paste { .. })) {
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
        let open = OpenAdoption { epoch: t.epoch, revision: t.revision, location };
        let retired = a.window.adopt(&mut a.host, candidate, open, || true, |s| s)?;
        a.document_retired();
        t.payload = Payload::Retired { _renderer: retired };
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
        if !matches!(a.host.session.document_request(id as u32), Ok(DocumentRequest::Export { .. })) {
            return Err("Export is no longer active".into());
        }
        a.host.prepare_canvas_frame(now as u64, now as u64, true)?;
        if !a.host.startup.canvas_ready || a.host.session.engine().has_pending_document_edits() {
            return Ok(0);
        }
        let epoch = a.host.session.state().document_file.epoch;
        let revision = a.host.session.engine().document().revision;
        let export = ExportTask::capture(
            &a.host.session,
            id as u32,
            "",
            layer_core::color::RgbSpace::Srgb,
        )?;
        Ok(Box::into_raw(Box::new(Task::new(
            a.window.documents.selected(), epoch, revision, id as u32, a.gpu_generation,
            Payload::Export {
                export: Box::new(export),
                control: crate::inspection::control(cancel),
            },
        ))) as jlong)
    })();
    or_throw(&mut env, result, 0)
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
        let localization = &*crate::launch::active_localization()?;
        let selected: layer_ui::ExportRecipe =
            serde_json::from_str(&read(&mut env, &value)?).map_err(error)?;
        let Payload::Export { export, .. } = &mut unsafe { crate::inspection::borrow::<Task>(handle) }.payload else {
            return Err("Export task is no longer configurable".into());
        };
        export.configure(selected).map_err(|reason| reason.message(localization))
    })();
    fail(&mut env, result);
}

/// Complete only this transfer's initiating request, then poll exact backing.
/// Its stable owner and activation generation remain checked during adoption.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_projectParkReady(mut env:JNIEnv,_:JClass,handle:jlong,transfer:jlong)->jboolean {
    let result=(|| {
        let a=unsafe{app(handle)};let t=unsafe{crate::inspection::borrow::<Task>(transfer)};
        if t.owner!=a.window.documents.selected() || t.epoch!=a.host.session.state().document_file.epoch || t.revision!=a.host.session.engine().document().revision || t.gpu_generation!=a.gpu_generation {
            return Err("The drawing changed while opening; try again".into());
        }
        if a.host.session.state().requests.iter().any(|r|r.id==t.request) {
            a.host.session.complete_document_request(t.request,Ok(true))?;
        }
        a.window.park_ready(&a.host)
    })();or_throw(&mut env, result.map(|ready| u8::from(ready)), 0)
}

/// Shared native-drawing/photo classification for external drop routing. Actual
/// decoding and validation still run in the bounded import worker.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_importSource(mut env:JNIEnv,_:JClass,prefix:jni::objects::JByteArray)->jni::sys::jstring {
    let result=env.convert_byte_array(prefix).map_err(error).and_then(|bytes|layer_ui::ImportSource::identify(&bytes,layer_ui::ImportIntent::Open)).and_then(|source|serde_json::to_string(&source).map_err(error));
    crate::android::string(&mut env,result)
}
