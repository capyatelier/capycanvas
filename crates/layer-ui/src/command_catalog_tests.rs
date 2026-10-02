fn search_action(s: &mut UiSession<Recorder>, action: CommandSearchAction) {
    s.dispatch(UiAction::CommandSearch { action }).unwrap();
}

#[test]
fn command_catalog_covers_live_commands_and_keeps_legacy_bindings() {
    let mut s = session(Platform::Gtk);
    let catalog = s.command_catalog();
    let ids: std::collections::BTreeSet<_> = catalog.iter().map(|d| &d.id).collect();
    assert_eq!(ids.len(), catalog.len());
    for command in CommandId::ALL {
        let wire = serde_json::to_value(command).unwrap();
        let id = format!("command.{}", wire.as_str().unwrap());
        if s.proof_panel_command(command) {
            assert!(!ids.iter().any(|v| **v == id), "the Proof panel owns {command:?}");
        } else if command.available_on(Platform::Gtk) {
            let d = catalog.iter().find(|d| d.id == id).unwrap();
            assert_eq!(d.enabled, s.command(command).enabled, "{command:?}");
            assert_eq!(d.disabled_reason.is_some(), !d.enabled);
        } else {
            assert!(!ids.iter().any(|v| **v == id), "retired {command:?}");
        }
    }
    assert!(s.execute_catalog_command("canvas.pan", None).is_err());
    assert!(s.execute_catalog_command("complete_request", None).is_err());
}

#[test]
fn command_search_ranking_disabled_reasons_parameters_and_recents() {
    let mut s = session(Platform::Gtk);
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
    assert_eq!(s.state.command_search.as_ref().unwrap().detail, "Nothing to undo");
    search_action(
        &mut s,
        CommandSearchAction::Execute {
            id: "command.undo".into(),
            value: None,
        },
    );
    let view = s.state.command_search.as_ref().unwrap();
    assert_eq!(Some(&view.detail), view.error.as_ref());
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
    let view = s.state.command_search.as_ref().unwrap();
    assert!(view.results[0].description.ends_with("Range 0.5–2048 px"));
    assert_eq!(view.detail, view.results[0].description);
    search_action(
        &mut s,
        CommandSearchAction::Execute {
            id: "tool_setting.size".into(),
            value: None,
        },
    );
    let view = s.state.command_search.as_ref().unwrap();
    assert_eq!(view.detail, view.parameter.as_ref().unwrap().description);
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
    let view = s.state.command_search.as_ref().unwrap();
    assert!(view.error.is_some());
    assert_eq!(Some(&view.detail), view.error.as_ref());
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
    assert!(
        s.state.command_search.as_ref().unwrap().results[0]
            .description
            .contains("Current 24 px")
    );
    search_action(&mut s, CommandSearchAction::Back);
    assert!(s.state.command_search.is_none());
}

#[test]
fn catalog_invocation_rechecks_current_layer_and_preserves_history() {
    let mut s = session(Platform::Gtk);
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
    layer(&mut s, LayerAction::Lock { id: active.0, value: true, });
    assert!(s.execute_catalog_command(&clear.id, None).is_err());
    search_action(&mut s, CommandSearchAction::Close);
    layer(&mut s, LayerAction::Lock { id: active.0, value: false, });
    let before = s.engine.document().layers.len();
    s.execute_catalog_command("command.add_layer", None)
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
    let revision = s.state.revision;
    for action in [
        CommandSearchAction::Query { text: "late".into() },
        CommandSearchAction::Execute { id: "command.fit_canvas".into(), value: None },
    ] {
        assert_eq!(s.dispatch(UiAction::CommandSearch { action }).unwrap().regions, 0, "late actions after closing are ignored");
    }
    assert!(s.state.command_search.is_none());
    assert_eq!(s.state.revision, revision);
}

#[test]
fn command_search_does_not_rebuild_workspace_models() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::SearchCommands);
    let revision = s.workspace_model_revision();
    for text in ["brush", "undo", "fit cnvs", "select color", "zzzzzz"] {
        search_action(&mut s, CommandSearchAction::Query { text: text.into() });
        assert_eq!(s.workspace_model_revision(), revision);
    }
}

