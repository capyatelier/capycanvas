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
    fn saved_documents_tolerate_missing_and_unknown_fields() {
        use serde_json::{Value,json};
        fn check(original:Value,restore:impl Fn(Value)->Value) {
            let mut unknown=original.clone();unknown["future_option"]=json!({"value":17});
            assert_eq!(restore(unknown),original);
            for removed in original.as_object().unwrap().keys() {
                let mut missing=original.clone();missing.as_object_mut().unwrap().remove(removed);
                let restored=restore(missing);
                for (field,value) in original.as_object().unwrap() {
                    if field!=removed {assert_eq!(&restored[field],value,"missing {removed}, retained {field}");}
                }
            }
        }
        let english=Localizer::shared(UiLanguage::English);
        let directory=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../layer-workspace/tests/compat/1.0.15");
        let settings=crate::Settings::restore(&std::fs::read_to_string(directory.join("settings.json")).unwrap());
        check(serde_json::to_value(settings).unwrap(),|value|serde_json::to_value(crate::Settings::restore(&value.to_string())).unwrap());
        let editing=EditingState::restore(&std::fs::read_to_string(directory.join("editing.json")).unwrap(),&english);
        let original=serde_json::to_value(editing).unwrap();let mut nested=original.clone();
        for pointer in ["/tools","/tool_slots","/colors","/mask_colors","/color_library","/selection","/gradient"] {
            nested.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("future_option".into(),true.into());
        }
        for palette in nested["color_library"]["palettes"].as_array_mut().unwrap() {
            palette["future_option"]=true.into();
            for swatch in palette["swatches"].as_array_mut().unwrap() {swatch["future_option"]=true.into();swatch["color"]["future_option"]=true.into();}
        }
        assert_eq!(serde_json::to_value(EditingState::restore(&nested.to_string(),&english)).unwrap(),original);
        check(original,|value|serde_json::to_value(EditingState::restore(&value.to_string(),&english)).unwrap());
        check(json!({"version":1,"zen_mode":true}),|value|serde_json::to_value(WorkspaceWorkingState::restore(value)).unwrap());
    }
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
        saved.as_object_mut().unwrap().remove("region_smooth_edges");
        for version in [1,2,999] {
            saved["version"] = version.into();
            saved["region_sources"] = serde_json::json!(["reference","visible"]);
            saved["region_values"]["smoothing"] = 0.into();
            let restored = EditingState::restore(&saved.to_string(), &english);
            assert_eq!(restored.colors,state.colors);
            assert_eq!(restored.version,4);
            if version == 1 {
                assert_eq!(restored.region_sources,[crate::RegionSource::Reference,crate::RegionSource::Visible,crate::RegionSource::Auto]);
                assert!(!restored.region_smooth_edges);
            } else {
                assert_eq!(restored.region_sources,EditingState::default().region_sources);
                assert!(restored.region_smooth_edges);
            }
        }
        saved["version"] = 1.into();
        saved["region_smooth_edges"] = true.into();
        let restored = EditingState::restore(&saved.to_string(), &english);
        assert!(restored.region_smooth_edges);
        assert_eq!(restored.region_values["smoothing"],0.);
    }
    #[test]
    fn released_fill_choice_wins_over_lasso_fill_without_dropping_toolbar_tiles() {
        use crate::{CommandId, ToolSlotId, ToolVariant, ToolbarControl};
        let english = Localizer::shared(UiLanguage::English);
        let fill = ToolVariant::Command { command: CommandId::Fill };
        let lasso = ToolVariant::Command { command: CommandId::LassoFill };
        let mut saved = serde_json::to_value(EditingState::default()).unwrap();
        saved["version"] = 1.into();
        saved["tool_slots"]["choices"] = serde_json::json!({"fill":fill,"lasso_fill":lasso});
        assert_eq!(EditingState::restore(&saved.to_string(), &english).tool_slots.choices[&ToolSlotId::Fill], fill);
        saved["tool_slots"]["choices"].as_object_mut().unwrap().remove("fill");
        assert_eq!(EditingState::restore(&saved.to_string(), &english).tool_slots.choices[&ToolSlotId::Fill], lasso);
        let tiles: Vec<ToolbarControl> = serde_json::from_value(serde_json::json!([
            {"kind":"tool_slot","slot":"fill"}, {"kind":"tool_slot","slot":"lasso_fill"}
        ])).unwrap();
        assert_eq!(tiles, vec![ToolbarControl::ToolSlot { slot: ToolSlotId::Fill }; 2]);
    }
    #[test]
    fn legacy_region_values_convert_without_losing_saved_fields() {
        let english = Localizer::shared(UiLanguage::English);
        let mut state = EditingState::default();
        state.colors.set_rgba([0.2,0.4,0.6,1.]).unwrap();
        state.region_values.insert("tolerance".into(),0.37);
        state.region_values.insert("gap_closing".into(),0.);
        state.region_values.insert("expansion".into(),-2.);
        let mut saved = serde_json::to_value(&state).unwrap();
        saved["version"] = 3.into();
        saved.as_object_mut().unwrap().remove("region_close_gaps");
        saved.as_object_mut().unwrap().remove("region_edges");
        let converted = EditingState::restore(&saved.to_string(), &english);
        state.region_close_gaps = false;
        state.region_edges = layer_core::RegionEdges::GrowShrink(-2);
        assert_eq!(converted,state);
        assert_eq!(EditingState::restore(&serde_json::to_string(&converted).unwrap(),&english),converted);
        saved["region_close_gaps"] = true.into();
        saved["region_edges"] = serde_json::to_value(layer_core::RegionEdges::Exact).unwrap();
        let explicit = EditingState::restore(&saved.to_string(),&english);
        assert!(explicit.region_close_gaps);
        assert_eq!(explicit.region_edges,layer_core::RegionEdges::Exact);
        saved["region_values"].as_object_mut().unwrap().remove("gap_closing");
        saved["region_values"].as_object_mut().unwrap().remove("expansion");
        saved.as_object_mut().unwrap().remove("region_close_gaps");
        saved.as_object_mut().unwrap().remove("region_edges");
        let defaults = EditingState::restore(&saved.to_string(),&english);
        assert!(defaults.region_close_gaps);
        assert_eq!(defaults.region_edges,layer_core::RegionEdges::LineMiddle);
    }
    #[test]
    fn released_region_options_convert_without_rewriting_inputs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../layer-workspace/tests/compat");
        let english = Localizer::shared(UiLanguage::English);
        for name in ["editing.json", "editing-options.json"] {
            let saved = std::fs::read_to_string(root.join("1.0.15").join(name)).unwrap();
            let restored = EditingState::restore(&saved,&english);
            let expected: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("expected/1.0.15").join(format!("{name}.expected.json"))).unwrap()).unwrap();
            assert_eq!(serde_json::to_value(&restored).unwrap(),expected);
            assert!(restored.region_close_gaps);
            assert_eq!(restored.region_edges,layer_core::RegionEdges::GrowShrink(2));
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
