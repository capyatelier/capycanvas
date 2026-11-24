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

#[test]
fn taper_section_follows_tip_with_its_choice_then_the_current_mode_sliders() {
    let section = |s: &UiSession<Recorder>| -> Option<Vec<&'static str>> {
        let options = s.state.tool_options();
        let at = options.iter().position(|o| matches!(o, ToolOption::Choice { id: "taper", .. }))?;
        let ToolOption::Choice { label, labeled: true, segmented: true, items, heading, .. } = &options[at] else { panic!("an inline labeled bar") };
        assert_eq!((label.as_ref(), items.len()), ("Taper type", 3));
        let field = |o: &ToolOption| match o { ToolOption::Numeric(f) => Some(f.clone()), _ => None };
        assert_eq!(field(&options[at - 1]).unwrap().group_id, MessageId::TOOL_SETTING_GROUP_TIP.key(), "the section follows Tip");
        assert_eq!(options[at].anchor(), options.get(at + 1).and_then(field).map(|f| f.id), "hosts place the choice by its anchor");
        let sliders: Vec<_> = options[at + 1..].iter().map_while(field)
            .take_while(|f| f.group_id == MessageId::TOOL_SETTING_GROUP_TAPER.key()).map(|f| f.id).collect();
        assert_eq!(sliders, s.state.tool_settings.iter().filter(|f| f.id.starts_with("taper_")).map(|f| f.id).collect::<Vec<_>>());
        let titles = heading.iter().map(|h| h.to_string()).chain(options.get(at + 1).and_then(field)
            .filter(|f| f.group_id == MessageId::TOOL_SETTING_GROUP_TAPER.key()).map(|f| f.group.to_string())).collect::<Vec<_>>();
        assert_eq!(titles, ["Taper"], "one Taper heading, from the choice only when the section has no sliders");
        Some(sliders)
    };
    let mut s = session(Platform::Gtk);
    s.select_brush(DefaultBrushPreset::GPen as u32).unwrap();
    assert!(s.command(CommandId::TaperOff).selected);
    assert!(s.state.tool_actions.iter().all(|a| tool_settings::taper(a.command).is_none()), "the choice is not a mode bar");
    assert_eq!(section(&s).unwrap(), [""; 0]);
    s.dispatch(UiAction::Invoke { command: CommandId::TaperFixed }).unwrap();
    assert_eq!(section(&s).unwrap(), ["taper_start", "taper_end", "taper_pointiness"]);
    s.dispatch(UiAction::SetToolSetting { id: "taper_start_widths".into(), value: 4.5 }).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::TaperProportional }).unwrap();
    assert_eq!(section(&s).unwrap(), ["taper_start", "taper_end", "taper_pointiness"]);
    s.dispatch(UiAction::SetToolSetting { id: "taper_end_share".into(), value: 0.45 }).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::TaperOff }).unwrap();
    assert_eq!(section(&s).unwrap(), [""; 0]);
    assert!(s.dispatch(UiAction::SetToolSetting { id: "taper_start_widths".into(), value: 1. }).is_err());
    s.dispatch(UiAction::Invoke { command: CommandId::TaperFixed }).unwrap();
    let taper = s.engine.configured_brush().taper;
    assert_eq!((taper.mode, taper.fixed_widths[0], taper.proportional_shares[1]), (layer_core::TaperMode::Fixed, 4.5, 0.45));
    assert!(!s.engine.can_undo(), "brush options are not document edits");

    s.select_brush(DefaultBrushPreset::Paintbrush as u32).unwrap();
    assert!(layer_engine::can_taper(s.engine.configured_brush()));
    assert!(section(&s).is_none() && !s.state.tool_settings.iter().any(|f| f.group_id == MessageId::TOOL_SETTING_GROUP_TAPER.key()));
    for command in crate::tool_settings::TAPER_COMMANDS {
        assert!(!s.command(command).enabled);
        assert_eq!(s.dispatch(UiAction::Invoke { command }).unwrap_err(), "Choose a pen first");
    }
}