#[test]
fn command_submit_uses_latest_text_and_freezes_palette_history_focus() {
    let mut s = session(Platform::Gtk);
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
    let mut s = session(Platform::Gtk);
    assert!(!key(&mut s, "p", true, false, true).handled);
    key(&mut s, "p", false, false, true);
    assert!(key(&mut s, "k", true, true, true).handled);
    assert!(s.state.command_search.is_some());
    let undo = s
        .command_catalog()
        .into_iter()
        .find(|d| d.id == "command.undo")
        .unwrap();
    assert!(!undo.enabled, "text focus must not silently undo artwork");
    // A popup may own the opener's release; closing cannot leave repeat stuck.
    search_action(&mut s, CommandSearchAction::Close);
    assert!(key(&mut s, "k", true, true, true).handled);
    assert!(s.state.command_search.is_some());
}

#[test]
fn active_layer_command_ids_resolve_new_targets_and_toggle_values() {
    let mut s = session(Platform::Gtk);
    let first = s.engine.document().active_layer;
    let lock = s
        .command_catalog()
        .into_iter()
        .find(|d| d.id.contains("\"op\":\"lock\""))
        .unwrap()
        .id;
    invoke(&mut s, CommandId::AddLayer);
    let second = s.engine.document().active_layer;
    s.execute_catalog_command(&lock, None).unwrap();
    assert!(!s.engine.document().is_locked(first));
    assert!(s.engine.document().is_locked(second));
    s.execute_catalog_command(&lock, None).unwrap();
    assert!(!s.engine.document().is_locked(second));
}

#[test]
fn command_search_top_is_a_fifth_of_the_workspace_within_bounds() {
    let style = COMMAND_SEARCH_STYLE;
    assert_eq!(style.top(100.), 48.);
    assert_eq!(style.top(600.), 120.);
    assert_eq!(style.top(2160.), 192.);
}

