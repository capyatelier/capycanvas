mod selection_refine_checks {
    use super::*;
    use layer_core::{Selection, SelectionPixels, SelectionShape, SelectionTarget};
    use layer_render::{RegionRequest, RegionResult, RegionSource};
    use std::sync::Arc;

    fn rectangle() -> Selection {
        Selection::polygon(vec![
            Point { x: 100., y: 100. },
            Point { x: 300., y: 100. },
            Point { x: 300., y: 300. },
            Point { x: 100., y: 300. },
        ])
        .unwrap()
    }
    fn soft() -> Selection {
        let words: Vec<u32> = (0..100 * 400).map(|i| if (25..75).contains(&(i % 100)) { 0x8080_8080 } else { 0 }).collect();
        Selection::pixels(Arc::new(SelectionPixels::bytes([400, 400], [100, 0, 300, 400], words).unwrap()))
    }
    fn select(s: &mut UiSession<Recorder>, selection: Selection) {
        s.layer_edit(layer_core::Edit::SetSelection(Some(selection))).unwrap();
        s.frame(1, 1).unwrap();
    }
    fn mask(word: u32) -> Selection {
        Selection::pixels(Arc::new(SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![word]).unwrap()))
    }
    fn selection_action(s: &mut UiSession<Recorder>, action: SelectionAction) {
        s.dispatch(UiAction::Selection { action }).unwrap();
    }
    const COARSE: layer_core::Affine = layer_core::Affine([2., 0., 0., 2., 10., 20.]);
    fn modify(request: &RegionRequest) -> &layer_render::SelectionModify {
        match &request.source {
            RegionSource::Modify(modify) => modify,
            _ => panic!("refinements modify a selection"),
        }
    }
    /// Answer the job in flight at `now` with `mask(word)`, placed on coarse
    /// cells unless the job is exact or `exact` asks for exact coverage.
    fn answer_at(s: &mut UiSession<Recorder>, word: u32, now: u64, exact: bool) -> RegionRequest {
        s.frame(now, now).unwrap();
        let request = s.renderer_mut().region_requests.last().unwrap().clone();
        let SelectionShape::Pixels(pixels) = mask(word).shape else { unreachable!() };
        let placement = if exact || modify(&request).preview.is_none() { layer_core::Affine::IDENTITY } else { COARSE };
        s.renderer_mut().region_reply = Some(RegionResult { request_id: request.request_id, tonal_sample: None, pixels, placement });
        s.frame(now + 1, now + 1).unwrap();
        request
    }
    fn answer(s: &mut UiSession<Recorder>, word: u32) -> RegionRequest {
        answer_at(s, word, 1, false)
    }
    fn preview(word: u32) -> Selection {
        Selection { affine: COARSE, ..mask(word) }
    }
    fn settle(s: &mut UiSession<Recorder>, now: u64) -> u64 {
        let later = now + super::selection_refine::SETTLE_NS + 1;
        s.frame(later, later).unwrap();
        later
    }
    fn check_steps(kind: RefineKind, radius: i32, request: &RegionRequest, original: &Selection) {
        let modify = modify(request);
        assert_eq!(modify.selection.as_ref(), original);
        let resize = |resize, chained| layer_render::ModifyStep { resize, chained, ..Default::default() };
        let expected = match kind {
            RefineKind::Grow => vec![resize(radius, false)],
            RefineKind::Shrink => vec![resize(-radius, false)],
            RefineKind::Feather => vec![layer_render::ModifyStep { feather: radius as f32, ..Default::default() }],
            RefineKind::Border => vec![resize(radius, false), layer_render::ModifyStep { subtract: true, ..resize(-radius, false) }],
            RefineKind::Smooth => vec![
                resize(radius, false),
                layer_render::ModifyStep { keep_canvas_edges: true, ..resize(-2 * radius, true) },
                resize(radius, true),
            ],
        };
        assert_eq!(modify.steps, expected, "{kind:?}");
    }

    #[test]
    fn each_refinement_previews_then_amends_one_step_that_cancel_withdraws() {
        for kind in RefineKind::ALL {
            let mut s = session(Platform::Gtk);
            select(&mut s, rectangle());
            let original = s.engine.document().selection.clone().unwrap();
            invoke(&mut s, kind.command());
            let view = s.state.layer_tools.selection_resize.clone().expect("the dialog opens");
            assert_eq!((view.kind, view.radius), (kind, 5.));
            assert!(!view.label.is_empty() && view.label != "Distance", "{kind:?} names its value");
            assert_eq!(view.numeric.max as f32, match kind {
                RefineKind::Feather => 100.,
                RefineKind::Smooth => 64.,
                _ => 128.,
            });
            let request = answer(&mut s, 0xff);
            check_steps(kind, 5, &request, &original);
            assert!(modify(&request).preview.is_some());
            assert_eq!(s.engine.document().selection.as_ref(), Some(&original), "{kind:?}: a preview leaves the document");
            assert_eq!(s.engine.display_selection().as_deref(), Some(&preview(0xff)), "and shows on the canvas");
            let now = settle(&mut s, 2);
            let exact = answer_at(&mut s, 0xff00, now, false);
            assert!(modify(&exact).preview.is_none(), "{kind:?}: a resting value runs exactly");
            check_steps(kind, 5, &exact, &original);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&mask(0xff00)), "{kind:?}: and becomes a step");
            assert_eq!(s.engine.display_selection().as_deref(), Some(&mask(0xff00)));

            selection_action(&mut s, SelectionAction::ResizeRadius { radius: 7. });
            let request = answer_at(&mut s, 0x10, now + 2, false);
            check_steps(kind, 7, &request, &original);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&mask(0xff00)), "{kind:?}: previews amend nothing");
            let now = settle(&mut s, now + 3);
            answer_at(&mut s, 0x1000, now, false);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&mask(0x1000)), "{kind:?}: a new value amends it");
            selection_action(&mut s, SelectionAction::ApplyResize);
            assert!(s.state.layer_tools.selection_resize.is_none());
            assert!(s.selection_masks.refine.is_none(), "{kind:?}: Apply closes a settled value at once");
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&original), "{kind:?}: one undo step");
            invoke(&mut s, CommandId::Redo);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&mask(0x1000)));

            invoke(&mut s, kind.command());
            let now = settle(&mut s, now + 2);
            answer_at(&mut s, 0x20, now, false);
            let now = settle(&mut s, now + 2);
            answer_at(&mut s, 0x2000, now, false);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&mask(0x2000)));
            selection_action(&mut s, SelectionAction::CancelResize);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&mask(0x1000)), "{kind:?}: Cancel restores");
            assert!(!s.engine.can_redo(), "Cancel leaves nothing to redo");
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&original), "and no undo step");
        }
    }

    #[test]
    fn a_dragged_value_previews_without_publishing_and_apply_waits_for_the_exact_last_value() {
        let mut s = session(Platform::Gtk);
        select(&mut s, rectangle());
        let original = s.engine.document().selection.clone();
        invoke(&mut s, CommandId::GrowSelection);
        s.frame(1, 1).unwrap();
        let revision = s.state.revision;
        let changed = s.dispatch(UiAction::Selection { action: SelectionAction::ResizeRadius { radius: 8. } }).unwrap();
        assert_eq!(changed.regions, 0, "the panel shows its own value, published once it rests");
        assert_eq!(s.state.layer_tools.selection_resize.as_ref().unwrap().radius, 8.);
        selection_action(&mut s, SelectionAction::ResizeRadius { radius: 9. });
        let running = answer_at(&mut s, 0xff, 2, false);
        assert_eq!(modify(&running).steps[0].resize, 5, "values that arrive while a preview runs wait for it");
        assert_eq!(s.engine.display_selection().as_deref(), Some(&preview(0xff)), "and it shows");
        let latest = answer_at(&mut s, 0xff00, 4, false);
        assert_ne!(latest.request_id, running.request_id);
        assert_eq!(modify(&latest).steps[0].resize, 9, "then only the latest value runs");
        assert_eq!(s.engine.display_selection().as_deref(), Some(&preview(0xff00)));
        assert_eq!(s.engine.document().selection, original, "previews never enter the document");
        assert_eq!(s.selection_masks.refine_previews, 2);
        assert_eq!(s.renderer_stats().selection_previews, 2);
        let requests = s.renderer_mut().region_requests.len();
        s.frame(6, 6).unwrap();
        assert_eq!(s.renderer_mut().region_requests.len(), requests, "a moving value runs nothing exact");
        assert!(s.wants_continuous_frames(), "frames continue until the value settles");
        assert!(s.state.revision - revision <= 3, "previews publish nothing: {}", s.state.revision - revision);
        let idle = s.state.commands.clone();
        selection_action(&mut s, SelectionAction::ResizeRadius { radius: 10. });
        s.frame(7, 7).unwrap();
        assert!(s.region_tools.busy(), "a preview runs");
        assert_eq!(s.state.commands, idle, "commands hold still while a preview runs");

        answer_at(&mut s, 0xff0f, 8, false);
        let now = settle(&mut s, 9);
        assert!(modify(s.renderer_mut().region_requests.last().unwrap()).preview.is_none(), "the resting value runs exactly");
        assert!(!s.command(CommandId::InvertSelection).enabled, "an exact job holds other edits");
        selection_action(&mut s, SelectionAction::ResizeRadius { radius: 11. });
        assert_eq!(s.renderer_mut().region_cancels, 1, "a new value stops the exact job");
        let moved = answer_at(&mut s, 0xff_0000, now + 1, false);
        assert_eq!(modify(&moved).steps[0].resize, 11);
        assert!(modify(&moved).preview.is_some());
        selection_action(&mut s, SelectionAction::ResizeRadius { radius: 12. });
        selection_action(&mut s, SelectionAction::ApplyResize);
        assert!(s.state.layer_tools.selection_resize.is_none(), "Apply closes the dialog at once");
        assert!(s.dispatch(UiAction::Selection { action: SelectionAction::ResizeRadius { radius: 3. } }).is_err());
        assert_eq!(s.engine.document().selection, original);
        assert_eq!(modify(&answer_at(&mut s, 0xf0f0, now + 3, false)).steps[0].resize, 12);
        assert!(s.selection_masks.refine.is_some(), "Apply waits for the last value");
        let exact = answer_at(&mut s, 0xff00_0000, now + 5, false);
        assert!(modify(&exact).preview.is_none() && modify(&exact).steps[0].resize == 12, "and runs it exactly at once");
        assert_eq!(s.engine.document().selection, Some(mask(0xff00_0000)), "and keeps it");
        assert!(s.selection_masks.refine.is_none());
        assert_eq!(s.engine.display_selection().as_deref(), Some(&mask(0xff00_0000)));
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().selection, original);
        assert!(s.dispatch(UiAction::Selection { action: SelectionAction::ResizeRadius { radius: 3. } }).is_err());
        assert!(s.dispatch(UiAction::Selection { action: SelectionAction::ApplyResize }).is_err());
    }

    #[test]
    fn an_exact_preview_settles_without_another_job_and_cancel_hides_a_preview() {
        let mut s = session(Platform::Gtk);
        select(&mut s, rectangle());
        let original = s.engine.document().selection.clone();
        invoke(&mut s, CommandId::FeatherSelection);
        answer_at(&mut s, 0xff, 1, true);
        assert_eq!(s.engine.document().selection, original, "an exact preview waits for the value to rest");
        assert_eq!(s.engine.display_selection().as_deref(), Some(&mask(0xff)));
        let requests = s.renderer_mut().region_requests.len();
        settle(&mut s, 2);
        assert_eq!(s.renderer_mut().region_requests.len(), requests, "then becomes the step as it is");
        assert_eq!(s.engine.document().selection, Some(mask(0xff)));
        selection_action(&mut s, SelectionAction::CancelResize);
        assert_eq!(s.engine.document().selection, original);

        invoke(&mut s, CommandId::FeatherSelection);
        answer_at(&mut s, 0xff, 1, false);
        assert_eq!(s.engine.display_selection().as_deref(), Some(&preview(0xff)));
        selection_action(&mut s, SelectionAction::CancelResize);
        assert_eq!(s.engine.display_selection().as_deref(), original.as_ref(), "Cancel takes the preview away");
        assert_eq!(s.engine.document().selection, original);
        assert!(!s.wants_continuous_frames());
    }

    #[test]
    fn values_are_validated_and_a_stale_draft_closes() {
        let mut s = session(Platform::Gtk);
        select(&mut s, rectangle());
        invoke(&mut s, CommandId::SmoothSelection);
        for radius in [0., 65., 2.5, f32::NAN] {
            assert!(s.dispatch(UiAction::Selection { action: SelectionAction::ResizeRadius { radius } }).is_err(), "{radius}");
        }
        selection_action(&mut s, SelectionAction::CancelResize);
        invoke(&mut s, CommandId::FeatherSelection);
        selection_action(&mut s, SelectionAction::ResizeRadius { radius: 2.5 });
        answer(&mut s, 0xf0);
        assert_eq!(modify(&answer_at(&mut s, 0xf1, 3, false)).steps[0].feather, 2.5);
        let now = settle(&mut s, 4);
        answer_at(&mut s, 0xff, now, false);
        s.layer_edit(layer_core::Edit::SetSelection(Some(rectangle()))).unwrap();
        s.frame(now + 3, now + 3).unwrap();
        assert!(s.selection_masks.refine.is_none(), "another edit ends the draft");
        assert!(s.state.layer_tools.selection_resize.is_none());
        assert!(s.state.notice.is_some(), "and says so");
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().selection, Some(mask(0xff)), "without withdrawing its step");
    }

    #[test]
    fn refinements_change_quick_mask_and_the_edited_selection_layer() {
        let mut s = session(Platform::Gtk);
        select(&mut s, rectangle());
        invoke(&mut s, CommandId::QuickMask);
        assert!(!s.command(CommandId::TransformSelectionOutline).enabled);
        assert_eq!(
            s.command_disabled_reason(CommandId::TransformSelectionOutline).as_deref(),
            Some("Return to the artwork first")
        );
        let modify_menu = |menu: &ContextMenu| -> Vec<(String, Option<UiAction>, bool)> {
            let modify = menu.sections.iter().flatten().find(|i| i.label == "Modify").unwrap();
            modify.sections[1].iter().map(|i| (i.label.clone(), i.action.clone(), i.enabled)).collect()
        };
        let quick = modify_menu(&s.quick_mask_menu());
        assert_eq!(
            quick,
            RefineKind::ALL
                .map(|kind| (canvas_bar::short_label(kind.command()).to_string(), Some(UiAction::Invoke { command: kind.command() }), true))
                .to_vec()
        );
        invoke(&mut s, CommandId::FeatherSelection);
        assert_eq!(modify(&answer(&mut s, 0xf0)).steps[0].feather, 5.);
        assert_eq!(s.engine.display_selection().as_deref(), Some(&preview(0xf0)), "Quick Mask shows the preview");
        selection_action(&mut s, SelectionAction::ApplyResize);
        answer_at(&mut s, 0xff, 3, false);
        assert!(s.selection_masks.quick(), "Quick Mask stays open");
        assert_eq!(s.engine.document().selection, Some(mask(0xff)));
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().selection, Some(rectangle()));

        invoke(&mut s, CommandId::ReturnToArtwork);
        invoke(&mut s, CommandId::SaveSelectionLayer);
        let Some(SelectionTarget::Saved(id)) = s.selection_masks.target() else {
            panic!("saving edits the new Selection Layer")
        };
        let saved = |s: &UiSession<Recorder>| s.engine.document().saved_selection(id).unwrap();
        let before = saved(&s);
        invoke(&mut s, CommandId::GrowSelection);
        let request = answer(&mut s, 0xf000);
        assert_eq!(modify(&request).selection.as_ref(), &before);
        assert_eq!(saved(&s), before, "a preview leaves the Selection Layer");
        selection_action(&mut s, SelectionAction::ApplyResize);
        answer_at(&mut s, 0xff00, 3, false);
        assert_eq!(saved(&s), mask(0xff00));
        assert_eq!(s.engine.document().selection, Some(rectangle()), "the current selection is untouched");
        invoke(&mut s, CommandId::Undo);
        assert_eq!(saved(&s), before);

        let layer_menu = modify_menu(&s.selection_layer_menu(id).unwrap());
        assert_eq!(
            layer_menu,
            RefineKind::ALL
                .map(|kind| (
                    canvas_bar::short_label(kind.command()).to_string(),
                    Some(UiAction::Selection { action: SelectionAction::BeginRefine { kind, layer: Some(id.0) } }),
                    true,
                ))
                .to_vec()
        );
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: id.0, value: true } }).unwrap();
        assert!(modify_menu(&s.selection_layer_menu(id).unwrap()).iter().all(|(_, _, enabled)| !enabled));
        assert_eq!(
            s.command_disabled_reason(CommandId::BorderSelection).as_deref(),
            Some("This selection layer is locked")
        );
        assert!(s.dispatch(UiAction::Selection { action: SelectionAction::BeginRefine { kind: RefineKind::Border, layer: Some(id.0) } }).is_err());
    }

    #[test]
    fn refine_commands_need_a_selection_and_appear_in_the_select_menu() {
        let mut s = session(Platform::Gtk);
        for kind in RefineKind::ALL {
            assert_eq!(s.command_disabled_reason(kind.command()).as_deref(), Some("Make a selection first"));
        }
        assert_eq!(
            s.command_disabled_reason(CommandId::TransformSelectionOutline).as_deref(),
            Some("Make a selection first")
        );
        select(&mut s, Selection::empty());
        assert_eq!(
            s.command_disabled_reason(CommandId::TransformSelectionOutline).as_deref(),
            Some("The selection is empty")
        );
        select(&mut s, rectangle());
        let menu = s.application_menu(ApplicationMenu::Select);
        let labels: Vec<_> = menu.sections.iter().flatten().map(|i| i.label.as_str()).collect();
        for command in RefineKind::ALL.map(RefineKind::command).into_iter().chain([CommandId::TransformSelectionOutline]) {
            assert!(s.command(command).enabled, "{command:?}");
            assert!(labels.contains(&command.label()), "{command:?} is in the Select menu");
        }
        assert!(!labels.contains(&"Grow…"), "the old items are replaced");
    }

    fn filled(selection: Selection) -> UiSession<Recorder> {
        let mut s = session(Platform::Gtk);
        s.fill_selection(rectangle()).unwrap();
        s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
        invoke(&mut s, CommandId::FitCanvas);
        s.frame(1, 1).unwrap();
        select(&mut s, selection);
        invoke(&mut s, CommandId::Move);
        s
    }

    #[test]
    fn transform_outline_changes_only_the_selection_placement_in_one_step() {
        let mut inverted = rectangle();
        inverted.inverted = true;
        let corner = Point { x: 100., y: 100. };
        for (selection, moved) in [
            (rectangle(), Point { x: 0., y: 100. }),
            (soft(), Point { x: 0., y: 100. }),
            (inverted, Point { x: -300., y: 100. }),
        ] {
            let mut s = filled(selection.clone());
            let layers = s.engine.document().layers.clone();
            let bar = s.state.canvas_bar.clone().unwrap();
            let refine = s.canvas_bar_choice_menu(bar.context, "refine").unwrap();
            s.dispatch(refine.sections[1][0].action.clone().unwrap()).unwrap();
            assert!(s.operation.outline());
            assert!(s.engine.transform_preview().is_none(), "no pixels move");
            assert!(s.renderer_mut().moving_layer.is_none());
            let bar = s.state.canvas_bar.clone().unwrap();
            assert_eq!((bar.context.kind, bar.label.as_deref()), (CanvasBarKind::Transform, Some("Transform Outline")));
            let modes = match &bar.items[0].option {
                ToolOption::Choice { items, .. } => items.iter().map(|i| i.action.clone()).collect::<Vec<_>>(),
                _ => panic!("the bar starts with its modes"),
            };
            assert_eq!(modes, [CommandId::TransformFree, CommandId::TransformUniform].map(|command| UiAction::Invoke { command }));
            let commands: Vec<_> = bar.items.iter().chain(&bar.completion).filter_map(|i| match &i.option {
                ToolOption::Action { state, .. } => Some(state.id),
                _ => None,
            }).collect();
            assert_eq!(commands, [
                CommandId::TransformFlipHorizontal, CommandId::TransformFlipVertical, CommandId::TransformRotateLeft,
                CommandId::TransformRotateRight, CommandId::ResetTransform, CommandId::CancelTransform, CommandId::ApplyTransform,
            ]);
            for command in [CommandId::TransformDistort, CommandId::TransformWarp] {
                assert_eq!(s.command_disabled_reason(command).as_deref(), Some(operation::OUTLINE_AFFINE));
            }
            assert_eq!(s.command_disabled_reason(CommandId::TransformBicubic).as_deref(), Some(operation::OUTLINE_PIXELS));
            assert!(!s.state.tool_actions.iter().any(|a| a.command == CommandId::TransformBicubic));
            assert!(s.set_transform_mode(operation::TransformMode::Warp, false).is_err());

            s.set_transform_control("transform_width", 2.).unwrap();
            s.frame(2, 2).unwrap();
            let preview = s.engine.display_selection().unwrap().into_owned();
            let near = |a: Point, b: Point| assert!((a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3, "{a:?} != {b:?}");
            near(preview.affine.map(corner), moved);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&selection), "Apply is the only edit");
            invoke(&mut s, CommandId::ApplyTransform);
            assert!(!s.operation.active());
            let applied = s.engine.document().selection.clone().unwrap();
            assert_eq!(applied, preview);
            assert_eq!(applied.inverted, selection.inverted);
            assert_eq!(applied.shape, selection.shape);
            match (&applied.shape, &selection.shape) {
                (SelectionShape::Pixels(a), SelectionShape::Pixels(b)) => assert!(Arc::ptr_eq(a, b), "coverage is shared"),
                (SelectionShape::Contours(a), SelectionShape::Contours(b)) => assert!(Arc::ptr_eq(a, b)),
                _ => unreachable!(),
            }
            s.frame(3, 3).unwrap();
            assert_eq!(s.engine.document().layers, layers, "the pixels stay where they are");
            assert!(s.renderer_mut().pending_operations.is_empty());
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&selection), "one undo step");
            invoke(&mut s, CommandId::Redo);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&applied));

            invoke(&mut s, CommandId::TransformSelectionOutline);
            invoke(&mut s, CommandId::TransformFlipVertical);
            invoke(&mut s, CommandId::CancelTransform);
            assert_eq!(s.engine.document().selection.as_ref(), Some(&applied), "Cancel keeps the outline");
            assert_eq!(s.engine.display_selection().as_deref(), Some(&applied));
            assert!(!s.engine.can_redo());
        }
    }
}
