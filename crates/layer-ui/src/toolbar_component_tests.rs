fn check_choice_items(s: &mut UiSession<Recorder>, id: &str) {
    let choice = |state: &UiState| {
        state.tool_options().into_iter().find(|o|
            matches!(o, ToolOption::Choice { id: key, .. } if *key == id)).unwrap()
    };
    let original = choice(s.state());
    let ToolOption::Choice { items, .. } = &original else { panic!() };
    assert_eq!(items.iter().filter(|i| i.selected).count(), 1);
    for item in items {
        s.dispatch(UiAction::ToolbarEdit {
            context: s.state().toolbar_context(),
            action: Box::new(item.action.clone()),
        }).unwrap();
        let current = choice(s.state());
        assert!(original.same_schema(&current));
        let ToolOption::Choice { items: selected, .. } = current else { panic!() };
        assert_eq!(selected.iter().filter(|i| i.selected).count(), 1);
        assert!(selected.iter().any(|i| i.selected && i.action == item.action),
            "published {id} choices are accepted toolbar edits");
    }
}

#[test]
fn language_refresh_retains_toolbar_editor_schema_and_action_context() {
    let mut s = session(Platform::Gtk);
    for command in [CommandId::Pen, CommandId::Pencil, CommandId::Brush, CommandId::Blend, CommandId::Fill, CommandId::RectangleSelect, CommandId::Zoom] {
        s.dispatch(UiAction::Invoke { command }).unwrap();
        let context = s.state().toolbar_context();
        let initial = s.state().tool_options();
        for language in UiLanguage::ALL {
            s.set_localization(Localizer::shared(language));
            if command == CommandId::Zoom {
                let help = s.state().localization.text(MessageId::ZOOM_TOOL_CENTER_ON_CLICK_TOOLTIP);
                assert!(!help.is_empty());
                for keymap in ["capy", "photoshop"] {
                    crate::keymaps::select(&mut s.state.settings, keymap).unwrap();
                    s.refresh_shortcuts(true);
                    let center = s.state().commands.iter().find(|entry|entry.id == CommandId::CenterZoomClicks).unwrap();
                    assert_eq!(center.label, s.state().localization.text(MessageId::COMMAND_CENTER_ZOOM_CLICKS));
                    assert!(center.tooltip.starts_with(help.as_ref()));
                    assert_ne!(center.label.as_ref(), help.as_ref());
                    if language == UiLanguage::English {
                        assert_eq!(center.label.as_ref(), "Center on click");
                        assert_eq!(help.as_ref(), "Move the clicked point to the center of the view when you click to zoom in or out.");
                    }
                }
            }
            let updated = s.state().tool_options();
            assert_eq!(initial.len(), updated.len());
            assert!(initial.iter().zip(&updated).all(|(before, after)| before.same_schema(after)));
            assert_eq!(context, s.state().toolbar_context());
            if let Some(ToolOption::Numeric(field)) = updated.iter().find(|option| matches!(option, ToolOption::Numeric(_))) {
                let mut different = field.clone();
                different.id = "different-control";
                assert!(!ToolOption::Numeric(field.clone()).same_schema(&ToolOption::Numeric(different)));
            }
        }
    }
}

#[test]
fn toolbar_resets_use_tool_defaults_and_reject_stale_context() {
    let mut s = session(Platform::Gtk);
    let original = s.state().brush.diameter;
    let context = s.state().toolbar_context();
    s.dispatch(UiAction::SetToolSetting { id: "size".into(), value: 517. }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "opacity".into(), value: 0.42 }).unwrap();
    s.dispatch(UiAction::ToolbarEdit {
        context,
        action: Box::new(UiAction::ResetToolSetting { id: "size".into() }),
    }).unwrap();
    assert_eq!(s.state().brush.diameter, original);
    assert_eq!(s.state().brush.opacity, 0.42);
    assert!(!s.tools.overrides.get(&context.brush).unwrap().contains_key("size"));
    s.dispatch(UiAction::Invoke { command: CommandId::Fill }).unwrap();
    assert!(s.dispatch(UiAction::ToolbarEdit {
        context,
        action: Box::new(UiAction::ResetToolSetting { id: "opacity".into() }),
    }).is_err());
    let default = s.state().tool_settings.iter().find(|f| f.id == "tolerance").unwrap().value;
    s.dispatch(UiAction::SetToolSetting { id: "tolerance".into(), value: 0.72 }).unwrap();
    s.dispatch(UiAction::ResetToolSetting { id: "tolerance".into() }).unwrap();
    assert_eq!(s.state().tool_settings.iter().find(|f| f.id == "tolerance").unwrap().value, default);
    assert!(s.dispatch(UiAction::ResetToolSetting { id: "size".into() }).is_err());
    let opacity = ToolbarNumericBinding::BrushStrength.numeric();
    let value = opacity.resolve(0., NumericOperation::Position { position: 0.427 }).unwrap();
    assert!((value.value - 0.43).abs() < 1e-6);
    assert_eq!(value.text, "43 %");
}

#[test]
fn toolbar_components_edit_shared_parameters_and_reject_obsolete_contexts() {
    let mut s = session(Platform::Gtk);
    let context = s.state().toolbar_context();
    let history = s.capture_workspace().unwrap().history;
    for value in [0.5, 27.3, 2048.0] {
        s.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(UiAction::SetBrushSize { value }),
        })
        .unwrap();
        assert_eq!(s.state().brush.diameter, value);
        assert_eq!(
            s.state()
                .tool_settings
                .iter()
                .find(|f| f.id == "size")
                .unwrap()
                .value,
            value
        );
        assert_eq!(
            context,
            s.state().toolbar_context(),
            "value updates retain editors"
        );
    }
    s.dispatch(UiAction::ToolbarEdit {
        context,
        action: Box::new(UiAction::SetBrushOpacity { value: 0.42 }),
    })
    .unwrap();
    assert_eq!(s.state().brush.opacity, 0.42);
    assert_eq!(
        history,
        s.capture_workspace().unwrap().history,
        "numeric edits are not layout changes"
    );
    assert!(
        s.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(UiAction::SetBrushSize { value: f32::NAN })
        })
        .is_err()
    );
    invoke(&mut s, CommandId::Eraser);
    assert!(
        s.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(UiAction::SetBrushSize { value: 17.0 })
        })
        .is_err()
    );
    s.dispatch(UiAction::SelectBrush { id: context.brush })
        .unwrap();
    assert_ne!(
        context,
        s.state().toolbar_context(),
        "returning to the same brush does not revive a stale edit"
    );
    assert!(
        s.dispatch(UiAction::ToolbarEdit {
            context: s.state().toolbar_context(),
            action: Box::new(UiAction::Invoke {
                command: CommandId::Undo
            })
        })
        .is_err()
    );
    let context = s.state().toolbar_context();
    let mut next = session(Platform::Gtk);
    next.inherit_window_state(&s).unwrap();
    assert!(
        next.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(UiAction::SetBrushSize { value: 17. })
        })
        .is_err(),
        "replacing a document invalidates otherwise identical targets"
    );
    use layer_core::DefaultBrushPreset as P;
    let binding = ToolbarNumericBinding::BrushStrength;
    for (preset, id) in [(P::Pencil,"flow"),(P::Paintbrush,"flow"),(P::Eraser,"flow"),
        (P::Smudge,"flow"),(P::CloneStamp,"flow"),(P::HealingBrush,"flow"),
        (P::SpotHealingBrush,"flow"),(P::LiquifyPush,"strength")] {
        s.dispatch(UiAction::SelectBrush { id:preset as u32 }).unwrap();
        let field=binding.field(s.state()).unwrap();
        assert_eq!((field.id,field.label.as_ref()),(id,"Strength"),"{preset:?}");
        let context=s.state().toolbar_context();
        let opacity=s.engine.configured_brush().opacity;
        for value in [0.37,0.] {
            s.dispatch(UiAction::ToolbarEdit {context,action:Box::new(binding.action(value))}).unwrap();
            assert_eq!(binding.field(s.state()).unwrap().value,value,"{preset:?}");
            let brush=s.engine.configured_brush();
            assert_eq!(if id=="strength" {brush.deform.strength} else {brush.flow},value,"{preset:?}");
            assert_eq!(brush.opacity,opacity,"{preset:?}: Strength preserves Opacity");
            assert_eq!(context,s.state().toolbar_context());
        }
        for value in [f32::NAN,-0.01,1.01] {
            assert!(s.dispatch(UiAction::ToolbarEdit {context,action:Box::new(binding.action(value))}).is_err());
            assert_eq!(binding.field(s.state()).unwrap().value,0.);
        }
    }
    let context=s.state().toolbar_context();
    invoke(&mut s,CommandId::Fill);
    assert!(binding.field(s.state()).is_none());
    for context in [context,s.state().toolbar_context()] {
        assert!(s.dispatch(UiAction::ToolbarEdit {context,action:Box::new(binding.action(0.4))}).is_err());
    }
}

