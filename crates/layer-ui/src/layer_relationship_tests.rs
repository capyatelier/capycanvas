fn relationship_row(s: &UiSession<Recorder>, id: u64) -> &LayerState {
    s.state.layers.iter().find(|row| row.id == id).unwrap()
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
        assert_eq!(s.layer_drop_preview(paint,target,0.,LayerDropSurface::Row),Some(LayerDropHint{target:effect,position:LayerDropPosition::Above}));
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
    assert_eq!(s.state.layer_tools.attachment.label, "Release clipping from Base {ink} 🎨");
    let mut effects = Vec::new();
    for effect in ["curves", "exposure"] {
        insert_effect(&mut s, effect);
        let id = occurrence_token(s.engine.document().working.occurrence.unwrap());
        layer(&mut s, LayerAction::Clip { id, value: true });
        effects.push(id);
        assert_eq!(relationship_row(&s, id).relationship, Some(LayerRelation { kind: LayerRelationKind::Effect, target: owner }));
        assert!(relationship_row(&s, id).adjustment_effect);
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
    assert_eq!(attached.icon, Some("layer-effect-link-symbolic"));
    assert!(!menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten()).any(|item| item.label == "Clip to layer below"));
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
    assert_eq!(s.layer_drop_preview(saved, owner, 0., LayerDropSurface::Row), Some(LayerDropHint { target: effects[1], position: LayerDropPosition::Above }));
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
    assert_eq!(relationship_row(&s, group).description, "Normal");
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
    let mode = menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten()).find(|item| item.label == "Use Pass Through").unwrap();
    assert!(mode.enabled);
    assert_eq!(mode.action, Some(UiAction::Layer { action: swipe.clone() }));
    layer(&mut s, swipe);
    assert!(relationship_row(&s, group).pass_through);
    assert_eq!(relationship_row(&s, group).description, "Pass Through");
    let isolate = relationship_row(&s, group).right_swipe.clone().unwrap();
    layer(&mut s, isolate);
    assert_eq!(relationship_row(&s, group).blend, layer_core::LayerBlend::Multiply.code());
    invoke(&mut s, CommandId::Undo);
    assert!(relationship_row(&s, group).pass_through);
    invoke(&mut s, CommandId::Redo);
    assert_eq!(relationship_row(&s, group).description, "Multiply");
}

#[test]
fn layer_relationships_offer_explicit_isolation_and_disable_quick_mask_attachment() {
    let mut s = session(Platform::Gtk);
    let base = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::New { group: true, clipped: false });
    let group = occurrence_token(s.engine.document().working.occurrence.unwrap());
    layer(&mut s, LayerAction::Blend { id: group, value: layer_core::LayerBlend::PassThrough.code() });
    let control = s.state.layer_tools.attachment.clone();
    assert_eq!(control.label, "Isolate group and attach");
    assert_eq!(control.description, "Clip to Current ink");
    assert_eq!(control.action, Some(LayerAction::IsolateAndAttach { id: group }));
    layer(&mut s, control.action.unwrap());
    assert!(!relationship_row(&s, group).pass_through);
    assert!(relationship_row(&s, group).right_swipe.is_none());
    assert_eq!(relationship_row(&s, group).relationship.unwrap().kind, LayerRelationKind::Clip);
    layer(&mut s, LayerAction::Clip { id: group, value: false });
    layer(&mut s, LayerAction::Blend { id: group, value: layer_core::LayerBlend::PassThrough.code() });
    layer(&mut s, LayerAction::Reparent { id: base, parent: None, index: 0 });
    layer(&mut s, LayerAction::Lock { id: group, value: true });
    layer(&mut s, LayerAction::Select { id: base, mask: false });
    assert_eq!((s.state.layer_tools.attachment.label.as_str(), s.state.layer_tools.attachment.action.as_ref()), ("Isolate group and attach", None));
    invoke(&mut s, CommandId::QuickMask);
    assert!(relationship_row(&s, 0).relationship.is_none());
    assert!(relationship_row(&s, 0).right_swipe.is_none());
    assert!(!relationship_row(&s, 0).visibility_blocked);
    assert!(!relationship_row(&s, 0).adjustment_effect);
    assert_eq!(s.state.layer_tools.attachment.icon, "layer-clip-symbolic");
    assert_eq!(s.state.layer_tools.attachment.label, "No layer below to attach to");
    assert!(s.state.layer_tools.attachment.action.is_none());
}
