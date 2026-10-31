#[test]
fn layer_drawer_frames_resize_retained_pages_and_bridge_together() {
    let from = DrawerPlacement {tab_presentations:Vec::new(),
        bounds: Bounds {x:700., y:56., width:280., height:300.},
        anchor: Bounds {x:800., y:12., width:36., height:36.},
        direction: Edge::Bottom,
        columns: vec![Bounds {x:0., y:0., width:280., height:300.}],
    };
    let to = DrawerPlacement {tab_presentations:Vec::new(),
        bounds: Bounds {height:700., ..from.bounds},
        columns: vec![Bounds {height:700., ..from.columns[0]}],
        ..from.clone()
    };
    for (progress, height) in [(0.,300.), (0.25,400.), (0.5,500.), (1.,700.)] {
        for clip_contents in [false, true] {
            let frame = crate::drawer_frame(&serde_json::json!({"from":from,"to":to,"progress":progress,"clip_contents":clip_contents}).to_string()).unwrap();
            assert_eq!(frame["placement"]["bounds"]["height"], height);
            assert_eq!(frame["placement"]["columns"][0]["height"], if clip_contents {700.} else {height});
            assert_eq!(frame["connection"]["bounds"]["y"], 48.);
            assert_eq!(frame["connection"]["bounds"]["height"], 8.);
        }
    }
    let mid = crate::drawer_frame(&serde_json::json!({"from":from,"to":to,"progress":0.5}).to_string()).unwrap();
    let reversed = crate::drawer_frame(&serde_json::json!({"from":mid["placement"],"to":from,"progress":0.5}).to_string()).unwrap();
    assert_eq!(reversed["placement"]["bounds"]["height"], 400.);
    assert_eq!(reversed["placement"]["columns"][0]["height"], 400.);
    let closing = crate::drawer_frame(&serde_json::json!({"from":from,"to":from,"progress":1.,"closing":true}).to_string()).unwrap();
    assert_eq!(closing["placement"]["bounds"]["height"], 0.);
    assert!(closing["connection"].is_null());
}

#[test]
fn layer_drawer_sizes_each_page_immediately_within_the_viewport() {
    let measurement = crate::PanelScrollMeasurement {fixed_height:100., unit_height:40.};
    let mut sizing = LayerDrawerSizing::default();
    assert!(sizing.update(measurement, 2, 1000., None));
    let minimum = sizing.height();
    assert!(!sizing.update(measurement, 6, 1000., None));
    assert!(sizing.update(measurement, 7, 1000., None));
    assert_eq!(sizing.height(), minimum + 40.);
    assert!(sizing.update(measurement, 6, 1000., None));
    assert_eq!(sizing.height(), minimum);
    assert!(sizing.update(measurement, 12, 1000., None));
    let expanded = sizing.height();
    assert!(expanded > minimum);
    assert!(sizing.update(measurement, 1000, 1000., None));
    assert_eq!(sizing.height(), 850.);
    assert!(!sizing.update(measurement, 1001, 1000., None));
    assert!(sizing.update(measurement, 1000, 1000., Some(120.)));
    assert_eq!(sizing.height(), minimum);
    assert!(sizing.update(measurement, 1000, 1000., Some(600.)));
    assert_eq!(sizing.height(), 649.);
    assert!(sizing.update(measurement, 1000, 320., Some(600.)));
    assert_eq!(sizing.height(), 272.);
    assert!(sizing.update(measurement, 1000, 1000., Some(600.)));
    assert_eq!(sizing.height(), 649.);
    assert!(sizing.update(measurement, 12, 1000., None));
    assert_eq!(sizing.height(), expanded);
    assert!(sizing.update(measurement, 2, 1000., None));
    assert_eq!(sizing.height(), minimum);

    let mut s = session(Platform::Gtk);
    s.state.workspace.layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    let id = s.state.workspace.layout.header.entries().find(|e| e.item == HeaderItem::Tool {control:ToolbarControl::Panel {panel:Panel::Layers}}).unwrap().id;
    s.dispatch(UiAction::MeasureHeader {height:60., items:vec![HeaderItemBounds {id, bounds:Bounds {x:800., y:0., width:40., height:60.}}]}).unwrap();
    customize(&mut s, CustomizationAction::ToggleHeaderDrawer {id});
    let drawer = s.state.customization.drawer.as_ref().unwrap();
    let large = drawer.placement(&s.state.workspace.layout, [1000., 1000.], &[10000.]).unwrap();
    assert_eq!(large.bounds.height, 850.);
    let short = drawer.placement(&s.state.workspace.layout, [1000., 200.], &[10000.]).unwrap();
    assert!(short.bounds.y + short.bounds.height <= 200. - WORKSPACE_SPACING);
    let mut ordinary = drawer.clone(); ordinary.layer_properties = None;
    assert!(ordinary.placement(&s.state.workspace.layout, [1000., 1000.], &[10000.]).unwrap().bounds.height > large.bounds.height);
}

