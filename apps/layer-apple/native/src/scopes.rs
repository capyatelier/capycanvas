//! Histogram, Waveform and tonal plots for the photo scope panels and Properties.
use super::*;
use layer_ui::HistogramView;

#[derive(Default)]
pub(crate) struct Cache {
    views: [Option<HistogramView>; 3],
    colors: Option<[[u8; 3]; 4]>,
}
pub struct CapyScopes {
    plots: CString,
    pixels: Vec<u8>,
}
#[repr(C)]
pub struct CapyScopeInfo {
    pub plots: *const c_char,
    pub pixels: *const u8,
    pub count: usize,
}

/// # Safety
/// Call on the serial editor owner. The returned plots hold no editor references.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_take_scopes(app: *mut CapyApple) -> *mut CapyScopes {
    let Some(app) = (unsafe { app.as_mut() }) else { return std::ptr::null_mut(); };
    app.perform(|app| {
        let state = app.host.session.state();
        let colors = state.palette.histogram_colors().map(|color| color.0);
        let changed = app.scopes.colors != Some(colors);
        let mut plots = serde_json::Map::new();
        let mut pixels = Vec::new();
        for (index, (name, view)) in [("histogram", &state.histogram), ("waveform", &state.waveform), ("tonal_histogram", &state.tonal_histogram)].into_iter().enumerate() {
            if !changed && app.scopes.views[index].as_ref().is_some_and(|old| old.channel == view.channel && old.logarithmic == view.logarithmic
                && old.data.as_ref().map(std::sync::Arc::as_ptr) == view.data.as_ref().map(std::sync::Arc::as_ptr)) { continue; }
            let mut value = serde_json::json!({"plot": if index == 1 { Vec::new() } else { view.histogram_plot() }});
            if index == 1 {
                let image = view.waveform_premultiplied_rgba(colors);
                value["extent"] = serde_json::json!(image.as_ref().map(|(extent, _)| *extent));
                pixels = image.map(|(_, bytes)| bytes).unwrap_or_default();
            }
            plots.insert(name.into(), value);
            app.scopes.views[index] = Some(view.clone());
        }
        app.scopes.colors = Some(colors);
        if plots.is_empty() { return Ok(std::ptr::null_mut()); }
        let plots = CString::new(serde_json::json!({"plots": plots, "colors": colors}).to_string()).map_err(|e| e.to_string())?;
        Ok(Box::into_raw(Box::new(CapyScopes { plots, pixels })))
    }).unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// Both pointers must be valid. Output views are borrowed until scopes_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_scopes_read(scopes: *const CapyScopes, output: *mut CapyScopeInfo) {
    let (Some(scopes), Some(output)) = (unsafe { scopes.as_ref() }, unsafe { output.as_mut() }) else { return; };
    *output = CapyScopeInfo { plots: scopes.plots.as_ptr(), pixels: scopes.pixels.as_ptr(), count: scopes.pixels.len() };
}

/// # Safety
/// Free exactly once, after all borrowed views have finished reading.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_scopes_free(scopes: *mut CapyScopes) {
    if !scopes.is_null() { unsafe { drop(Box::from_raw(scopes)) }; }
}
