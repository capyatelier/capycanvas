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
fn toolbar_component_defaults_are_gtk_only_and_round_trip() {
    for preset in WorkspacePreset::ALL {
        let layout = preset.layout(Platform::Gtk);
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
                vec![
                    ToolbarControl::BrushSizeSlider,
                    ToolbarControl::BrushOpacitySlider
                ]
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
            Platform::Web,
            Platform::Android,
            Platform::Mac,
            Platform::Ios,
            Platform::Windows,
        ] {
            assert_eq!(
                preset.layout(platform),
                preset.legacy_toolbar_components_layout(platform)
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
                    assert!(b.width > 0. && b.height > 0. && b.x.is_finite() && b.y.is_finite());
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
            let parts = toolbar_slider_layout(size, size, axis, 40.);
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
            assert!(track >= 0., "tiny tracks become value-only launchers");
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
    s.dispatch(UiAction::SetColorSampleSize { width: 3 })
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
    assert!(ToolbarNumericBinding::BrushSize.field(s.state()).is_none());
    let context = s.state().toolbar_context();
    assert!(
        s.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(ToolbarNumericBinding::BrushSize.action(17.))
        })
        .is_err()
    );
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
    assert!(p.detached && p.connection().is_none());
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
