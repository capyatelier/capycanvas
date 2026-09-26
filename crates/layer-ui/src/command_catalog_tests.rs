fn search_action(s: &mut UiSession<Recorder>, action: CommandSearchAction) {
    s.dispatch(UiAction::CommandSearch { action }).unwrap();
}

#[test]
fn command_catalog_covers_live_commands_and_keeps_legacy_bindings() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    let catalog = s.command_catalog();
    let ids: std::collections::BTreeSet<_> = catalog.iter().map(|d| &d.id).collect();
    assert_eq!(ids.len(), catalog.len());
    for command in CommandId::ALL {
        let wire = serde_json::to_value(command).unwrap();
        let id = format!("command.{}", wire.as_str().unwrap());
        if command.available_on(Platform::Gtk) {
            let d = catalog.iter().find(|d| d.id == id).unwrap();
            assert_eq!(d.enabled, s.command(command).enabled, "{command:?}");
            assert_eq!(d.disabled_reason.is_some(), !d.enabled);
        } else {
            assert!(!ids.iter().any(|v| **v == id), "retired {command:?}");
        }
        // Compatibility snapshot of the v1 spelling, now explicit in source.
        assert_eq!(command.shortcut_id(), format!("command.{command:?}"));
    }
    assert_eq!(
        catalog.iter().find(|d| d.id == "canvas.pan").unwrap().kind,
        CommandKind::Held
    );
    assert!(
        s.dispatch(UiAction::ExecuteCommand {
            id: "canvas.pan".into(),
            value: None
        })
        .is_err()
    );
    assert!(
        s.dispatch(UiAction::ExecuteCommand {
            id: "complete_request".into(),
            value: None
        })
        .is_err()
    );
}

#[test]
fn command_search_ranking_disabled_reasons_parameters_and_recents() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    invoke(&mut s, CommandId::SearchCommands);
    search_action(
        &mut s,
        CommandSearchAction::Query {
            text: "undo".into(),
        },
    );
    let undo = &s.state.command_search.as_ref().unwrap().results[0];
    assert_eq!(undo.id, "command.undo");
    assert_eq!(undo.disabled_reason.as_deref(), Some("Nothing to undo"));
    search_action(
        &mut s,
        CommandSearchAction::Execute {
            id: "command.undo".into(),
            value: None,
        },
    );
    assert!(s.state.command_search.as_ref().unwrap().error.is_some());
    search_action(
        &mut s,
        CommandSearchAction::Query {
            text: "fit cnvs".into(),
        },
    );
    assert_eq!(
        s.state.command_search.as_ref().unwrap().results[0].id,
        "command.fit_canvas"
    );
    search_action(
        &mut s,
        CommandSearchAction::Query {
            text: "brush size".into(),
        },
    );
    assert_eq!(
        s.state.command_search.as_ref().unwrap().results[0].id,
        "tool_setting.size"
    );
    search_action(
        &mut s,
        CommandSearchAction::Execute {
            id: "tool_setting.size".into(),
            value: None,
        },
    );
    assert!(s.state.command_search.as_ref().unwrap().parameter.is_some());
    search_action(
        &mut s,
        CommandSearchAction::Query {
            text: "late native text".into(),
        },
    );
    assert!(s.state.command_search.as_ref().unwrap().parameter.is_some());
    let before = s.state.brush.diameter;
    search_action(
        &mut s,
        CommandSearchAction::Execute {
            id: "tool_setting.size".into(),
            value: Some("oops".into()),
        },
    );
    assert_eq!(s.state.brush.diameter, before);
    assert!(s.state.command_search.as_ref().unwrap().error.is_some());
    search_action(&mut s, CommandSearchAction::Back);
    assert_eq!(s.state.command_search.as_ref().unwrap().query, "brush size");
    search_action(
        &mut s,
        CommandSearchAction::Execute {
            id: "tool_setting.size".into(),
            value: Some("12 * 2".into()),
        },
    );
    assert_eq!(s.state.brush.diameter, 24.);
    assert!(s.state.command_search.is_none());
    invoke(&mut s, CommandId::SearchCommands);
    assert!(s.state.command_search.as_ref().unwrap().query.is_empty());
    assert_eq!(
        s.state.command_search.as_ref().unwrap().results[0].id,
        "tool_setting.size"
    );
    search_action(&mut s, CommandSearchAction::Back);
    assert!(s.state.command_search.is_none());
}

#[test]
fn catalog_invocation_rechecks_current_layer_and_preserves_history() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    let clear = s
        .command_catalog()
        .into_iter()
        .find(|d| d.id == "command.clear_layer")
        .unwrap();
    invoke(&mut s, CommandId::SearchCommands);
    search_action(
        &mut s,
        CommandSearchAction::Query {
            text: "clear layer".into(),
        },
    );
    let active = s.engine.document().active_layer;
    s.dispatch(UiAction::Layer {
        action: LayerAction::Lock {
            id: active.0,
            value: true,
        },
    })
    .unwrap();
    assert!(
        s.dispatch(UiAction::ExecuteCommand {
            id: clear.id,
            value: None
        })
        .is_err()
    );
    search_action(&mut s, CommandSearchAction::Close);
    s.dispatch(UiAction::Layer {
        action: LayerAction::Lock {
            id: active.0,
            value: false,
        },
    })
    .unwrap();
    let before = s.engine.document().layers.len();
    s.dispatch(UiAction::ExecuteCommand {
        id: "command.add_layer".into(),
        value: None,
    })
    .unwrap();
    assert_eq!(s.engine.document().layers.len(), before + 1);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().layers.len(), before);
    invoke(&mut s, CommandId::SearchCommands);
    s.state.document_file.epoch += 1;
    assert!(
        s.dispatch(UiAction::CommandSearch {
            action: CommandSearchAction::Execute {
                id: "command.fit_canvas".into(),
                value: None
            }
        })
        .is_err()
    );
    search_action(&mut s, CommandSearchAction::Close);
}

