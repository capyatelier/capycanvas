fn blending_submenu(s: &UiSession<Recorder>) -> Vec<(String, bool, Option<bool>)> {
    s.application_menu(ApplicationMenu::Edit)
        .sections
        .into_iter()
        .flatten()
        .find(|item| item.label == "Blending")
        .expect("Edit ▸ Blending")
        .sections
        .into_iter()
        .flatten()
        .map(|item| (item.label, item.enabled, item.selected))
        .collect()
}

#[test]
fn edit_blending_changes_how_layers_combine_in_one_undo_step() {
    let mut s = session(Platform::Gtk);
    let pixels = s.engine.document().layers.clone();
    assert_eq!(blending_submenu(&s), [
        ("Perceptual Blending".into(), true, Some(false)),
        ("Linear Light Blending".into(), true, Some(true)),
    ]);
    invoke(&mut s, CommandId::BlendPerceptual);
    assert_eq!(s.engine.document().blend_space, layer_core::BlendSpace::Perceptual);
    assert_eq!(s.engine.document().layers, pixels, "painted pixels keep their values");
    assert!(s.command(CommandId::BlendPerceptual).selected && !s.command(CommandId::BlendLinear).selected);
    invoke(&mut s, CommandId::BlendPerceptual);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().blend_space, layer_core::BlendSpace::Linear);
    assert!(!s.command(CommandId::Undo).enabled, "choosing the current space adds no step");
    invoke(&mut s, CommandId::SearchCommands);
    search_action(&mut s, CommandSearchAction::Query { text: "blending".into() });
    let found: Vec<_> = s.state.command_search.as_ref().unwrap().results.iter().map(|r| r.id.clone()).collect();
    for id in ["command.blend_perceptual", "command.blend_linear"] {
        assert!(found.iter().any(|f| f == id), "{id} in {found:?}");
    }
}

#[test]
fn float_documents_blend_in_linear_light() {
    let mut document = Document::new("float", 256, 256);
    document.color.depth = layer_core::color::SampleDepth::F16;
    let renderer = Recorder { color: document.color, ..Default::default() };
    let mut s = UiSession::new(renderer, document, [1000, 1000], Platform::Gtk).unwrap();
    assert_eq!(blending_submenu(&s), [
        ("Perceptual Blending".into(), false, Some(false)),
        ("Linear Light Blending".into(), false, Some(true)),
    ]);
    for id in [CommandId::BlendPerceptual, CommandId::BlendLinear] {
        assert_eq!(s.command(id).disabled_reason.as_deref(), Some("Float documents blend in linear light"));
        assert!(s.dispatch(UiAction::Invoke { command: id }).is_err());
    }
    assert_eq!(s.engine.document().blend_space, layer_core::BlendSpace::Linear);
}