#[test]
fn catalog_reaches_tool_variants_layer_properties_workspaces_and_paint_slots() {
    let mut s = session(Platform::Gtk);
    let find = |s: &UiSession<Recorder>, label: &str| {
        s.command_catalog()
            .into_iter()
            .find(|d| d.label == label)
            .unwrap_or_else(|| panic!("{label} is cataloged"))
    };
    let execute = |s: &mut UiSession<Recorder>, id: &str, value: Option<&str>| {
        s.execute_catalog_command(id, value.map(Into::into)).unwrap();
    };
    let radial = find(&s, "Ruler › Radial");
    assert!(!radial.selected);
    execute(&mut s, &radial.id, None);
    assert_eq!(
        s.layer_interaction.tool,
        LayerCanvasTool::Ruler { kind: RulerKind::Radial }
    );
    assert!(find(&s, "Ruler › Radial").selected);
    assert!(!find(&s, "Ruler › Straight").selected);

    let id = find(&s, "Watercolor brushes").id;
    execute(&mut s, &id, None);
    assert_eq!(s.layer_interaction.tool, LayerCanvasTool::Paint);
    assert_eq!(crate::tools::group(s.state.brush.preset), ToolGroup::Watercolor);

    assert!(s.command_catalog().iter().all(|d| !d.label.starts_with("Sample size ›")));
    invoke(&mut s, CommandId::Eyedropper);
    let id = find(&s, "Sample size › 5 px circle").id;
    execute(&mut s, &id, None);
    assert_eq!(s.state.color_picker.sample_width, 5);

    invoke(&mut s, CommandId::AddLayer);
    let layer = s.engine.document().active_layer;
    let opacity = find(&s, "Layer opacity…");
    assert_eq!(opacity.id, "layer_property.opacity");
    assert!(opacity.description.ends_with("Current 100 % · Range 0–100 %"), "{}", opacity.description);
    execute(&mut s, &opacity.id, Some("40"));
    let properties = |s: &UiSession<Recorder>| s.engine.document().layer(layer).unwrap().properties.clone();
    assert!((s.engine.document().layer(layer).unwrap().opacity - 0.4).abs() < 1e-4);
    let blend = |s: &UiSession<Recorder>, label: &str| {
        s.command_catalog()
            .into_iter()
            .find(|d| d.label == label && d.category == "Layer › Blend Mode")
            .unwrap_or_else(|| panic!("{label} is cataloged"))
    };
    assert!(s.command_catalog().iter().all(|d| !d.label.starts_with("Layer blend mode")));
    let id = blend(&s, "Multiply").id;
    assert!(id.contains(r#""op":"blend""#) && !id.contains(r#""id""#), "{id}");
    execute(&mut s, &id, None);
    assert_eq!(properties(&s).blend, layer_core::LayerBlend::Multiply);
    assert!(blend(&s, "Multiply").selected);
    assert!(!blend(&s, "Normal").selected);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(properties(&s).blend, layer_core::LayerBlend::Normal);
    assert!((s.engine.document().layer(layer).unwrap().opacity - 0.4).abs() < 1e-4);

    let id = find(&s, "Background color").id;
    execute(&mut s, &id, None);
    assert_eq!(s.state.colors.slot, ColorSlot::Background);

    s.configure_workspace_manager(ManagedWorkspace {
        id: "w0".into(),
        name: "Workspace 0".into(),
        baseline: durable_layout(&s.state.workspace.layout),
        choices: (0..8)
            .map(|i| WorkspaceChoice { id: format!("w{i}"), name: format!("Workspace {i}") })
            .collect(),
    })
    .unwrap();
    let catalog = s.command_catalog();
    for i in 0..8 {
        let choice = catalog.iter().find(|d| d.label == format!("Workspace {i}")).unwrap();
        assert_eq!(choice.selected, i == 0);
    }
    assert_eq!(catalog.iter().filter(|d| d.label == "Restore Starting Layout…").count(), 1);
}

#[test]
fn equivalent_menu_actions_share_command_identities_and_explain_unavailability() {
    let mut s = session(Platform::Gtk);
    let catalog = s.command_catalog();
    let active = s.engine.document().active_layer.0;
    for action in [
        LayerAction::Clear { id: active },
        LayerAction::Delete { id: active },
    ] {
        let id = command_catalog::identity(&UiAction::Layer { action });
        assert!(!catalog.iter().any(|d| d.id == id), "{id} is the command entry");
    }
    let labels: Vec<_> = catalog.iter().map(|d| d.label.to_lowercase()).collect();
    assert!(!labels.iter().any(|l| l.ends_with("panel panel") || l.ends_with("toolbar panel")));
    let unique: std::collections::BTreeSet<_> = labels.iter().collect();
    let repeated: Vec<_> = labels.iter().filter(|l| labels.iter().filter(|m| m == l).count() > 1).collect();
    assert_eq!(unique.len(), labels.len(), "every search result name is distinct: {repeated:?}");
    for label in ["Pencil", "Pencil brush", "Pencil filter", "Eraser", "Eraser brush"] {
        assert!(catalog.iter().any(|d| d.label == label), "{label}");
    }
    let reason = |s: &UiSession<Recorder>, id: &str| {
        s.command_catalog()
            .into_iter()
            .find(|d| d.id == id)
            .and_then(|d| d.disabled_reason)
            .unwrap_or_default()
    };
    assert_eq!(reason(&s, "command.undo_workspace"), "No workspace change to undo");
    assert_eq!(reason(&s, "command.return_to_artwork"), "Edit a selection mask first");
    assert_eq!(
        reason(&s, &command_catalog::identity(&UiAction::Layer { action: LayerAction::PasteMask { id: active } })),
        "Copy a layer mask first"
    );
    while s.command(CommandId::ZoomIn).enabled {
        invoke(&mut s, CommandId::ZoomIn);
    }
    assert_eq!(reason(&s, "command.zoom_in"), "Already at the maximum zoom");
    layer(&mut s, LayerAction::Lock { id: active, value: true });
    assert_eq!(reason(&s, "command.clear_layer"), "The active layer is locked");
    let generic = "Unavailable in the current tool or edit target";
    let unexplained: Vec<_> = s
        .command_catalog()
        .into_iter()
        .filter(|d| !d.enabled && d.disabled_reason.as_deref().is_none_or(str::is_empty))
        .map(|d| d.id)
        .collect();
    assert!(unexplained.is_empty(), "{unexplained:?}");
    assert!(s.command_catalog().iter().filter(|d| d.disabled_reason.as_deref() == Some(generic)).count() < 3);
}

#[test]
fn refresh_commands_preserves_unchanged_availability_in_each_edit_state() {
    let mut s = filled_selection_session();
    let unchanged = |s: &mut UiSession<Recorder>| {
        s.refresh_commands();
        assert!(!s.refresh_commands());
    };
    unchanged(&mut s);
    invoke(&mut s, CommandId::ScaleRotate);
    unchanged(&mut s);
    invoke(&mut s, CommandId::CancelTransform);
    s.pen(event(&s, 1, PenPhase::Down, 0.5)).unwrap();
    s.frame(2, 2).unwrap();
    assert!(s.require_idle().is_err());
    unchanged(&mut s);
}

fn assert_published_reasons(s: &mut UiSession<Recorder>, state: &str) {
    s.refresh_commands();
    for (published, id) in s.state.commands.iter().zip(CommandId::ALL) {
        let live = s.command_disabled_reason(id);
        assert_eq!(published.disabled_reason.as_deref(), live.as_deref(), "{state}: {id:?}");
        assert_eq!(published.enabled, live.is_none(), "{state}: {id:?}");
        assert_eq!(s.command(id).disabled_reason, published.disabled_reason, "{state}: {id:?}");
    }
}

#[test]
fn published_disabled_reasons_match_command_disabled_reason() {
    let mut s = session(Platform::Android);
    assert_published_reasons(&mut s, "android");
    let mut s = filled_selection_session();
    assert_published_reasons(&mut s, "selection");
    invoke(&mut s, CommandId::ScaleRotate);
    assert_published_reasons(&mut s, "transform");
    invoke(&mut s, CommandId::CancelTransform);
    invoke(&mut s, CommandId::QuickMask);
    assert_published_reasons(&mut s, "quick mask");
    let mut s = distorted_pixel_selection();
    invoke(&mut s, CommandId::ApplyTransform);
    assert!(s.region_tools.applying_transform());
    assert_published_reasons(&mut s, "applying a distortion");
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
    assert_published_reasons(&mut s, "paper");
    let reasons: Vec<_> = s.state.commands.iter().filter_map(|c| c.disabled_reason.as_deref()).collect();
    assert!(reasons.contains(&"Nothing to undo"));
    assert!(reasons.contains(&s.localization().text(super::notices::NO_REFERENCE_BELOW).as_ref()));
    s.dispatch(UiAction::SelectLayer { id: 1 }).unwrap();
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: 1, value: true } }).unwrap();
    assert_published_reasons(&mut s, "locked");
}

