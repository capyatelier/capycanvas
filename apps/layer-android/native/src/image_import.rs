//! Provider descriptors are decoded on the file worker. Nothing enters the live
//! document until the complete, interpreted batch passes identity checks.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jint, jlong, jstring},
};
use layer_ui::{DocumentRequest, HostRequestKind, ImagePlacementContext};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
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
}
unsafe fn batch<'a>(handle: jlong) -> &'a mut Batch {
    unsafe { &mut *(handle as *mut Batch) }
}
fn active(a: &crate::app::App, id: u32) -> Result<(), String> {
    if a.host.session.state().requests.iter().any(|r| {
        r.id == id
            && matches!(
                r.kind,
                HostRequestKind::Document {
                    request: DocumentRequest::Place | DocumentRequest::Paste
                }
            )
    }) {
        Ok(())
    } else {
        Err("The image import request is no longer active".into())
    }
}
struct CancelRead {
    file: File,
    control: layer_render_wgpu::snapshot::CaptureControl,
}
impl Read for CancelRead {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.control.is_cancelled() {
            return Err(std::io::Error::other("Image import cancelled"));
        }
        self.file.read(out)
    }
}
impl Seek for CancelRead {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(from)
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
        Ok(Box::into_raw(Box::new(Batch {
            id: id as u32,
            context,
            control: crate::inspection::control(cancel),
            images: layer_ui::ImageImportBatch::new(a.host.session.state().settings.photo_open,
                a.host.session.engine().document().color.space, Default::default()),
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
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageImportRead(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    fd: jint,
    name: JString,
) {
    if fd < 0 {
        unsafe { batch(handle) }.images.invalidate();
        fail(&mut env, Err("Missing image descriptor".into()));
        return;
    }
    // Take descriptor ownership even if this batch has already failed.
    let file = unsafe { File::from_raw_fd(fd) };
    let b = unsafe { batch(handle) };
    let result = (|| {
        let name = read(&mut env, &name)?;
        let name = name
            .rsplit_once('.')
            .map_or(name.as_str(), |(stem, _)| stem);
        b.images.read(BufReader::new(CancelRead { file, control: b.control.clone() }), name, b.control.cancellation_flag())
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
    let b = unsafe { batch(handle) };
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
    let b = unsafe { batch(handle) };
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
        let b = unsafe { batch(task) };
        active(a, b.id)?;
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
        a.host.session.place_layer_sources(
            b.images.take_sources(b.control.is_cancelled())?,
            b.context.placement.center,
            b.context.placement.destination,
        )?;
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
