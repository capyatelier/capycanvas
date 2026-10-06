fn relationship_row(s: &UiSession<Recorder>, id: u64) -> &LayerState {
    s.state.layers.iter().find(|row| row.id == id).unwrap()
}

#[test]
fn sole_editing_references_keep_the_lighthouse_without_a_paint_target() {
    for platform in Platform::ALL {
        for group in [false, true] {
            for mask in [false, true] {
                let mut s = session(platform);
                let base = occurrence_token(s.engine.document().working.occurrence.unwrap());
                layer(&mut s, LayerAction::New { group, clipped: false });
                let id = occurrence_token(s.engine.document().working.occurrence.unwrap());
                if mask { layer(&mut s, LayerAction::AddMask { id, replace: false }); }
                let ordinary = relationship_row(&s, id).selection_icon;
                layer(&mut s, LayerAction::ReferenceSelection);
                assert!(relationship_row(&s, id).selected && relationship_row(&s, id).editing);
                assert_eq!(relationship_row(&s, id).selection_icon, "layer-reference-symbolic");
                invoke(&mut s, CommandId::Undo);
                assert_eq!(relationship_row(&s, id).selection_icon, ordinary);
                invoke(&mut s, CommandId::Redo);
                assert_eq!(relationship_row(&s, id).selection_icon, "layer-reference-symbolic");
                layer(&mut s, LayerAction::Select { id: base, mask: false });
                assert_eq!(relationship_row(&s, id).selection_icon, "layer-reference-symbolic");
                layer(&mut s, LayerAction::ToggleSelection { id });
                assert_eq!(relationship_row(&s, id).selection_icon, "layer-selection-checked-symbolic");
                layer(&mut s, LayerAction::ToggleSelection { id: base });
                assert_eq!(relationship_row(&s, id).selection_icon, "layer-selection-checked-symbolic");
                layer(&mut s, LayerAction::Select { id, mask: false });
                assert_eq!(relationship_row(&s, id).selection_icon, "layer-reference-symbolic");
                layer(&mut s, LayerAction::Lock { id, value: true });
                assert_eq!(relationship_row(&s, id).selection_icon, "layer-reference-symbolic");
            }
        }
    }
}

#[test]
fn layer_settings_keep_labels_checks_and_packaged_icons_on_every_host() {
    fn icons(sections: &[Vec<ContextMenuItem>]) {
        for item in sections.iter().flatten() {
            if let Some(icon) = item.icon.as_deref() { assert!(crate::icon_ships(icon), "{icon}: missing menu icon"); }
            icons(&item.sections);
        }
    }
    for platform in Platform::ALL {
        let mut s = session(platform);
        layer(&mut s, LayerAction::New { group: true, clipped: false });
        let group = occurrence_token(s.engine.document().working.occurrence.unwrap());
        assert_eq!(relationship_row(&s, group).description, "");
        s.dispatch(UiAction::SetLayerOpacity { id: Some(group), opacity: 0.5 }).unwrap();
        assert_eq!(relationship_row(&s, group).description, "50%");
        s.dispatch(UiAction::SetLayerOpacity { id: Some(group), opacity: 1. }).unwrap();
        for checked in [false, true, false] {
            let menu = s.layer_menu(group, false).unwrap();
            icons(&menu.sections);
            let mode = menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten())
                .find(|item| item.icon.as_deref() == Some("group-pass-through")).unwrap();
            assert_eq!(mode.label, "Pass Through");
            assert_eq!(mode.selected, Some(checked));
            assert!(mode.enabled);
            s.dispatch(mode.action.clone().unwrap()).unwrap();
        }
        layer(&mut s, LayerAction::TogglePassThrough { id: group });
        for checked in [false, true] {
            let menu = s.layer_menu(group, false).unwrap();
            icons(&menu.sections);
            let clip = menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten())
                .find(|item| item.icon.as_deref() == Some("clip")).unwrap();
            assert_eq!(clip.label, "Clip to Layer Below");
            assert_eq!(clip.selected, Some(checked));
            assert!(clip.enabled);
            assert_eq!(clip.hint, if checked { "Clipped to Current ink" } else { "Clip to Current ink" });
            s.dispatch(clip.action.clone().unwrap()).unwrap();
        }
        invoke(&mut s, CommandId::Undo);
        assert!(s.state.layer_tools.attachment.checked);
        assert_eq!(s.state.layer_tools.attachment.label, "Clip to Layer Below");
        invoke(&mut s, CommandId::Redo);
        assert!(!s.state.layer_tools.attachment.checked);
    }
}