#[test]
fn disabled_reasons_stay_steady_during_a_canvas_contact() {
    let mut s = session(Platform::Gtk);
    s.frame(1, 1).unwrap();
    let before = s.state.commands.clone();
    let undo = CommandId::ALL.iter().position(|c| *c == CommandId::Undo).unwrap();
    assert_eq!(before[undo].disabled_reason.as_deref(), Some("Nothing to undo"));
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Move, 1.)).unwrap();
    s.frame(2, 2).unwrap();
    assert!(s.require_idle().is_err());
    assert_eq!(
        s.command_disabled_reason(CommandId::AddLayer).as_deref(),
        Some("Finish the canvas interaction first")
    );
    assert!(!s.refresh_commands(), "the contact publishes no availability change");
    for ((published, previous), id) in s.state.commands.iter().zip(&before).zip(CommandId::ALL) {
        assert_eq!(published.enabled, previous.enabled, "{id:?}");
        assert_eq!(published.disabled_reason, previous.disabled_reason, "{id:?}");
    }
    s.pen(event(&s, 3, PenPhase::Up, 1.)).unwrap();
    s.frame(3, 3).unwrap();
    assert!(s.state.commands[undo].enabled);
    assert_eq!(s.state.commands[undo].disabled_reason, None);
}

#[test]
fn canvas_bar_items_carry_the_reason_they_are_disabled() {
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::Lasso);
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: 1, value: true } }).unwrap();
    let bar = s.state.canvas_bar.clone().expect("selection bar");
    let fill = bar
        .items
        .iter()
        .find_map(|i| match &i.option {
            ToolOption::Action { state, .. } if state.id == CommandId::FillSelection => Some(state),
            _ => None,
        })
        .unwrap();
    assert!(!fill.enabled);
    assert_eq!(fill.disabled_reason.as_deref(), Some("The active layer is locked"));
    let deselect = bar.items.iter().find_map(|i| match &i.option {
        ToolOption::Action { state, .. } if state.id == CommandId::Deselect => Some(state),
        _ => None,
    });
    assert_eq!(deselect.unwrap().disabled_reason, None);
}

