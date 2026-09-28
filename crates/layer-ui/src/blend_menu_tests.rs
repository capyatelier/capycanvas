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
        .map(|group| group.iter().map(|b| (b.label().to_string(), b.code(), *b == layer_core::LayerBlend::Normal)).collect())
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
    assert!(s.dispatch(UiAction::Layer { action: LayerAction::Blend { id, value: 24 } }).is_err());
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
        assert_eq!(offered.contains(&blend.label().to_string()), blend.range() == layer_core::BlendRange::Unbounded, "{blend:?}");
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
