use crate::android::{app, error, fail, or_throw, read, string};
use jni::{JNIEnv, objects::{JClass, JString}, sys::{jboolean, jlong, jstring}};
use layer_core::authored::{Affine64, PortableId};

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_imageObjects(
    mut env: JNIEnv, _: JClass, handle: jlong,
) -> jstring {
    let a = unsafe { app(handle) };
    let document = a.host.session.engine().document();
    let mut sources = Vec::new();
    let rows: Vec<_> = document.artwork.objects.iter().map(|(object, id, value)| {
        let source = value.image.storage();
        let owner = sources.iter().position(|previous| std::sync::Arc::ptr_eq(previous, source)).unwrap_or_else(|| {
            sources.push(source.clone()); sources.len()-1
        });
        serde_json::json!({"id":id, "image":value.image.id(), "name":value.name,
            "source_owner":owner,
            "paint_base_image_shared":document.artwork.paint.iter().any(|(_,_,paint)| paint.base.as_ref().is_some_and(|base|base.image.id()==value.image.id())),
            "paint_base_source_shared":document.artwork.paint.iter().any(|(_,_,paint)| paint.base.as_ref().is_some_and(|base|std::sync::Arc::ptr_eq(base.image.storage(),source))),
            "affine":value.affine, "visible":value.visible, "extent":value.image.extent,
            "owner":document.scene().object_owner(object).map(|owner|owner.index())})
    }).collect();
    string(&mut env, serde_json::to_string(&rows).map_err(error))
}

fn motion_change(handle: jlong, update: impl FnOnce(&mut layer_host::NativeHost) -> Result<layer_ui::UiChange, String>) -> Result<jlong, String> {
    let a = unsafe { app(handle) };
    let previous = a.host.session.state().revision;
    let change = update(&mut a.host)?;
    a.host.apply_change(previous, change);
    Ok(a.host.session.engine().document().revision as jlong)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_beginObjectMotion(
    mut env: JNIEnv, _: JClass, handle: jlong, objects: JString,
) {
    let result = (|| {
        let ids: Vec<PortableId> = serde_json::from_str(&read(&mut env, &objects)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let store = &a.host.session.engine().document().artwork.objects;
        let objects = ids.iter().map(|id| store.resolve(*id).ok_or("Unknown image object")).collect::<Result<Vec<_>, _>>()?;
        motion_change(handle, |host| host.session.begin_object_motion(&objects)).map(|_| ())
    })();
    fail(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_previewObjectMotion(
    mut env: JNIEnv, _: JClass, handle: jlong, delta: JString,
) -> jlong {
    let result = (|| {
        let delta: Affine64 = serde_json::from_str(&read(&mut env, &delta)?).map_err(error)?;
        motion_change(handle, |host| host.session.preview_object_motion(delta))
    })();
    or_throw(&mut env, result, -1)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_finishObjectMotion(
    mut env: JNIEnv, _: JClass, handle: jlong, commit: jboolean,
) -> jlong {
    let result = motion_change(handle, |host| if commit != 0 { host.session.commit_object_motion() } else { host.session.cancel_object_motion() });
    or_throw(&mut env, result, -1)
}
