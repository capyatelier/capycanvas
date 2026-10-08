//! Provider descriptors are decoded on the file worker. Nothing enters the live
//! document until the complete, interpreted batch passes identity checks.
use crate::android::{app, error, fail, or_throw, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jint, jlong, jstring},
};
use layer_ui::{DocumentRequest, ImagePlacementContext};
use std::{
    fs::File,
    io::BufReader,
    os::fd::FromRawFd,
};

#[derive(serde::Serialize, serde::Deserialize)]
struct Context {
    placement: ImagePlacementContext,
    generation: u64,
}
struct Batch {
    id: u32,
    context: Context,
    control: layer_render_wgpu::snapshot::CaptureControl,
    images: layer_ui::ImageImportBatch,
    open: Option<NewImage>,
}
struct NewImage {
    environment: layer_host::open::OpenEnvironment,
    clip: Option<layer_ui::PixelClip>,
    candidate: Option<Box<layer_ui::UiSession<layer_host::Renderer>>>,
    retired: Option<Box<layer_render_wgpu::WgpuRasterizer>>,
}
fn active(a: &crate::app::App, id: u32) -> Result<(), String> {
    if matches!(a.host.session.document_request(id), Ok(DocumentRequest::Place | DocumentRequest::Paste { .. })) {
        Ok(())
    } else {
        Err("The image import request is no longer active".into())
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_photoFormats(
    mut env: JNIEnv,
    _: JClass,
) -> jstring {
    string(
        &mut env,
        serde_json::to_string(&layer_color::photo::formats().collect::<Vec<_>>()).map_err(error),
    )
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportContext(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    screen: JString,
    destination: JString,
) -> jstring {
    let result = (|| {
        let a = unsafe { app(handle) };
        let screen = serde_json::from_str(&read(&mut env, &screen)?).map_err(error)?;
        let destination = serde_json::from_str(&read(&mut env, &destination)?).map_err(error)?;
        serde_json::to_string(&Context {
            placement: a
                .host
                .session
                .image_placement_context(screen, destination)?,
            generation: a.gpu_generation,
        })
        .map_err(error)
    })();
    string(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    context: JString,
    cancel: jlong,
    nonce: JString,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        active(a, id as u32)?;
        let context: Context = serde_json::from_str(&read(&mut env, &context)?).map_err(error)?;
        a.host
            .session
            .validate_image_placement(&context.placement)?;
        if context.generation != a.gpu_generation {
            return Err("The canvas changed while importing; try again".into());
        }
        let open = if a.host.session.pasting_new_image() {
            let nonce = read(&mut env, &nonce)?;
            let clip = if nonce.is_empty() { None } else { Some(a.window.documents.clip.as_ref().filter(|clip| clip.nonce == nonce).ok_or("The clipboard changed; paste again")?.clone()) };
            Some(NewImage {
                environment: layer_host::open::OpenEnvironment::capture(&a.host.session,
                    a.window.documents.admission(&a.host.session.retained_document_tiles()), a.host.renderer_options(Some(a.cache_directory.clone().into())))?,
                clip, candidate: None, retired: None,
            })
        } else { None };
        Ok(Box::into_raw(Box::new(Batch {
            id: id as u32,
            context,
            control: crate::inspection::control(cancel),
            images: layer_ui::ImageImportBatch::new(a.host.session.state().settings.photo_open,
                a.host.session.engine().document().composition().color.space, Default::default()),
            open,
        })) as jlong)
    })();
    or_throw(&mut env, result, 0)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportPrepare(mut env: JNIEnv, _: JClass, handle: jlong) {
    let b = unsafe { crate::inspection::borrow::<Batch>(handle) };
    let result = (|| {
        let Some(open) = &mut b.open else { return Ok(()); };
        if b.control.is_cancelled() { return Err("Paste cancelled".into()); }
        let project = match open.clip.take() {
            Some(clip) => clip.document(&open.environment.localization)?,
            None => layer_ui::clipboard_document(b.images.take_sources(b.control.is_cancelled())?, open.environment.photo_policy, &open.environment.localization)?,
        };
        open.candidate = Some(open.environment.prepare(project, || b.control.is_cancelled())?);
        Ok(())
    })();
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportParkReady(mut env: JNIEnv, _: JClass, handle: jlong, task: jlong) -> jboolean {
    let result = (|| {
        let a = unsafe { app(handle) };
        let b = unsafe { crate::inspection::borrow::<Batch>(task) };
        if b.control.is_cancelled() || b.open.as_ref().and_then(|open| open.candidate.as_ref()).is_none() { return Err("Paste cancelled or not prepared".into()); }
        if a.host.session.state().document_file.epoch != b.context.placement.epoch || a.host.session.engine().document().revision != b.context.placement.revision || a.gpu_generation != b.context.generation {
            return Err("The drawing changed while pasting; try again".into());
        }
        active(a, b.id)?;
        a.window.adoption_ready(&a.host)
    })();
    or_throw(&mut env, result.map(u8::from), 0)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportRead(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    fd: jint,
    name: JString,
) {
    if fd < 0 {
        unsafe { crate::inspection::borrow::<Batch>(handle) }.images.invalidate();
        fail(&mut env, Err("Missing image descriptor".into()));
        return;
    }
    // Take descriptor ownership even if this batch has already failed.
    let file = unsafe { File::from_raw_fd(fd) };
    let b = unsafe { crate::inspection::borrow::<Batch>(handle) };
    let result = (|| {
        let name = read(&mut env, &name)?;
        let name = name
            .rsplit_once('.')
            .map_or(name.as_str(), |(stem, _)| stem);
        let control = b.control.clone();
        let file = layer_core::Cancellable { inner: file, cancelled: move || control.is_cancelled() };
        b.images.read(BufReader::new(file), name, b.control.cancellation_flag())
    })();
    if result.is_err() {
        b.images.invalidate();
    }
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportProfilePrompt(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    let b = unsafe { crate::inspection::borrow::<Batch>(handle) };
    string(
        &mut env,
        serde_json::to_string(&b.images.pending_source().map(|s| &s.interpretation)).map_err(error),
    )
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportAssumeProfile(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    profile: JString,
) {
    let b = unsafe { crate::inspection::borrow::<Batch>(handle) };
    let result = (|| {
        let profile = serde_json::from_str(&read(&mut env, &profile)?).map_err(error)?;
        b.images.interpret(profile, b.control.is_cancelled())
    })();
    if result.is_err() {
        b.images.invalidate();
    }
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportAdopt(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    task: jlong,
) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let b = unsafe { crate::inspection::borrow::<Batch>(task) };
        active(a, b.id)?;
        if let Some(open) = &mut b.open {
            if b.control.is_cancelled() || a.gpu_generation != b.context.generation || a.gpu_watch.failure().is_some() { return Err("Paste cancelled or canvas changed".into()); }
            let adoption = layer_host::window::OpenAdoption { epoch: b.context.placement.epoch, revision: b.context.placement.revision, location: None };
            open.retired = a.window.adopt(&mut a.host, &mut open.candidate, adoption, || !b.control.is_cancelled(), |s| s)?;
            a.document_retired();
            a.project_adopted();
            return Ok(());
        }
        a.host
            .session
            .validate_image_placement(&b.context.placement)?;
        if a.gpu_generation != b.context.generation
            || a.gpu_watch.failure().is_some()
            || a.host.session.engine().backend().0.is_none()
        {
            return Err("The canvas changed while importing; try again".into());
        }
        let previous = a.host.session.state().revision;
        let sources = b.images.take_sources(b.control.is_cancelled())?;
        let mode = match a.host.session.document_request(b.id) {
            Ok(DocumentRequest::Paste { mode }) => Some(*mode),
            _ => None,
        };
        match mode {
            Some(mode) => a.host.session.paste_layer_sources(sources, mode, &b.context.placement)?,
            None => a.host.session.place_layer_sources(sources, b.context.placement.center, b.context.placement.destination)?,
        }
        let mut change = a.host.session.complete_document_request(b.id, Ok(true))?;
        change.canvas_wake = true;
        change.regions |= 255;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportFree(
    _: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut Batch) });
    }
}
