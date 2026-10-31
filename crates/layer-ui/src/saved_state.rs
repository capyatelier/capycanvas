use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

pub(crate) fn restore_fields<T: DeserializeOwned + Serialize>(
    value: Value,
    defaults: impl FnOnce() -> T,
    validate: impl Fn(&T) -> bool,
) -> T {
    let valid = |value: &Value| serde_json::from_value::<T>(value.clone()).ok().filter(&validate);
    if let Some(restored) = valid(&value) { return restored; }
    let defaults = defaults();
    let mut kept = serde_json::to_value(&defaults).unwrap();
    if let Value::Object(fields) = value {
        for (key, value) in fields {
            let mut candidate = kept.clone();
            candidate[key] = value;
            if valid(&candidate).is_some() { kept = candidate; }
        }
    }
    valid(&kept).unwrap_or(defaults)
}

#[cfg(test)]
mod tests {
    use crate::{EditingState, Localizer, UiLanguage, WorkspaceWorkingState};
    #[test]
    fn editing_restore_keeps_valid_fields_and_converts_only_the_released_version() {
        let mut state = EditingState::default();
        state.colors.set_rgba([0.2,0.4,0.6,1.]).unwrap();
        let mut saved = serde_json::to_value(&state).unwrap();
        saved["preset"] = "unreadable".into();
        saved["unknown_option"] = true.into();
        let english = Localizer::shared(UiLanguage::English);
        let restored = EditingState::restore(&saved.to_string(), &english);
        assert_eq!(restored,state);
        for version in [1,2,999] {
            saved["version"] = version.into();
            saved["region_sources"] = serde_json::json!(["reference","visible"]);
            saved["region_values"]["smoothing"] = 0.into();
            let restored = EditingState::restore(&saved.to_string(), &english);
            assert_eq!(restored.colors,state.colors);
            assert_eq!(restored.version,3);
            if version == 1 {
                assert_eq!(restored.region_sources,[crate::RegionSource::Reference,crate::RegionSource::Visible,crate::RegionSource::Auto]);
                assert!(!restored.region_smooth_edges);
            } else {
                assert_eq!(restored.region_sources,EditingState::default().region_sources);
                assert!(restored.region_smooth_edges);
            }
        }
    }
    #[test]
    fn unreadable_palettes_use_localized_defaults_without_losing_other_fields() {
        let japanese = Localizer::shared(UiLanguage::Japanese);
        let mut state = EditingState::new_localized(&japanese);
        assert_ne!(state.color_library, EditingState::default().color_library);
        state.colors.set_rgba([0.2,0.4,0.6,1.]).unwrap();
        let mut saved = serde_json::to_value(&state).unwrap();
        saved["color_library"]["palettes"] = "unreadable".into();
        assert_eq!(EditingState::restore(&saved.to_string(), &japanese), state);
    }
    #[test]
    fn working_restore_defaults_only_invalid_fields() {
        assert_eq!(WorkspaceWorkingState::restore(serde_json::json!({"version":"future","zen_mode":true,"option":1})),WorkspaceWorkingState {version:1,zen_mode:true});
        assert_eq!(WorkspaceWorkingState::restore(serde_json::json!({"version":999,"zen_mode":{}})),WorkspaceWorkingState::default());
    }
}
