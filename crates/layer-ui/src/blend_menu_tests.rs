fn blend_items(menu: &[Vec<ContextMenuItem>]) -> Vec<Vec<(String, u32, bool)>> {
    menu.iter()
        .map(|section| {
            section
                .iter()
                .map(|item| match &item.action {
                    Some(UiAction::Layer { action: LayerAction::Blend { value, .. } }) => {
                        (item.label.clone(), *value, item.selected == Some(true))
                    }
                    other => panic!("{} is not a blend choice: {other:?}", item.label),
                })
                .collect()
        })
        .collect()
}

fn layer_blend_submenu(s: &UiSession<Recorder>) -> Vec<Vec<ContextMenuItem>> {
    s.application_menu(ApplicationMenu::Layer)
        .sections
        .into_iter()
        .flatten()
        .find(|item| item.label == "Blend Mode")
        .expect("Layer › Blend Mode")
        .sections
}

#[test]
fn blend_menu_groups_every_mode_by_code_and_sets_it_in_one_step() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::AddLayer);
    let id = s.engine.document().active_layer.0;
    let menu = s.layer_blend_menu(id).unwrap();
    assert_eq!(menu.title, "Blend Mode");
    assert_eq!(blend_items(&layer_blend_submenu(&s)), blend_items(&menu.sections));
    let expected: Vec<Vec<_>> = layer_core::LayerBlend::MENU
        .iter()
        .map(|group| {
            group
                .iter()
                .filter(|b| **b != layer_core::LayerBlend::PassThrough)
                .map(|b| (b.label().to_string(), b.code(), *b == layer_core::LayerBlend::Normal))
                .collect()
        })
        .collect();
    assert_eq!(blend_items(&menu.sections), expected);
    assert_eq!(
        ui_catalog().layer_blends,
        layer_core::LayerBlend::ALL.map(|b| b.label()).to_vec(),
        "Apple and Windows index the flat list by code"
    );
    for item in menu.sections.iter().flatten() {
        s.dispatch(item.action.clone().unwrap()).unwrap();
        let blend = s.engine.document().layer(LayerId(id)).unwrap().properties.blend;
        assert_eq!(blend.label(), item.label);
        assert_eq!(s.state.layer_tools.editing_layer.as_ref().unwrap().blend, blend.code());
        let checked: Vec<_> = blend_items(&s.layer_blend_menu(id).unwrap().sections)
            .into_iter()
            .flatten()
            .filter(|(_, _, checked)| *checked)
            .map(|(label, ..)| label)
            .collect();
        assert_eq!(checked, std::slice::from_ref(&item.label));
    }
    s.dispatch(UiAction::Layer { action: LayerAction::Blend { id, value: layer_core::LayerBlend::Luminosity.code() } })
        .unwrap();
    assert!(s.dispatch(UiAction::Layer { action: LayerAction::Blend { id, value: 25 } }).is_err());
    let blend = |s: &UiSession<Recorder>| s.engine.document().layer(LayerId(id)).unwrap().properties.blend;
    invoke(&mut s, CommandId::Undo);
    assert_eq!(blend(&s), layer_core::LayerBlend::Color, "each choice is one step; the current mode adds none");
    invoke(&mut s, CommandId::Redo);
    assert_eq!(blend(&s), layer_core::LayerBlend::Luminosity);
}

#[test]
fn float_documents_offer_only_modes_defined_above_one() {
    use layer_core::color::SampleDepth;
    let mut document = Document::new("HDR", 32, 32);
    document.color.depth = SampleDepth::F32;
    let renderer = Recorder { color: document.color, ..Default::default() };
    let mut s = UiSession::new(renderer, document, [32, 32], Platform::Gtk).unwrap();
    invoke(&mut s, CommandId::AddLayer);
    let id = s.engine.document().active_layer.0;
    let labels = |s: &UiSession<Recorder>| -> Vec<String> {
        blend_items(&s.layer_blend_menu(id).unwrap().sections)
            .into_iter()
            .flatten()
            .map(|(label, ..)| label)
            .collect()
    };
    let offered = labels(&s);
    for blend in layer_core::LayerBlend::ALL {
        let expected = blend.range() == layer_core::BlendRange::Unbounded && blend != layer_core::LayerBlend::PassThrough;
        assert_eq!(offered.contains(&blend.label().to_string()), expected, "{blend:?}");
    }
    assert!(s.command_catalog().iter().all(|d| d.category != "Layer › Blend Mode" || offered.contains(&d.label)));
    let mut layer = s.engine.document().layer(LayerId(id)).unwrap().clone();
    layer.properties.blend = layer_core::LayerBlend::Overlay;
    s.layer_edit(layer_core::Edit::ReplaceLayer(Box::new(layer))).unwrap();
    let current = labels(&s);
    assert!(current.contains(&"Overlay".to_string()), "a layer's current mode stays visible");
    assert!(!current.contains(&"Soft Light".to_string()));
}