#[test]
fn toolbar_options_follow_tools_and_preserve_completion_actions() {
    let mut s = session(Platform::Gtk);
    let check_fields = |state: &crate::UiState, tool: CommandId| {
        let options = state.tool_options();
        assert_eq!(options.iter().filter_map(|o| if let ToolOption::Numeric(f) = o { Some(f.id) } else { None })
            .collect::<Vec<_>>(), state.tool_settings.iter().map(|f| f.id).collect::<Vec<_>>(), "{tool:?}: shared field order");
        for field in &state.tool_settings {
            assert_eq!(field.group_id.is_empty(), field.group.is_empty(), "{tool:?}/{}: heading identity", field.id);
            if matches!(field.id, "opacity" | "selection_feather") && !matches!(tool, CommandId::Brush | CommandId::Eraser) {
                assert!(field.group_id.is_empty(), "{tool:?}/{}: standalone setting", field.id);
            }
            assert!(
                options
                    .iter()
                    .any(|o| matches!(o, ToolOption::Numeric(f) if f == field))
            );
        }
    };
    for tool in [CommandId::Brush, CommandId::Eraser, CommandId::Fill,
        CommandId::RectangleSelect, CommandId::Gradient, CommandId::Eyedropper] {
        s.dispatch(UiAction::Invoke { command: tool }).unwrap();
        check_fields(s.state(), tool);
        let options = s.state().tool_options();
        for action in &s.state().tool_actions {
            assert!(options.iter().any(|o| match o {
                ToolOption::Action { state, .. } => state.id == action.command,
                ToolOption::Choice { items, .. } => items.iter().any(|i| i.action
                    == UiAction::Invoke {
                        command: action.command
                    }),
                _ => false,
            }));
        }
    }
    for tool in [CommandId::EncloseFill, CommandId::EllipseSelect, CommandId::PolygonSelect,
        CommandId::Figure, CommandId::LassoFill, CommandId::Crop] {
        invoke(&mut s, tool);
        check_fields(s.state(), tool);
        if tool == CommandId::Crop { invoke(&mut s, CommandId::CancelTransform); }
    }
    invoke(&mut s, CommandId::Brush);
    use layer_core::DefaultBrushPreset as P;
    for (preset, primary) in [
        (P::Pencil, vec!["size", "flow", "grain_depth", "material_hardness"]),
        (P::GPen, vec!["size", "flow", "hardness"]),
        (P::BristlePaintbrush, vec!["size", "flow", "bristle_load", "bristle_scale"]),
        (P::WetRound, vec!["size", "flow", "paint", "pull"]),
        (P::WatercolorWash, vec!["size", "flow", "water_load", "wet_edge"]),
        (P::Smudge, vec!["size", "flow", "pull"]),
        (P::LiquifyPush, vec!["size", "strength"]),
    ] {
        s.dispatch(UiAction::SelectBrush {id:preset as u32}).unwrap();
        let fields=s.state().tool_settings.clone();
        assert_eq!(fields.iter().take(primary.len()).map(|f|f.id).collect::<Vec<_>>(),primary);
        assert!(fields[..primary.len()].iter().all(|f|f.group_id.is_empty()));
        let options=s.state().tool_options();
        assert_eq!(options.iter().filter_map(|o|if let ToolOption::Numeric(f)=o {Some(f.id)} else {None})
            .collect::<Vec<_>>(),fields.iter().map(|f|f.id).collect::<Vec<_>>());
        for id in primary.iter().copied().filter(|id|*id!="size") {
            let value=fields.iter().find(|f|f.id==id).unwrap().numeric.min as f32;
            s.dispatch(UiAction::SetToolSetting {id:id.into(),value}).unwrap();
        }
        if preset==P::WetRound {
            s.dispatch(UiAction::SetToolSetting {id:"dilution".into(),value:0.}).unwrap();
            let options=s.state().tool_options();
            let mixing=options.iter().position(|o|matches!(o,ToolOption::Choice {id:"color-mixing",..})).unwrap();
            let last_primary=options.iter().position(|o|matches!(o,ToolOption::Numeric(f) if f.id=="pull")).unwrap();
            let opacity=options.iter().position(|o|matches!(o,ToolOption::Numeric(f) if f.id=="opacity")).unwrap();
            assert!(last_primary<mixing && mixing<opacity);
        }
        assert!(fields.iter().zip(&s.state().tool_settings).all(|(a,b)|a.id==b.id && a.group_id==b.group_id));
    }
    s.dispatch(UiAction::SelectBrush {id:P::Paintbrush as u32}).unwrap();
    let selection = rectangle([100., 100., 300., 300.]);
    s.fill_selection(selection).unwrap();
    s.frame(1, 1).unwrap();
    invoke(&mut s, CommandId::ScaleRotate);
    assert!(s.state().toolbar_context().operation);
    assert!(
        matches!(s.state().tool_options()[0], ToolOption::Action { ref state, .. } if state.id == CommandId::ApplyTransform)
    );
    for (i, option) in s.state().tool_options().iter().enumerate() {
        if matches!(option, ToolOption::Action { state, .. } if matches!(state.id, CommandId::ApplyTransform | CommandId::CancelTransform))
        {
            assert!(i < 2);
        }
    }
}

#[test]
fn toolbar_options_keep_fitting_rows_before_tall_fields_overflow() {
    for (width, height, sizes) in [
        (1308., 36., [[36., 36.], [36., 36.], [100., 54.], [36., 36.]]),
        (100., 90., [[80., 36.], [36., 36.], [36., 54.], [36., 36.]]),
    ] {
        let layout = tool_options_layout(width, height, Axis::Horizontal, &sizes, [36., 36.], 10.);
        for field in &layout.fields[..2] {
            let bounds = field.expect("fitting prefix remains visible");
            assert_eq!(bounds.height, 36.);
            assert!(bounds.y + bounds.height <= height);
            assert!(bounds.intersection(layout.more).is_none());
        }
        assert!(layout.fields[2..].iter().all(Option::is_none));
    }
}

#[test]
fn toolbar_component_defaults_round_trip_on_supported_hosts() {
    for (preset, platform) in WorkspacePreset::ALL.into_iter().flat_map(|p| Platform::ALL.map(|platform| (p, platform))) {
        let layout = preset.layout(platform);
        layout.validate().unwrap();
        let loaded: DockLayout =
            serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
        assert_eq!(layout, loaded);
        if preset == WorkspacePreset::Painter {
            let panel = layout
                .panels
                .iter()
                .find(|p| {
                    p.tiles()
                        .iter()
                        .any(|t| t.control == ToolbarControl::BrushSizeSlider)
                })
                .unwrap()
                .id;
            assert!(matches!(panel, Panel::CustomToolbar(_)));
            assert_eq!(
                layout.group_edge(layout.panel_group(panel).unwrap()),
                Some(Edge::Left)
            );
            assert_eq!(
                layout
                    .panel(panel)
                    .unwrap()
                    .tiles()
                    .iter()
                    .map(|t| t.control)
                    .collect::<Vec<_>>(),
                [ToolbarControl::BrushSizeSlider,
                    ToolbarControl::ColorPicker, ToolbarControl::BrushStrengthSlider,
                    ToolbarControl::Command { command: CommandId::Undo }, ToolbarControl::Command { command: CommandId::Redo }]
            );
        }
        if preset == WorkspacePreset::Photographer {
            assert_eq!(
                layout
                    .panel(Panel::Commands)
                    .unwrap()
                    .tiles()
                    .last()
                    .unwrap()
                    .control,
                ToolbarControl::TOOL_OPTIONS
            );
        }
    }
    let mut header = HeaderLayout::default();
    assert!(
        header
            .add(
                HeaderZone::Left,
                None,
                &[HeaderItem::Tool {
                    control: ToolbarControl::TOOL_OPTIONS
                }]
            )
            .is_err()
    );
}