#[test]
fn new_group_wraps_checked_rows_and_reordering_moves_them_together() {
    let mut s = session(Platform::Gtk);
    let first = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let second = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let third = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::ToggleSelection { id: occurrence_token(first) });
    let checked = s.selected_layers().clone();
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    let group = s.engine.document().working.occurrence.unwrap();
    assert_eq!(s.engine.document().scene().children(Some(group)), [third, first]);
    layer(&mut s, LayerAction::Ungroup { id: occurrence_token(group) });
    assert_eq!(s.selected_layers(), &checked);
    invoke(&mut s, CommandId::Undo);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.selected_layers(), &checked);
    assert_eq!(s.engine.document().scene().children(None)[..3], [third, second, first]);
    let paper = s.state.layers.iter().find(|row| row.label == "Paper").unwrap().id;
    layer(&mut s, LayerAction::Drop { id: occurrence_token(third), target: paper, fraction: 1., surface: LayerDropSurface::Row });
    assert_eq!(s.engine.document().scene().children(None), [second, occurrence_handle(paper).unwrap(), third, first]);
    assert_eq!(s.selected_layers(), &checked);
    invoke(&mut s, CommandId::Undo);
    invoke(&mut s, CommandId::DeleteLayer);
    assert!(s.engine.document().scene().occurrence(third).is_none());
    assert!(s.engine.document().scene().occurrence(first).is_none());
    assert!(s.engine.document().scene().occurrence(second).is_some());
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.selected_layers(), &checked);
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    layer(&mut s, LayerAction::Lock { id: occurrence_token(first), value: true });
    assert!(!s.command(CommandId::LowerLayer).enabled);
    assert_eq!(s.command_disabled_reason(CommandId::LowerLayer).as_deref(), Some("The layer is locked"));
    layer(&mut s, LayerAction::Lock { id: occurrence_token(first), value: false });
    layer(&mut s, LayerAction::Select { id: occurrence_token(third), mask: false });
    layer(&mut s, LayerAction::ToggleSelection { id: occurrence_token(second) });
    for command in [CommandId::RaiseLayer, CommandId::LowerLayer] {
        assert!(!s.command(command).enabled);
        assert_eq!(s.command_disabled_reason(command).as_deref(), Some("Select layers in the same group"));
    }
}

#[test]
fn layer_steps_use_siblings_for_paint_groups_and_filters() {
    let mut s = session(Platform::Gtk);
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    let group = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let first = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let second = s.engine.document().working.occurrence.unwrap();
    assert!(!s.command(CommandId::RaiseLayer).enabled);
    assert_eq!(s.command_disabled_reason(CommandId::RaiseLayer).as_deref(), Some("The layer is already at the top"));
    assert!(s.command(CommandId::LowerLayer).enabled);
    invoke(&mut s, CommandId::LowerLayer);
    assert_eq!(s.engine.document().scene().children(Some(group)), [first, second]);
    assert!(!s.command(CommandId::LowerLayer).enabled);
    assert_eq!(s.command_disabled_reason(CommandId::LowerLayer).as_deref(), Some("The layer is already at the bottom"));
    invoke(&mut s, CommandId::RaiseLayer);
    assert_eq!(s.engine.document().scene().children(Some(group)), [second, first]);
    insert_effect(&mut s, "curves");
    let filter = s.engine.document().working.occurrence.unwrap();
    assert!(!relationship_row(&s, occurrence_token(filter)).content_selected);
    invoke(&mut s, CommandId::LowerLayer);
    assert_eq!(s.engine.document().scene().children(Some(group)), [second, filter, first]);
    layer(&mut s, LayerAction::Select { id: occurrence_token(group), mask: false });
    invoke(&mut s, CommandId::LowerLayer);
    assert_eq!(s.engine.document().scene().children(None)[1], group);
}

