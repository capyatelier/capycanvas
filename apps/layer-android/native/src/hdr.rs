//! Host-owned asynchronous HDR analysis. Workers never borrow the render owner.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jint, jlong, jstring},
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use layer_ui::proof_workflow::ToneKey;
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct ToneState {
    key: Option<ToneKey>,
    owner: u64,
    pub generation: u32,
    pub published_generation: Option<u32>,
    published: Option<ToneKey>,
    ready: bool,
    publications: u32,
    pub pending: Option<CaptureControl>,
    pub guide: Option<Arc<layer_render_wgpu::local_tone::GpuToneGuide>>,
    error: Option<String>,
    analysed_time: f32,
}
impl ToneState {
    pub fn clear_incompatible(
        &mut self,
        session: &layer_ui::UiSession<layer_host::Renderer>,
        owner: u64,
    ) {
        if self.owner != owner
            || self
                .published
                .as_ref()
                .is_some_and(|key| !key.can_preview_current(session))
        {
            if let Some(control) = self.pending.take() {
                control.cancel();
            }
            self.guide = None;
            self.published = None;
            self.published_generation = None;
            self.ready = false;
        }
    }
}
struct Task {
    key: ToneKey,
    owner: u64,
    project: Option<layer_core::Project>,
    gpu: SnapshotGpu,
    background: [f32; 4],
    time: f32,
    control: CaptureControl,
    guide: Option<Arc<layer_render_wgpu::local_tone::GpuToneGuide>>,
}
unsafe fn task<'a>(id: jlong) -> &'a mut Task {
    unsafe { &mut *(id as *mut Task) }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofControl(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    action: JString,
) {
    let result = (|| {
        let action = serde_json::from_str(&read(&mut env, &action)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let previous = a.host.session.state().revision;
        let change = layer_ui::proof_panel::apply(&mut a.host.session, action)?;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_toneStatus(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    let a = unsafe { app(handle) };
    let key = ToneKey::current(&a.host.session);
    if key != a.tone.key || a.tone.owner != a.gpu_generation {
        let retain = a.tone.owner == a.gpu_generation
            && a.tone
                .published
                .as_ref()
                .zip(key.as_ref())
                .is_some_and(|(old, next)| old.can_preview(next));
        if let Some(control) = a.tone.pending.take() {
            control.cancel();
        }
        if !retain {
            a.tone.guide = None;
            a.tone.published = None;
            a.tone.published_generation = None;
        }
        a.tone.ready = false;
        a.tone.key = key;
        a.tone.owner = a.gpu_generation;
        a.tone.generation = a.tone.generation.wrapping_add(1);
        a.tone.error = None;
        a.host.dirty = true;
    }
    let animated = a.host.session.engine().document().has_animated_effects()
        && (a.host.session.engine().animation_time() - a.tone.analysed_time).abs() >= 0.5;
    string(&mut env,Ok(serde_json::json!({"generation":a.tone.generation,"hdr":a.tone.key.is_some(),"ready":a.tone.ready,"retained":a.tone.guide.is_some(),"publications":a.tone.publications,"idle":a.host.session.require_document_snapshot_idle().is_ok(),"error":a.tone.error,
        "display_hdr":a.hdr_capable(),"display_headroom":a.hdr_headroom(),"reported_headroom":a.display_headroom,"requested_headroom":a.requested_headroom(),"proof_mode":a.host.session.proof_panel_mode(),
        "needed":a.tone.key.is_some()&&(!a.tone.ready||animated)&&a.tone.error.is_none()&&a.host.session.require_document_snapshot_idle().is_ok()}).to_string()))
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_toneTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    control: jlong,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let s = &a.host.session;
        s.require_document_snapshot_idle()?;
        let control = crate::inspection::control(control);
        if let Some(previous) = a.tone.pending.replace(control.clone()) {
            previous.cancel();
        }
        Ok(Box::into_raw(Box::new(Task {
            key: ToneKey::current(s).ok_or("HDR analysis requires HDR artwork")?,
            owner: a.gpu_generation,
            project: Some(s.capture_project_recovery()?),
            gpu: s
                .engine()
                .backend()
                .0
                .as_ref()
                .ok_or("Canvas unavailable")?
                .snapshot_gpu(),
            background: s.engine().view().background_rgba_linear,
            time: s.engine().animation_time(),
            control,
            guide: None,
        })) as jlong)
    })();
    match result {
        Ok(id) => id,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_toneWork(mut env: JNIEnv, _: JClass, id: jlong) {
    let t = unsafe { task(id) };
    let result = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("capy-hdr-analysis".into())
            .stack_size(8 * 1024 * 1024)
            .spawn_scoped(scope, || {
                let mut renderer = t
                    .gpu
                    .capture(
                        t.project.take().ok_or("HDR analysis already consumed")?,
                        t.background,
                        t.time,
                        Default::default(),
                        t.control.clone(),
                    )
                    .map_err(error)?;
                t.guide = Some(renderer.gpu_local_tone_guide()?);
                Ok(())
            })
            .map_err(error)?
            .join()
            .map_err(|_| "HDR analysis worker stopped".to_string())?
    });
    fail(&mut env, result)
}
/// Hardware conformance hook: bounded fixture only, invoked by instrumentation
/// on its worker. Production guide preparation never executes the CPU oracle.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_toneReferenceDifference(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
) -> jstring {
    let t = unsafe { task(id) };
    let result = (|| {
        let project = t
            .project
            .as_ref()
            .ok_or("Analysis already consumed")?
            .clone();
        let extent = [project.document.width, project.document.height];
        if u64::from(extent[0]) * u64::from(extent[1]) > 1_000_000 {
            return Err("Conformance fixture must be at most one megapixel".into());
        }
        let space = project.document.color.space;
        let mut capture = t
            .gpu
            .capture(
                project,
                t.background,
                t.time,
                Default::default(),
                t.control.clone(),
            )
            .map_err(error)?;
        let gpu = capture.local_tone_guide()?;
        let mut reference = layer_core::color::hdr::LocalToneBuilder::new(extent, space)?;
        for y in (0..extent[1]).step_by(64) {
            let pixels = capture
                .read_region([0, y, extent[0], 64.min(extent[1] - y)])
                .map_err(error)?;
            for row in pixels.chunks_exact(extent[0] as usize) {
                reference.push(row)?;
            }
        }
        let reference = reference.finish(|| t.control.is_cancelled())?;
        let mut difference = [0f32; 3];
        for (a, b) in gpu.samples.iter().zip(&reference.samples) {
            for c in 0..3 {
                difference[c] = difference[c].max((a[c] - b[c]).abs());
            }
        }
        Ok(serde_json::json!({"max_error":difference,"guide_extent":gpu.extent,"peak_error":(gpu.peak-reference.peak).abs()}).to_string())
    })();
    string(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_toneApply(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jlong,
) -> jboolean {
    let a = unsafe { app(handle) };
    let t = unsafe { task(id) };
    if t.control.is_cancelled()
        || t.owner != a.gpu_generation
        || a.host.session.require_document_snapshot_idle().is_err()
        || ToneKey::current(&a.host.session).as_ref() != Some(&t.key)
    {
        return 0;
    }
    let Some(guide) = &t.guide else {
        fail(&mut env, Err("HDR analysis has no completed guide".into()));
        return 0;
    };
    a.tone.guide = Some(guide.clone());
    a.tone.published = Some(t.key.clone());
    a.tone.published_generation = Some(a.tone.generation);
    a.tone.ready = true;
    a.tone.publications = a.tone.publications.wrapping_add(1);
    a.tone.error = None;
    a.tone.analysed_time = t.time;
    a.host.dirty = true;
    1
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_toneFailed(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    generation: jint,
    message: JString,
) {
    let a = unsafe { app(handle) };
    let result = read(&mut env, &message).map(|e| {
        if a.tone.generation == generation as u32 {
            a.tone.error = Some(e)
        }
    });
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_toneRelease(_: JNIEnv, _: JClass, id: jlong) {
    if id != 0 {
        drop(unsafe { Box::from_raw(id as *mut Task) })
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofTexture(
    mut env: JNIEnv,
    _: JClass,
    edge: jint,
) -> jni::sys::jintArray {
    let bytes = layer_ui::proof_panel::sdr_direction_texture(edge.clamp(1, 512) as u32);
    let pixels: Vec<i32> = bytes
        .chunks_exact(4)
        .map(|p| i32::from_be_bytes([p[3], p[0], p[1], p[2]]))
        .collect();
    let result = (|| {
        let array = env.new_int_array(pixels.len() as i32).map_err(error)?;
        env.set_int_array_region(&array, 0, &pixels)
            .map_err(error)?;
        Ok(array.into_raw())
    })();
    match result {
        Ok(a) => a,
        Err(e) => {
            fail(&mut env, Err(e));
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorFieldMapped(
    mut env: JNIEnv,
    _: JClass,
    size: jint,
    state: JString,
    rendition: JString,
) -> jni::sys::jintArray {
    let result = (|| {
        if !(1..=2048).contains(&size) {
            return Err("Invalid picker size".into());
        }
        let state: layer_ui::ColorState =
            serde_json::from_str(&read(&mut env, &state)?).map_err(error)?;
        let rendition = serde_json::from_str(&read(&mut env, &rendition)?).map_err(error)?;
        let mut bytes = vec![0; size as usize * size as usize * 4];
        if !state.render_field_mapped(size as u32, rendition, &mut bytes) {
            return Err("Invalid picker field".into());
        }
        let pixels: Vec<i32> = bytes
            .chunks_exact(4)
            .map(|p| i32::from_be_bytes([p[3], p[0], p[1], p[2]]))
            .collect();
        let array = env.new_int_array(pixels.len() as i32).map_err(error)?;
        env.set_int_array_region(&array, 0, &pixels)
            .map_err(error)?;
        Ok(array.into_raw())
    })();
    match result {
        Ok(v) => v,
        Err(e) => {
            fail(&mut env, Err(e));
            std::ptr::null_mut()
        }
    }
}