#[test]
fn apply_mask_explains_group_and_effect_masks() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
    let group = s.engine.document().active_layer.0;
    s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id: group, replace: false } }).unwrap();
    let apply = UiAction::Layer { action: LayerAction::ApplyMask { id: group } };
    let error = s.dispatch(apply.clone()).unwrap_err();
    assert_eq!(error, "A group's mask can't be applied on its own; Merge Group applies it");
    let command = UiAction::Invoke { command: CommandId::ApplyLayerMask };
    let entry = s
        .command_catalog()
        .into_iter()
        .find(|d| d.id == command_catalog::identity(&command))
        .unwrap();
    assert_eq!(entry.disabled_reason.as_deref(), Some(error.as_str()));
    assert_eq!(entry.label, "Apply mask to layer", "the Layer menu routes the active layer's Apply to the command");
    assert_eq!(s.dispatch(command).unwrap_err(), error);
    assert_eq!(
        art_layers::apply_mask_refusal(LayerKind::Effect, s.localization()).as_deref(),
        Some("An effect layer's mask sets where the effect shows; it can't be applied")
    );
}

#[test]
fn command_search_matches_active_and_canonical_labels_with_stable_ids() {
    let mut s = UiSession::blank_localized(Recorder::default(), [256, 256], Platform::Gtk,
        Localizer::shared(UiLanguage::Japanese)).unwrap();
    invoke(&mut s, CommandId::SearchCommands);
    for query in ["元に戻す", "undo", "ＵＮＤＯ"] {
        search_action(&mut s, CommandSearchAction::Query { text: query.into() });
        let results = &s.state.command_search.as_ref().unwrap().results;
        let undo = results.iter().find(|row| row.id == "command.undo").unwrap();
        assert_eq!(undo.label, "元に戻す");
    }
    let catalog = s.command_catalog();
    assert!(catalog.iter().any(|row| row.category == "ブラシ"));
    for query in ["brush", "ブラシ"] {
        search_action(&mut s, CommandSearchAction::Query { text: query.into() });
        assert!(s.state.command_search.as_ref().unwrap().results.iter().all(|row| row.category != "ブラシ"));
    }
}

#[test]
fn disabled_command_reasons_retain_cached_labels_across_refreshes() {
    let mut s = session(Platform::Gtk);
    let cached = s.localization().text(MessageId::COMMANDS_NOTHING_TO_UNDO);
    assert!(std::sync::Arc::ptr_eq(&cached, &s.disabled_reason_unchecked(CommandId::Undo)));
    for _ in 0..2 {
        s.refresh_commands();
        let undo = s.state.commands.iter().find(|command| command.id == CommandId::Undo).unwrap();
        assert!(std::sync::Arc::ptr_eq(&cached, undo.disabled_reason.as_ref().unwrap()));
        assert_eq!(serde_json::to_value(undo).unwrap()["disabled_reason"], "Nothing to undo");
    }
}