#[test]
fn toolbar_dry_settings_preserve_writing_size_and_default_pose_footprints() {
    use layer_core::DefaultBrushPreset as P;
    let mut s=session(Platform::Gtk);
    for (preset,diameter,poses,block,loading,gain) in [
        (P::Pencil,43.2f64,[[0.65f64,0.65],[1.,0.65],[4.8,1.2]],false,0.70,0.7),
        (P::PointyPencil,100./3.,[[0.55,0.55],[0.9,0.5],[5.,0.8]],false,0.75,0.7),
        (P::ShadingPencil,64.,[[0.28,0.28],[1.,0.55],[2.6,0.8]],false,0.65,0.63),
        (P::Charcoal,48.,[[0.65,0.65],[1.,0.65],[2.,0.8]],false,0.55,0.85),
        (P::Chalk,36.,[[0.55,0.55],[1.,0.8],[1.6,0.85]],true,0.15,0.75),
        (P::PastelBlock,64.,[[0.55,0.55],[1.,0.65],[3.2,0.55]],true,0.50,0.85),
        (P::Crayon,32.,[[0.65,0.65],[1.,0.7],[2.6,0.85]],false,0.90,0.7),
    ] {
        s.dispatch(UiAction::SelectBrush {id:preset as u32}).unwrap();
        let fields=&s.state().tool_settings;
        let ids=fields.iter().map(|f|f.id).collect::<Vec<_>>();
        assert_eq!(ids,["size","flow","grain_depth","material_hardness","smoothing_strength","angle","opacity"],"{preset:?}");
        assert!(fields[..5].iter().all(|f|f.group_id.is_empty() && f.group.is_empty()));
        assert_eq!(fields[4].label.as_ref(),"Smoothing");
        assert_eq!(fields[5].group_id,MessageId::TOOL_SETTING_GROUP_TIP.key());
        assert_eq!(fields[6].group_id,MessageId::TOOL_SETTING_GROUP_BLENDING.key());
        assert_eq!(fields.iter().find(|f|f.id=="angle").unwrap().label.as_ref(),"Tip angle");
        assert_eq!(fields.iter().find(|f|f.id=="grain_depth").unwrap().numeric.max,2.);
        let material=fields.iter().find(|f|f.id=="material_hardness").unwrap();
        assert_eq!(material.label.as_ref(),"Material hardness");
        assert_eq!((material.numeric.min,material.numeric.max,material.numeric.scale),(0.,1.,100.));
        assert!(material.numeric.endpoint_labels.is_none());
        let brush=s.engine.configured_brush();let dry=brush.contact.unwrap().dry.unwrap();
        assert_eq!((brush.flow,dry.loading,brush.contact.unwrap().pressure_gain),(1.,loading,gain),"{preset:?}: authored defaults");
        assert_eq!(material.value,loading);
        assert_eq!(dry.softness,0.05);
        assert_eq!(dry.poses[1].radii[0].max(dry.poses[1].radii[1]),1.,"{preset:?}: 40° long axis");
        let extent=if preset==P::PastelBlock {1.95} else {poses.iter().map(|p|if block {p[0].hypot(p[1])} else {p[0].max(p[1])}).fold(0f64,f64::max)};
        for (pose,expected) in dry.poses.iter().zip(poses) {for (axis,&expected_radius) in expected.iter().enumerate() {
            assert!((f64::from(brush.diameter*pose.radii[axis])-diameter*expected_radius/extent).abs()<2e-5,
                "{preset:?}: full-pressure footprint axis{axis}");
        }}
        let mut support_brush=brush.clone();support_brush.diameter=24.;
        assert_eq!(support_brush.angle_radians,0.);
        assert_eq!(support_brush.shape,layer_core::BrushShape::default());
        assert_eq!(support_brush.path,layer_core::BrushPath::default());
        for tilt in [0f32,40.,60.] {for pressures in [[0.;4],[0.15;4],[0.45;4],[1.;4],[0.,0.15,0.45,1.]] {
            let points:Vec<_>=(0..5).map(|i|layer_core::StrokePoint {
                position:layer_core::Point{x:24.+i.min(3) as f32*80./3.,y:64.},pressure:pressures[i.min(3)],
                tilt:[tilt.to_radians(),0.],twist:0.,elapsed_micros:i as u32*4000,
            }).collect();
            let stroke=layer_core::Stroke::new(layer_core::StrokeId(1),s.engine.document().working.target.unwrap(),
                layer_core::StrokeTool::Brush,support_brush.clone(),points).unwrap();
            let mut dabs=Vec::new();layer_engine::DabGenerator::generate(&stroke,s.engine.document().composition().color.space,&mut dabs);
            assert!(!dabs.is_empty());
            for dab in dabs {
                assert_eq!(dab.rotation,[1.,0.]);assert_eq!(&dab.previous[2..],&[1.,0.]);assert_eq!(dab.motion[1],0.);
                for previous in [false,true] {
                    let mut endpoint=dab;
                    if previous {endpoint.center.x-=dab.motion[0];endpoint.center.y-=dab.motion[1];endpoint.radii=[dab.previous[0],dab.previous[1]];}
                    endpoint.previous=[0.;4];
                    let bounds=endpoint.bounds();
                    assert!(bounds.min.y>=40. && bounds.max.y<=88.,"{preset:?} tilt={tilt} pressures={pressures:?}: {bounds:?}");
                }
            }
        }}
        for (id,value) in [("material_hardness",0.65),("grain_depth",2.)] {
            s.dispatch(UiAction::SetToolSetting {id:id.into(),value}).unwrap();
        }
        let brush=s.engine.configured_brush();let edited=brush.contact.unwrap().dry.unwrap();
        assert_eq!((edited.softness,edited.loading),(0.05,0.65));
        assert_eq!(edited.poses,dry.poses);assert_eq!(brush.grain.as_ref().unwrap().depth,2.);
        let before=brush.clone();
        for (id,value) in [("grain_depth",2.01),("contact_softness",0.12),
            ("material_hardness",1.01),("spacing",0.2),("size_jitter",0.2),("rotation_jitter",0.2),("hardness",0.2)] {
            assert!(s.dispatch(UiAction::SetToolSetting {id:id.into(),value}).is_err(),"{preset:?}/{id}");
            assert_eq!(s.engine.configured_brush(),&before);
        }
    }
    s.dispatch(UiAction::SelectBrush {id:P::GPen as u32}).unwrap();
    for id in ["spacing","size_jitter","rotation_jitter","hardness"] {
        assert!(s.state().tool_settings.iter().any(|f|f.id==id),"legacy contact {id}");
    }
}

#[test]
fn toolbar_components_have_atomic_bounds_and_fill_remaining_width() {
    let tiles: Vec<_> = [
        ToolbarControl::Color,
        ToolbarControl::BrushSizeSlider,
        ToolbarControl::Divider,
        ToolbarControl::BrushStrengthSlider,
        ToolbarControl::TOOL_OPTIONS,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, control)| ToolbarTile { slider_style: Default::default(),
        id: i as u32 + 1,
        control,
    })
    .collect();
    for style in [
        TileStyle::Small,
        TileStyle::Medium,
        TileStyle::Large,
        TileStyle::MediumLabeled,
        TileStyle::Labeled,
    ] {
        for axis in [Axis::Horizontal, Axis::Vertical] {
            for (width, height) in [(32., 32.), (180., 300.), (900., 600.), (1600., 80.)] {
                let layout = toolbar_tile_layout(width, height, axis, &tiles, true, style);
                assert_eq!(layout.tiles.len(), tiles.len());
                assert_eq!(layout.insertion.len(), tiles.len() + 1);
                for b in &layout.tiles {
                    assert!(b.width > 0. && b.height >= 0. && b.x.is_finite() && b.y.is_finite());
                    if axis == Axis::Vertical { assert!(b.y + b.height <= height); }
                    for other in &layout.tiles {
                        if b != other {
                            assert!(b.intersection(*other).is_none());
                        }
                    }
                }
            }
        }
    }
    let layout = toolbar_tile_layout(1200., 36., Axis::Horizontal, &tiles, true, TileStyle::Small);
    let last = layout.tiles.last().unwrap();
    assert!(last.width > 500.);
    assert!((last.x + last.width + 2. - layout.grip.unwrap().x).abs() < 0.1);
    let ToolOptionsLayout { fields, more } = tool_options_layout(
        330.,
        36.,
        Axis::Horizontal,
        &[[100., 30.]; 3],
        [32., 32.],
        6.,
    );
    assert!(fields[0].is_some() && fields[1].is_some() && fields[2].is_none());
    assert!(more.x + more.width <= 330.);
    let ToolOptionsLayout { fields, more } =
        tool_options_layout(36., 36., Axis::Vertical, &[[100., 30.]], [32., 32.], 6.);
    assert!(fields[0].is_none() && more.height > 0.);
    let duplicates = vec![
        ToolbarTile { slider_style: Default::default(),
            id: 1,
            control: ToolbarControl::TOOL_OPTIONS,
        },
        ToolbarTile { slider_style: Default::default(),
            id: 2,
            control: ToolbarControl::TOOL_OPTIONS,
        },
    ];
    let layout = toolbar_tile_layout(
        900.,
        36.,
        Axis::Horizontal,
        &duplicates,
        true,
        TileStyle::Small,
    );
    assert_eq!(layout.tiles[0].width, layout.tiles[1].width);
    for axis in [Axis::Horizontal, Axis::Vertical] {
        for size in [0., 1., 20., 36., 80., 200., f32::NAN, f32::INFINITY] {
            let parts = toolbar_slider_layout(size, size, axis);
            for b in parts {
                assert!(
                    b.width.is_finite() && b.height.is_finite() && b.width >= 0. && b.height >= 0.
                );
            }
            let track = if axis == Axis::Horizontal {
                parts[1].width
            } else {
                parts[1].height
            };
            assert!(track >= 0.);
            if size.is_finite() {
                let leading = if axis == Axis::Horizontal { parts[1].x } else { parts[1].y };
                assert_eq!(leading, size - leading - track, "equal slider end insets");
            }
        }
    }
}

#[test]
fn toolbar_components_customize_and_restore_as_atomic_items() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Photographer.layout(Platform::Gtk),
            ..WorkspaceState::default()
        }),
    })
    .unwrap();
    let before = s.state().workspace.layout.clone();
    let controls = [
        ToolbarControl::BrushSizeSlider,
        ToolbarControl::BrushStrengthSlider,
        ToolbarControl::TOOL_OPTIONS,
    ];
    customize(&mut s, CustomizationAction::InsertTools { panel: Panel::Commands, before: None });
    for control in controls {
        assert!(
            s.tool_picker()
                .unwrap()
                .choices
                .iter()
                .any(|c| c.control == control)
        );
        customize(&mut s, CustomizationAction::PickerSelect { control, selected: true });
    }
    customize(&mut s, CustomizationAction::ConfirmTools);
    let added = s.state().workspace.layout.clone();
    let options = added
        .panel(Panel::Commands)
        .unwrap()
        .tiles()
        .iter()
        .find(|t| t.control == ToolbarControl::TOOL_OPTIONS)
        .unwrap()
        .id;
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(s.state().workspace.layout, before);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(s.state().workspace.layout, added);
    customize(&mut s, CustomizationAction::RemoveTool { panel: Panel::Commands, tile: options });
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(s.state().workspace.layout, added);
}