#[test]
fn layer_drawer_bridges_centered_header_buttons() {
    let mut s = session(Platform::Gtk);
    s.state.workspace.layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    let id = s.state.workspace.layout.header.entries().find(|e| e.item == HeaderItem::Tool {control:ToolbarControl::Panel {panel:Panel::Layers}}).unwrap().id;
    for height in [36., 60., 84.] {
        let anchor = Bounds {x:800., y:(height-36.)/2., width:36., height:36.};
        s.dispatch(UiAction::MeasureHeader {height, items:vec![HeaderItemBounds {id, bounds:anchor}]}).unwrap();
        let drawer = crate::ContentDrawer::for_header(&s.state.workspace.layout, id).unwrap();
        let placement = drawer.placement(&s.state.workspace.layout, [1000., 1000.], &[395.]).unwrap();
        let bridge = placement.connection().unwrap();
        assert_eq!(bridge.bounds.y, anchor.y + anchor.height);
        assert_eq!(bridge.bounds.y + bridge.bounds.height, placement.bounds.y);
        assert!(bridge.glass().iter().all(|(bounds, _)| bounds[2] > 0. && bounds[3] > 0.));
    }
}

#[test]
fn layer_drawer_reveals_changed_targets_and_allows_return_from_empty_properties() {
    let mut s = session(Platform::Gtk);
    let epoch = s.state.document_file.epoch;
    let id = s.state.layer_tools.editing_layer.as_ref().unwrap().id;
    let previous = Some((epoch, Some(id)));
    assert!(s.state.layer_tools.drawer_reveal_row(&s.state.layers, epoch, None).is_some());
    assert_eq!(s.state.layer_tools.drawer_reveal_row(&s.state.layers, epoch, previous), None);
    assert!(s.state.layer_tools.drawer_reveal_row(&s.state.layers, epoch + 1, previous).is_some());
    s.dispatch(UiAction::Layer {action:LayerAction::New {group:false, clipped:false}}).unwrap();
    assert!(s.state.layer_tools.drawer_reveal_row(&s.state.layers, epoch, previous).is_some());
    assert_eq!(s.state.layer_tools.drawer_reveal_row(&[], epoch, previous), None);
    let empty = LayerPropertiesView::default();
    assert!(!empty.drawer_toggle_enabled(false));
    assert!(empty.drawer_toggle_enabled(true));
    assert!(s.state.layer_properties.drawer_toggle_enabled(false));
    s.state.workspace.layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    let id = s.state.workspace.layout.header.entries().find(|e| e.item == HeaderItem::Tool {control:ToolbarControl::Panel {panel:Panel::Layers}}).unwrap().id;
    s.dispatch(UiAction::MeasureHeader {height:60., items:vec![HeaderItemBounds {id, bounds:Bounds {x:800., y:12., width:36., height:36.}}]}).unwrap();
    customize(&mut s, CustomizationAction::ToggleHeaderDrawer {id});
    customize(&mut s, CustomizationAction::ToggleLayerDrawerProperties);
    s.dispatch(UiAction::Layer {action:LayerAction::SelectAllLayers {selected:true}}).unwrap();
    s.dispatch(UiAction::Layer {action:LayerAction::DeleteSelected}).unwrap();
    assert!(s.state.layer_properties.layer.is_none());
    assert_eq!(s.state.customization.drawer.as_ref().unwrap().layer_properties, Some(true));
    customize(&mut s, CustomizationAction::ToggleLayerDrawerProperties);
    assert_eq!(s.state.customization.drawer.as_ref().unwrap().layer_properties, Some(false));
    customize(&mut s, CustomizationAction::ToggleLayerDrawerProperties);
    assert_eq!(s.state.customization.drawer.as_ref().unwrap().layer_properties, Some(false));
}

