use jni::{JNIEnv, JavaVM, objects::{GlobalRef, JClass, JLongArray}};
use layer_core::memory_budget::AndroidMemory;
use std::sync::OnceLock;

static SYSTEM: OnceLock<(JavaVM, GlobalRef)> = OnceLock::new();

pub(crate) fn install(env: &mut JNIEnv) -> Result<(), String> {
    let class = env.find_class("art/capycanvas/SystemMemory").and_then(|class| env.new_global_ref(class)).map_err(|error| error.to_string())?;
    let vm = env.get_java_vm().map_err(|error| error.to_string())?;
    SYSTEM.set((vm, class)).map_err(|_| "System memory is already observed".to_owned())?;
    layer_core::memory_budget::read_android_memory_with(read);
    Ok(())
}

fn read() -> Option<AndroidMemory> {
    let (vm, class) = SYSTEM.get()?;
    let mut env = vm.attach_current_thread_permanently().ok()?;
    let sample = env.with_local_frame(4, |env| -> jni::errors::Result<Option<Vec<i64>>> {
        let class: &JClass = class.as_obj().into();
        let array = env.call_static_method(class, "sample", "()[J", &[])?.l()?;
        if array.is_null() { return Ok(None); }
        let array = JLongArray::from(array);
        let mut values = vec![0; usize::try_from(env.get_array_length(&array)?).unwrap_or(0)];
        env.get_long_array_region(&array, 0, &mut values)?;
        Ok(Some(values))
    }).unwrap_or_else(|_| { let _ = env.exception_clear(); None })?;
    let [total, available, threshold, low_memory, trims @ ..] = sample.as_slice() else { return None; };
    Some(AndroidMemory { total_bytes: u64::try_from(*total).ok()?, available_bytes: u64::try_from(*available).ok()?,
        threshold_bytes: u64::try_from(*threshold).ok()?, low_memory: *low_memory != 0,
        trim_levels: trims.iter().filter_map(|level| i32::try_from(*level).ok()).collect() })
}