#[test]
fn toolbar_choices_keep_independent_selections_and_disable_unavailable_sliders() {
    for (language, source, sample_size) in [(UiLanguage::English, "Source", "Sample size"), (UiLanguage::Japanese, "参照対象", "取得範囲")] {
    let mut s = UiSession::blank_localized(Recorder::default(), [256, 256], Platform::Gtk,
        Localizer::shared(language)).unwrap();
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
        layout: WorkspacePreset::Photographer.layout(Platform::Gtk), ..WorkspaceState::default()
    }) }).unwrap();
    invoke(&mut s, CommandId::Pen);
    let presets: Vec<_> = s.state().tool_set.subtools.iter()
        .filter(|item| matches!(item.action, UiAction::SelectBrush { .. }))
        .take(2).map(|item| item.action.clone()).collect();
    assert_eq!(presets.len(), 2);
    for preset in presets {
        s.dispatch(UiAction::ToolbarEdit {
            context: s.state().toolbar_context(), action: Box::new(preset.clone()),
        }).unwrap();
        let state = s.state();
        assert!(state.tool_set.subtools.iter().any(|item|
            item.selected && matches!(item.action, UiAction::SelectToolGroup { .. })));
        assert!(state.tool_set.subtools.iter().any(|item| item.selected && item.action == preset));
        let ToolOption::Choice { items, .. } = state.tool_options().into_iter()
            .find(|option| matches!(option, ToolOption::Choice { id: "variant", .. })).unwrap()
            else { panic!() };
        assert_eq!(items.iter().filter(|item| item.selected).count(), 1);
        assert!(items.iter().any(|item| item.selected && item.action == preset));
    }
    invoke(&mut s, CommandId::Eyedropper);
    let choices = s.state().tool_options();
    assert!(choices.iter().any(|o| matches!(o, ToolOption::Choice { id: "variant", label, .. } if label.as_ref() == source)));
    assert!(choices.iter().any(|o| matches!(o, ToolOption::Choice { id: "sample-size", label, .. } if label.as_ref() == sample_size)));
    s.dispatch(UiAction::SetColorSampleSize { width: 101 })
        .unwrap();
    for id in ["variant", "sample-size"] {
        check_choice_items(&mut s, id);
    }
    assert!(ToolbarNumericBinding::BrushSize.field(s.state()).is_none());
    let context = s.state().toolbar_context();
    assert!(
        s.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(ToolbarNumericBinding::BrushSize.action(17.))
        })
        .is_err()
    );
    key(&mut s, "Escape", true, false, false);
    invoke(&mut s, CommandId::AutoSelect);
    let options = s.state().tool_options();
    assert!(options.iter().any(|o| matches!(
        o,
        ToolOption::Choice {
            id: "selection-source",
            ..
        }
    )));
    invoke(&mut s, CommandId::Brush);
    let size = ToolbarNumericBinding::BrushSize.field(s.state()).unwrap();
    assert_eq!(size.id, "size");
    assert_eq!(size.numeric, NumericControl::brush_size());
    }
}

#[test]
fn toolbar_drawer_measurement_contains_all_components_across_styles_and_widths() {
    let controls = [
        ToolbarControl::Color,
        ToolbarControl::BrushSizeSlider,
        ToolbarControl::BrushStrengthSlider,
        ToolbarControl::Divider,
        ToolbarControl::TOOL_OPTIONS,
    ];
    for style in [
        TileStyle::Small,
        TileStyle::Medium,
        TileStyle::Large,
        TileStyle::MediumLabeled,
        TileStyle::Labeled,
    ] {
        for count in 1..=5 {
            for shift in 0..controls.len() {
                let tiles: Vec<_> = (0..count)
                    .map(|i| ToolbarTile { slider_style: Default::default(),
                        id: i as u32 + 1,
                        control: controls[(i + shift) % controls.len()],
                    })
                    .collect();
                for extra in (0..600).step_by(11) {
                    let width = style.size()[0] + 8. + extra as f32;
                    let height = toolbar_content_height(width, &tiles, style);
                    let layout =
                        toolbar_tile_layout(width, height, Axis::Vertical, &tiles, false, style);
                    for (i, b) in layout.tiles.iter().enumerate() {
                        assert!(
                            b.x >= 0.
                                && b.y >= 0.
                                && b.x + b.width <= width + 0.01
                                && b.y + b.height <= height + 0.01,
                            "{style:?} {width}×{height}: {b:?}"
                        );
                        for next in &layout.tiles[i + 1..] {
                            assert!(b.intersection(*next).is_none());
                        }
                        if tiles[i].control.slider().is_some() {
                            assert!(b.height >= 4. * (style.size()[1] + 2.) - 2.);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn options_preferences_round_trip_and_undo_without_losing_controls() {
    let mut s = session(Platform::Gtk);
    let layout = WorkspacePreset::Photographer.layout(Platform::Gtk);
    let tile = layout
        .panel(Panel::Commands)
        .unwrap()
        .tiles()
        .iter()
        .find(|t| t.control.options_style().is_some())
        .unwrap()
        .id;
    s.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState {
            layout,
            ..WorkspaceState::default()
        }),
    })
    .unwrap();
    let before = s.state().workspace.layout.clone();
    let style = ToolOptionsStyle {
        text: false,
        sliders: false,
    };
    customize(&mut s, CustomizationAction::SetToolOptionsStyle {
        panel: Panel::Commands,
        tile,
        style,
    });
    let after = s.state().workspace.layout.clone();
    assert_eq!(
        after
            .panel(Panel::Commands)
            .unwrap()
            .tiles()
            .iter()
            .find(|t| t.id == tile)
            .unwrap()
            .control
            .options_style(),
        Some(style)
    );
    let loaded: DockLayout = serde_json::from_str(&serde_json::to_string(&after).unwrap()).unwrap();
    assert_eq!(after, loaded);
    assert_eq!(
        serde_json::from_str::<ToolbarControl>(r#"{"kind":"tool_options"}"#).unwrap(),
        ToolbarControl::TOOL_OPTIONS
    );
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(s.state().workspace.layout, before);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(s.state().workspace.layout, after);
    s.dispatch(UiAction::ActivateTile {
        panel: Panel::Commands,
        tile,
    })
    .unwrap();
    let drawer = s.state().customization.drawer.as_ref().unwrap();
    assert_eq!(
        drawer.columns,
        vec![vec![Panel::Brushes], vec![Panel::ToolSettings]]
    );
    let p = drawer
        .placement(&after, [1600., 1000.], &[300., 200.])
        .unwrap();
    assert!(p.connection().is_some());
    assert_eq!(
        p.anchor.width,
        after.panel(Panel::Commands).unwrap().tile_style.size()[0]
    );
    assert!((p.bounds.x + p.bounds.width - p.anchor.x - p.anchor.width).abs() < 0.01);
    assert!(p.bounds.y > p.anchor.y + p.anchor.height);
}

#[test]
fn slider_styles_are_independent_saved_and_undoable() {
    for platform in Platform::ALL {
    let mut s = session(platform);
    let mut layout = WorkspacePreset::Painter.layout(platform);
    let panel = layout.add_toolbar(None, "Sliders", &[
        ToolbarControl::BrushSizeSlider, ToolbarControl::BrushStrengthSlider, ToolbarControl::Color,
    ]).unwrap();
    let ids: Vec<_> = layout.panel(panel).unwrap().tiles().iter().map(|tile| tile.id).collect();
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
        layout, ..WorkspaceState::default()
    }) }).unwrap();
    let before = s.state().workspace.layout.clone();
    let artwork = s.engine().document().revision;
    for (index, tile) in ids[..2].iter().copied().enumerate() {
        let target = ContextTarget::Tile { panel, tile };
        let menu = s.context_menu(target).unwrap();
        assert_eq!(menu.sections[0].iter().map(|item| (item.label.as_str(), item.selected)).collect::<Vec<_>>(),
            [("Show value", Some(false)), ("Expand to fit", Some(false))]);
        for option in 0..2 {
            let action = s.context_menu(target).unwrap().sections[0][option].action.clone().unwrap();
            s.dispatch(action).unwrap();
        }
        let view = s.panel_view(panel).unwrap();
        assert_eq!(view.tiles[index].slider_style, ToolbarSliderStyle { show_value: true, expand: true });
        assert_eq!(view.tiles[2].slider_style, ToolbarSliderStyle::default());
    }
    let after = s.state().workspace.layout.clone();
    let saved = serde_json::to_vec(&s.state().workspace).unwrap();
    assert_eq!(serde_json::from_slice::<WorkspaceState>(&saved).unwrap(), s.state().workspace);
    assert_eq!(s.engine().document().revision, artwork);
    for _ in 0..4 { invoke(&mut s, CommandId::UndoWorkspace); }
    assert_eq!(s.state().workspace.layout, before);
    for _ in 0..4 { invoke(&mut s, CommandId::RedoWorkspace); }
    assert_eq!(s.state().workspace.layout, after);
    let mut copy = after.clone();
    let duplicate = copy.duplicate_toolbar(panel, "Copied Sliders").unwrap();
    for (source, tile) in after.panel(panel).unwrap().tiles().iter().zip(copy.panel(duplicate).unwrap().tiles()) {
        assert_eq!(source.slider_style, tile.slider_style);
        assert_ne!(source.id, tile.id);
    }
    assert!(s.dispatch(UiAction::Customize { action: CustomizationAction::SetSliderStyle {
        panel, tile: ids[2], style: ToolbarSliderStyle { show_value: true, expand: true },
    } }).is_err());
    assert_eq!(s.state().workspace.layout, after);
}
}

#[test]
fn slider_expansion_fills_rows_without_stretching_neighbors() {
    for style in [TileStyle::Small, TileStyle::Medium, TileStyle::Large, TileStyle::MediumLabeled, TileStyle::Labeled] {
        let [w, h] = style.size();
        let gap = style.gap();
        let mut tiles = vec![
            ToolbarTile { id: 1, control: ToolbarControl::BrushSizeSlider,
                slider_style: ToolbarSliderStyle { show_value: true, expand: true } },
            ToolbarTile { id: 2, control: ToolbarControl::BrushStrengthSlider,
                slider_style: ToolbarSliderStyle { show_value: true, expand: true } },
            ToolbarTile { id: 3, control: ToolbarControl::Color, slider_style: Default::default() },
        ];
        let width = w * 2. + gap;
        let height = 800.;
        let layout = toolbar_tile_layout(width, height, Axis::Vertical, &tiles, true, style);
        let a = layout.tiles[0];
        let b = layout.tiles[1];
        assert_eq!(a.y, b.y);
        assert_eq!(a.height, b.height);
        assert!(a.height > 600.);
        assert!(a.x + a.width <= b.x);
        assert_eq!(layout.tiles[2].height, h);
        assert_eq!(layout.tiles[2].y + h, height - 20. - gap);
        tiles[1].slider_style.expand = false;
        let mixed = toolbar_tile_layout(width, height, Axis::Vertical, &tiles, true, style);
        assert!(mixed.tiles[0].height > mixed.tiles[1].height);
        let compact = toolbar_tile_layout(width, 100., Axis::Vertical, &tiles, true, style);
        assert!(compact.tiles.iter().all(|b| b.y + b.height <= 100.));
        let stacked = toolbar_tile_layout(w, height, Axis::Vertical, &tiles[..2], true, style);
        assert!(stacked.tiles[0].y + stacked.tiles[0].height <= stacked.tiles[1].y);
        for axis in [Axis::Horizontal, Axis::Vertical] {
            let parts = toolbar_slider_styled_layout(a.width, a.height, axis, style, tiles[0].slider_style);
            assert!(parts.iter().all(|b| b.width >= 0. && b.height >= 0.));
            assert!(parts[0].intersection(parts[1]).is_none());
        }
    }
}

#[test]
fn slider_readouts_have_numeric_values_units_and_localized_labels() {
    let s = session(Platform::Gtk);
    for control in [ToolbarControl::BrushSizeSlider, ToolbarControl::BrushStrengthSlider] {
        let view = s.state().toolbar_component(control).unwrap();
        assert!(!view.value_label.is_empty());
        let field = view.numeric.unwrap();
        assert_eq!(field.numeric.unit, if control == ToolbarControl::BrushSizeSlider { "px" } else { "%" });
        for value in [field.numeric.min, field.value as f64, field.numeric.max] {
            let text = field.numeric.compact_value(value);
            assert!(!text.contains("px") && !text.contains('%'));
        }
    }
    for axis in [Axis::Horizontal, Axis::Vertical] {
        for style in [TileStyle::Small, TileStyle::Medium, TileStyle::Large, TileStyle::MediumLabeled, TileStyle::Labeled] {
            for (value, long) in [("0.5", false), ("24", false), ("99", false), ("100", true), ("430", true), ("999", true), ("1000", true), ("2048", true)] {
                let metrics = toolbar_slider_readout_metrics(style, axis, value);
                assert_eq!(metrics.units_below, axis == Axis::Vertical
                    && (style == TileStyle::Small || (style == TileStyle::Medium && long)), "{axis:?}/{style:?}/{value}");
                assert_eq!(metrics.inset, PANEL_CONTROL_METRICS.field_padding);
                assert!(metrics.extent >= PANEL_CONTROL_METRICS.control_height * 2.);
            }
        }
    }
}

#[test]
fn vertical_options_shrink_before_reflowing_and_keep_more_accessible() {
    let tiles = vec![
        ToolbarTile { slider_style: Default::default(),
            id: 1,
            control: ToolbarControl::Color,
        },
        ToolbarTile { slider_style: Default::default(),
            id: 2,
            control: ToolbarControl::TOOL_OPTIONS,
        },
    ];
    for style in [
        TileStyle::Small,
        TileStyle::Medium,
        TileStyle::Large,
        TileStyle::MediumLabeled,
        TileStyle::Labeled,
    ] {
        let [w, h] = style.size();
        let grip = 20. + style.gap();
        let height = 3. * (h + style.gap()) + grip;
        let layout = toolbar_tile_layout(w, height, Axis::Vertical, &tiles, true, style);
        assert_eq!(layout.tiles[0].x, layout.tiles[1].x);
        assert!(layout.tiles[1].height < 8. * h);
        assert!(layout.tiles[1].y + layout.tiles[1].height <= height - grip);
        let fitting = tool_options_layout(
            w,
            layout.tiles[1].height,
            Axis::Vertical,
            &[[w, h]; 20],
            [w, h],
            4.,
        );
        assert!(fitting.more.height >= h);
    }
}

#[test]
fn compact_edge_moves_preserve_toolbar_identity_and_one_step_history() {
    let mut s = session(Platform::Gtk);
    let layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    let panel = layout
        .panels
        .iter()
        .find(|p| {
            p.tiles()
                .iter()
                .any(|t| t.control == ToolbarControl::BrushSizeSlider)
        })
        .unwrap()
        .id;
    let tiles = layout.panel(panel).unwrap().tiles().to_vec();
    assert!(
        layout
            .bands
            .iter()
            .any(|b| b.edge == Edge::Left && b.alignment == Some(EdgeAlignment::Center))
    );
    s.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState {
            layout: layout.clone(),
            ..Default::default()
        }),
    })
    .unwrap();
    s.dispatch(UiAction::MovePanel {
        panel,
        target: DockTarget::CompactEdge {
            edge: Edge::Right,
            alignment: EdgeAlignment::Center,
        },
        viewport: [1200., 900.],
    })
    .unwrap();
    let moved = s.state().workspace.layout.clone();
    assert_eq!(moved.panel(panel).unwrap().tiles(), tiles);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(s.state().workspace.layout, layout);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(s.state().workspace.layout, moved);
}