#[test]
fn stored_taper_overrides_restore_in_either_order() {
    use crate::tool_settings::edit_value;
    let pen = DefaultBrushPreset::GPen;
    let values = [("taper_start_widths", 6.), ("taper_end_share", 0.2), ("taper_pointiness", 0.8)];
    let mode = ("taper", layer_core::TaperMode::Proportional as u8 as f32);
    let apply = |order: &[(&str, f32)]| order.iter()
        .try_fold(layer_core::default_brush(pen), |brush, &(id, value)| edit_value(pen as u32, &brush, id, value)).unwrap().taper;
    let expected = apply(&[mode, values[0], values[1], values[2]]);
    assert_eq!(apply(&[values[0], values[1], values[2], mode]), expected);
    assert_eq!(expected, layer_core::BrushTaper {
        mode: layer_core::TaperMode::Proportional, fixed_widths: [6., 10.], proportional_shares: [0.3, 0.2], pointiness: 0.8,
    });
    for (id, value) in [("taper", 3.), ("taper_start_widths", 200.5), ("taper_end_share", 0.6), ("taper_pointiness", -0.1)] {
        assert!(edit_value(pen as u32, &layer_core::default_brush(pen), id, value).is_err(), "{id} {value}");
    }

    let mut s = session(Platform::Gtk);
    let mut editing = s.editing_state();
    editing.tools.overrides.insert(pen as u32, values.into_iter().chain([mode]).map(|(id, value)| (id.into(), value)).collect());
    s.restore_editing(editing).unwrap();
    s.select_brush(pen as u32).unwrap();
    assert_eq!(s.engine.configured_brush().taper, expected);
    assert_eq!(s.state.tool_settings.iter().filter(|c| c.id.starts_with("taper_")).map(|c| (c.id, c.value)).collect::<Vec<_>>(),
        [("taper_start", 0.3), ("taper_end", 0.2), ("taper_pointiness", 0.8)]);
}

#[test]
fn only_pens_take_taper_values_from_settings_or_saved_overrides() {
    use layer_core::TaperMode;
    let brush = DefaultBrushPreset::Paintbrush;
    let mut s = session(Platform::Gtk);
    s.select_brush(brush as u32).unwrap();
    assert!(layer_engine::can_taper(s.engine.configured_brush()));
    for (id, value) in [("taper", TaperMode::Fixed as u8 as f32), ("taper_end_widths", 4.), ("taper_pointiness", 0.2)] {
        assert!(s.dispatch(UiAction::SetToolSetting { id: id.into(), value }).is_err(), "{id}");
    }
    assert_eq!(s.engine.configured_brush().taper, layer_core::BrushTaper::default());

    let saved = [("taper", TaperMode::Fixed as u8 as f32), ("taper_end_widths", 4.), ("flow", 0.5)];
    let mut editing = s.editing_state();
    editing.tools.overrides.insert(brush as u32, saved.into_iter().map(|(id, value)| (id.into(), value)).collect());
    let text = serde_json::to_string(&editing).unwrap();
    s.restore_editing(editing).unwrap();
    s.select_brush(brush as u32).unwrap();
    let configured = s.engine.configured_brush();
    assert_eq!((configured.taper, configured.flow), (layer_core::BrushTaper::default(), 0.5));
    assert_eq!(s.editing_state().tools.overrides[&(brush as u32)].len(), 3, "reading keeps the values it ignores");
    let restored = crate::EditingState::restore(&text, &crate::Localizer::shared(crate::UiLanguage::English));
    assert_eq!(restored.tools.overrides[&(brush as u32)].len(), 3);
}

