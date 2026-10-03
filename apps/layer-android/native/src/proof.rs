//! JNI ownership: App stays on the render Looper; Task is borrowed by exactly
//! one IO job at a time. CaptureControl alone is shared with cancellation.
use crate::color_preferences::{ColorCallError, color_or_throw};
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::proof_workflow::ProofPreparation;
use std::sync::Arc;
struct Task {
    job: ProofPreparation,
    control: CaptureControl,
    lut: Option<Arc<layer_color::ProofLut>>,
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_presentationTimings(
    mut env: JNIEnv, _: JClass, handle: jlong, enabled: jboolean,
) -> jstring {
    string(&mut env, Ok(unsafe { app(handle) }.presentation_timings(enabled != 0).to_string()))
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    recipe: JString,
    control: jlong,
) -> jlong {
    let result = (|| -> Result<_, ColorCallError> {
        let recipe: Option<layer_core::color::ProofRecipe> = serde_json::from_str(&read(&mut env, &recipe)?).map_err(error)?;
        let job = if id < 0 { ProofPreparation::panel(&unsafe { app(handle) }.host.session, recipe.ok_or(layer_ui::ColorFeatureError::ProofChooseProfile)?)? } else { ProofPreparation::begin(
            &unsafe { app(handle) }.host.session,
            (id != 0).then_some(id as u32),
            recipe,
        )? };
        Ok(Box::into_raw(Box::new(Task {
            job,
            control: crate::inspection::control(control),
            lut: None,
        })) as jlong)
    })();
    color_or_throw(&mut env, result, 0)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofWork(mut env: JNIEnv, _: JClass, id: jlong) {
    let t = unsafe { crate::inspection::borrow::<Task>(id) };
    let result = t
        .job
        .build(|| t.control.is_cancelled())
        .map(|lut| t.lut = Some(lut));
    color_or_throw(&mut env, result.map_err(Into::into), ());
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofCheck(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jlong,
) {
    let t = unsafe { crate::inspection::borrow::<Task>(id) };
    color_or_throw(
        &mut env,
        if t.control.is_cancelled() {
            Err(layer_ui::ColorFeatureError::ProofCancelled)
        } else {
            t.job.validate(&unsafe { app(handle) }.host.session)
        }.map_err(Into::into), (),
    );
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofPreservation(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
) -> jbyteArray {
    match unsafe { crate::inspection::borrow::<Task>(id) }.job.preservation() {
        None => std::ptr::null_mut(),
        Some(bytes) => match env.byte_array_from_slice(bytes) {
            Ok(v) => v.into_raw(),
            Err(e) => {
                fail(&mut env, Err(error(e)));
                std::ptr::null_mut()
            }
        },
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofApply(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jlong,
    preserved: jboolean,
) {
    let result = (|| -> Result<_, ColorCallError> {
        let (a, t) = unsafe { (app(handle), crate::inspection::borrow::<Task>(id)) };
        if t.control.is_cancelled() {
            return Err(layer_ui::ColorFeatureError::ProofCancelled.into());
        }
        let lut = t.lut.clone().ok_or(layer_ui::ColorFeatureError::ProofPreviewNotPrepared)?;
        let previous = a.host.session.state().revision;
        let change = t.job.apply(&mut a.host.session, preserved != 0)?;
        a.host.proof.retain(&t.job, lut)?;
        a.host.apply_change(previous, change);
        a.host.dirty = true;
        Ok(())
    })();
    color_or_throw(&mut env, result, ());
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofFailed(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jlong,
    message: JString,
) {
    let result = (|| {
        let message = read(&mut env, &message)?;
        let (a, t) = unsafe { (app(handle), crate::inspection::borrow::<Task>(id)) };
        a.host.proof.fail(&a.host.session, &t.job, message);
        Ok(())
    })();
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofFailedReason(
    mut env: JNIEnv, _: JClass, handle: jlong, id: jlong, reason: JString,
) {
    let result = (|| {
        let reason = serde_json::from_str(&read(&mut env, &reason)?).map_err(error)?;
        let (a, t) = unsafe { (app(handle), crate::inspection::borrow::<Task>(id)) };
        a.host.proof.fail_reason(&a.host.session, &t.job, reason);
        Ok(())
    })();
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofRelease(_: JNIEnv, _: JClass, id: jlong) {
    if id != 0 {
        drop(unsafe { Box::from_raw(id as *mut Task) });
    }
}
