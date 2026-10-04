use super::*;
use layer_ui::HistogramView;
use layer_core::ArtworkSource;

#[derive(Default)]
pub(super) struct Cache {
    views: [Option<HistogramView>; 3],
    colors: Option<[[u8; 3]; 4]>,
    plots: [Option<JsValue>; 3],
    image: Option<JsValue>,
}

pub(super) fn publish(state: &layer_ui::UiState, cache: &mut Cache, result: &js_sys::Object) -> Result<(), JsValue> {
    let colors = state.palette.histogram_colors().map(|color| color.0);
    let color_changed = cache.colors != Some(colors);
    if color_changed {
        js_sys::Reflect::set(result, &js("scope_colors"), &serialize(&colors)?)?;
        cache.colors = Some(colors);
    }
    for (index, (name, view)) in [("histogram", &state.histogram), ("waveform", &state.waveform), ("tonal_histogram", &state.tonal_histogram)].into_iter().enumerate() {
        if !color_changed && cache.views[index].as_ref().is_some_and(|old| old.same_publication(view)) { continue; }
        let redraw=color_changed || cache.views[index].as_ref().is_none_or(|old| old.channel!=view.channel || old.logarithmic!=view.logarithmic
            || old.data.as_ref().map(std::sync::Arc::as_ptr)!=view.data.as_ref().map(std::sync::Arc::as_ptr));
        let value = serialize(view)?;
        publish_source(view,&value)?;
        if redraw { cache.plots[index]=Some(serialize(&view.histogram_plot())?); }
        js_sys::Reflect::set(&value, &js("plot"), cache.plots[index].as_ref().unwrap())?;
        if index == 1 {
            if redraw {
                cache.image=Some(if let Some(([width, height], bytes)) = view.waveform_rgba(colors) {
                    let image = js_sys::Object::new();
                    js_sys::Reflect::set(&image, &js("width"), &JsValue::from(width))?;
                    js_sys::Reflect::set(&image, &js("height"), &JsValue::from(height))?;
                    js_sys::Reflect::set(&image, &js("bytes"), &js_sys::Uint8Array::from(bytes.as_slice()))?;
                    image.into()
                } else { JsValue::NULL });
            }
            js_sys::Reflect::set(&value, &js("image"), cache.image.as_ref().unwrap())?;
        }
        js_sys::Reflect::set(result, &js(name), &value)?;
        cache.views[index] = Some(view.clone());
    }
    Ok(())
}

pub(super) fn publish_source(view:&HistogramView, value:&JsValue) -> Result<(),JsValue> {
    let source=match view.captured_source.as_ref() {
        Some(ArtworkSource::Visible)=>serde_json::json!("Visible"),
        Some(ArtworkSource::Reference)=>serde_json::json!("Reference"),
        Some(ArtworkSource::Source(target))=>serde_json::json!({"Source":target}),
        Some(ArtworkSource::EffectInput(handle))=>serde_json::json!({"EffectInput":layer_ui::occurrence_token(*handle)}),
        Some(ArtworkSource::EffectChannels(handle))=>serde_json::json!({"EffectChannels":layer_ui::occurrence_token(*handle)}),
        Some(ArtworkSource::EffectBaseline(baseline))=>serde_json::json!({"EffectBaseline":layer_ui::occurrence_token(baseline.occurrence)}),
        None=>serde_json::Value::Null,
    };
    js_sys::Reflect::set(value,&js("captured_source"),&serialize(&source)?)?;
    Ok(())
}
