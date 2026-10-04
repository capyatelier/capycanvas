use jni::{JNIEnv, objects::{JClass, JObject}, sys::{jlong, jobjectArray}};
use layer_ui::HistogramView;
use crate::android::{app, error, or_throw};

#[derive(Default)]
pub(crate) struct Cache {
    views: [Option<HistogramView>; 3],
    colors: Option<[[u8; 3]; 4]>,
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_takeScopes(mut env: JNIEnv, _: JClass, handle: jlong) -> jobjectArray {
    let result = (|| {
        let a = unsafe { app(handle) };
        let state = a.host.session.state();
        let colors = state.palette.histogram_colors().map(|color| color.0);
        let changed = a.scopes.colors != Some(colors);
        let mut plots = serde_json::Map::new();
        let mut image = None;
        for (index, (name, view)) in [("histogram", &state.histogram), ("waveform", &state.waveform), ("tonal_histogram", &state.tonal_histogram)].into_iter().enumerate() {
            if !changed && a.scopes.views[index].as_ref().is_some_and(|old| old.channel == view.channel && old.logarithmic == view.logarithmic
                && old.data.as_ref().map(std::sync::Arc::as_ptr) == view.data.as_ref().map(std::sync::Arc::as_ptr)) { continue; }
            let mut value = serde_json::json!({"plot": if index == 1 {Vec::new()} else {view.histogram_plot()}});
            if index == 1 {
                image = view.waveform_rgba(colors);
                value["extent"] = serde_json::json!(image.as_ref().map(|(extent, _)| *extent));
            }
            plots.insert(name.into(), value);
            a.scopes.views[index] = Some(view.clone());
        }
        a.scopes.colors = Some(colors);
        if plots.is_empty() { return Ok(std::ptr::null_mut()); }
        let result = env.new_object_array(2, "java/lang/Object", JObject::null()).map_err(error)?;
        let header = env.new_string(serde_json::json!({"plots": plots, "colors": colors}).to_string()).map_err(error)?;
        env.set_object_array_element(&result, 0, header).map_err(error)?;
        if let Some((_, bytes)) = image {
            let pixels = unsafe { jni::objects::JIntArray::from_raw(crate::android::argb_array(&mut env, &bytes)?) };
            env.set_object_array_element(&result, 1, pixels).map_err(error)?;
        }
        Ok(result.into_raw())
    })();
    or_throw(&mut env, result, std::ptr::null_mut())
}
