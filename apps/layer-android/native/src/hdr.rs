//! HDR display status, proof control and mapped picker fields for Kotlin.
use crate::color_preferences::{ColorCallError, color_or_throw};
use crate::android::{app, argb_array, error, or_throw, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jint, jlong, jstring},
};

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofControl(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    action: JString,
) {
    let result = (|| -> Result<_, ColorCallError> {
        let action = serde_json::from_str(&read(&mut env, &action)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let previous = a.host.session.state().revision;
        let change = layer_ui::proof_panel::apply(&mut a.host.session, action)?;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    color_or_throw(&mut env, result, ())
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_toneStatus(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    let a = unsafe { app(handle) };
    let changed = a.tick_tone() | a.screen_tick();
    let s = &a.host.session;
    let mut status = a.tone.status();
    for (key, value) in [
        ("changed", changed.into()),
        ("hdr", (s.engine().document().composition().color.depth.is_float() && !s.rendering_suspended()).into()),
        ("idle", s.require_document_snapshot_idle().is_ok().into()),
        ("display_hdr", a.hdr_capable().into()),
        ("hdr_output", a.hdr_output().into()),
        ("proof_mode", serde_json::json!(s.proof_panel_mode())),
    ] {
        status[key] = value;
    }
    string(&mut env, Ok(status.to_string()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofTexture(
    mut env: JNIEnv,
    _: JClass,
    edge: jint,
) -> jni::sys::jintArray {
    let bytes = layer_ui::proof_panel::sdr_direction_texture(edge.clamp(1, 512) as u32);
    let result = argb_array(&mut env, &bytes);
    or_throw(&mut env, result, std::ptr::null_mut())
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
        argb_array(&mut env, &bytes)
    })();
    or_throw(&mut env, result, std::ptr::null_mut())
}