#[test]
fn layer_creation_obeys_locks_and_reveals_its_destination() {
    let mut s = session(Platform::Gtk);
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    let group = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::Collapse { id: occurrence_token(group) });
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let child = s.engine.document().working.occurrence.unwrap();
    assert!(relationship_row(&s, occurrence_token(child)).selected);
    layer(&mut s, LayerAction::Lock { id: occurrence_token(group), value: true });
    let before = s.engine.document().clone();
    assert!(s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }).is_err());
    assert_eq!(s.engine.document(), &before);
    let menu = s.layer_menu(occurrence_token(child), false).unwrap();
    assert!(!menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten())
        .find(|item| matches!(item.action, Some(UiAction::Layer { action: LayerAction::Duplicate { .. } }))).unwrap().enabled);
    layer(&mut s, LayerAction::Lock { id: occurrence_token(group), value: false });
    layer(&mut s, LayerAction::SelectAllLayers { selected: true });
    layer(&mut s, LayerAction::DeleteSelected);
    assert!(s.engine.document().scene().order().is_empty());
    assert_eq!(s.image_layer_destination(None).unwrap(), (0, None));
    insert_effect(&mut s, "solid_color");
    assert_eq!(s.engine.document().scene().order().len(), 1);
    assert!(s.state.layers[0].selected);
    assert!(!s.state.layers[0].content_selected);
}

#[test]
fn collapsed_folder_deletion_keeps_unchecked_external_effects() {
    let mut s = session(Platform::Gtk);
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    let group = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let child = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::Select { id: occurrence_token(group), mask: false });
    insert_effect(&mut s, "curves");
    let effect = s.engine.document().working.occurrence.unwrap();
    layer(&mut s, LayerAction::Clip { id: occurrence_token(effect), value: true });
    layer(&mut s, LayerAction::Select { id: occurrence_token(group), mask: false });
    assert!(!s.state.layer_tools.can_delete);
    layer(&mut s, LayerAction::Collapse { id: occurrence_token(group) });
    assert!(s.state.layer_tools.can_delete);
    layer(&mut s, LayerAction::DeleteSelected);
    assert!(s.engine.document().scene().occurrence(group).is_none());
    assert!(s.engine.document().scene().occurrence(child).is_none());
    assert_eq!(s.engine.document().scene().occurrence(effect).unwrap().attachment, layer_core::Attachment::None);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().scene().effect_owner(effect), Some(group));
}

fn assert_adjacent_effect_connections(s: &UiSession<Recorder>) {
    for edge in s.state.layer_tools.connections.iter().filter(|edge| edge.kind == LayerRelationKind::Effect) {
        let from = s.state.layers.iter().position(|row| row.id == edge.from).unwrap();
        let to = s.state.layers.iter().position(|row| row.id == edge.to).unwrap();
        assert_eq!(to, from + 1);
    }
}