#[test]
fn static_refusal_providers_retain_cached_reasons_before_published_comparison() {
    fn shared(left: Option<std::sync::Arc<str>>, right: Option<std::sync::Arc<str>>) {
        assert!(std::sync::Arc::ptr_eq(&left.unwrap(), &right.unwrap()));
    }
    let mut s = session(Platform::Gtk);
    for command in [CommandId::GrowSelection, CommandId::ShrinkSelection, CommandId::FeatherSelection,
        CommandId::BorderSelection, CommandId::SmoothSelection, CommandId::CropCanvasToSelection,
        CommandId::PasteInto, CommandId::TransformSelectionOutline, CommandId::UseReferenceBelow,
        CommandId::ClearSelected, CommandId::ClearOutside, CommandId::CutSelectionToLayer,
        CommandId::RevertToOriginal, CommandId::MergeGroup, CommandId::StraightenToGuide] {
        assert!(!s.command(command).enabled, "{command:?}");
        let direct = s.disabled_reason_unchecked(command);
        assert!(std::sync::Arc::ptr_eq(&direct, &s.disabled_reason_unchecked(command)), "{command:?}");
        s.refresh_commands();
        let published = s.state.commands.iter().find(|state| state.id == command).unwrap();
        assert!(std::sync::Arc::ptr_eq(&direct, published.disabled_reason.as_ref().unwrap()), "{command:?}");
    }
    s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
    shared(s.copy_refusal(CommandId::Copy), s.copy_refusal(CommandId::Copy));
    shared(s.separation_refusal(), s.separation_refusal());
    shared(art_layers::apply_mask_refusal(LayerKind::Group, s.localization()),
        art_layers::apply_mask_refusal(LayerKind::Group, s.localization()));
    invoke(&mut s, CommandId::QuickMask);
    shared(s.canvas_geometry_refusal(), s.canvas_geometry_refusal());
    shared(s.dodge_burn_refusal(), s.dodge_burn_refusal());
    shared(s.copy_refusal(CommandId::Copy), s.copy_refusal(CommandId::Copy));
    shared(s.clear_refusal(), s.clear_refusal());
    let mut s = filled_selection_session();
    invoke(&mut s, CommandId::ScaleRotate);
    assert!(std::sync::Arc::ptr_eq(&s.operation_refusal(), &s.operation_refusal()));
    let mut document = Document::new("HDR", 32, 32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.color.depth = layer_core::color::SampleDepth::F32;
    let renderer = Recorder { color: document.color, ..Default::default() };
    let s = UiSession::new(renderer, document, [256, 256], Platform::Gtk).unwrap();
    shared(s.blending_refusal(), s.blending_refusal());
}

#[test]
fn toolbar_visibility_search_projects_default_and_literal_custom_titles() {
    let localization = Localizer::shared(UiLanguage::Japanese);
    let mut session = UiSession::blank_localized(Recorder::default(), [256, 256], Platform::Gtk, localization.clone()).unwrap();
    let descriptor = |session: &UiSession<Recorder>| {
        let identity = command_catalog::identity(&UiAction::Customize { action: CustomizationAction::SetPanelVisible { panel: Panel::Toolbar, visible: true } });
        session.command_catalog().into_iter().find(|entry| entry.id == identity).unwrap().label
    };
    assert!(descriptor(&session).contains("ツール"));
    session.dispatch(UiAction::Customize { action: CustomizationAction::RenameToolbar { panel: Panel::Toolbar } }).unwrap();
    session.dispatch(UiAction::Customize { action: CustomizationAction::ToolbarName { name: "Tools 日本語 {literal}".into() } }).unwrap();
    session.dispatch(UiAction::Customize { action: CustomizationAction::ConfirmToolbar }).unwrap();
    assert!(descriptor(&session).contains("Tools 日本語 {literal}"));
    invoke(&mut session, CommandId::UndoWorkspace);
    assert!(descriptor(&session).contains("ツール"));
}

#[test]
fn command_numbers_refuse_compatibility_input_and_leave_rejected_edits_unchanged() {
    let mut s = session(Platform::Gtk);
    let before = s.state.brush.diameter;
    for text in ["ｓｑｒｔ（－１）", "１２＋３ ｐｘ", "２＋３", "２３", "１２３", "sqrt(-1)"] {
        assert!(s.execute_catalog_command("tool_setting.size", Some(text.into())).is_err());
        assert_eq!(s.state.brush.diameter, before);
        assert_eq!(s.engine.configured_brush().diameter, before);
    }
    s.execute_catalog_command("tool_setting.size", Some("12+3 px".into())).unwrap();
    assert_eq!(s.state.brush.diameter, 15.);
    assert_eq!(s.engine.configured_brush().diameter, 15.);
}

#[test]
fn numeric_parameters_match_active_and_canonical_labels_with_stable_targets() {
    let mut canonical = session(Platform::Gtk);
    invoke(&mut canonical, CommandId::AddLayer);
    canonical.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "exposure".into() } }).unwrap();
    canonical.dispatch(UiAction::Layer { action: LayerAction::Rename { id: canonical.engine.document().active_layer.0, name: "Literal { $name }".into() } }).unwrap();
    let english = canonical.command_catalog();
    for language in [UiLanguage::Japanese, UiLanguage::SimplifiedChinese,
        UiLanguage::TraditionalChinese, UiLanguage::Korean] {
        let mut s = UiSession::blank_localized(Recorder::default(), [256, 256], Platform::Gtk,
            Localizer::shared(language)).unwrap();
        invoke(&mut s, CommandId::AddLayer);
        s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "exposure".into() } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Rename { id: s.engine.document().active_layer.0, name: "Literal { $name }".into() } }).unwrap();
        let active = s.command_catalog();
        assert!(active.iter().any(|row| row.parameter.is_some() && row.id.starts_with("layer_property.") && row.id != "layer_property.opacity"));
        invoke(&mut s, CommandId::SearchCommands);
        for row in active.iter().filter(|row| row.parameter.is_some()) {
            let canonical = english.iter().find(|item| item.id == row.id).unwrap();
            for label in [&row.label, &canonical.label] {
                search_action(&mut s, CommandSearchAction::Query { text: label.trim_end_matches('…').into() });
                let result = s.state.command_search.as_ref().unwrap().results.iter()
                    .find(|result| result.id == row.id).unwrap_or_else(|| panic!("{language:?}: {} -> {}", label, row.id));
                assert_eq!(result.label, row.label);
                assert_eq!(serde_json::to_value(&result.parameter).unwrap(), serde_json::to_value(&row.parameter).unwrap());
            }
        }
        let size = active.iter().find(|row| row.id == "tool_setting.size").unwrap();
        search_action(&mut s, CommandSearchAction::Query { text: "brush size".into() });
        assert_eq!(s.state.command_search.as_ref().unwrap().results[0].id, size.id);
    }
}