#[test]
fn toolbar_toolboxes_pack_rows_and_span_dividers_and_options() {
    let controls = [
        ToolbarControl::Color,
        ToolbarControl::Color,
        ToolbarControl::Color,
        ToolbarControl::Divider,
        ToolbarControl::Color,
        ToolbarControl::Color,
        ToolbarControl::TOOL_OPTIONS,
    ];
    let tiles: Vec<_> = controls
        .into_iter()
        .enumerate()
        .map(|(i, control)| ToolbarTile { slider_style: Default::default(),
            id: i as u32,
            control,
        })
        .collect();
    for style in [
        TileStyle::Small,
        TileStyle::Medium,
        TileStyle::Large,
        TileStyle::MediumLabeled,
        TileStyle::Labeled,
    ] {
        let [w, h] = style.size();
        for columns in [2, 3, 4] {
            let width = columns as f32 * (w + style.gap()) - style.gap();
            let layout = toolbar_tile_layout(width, 1000., Axis::Vertical, &tiles, true, style);
            let b = &layout.tiles;
            assert_eq!(b[0].y, b[1].y);
            assert!(b[1].x > b[0].x);
            if columns == 2 {
                assert!(b[2].y > b[1].y);
            }
            assert_eq!(b[3].width, width);
            assert_eq!(b[3].height, 8.);
            assert_eq!(b[6].x, 0.);
            assert_eq!(b[6].width, width);
            assert!(b[6].y >= b[5].y + h);
            let options = tool_options_layout(
                width,
                b[6].height,
                Axis::Vertical,
                &[[w, h]; 12],
                [w, h],
                2.,
            );
            let first = options.fields[0].unwrap();
            let second = options.fields[1].unwrap();
            assert_eq!(first.y, second.y);
            assert!(second.x > first.x);
            for cell in options.fields.into_iter().flatten() {
                assert!(cell.x + cell.width <= width && cell.y + cell.height <= b[6].height);
                assert!(cell.intersection(options.more).is_none());
            }
        }
    }
    // The new Photo band owns both top corners, so side panels start below it.
    let layout = WorkspacePreset::Photographer.layout(Platform::Gtk);
    let resolved = layout.workspace(1600., 1000., HEADER_HEIGHT, STATUS_HEIGHT);
    let top = resolved
        .groups
        .iter()
        .find(|g| g.active == Panel::Commands)
        .unwrap();
    assert_eq!(top.bounds.x, WORKSPACE_SPACING);
    assert_eq!(top.bounds.width, 1600. - 2. * WORKSPACE_SPACING);
    let tools = resolved
        .groups
        .iter()
        .find(|g| g.active == Panel::Toolbar)
        .unwrap();
    assert!(tools.bounds.y >= top.bounds.y + top.bounds.height);
}

#[test]
fn toolbar_number_width_samples_cover_ranges_and_units() {
    for spec in [
        NumericControl::brush_size(),
        NumericControl::percent(),
        NumericControl::number(-131072., 131072., 1., 0).unit("px"),
        NumericControl::number(0., 32., 1., 1).unit("px"),
        NumericControl::number(-180., 180., 1., 0).unit("°"),
    ] {
        for compact in [false, true] {
            let reserved = spec
                .width_samples(compact)
                .iter()
                .map(|s| s.chars().count())
                .max()
                .unwrap();
            for i in 0..=1000 {
                let v = spec.min + (spec.max - spec.min) * i as f64 / 1000.;
                let text = if compact {
                    spec.compact_text(v)
                } else {
                    spec.resolve(v, NumericOperation::Format).unwrap().text
                };
                assert!(
                    text.chars().count() <= reserved,
                    "{text} exceeds range reservation {reserved}"
                );
            }
        }
    }
}