#[test]
fn layer_relationship_drop_previews_keep_outer_gaps_unattached() {
    let mut s=session(Platform::Gtk);
    let base=occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s,LayerAction::New{group:false,clipped:true});
    let owner=occurrence_token(s.engine.document().working.occurrence.unwrap());
    insert_effect(&mut s,"motion_blur");
    let effect=occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s,LayerAction::Clip{id:effect,value:true});
    layer(&mut s,LayerAction::New{group:false,clipped:false});
    let paint=occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s,LayerAction::Drop{id:paint,target:base,fraction:1.,surface:LayerDropSurface::Row});
    for target in [effect,owner] {
        let before=s.engine.document().clone();
        assert_eq!(s.layer_drop_preview(paint,target,0.,LayerDropSurface::Row),Some(LayerDropHint{target:effect,position:LayerDropPosition::Above, effect_owner: None}));
        assert_eq!(s.engine.document(),&before);
        layer(&mut s,LayerAction::Drop{id:paint,target,fraction:0.,surface:LayerDropSurface::Row});
        assert_eq!(relationship_row(&s,paint).relationship,None);
        assert_eq!(relationship_row(&s,effect).relationship,Some(LayerRelation{kind:LayerRelationKind::Effect,target:owner}));
        assert_eq!(relationship_row(&s,owner).relationship,Some(LayerRelation{kind:LayerRelationKind::Clip,target:base}));
        invoke(&mut s,CommandId::Undo);
        assert_live_artwork_eq(s.engine.document(),&before);
    }
    layer(&mut s,LayerAction::Drop{id:paint,target:base,fraction:0.,surface:LayerDropSurface::Row});
    assert_eq!(relationship_row(&s,paint).relationship,Some(LayerRelation{kind:LayerRelationKind::Clip,target:base}));
}

#[test]
fn clipping_filter_previews_publish_the_owner_and_commit_selected_filters_atomically() {
    for platform in Platform::ALL {
        let mut s=session(platform);let base=occurrence_token(s.engine.document().working.occurrence.unwrap());
        layer(&mut s,LayerAction::New{group:false,clipped:true});let owner=occurrence_token(s.engine.document().working.occurrence.unwrap());
        insert_effect(&mut s,"motion_blur");let existing=occurrence_token(s.engine.document().working.occurrence.unwrap());
        layer(&mut s,LayerAction::AttachEffect{id:existing,owner});
        let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
        assert_eq!(s.layer_drop_preview(existing,owner,0.,LayerDropSurface::Row),None);
        layer(&mut s,LayerAction::Drop{id:existing,target:owner,fraction:0.,surface:LayerDropSurface::Row});
        assert_eq!(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),checkpoint);
        let mut free=Vec::new();
        for effect in ["exposure","curves"] {
            layer(&mut s,LayerAction::Select{id:base,mask:false});insert_effect(&mut s,effect);
            free.push(occurrence_token(s.engine.document().working.occurrence.unwrap()));
        }
        layer(&mut s,LayerAction::ToggleSelection{id:free[0]});
        let before=s.engine.document().clone();
        for (target,fraction) in [(existing,1.),(owner,0.)] {
            let preview_before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
            assert_eq!(s.layer_drop_preview(free[1],target,fraction,LayerDropSurface::Row),Some(LayerDropHint{target:owner,position:LayerDropPosition::Above,effect_owner:Some(owner)}));
            assert_eq!(s.engine.document(),&preview_before);assert_eq!(s.engine.checkpoint(),checkpoint);
            layer(&mut s,LayerAction::Drop{id:free[1],target,fraction,surface:LayerDropSurface::Row});
            assert_eq!(s.engine.document().scene().attached_effects(occurrence_handle(owner).unwrap()),[occurrence_handle(free[1]).unwrap(),occurrence_handle(free[0]).unwrap(),occurrence_handle(existing).unwrap()]);
            invoke(&mut s,CommandId::Undo);assert_live_artwork_eq(s.engine.document(),&before);
            invoke(&mut s,CommandId::Redo);
            for id in &free {assert_eq!(relationship_row(&s,*id).relationship,Some(LayerRelation{kind:LayerRelationKind::Effect,target:owner}));}
            invoke(&mut s,CommandId::Undo);
        }
        layer(&mut s,LayerAction::Select{id:free[0],mask:false});
        assert_eq!(s.layer_drop_preview(free[0],base,0.,LayerDropSurface::Row).unwrap().effect_owner,Some(base));
        layer(&mut s,LayerAction::Drop{id:free[0],target:base,fraction:0.,surface:LayerDropSurface::Row});
        invoke(&mut s,CommandId::RaiseLayer);
        assert_eq!(relationship_row(&s,free[0]).relationship,Some(LayerRelation{kind:LayerRelationKind::Effect,target:owner}));
        invoke(&mut s,CommandId::LowerLayer);
        assert_eq!(relationship_row(&s,free[0]).relationship,Some(LayerRelation{kind:LayerRelationKind::Effect,target:base}));
        layer(&mut s,LayerAction::Lock{id:base,value:true});
        assert!(!s.command(CommandId::RaiseLayer).enabled);
        assert_eq!(s.layer_drop_preview(free[0],owner,0.5,LayerDropSurface::Thumbnail),None);
    }
}