#[test]
fn filter_insertions_match_active_and_canonical_labels_with_stable_targets() {
    let english = Localizer::shared(UiLanguage::English);
    for language in [UiLanguage::Japanese, UiLanguage::SimplifiedChinese,
        UiLanguage::TraditionalChinese, UiLanguage::Korean]
    {
        let localization = Localizer::shared(language);
        let mut s = UiSession::blank_localized(Recorder::default(), [256, 256], Platform::Windows,
            localization.clone()).unwrap();
        for (effect, label, display) in [("exposure", MessageId::RESOURCES_FILTER_EXPOSURE, MessageId::RESOURCES_FILTER_EXPOSURE),
            ("gaussian_blur", MessageId::RESOURCES_FILTER_GAUSSIAN_BLUR, MessageId::RESOURCES_FILTER_GAUSSIAN_BLUR),
            ("solid_color", MessageId::RESOURCES_FILTER_SOLID_COLOR, MessageId::MENU_SOLID_COLOR_FILL)]
        {
            let action = UiAction::Effect { action: EffectAction::Insert { effect: effect.into() } };
            let id = command_catalog::identity(&action);
            let row = s.command_catalog().into_iter().find(|row| row.id == id).unwrap();
            assert_eq!(row.label, localization.text(display).to_string());
            assert!(row.enabled);
            invoke(&mut s, CommandId::SearchCommands);
            for query in [english.text(label), localization.text(display)] {
                search_action(&mut s, CommandSearchAction::Query { text: query.to_string() });
                let result = s.state.command_search.as_ref().unwrap().results.iter()
                    .find(|result| result.id == id).unwrap_or_else(|| panic!("{language:?}: {query} -> {id}"));
                assert_eq!(result.label, row.label);
                assert_eq!(result.enabled, row.enabled);
                assert!(result.parameter.is_none());
            }
            search_action(&mut s, CommandSearchAction::Execute { id, value: None });
            assert!(s.state.command_search.is_none());
            let document = s.engine.document();
            let inserted = document.layer(document.active_layer).unwrap().effect.as_ref().unwrap();
            assert_eq!(&*inserted.program.id, effect);
            assert_eq!(inserted.program.label, s.effect_catalog.get(effect).unwrap().program.label);
        }
    }
}

