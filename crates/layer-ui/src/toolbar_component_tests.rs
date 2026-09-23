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
            assert_eq!(
                layout.group_edge(layout.panel_group(Panel::Commands).unwrap()),
                Some(Edge::Bottom)
            );
            assert_eq!(
                layout
                    .panel(Panel::Commands)
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
                ToolbarControl::ToolOptions
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
                    control: ToolbarControl::ToolOptions
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
        ToolbarControl::ToolOptions,
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
    let (_, fields, more) = tool_options_layout(330., 36., Axis::Horizontal, &[100., 100., 100.]);
    assert!(fields[0].is_some() && fields[1].is_some() && fields[2].is_none());
    assert!(more.x + more.width <= 330.);
    let (_, fields, more) = tool_options_layout(36., 36., Axis::Vertical, &[100.]);
    assert!(fields[0].is_none() && more.height > 0.);
    let duplicates = vec![
        ToolbarTile {
            id: 1,
            control: ToolbarControl::ToolOptions,
        },
        ToolbarTile {
            id: 2,
            control: ToolbarControl::ToolOptions,
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
                parts[2].width
            } else {
                parts[2].height
            };
            assert!(
                track == 0. || track >= 32.,
                "tiny tracks become value-only launchers"
            );
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
        ToolbarControl::ToolOptions,
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
        .find(|t| t.control == ToolbarControl::ToolOptions)
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