#[test]
fn tool_categories_follow_behavior_and_live_parameter_schema() {
    for (command, category) in [
        (CommandId::Pen, ToolCategory::Drawing),
        (CommandId::Eraser, ToolCategory::Erasing),
        (CommandId::Blend, ToolCategory::Blending),
        (CommandId::Liquify, ToolCategory::Warping),
        (CommandId::RectangleSelect, ToolCategory::Selection),
        (CommandId::Gradient, ToolCategory::FillGradient),
        (CommandId::Figure, ToolCategory::ShapesRulers),
        (CommandId::Move, ToolCategory::MoveTransform),
        (CommandId::Eyedropper, ToolCategory::ColorSampling),
        (CommandId::Hand, ToolCategory::Navigation),
    ] {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, command);
        let context = s.command_tool_context();
        assert_eq!(context.category, category, "{command:?}");
        assert_eq!(
            context.parameters,
            s.state
                .tool_settings
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn command_search_does_not_rebuild_workspace_models() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    invoke(&mut s, CommandId::SearchCommands);
    let revision = s.workspace_model_revision();
    let start = std::time::Instant::now();
    for _ in 0..100 {
        for text in ["brush", "undo", "fit cnvs", "select color", "zzzzzz"] {
            search_action(&mut s, CommandSearchAction::Query { text: text.into() });
        }
    }
    assert_eq!(s.workspace_model_revision(), revision);
    eprintln!("command search mean: {:?}", start.elapsed() / 500);
}

#[test]
fn command_submit_uses_latest_text_and_freezes_palette_history_focus() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    invoke(&mut s, CommandId::SearchCommands);
    search_action(
        &mut s,
        CommandSearchAction::Query {
            text: "eraser".into(),
        },
    );
    search_action(
        &mut s,
        CommandSearchAction::Commit {
            text: "pencil".into(),
        },
    );
    assert_eq!(s.state.brush.tool, Tool::Pencil);
    assert!(s.state.command_search.is_none());
    let palette = s.state.colors.library.active_palette().id;
    for (name, color) in [
        ("Catalog One", layer_core::color::RgbColor::BLACK),
        ("Catalog Two", layer_core::color::RgbColor::WHITE),
    ] {
        s.dispatch(UiAction::Color {
            action: ColorAction::Library {
                action: ColorLibraryAction::Store {
                    palette,
                    name: name.into(),
                    color,
                },
            },
        })
        .unwrap();
    }
    let before: Vec<_> = s
        .state
        .colors
        .library
        .active_palette()
        .swatches
        .iter()
        .map(|s| s.id)
        .collect();
    s.dispatch(UiAction::Color {
        action: ColorAction::Library {
            action: ColorLibraryAction::Reorder {
                palette,
                id: *before.last().unwrap(),
                before: Some(before[0]),
            },
        },
    })
    .unwrap();
    invoke(&mut s, CommandId::AddLayer);
    let layers = s.engine.document().layers.len();
    s.set_command_focus(CommandFocus::Palette);
    invoke(&mut s, CommandId::SearchCommands);
    s.set_command_focus(CommandFocus::Canvas); // The search entry taking focus cannot retarget history.
    search_action(
        &mut s,
        CommandSearchAction::Commit {
            text: "undo".into(),
        },
    );
    assert_eq!(s.engine.document().layers.len(), layers);
    assert_eq!(
        s.state
            .colors
            .library
            .active_palette()
            .swatches
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        before
    );
}

#[test]
fn command_opener_works_from_text_focus_without_stealing_plain_typing() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    assert!(!key(&mut s, "p", true, false, true).handled);
    key(&mut s, "p", false, false, true);
    assert!(key(&mut s, "k", true, true, true).handled);
    assert!(s.state.command_search.is_some());
    let undo = s
        .command_catalog()
        .into_iter()
        .find(|d| d.id == "command.undo")
        .unwrap();
    assert_eq!(undo.history, CommandHistory::Native);
    assert!(!undo.enabled, "text focus must not silently undo artwork");
    // A popup may own the opener's release; closing cannot leave repeat stuck.
    search_action(&mut s, CommandSearchAction::Close);
    assert!(key(&mut s, "k", true, true, true).handled);
    assert!(s.state.command_search.is_some());
}

#[test]
fn active_layer_command_ids_resolve_new_targets_and_toggle_values() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    let first = s.engine.document().active_layer;
    let lock = s
        .command_catalog()
        .into_iter()
        .find(|d| d.id.contains("\"op\":\"lock\""))
        .unwrap()
        .id;
    invoke(&mut s, CommandId::AddLayer);
    let second = s.engine.document().active_layer;
    s.dispatch(UiAction::ExecuteCommand {
        id: lock.clone(),
        value: None,
    })
    .unwrap();
    assert!(!s.engine.document().is_locked(first));
    assert!(s.engine.document().is_locked(second));
    s.dispatch(UiAction::ExecuteCommand {
        id: lock,
        value: None,
    })
    .unwrap();
    assert!(!s.engine.document().is_locked(second));
}
