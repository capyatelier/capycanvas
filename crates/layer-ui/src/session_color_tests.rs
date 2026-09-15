// Included in session::tests, using the protocol recorder (no simulated pixels).
#[test]
fn color_transitions_update_picker_coordinates_and_route_exact_history_through_the_host() {
    use layer_core::{ColorTransition, color::{DocumentColor, IntegerDepth, RgbColor, RgbSpace}};
    let mut s = session();
    s.set_platform(Platform::Gtk);
    let definition = RgbColor::new(RgbSpace::DisplayP3, [0.8, 0.3, 0.1, 1.]).unwrap();
    s.dispatch(UiAction::Color { action: ColorAction::Definition { color: definition } }).unwrap();
    s.frame(1, 1).unwrap();
    let before = s.engine.document().clone();
    let color = DocumentColor { space: RgbSpace::ProPhoto, depth: IntegerDepth::U16 };
    let prepare = |s: &UiSession<Recorder>| s.prepare_document_color_transition(ColorTransition::Apply {
        color, layers: s.engine.document().layers.clone(),
    }).unwrap().0;
    let prepared = prepare(&s);
    assert!(s.commit_document_color_transition(prepared).unwrap_err().contains("not ready"));
    assert_eq!(s.engine.document(), &before);
    assert_eq!(s.state.colors.rgb_space(), before.color.space);
    s.renderer_mut().prepared_color = Some(color);
    s.commit_document_color_transition(prepare(&s)).unwrap();
    assert_eq!(s.state.colors.rgb_space(), color.space);
    assert_eq!(s.state.colors.definition(), definition);
    for (a, b) in s.engine.configured_brush().color_rgba_linear.into_iter().zip(definition.linear_in(color.space).unwrap()) {
        assert!((a - b).abs() < 2e-7);
    }
    let after = s.engine.document().clone();
    for (redo, expected) in [(false, &before), (true, &after)] {
        s.dispatch(UiAction::Invoke { command: if redo { CommandId::Redo } else { CommandId::Undo } }).unwrap();
        let request = s.state.requests.first().unwrap();
        let id = request.id;
        assert!(matches!(request.kind, HostRequestKind::Document { request: DocumentRequest::ColorHistory { redo: value } } if value == redo));
        let (prepared, project) = s.prepare_document_color_transition(if redo { ColorTransition::Redo } else { ColorTransition::Undo }).unwrap();
        assert_eq!(project.document.color, expected.color);
        s.renderer_mut().prepared_color = Some(expected.color);
        s.commit_document_color_transition(prepared).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        assert_eq!(s.engine.document().layers, expected.layers);
        assert_eq!(s.engine.document().color, expected.color);
        assert_eq!(s.state.colors.rgb_space(), expected.color.space);
        assert_eq!(s.state.colors.definition(), definition);
        assert!(!s.state.document_file.busy);
    }
}