#[test]
fn locked_layers_show_their_blend_without_offering_changes() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::AddLayer);
    let id = s.engine.document().active_layer.0;
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id, value: true } }).unwrap();
    let menu = s.layer_blend_menu(id).unwrap();
    assert!(menu.sections.iter().flatten().all(|item| !item.enabled));
    assert!(s.layer_blend_menu(9999).is_err());
}

fn blend_of(s: &UiSession<Recorder>, id: LayerId) -> layer_core::LayerBlend {
    s.engine.document().layer(id).unwrap().properties.blend
}

#[test]
fn only_groups_offer_pass_through_and_choosing_it_is_one_step() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::AddLayer);
    let paint = s.engine.document().active_layer;
    let labels = |s: &UiSession<Recorder>, id: LayerId| -> Vec<String> {
        blend_items(&s.layer_blend_menu(id.0).unwrap().sections).into_iter().flatten().map(|(label, ..)| label).collect()
    };
    assert!(!labels(&s, paint).contains(&"Pass Through".to_string()));
    let refused = s
        .dispatch(UiAction::Layer { action: LayerAction::Blend { id: paint.0, value: layer_core::LayerBlend::PassThrough.code() } })
        .unwrap_err();
    assert_eq!(refused, "Only groups can use Pass Through");
    let choices = |s: &UiSession<Recorder>| match &s.state.layer_properties.controls.iter().find(|c| c.key == "blend").unwrap().kind {
        PropertyKind::Choice { options } => options.iter().map(|o| o.to_string()).collect::<Vec<_>>(),
        other => panic!("{other:?}"),
    };
    assert!(!choices(&s).contains(&"Pass Through".to_string()));

    s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
    let group = s.engine.document().active_layer;
    assert_eq!(blend_of(&s, group), layer_core::LayerBlend::Normal, "new groups are isolated by default");
    let menu = s.layer_blend_menu(group.0).unwrap();
    assert_eq!(
        blend_items(&menu.sections)[0],
        [("Pass Through".to_string(), layer_core::LayerBlend::PassThrough.code(), false), ("Normal".to_string(), 0, true)],
        "Pass Through leads the group's first section, as in Photoshop"
    );
    assert_eq!(choices(&s).last().map(String::as_str), Some("Pass Through"));
    s.dispatch(menu.sections[0][0].action.clone().unwrap()).unwrap();
    assert_eq!(blend_of(&s, group), layer_core::LayerBlend::PassThrough);
    assert_eq!(s.state.layer_tools.editing_layer.as_ref().unwrap().blend_label, "Pass Through");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(blend_of(&s, group), layer_core::LayerBlend::Normal, "choosing Pass Through is one undo step");
    invoke(&mut s, CommandId::Redo);
    assert_eq!(blend_of(&s, group), layer_core::LayerBlend::PassThrough);
}

#[test]
fn the_pass_through_setting_picks_the_blend_of_every_new_group() {
    let saved = serde_json::to_value(Settings::default()).unwrap();
    let mut older = saved.as_object().unwrap().clone();
    older.remove("pass_through_groups");
    assert!(!Settings::restore(&serde_json::Value::Object(older).to_string()).pass_through_groups, "older settings read with it off");
    for on in [false, true] {
        let mut s = session(Platform::Gtk);
        s.dispatch(UiAction::Preferences {
            action: PreferenceAction::Edit { id: PreferenceId::PassThroughGroups, value: PreferenceValue::Bool(on) },
        })
        .unwrap();
        assert_eq!(s.state.settings.pass_through_groups, on);
        let expected = if on { layer_core::LayerBlend::PassThrough } else { layer_core::LayerBlend::Normal };
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
        assert_eq!(blend_of(&s, s.engine.document().active_layer), expected, "New Group, setting {on}");
        s.dispatch(UiAction::Layer { action: LayerAction::Select { id: s.engine.document().active_layer.0, mask: false } }).unwrap();
        invoke(&mut s, CommandId::AddLayer);
        let first = s.engine.document().active_layer;
        invoke(&mut s, CommandId::AddLayer);
        let second = s.engine.document().active_layer;
        s.dispatch(UiAction::Layer { action: LayerAction::Select { id: first.0, mask: false } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::ToggleSelection { id: second.0 } }).unwrap();
        let before = s.engine.document().layers.len();
        s.dispatch(UiAction::Layer { action: LayerAction::GroupSelected }).unwrap();
        assert_eq!(s.engine.document().layers.len(), before + 1);
        let grouped = s.engine.document().layer(first).unwrap().properties.parent.unwrap();
        assert_eq!(blend_of(&s, grouped), expected, "Group Selected, setting {on}");
    }
}
