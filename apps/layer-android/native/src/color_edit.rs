//! Color changes prepare immutable backing and a complete private GPU canvas on
//! IO. The render owner publishes renderer + document/history in one turn.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use layer_core::{BrushSnapshot, ColorTransition, PreparedColorTransition, Project};
use layer_render::{CanvasRenderer, ViewState};
use layer_render_wgpu::{
    WgpuRasterizer,
    snapshot::{CaptureControl, SnapshotGpu, SnapshotPreview},
};
use layer_ui::{DocumentColorOperation, DocumentRequest, HostRequestKind};

struct Task {
    original: Project,
    candidate: Option<Project>,
    transition: Option<PreparedColorTransition>,
    operation: Option<DocumentColorOperation>,
    renderer: Option<WgpuRasterizer>,
    gpu: SnapshotGpu,
    control: CaptureControl,
    brush: BrushSnapshot,
    view: ViewState,
    time: f32,
    epoch: u64,
    revision: u64,
    generation: u64,
    request: u32,
    previews: Vec<SnapshotPreview>,
    clipped: u64,
    copy: bool,
}
unsafe fn task<'a>(handle: jlong) -> &'a mut Task {
    unsafe { &mut *(handle as *mut Task) }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    cancel: jlong,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let s = &a.host.session;
        s.require_document_idle()?;
        let request = s
            .state()
            .requests
            .iter()
            .find(|r| r.id == id as u32)
            .ok_or("Color request is no longer active")?;
        let (operation, transition, candidate) = match &request.kind {
            HostRequestKind::Document {
                request: DocumentRequest::ChangeColor { operation },
            } => (Some(*operation), None, None),
            HostRequestKind::Document {
                request: DocumentRequest::ColorHistory { redo },
            } => {
                let (prepared, project) = s.prepare_document_color_transition(if *redo {
                    ColorTransition::Redo
                } else {
                    ColorTransition::Undo
                })?;
                (None, Some(prepared), Some(project))
            }
            _ => return Err("Not a document color request".to_string()),
        };
        let original = s.capture_project_recovery()?;
        Ok(Box::into_raw(Box::new(Task {
            revision: original.document.revision,
            original,
            candidate,
            transition,
            operation,
            renderer: None,
            gpu: s
                .engine()
                .backend()
                .0
                .as_ref()
                .ok_or("Canvas unavailable")?
                .snapshot_gpu(),
            control: crate::inspection::control(cancel),
            brush: s.engine().configured_brush().clone(),
            view: s.engine().view(),
            time: s.engine().animation_time(),
            epoch: s.state().document_file.epoch,
            generation: a.gpu_generation,
            request: id as u32,
            previews: Vec::new(),
            clipped: 0,
            copy: false,
        })) as jlong)
    })();
    match result {
        Ok(handle) => handle,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}