#[test]
fn portable_colors_follow_documents_workspaces_brushes_and_samples() {
    use layer_core::color::{DocumentColor, IntegerDepth, RgbColor, RgbSpace};
    use layer_render::{ColorSample, ColorSampleSource};
    let definition = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 0.37]).unwrap();
    let make = |space| {
        let color = DocumentColor {
            space,
            depth: IntegerDepth::U16,
        };
        let mut document = Document::new("wide", 1000, 1000);
        document.color = color;
        UiSession::new(
            Recorder {
                color,
                ..Default::default()
            },
            document,
            [1000; 2],
        )
        .unwrap()
    };
    let mut s = make(RgbSpace::DisplayP3);
    s.dispatch(UiAction::Color {
        action: ColorAction::Definition { color: definition },
    })
    .unwrap();
    s.dispatch(UiAction::SetBrushOpacity { value: 0.23 })
        .unwrap();
    let capture = s.capture_workspace().unwrap();
    let encoded = serde_json::to_vec(&capture).unwrap();
    for space in RgbSpace::ALL {
        let candidate = Box::new(make(space));
        let epoch = s.state.document_file.epoch;
        let revision = s.engine.document().revision;
        s.adopt_project(candidate, epoch, revision, None)
            .map_err(|(e, _)| e)
            .unwrap();
        assert_eq!(s.state.colors.definition(), definition);
        assert_eq!(s.state.colors.rgb_space(), space);
        assert_eq!(
            s.engine.configured_brush().color_rgba_linear,
            definition.linear_in(space).unwrap()
        );
        assert_eq!(s.state.brush.opacity, 0.23);
        let prepared = PreparedWorkspace::new(serde_json::from_slice(&encoded).unwrap()).unwrap();
        s.adopt_workspace(prepared).unwrap();
        assert_eq!(s.state.colors.rgb_space(), space);
        assert_eq!(s.state.colors.definition(), definition);
        assert_eq!(
            s.engine.configured_brush().color_rgba_linear,
            definition.linear_in(space).unwrap()
        );
        for preset in [
            DefaultBrushPreset::GPen,
            DefaultBrushPreset::WetRound,
            DefaultBrushPreset::GPen,
        ] {
            s.select_brush(preset as u32).unwrap();
            assert_eq!(
                s.engine.configured_brush().color_rgba_linear,
                definition.linear_in(space).unwrap()
            );
            let secondary = default_brush(preset)
                .color_dynamics
                .secondary_color_rgba_linear;
            let expected = RgbColor::from_linear(RgbSpace::Srgb, secondary)
                .unwrap()
                .linear_in(space)
                .unwrap();
            for (a, b) in s
                .engine
                .configured_brush()
                .color_dynamics
                .secondary_color_rgba_linear
                .into_iter()
                .zip(expected)
            {
                assert!((a - b).abs() < 1e-7);
            }
        }
        // Sampling preserves extended straight document RGB; alpha controls
        // whether a sample exists, while paint opacity remains independent.
        let opacity = s.state.brush.opacity;
        s.eyedropper.queue(ColorSampleSource::Composite, [32, 32]);
        s.frame(1, 1).unwrap();
        let request = *s.renderer_mut().sample_requests.last().unwrap();
        s.renderer_mut().sample_reply = Some(ColorSample {
            request_id: request.request_id,
            rgba: [-0.1, 1.2, 0.4, 0.00001],
        });
        s.frame(2, 2).unwrap();
        assert_eq!(s.state.colors.definition().space, space);
        for (a, b) in s
            .engine
            .configured_brush()
            .color_rgba_linear
            .into_iter()
            .zip([-0.1, 1.2, 0.4, 1.])
        {
            assert!((a - b).abs() < 2e-7, "{space:?}: {a} != {b}");
        }
        assert_eq!(s.state.brush.opacity, opacity);
        assert_eq!(s.engine.document().revision, revision);
        s.dispatch(UiAction::Color {
            action: ColorAction::Definition { color: definition },
        })
        .unwrap();
    }
}

#[test]
fn figures_and_gradients_convert_both_portable_paints() {
    use layer_core::color::{DocumentColor, IntegerDepth, RgbColor, RgbSpace};
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    };
    let mut document = Document::new("wide", 1000, 1000);
    document.color = color;
    let mut s = UiSession::new(
        Recorder {
            color,
            ..Default::default()
        },
        document,
        [1000; 2],
    )
    .unwrap();
    let foreground = RgbColor::new(RgbSpace::DisplayP3, [1., 0.1, 0.3, 0.37]).unwrap();
    let background = RgbColor::new(RgbSpace::AdobeRgb, [0.1, 1., 0.3, 0.73]).unwrap();
    s.state.colors.foreground = foreground;
    s.state.colors.background = background;
    s.state.brush.opacity = 0.23;
    s.layer_interaction.tool = LayerCanvasTool::Figure {
        shape: FigureShape::Rectangle,
        paint: FigurePaint::Both,
    };
    s.layer_interaction.path = vec![Point { x: 10., y: 10. }, Point { x: 60., y: 60. }];
    let figure = s.current_figure().unwrap();
    let expected = [foreground, background].map(|c| {
        let mut rgba = c.linear_in(color.space).unwrap();
        rgba[3] *= 0.23;
        rgba
    });
    assert_eq!(figure.colors, expected);
    // Exercise the gradient through its public canvas-tool and contact path.
    s.cancel_layer_gesture().unwrap();
    s.dispatch(UiAction::Layer {
        action: LayerAction::Tool {
            tool: LayerCanvasTool::Gradient {
                radial: false,
                transparent: false,
            },
        },
    })
    .unwrap();
    let mut down = event(&s, 1, PenPhase::Down, 1.);
    down.surface_position = Point { x: 50., y: 50. };
    s.pen(down).unwrap();
    let mut up = event(&s, 2, PenPhase::Up, 1.);
    up.surface_position = Point { x: 250., y: 250. };
    s.pen(up).unwrap();
    s.frame(10, 10).unwrap();
    let operations = &s.renderer_mut().pending_operations;
    let actual = operations
        .iter()
        .find_map(|(_, operation)| {
            if let layer_core::LayerOperationKind::Gradient { colors, .. } = &operation.kind {
                Some(*colors)
            } else {
                None
            }
        })
        .expect("gradient operation");
    assert_eq!(actual, expected);
}
