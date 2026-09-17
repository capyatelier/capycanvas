//! JNI ownership: App stays on the render Looper; Task is borrowed by exactly
//! one IO job at a time. CaptureControl alone is shared with cancellation.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::proof_workflow::{ProofPreparation, proof_form};
use std::sync::Arc;
struct Task {
    job: ProofPreparation,
    control: CaptureControl,
    lut: Option<Arc<layer_color::ProofLut>>,
}
unsafe fn task<'a>(id: jlong) -> &'a mut Task {
    unsafe { &mut *(id as *mut Task) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofStatus(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    let a = unsafe { app(handle) };
    string(
        &mut env,
        serde_json::to_string(&a.proof.observe(&a.host.session)).map_err(error),
    )
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofForm(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    string(
        &mut env,
        Ok(proof_form(&unsafe { app(handle) }.host.session).to_string()),
    )
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
    let result = (|| {
        let recipe = serde_json::from_str(&read(&mut env, &recipe)?).map_err(error)?;
        let job = ProofPreparation::begin(
            &unsafe { app(handle) }.host.session,
            (id != 0).then_some(id as u32),
            recipe,
        )?;
        Ok(Box::into_raw(Box::new(Task {
            job,
            control: crate::inspection::control(control),
            lut: None,
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
pub extern "system" fn Java_art_capycanvas_Native_proofWork(mut env: JNIEnv, _: JClass, id: jlong) {
    let t = unsafe { task(id) };
    let result = t
        .job
        .build(|| t.control.is_cancelled())
        .map(|lut| t.lut = Some(lut));
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofCheck(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jlong,
) {
    let t = unsafe { task(id) };
    fail(
        &mut env,
        if t.control.is_cancelled() {
            Err("Proof preparation cancelled".into())
        } else {
            t.job.validate(&unsafe { app(handle) }.host.session)
        },
    );
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_proofPreservation(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
) -> jbyteArray {
    match unsafe { task(id) }.job.preservation() {
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
    let result = (|| {
        let (a, t) = unsafe { (app(handle), task(id)) };
        if t.control.is_cancelled() {
            return Err("Proof preparation cancelled".into());
        }
        let lut = t.lut.clone().ok_or("Proof preview is not prepared")?;
        let previous = a.host.session.state().revision;
        let change = t.job.apply(&mut a.host.session, preserved != 0)?;
        a.proof.retain(&t.job, lut)?;
        a.host.apply_change(previous, change);
        a.host.dirty = true;
        Ok(())
    })();
    fail(&mut env, result);
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
        let (a, t) = unsafe { (app(handle), task(id)) };
        a.proof.fail(&a.host.session, &t.job, message);
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
