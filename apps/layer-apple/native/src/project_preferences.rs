//! Stateless preferences work; Swift owns serial I/O and atomic publication.
use super::*;

pub(super) fn validate_export(recipe: &layer_ui::ExportRecipe, color: layer_core::color::DocumentColor) -> Result<(), String> {
    recipe.validate()?;
    if layer_color::profile_channels(&recipe.profile.profile)? != recipe.profile.channels {
        return Err("Profile channels do not match the ICC data".into());
    }
    layer_color::WorkingEncoder::new(color.space, &recipe.interpretation(), recipe.encoding)?;
    Ok(())
}

/// # Safety
/// Worker only. Borrows descriptors at offset zero and NUL-terminated JSON.
/// input may be -1 for defaults; changed preferences require a temporary output.
/// Returns owned view/error JSON. The host publishes output only after success.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_export_presets(input: i32, output: i32, request: *const c_char, color: *const c_char) -> *mut c_char {
    let result = (|| -> Result<serde_json::Value, String> {
        let mut library = if input < 0 { layer_ui::ExportPresets::default() } else {
            let file = ManuallyDrop::new(unsafe { File::from_raw_fd(input) });
            let mut bytes = Vec::new();
            (&*file).take(layer_ui::ExportPresets::MAX_FILE_BYTES as u64 + 1)
                .read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            layer_ui::ExportPresets::decode(&bytes)?
        };
        let color = serde_json::from_str(unsafe { read_title(color) }?).map_err(|e| e.to_string())?;
        let action = serde_json::from_str(unsafe { read_title(request) }?).map_err(|e| e.to_string())?;
        let view = library.operate(action, color, |recipe| validate_export(recipe, color))?;
        if view.changed {
            if output < 0 { return Err("No export preferences output is open".into()); }
            let mut file = ManuallyDrop::new(unsafe { File::from_raw_fd(output) });
            file.write_all(&library.encode()?).map_err(|e| e.to_string())?;
        }
        serde_json::to_value(view).map_err(|e| e.to_string())
    })();
    CString::new(result.unwrap_or_else(|error| serde_json::json!({"error":error})).to_string()).unwrap().into_raw()
}