#[test]
fn taper_rows_keep_their_identity_when_the_taper_type_changes() {
    let taper = MessageId::TOOL_SETTING_GROUP_TAPER.key();
    let rows = |s: &UiSession<Recorder>| -> Vec<_> {
        s.state.tool_settings.iter().filter(|f| f.group_id == taper).map(|f| (f.id, f.label.clone(), f.numeric.clone(), f.value)).collect()
    };
    let identities = |rows: &[(&'static str, std::sync::Arc<str>, crate::NumericControl, f32)]| -> Vec<_> { rows.iter().map(|row| (row.0, row.1.clone())).collect() };
    let anchor = |s: &UiSession<Recorder>| s.state.tool_options().iter().find_map(|o| match o { ToolOption::Choice { id: "taper", before, .. } => Some(*before), _ => None });
    let pen = DefaultBrushPreset::GPen as u32;
    let mut s = session(Platform::Gtk);
    s.select_brush(pen).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::TaperFixed }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "taper_start".into(), value: 4.5 }).unwrap();
    let (fixed, fixed_anchor) = (rows(&s), anchor(&s));
    s.dispatch(UiAction::Invoke { command: CommandId::TaperProportional }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "taper_end".into(), value: 0.45 }).unwrap();
    let proportional = rows(&s);
    assert_eq!(identities(&fixed), identities(&proportional), "Fixed and Proportional show the same rows");
    assert_eq!(anchor(&s), fixed_anchor, "the Taper type choice keeps its place");
    assert_ne!(fixed[0].2, proportional[0].2, "the unit and range follow the type");
    assert_eq!((fixed[0].3, proportional[1].3), (4.5, 0.45));
    let overrides = &s.tools.overrides[&pen];
    assert_eq!((overrides.get("taper_start_widths"), overrides.get("taper_end_share")), (Some(&4.5), Some(&0.45)), "values keep their saved IDs");
    s.dispatch(UiAction::ResetToolSetting { id: "taper_end".into() }).unwrap();
    assert_eq!(s.engine.configured_brush().taper.proportional_shares[1], layer_core::BrushTaper::default().proportional_shares[1]);
    s.dispatch(UiAction::Invoke { command: CommandId::TaperFixed }).unwrap();
    assert_eq!(identities(&rows(&s)), identities(&fixed));
    assert_eq!(rows(&s)[0].3, 4.5);
    s.dispatch(UiAction::Invoke { command: CommandId::TaperOff }).unwrap();
    assert!(rows(&s).is_empty(), "Off hides the rows");
}

#[test]
fn fixed_taper_sliders_follow_the_smoothness_curve_to_fifty_widths() {
    use crate::{NumericMapping, NumericOperation};
    let mut s = session(Platform::Gtk);
    s.select_brush(DefaultBrushPreset::GPen as u32).unwrap();
    let numeric = |s: &UiSession<Recorder>, id: &str| s.state.tool_settings.iter().find(|c| c.id == id).unwrap().numeric.clone();
    let at = |control: &crate::NumericControl, position: f64| control.resolve(0., NumericOperation::Position { position }).unwrap().value;
    let smoothness = numeric(&s, "smoothing_strength");
    let curve = |position: f64| smoothness.log_offset * ((1. + smoothness.log_offset) / smoothness.log_offset).powf(position) - smoothness.log_offset;
    s.dispatch(UiAction::Invoke { command: CommandId::TaperFixed }).unwrap();
    for id in ["taper_start", "taper_end"] {
        let widths = numeric(&s, id);
        assert_eq!((widths.mapping, widths.soft_max, widths.max), (NumericMapping::Log, 50., 200.), "{id}");
        for position in [0., 0.1, 0.25, 0.5, 0.75, 0.9, 1.] {
            assert!((at(&smoothness, position) - curve(position)).abs() <= 0.005, "Smoothness at {position}");
            assert!((at(&widths, position) - 50. * curve(position)).abs() <= 0.05, "{id} at {position}: {}", at(&widths, position));
        }
        assert_eq!(widths.resolve(0., NumericOperation::Expression { text: "120".into() }).unwrap().value, 120.);
    }
    s.dispatch(UiAction::SetToolSetting { id: "taper_start".into(), value: 120. }).unwrap();
    assert_eq!(s.engine.configured_brush().taper.fixed_widths[0], 120.);
    s.dispatch(UiAction::Invoke { command: CommandId::TaperProportional }).unwrap();
    for id in ["taper_start", "taper_end"] {
        let share = numeric(&s, id);
        assert_eq!(share.mapping, NumericMapping::Linear, "{id}");
        assert!((at(&share, 0.5) - 0.25).abs() <= 0.001, "{id} stays linear");
    }
}
