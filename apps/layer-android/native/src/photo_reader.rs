use jni::{JNIEnv, JavaVM, objects::{GlobalRef, JClass, JValue}, sys::{JNI_ERR, JNI_VERSION_1_6, jboolean, jint, jlong}};
use layer_color::photo::{DecodeLimits, DecodedPhoto};
use layer_host::photo_reader::Input;
use std::{fs::File, io::{BufWriter, Write}, os::fd::{AsRawFd, FromRawFd}, sync::{OnceLock, atomic::AtomicBool}};

static READER: OnceLock<(JavaVM, GlobalRef)> = OnceLock::new();

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _: *mut std::ffi::c_void) -> jint {
    crate::jni_entry!(result => { result.unwrap_or(JNI_ERR) }; {
        let class = vm.get_env().and_then(|mut env| { let class = env.find_class("art/capycanvas/PhotoReader")?; env.new_global_ref(class) })
            .map_err(|error| error.to_string())?;
        READER.set((vm, class)).map_err(|_| "Photo decoder is already initialized".to_owned())?;
        Ok(JNI_VERSION_1_6)
    })
}

pub(crate) fn read(input: &mut dyn Input, limits: DecodeLimits, cancelled: &AtomicBool) -> Result<DecodedPhoto, String> {
    layer_host::photo_reader::decode(input, limits, cancelled, |job| {
        let (vm, class) = READER.get().ok_or("Photo decoder is unavailable")?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let [input, output, errors] = [&job.input, &job.output, &job.errors].map(|file| JValue::Int(file.as_raw_fd()));
        let class: &JClass = class.as_obj().into();
        env.call_static_method(class, "decode", "(IIIJJI)Z", &[input, output, errors, JValue::Long(limits.source_bytes as jlong),
            JValue::Long(limits.codec_bytes as jlong), JValue::Int(limits.dimension as jint)]).and_then(|value| value.z())
            .map_err(|e| { let _ = env.exception_clear(); format!("Photo decoder is unavailable: {e}") })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_servePhoto(_: JNIEnv, _: JClass, input: jint, output: jint, errors: jint, source_bytes: jlong, codec_bytes: jlong, dimension: jint) -> jboolean {
    crate::jni_entry!(result => {
        let mut errors = unsafe { File::from_raw_fd(errors) };
        match result {
            Ok(()) => 1,
            Err(error) => { let _ = errors.write_all(error.as_bytes()); 0 }
        }
    }; {
        let [input, output] = [input, output].map(|fd| unsafe { File::from_raw_fd(fd) });
        let number = |value: i64| usize::try_from(value).map_err(|e| e.to_string());
        let limits = DecodeLimits { source_bytes: number(source_bytes)?, codec_bytes: number(codec_bytes)?, dimension: u32::try_from(dimension).map_err(|e| e.to_string())? };
        let mut output = BufWriter::new(output);
        layer_host::photo_reader::serve(input, &mut output, limits)?;
        output.flush().map_err(|e| e.to_string())
    })
}