#[test]
fn toolbar_field_icons_cover_published_settings_and_exist_in_the_bank() {
    let localizer = Localizer::shared(UiLanguage::English);
    let mut fields = Vec::new();
    for brush in brush_catalog() {
        fields.extend(tool_settings::controls(brush.id, &layer_core::default_brush(
            tools::preset(brush.id).unwrap(),
        ), &localizer));
    }
    fields.extend(region_tools::RegionTools::default().controls(&localizer));
    for constraint in [
        SelectionConstraint::Free,
        SelectionConstraint::Ratio,
        SelectionConstraint::Size,
    ] {
        let options = SelectionOptions {
            constraint,
            ..Default::default()
        };
        fields.extend(options.controls(&localizer));
        fields.extend(options.edge_controls(&localizer));
    }
    for id in fields.iter().map(|f| f.id).chain([
        "transform_x",
        "transform_y",
        "transform_width",
        "transform_height",
        "transform_angle",
    ]) {
        let icon = tool_setting_icon(id);
        assert_ne!(icon, "settings", "{id} needs a meaningful icon");
        assert!(crate::icon_ships(icon));
    }
}

#[test]
fn toolbar_choices_preserve_segmented_modes_and_list_sources() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::AutoSelect);
    let options = s.state().tool_options();
    let mode = options
        .iter()
        .find(|o| {
            matches!(
                o,
                ToolOption::Choice {
                    id: "selection-mode",
                    ..
                }
            )
        })
        .unwrap();
    let ToolOption::Choice {
        segmented, items, ..
    } = mode
    else {
        panic!()
    };
    assert!(*segmented);
    assert_eq!(items.len(), 4);
    check_choice_items(&mut s, "selection-mode");
    assert!(options.iter().any(|o| matches!(
        o,
        ToolOption::Choice {
            id: "selection-source",
            segmented: false,
            ..
        }
    )));
    assert!(
        options
            .iter()
            .filter(|o| matches!(
                o,
                ToolOption::Choice {
                    segmented: true,
                    ..
                }
            ))
            .count()
            == 1
    );

    let sizes = [[36., 36.], [36., 144.], [36., 36.]];
    let full = tool_options_layout(36., 288., Axis::Vertical, &sizes, [36., 36.], 2.);
    assert_eq!(full.fields[0].unwrap().height, 36.);
    assert_eq!(full.fields[1].unwrap().y, 38.);
    assert_eq!(full.fields[1].unwrap().height, 144.);
    assert_eq!(full.fields[2].unwrap().y, 184.);
    let short = tool_options_layout(36., 180., Axis::Vertical, &sizes, [36., 36.], 2.);
    assert!(short.fields[0].is_some());
    assert!(
        short.fields[1..].iter().all(Option::is_none),
        "overflow hides the entire bar"
    );

    let photo = WorkspacePreset::Photographer.layout(Platform::Gtk);
    let commands = photo.panel(Panel::Commands).unwrap().tiles();
    assert!(!commands.iter().any(|t| t.control
        == ToolbarControl::Command {
            command: CommandId::FlipHorizontal
        }));
    assert!(
        !commands
            .windows(2)
            .any(|p| p.iter().all(|t| t.control == ToolbarControl::Divider))
    );
}

#[test]
fn transform_position_reference_projects_beside_x_and_floating_modes_are_joined() {
    let mut s = transformed_photo("position anchor metadata");
    s.frame(1, 1).unwrap();
    let options = s.state().tool_options();
    let reference = options.iter().find(|option| matches!(option, ToolOption::Choice { id: "transform-reference", .. })).unwrap();
    let ToolOption::Choice { columns, beside, segmented, items, .. } = reference else { panic!("reference choice required") };
    assert_eq!(*columns, Some(3));
    assert_eq!(*beside, Some("transform_x"));
    assert!(*segmented);
    assert_eq!(items.len(), 9);
    assert_eq!(items.iter().filter(|item| item.selected).count(), 1);
    let encoded = serde_json::to_value(reference).unwrap();
    assert_eq!(encoded["Choice"]["beside"], "transform_x");
    let bar = s.state.canvas_bar.as_ref().unwrap();
    let modes = bar.items.iter().find(|item| matches!(&item.option, ToolOption::Choice { items, .. } if items.iter().any(|item| matches!(item.action, UiAction::Invoke { command: CommandId::TransformFree })))).unwrap();
    let ToolOption::Choice { segmented, beside, items, .. } = &modes.option else { panic!("mode choice required") };
    assert!(*segmented);
    assert_eq!(*beside, None);
    assert_eq!(items.len(), 3);
    assert!(serde_json::to_value(&modes.option).unwrap()["Choice"].get("beside").is_none());
}

#[test]
fn slider_bookmarks_round_trip_follow_presets_and_reject_stale_editors() {
    let mut s = session(Platform::Gtk);
    let control = ToolbarControl::BrushSizeSlider;
    let context = s.state().toolbar_context();
    s.dispatch(UiAction::SetToolSetting {
        id: "size".into(),
        value: 64.,
    })
    .unwrap();
    let toggle = UiAction::ToolbarEdit {
        context,
        action: Box::new(UiAction::ToggleSliderBookmark { control }),
    };
    s.dispatch(toggle.clone()).unwrap();
    let view = s.state().toolbar_component(control).unwrap();
    assert_eq!(view.bookmarks.len(), 1);
    assert_eq!(view.bookmarks[0].value, 64.);
    assert!(view.bookmarks[0].selected);
    let strength=ToolbarControl::BrushStrengthSlider;
    s.dispatch(strength.slider().unwrap().action(0.37)).unwrap();
    s.dispatch(UiAction::ToolbarEdit {context,
        action:Box::new(UiAction::ToggleSliderBookmark {control:strength})}).unwrap();
    for (control,value) in [(control,64.),(strength,0.37)] {
        let bookmarks=s.state().toolbar_component(control).unwrap().bookmarks;
        assert_eq!(bookmarks.len(),1);assert_eq!(bookmarks[0].value,value);
        assert!(bookmarks[0].selected);
    }
    let saved = serde_json::to_string(&s.state().settings).unwrap();
    let settings: Settings = serde_json::from_str(&saved).unwrap();
    settings.validate().unwrap();
    assert_eq!(
        settings.slider_bookmarks,
        s.state().settings.slider_bookmarks
    );
    assert!(
        s.state()
            .requests
            .iter()
            .any(|r| matches!(r.kind, HostRequestKind::SaveSettings { .. }))
    );
    s.dispatch(UiAction::SetToolSetting {
        id: "size".into(),
        value: 65.,
    })
    .unwrap();
    assert!(!s.state().toolbar_component(control).unwrap().bookmarks[0].selected);
    s.dispatch(UiAction::SetToolSetting {
        id: "size".into(),
        value: 64.,
    })
    .unwrap();
    s.dispatch(toggle.clone()).unwrap();
    assert!(
        s.state()
            .toolbar_component(control)
            .unwrap()
            .bookmarks
            .is_empty()
    );
    s.dispatch(UiAction::SelectBrush { id: 2 }).unwrap();
    assert!(s.dispatch(toggle).is_err());
    assert!(
        s.state()
            .toolbar_component(control)
            .unwrap()
            .bookmarks
            .is_empty()
    );
    assert!(s.state().toolbar_component(strength).unwrap().bookmarks.is_empty());
    s.dispatch(UiAction::SelectBrush {id:context.brush}).unwrap();
    assert_eq!(s.state().toolbar_component(strength).unwrap().bookmarks[0].value,0.37);
}

#[test]
fn slider_bookmark_taps_are_nearby_bounded_and_choose_the_closest_mark() {
    for (control, value, neighbor) in [
        (ToolbarControl::BrushSizeSlider, 64., 80.),
        (ToolbarControl::BrushStrengthSlider, 0.5, 0.54),
    ] {
        let numeric = control.slider().unwrap().numeric();
        let fill = |v| numeric.resolve(v, NumericOperation::Format).unwrap().fill;
        let mark = fill(value as f64);
        for travel in [140., 200., 400.] {
            for sign in [-1., 1.] {
                let near = mark + sign * 16. / travel;
                assert_eq!(
                    slider_bookmark_value(control, &[value], near, travel, &Localizer::shared(UiLanguage::English)).unwrap(),
                    value as f64
                );
                let far = mark + sign * 24. / travel;
                assert_ne!(
                    slider_bookmark_value(control, &[value], far, travel, &Localizer::shared(UiLanguage::English)).unwrap(),
                    value as f64
                );
                // The same nearby position remains unsnapped during a drag.
                assert_ne!(
                    slider_bookmark_value(control, &[], near, travel, &Localizer::shared(UiLanguage::English)).unwrap(),
                    value as f64
                );
            }
        }
        assert_eq!(
            slider_bookmark_value(
                control,
                &[value, neighbor],
                fill(neighbor as f64) - 0.001,
                200.
            , &Localizer::shared(UiLanguage::English))
            .unwrap(),
            neighbor as f64
        );
        // A very short track must not turn most of its range into a tap target.
        assert_eq!(
            slider_bookmark_value(control, &[value], mark + 0.14, 40., &Localizer::shared(UiLanguage::English)).unwrap(),
            value as f64
        );
        assert_ne!(
            slider_bookmark_value(control, &[value], mark + 0.16, 40., &Localizer::shared(UiLanguage::English)).unwrap(),
            value as f64
        );
        for travel in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(slider_bookmark_value(control, &[value], mark, travel, &Localizer::shared(UiLanguage::English)).is_err());
        }
        for value in [numeric.min as f32, numeric.max as f32] {
            let position = if value == numeric.min as f32 {
                0.01
            } else {
                0.99
            };
            assert_eq!(
                slider_bookmark_value(control, &[value], position, 200., &Localizer::shared(UiLanguage::English)).unwrap(),
                value as f64
            );
        }
    }
}