#[test]
fn layer_properties_offer_add_filter_only_on_raster_and_object_layers() {
    let mut s = session(Platform::Gtk);
    assert!(s.state.layer_properties.add_filter.is_some());
    s.dispatch(UiAction::Layer {action:LayerAction::AddMask {id:1, replace:false}}).unwrap();
    s.dispatch(UiAction::Layer {action:LayerAction::Select {id:1, mask:true}}).unwrap();
    assert!(s.state.layer_properties.add_filter.is_none());
    s.dispatch(UiAction::Layer {action:LayerAction::Select {id:1, mask:false}}).unwrap();
    assert!(s.state.layer_properties.add_filter.is_some());
    s.dispatch(UiAction::Effect {action:EffectAction::InsertAttached {effect:"gaussian_blur".into(), owner:1, epoch:s.state.document_file.epoch}}).unwrap();
    assert!(s.state.layer_properties.add_filter.is_none());
    assert!(s.state.layer_tools.add_filter.is_some());
    invoke(&mut s, CommandId::Undo);
    assert!(s.state.layer_properties.add_filter.is_some());
    s.dispatch(UiAction::Layer {action:LayerAction::New {group:true, clipped:false}}).unwrap();
    assert!(s.state.layer_properties.add_filter.is_none());
    let source = layer_core::color::source::rgba8_source([8;2], |_,_| [17,33,65,255]);
    let mut doc = Document::from_artwork(layer_core::authored::Artwork::new([64;2]).unwrap()).unwrap();
    let (owner, edit) = doc.create_object_layer_edit("Photo", layer_core::authored::ImageObject::new(source.into()), None, 0).unwrap();
    doc.apply(edit).unwrap();
    let mut s = UiSession::new(Recorder {tiled_sources:true, ..Default::default()}, doc, [64;2], Platform::Gtk).unwrap();
    s.dispatch(UiAction::SelectLayer {id:occurrence_token(owner)}).unwrap();
    assert!(s.state.layer_properties.add_filter.is_some());
}