#[test]
fn filter_insertion_search_keeps_admitted_literal_replacements_and_custom_labels() {
    use layer_core::EffectInstallMode;
    let replacement_label = "私の明るさ { $label } 한글 \u{2068}mix\u{2069}";
    let custom_label = "My literal filter { $label } 한글 🎨";
    for language in [UiLanguage::Japanese, UiLanguage::SimplifiedChinese,
        UiLanguage::TraditionalChinese, UiLanguage::Korean]
    {
        let mut s = UiSession::blank_localized(Recorder::default(), [256, 256], Platform::Windows,
            Localizer::shared(language)).unwrap();
        s.frame(0, 0).unwrap();
        let mut replacement = s.effect_catalog.get("exposure").unwrap().clone();
        std::sync::Arc::make_mut(&mut replacement.program).label = replacement_label.into();
        let mut custom = s.effect_catalog.get("gaussian_blur").unwrap().clone();
        let program = std::sync::Arc::make_mut(&mut custom.program);
        program.id = "test.literal-filter".into();
        program.label = custom_label.into();
        let package = package_json(s.effect_catalog.categories().to_vec(), vec![replacement, custom]);
        s.load_effect_library(&package, |_| panic!("inline program"), EffectInstallMode::Merge).unwrap();
        s.renderer_mut().validation_result = Some(layer_render::EffectValidationResult {
            request_id: s.state.filter_load.request_id, result: Ok(()),
        });
        s.frame(1, 1).unwrap();
        assert!(!s.state.filter_load.pending);
        let replacement_id = command_catalog::identity(&UiAction::Effect {
            action: EffectAction::Insert { effect: "exposure".into() },
        });
        invoke(&mut s, CommandId::SearchCommands);
        search_action(&mut s, CommandSearchAction::Query { text: "Exposure".into() });
        assert!(s.state.command_search.as_ref().unwrap().results.iter().all(|row| row.id != replacement_id));
        for (effect, literal) in [("exposure", replacement_label), ("test.literal-filter", custom_label)] {
            let id = command_catalog::identity(&UiAction::Effect { action: EffectAction::Insert { effect: effect.into() } });
            search_action(&mut s, CommandSearchAction::Query { text: literal.into() });
            let row = s.state.command_search.as_ref().unwrap().results.iter().find(|row| row.id == id).unwrap();
            assert_eq!(row.label, literal);
            assert!(row.enabled);
            search_action(&mut s, CommandSearchAction::Execute { id, value: None });
            let document = s.engine.document();
            let inserted = document.layer(document.active_layer).unwrap().effect.as_ref().unwrap();
            assert_eq!(&*inserted.program.id, effect);
            assert_eq!(inserted.program.label, layer_core::ResourceLabel::from(literal));
            invoke(&mut s, CommandId::SearchCommands);
        }
    }
}
