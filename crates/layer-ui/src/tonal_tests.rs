mod tonal_checks {
    use super::*;
    use layer_render::{RegionResult, RegionSource, TonalSample};
    use std::sync::Arc;
    fn start() -> UiSession<Recorder> {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        s
    }
    fn reply(s: &mut UiSession<Recorder>, sample: Option<TonalSample>) -> u64 {
        s.frame(1, 1).unwrap();
        let id = s.renderer_mut().region_requests.last().unwrap().request_id;
        s.renderer_mut().region_reply = Some(RegionResult {
            request_id: id,
            tonal_sample: sample,
            pixels: Arc::new(
                layer_core::SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![0xff804020]).unwrap(),
            ),
        });
        s.frame(2, 2).unwrap();
        id
    }
    #[test]
    fn tonal_preview_has_fixed_baseline_single_undo_and_cancel() {
        let mut s = start();
        invoke(&mut s, CommandId::SelectAll);
        let baseline = s.engine.document().selection.clone();
        invoke(&mut s, CommandId::TonalSelect);
        assert!(s.tonal_tools.draft.is_some());
        assert!(!s.command(CommandId::ApplyTonalSelection).enabled);
        reply(&mut s, None);
        assert_eq!(
            s.engine.document().selection,
            baseline,
            "preview must not edit document"
        );
        assert!(s.command(CommandId::ApplyTonalSelection).enabled);
        assert_eq!(
            s.engine.display_selection().as_deref(),
            s.tonal_tools.preview.as_ref(),
            "completed preview is published in the completion frame"
        );
        invoke(&mut s, CommandId::SelectionIntersect);
        s.dispatch(UiAction::SetToolSetting {
            id: "tonal_lower".into(),
            value: 1.,
        })
        .unwrap();
        reply(&mut s, None);
        assert_eq!(
            s.renderer_mut()
                .region_requests
                .last()
                .unwrap()
                .selection
                .as_ref()
                .unwrap()
                .previous
                .as_deref(),
            baseline.as_ref()
        );
        s.dispatch(UiAction::SetToolSetting {
            id: "selection_feather".into(),
            value: 5.,
        })
        .unwrap();
        reply(&mut s, None);
        assert_eq!(
            s.renderer_mut()
                .region_requests
                .last()
                .unwrap()
                .selection
                .as_ref()
                .unwrap()
                .feather,
            5.
        );
        assert_eq!(s.engine.document().selection, baseline);
        invoke(&mut s, CommandId::ApplyTonalSelection);
        let result = s.engine.document().selection.clone();
        assert_ne!(result, baseline);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().selection, baseline);
        invoke(&mut s, CommandId::Redo);
        assert_eq!(s.engine.document().selection, result);
        s.dispatch(UiAction::SetToolSetting {
            id: "tonal_lower".into(),
            value: 2.,
        })
        .unwrap();
        s.frame(3, 3).unwrap();
        let id = s.renderer_mut().region_requests.last().unwrap().request_id;
        invoke(&mut s, CommandId::CancelTonalSelection);
        s.renderer_mut().region_reply = Some(RegionResult {
            request_id: id,
            tonal_sample: None,
            pixels: Arc::new(
                layer_core::SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![0xffffffff]).unwrap(),
            ),
        });
        s.frame(4, 4).unwrap();
        assert_eq!(s.engine.document().selection, result);
        assert!(!s.tonal_tools.ready);
    }
    #[test]
    fn tonal_custom_band_sampling_settings_and_shared_options() {
        let mut s = start();
        invoke(&mut s, CommandId::TonalSelect);
        reply(&mut s, None);
        let factory = s.selection_tools.options.tonal.bands[..7].to_vec();
        let context = s.state.toolbar_context();
        s.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(UiAction::Tonal {
                action: TonalAction::ToggleBand { index: 1 },
            }),
        })
        .unwrap();
        reply(&mut s, None);
        s.dispatch(UiAction::SetToolSetting {
            id: "tonal_lower".into(),
            value: -4.5,
        })
        .unwrap();
        assert_eq!(s.selection_tools.options.tonal.bands[..7], factory);
        assert_eq!(s.selection_tools.options.tonal.active, 7);
        assert!(!s.selection_tools.options.tonal.enabled[1]);
        assert!(s.selection_tools.options.tonal.enabled[4]);
        assert!(s.selection_tools.options.tonal.enabled[7]);
        s.dispatch(UiAction::SetToolText {
            id: "tonal-name".into(),
            value: "Window detail".into(),
        })
        .unwrap();
        invoke(&mut s, CommandId::TonalSaveBand);
        assert_eq!(s.state.settings.tonal_bands[0].name, "Window detail");
        let saved: Settings =
            serde_json::from_str(&serde_json::to_string(&s.state.settings).unwrap()).unwrap();
        saved.validate().unwrap();
        assert_eq!(saved.tonal_bands, s.state.settings.tonal_bands);
        let capture = s.capture_workspace().unwrap();
        PreparedWorkspace::new(
            serde_json::from_str(&serde_json::to_string(&capture).unwrap()).unwrap(),
        )
        .unwrap();
        assert!(
            s.state
                .tool_extra
                .iter()
                .all(|e| s.state.tool_options().contains(e))
        );
        // Sample a HDR point: preserve the edited one-stop width, globally.
        reply(&mut s, None);
        for phase in [PenPhase::Down, PenPhase::Up] {
            let mut e = event(&s, 1, phase, 1.);
            let m = s.state.camera.document_to_surface();
            e.surface_position = Point {
                x: m[0] * 80. + m[2] * 80. + m[4],
                y: m[1] * 80. + m[3] * 80. + m[5],
            };
            s.pen(e).unwrap();
        }
        s.frame(5, 5).unwrap();
        let request = s.renderer_mut().region_requests.last().unwrap();
        let RegionSource::Tonal(t) = &request.source else {
            panic!("tonal request")
        };
        assert!(t.probe.unwrap().point);
        assert_eq!(request.limit, None);
        reply(
            &mut s,
            Some(TonalSample {
                stops: [2.; 2],
                count: 25,
            }),
        );
        let b = &s.selection_tools.options.tonal.bands[7];
        assert_eq!([b.lower, b.upper], [Some(1.5), Some(2.5)]);
        assert!(s.engine.document().selection.is_none());
        invoke(&mut s, CommandId::Brush);
        reply(&mut s, None);
        assert!(s.engine.document().selection.is_none());
    }
    #[test]
    fn tonal_stale_edits_and_unsupported_platforms() {
        let mut s = start();
        invoke(&mut s, CommandId::TonalSelect);
        let context = s.state.toolbar_context();
        invoke(&mut s, CommandId::Brush);
        assert!(
            s.dispatch(UiAction::ToolbarEdit {
                context,
                action: Box::new(UiAction::Tonal {
                    action: TonalAction::ToggleBand { index: 0 }
                })
            })
            .is_err()
        );
        for platform in [
            Platform::Web,
            Platform::Android,
            Platform::Mac,
            Platform::Windows,
        ] {
            assert!(!CommandId::TonalSelect.available_on(platform));
        }
        assert!(
            s.selection_tools
                .options
                .tonal
                .edit("tonal_lower", f32::NAN)
                .is_err()
        );
    }
    #[test]
    fn tonal_sampling_latches_modifiers_across_async_preview() {
        let mut s = start();
        invoke(&mut s, CommandId::SelectAll);
        invoke(&mut s, CommandId::TonalSelect);
        reply(&mut s, None);
        s.interaction.modifiers.shift = true;
        let p = Point { x: 80., y: 80. };
        let mut e = event(&s, 1, PenPhase::Down, 1.);
        let m = s.state.camera.document_to_surface();
        e.surface_position = Point {
            x: m[0] * p.x + m[2] * p.y + m[4],
            y: m[1] * p.x + m[3] * p.y + m[5],
        };
        s.pen(e).unwrap();
        s.interaction.modifiers = Default::default();
        e.phase = PenPhase::Up;
        s.pen(e).unwrap();
        reply(
            &mut s,
            Some(TonalSample {
                stops: [-2.; 2],
                count: 25,
            }),
        );
        s.dispatch(UiAction::SetToolSetting {
            id: "tonal_falloff_low".into(),
            value: 0.75,
        })
        .unwrap();
        reply(&mut s, None);
        assert_eq!(
            s.renderer_mut()
                .region_requests
                .last()
                .unwrap()
                .selection
                .as_ref()
                .unwrap()
                .mode,
            SelectionMode::Add
        );
        assert!(s.command(CommandId::SelectionAdd).selected);
        invoke(&mut s, CommandId::ApplyTonalSelection);
        assert_eq!(s.selection_tools.options.mode, SelectionMode::New);
        assert!(s.command(CommandId::SelectionNew).selected);
    }
    #[test]
    fn tonal_renaming_disabled_preset_preserves_mask_recipe() {
        let mut options = TonalOptions::default();
        options.active = 0;
        options.rename("Dark detail".into()).unwrap();
        assert!(!options.enabled[0] && !options.enabled[7]);
        assert!(options.enabled[4]);
        options.validate().unwrap();
        let before = options.clone();
        assert!(options.edit("tonal_upper", f32::INFINITY).is_err());
        assert_eq!(options, before);
    }
    #[test]
    fn tonal_quick_mask_switch_preserves_preview_and_source() {
        let mut s = start();
        let artwork = s.engine.document().active_layer;
        invoke(&mut s, CommandId::TonalSelect);
        reply(&mut s, None);
        assert!(
            !s.renderer_mut().overlay.unwrap().active,
            "ordinary selection uses marching ants"
        );
        s.dispatch(UiAction::Tonal {
            action: TonalAction::Source {
                layer: Some(artwork.0),
            },
        })
        .unwrap();
        reply(&mut s, None);
        let preview = s.tonal_tools.preview.clone();
        for quick in [true, false, true] {
            invoke(&mut s, CommandId::QuickMask);
            assert_eq!(s.selection_masks.quick(), quick);
            assert!(s.tonal_active());
            assert_eq!(
                s.state
                    .commands
                    .iter()
                    .find(|c| c.id == CommandId::ApplyTonalSelection)
                    .unwrap()
                    .label,
                if quick {
                    "Apply mask"
                } else {
                    "Apply selection"
                }
            );
            assert!(
                s.tonal_tools.ready,
                "quick={quick} draft={} docrev={}",
                s.tonal_tools.draft.as_ref().map_or(999, |d| d.revision),
                s.engine.document().revision
            );
            assert_eq!(s.tonal_tools.preview, preview);
            assert_eq!(s.tonal_tools.source, Some(artwork));
            s.frame(2, 2).unwrap();
            assert_eq!(s.renderer_mut().overlay.unwrap().active, quick);
        }
        invoke(&mut s, CommandId::ApplyTonalSelection);
        assert_eq!(s.engine.document().selection, preview);
        assert!(s.selection_masks.quick());
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().selection.is_none());
    }
    #[test]
    fn tonal_saved_mask_is_destination_and_never_sampling_source() {
        let mut s = start();
        invoke(&mut s, CommandId::SelectAll);
        let current = s.engine.document().selection.clone();
        invoke(&mut s, CommandId::NewSelectionLayer);
        let target = s.selection_masks.target().unwrap();
        let layer_core::SelectionTarget::Saved(id) = target else {
            panic!("saved mask")
        };
        let before = s.mask_coverage(target).unwrap();
        invoke(&mut s, CommandId::TonalSelect);
        assert_eq!(s.selection_masks.target(), Some(target));
        reply(&mut s, None);
        let request = s.renderer_mut().region_requests.last().unwrap();
        let RegionSource::Tonal(t) = &request.source else {
            panic!("tonal request")
        };
        assert_eq!(t.source, RegionSource::Composite);
        assert_eq!(
            request.selection.as_ref().unwrap().previous.as_deref(),
            Some(&before)
        );
        let overlay = s.renderer_mut().overlay.unwrap();
        assert!(overlay.active);
        assert_eq!(overlay.editing, Some(id));
        let preview = s.tonal_tools.preview.clone().unwrap();
        assert_eq!(
            s.mask_coverage(target).unwrap(),
            before,
            "draft leaves history untouched"
        );
        invoke(&mut s, CommandId::ApplyTonalSelection);
        assert_eq!(s.mask_coverage(target).unwrap(), preview);
        assert_eq!(s.engine.document().selection, current);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.mask_coverage(target).unwrap(), before);
        invoke(&mut s, CommandId::Redo);
        assert_eq!(s.mask_coverage(target).unwrap(), preview);
        assert!(
            s.dispatch(UiAction::Tonal {
                action: TonalAction::Source { layer: Some(id.0) }
            })
            .is_err()
        );
        s.dispatch(UiAction::SetToolSetting {
            id: "tonal_lower".into(),
            value: -1.,
        })
        .unwrap();
        reply(&mut s, None);
        invoke(&mut s, CommandId::CancelTonalSelection);
        assert_eq!(s.mask_coverage(target).unwrap(), preview);
        assert_eq!(s.engine.document().selection, current);
    }
    #[test]
    fn tonal_simple_controls_and_global_softness_preserve_named_ranges() {
        let mut s = start();
        invoke(&mut s, CommandId::TonalSelect);
        reply(&mut s, None);
        assert_eq!(
            s.state
                .tool_settings
                .iter()
                .map(|f| f.id)
                .collect::<Vec<_>>(),
            vec!["tonal_softness", "selection_feather"]
        );
        assert!(
            !s.state
                .tool_extra
                .iter()
                .any(|o| matches!(o, ToolOption::Text { .. }))
        );
        assert!(
            !s.state
                .tool_extra
                .iter()
                .any(|o| matches!(o,ToolOption::Info {text,..} if text.len()>32))
        );
        let factory = s.selection_tools.options.tonal.bands.clone();
        s.dispatch(UiAction::Tonal {
            action: TonalAction::ToggleBand { index: 0 },
        })
        .unwrap();
        s.dispatch(UiAction::SetToolSetting {
            id: "tonal_softness".into(),
            value: 0.25,
        })
        .unwrap();
        reply(&mut s, None);
        let RegionSource::Tonal(request) = &s.renderer_mut().region_requests.last().unwrap().source
        else {
            panic!("tone")
        };
        assert_eq!(request.bands.len(), 2);
        assert!(request.bands.iter().all(|b| b.falloff == [0.125; 2]));
        assert_eq!(s.selection_tools.options.tonal.bands, factory);
        invoke(&mut s, CommandId::TonalDetails);
        assert!(s.state.tool_settings.iter().any(|f| f.id == "tonal_upper"));
        assert!(
            s.state
                .tool_extra
                .iter()
                .any(|o| matches!(o, ToolOption::Text { .. }))
        );
        invoke(&mut s, CommandId::TonalDetails);
        assert_eq!(s.state.tool_settings.len(), 2);
    }
}
