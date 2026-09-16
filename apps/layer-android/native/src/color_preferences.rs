//! Stateless worker operations; Kotlin owns atomic application-file publication.
use crate::android::{error, fail, read};
use jni::{
    JNIEnv,
    objects::{JByteArray, JClass, JObject, JString},
    sys::jobjectArray,
};
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_exportPresets(
    mut env: JNIEnv,
    _: JClass,
    bytes: JByteArray,
    request: JString,
    color: JString,
) -> jobjectArray {
    let result = (|| {
        let bytes = env.convert_byte_array(bytes).map_err(error)?;
        let mut library = if bytes.is_empty() {
            layer_ui::ExportPresets::default()
        } else {
            layer_ui::ExportPresets::decode(&bytes)?
        };
        let request = serde_json::from_str(&read(&mut env, &request)?).map_err(error)?;
        let color: layer_core::color::DocumentColor =
            serde_json::from_str(&read(&mut env, &color)?).map_err(error)?;
        let view = library.operate(request, color, |recipe| {
            recipe.validate()?;
            if layer_color::profile_channels(&recipe.profile.profile)? != recipe.profile.channels {
                return Err("Profile channels do not match the ICC data".into());
            }
            layer_color::WorkingEncoder::new(
                color.space,
                &recipe.interpretation(),
                recipe.encoding,
            )?;
            Ok(())
        })?;
        let result = env
            .new_object_array(2, "java/lang/Object", JObject::null())
            .map_err(error)?;
        let json = env
            .new_string(serde_json::to_string(&view).map_err(error)?)
            .map_err(error)?;
        env.set_object_array_element(&result, 0, json)
            .map_err(error)?;
        if view.changed {
            let bytes = env
                .byte_array_from_slice(&library.encode()?)
                .map_err(error)?;
            env.set_object_array_element(&result, 1, bytes)
                .map_err(error)?;
        }
        Ok(result.into_raw())
    })();
    match result {
        Ok(v) => v,
        Err(e) => {
            fail(&mut env, Err(e));
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectProfileSummary(
    mut env: JNIEnv,
    _: JClass,
    bytes: JByteArray,
) -> jni::sys::jstring {
    let result = (|| {
        if env.get_array_length(&bytes).map_err(error)? as usize > layer_color::MAX_ICC_BYTES {
            return Err("ICC profile exceeds 16 MiB".into());
        }
        let profile = layer_core::color::ColorProfile::Icc(
            env.convert_byte_array(bytes).map_err(error)?.into(),
        );
        serde_json::to_string(&serde_json::json!({"name":layer_color::profile_description(&profile)?,"channels":layer_color::profile_channels(&profile)?})).map_err(error)
    })();
    crate::android::string(&mut env, result)
}