#[test]
fn layer_relationships_publish_owner_chains_and_clipping_from_the_top_effect() {
    let mut s = session(Platform::Gtk);
    let base = occurrence_token(s.engine.document().working.occurrence.unwrap());
    assert!(!relationship_row(&s, base).adjustment_effect);
    let paper = s.state.layers.iter().find(|row| row.label == "Paper").unwrap();
    assert!(!paper.adjustment_effect);
    assert!(paper.has_thumbnail);
    layer(&mut s, LayerAction::Rename { id: base, name: "Base {ink} 🎨".into() });
    layer(&mut s, LayerAction::New { group: false, clipped: true });
    let owner = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::Rename { id: owner, name: "Shadows".into() });
    assert_eq!(s.state.layer_tools.attachment.label, "Clip to Layer Below");
    assert_eq!(s.state.layer_tools.attachment.description, "Clipped to Base {ink} 🎨");
    let mut effects = Vec::new();
    for effect in ["curves", "exposure"] {
        insert_effect(&mut s, effect);
        let id = occurrence_token(s.engine.document().working.occurrence.unwrap());
        layer(&mut s, LayerAction::Clip { id, value: true });
        effects.push(id);
        assert_eq!(relationship_row(&s, id).relationship, Some(LayerRelation { kind: LayerRelationKind::Effect, target: owner }));
        assert!(relationship_row(&s, id).adjustment_effect);
        assert_eq!(relationship_row(&s, id).selection_icon, "layer-selection-checked-symbolic");
    }
    assert_eq!(relationship_row(&s, owner).relationship, Some(LayerRelation { kind: LayerRelationKind::Clip, target: base }));
    let connections = s.state.layer_tools.connections.clone();
    for (kind, from, to) in [
        (LayerRelationKind::Effect, effects[1], effects[0]),
        (LayerRelationKind::Effect, effects[0], owner),
        (LayerRelationKind::Clip, effects[1], base),
    ] {
        assert!(connections.contains(&LayerConnection { kind, from, to, depth: 0 }));
    }
    assert_eq!(connections.len(), 3);
    assert_adjacent_effect_connections(&s);
    assert_eq!(s.state.layer_tools.attachment.label, "Apply to layers below");
    assert_eq!(s.state.layer_tools.attachment.description, "Applied to Shadows");
    assert_eq!(s.state.layer_tools.attachment.icon, "layer-effect-link-symbolic");
    let menu = s.layer_menu(effects[1], false).unwrap();
    let attached = menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten()).find(|item| item.label == "Apply to layers below").unwrap();
    assert_eq!(attached.selected, Some(true));
    assert_eq!(attached.icon.as_deref(), Some("effect-link"));
    assert!(crate::icon_ships(attached.icon.as_deref().unwrap()));
    assert!(!menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten()).any(|item| item.label == "Clip to Layer Below"));
    layer(&mut s, LayerAction::Visibility { id: effects[0], value: false });
    layer(&mut s, LayerAction::Visibility { id: owner, value: false });
    assert!(!relationship_row(&s, owner).visible);
    assert!(relationship_row(&s, effects[1]).visible);
    assert!(relationship_row(&s, effects[1]).visibility_blocked);
    assert!(!relationship_row(&s, effects[0]).visible);
    assert!(!relationship_row(&s, effects[0]).visibility_blocked);
    assert_eq!(s.state.layer_tools.connections, connections);
    layer(&mut s, LayerAction::Visibility { id: owner, value: true });
    assert!(!relationship_row(&s, effects[1]).visibility_blocked);
    assert!(!relationship_row(&s, effects[0]).visible);
    assert_eq!(s.state.layer_tools.connections, connections);
    let release = s.state.layer_tools.attachment.action.clone().unwrap();
    layer(&mut s, release);
    assert_eq!(relationship_row(&s, effects[1]).relationship, None);
    assert_eq!(s.state.layer_tools.attachment.label, "Apply to Shadows");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.state.layer_tools.connections, connections);
    invoke(&mut s, CommandId::NewSelectionLayer);
    let saved = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::Drop { id: saved, target: owner, fraction: 1., surface: LayerDropSurface::Row });
    assert_eq!(s.layer_drop_preview(saved, owner, 0., LayerDropSurface::Row), Some(LayerDropHint { target: effects[1], position: LayerDropPosition::Above, effect_owner: None }));
    layer(&mut s, LayerAction::Drop { id: saved, target: owner, fraction: 0., surface: LayerDropSurface::Row });
    assert_adjacent_effect_connections(&s);
    invoke(&mut s, CommandId::ReturnToArtwork);
    layer(&mut s, LayerAction::Select { id: effects[1], mask: false });
    insert_effect(&mut s, "curves");
    let added = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::Drop { id: added, target: saved, fraction: 0., surface: LayerDropSurface::Row });
    layer(&mut s, LayerAction::Clip { id: added, value: true });
    assert_eq!(relationship_row(&s, added).relationship, Some(LayerRelation { kind: LayerRelationKind::Effect, target: owner }));
    let saved_position = s.state.layers.iter().position(|row| row.id == saved).unwrap();
    let added_position = s.state.layers.iter().position(|row| row.id == added).unwrap();
    assert_eq!(added_position, saved_position + 1);
    assert_eq!(s.state.layer_tools.connections.iter().filter(|edge| edge.kind == LayerRelationKind::Effect).count(), 3);
    assert_adjacent_effect_connections(&s);
}