#[test]
fn sketch_layer_drawer_properties_scope_and_dismissal() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        let mut s = session(platform);
        s.state.workspace.layout = WorkspacePreset::Painter.layout(platform);
        let id = s.state.workspace.layout.header.entries().find(|e| e.item == HeaderItem::Tool {control:ToolbarControl::Panel {panel:Panel::Layers}}).unwrap().id;
        s.dispatch(UiAction::MeasureHeader {height:60.,items:vec![HeaderItemBounds {id,bounds:Bounds {x:800.,y:0.,width:40.,height:60.}}]}).unwrap();
        customize(&mut s, CustomizationAction::ToggleHeaderDrawer {id});
        assert_eq!(s.state.customization.drawer.as_ref().unwrap().layer_properties, Some(false));
        let layout = s.state.workspace.clone();
        let history = s.capture_workspace();
        customize(&mut s, CustomizationAction::ToggleLayerDrawerProperties);
        assert_eq!(s.state.customization.drawer.as_ref().unwrap().layer_properties, Some(true));
        assert!(s.panel_is_presented(Panel::Properties));
        assert!(!s.panel_is_presented(Panel::Layers));
        let reply = key(&mut s, "Escape", true, false, false);
        assert!(reply.handled);
        assert_eq!(s.state.customization.drawer.as_ref().unwrap().layer_properties, Some(false));
        assert!(s.panel_is_presented(Panel::Layers));
        assert!(!s.panel_is_presented(Panel::Properties));
        assert_eq!(s.state.workspace, layout);
        assert_eq!(s.capture_workspace(), history);
        s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"gaussian_blur".into()}}).unwrap();
        assert_eq!(s.state.customization.drawer.as_ref().unwrap().layer_properties, Some(true));
        assert_eq!(s.state.workspace, layout);
        customize(&mut s, CustomizationAction::ToggleLayerDrawerProperties);
        key(&mut s, "Escape", false, false, false);
        key(&mut s, "Escape", true, false, false);
        assert!(s.state.customization.drawer.is_none());
    }
    for preset in [WorkspacePreset::Illustrator, WorkspacePreset::Photographer] {
        let mut s = session(Platform::Gtk);
        s.state.workspace.layout = preset.layout(Platform::Gtk);
        s.state.workspace.layout.header = HeaderLayout::painter_for_platform(Platform::Gtk);
        let id = s.state.workspace.layout.header.entries().find(|e| e.item == HeaderItem::Tool {control:ToolbarControl::Panel {panel:Panel::Layers}}).unwrap().id;
        s.dispatch(UiAction::MeasureHeader {height:60.,items:vec![HeaderItemBounds {id,bounds:Bounds {x:800.,y:0.,width:40.,height:60.}}]}).unwrap();
        customize(&mut s, CustomizationAction::ToggleHeaderDrawer {id});
        assert_eq!(s.state.customization.drawer.as_ref().unwrap().layer_properties, Some(false));
    }
}

#[test]
fn layer_drawer_disclosure_uses_types_and_nonredundant_authored_values() {
    let mut s = session(Platform::Gtk);
    assert_eq!(s.state.layer_properties.disclosure.label, "Layer properties");
    assert_eq!(s.state.layer_properties.disclosure.return_label, "All layers");
    assert!(s.state.layer_properties.disclosure.summary.is_empty());
    s.dispatch(UiAction::Layer {action:LayerAction::ColorMode {id:1, epoch:s.state.document_file.epoch, mode:layer_core::color::LayerColorMode::Grayscale}}).unwrap();
    assert!(s.state.layers.iter().find(|l| l.id == 1).unwrap().description.contains("Grayscale"));
    assert!(s.state.layer_properties.disclosure.summary.is_empty());
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"gaussian_blur".into()}}).unwrap();
    let id = s.state.layer_properties.layer.unwrap();
    assert_eq!(s.state.layer_properties.disclosure.label, "Gaussian Blur");
    assert!(s.state.layer_properties.disclosure.summary.contains("Radius"));
    s.dispatch(UiAction::Layer {action:LayerAction::Rename {id,name:"Soft edges".into()}}).unwrap();
    s.dispatch(UiAction::Effect {action:EffectAction::Set {layer:id,key:"sigma".into(),value:layer_core::EffectValue::Number(8.)}}).unwrap();
    assert_eq!(s.state.layer_properties.disclosure.label, "Gaussian Blur");
    assert!(s.state.layer_properties.disclosure.summary.contains("8 px"));
    assert!(!s.state.layer_properties.disclosure.summary.contains("Gaussian Blur"));
    s.dispatch(UiAction::Invoke {command:CommandId::Undo}).unwrap();
    assert!(!s.state.layer_properties.disclosure.summary.contains("8 px"));
    s.set_localization(Localizer::shared(UiLanguage::French));
    assert_eq!(s.state.layer_properties.disclosure.label, s.localization().text(MessageId::RESOURCES_FILTER_GAUSSIAN_BLUR).as_ref());
    assert_eq!(s.state.layer_properties.name, "Soft edges");
    assert!(!s.state.layer_properties.disclosure.summary.contains("Radius"));
    s.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"curves".into()}}).unwrap();
    assert!(s.state.layer_properties.disclosure.summary.is_empty());
    s.dispatch(UiAction::Layer {action:LayerAction::New {group:true, clipped:false}}).unwrap();
    assert_eq!(s.state.layer_properties.disclosure.label, s.localization().text(MessageId::RESOURCES_LAYER_GROUP_PROPERTIES).as_ref());
    assert_eq!(s.state.layer_properties.disclosure.return_label, s.localization().text(MessageId::RESOURCES_LAYER_ALL_LAYERS).as_ref());
    s.set_localization(Localizer::shared(UiLanguage::English));
    assert_eq!(s.state.layer_properties.disclosure.label, "Group properties");
    assert_eq!(s.state.layer_properties.disclosure.return_label, "All layers");
}

