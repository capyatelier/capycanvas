//! Color changes prepare immutable backing and a complete private GPU canvas on
//! IO. The render owner publishes renderer + document/history in one turn.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use layer_core::BrushSnapshot;
use layer_render::{CanvasRenderer, ViewState};
use layer_render_wgpu::{
    WgpuRasterizer,
    snapshot::{CaptureControl, SnapshotGpu, SnapshotPreview},
};
use layer_ui::{ColorWorkflow, ColorPreparation};

struct Task {
    workflow: ColorWorkflow,
    renderer: Option<Box<WgpuRasterizer>>,
    gpu: SnapshotGpu,
    control: CaptureControl,
    brush: BrushSnapshot,
    view: ViewState,
    time: f32,
    generation: u64,
    previews: Vec<SnapshotPreview>,
    clipped: u64,
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
        let workflow = ColorWorkflow::begin(s, id as u32)?;
        Ok(Box::into_raw(Box::new(Task {
            workflow,
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
            generation: a.gpu_generation,
            previews: Vec::new(),
            clipped: 0,
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
    let plan = t.workflow.select(choice, copy)?;
    let prepared = match plan {
        ColorPreparation::History => None,
        ColorPreparation::Flatten { color, options } => Some(t.gpu.capture(
            t.workflow.original.clone(), t.view.background_rgba_linear, t.time,
            Default::default(), t.control.clone(),
        ).map_err(error)?.flattened_document(color, options, 512 * 1024 * 1024)?),
        ColorPreparation::Edit(change) => Some(layer_color::prepare_document_color(
            &t.workflow.original, change, 512 * 1024 * 1024, || t.control.is_cancelled(),
        )?),
    };
    if let Some(prepared) = prepared {
        t.clipped = prepared.statistics.clipped_channels;
        t.workflow.candidate = Some(prepared.project);
    }
    let project = t.workflow.candidate.as_ref().ok_or("Color candidate is missing")?;
    let mut view = t.view;
    let mut brush = t.brush.clone();
    layer_render::remap_document_colors(t.workflow.original.document.color.space, project.document.color.space, &mut brush, &mut view);
    if !t.workflow.is_history() {
        for (source, background) in [
            (&t.workflow.original, t.view.background_rgba_linear),
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
    let color = project.document.color;
    if !t.workflow.is_history() { t.workflow.comparison_completed()?; }
    if !copy {
        let mut canvas = t
            .gpu
            .color_canvas(t.workflow.candidate.as_ref().unwrap().clone(), &brush, view, t.time, t.control.clone())
            .map_err(error)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while !canvas.poll().map_err(error)? {
            if std::time::Instant::now() > deadline {
                return Err("Color canvas preparation timed out".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        t.renderer = Some(canvas.take_ready().map_err(error)?.into());
    }
    serde_json::to_string(
        &serde_json::json!({"color":color,"clipped_channels":t.clipped}),
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
        let s = &mut a.host.session;
        let prepared = t.workflow.prepare_commit(s, t.control.is_cancelled(), a.gpu_generation == t.generation)?;
        let mut next = t.renderer.take().ok_or("Color canvas is not ready")?;
        let [width, height] = s.state().camera.viewport;
        next.resize_surface(width, height).map_err(error)?;
        t.renderer = Some(next);
        s.commit_document_color_candidate(prepared, |renderer| std::mem::swap(&mut renderer.0, &mut t.renderer))?;
        // The job retains the old GPU state for destruction on IO.
        s.complete_document_request(t.workflow.identity.request(), Ok(true))?;
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
        let project = t.workflow.copy_project(t.control.is_cancelled())?;
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