#[test]
fn slider_preview_geometry_and_embedded_opacity_are_shared() {
    let size = slider_preview_layout(
        ToolbarControl::BrushSizeSlider,
        TileStyle::Small,
        64.,
        180.,
        1.,
     &Localizer::shared(UiLanguage::English))
    .unwrap();
    assert_eq!(size.stamp.width, 64.);
    assert_eq!(size.stamp.y, size.stamp.x);
    assert_eq!(size.viewport, Bounds { x: 0., y: 0., width: 180., height: 180. });
    assert_eq!(size.header_fade, 65. * 36. / 54.);
    assert_eq!(size.header_fade_opacity, 0.5);
    assert_eq!(size.opacity, 1.);
    assert_eq!(size.text, "Size: 64 px");
    let opacity = slider_preview_layout(
        ToolbarControl::BrushStrengthSlider,
        TileStyle::Small,
        0.42,
        180.,
        1.,
     &Localizer::shared(UiLanguage::English))
    .unwrap();
    assert_eq!(opacity.text, "Strength: 42 %");
    assert_eq!(opacity.opacity, 1.);
    assert_eq!(
        opacity.stamp,
        slider_preview_layout(
            ToolbarControl::BrushStrengthSlider,
            TileStyle::Small,
            1.,
            180.,
            1.
        , &Localizer::shared(UiLanguage::English))
        .unwrap()
        .stamp
    );
    for (style, icon) in [
        (TileStyle::Small, 16.),
        (TileStyle::Medium, 24.),
        (TileStyle::Large, 32.),
        (TileStyle::Labeled, 32.),
    ] {
        let tile = style.size()[1];
        let layout =
            slider_preview_layout(ToolbarControl::BrushStrengthSlider, style, 1., 0., 1., &Localizer::shared(UiLanguage::English)).unwrap();
        assert_eq!(layout.radius, style.corner_radius());
        assert_eq!(
            layout.bookmark,
            Bounds {
                x: layout.side - tile,
                y: 0.,
                width: tile,
                height: tile
            }
        );
        assert_eq!(layout.icon, icon);
        assert_eq!(layout.caption.height, tile);
        assert!(
            layout.caption.width >= 104.,
            "{style:?} leaves room for its value"
        );
        assert_eq!(layout.viewport.y, tile);
        let size =
            slider_preview_layout(ToolbarControl::BrushSizeSlider, style, 64., 0., 1., &Localizer::shared(UiLanguage::English)).unwrap();
        assert_eq!(size.header_fade, 65. * tile / 54., "{style:?} header fade");
        assert!(layout.stamp.y >= tile);
    }
    assert!(slider_bookmark_value(ToolbarControl::BrushSizeSlider, &[], f64::NAN, 200., &Localizer::shared(UiLanguage::English)).is_err());
    assert!(
        slider_preview_layout(
            ToolbarControl::BrushSizeSlider,
            TileStyle::Small,
            f32::INFINITY,
            180.,
            1.
        , &Localizer::shared(UiLanguage::English))
        .is_err()
    );
    let invalid = r#"{"size":[64,32],"strength":[]}"#;
    assert!(
        serde_json::from_str::<SliderBookmarks>(invalid)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn slider_stamp_plan_uses_completed_writing_contact_and_tool_strength() {
    use layer_core::DefaultBrushPreset as P;
    let mut s = session(Platform::Gtk);
    for preset in [P::Pencil,P::PointyPencil,P::ShadingPencil,P::Charcoal,P::Chalk,P::PastelBlock,P::Crayon] {
        s.dispatch(UiAction::SelectBrush {id:preset as u32}).unwrap();
        let brush = s.engine.configured_brush().clone();
        let pose = brush.contact.unwrap().dry.unwrap().poses[1];
        let (full, extent) = s.stamp_request().unwrap();
        assert!(!full.dabs.is_empty(), "{preset:?}: completed contact");
        assert_eq!(full.shape_strength,None);
        for strength in [0.,0.37,1.] {
            let mut edited=brush.clone();edited.flow=strength;edited.opacity=0.42;
            s.engine.set_brush(edited).unwrap();
            let (request, actual_extent)=s.stamp_request().unwrap();
            assert_eq!((request.bounds,actual_extent),(full.bounds,extent),"{preset:?}: zero Strength retains geometry");
            assert_eq!(request.style.opacity,0.42);
            assert_eq!(request.style.contact.unwrap().pressure_gain,brush.contact.unwrap().pressure_gain);
            assert_eq!(request.dabs.len(),if strength==0. {0} else {full.dabs.len()});
            for dab in request.dabs {
                assert_eq!(dab.center,Point{x:0.,y:0.});
                assert_eq!(dab.motion,[0.;2]);
                assert_eq!(dab.radii,pose.radii.map(|r|r*brush.diameter*0.5));
                assert_eq!(dab.contact[1..],pose.relief);
                assert!((dab.contact[0]-brush.contact.unwrap().pressure_gain*strength).abs()<1e-6);
                assert_eq!((dab.flow,dab.material[3]),(1.,1.));
            }
        }
    }
    for (preset,shape_only) in [(P::Paintbrush,false),(P::Eraser,true),(P::Smudge,true),
        (P::CloneStamp,true),(P::HealingBrush,true),(P::SpotHealingBrush,true),(P::LiquifyPush,true)] {
        s.dispatch(UiAction::SelectBrush {id:preset as u32}).unwrap();
        for strength in [0.,0.37] {
            s.dispatch(ToolbarNumericBinding::BrushStrength.action(strength)).unwrap();
            let (request,extent)=s.stamp_request().unwrap();
            assert_eq!(request.shape_strength,shape_only.then_some(strength),"{preset:?}");
            assert!(extent.is_finite() && extent>0. && request.bounds.max.x>request.bounds.min.x);
        }
    }
}

#[test]
fn slider_stamp_plan_accepts_every_builtin_tool_context() {
    let mut s=session(Platform::Gtk);
    let choices=brush_catalog().collect::<Vec<_>>();
    let mut actual=choices.iter().map(|choice|choice.id).collect::<Vec<_>>();
    let mut expected=DefaultBrushPreset::ALL.map(|preset|preset as u32).to_vec();
    actual.sort_unstable();expected.sort_unstable();
    assert_eq!(actual,expected);
    for choice in choices {
        s.dispatch(UiAction::SelectBrush{id:choice.id}).unwrap();
        assert_eq!((s.state().toolbar_context().brush,s.state().brush.tool),(choice.id,crate::tools::tool(choice.id)));
        let brush=s.engine.configured_brush();brush.validate().unwrap();
        let strength=ToolbarNumericBinding::BrushStrength.field(s.state()).unwrap().value;
        assert!(strength>0.,"{}: default Strength",choice.label);
        let (request,extent)=s.stamp_request().unwrap();
        assert!(!request.bounds.is_empty() && extent.is_finite() && extent>0.,"{}: footprint",choice.label);
        assert!([request.bounds.min.x,request.bounds.min.y,request.bounds.max.x,request.bounds.max.y]
            .into_iter().all(f32::is_finite),"{}: finite bounds",choice.label);
        assert!(!request.dabs.is_empty() && request.dabs.len()<=64,"{}: {} contacts",choice.label,request.dabs.len());
        assert!((0.0..=1.0).contains(&request.style.opacity));
        assert_eq!((request.style.tip,request.style.execution),(brush.tip.clone(),brush.execution_class()));
        let shape_only=matches!(crate::tools::tool(choice.id),Tool::Eraser|Tool::Blend|Tool::Liquify|Tool::Clone|Tool::Heal|Tool::SpotHeal);
        assert_eq!(request.shape_strength,shape_only.then_some(strength),"{}: tool semantics",choice.label);
        for dab in request.dabs {
            assert!(dab.radii.into_iter().all(|r|r>0.));
            assert!([dab.center.x,dab.center.y,dab.flow,dab.hardness].into_iter()
                .chain(dab.radii).chain(dab.rotation).chain(dab.motion).chain(dab.color_rgba_linear)
                .chain(dab.texture_sign).chain(dab.material).chain(dab.previous).chain(dab.contact)
                .chain(dab.previous_contact).all(f32::is_finite),"{}: finite contact",choice.label);
        }
    }
}

struct StampRecorder {
    inner: Recorder,
    requests: Vec<(u64,layer_render::BrushStampRequest)>,
    reply: Option<Result<layer_render::ReadbackImage,layer_render::BackendError>>,
    accepts: bool,
    error: bool,
}
impl Default for StampRecorder {
    fn default()->Self {Self{inner:Recorder::default(),requests:Vec::new(),reply:None,accepts:true,error:false}}
}
impl StampRecorder {
    fn complete(&mut self,id:u64,value:u8)->*const u8 {
        let bytes=vec![value;4];let pointer=bytes.as_ptr();
        self.reply=Some(Ok(layer_render::ReadbackImage{request_id:id,width:2,height:2,stride:2,bytes}));pointer
    }
}
impl CanvasRenderer for StampRecorder {
    type Error=layer_render::BackendError;
    fn resize_surface(&mut self,w:u32,h:u32)->Result<(),Self::Error>{self.inner.resize_surface(w,h)}
    fn prepare_asset(&mut self,id:&AssetId,image:layer_render::HostImage<'_>)->Result<(),Self::Error>{self.inner.prepare_asset(id,image)}
    fn release_asset(&mut self,id:&AssetId){self.inner.release_asset(id)}
    fn submit(&mut self,packet:layer_render::FramePacket<'_>)->Result<(),Self::Error>{self.inner.submit(packet)}
    fn request_brush_stamp(&mut self,id:u64,request:&layer_render::BrushStampRequest)->Result<bool,Self::Error>{
        if self.error {return Err(layer_render::BackendError("stamp request failed"));}
        self.requests.push((id,request.clone()));Ok(self.accepts)
    }
    fn take_brush_stamp(&mut self)->Option<Result<layer_render::ReadbackImage,Self::Error>>{self.reply.take()}
}

#[test]
fn slider_stamp_coalesces_full_brush_keys_and_rejects_stale_owners() {
    let mut s=UiSession::blank(StampRecorder::default(),[1000,1000],Platform::Gtk).unwrap();
    let context=s.state().toolbar_context();
    assert!(s.toolbar_stamp(context,None).unwrap().is_none());
    let first=s.renderer_mut().requests[0].0;
    let pixels=s.renderer_mut().complete(first,7);
    let ready=s.toolbar_stamp(context,None).unwrap().unwrap();
    assert!(!ready.pending);
    assert_eq!(ready.alpha.as_ptr(),pixels,"GPU reply allocation moves to the client");
    assert_eq!((ready.request_id,ready.size,ready.alpha.as_slice()),(first,2,&[7;4][..]));
    let unchanged=s.toolbar_stamp(context,Some(first)).unwrap().unwrap();
    assert_eq!((unchanged.request_id,unchanged.size,unchanged.extent),(first,ready.size,ready.extent));
    assert!(!unchanged.pending && unchanged.alpha.is_empty(),"completed pixels are delivered once");
    assert_eq!(s.renderer_mut().requests.len(),1);
    let mut brush=s.engine.configured_brush().clone();
    brush.tip=layer_core::BrushTip::Mask(AssetId::from("preview-test"));
    s.engine.set_brush(brush.clone()).unwrap();
    let pending=s.toolbar_stamp(context,Some(first)).unwrap().unwrap();
    assert!(pending.pending && pending.alpha.is_empty());
    assert_eq!(pending.request_id,first);
    assert_eq!(s.renderer_mut().requests[1].1.style.tip,brush.tip);
    let second=s.renderer_mut().requests[1].0;
    brush.tip=layer_core::BrushTip::AnalyticEllipse;
    brush.grain=Some(layer_core::BrushGrain{depth:0.5});
    s.engine.set_brush(brush.clone()).unwrap();
    assert!(s.toolbar_stamp(context,Some(first)).unwrap().unwrap().pending);
    assert_eq!(s.renderer_mut().requests.len(),2,"one request in flight");
    s.renderer_mut().complete(second,11);
    let pending=s.toolbar_stamp(context,Some(first)).unwrap().unwrap();
    assert!(pending.pending);
    assert_eq!(pending.alpha.as_slice(),&[11;4]);
    assert_eq!(s.renderer_mut().requests[2].1.style.grain,brush.grain);
    let paper=s.engine.document().paper(None);
    assert_eq!(s.renderer_mut().requests[2].1.style.paper.as_ref().map(|paper|paper.surface.as_ref()),Some(&paper));
    let third=s.renderer_mut().requests[2].0;
    s.dispatch(UiAction::SelectBrush{id:layer_core::DefaultBrushPreset::Charcoal as u32}).unwrap();
    let next=s.state().toolbar_context();
    assert_ne!(next,context);
    s.renderer_mut().complete(third,19);
    assert_eq!(s.toolbar_stamp(context,Some(second)).err().unwrap(),"Obsolete brush preview");
    assert!(s.toolbar_stamp(next,None).unwrap().is_none(),"old context never presents its completed image");
    let fourth=s.renderer_mut().requests.last().unwrap().0;
    s.renderer_mut().complete(fourth,23);
    assert!(!s.toolbar_stamp(next,None).unwrap().unwrap().pending);
    s.replace_renderer(StampRecorder::default()).unwrap();
    assert!(s.toolbar_stamp(next,Some(fourth)).unwrap().is_none(),"replacement GPU discards cached pixels");
    assert_eq!(s.renderer_mut().requests.len(),1);
    assert!(s.renderer_mut().requests[0].0>fourth,"replacement does not reuse request identity");
}

#[test]
fn slider_stamp_retries_admission_errors_and_mismatched_replies() {
    let mut s=UiSession::blank(StampRecorder{accepts:false,..Default::default()},[1000,1000],Platform::Gtk).unwrap();
    let context=s.state().toolbar_context();
    for _ in 0..2 {assert!(s.toolbar_stamp(context,None).unwrap().is_none());}
    assert_eq!(s.renderer_mut().requests.iter().map(|r|r.0).collect::<Vec<_>>(),[1,1]);
    s.renderer_mut().accepts=true;s.renderer_mut().error=true;
    assert_eq!(s.toolbar_stamp(context,None).err().unwrap(),"stamp request failed");
    s.renderer_mut().error=false;
    assert!(s.toolbar_stamp(context,None).unwrap().is_none());
    s.renderer_mut().reply=Some(Err(layer_render::BackendError("stamp readback failed")));
    assert_eq!(s.toolbar_stamp(context,None).err().unwrap(),"stamp readback failed");
    assert!(s.toolbar_stamp(context,None).unwrap().is_none());
    let id=s.renderer_mut().requests.last().unwrap().0;
    assert_eq!(id,2);
    let in_flight=s.renderer_mut().requests.len();
    s.renderer_mut().complete(id-1,29);
    assert!(s.toolbar_stamp(context,None).unwrap().is_none(),"mismatched reply is never cached");
    assert_eq!(s.renderer_mut().requests.len(),in_flight,"old reply cannot replace the active request");
    s.renderer_mut().complete(id,31);
    let ready=s.toolbar_stamp(context,None).unwrap().unwrap();
    assert!(!ready.pending);
    assert_eq!((ready.request_id,s.renderer_mut().requests.len()),(id,in_flight));
    assert_eq!(ready.alpha.as_slice(),&[31;4]);
    assert!(s.toolbar_stamp(context,Some(id)).unwrap().unwrap().alpha.is_empty());
    let count=s.renderer_mut().requests.len();
    let reopened=s.toolbar_stamp(context,None).unwrap().unwrap();
    assert!(reopened.pending && reopened.alpha.is_empty(),"reopened popup needs a new image");
    assert_eq!(s.renderer_mut().requests.len(),count+1);
    let new_id=s.renderer_mut().requests.last().unwrap().0;
    s.renderer_mut().complete(new_id,37);
    let current=s.toolbar_stamp(context,Some(id)).unwrap().unwrap();
    assert!(!current.pending && current.alpha==[37;4]);
    let missed=s.toolbar_stamp(context,Some(id)).unwrap().unwrap();
    assert!(missed.pending && missed.alpha.is_empty(),"client with a missed result retries");
    assert_eq!(s.renderer_mut().requests.len(),count+2);
}

#[test]
fn toolbar_brush_metadata_updates_while_raster_backing_is_pending() {
    for platform in Platform::ALL {
        for wrapped in [false, true] {
            let mut s = session(platform);
            let checkpoint = s.engine.checkpoint();
            s.renderer_mut().settling = true;
            assert!(s.engine.raster_backing_pending());
            for value in [13.5, 62., 174.] {
                let action = UiAction::SetToolSetting { id: "size".into(), value };
                let action = if wrapped { UiAction::ToolbarEdit {
                    context: s.state().toolbar_context(), action: Box::new(action),
                } } else { action };
                s.dispatch(action).unwrap();
                assert_eq!(s.state().brush.diameter, value, "{platform:?}, wrapped={wrapped}");
                assert_eq!(s.engine.configured_brush().diameter, value);
                assert!(s.deferred_edits.is_empty());
                assert_eq!(s.engine.checkpoint(), checkpoint);
            }
        }
    }
}

#[test]
fn dab_settings_change_the_brush_stamp_wherever_a_brush_has_them() {
    let same = |a: &layer_render::BrushStampRequest, b: &layer_render::BrushStampRequest| a.style == b.style && a.dabs == b.dabs && a.shape_strength == b.shape_strength;
    let mut s = session(Platform::Gtk);
    let mut seen = std::collections::BTreeSet::new();
    for choice in brush_catalog().collect::<Vec<_>>() {
        s.dispatch(UiAction::SelectBrush { id: choice.id }).unwrap();
        let (before, _) = s.stamp_request().unwrap();
        for control in s.state().tool_settings.clone() {
            let n = &control.numeric;
            let other = if (control.value as f64) < (n.min + n.max) * 0.5 { n.max } else { n.min } as f32;
            s.dispatch(UiAction::SetToolSetting { id: control.id.into(), value: other }).unwrap();
            let changed = !same(&before, &s.stamp_request().unwrap().0);
            s.dispatch(UiAction::SetToolSetting { id: control.id.into(), value: control.value }).unwrap();
            if crate::tool_settings::shapes_dab(control.id) && control.id != "angle" {
                assert!(changed, "{} changes the stamp of brush {}", control.id, choice.id);
            }
            if crate::tool_settings::shapes_dab(control.id) && changed {
                seen.insert(control.id);
            }
            if matches!(control.id, "spacing" | "smoothing_strength") {
                assert!(!changed, "{} leaves a single dab alone", control.id);
            }
        }
    }
    assert!(["size", "flow", "strength", "opacity", "hardness", "angle", "grain_depth", "material_hardness", "bristle_scale", "bristle_load"].iter().all(|id| seen.contains(id)), "{seen:?}");
}

