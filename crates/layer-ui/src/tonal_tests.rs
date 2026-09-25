mod tonal_checks {
    use super::*;
    use layer_core::{Selection, SelectionTarget};
    use layer_render::{RegionResult, RegionSource, TonalSample};
    use std::sync::Arc;
    fn start() -> UiSession<Recorder> {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        s
    }
    fn choose(s: &mut UiSession<Recorder>, index: usize) {
        s.dispatch(UiAction::Tonal {
            action: TonalAction::Preset { index },
        })
        .unwrap();
    }
    fn reply(s: &mut UiSession<Recorder>, sample: Option<TonalSample>, word: u32) {
        s.frame(1, 1).unwrap();
        let id = s.renderer_mut().region_requests.last().unwrap().request_id;
        s.renderer_mut().region_reply = Some(RegionResult {
            request_id: id,
            tonal_sample: sample,
            pixels: Arc::new(
                layer_core::SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![word]).unwrap(),
            ),
        });
        s.frame(2, 2).unwrap();
    }
    fn setting(s: &mut UiSession<Recorder>, id: &str, value: f32) {
        s.dispatch(UiAction::SetToolSetting {
            id: id.into(),
            value,
        })
        .unwrap();
    }
    #[test]
    fn tonal_direct_selection_refines_one_history_entry_with_fixed_baseline() {
        let mut s = start();
        invoke(&mut s, CommandId::SelectAll);
        let baseline = s.engine.document().selection.clone();
        invoke(&mut s, CommandId::TonalSelect);
        assert!(
            s.tonal_tools.draft.is_none(),
            "opening a tool must not change the selection"
        );
        assert_eq!(s.engine.document().selection, baseline);
        invoke(&mut s, CommandId::SelectionIntersect);
        assert!(s.tonal_tools.draft.is_none());
        choose(&mut s, 0);
        reply(&mut s, None, 0xff804020);
        assert_ne!(s.engine.document().selection, baseline, "no Apply required");
        assert!(!s.renderer_mut().overlay.unwrap().active);
        setting(&mut s, "tonal_softness", 0.5);
        reply(&mut s, None, 0xff806010);
        setting(&mut s, "selection_feather", 4.);
        reply(&mut s, None, 0xff906010);
        let request = s.renderer_mut().region_requests.last().unwrap();
        assert_eq!(
            request.selection.as_ref().unwrap().previous.as_deref(),
            baseline.as_ref()
        );
        assert_eq!(
            request.selection.as_ref().unwrap().mode,
            SelectionMode::Intersect
        );
        assert_eq!(request.selection.as_ref().unwrap().feather, 4.);
        let result = s.engine.document().selection.clone();
        assert_eq!(s.engine.display_selection().as_deref(), result.as_ref());
        invoke(&mut s, CommandId::Brush);
        assert_eq!(s.engine.document().selection, result);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().selection, baseline);
        invoke(&mut s, CommandId::Redo);
        assert_eq!(s.engine.document().selection, result);
    }
    #[test]
    fn tonal_new_choice_adds_to_existing_selection_and_has_own_undo() {
        let mut s = start();
        invoke(&mut s, CommandId::TonalSelect);
        choose(&mut s, 0);
        reply(&mut s, None, 0xff000000);
        let first = s.engine.document().selection.clone();
        invoke(&mut s, CommandId::SelectionAdd);
        choose(&mut s, 4);
        reply(&mut s, None, 0xff0000ff);
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
            first.as_ref()
        );
        setting(&mut s, "tonal_softness", 0.25);
        reply(&mut s, None, 0xff000080);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().selection, first);
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().selection.is_none());
    }
    #[test]
    fn tonal_quick_mask_preserves_result_and_always_samples_visible_artwork() {
        let mut s = start();
        invoke(&mut s, CommandId::TonalSelect);
        choose(&mut s, 4);
        reply(&mut s, None, 0xff804020);
        let selected = s.engine.document().selection.clone();
        for quick in [true, false, true] {
            invoke(&mut s, CommandId::QuickMask);
            s.frame(2, 2).unwrap();
            assert!(s.tonal_active());
            assert_eq!(s.selection_masks.quick(), quick);
            assert_eq!(s.engine.document().selection, selected);
            assert_eq!(s.renderer_mut().overlay.unwrap().active, quick);
        }
        setting(&mut s, "tonal_softness", 0.2);
        reply(&mut s, None, 0xff806010);
        let RegionSource::Tonal(t) = &s.renderer_mut().region_requests.last().unwrap().source
        else {
            panic!("tone")
        };
        assert_eq!(t.source, RegionSource::Composite);
        assert!(!t.invert);
        assert_eq!(t.bands[0].falloff, [0.1; 2]);
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().selection.is_none());
    }
    #[test]
    fn tonal_saved_destination_is_immediate_and_preserves_current_selection() {
        let mut s = start();
        invoke(&mut s, CommandId::SelectAll);
        let current = s.engine.document().selection.clone();
        invoke(&mut s, CommandId::TonalSelect);
        invoke(&mut s, CommandId::NewSelectionLayer);
        assert!(s.tonal_active());
        assert!(s.tonal_tools.draft.is_none());
        let target = s.selection_masks.target().unwrap();
        let before = s.mask_coverage(target).unwrap();
        let SelectionTarget::Saved(id) = target else {
            panic!("saved")
        };
        choose(&mut s, 0);
        reply(&mut s, None, 0xff804020);
        assert_ne!(s.mask_coverage(target).unwrap(), before);
        assert_eq!(s.renderer_mut().overlay.unwrap().editing, Some(id));
        setting(&mut s, "tonal_softness", 0.);
        reply(&mut s, None, 0xffff0000);
        let mask = s.mask_coverage(target).unwrap();
        assert_eq!(s.engine.document().selection, current);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.mask_coverage(target).unwrap(), before);
        invoke(&mut s, CommandId::Redo);
        assert_eq!(s.mask_coverage(target).unwrap(), mask);
        invoke(&mut s, CommandId::ReturnToArtwork);
        assert_eq!(s.engine.document().selection, current);
        assert!(s.tonal_tools.draft.is_none());
    }
    #[test]
    fn tonal_custom_sampling_and_toolbar_choices_share_simple_controls() {
        let mut s = start();
        invoke(&mut s, CommandId::TonalSelect);
        assert_eq!(
            s.state
                .tool_settings
                .iter()
                .map(|f| f.id)
                .collect::<Vec<_>>(),
            ["tonal_softness", "selection_feather"]
        );
        assert!(s.state.tool_settings.iter().all(|f| f.group.is_empty()));
        assert_eq!(s.state.tool_actions.len(), 4);
        assert!(
            matches!(s.state.tool_extra.as_slice(),[ToolOption::Choice {id:"tonal-tones",items,..}] if items.len()==8 && items.iter().all(|i|!i.selected))
        );
        let context = s.state.toolbar_context();
        s.dispatch(UiAction::ToolbarEdit {
            context,
            action: Box::new(UiAction::Tonal {
                action: TonalAction::Preset { index: 7 },
            }),
        })
        .unwrap();
        reply(&mut s, None, 0xff804020);
        assert_eq!(s.state.tool_settings.len(), 4);
        setting(&mut s, "tonal_lower", -4.);
        reply(&mut s, None, 0xff804020);
        assert!(
            s.state
                .tool_extra
                .iter()
                .all(|e| s.state.tool_options().contains(e))
        );
        let before = s.engine.document().selection.clone();
        s.interaction.modifiers.shift = true;
        let mut e = event(&s, 1, PenPhase::Down, 1.);
        let m = s.state.camera.document_to_surface();
        e.surface_position = Point {
            x: m[0] * 80. + m[2] * 80. + m[4],
            y: m[1] * 80. + m[3] * 80. + m[5],
        };
        s.pen(e).unwrap();
        s.interaction.modifiers = Default::default();
        e.phase = PenPhase::Up;
        s.pen(e).unwrap();
        reply(
            &mut s,
            Some(TonalSample {
                stops: [2.; 2],
                count: 25,
            }),
            0,
        );
        assert_eq!(s.selection_tools.options.tonal.custom, [0.75, 3.25]);
        reply(&mut s, None, 0xff800080);
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
        assert_eq!(
            s.effective_selection_mode(),
            SelectionMode::New,
            "sample modifiers do not become the next operation's mode"
        );
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().selection, before);
        let capture = s.capture_workspace().unwrap();
        PreparedWorkspace::new(
            serde_json::from_str(&serde_json::to_string(&capture).unwrap()).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn tonal_late_results_do_not_overwrite_other_edits_or_tool_changes() {
        let mut s = start();
        invoke(&mut s, CommandId::TonalSelect);
        choose(&mut s, 4);
        s.frame(1, 1).unwrap();
        let context = s.state.toolbar_context();
        invoke(&mut s, CommandId::Brush);
        reply(&mut s, None, 0xffffffff);
        assert!(s.engine.document().selection.is_none());
        assert!(
            s.dispatch(UiAction::ToolbarEdit {
                context,
                action: Box::new(UiAction::Tonal {
                    action: TonalAction::Preset { index: 0 }
                })
            })
            .is_err()
        );
        invoke(&mut s, CommandId::TonalSelect);
        choose(&mut s, 0);
        reply(&mut s, None, 0xff000000);
        setting(&mut s, "tonal_softness", 0.2);
        s.frame(1, 1).unwrap();
        s.engine
            .apply_edit(layer_core::Edit::SetSelection(Some(Selection::empty())))
            .unwrap();
        let all = s.engine.document().selection.clone();
        reply(&mut s, None, 0);
        assert_eq!(s.engine.document().selection, all);
    }
    #[test]
    fn tonal_legacy_workspace_and_invalid_controls() {
        let old = serde_json::json!({"bands":layer_core::tonal::TonalBand::defaults(),"active":4,"enabled":[false,false,false,false,true,false,false],"invert":true,"linked":false,"softness":0.4});
        let options: TonalOptions = serde_json::from_value(old).unwrap();
        options.validate().unwrap();
        assert_eq!(options.tone, 4);
        assert_eq!(options.softness, 0.4);
        let mut options = TonalOptions::default();
        assert!(options.edit("tonal_lower", 0.).is_err());
        for value in [f32::NAN, -1., 3.] {
            assert!(options.edit("tonal_softness", value).is_err());
        }
        options.tone = 7;
        options.edit("tonal_lower", 2.).unwrap();
        assert_eq!(options.custom, [2., 2.]);
        options.edit("tonal_upper", -2.).unwrap();
        assert_eq!(options.custom, [-2., -2.]);
        for command in [
            CommandId::ApplyTonalSelection,
            CommandId::TonalDetails,
            CommandId::TonalInvert,
        ] {
            assert!(!command.available_on(Platform::Gtk));
        }
    }
}
