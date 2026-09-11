//! File workers own transfer jobs, never the live editor. Only adoption and
//! checkpoint acknowledgement return to the render Looper.
use crate::android::{app, error, fail, read};
use jni::{
    JNIEnv,
    objects::{JClass, JObject, JString},
    sys::{jboolean, jint, jlong},
};
use layer_core::{Project, ProjectLimits};
use layer_host::Renderer;
use layer_render::{CanvasRenderer, EffectValidationRequest};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{DocumentLocation, DocumentRequest, HostRequestKind, UiSession};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
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
}
enum Payload {
    Save(Option<Project>),
    Open {
        environment: Option<Environment>,
        candidate: Option<Box<UiSession<Renderer>>>,
    },
    Retired {
        _session: Box<UiSession<Renderer>>,
    },
}
struct Task {
    epoch: u64,
    revision: u64,
    request: u32,
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
        session.require_document_idle()?;
        let request = session
            .state()
            .requests
            .iter()
            .find_map(|r| match &r.kind {
                HostRequestKind::Document { request } if r.id == id as u32 => Some(request.clone()),
                _ => None,
            })
            .ok_or("The document request is no longer active")?;
        let payload = match request {
            DocumentRequest::Save { .. } => {
                let location: DocumentLocation =
                    serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
                Payload::Save(Some(session.capture_project_save(id as u32, location)?))
            }
            DocumentRequest::Open | DocumentRequest::New => {
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

fn prepare(t: &mut Task, input: Option<File>, width: u32, height: u32) -> Result<(), String> {
    let Payload::Open {
        environment,
        candidate,
    } = &mut t.payload
    else {
        return Err("Not an open request".into());
    };
    let e = environment.take().ok_or("Project worker already ran")?;
    let limits = ProjectLimits {
        dimension: e
            .device
            .limits()
            .max_texture_dimension_2d
            .min(ProjectLimits::default().dimension),
        ..Default::default()
    };
    let project = match input {
        Some(file) => Project::read(BufReader::new(file), limits)?,
        None => layer_ui::new_drawing(width, height)?,
    };
    let mut gpu = WgpuRasterizer::from_wgpu_staged_cached(
        e.adapter,
        e.device,
        e.queue,
        std::path::Path::new(&e.cache),
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
    let deadline = Instant::now() + Duration::from_secs(60);
    if !programs.is_empty() {
        gpu.request_effect_validation(EffectValidationRequest {
            request_id: 1,
            namespace: programs.clone(),
            programs,
        })
        .map_err(error)?;
        loop {
            gpu.device().poll(wgpu::PollType::Poll).map_err(error)?;
            if let Some(result) = gpu.take_effect_validation() {
                result.result?;
                break;
            }
            if Instant::now() > deadline {
                return Err("Project shader preparation timed out".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    gpu.prepare_startup(&project.document, &e.brush, false)
        .map_err(error)?;
    loop {
        let ready = gpu.poll_startup().map_err(error)?;
        if ready.canvas_ready && ready.brush_ready {
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
                Payload::Open { .. } => {
                    prepare(t, input, width.max(0) as u32, height.max(0) as u32)
                }
                Payload::Retired { .. } => Err("Project already adopted".into()),
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
        let location: Option<DocumentLocation> =
            serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
        let Payload::Open { candidate, .. } = &mut t.payload else {
            return Err("Not an open request".into());
        };
        let next = candidate.take().ok_or("Project is not prepared")?;
        match a
            .host
            .session
            .adopt_project(next, t.epoch, t.revision, location)
        {
            Ok(retired) => t.payload = Payload::Retired { _session: retired },
            Err((error, next)) => {
                *candidate = Some(next);
                return Err(error);
            }
        }
        a.host
            .session
            .complete_document_request(t.request, Ok(true))?;
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
pub extern "system" fn Java_art_capycanvas_Native_documentPixels(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
) -> jni::sys::jobjectArray {
    let result = (|| {
        let a = unsafe { app(handle) };
        if a.host.dirty
            || !a.host.startup.canvas_ready
            || a.host.session.engine().has_pending_document_edits()
        {
            return Ok(std::ptr::null_mut());
        }
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
        let renderer = a.host.session.renderer_mut();
        renderer.request_readback(id as u64).map_err(error)?;
        let image = renderer
            .take_readback()
            .ok_or("Export produced no image")?
            .map_err(error)?;
        let values = env
            .new_object_array(2, "java/lang/Object", JObject::null())
            .map_err(error)?;
        let header = env
            .new_string(serde_json::json!([image.width, image.height]).to_string())
            .map_err(error)?;
        let pixels = env.byte_array_from_slice(&image.bytes).map_err(error)?;
        env.set_object_array_element(&values, 0, header)
            .map_err(error)?;
        env.set_object_array_element(&values, 1, pixels)
            .map_err(error)?;
        Ok(values.into_raw())
    })();
    match result {
        Ok(value) => value,
        Err(e) => {
            fail(&mut env, Err(e));
            std::ptr::null_mut()
        }
    }
}
