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

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_setImageObjectAffine(
    mut env: JNIEnv, _: JClass, handle: jlong, object: JString, affine: JString,
) -> jlong {
    let result = (|| {
        let id: PortableId = read(&mut env, &object)?.parse().map_err(error)?;
        let affine: Affine64 = serde_json::from_str(&read(&mut env, &affine)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let object = a.host.session.engine().document().artwork.objects.resolve(id).ok_or("Unknown image object")?;
        let previous = a.host.session.state().revision;
        let change = a.host.session.set_image_object_affine(object, affine)?;
        a.host.apply_change(previous, change);
        Ok(a.host.session.engine().document().revision as jlong)
    })();
    or_throw(&mut env, result, -1)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_setImageObjectMotion(
    mut env: JNIEnv, _: JClass, handle: jlong, object: JString, moving: jboolean,
) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let object = if moving != 0 {
            let id: PortableId = read(&mut env, &object)?.parse().map_err(error)?;
            Some(a.host.session.engine().document().artwork.objects.resolve(id).ok_or("Unknown image object")?)
        } else { None };
        let previous = a.host.session.state().revision;
        let change = a.host.session.set_image_object_motion(object)?;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result)
}