#[test]
fn layer_relationships_keep_collapsed_group_edges_and_offer_reversible_swipe_actions() {
    let mut s = session(Platform::Gtk);
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    let group = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::Blend { id: group, value: layer_core::LayerBlend::Normal.code() });
    assert_eq!(relationship_row(&s, group).description, "");
    layer(&mut s, LayerAction::New { group: false, clipped: false });
    let child = occurrence_token(s.engine.document().working.occurrence.unwrap());
    assert!(matches!(relationship_row(&s, child).right_swipe, Some(LayerAction::ToggleAlphaLock { .. })));
    layer(&mut s, LayerAction::Select { id: group, mask: false });
    insert_effect(&mut s, "curves");
    let effect = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::Clip { id: effect, value: true });
    layer(&mut s, LayerAction::Select { id: group, mask: false });
    layer(&mut s, LayerAction::Collapse { id: group });
    assert!(!s.state.layers.iter().any(|row| row.id == child));
    let edge = LayerConnection { kind: LayerRelationKind::Effect, from: effect, to: group, depth: 0 };
    assert!(s.state.layer_tools.connections.contains(&edge));
    assert_adjacent_effect_connections(&s);
    assert!(relationship_row(&s, group).right_swipe.is_none());
    layer(&mut s, LayerAction::Visibility { id: group, value: false });
    assert!(!relationship_row(&s, group).visible);
    assert!(relationship_row(&s, effect).visible);
    assert!(relationship_row(&s, effect).visibility_blocked);
    assert!(s.state.layer_tools.connections.contains(&edge));
    layer(&mut s, LayerAction::Clip { id: effect, value: false });
    layer(&mut s, LayerAction::Blend { id: group, value: layer_core::LayerBlend::Multiply.code() });
    layer(&mut s, LayerAction::Lock { id: group, value: true });
    assert!(relationship_row(&s, group).right_swipe.is_none());
    layer(&mut s, LayerAction::Lock { id: group, value: false });
    let before = s.engine.document().clone();
    let swipe = relationship_row(&s, group).right_swipe.clone().unwrap();
    s.refresh_layer_presentation();
    assert_eq!(s.engine.document(), &before);
    let menu = s.layer_menu(group, false).unwrap();
    let mode = menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten()).find(|item| item.icon.as_deref() == Some("group-pass-through")).unwrap();
    assert_eq!(mode.label, "Pass Through");
    assert_eq!(mode.selected, Some(false));
    assert!(crate::icon_ships(mode.icon.as_deref().unwrap()));
    assert!(mode.enabled);
    assert_eq!(mode.action, Some(UiAction::Layer { action: swipe.clone() }));
    layer(&mut s, swipe);
    assert!(relationship_row(&s, group).pass_through);
    assert_eq!(relationship_row(&s, group).description, "Pass Through");
    let menu = s.layer_menu(group, false).unwrap();
    let mode = menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten()).find(|item| item.icon.as_deref() == Some("group-pass-through")).unwrap();
    assert_eq!(mode.label, "Pass Through");
    assert_eq!(mode.selected, Some(true));
    let isolate = relationship_row(&s, group).right_swipe.clone().unwrap();
    layer(&mut s, isolate);
    assert_eq!(relationship_row(&s, group).blend, layer_core::LayerBlend::Normal.code());
    invoke(&mut s, CommandId::Undo);
    assert!(relationship_row(&s, group).pass_through);
    invoke(&mut s, CommandId::Redo);
    assert_eq!(relationship_row(&s, group).description, "");
}

