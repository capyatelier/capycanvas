#[test]
fn toolbar_resets_use_tool_defaults_and_reject_stale_context() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
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
    let opacity = ToolbarNumericBinding::BrushOpacity.numeric();
    let value = opacity.resolve(0., NumericOperation::Position { position: 0.427 }).unwrap();
    assert!((value.value - 0.43).abs() < 1e-6);
    assert_eq!(value.text, "43 %");
}

#[test]
fn toolbar_components_edit_shared_parameters_and_reject_obsolete_contexts() {
    let mut s = session();
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
    s.dispatch(UiAction::Invoke {
        command: CommandId::Eraser,
    })
    .unwrap();
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
    let file = s.state().document_file.clone();
    let revision = s.engine().document().revision;
    assert!(
        s.adopt_project(Box::new(session()), file.epoch, revision, None)
            .is_ok()
    );
    assert!(
        s.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(UiAction::SetBrushSize { value: 17. })
        })
        .is_err(),
        "replacing a document invalidates otherwise identical targets"
    );
}

#[test]
fn toolbar_options_follow_tools_and_preserve_completion_actions() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    for tool in [
        CommandId::Brush,
        CommandId::Eraser,
        CommandId::Fill,
        CommandId::RectangleSelect,
        CommandId::Gradient,
        CommandId::Eyedropper,
    ] {
        s.dispatch(UiAction::Invoke { command: tool }).unwrap();
        let options = s.state().tool_options();
        for field in &s.state().tool_settings {
            assert!(
                options
                    .iter()
                    .any(|o| matches!(o, ToolOption::Numeric(f) if f == field))
            );
        }
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
    s.dispatch(UiAction::Invoke {
        command: CommandId::Brush,
    })
    .unwrap();
    let selection = layer_core::Selection::polygon(vec![
        Point { x: 100., y: 100. },
        Point { x: 300., y: 100. },
        Point { x: 300., y: 300. },
        Point { x: 100., y: 300. },
    ])
    .unwrap();
    s.fill_selection(selection).unwrap();
    s.frame(1, 1).unwrap();
    s.dispatch(UiAction::Invoke {
        command: CommandId::ScaleRotate,
    })
    .unwrap();
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
fn toolbar_component_defaults_round_trip_on_supported_hosts() {
    for (preset, platform) in WorkspacePreset::ALL.into_iter().flat_map(|p| [Platform::Gtk, Platform::Web, Platform::Android].map(|platform| (p, platform))) {
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
                if platform.color_picker() { vec![ToolbarControl::BrushSizeSlider,
                    ToolbarControl::ColorPicker, ToolbarControl::BrushOpacitySlider,
                    ToolbarControl::Command { command: CommandId::Undo }, ToolbarControl::Command { command: CommandId::Redo }] }
                else { vec![ToolbarControl::BrushSizeSlider, ToolbarControl::BrushOpacitySlider] }
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
        for platform in [
            Platform::Mac,
            Platform::Ios,
            Platform::Windows,
        ] {
            assert_eq!(
                preset.layout(platform),
                preset.legacy_selection_drawers_layout(platform)
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
fn toolbar_components_have_atomic_bounds_and_fill_remaining_width() {
    let tiles: Vec<_> = [
        ToolbarControl::Color,
        ToolbarControl::BrushSizeSlider,
        ToolbarControl::Divider,
        ToolbarControl::BrushOpacitySlider,
        ToolbarControl::TOOL_OPTIONS,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, control)| ToolbarTile {
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
        ToolbarTile {
            id: 1,
            control: ToolbarControl::TOOL_OPTIONS,
        },
        ToolbarTile {
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
    let mut s = session();
    s.set_platform(Platform::Gtk);
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
        ToolbarControl::BrushOpacitySlider,
        ToolbarControl::TOOL_OPTIONS,
    ];
    s.dispatch(UiAction::Customize {
        action: CustomizationAction::InsertTools {
            panel: Panel::Commands,
            before: None,
        },
    })
    .unwrap();
    for control in controls {
        assert!(
            s.tool_picker()
                .unwrap()
                .choices
                .iter()
                .any(|c| c.control == control)
        );
        s.dispatch(UiAction::Customize {
            action: CustomizationAction::PickerSelect {
                control,
                selected: true,
            },
        })
        .unwrap();
    }
    s.dispatch(UiAction::Customize {
        action: CustomizationAction::ConfirmTools,
    })
    .unwrap();
    let added = s.state().workspace.layout.clone();
    let options = added
        .panel(Panel::Commands)
        .unwrap()
        .tiles()
        .iter()
        .find(|t| t.control == ToolbarControl::TOOL_OPTIONS)
        .unwrap()
        .id;
    s.dispatch(UiAction::Invoke {
        command: CommandId::UndoWorkspace,
    })
    .unwrap();
    assert_eq!(s.state().workspace.layout, before);
    s.dispatch(UiAction::Invoke {
        command: CommandId::RedoWorkspace,
    })
    .unwrap();
    assert_eq!(s.state().workspace.layout, added);
    s.dispatch(UiAction::Customize {
        action: CustomizationAction::RemoveTool {
            panel: Panel::Commands,
            tile: options,
        },
    })
    .unwrap();
    s.dispatch(UiAction::Invoke {
        command: CommandId::UndoWorkspace,
    })
    .unwrap();
    assert_eq!(s.state().workspace.layout, added);
    s.dispatch(UiAction::ActivateTile {
        panel: Panel::Commands,
        tile: options,
    })
    .unwrap();
    assert_eq!(
        s.state().customization.drawer.as_ref().unwrap().columns,
        vec![vec![Panel::Brushes], vec![Panel::ToolSettings]]
    );
}

#[test]
fn toolbar_choices_keep_independent_selections_and_disable_unavailable_sliders() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    s.dispatch(UiAction::Invoke {
        command: CommandId::Eyedropper,
    })
    .unwrap();
    s.dispatch(UiAction::SetColorSampleSize { width: 101 })
        .unwrap();
    let options = s.state().tool_options();
    for id in ["variant", "sample-size"] {
        let ToolOption::Choice { items, .. } = options
            .iter()
            .find(|o| matches!(o, ToolOption::Choice { id: key, .. } if *key == id))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(items.iter().filter(|i| i.selected).count(), 1);
    }
    for id in ["variant", "sample-size"] {
        let choice = |state: &UiState| {
            state.tool_options().into_iter().find_map(|o| match o {
                ToolOption::Choice { id: key, items, .. } if key == id => Some(items),
                _ => None,
            })
        };
        let item = choice(s.state()).unwrap().into_iter().find(|i| !i.selected).unwrap();
        let context = s.state().toolbar_context();
        s.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(item.action.clone()),
        })
        .unwrap();
        assert!(
            choice(s.state()).unwrap().iter().any(|i| i.selected && i.action == item.action),
            "published {id} choices are accepted toolbar edits"
        );
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
    s.dispatch(UiAction::Invoke {
        command: CommandId::AutoSelect,
    })
    .unwrap();
    let options = s.state().tool_options();
    assert!(options.iter().any(|o| matches!(
        o,
        ToolOption::Choice {
            id: "selection-source",
            ..
        }
    )));
    s.dispatch(UiAction::Invoke {
        command: CommandId::Brush,
    })
    .unwrap();
    assert!(ToolbarNumericBinding::BrushSize.field(s.state()).is_some());
}

#[test]
fn toolbar_drawer_measurement_contains_all_components_across_styles_and_widths() {
    let controls = [
        ToolbarControl::Color,
        ToolbarControl::BrushSizeSlider,
        ToolbarControl::BrushOpacitySlider,
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
                    .map(|i| ToolbarTile {
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
    let mut s = session();
    s.set_platform(Platform::Gtk);
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
    s.dispatch(UiAction::Customize {
        action: CustomizationAction::SetToolOptionsStyle {
            panel: Panel::Commands,
            tile,
            style,
        },
    })
    .unwrap();
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
    s.dispatch(UiAction::Invoke {
        command: CommandId::UndoWorkspace,
    })
    .unwrap();
    assert_eq!(s.state().workspace.layout, before);
    s.dispatch(UiAction::Invoke {
        command: CommandId::RedoWorkspace,
    })
    .unwrap();
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
fn vertical_options_shrink_before_reflowing_and_keep_more_accessible() {
    let tiles = vec![
        ToolbarTile {
            id: 1,
            control: ToolbarControl::Color,
        },
        ToolbarTile {
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
        let height = 3. * (h + 2.) + 22.;
        let layout = toolbar_tile_layout(w, height, Axis::Vertical, &tiles, true, style);
        assert_eq!(layout.tiles[0].x, layout.tiles[1].x);
        assert!(layout.tiles[1].height < 8. * h);
        assert!(layout.tiles[1].y + layout.tiles[1].height <= height - 22.);
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
    let mut s = session();
    s.set_platform(Platform::Gtk);
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
    s.dispatch(UiAction::Invoke {
        command: CommandId::UndoWorkspace,
    })
    .unwrap();
    assert_eq!(s.state().workspace.layout, layout);
    s.dispatch(UiAction::Invoke {
        command: CommandId::RedoWorkspace,
    })
    .unwrap();
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
        .map(|(i, control)| ToolbarTile {
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
            let width = columns as f32 * (w + 2.) - 2.;
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
    let mut fields = Vec::new();
    for brush in brush_catalog() {
        fields.extend(tool_settings::controls(&layer_core::default_brush(
            tools::preset(brush.id).unwrap(),
        )));
    }
    fields.extend(region_tools::RegionTools::default().controls());
    for constraint in [
        SelectionConstraint::Free,
        SelectionConstraint::Ratio,
        SelectionConstraint::Size,
    ] {
        let options = SelectionOptions {
            constraint,
            ..Default::default()
        };
        fields.extend(options.controls());
        fields.extend(options.edge_controls());
    }
    let bank = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/layer-web/icons");
    for id in fields.iter().map(|f| f.id).chain([
        "transform_x",
        "transform_y",
        "transform_width",
        "transform_height",
        "transform_angle",
    ]) {
        let icon = tool_setting_icon(id);
        assert_ne!(icon, "settings", "{id} needs a meaningful icon");
        assert!(ui_catalog().icons.contains(&icon));
        assert!(bank.join(format!("layer-{icon}-symbolic.svg")).is_file());
    }
}

#[test]
fn toolbar_choices_preserve_segmented_modes_and_list_sources() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    s.dispatch(UiAction::Invoke {
        command: CommandId::AutoSelect,
    })
    .unwrap();
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
    for item in items {
        s.dispatch(UiAction::ToolbarEdit {
            context: s.state().toolbar_context(),
            action: Box::new(item.action.clone()),
        })
        .unwrap();
        let current = s.state().tool_options();
        let current = current
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
        assert!(mode.same_schema(current));
        let ToolOption::Choice {
            items: selected, ..
        } = current
        else {
            panic!()
        };
        assert_eq!(selected.iter().filter(|i| i.selected).count(), 1);
        assert!(
            selected
                .iter()
                .any(|i| i.selected && i.action == item.action)
        );
    }
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
fn slider_bookmarks_round_trip_follow_presets_and_reject_stale_editors() {
    let mut s = session();
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
}

#[test]
fn slider_bookmark_taps_are_nearby_bounded_and_choose_the_closest_mark() {
    for (control, value, neighbor) in [
        (ToolbarControl::BrushSizeSlider, 64., 80.),
        (ToolbarControl::BrushOpacitySlider, 0.5, 0.54),
    ] {
        let numeric = control.slider().unwrap().numeric();
        let fill = |v| numeric.resolve(v, NumericOperation::Format).unwrap().fill;
        let mark = fill(value as f64);
        for travel in [140., 200., 400.] {
            for sign in [-1., 1.] {
                let near = mark + sign * 16. / travel;
                assert_eq!(
                    slider_bookmark_value(control, &[value], near, travel).unwrap(),
                    value as f64
                );
                let far = mark + sign * 24. / travel;
                assert_ne!(
                    slider_bookmark_value(control, &[value], far, travel).unwrap(),
                    value as f64
                );
                // The same nearby position remains unsnapped during a drag.
                assert_ne!(
                    slider_bookmark_value(control, &[], near, travel).unwrap(),
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
            )
            .unwrap(),
            neighbor as f64
        );
        // A very short track must not turn most of its range into a tap target.
        assert_eq!(
            slider_bookmark_value(control, &[value], mark + 0.14, 40.).unwrap(),
            value as f64
        );
        assert_ne!(
            slider_bookmark_value(control, &[value], mark + 0.16, 40.).unwrap(),
            value as f64
        );
        for travel in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(slider_bookmark_value(control, &[value], mark, travel).is_err());
        }
        for value in [numeric.min as f32, numeric.max as f32] {
            let position = if value == numeric.min as f32 {
                0.01
            } else {
                0.99
            };
            assert_eq!(
                slider_bookmark_value(control, &[value], position, 200.).unwrap(),
                value as f64
            );
        }
    }
}

#[test]
fn slider_preview_geometry_opacity_and_tip_raster_are_shared() {
    let s = session();
    let stamp = s.toolbar_stamp(s.state().toolbar_context()).unwrap();
    assert_eq!(stamp.alpha.len(), (stamp.size * stamp.size) as usize);
    assert!(stamp.alpha.iter().any(|&a| a > 0));
    assert_eq!(stamp.alpha[0], 0);
    let size = slider_preview_layout(ToolbarControl::BrushSizeSlider, 64., 180., 1.).unwrap();
    assert_eq!(size.stamp.width, 64.);
    assert_eq!(size.stamp.y, size.stamp.x);
    assert_eq!(size.viewport, Bounds { x: 0., y: 0., width: 180., height: 180. });
    assert!(size.header_fade > 32.);
    assert_eq!(size.opacity, 1.);
    assert_eq!(size.text, "Size: 64 px");
    let opacity =
        slider_preview_layout(ToolbarControl::BrushOpacitySlider, 0.42, 180., 1.).unwrap();
    assert_eq!(opacity.text, "Opacity: 42 %");
    assert_eq!(opacity.opacity, 0.42);
    assert_eq!(
        opacity.stamp,
        slider_preview_layout(ToolbarControl::BrushOpacitySlider, 1., 180., 1.)
            .unwrap()
            .stamp
    );
    assert!(slider_bookmark_value(ToolbarControl::BrushSizeSlider, &[], f64::NAN, 200.).is_err());
    assert!(
        slider_preview_layout(ToolbarControl::BrushSizeSlider, f32::INFINITY, 180., 1.).is_err()
    );
    let invalid = r#"{"size":[64,32],"opacity":[]}"#;
    assert!(
        serde_json::from_str::<SliderBookmarks>(invalid)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn slider_stamp_preserves_mask_holes_and_brush_grain() {
    let mut s = session();
    let mut brush = layer_core::BrushSnapshot::default();
    brush.aspect = 1.;
    brush.angle_radians = 0.;
    s.engine.set_brush(brush.clone()).unwrap();
    let context = s.state().toolbar_context();
    let plain = s.toolbar_stamp(context).unwrap();
    let center = (plain.size * plain.size / 2 + plain.size / 2) as usize;
    assert_eq!(plain.alpha[center], 255);
    brush.tip = layer_core::BrushTip::Mask(AssetId::from("preview-test"));
    s.engine.set_brush(brush.clone()).unwrap();
    let mask = s.toolbar_stamp(context).unwrap();
    assert_eq!(mask.alpha[center], 0, "the tip's hole survives the preview");
    assert!(mask.alpha.iter().any(|&a| a > 0));
    brush.tip = layer_core::BrushTip::AnalyticEllipse;
    brush.grain = Some(layer_core::BrushGrain {
        asset: AssetId::from("preview-test"),
        behavior: layer_core::BrushGrainBehavior::Moving,
        scale: 1.,
        depth: 1.,
        rotation_radians: 0.,
        offset_jitter: 0.,
    });
    s.engine.set_brush(brush).unwrap();
    let grain = s.toolbar_stamp(context).unwrap();
    assert_eq!(grain.alpha[center], 0);
    assert!(grain.alpha.iter().any(|&a| a > 0));
    assert_ne!(grain.alpha, plain.alpha);
}