#[test]
fn layer_pickup_requires_a_resting_row_and_a_handle_for_direct_contacts() {
    let s = session(Platform::Gtk);
    let mut row = s.state.layers.iter().find(|row| row.id == 1).unwrap().clone();
    for kind in [crate::PointerKind::Mouse, crate::PointerKind::Touch, crate::PointerKind::Pen] {
        assert!(row.can_drag(kind, true, true));
        assert_eq!(row.can_drag(kind, false, true), kind == crate::PointerKind::Mouse);
        assert!(!row.can_drag(kind, false, false));
        assert!(!row.can_drag(kind, true, false));
    }
    row.drag_from_handle = false;
    row.drag_from_body = &[];
    assert!(!row.can_drag(crate::PointerKind::Mouse, true, true));
    assert!(!row.can_drag(crate::PointerKind::Touch, true, true));
    assert!(!row.can_drag(crate::PointerKind::Pen, true, true));
}

#[test]
fn serialized_layer_body_drag_devices_match_shared_pickup_policy() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        let verify=|s:&UiSession<Recorder>|{for row in &s.state.layers {
            let serialized = serde_json::to_value(row).unwrap();
            let devices = serialized["drag_from_body"].as_array().unwrap();
            let handle = serialized["drag_from_handle"].as_bool().unwrap();
            assert_eq!(handle, row.can_drop_below&&!row.locked);
            assert_eq!(devices, &if handle { vec![serde_json::json!("mouse")] } else { vec![] });
            for kind in [crate::PointerKind::Mouse, crate::PointerKind::Touch, crate::PointerKind::Pen] {
                assert_eq!(devices.contains(&serde_json::to_value(kind).unwrap()), row.can_drag(kind, false, true));
                assert_eq!(row.can_drag(kind,true,true),handle);
            }
        }};
        verify(&s);
        layer(&mut s,LayerAction::New {group:false,clipped:false});
        let target=s.state.layer_properties.layer.unwrap();
        layer(&mut s,LayerAction::Lock {id:1,value:true});verify(&s);
        assert!(s.state.layers.iter().find(|row|row.id==1).unwrap().can_drop_below);
        assert!(s.layer_drop_preview(1,target,0.,LayerDropSurface::Row).is_none());
        layer(&mut s,LayerAction::Lock {id:1,value:false});verify(&s);
        assert!(s.layer_drop_preview(1,target,0.,LayerDropSurface::Row).is_some());
        layer(&mut s,LayerAction::New {group:true,clipped:false});let group=s.state.layer_properties.layer.unwrap();
        layer(&mut s,LayerAction::New {group:false,clipped:false});let child=s.state.layer_properties.layer.unwrap();
        layer(&mut s,LayerAction::Lock {id:group,value:true});verify(&s);
        assert!(s.state.layers.iter().find(|row|row.id==child).unwrap().locked);
        assert!(s.layer_drop_preview(child,1,0.,LayerDropSurface::Row).is_none());
        layer(&mut s,LayerAction::Lock {id:group,value:false});verify(&s);
        assert!(s.layer_drop_preview(child,1,0.,LayerDropSurface::Row).is_some());
    }
}

