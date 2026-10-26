#[test]
fn color_mixing_is_a_tool_option_of_mixing_brushes_kept_per_brush() {
    use layer_core::ColorMixSpace;
    let mixing = crate::tool_settings::COLOR_MIXING_COMMANDS;
    let shown = |s: &UiSession<Recorder>| {
        s.state.tool_actions.iter().filter(|a| a.group() == Some(ToolActionGroup::ColorMixing)).map(|a| a.command).collect::<Vec<_>>()
    };
    let mixing_space = |s: &UiSession<Recorder>| s.engine.configured_brush().wet_mix.mix_space;
    let mut s = session(Platform::Gtk);
    s.select_brush(DefaultBrushPreset::GPen as u32).unwrap();
    assert!(shown(&s).is_empty());
    for command in mixing {
        assert!(!s.command(command).enabled);
        assert_eq!(
            s.dispatch(UiAction::Invoke { command }).unwrap_err(),
            "Choose a brush that mixes paint first"
        );
    }
    for preset in [DefaultBrushPreset::Smudge, DefaultBrushPreset::NaturalBlender, DefaultBrushPreset::WetRound, DefaultBrushPreset::WatercolorWash] {
        s.select_brush(preset as u32).unwrap();
        assert_eq!(shown(&s), mixing, "{preset:?}");
        assert!(s.command(CommandId::ColorMixOklab).selected, "{preset:?} mixes in Oklab by default");
    }
    let blender = DefaultBrushPreset::NaturalBlender as u32;
    s.select_brush(blender).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::ColorMixClassic }).unwrap();
    assert_eq!(mixing_space(&s), ColorMixSpace::Classic);
    assert_eq!(mixing.map(|c| s.command(c).selected), [false, false, true]);
    assert!(!s.engine.can_undo(), "brush options are not document edits");
    assert!(s.state.tool_options().iter().any(|o| matches!(o,
        ToolOption::Choice { id: "color-mixing", label, items, .. } if label.as_ref() == "Color mixing" && items.len() == 3 && items[2].selected)));
    s.dispatch(UiAction::Invoke { command: CommandId::Move }).unwrap();
    assert!(shown(&s).is_empty(), "only the brush tool shows brush options");
    s.select_brush(DefaultBrushPreset::WetRound as u32).unwrap();
    assert_eq!(mixing_space(&s), ColorMixSpace::Oklab, "each brush keeps its own choice");

    let saved = serde_json::to_string(&s.editing_state()).unwrap();
    let mut reopened = session(Platform::Gtk);
    reopened.restore_editing(serde_json::from_str(&saved).unwrap()).unwrap();
    reopened.select_brush(blender).unwrap();
    assert_eq!(mixing_space(&reopened), ColorMixSpace::Classic);
    reopened.dispatch(UiAction::Invoke { command: CommandId::ColorMixLinear }).unwrap();
    assert_eq!(mixing_space(&reopened), ColorMixSpace::LinearRgb);
    reopened.dispatch(UiAction::Invoke { command: CommandId::ColorMixOklab }).unwrap();
    assert!(reopened.tools.overrides.get(&blender).is_none_or(|o| !o.contains_key("color_mixing")));

    let mut older: serde_json::Value = serde_json::from_str(&saved).unwrap();
    let overrides = older["tools"]["overrides"][blender.to_string()].as_object_mut().unwrap();
    assert!(overrides.remove("color_mixing").is_some());
    let mut reopened = session(Platform::Gtk);
    reopened.restore_editing(serde_json::from_value(older).unwrap()).unwrap();
    reopened.select_brush(blender).unwrap();
    assert_eq!(mixing_space(&reopened), ColorMixSpace::Oklab);
}

#[test]
fn dither_hides_color_mixing_without_discarding_saved_choice() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        let preset = DefaultBrushPreset::DitherBlender as u32;
        let mut editing = s.editing_state();
        editing.tools.overrides.entry(preset).or_default().insert("color_mixing".into(), layer_core::ColorMixSpace::Classic as u8 as f32);
        s.restore_editing(editing).unwrap();
        s.select_brush(preset).unwrap();
        assert!(!s.state.tool_actions.iter().any(|a| a.group() == Some(ToolActionGroup::ColorMixing)));
        assert_eq!(s.engine.configured_brush().wet_mix.mix_space, layer_core::ColorMixSpace::Classic);
        assert!(s.editing_state().tools.overrides[&preset].contains_key("color_mixing"));
    }
}