#[test]
fn layer_relationships_require_group_mode_changes_before_attachment() {
    let mut s = session(Platform::Gtk);
    let base = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    let group = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::Rename { id: group, name: "G{roup} 🎨".into() });
    layer(&mut s, LayerAction::Blend { id: group, value: layer_core::LayerBlend::PassThrough.code() });
    let control = s.state.layer_tools.attachment.clone();
    assert_eq!(control.label, "Clip to Layer Below");
    assert_eq!(control.description, "Turn off Pass Through on “G{roup} 🎨” first");
    assert_eq!(control.action, None);
    layer(&mut s, LayerAction::TogglePassThrough { id: group });
    assert_eq!(s.state.layer_tools.attachment.label, "Clip to Layer Below");
    assert_eq!(s.state.layer_tools.attachment.description, "Clip to Current ink");
    let action = s.state.layer_tools.attachment.action.clone().unwrap();
    layer(&mut s, action);
    assert!(!relationship_row(&s, group).pass_through);
    assert!(relationship_row(&s, group).right_swipe.is_none());
    assert_eq!(relationship_row(&s, group).relationship.unwrap().kind, LayerRelationKind::Clip);
    layer(&mut s, LayerAction::Clip { id: group, value: false });
    layer(&mut s, LayerAction::Blend { id: group, value: layer_core::LayerBlend::PassThrough.code() });
    layer(&mut s, LayerAction::Reparent { id: base, parent: None, index: 0 });
    layer(&mut s, LayerAction::Lock { id: group, value: true });
    layer(&mut s, LayerAction::Select { id: base, mask: false });
    assert_eq!((s.state.layer_tools.attachment.label.as_str(), s.state.layer_tools.attachment.action.as_ref()), ("Clip to Layer Below", None));
    invoke(&mut s, CommandId::QuickMask);
    assert!(relationship_row(&s, 0).relationship.is_none());
    assert!(relationship_row(&s, 0).right_swipe.is_none());
    assert!(!relationship_row(&s, 0).visibility_blocked);
    assert!(!relationship_row(&s, 0).adjustment_effect);
    assert_eq!(s.state.layer_tools.attachment.icon, "layer-clip-symbolic");
    assert_eq!(s.state.layer_tools.attachment.label, "Clip to Layer Below");
    assert_eq!(s.state.layer_tools.attachment.description, "No layer below to attach to");
    assert!(s.state.layer_tools.attachment.action.is_none());
}