#[test]
fn layer_swipe_actions_follow_parent_locks_and_undo() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        let row = |s: &UiSession<Recorder>, id| s.state.layers.iter().find(|row| row.id == id).unwrap().clone();
        let enabled = |s: &UiSession<Recorder>, id, key| row(s, id).swipe_actions.iter().find(|action|action.id == key).is_some_and(|action|action.enabled);
        assert_eq!(row(&s, 1).swipe_actions.iter().map(|action|action.id).collect::<Vec<_>>(), ["lock", "duplicate", "delete"]);
        assert!(enabled(&s, 1, "lock") && enabled(&s, 1, "duplicate"));
        layer(&mut s, LayerAction::Lock { id:1, value:true });
        assert!(enabled(&s, 1, "lock") && enabled(&s, 1, "duplicate"));
        assert_eq!(row(&s, 1).swipe_actions[0].label.as_ref(), "Unlock");
        assert!(!row(&s, 1).can_delete);
        layer(&mut s, LayerAction::Duplicate { id:1 });
        let copy = s.state.layer_properties.layer.unwrap();
        assert_ne!(copy, 1);
        assert!(row(&s, copy).locked);
        invoke(&mut s, CommandId::Undo);
        assert!(!s.state.layers.iter().any(|row| row.id == copy));
        invoke(&mut s, CommandId::Undo);
        layer(&mut s, LayerAction::New { group:true, clipped:false });
        let group = s.state.layer_properties.layer.unwrap();
        layer(&mut s, LayerAction::New { group:false, clipped:false });
        let child = s.state.layer_properties.layer.unwrap();
        layer(&mut s, LayerAction::Lock { id:group, value:true });
        assert!(!enabled(&s, child, "lock") && !enabled(&s, child, "duplicate"));
        assert!(enabled(&s, group, "lock") && enabled(&s, group, "duplicate"));
        assert!(s.dispatch(UiAction::Layer { action:LayerAction::Lock { id:child, value:false } }).is_err());
        assert!(s.dispatch(UiAction::Layer { action:LayerAction::Duplicate { id:child } }).is_err());
        invoke(&mut s, CommandId::Undo);
        assert!(enabled(&s, child, "lock") && enabled(&s, child, "duplicate"));
        invoke(&mut s, CommandId::QuickMask);
        assert_eq!(row(&s, 0).swipe_actions.iter().map(|action|action.id).collect::<Vec<_>>(), ["delete"]);
        assert!(enabled(&s, 0, "delete"));
    }
}

#[test]
fn combined_drawers_use_the_same_controls_on_any_toolbar() {
    for platform in Platform::ALL {
        for preset in [WorkspacePreset::Painter, WorkspacePreset::Illustrator, WorkspacePreset::Photographer] {
            let mut layout = preset.layout(platform);
            let controls = [ToolbarControl::Panel {panel:Panel::Layers}, ToolbarControl::Color, ToolbarControl::Panel {panel:Panel::Color}];
            let panel = layout.add_toolbar(None, "Combined panels", &controls).unwrap();
            let tiles = layout.panel(panel).unwrap().tiles().to_vec();
            for tile in tiles {
                layout.header.add(HeaderZone::Left, None, &[HeaderItem::Tool {control:tile.control}]).unwrap();
                let id = layout.header.entries().filter(|entry|entry.item == HeaderItem::Tool {control:tile.control}).last().unwrap().id;
                let header = ContentDrawer::for_header(&layout, id).unwrap();
                let toolbar = ContentDrawer::for_tile(&layout, TileAnchor {panel, tile:tile.id}).unwrap();
                assert_eq!(header.columns, toolbar.columns);
                assert_eq!(header.layer_properties, toolbar.layer_properties);
                if tile.control == controls[0] {
                    assert_eq!(toolbar.layer_properties, Some(false));
                    assert_eq!(toolbar.columns, [vec![Panel::Layers]]);
                } else {
                    assert_eq!(toolbar.columns, [vec![Panel::Color, Panel::Palettes]]);
                    assert_eq!(toolbar.layer_properties, None);
                }
            }
        }
    }
}