fn work(
    t: &mut Task,
    choice: Option<layer_color::DocumentColorChange>,
    copy: bool,
) -> Result<String, String> {
    if t.renderer.is_some() || !t.previews.is_empty() {
        return Err("Color candidate was already prepared".into());
    }
    if copy && t.operation != Some(DocumentColorOperation::Convert) {
        return Err("Only color conversion can create a flattened copy".into());
    }
    t.copy = copy;
    if let Some(operation) = t.operation {
        let change = choice.ok_or("Choose a color change")?;
        if !matches!(
            (operation, change),
            (
                DocumentColorOperation::Assign,
                layer_color::DocumentColorChange::Assign(_)
            ) | (
                DocumentColorOperation::Convert,
                layer_color::DocumentColorChange::Convert { .. }
            ) | (
                DocumentColorOperation::Depth,
                layer_color::DocumentColorChange::Depth { .. }
            )
        ) {
            return Err("Color choice does not match the request".into());
        }
        let prepared = if copy {
            let layer_color::DocumentColorChange::Convert { space, options } = change else {
                unreachable!()
            };
            t.gpu
                .capture(
                    t.original.clone(),
                    t.view.background_rgba_linear,
                    t.time,
                    Default::default(),
                    t.control.clone(),
                )
                .map_err(error)?
                .flattened_document(
                    layer_core::color::DocumentColor {
                        space,
                        depth: t.original.document.color.depth,
                    },
                    options,
                    512 * 1024 * 1024,
                )?
        } else {
            layer_color::prepare_document_color(&t.original, change, 512 * 1024 * 1024, || {
                t.control.is_cancelled()
            })?
        };
        t.clipped = prepared.statistics.clipped_channels;
        t.candidate = Some(prepared.project);
    }
    let project = t.candidate.as_ref().ok_or("Color candidate is missing")?;
    let matrix = t
        .original
        .document
        .color
        .space
        .linear_transform(project.document.color.space);
    let transform = |color: &mut [f32; 4]| {
        let rgb =
            layer_core::color::rgb::apply(matrix, [color[0], color[1], color[2]].map(f64::from));
        color[..3].copy_from_slice(&rgb.map(|v| v as f32));
    };
    let mut view = t.view;
    transform(&mut view.background_rgba_linear);
    let mut brush = t.brush.clone();
    transform(&mut brush.color_rgba_linear);
    transform(&mut brush.color_dynamics.secondary_color_rgba_linear);
    if t.operation.is_some() {
        for (source, background) in [
            (&t.original, t.view.background_rgba_linear),
            (project, view.background_rgba_linear),
        ] {
            let mut snapshot = t
                .gpu
                .capture(
                    source.clone(),
                    background,
                    t.time,
                    Default::default(),
                    t.control.clone(),
                )
                .map_err(error)?;
            t.previews
                .push(snapshot.preview_document([512, 384], layer_core::color::RgbSpace::Srgb)?);
        }
    }
    if !copy {
        let mut canvas = t
            .gpu
            .color_canvas(project.clone(), &brush, view, t.time, t.control.clone())
            .map_err(error)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while !canvas.poll().map_err(error)? {
            if std::time::Instant::now() > deadline {
                return Err("Color canvas preparation timed out".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        t.renderer = Some(canvas.take_ready().map_err(error)?);
    }
    serde_json::to_string(
        &serde_json::json!({"color":project.document.color,"clipped_channels":t.clipped}),
    )
    .map_err(error)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    choice: JString,
    copy: jboolean,
) -> jstring {
    let result = (|| {
        let choice = serde_json::from_str(&read(&mut env, &choice)?).map_err(error)?;
        let t = unsafe { task(handle) };
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("capy-color".into())
                .stack_size(8 * 1024 * 1024)
                .spawn_scoped(scope, move || work(t, choice, copy != 0))
                .map_err(error)?
                .join()
                .map_err(|_| "Color worker failed".to_string())?
        })
    })();
    string(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorPreview(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    after: jboolean,
) -> jbyteArray {
    let result = (|| {
        let preview = unsafe { task(handle) }
            .previews
            .get(usize::from(after != 0))
            .ok_or("Comparison is unavailable")?;
        let mut bytes = Vec::with_capacity(8 + preview.pixels.len() * 4);
        for v in preview.extent {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend(preview.srgb_bytes()?);
        env.byte_array_from_slice(&bytes)
            .map(|a| a.into_raw())
            .map_err(error)
    })();
    match result {
        Ok(bytes) => bytes,
        Err(e) => {
            fail(&mut env, Err(e));
            std::ptr::null_mut()
        }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorAdopt(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    job: jlong,
) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let t = unsafe { task(job) };
        if t.copy {
            return Err("Save the converted copy as a separate document".into());
        }
        let s = &mut a.host.session;
        if t.control.is_cancelled()
            || a.gpu_generation != t.generation
            || s.state().document_file.epoch != t.epoch
            || s.engine().document().revision != t.revision
            || !s.state().requests.iter().any(|r| r.id == t.request)
        {
            return Err("The document or canvas changed; prepare the color change again".into());
        }
        s.require_document_idle()?;
        let prepared = if let Some(prepared) = t.transition.take() {
            prepared
        } else {
            let candidate = t.candidate.as_ref().ok_or("Color candidate is missing")?;
            s.prepare_document_color_transition(ColorTransition::Apply {
                color: candidate.document.color,
                layers: candidate.document.layers.clone(),
            })?
            .0
        };
        let mut next = t.renderer.take().ok_or("Color canvas is not ready")?;
        let [width, height] = s.state().camera.viewport;
        next.resize_surface(width, height).map_err(error)?;
        let retired = s.renderer_mut().0.replace(next);
        if let Err(e) = s.commit_document_color_transition(prepared) {
            t.renderer = std::mem::replace(&mut s.renderer_mut().0, retired);
            return Err(e);
        }
        t.renderer = retired; // Destroy old GPU state on IO when this job is freed.
        s.complete_document_request(t.request, Ok(true))?;
        a.host.document_adopted();
        a.project_adopted();
        Ok(())
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorFree(_: JNIEnv, _: JClass, handle: jlong) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut Task) });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorWriteCopy(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    fd: jint,
) {
    use std::io::Write;
    use std::os::fd::FromRawFd;
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let result = (|| {
        let t = unsafe { task(handle) };
        if !t.copy || t.previews.len() != 2 {
            return Err("Preview a flattened copy before saving".into());
        }
        if t.control.is_cancelled() {
            return Err("Converted copy cancelled".into());
        }
        let project = t.candidate.as_ref().ok_or("Converted copy is not ready")?;
        let mut output = std::io::BufWriter::new(file);
        project.write(&mut output)?;
        output.flush().map_err(error)?;
        if t.control.is_cancelled() {
            return Err("Converted copy cancelled".into());
        }
        Ok(())
    })();
    fail(&mut env, result)
}
