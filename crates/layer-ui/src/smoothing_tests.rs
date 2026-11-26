#[test]
fn pointer_prediction_keeps_smoothing_and_restores_display_pen_lead_on_every_platform() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows, Platform::Mac, Platform::Ios] {
        for native in [false, true] {
            for strength in [0., 0.5] {
                let mut endpoints = Vec::new();
                for enabled in [false, true] {
                    let mut s = session(platform);
                    s.set_platform_prediction_available(native);
                    s.apply_settings(Settings { feedback: enabled, prediction_ms: 32., ..Default::default() }).unwrap();
                    s.dispatch(UiAction::SetToolSetting { id: "smoothing_strength".into(), value: strength }).unwrap();
                    let mut tips = Vec::new();
                    for (contact, (tool, indirect)) in [
                        (ToolKind::Mouse, false),
                        (ToolKind::Pen, true),
                        (ToolKind::Eraser, true),
                        (ToolKind::Pen, false),
                    ].into_iter().enumerate() {
                        let before = s.engine.metrics();
                        let mut last = event(&s, 1, PenPhase::Down, 1.);
                        for index in 0..101 {
                            last = PenEvent {
                                sequence: contact as u64 * 200 + index + 1,
                                timestamp_ns: 1_000_000_000 + contact as u64 * 2_000_000_000 + index * 4_000_000,
                                phase: if index == 0 { PenPhase::Down } else { PenPhase::Move },
                                surface_position: Point { x: 200. + index as f32 * 2., y: 300. },
                                tool,
                                flags: SampleFlags(SampleFlags::PRIMARY.0 | if indirect { SampleFlags::INDIRECT_POINTER.0 } else { 0 }),
                                ..last
                            };
                            s.pen(last);
                            if native && index > 0 {
                                s.pen(PenEvent {
                                    timestamp_ns: last.timestamp_ns + 32_000_000,
                                    phase: PenPhase::Move,
                                    surface_position: Point { x: last.surface_position.x + 16., ..last.surface_position },
                                    flags: SampleFlags(last.flags.0 | SampleFlags::PREDICTED.0),
                                    ..last
                                });
                            }
                            s.renderer_mut().recorded_dabs.clear();
                            s.frame(last.timestamp_ns, last.timestamp_ns + 32_000_000).unwrap();
                        }
                        let after = s.engine.metrics();
                        let uses_native = enabled && contact == 3 && native && !matches!(platform, Platform::Gtk | Platform::Mac);
                        assert_eq!(after.engine_prediction_frames > before.engine_prediction_frames, enabled && contact == 3 && !uses_native, "{platform:?}, native={native}");
                        assert_eq!(after.platform_prediction_frames > before.platform_prediction_frames, uses_native, "{platform:?}, native={native}");
                        let tip = s.renderer_mut().recorded_dabs.last().unwrap().center;
                        let [a, b, c, d, x, y] = s.state.camera.document_to_surface();
                        let tip = Point { x: a * tip.x + c * tip.y + x, y: b * tip.x + d * tip.y + y };
                        if contact < 3 {
                            assert!(tip.x <= last.surface_position.x + 0.1, "{tool:?}, strength={strength}, enabled={enabled}: {tip:?}");
                            if strength == 0. { assert!((tip.x - last.surface_position.x).abs() < 0.1); }
                        }
                        tips.push(tip.x);
                        s.pen(PenEvent { phase: PenPhase::Up, sequence: last.sequence + 1, timestamp_ns: last.timestamp_ns + 4_000_000, ..last });
                        s.frame(last.timestamp_ns + 100_000_000, last.timestamp_ns + 132_000_000).unwrap();
                        s.frame(last.timestamp_ns + 200_000_000, last.timestamp_ns + 232_000_000).unwrap();
                        assert_eq!(s.engine.metrics().committed_strokes, before.committed_strokes + 1);
                        assert_eq!(s.state.settings.feedback, enabled);
                        assert_eq!(s.state.settings.prediction_ms, 32.);
                    }
                    endpoints.push(tips);
                }
                if strength > 0. {
                    for contact in 0..3 {
                        assert!(endpoints[1][contact] > endpoints[0][contact] + 1., "smoothing preview: {endpoints:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn smoothing_controls_keep_per_brush_choices_and_restore_defaults() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows, Platform::Mac, Platform::Ios] {
        let mut s = session(platform);
        let pen = DefaultBrushPreset::GPen as u32;
        s.select_brush(pen).unwrap();
        assert_eq!(s.engine.configured_brush().stabilization.strength, 0.1);

        let control = s.state.tool_settings.iter().find(|c| c.id == "smoothing_strength").unwrap();
        assert_eq!(control.label.as_ref(), "Smoothing");
        assert!(control.group_id.is_empty() && control.group.is_empty());
        assert_eq!(control.numeric.scale, 100.);
        assert_eq!((control.numeric.min, control.numeric.max), (0., 1.));
        assert_eq!(control.numeric.compact_text(control.value as f64), "10");
        for (position, strength) in [(0., 0.), (0.25, 0.05), (0.5, 0.16), (0.75, 0.41), (1., 1.)] {
            let resolved = control.numeric.resolve(0., NumericOperation::Position { position }).unwrap();
            assert!((resolved.value - strength).abs() < 1e-10);
        }
        assert!((control.numeric.resolve(0.3, NumericOperation::Format).unwrap().fill - 0.6623519).abs() < 1e-6);
        assert!((control.numeric.resolve(control.value as f64, NumericOperation::Format).unwrap().fill - 0.3921368).abs() < 1e-6);
        assert_eq!(control.numeric.resolve(0.3, NumericOperation::Step { steps: 1. }).unwrap().text, "31");
        assert!((control.numeric.resolve(0., NumericOperation::Expression { text: "30".into() }).unwrap().value - 0.3).abs() < 1e-10);
        let serialized = serde_json::to_string(&control.numeric).unwrap();
        let restored: NumericControl = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored, control.numeric);
        s.dispatch(UiAction::SetToolSetting { id: "smoothing_strength".into(), value: 0.5 }).unwrap();

        assert!(!s.engine.can_undo());
        s.select_brush(DefaultBrushPreset::Pencil as u32).unwrap();
        assert_eq!(s.engine.configured_brush().stabilization.strength, 0.1);
        let saved = serde_json::to_vec(&s.editing_state()).unwrap();
        let mut reopened = session(platform);
        reopened.restore_editing(serde_json::from_slice(&saved).unwrap()).unwrap();
        reopened.select_brush(pen).unwrap();
        assert_eq!(reopened.engine.configured_brush().stabilization.strength, 0.5);

        reopened.dispatch(UiAction::ResetToolSetting { id: "smoothing_strength".into() }).unwrap();

        assert_eq!(reopened.engine.configured_brush().stabilization, crate::tools::defaults(DefaultBrushPreset::GPen).stabilization);
        assert!(reopened.tools.overrides.get(&pen).is_none_or(|values| !values.keys().any(|id| id.starts_with("smoothing_"))));
        reopened.dispatch(UiAction::Invoke { command: CommandId::RectangleSelect }).unwrap();
        assert!(!reopened.state.tool_settings.iter().any(|c| c.id == "smoothing_strength"));
        reopened.select_brush(pen).unwrap();
        reopened.dispatch(UiAction::Invoke { command: CommandId::QuickMask }).unwrap();

        assert!(!reopened.state.tool_settings.iter().any(|c| c.id == "smoothing_strength"));
        reopened.dispatch(UiAction::Invoke { command: CommandId::ReturnToArtwork }).unwrap();

        reopened.select_brush(DefaultBrushPreset::CloneStamp as u32).unwrap();
        assert!(reopened.state.tool_settings.iter().any(|field| field.id == "smoothing_strength"));
    }
}
