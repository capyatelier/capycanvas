mod selection_pixel_checks {
    use super::*;
    use layer_core::{LayerOperation, LayerOperationKind, Selection};
    use std::sync::Arc;

    /// Half coverage over `[x0, y0, x1, y1]`, in whole groups of four pixels.
    fn soft(extent: [u32; 2], [x0, y0, x1, y1]: [u32; 4]) -> Selection {
        let row = extent[0].div_ceil(4) as usize;
        let mut words = vec![0u32; row * extent[1] as usize];
        for y in y0..y1 {
            for x in (x0..x1).step_by(4) {
                words[y as usize * row + x as usize / 4] = 0x8080_8080;
            }
        }
        let pixels = layer_core::SelectionPixels::bytes(extent, [x0, y0, x1, y1], words).unwrap();
        Selection::pixels(Arc::new(pixels))
    }

    /// The operations the last frame submitted.
    fn submitted(s: &mut UiSession<Recorder>) -> Vec<(LayerId, LayerOperation)> {
        s.frame(2, 2).unwrap();
        s.renderer_mut().pending_operations.clone()
    }

    fn erase(operation: &LayerOperation) -> bool {
        matches!(operation.kind, LayerOperationKind::Erase { alpha_locked: false })
    }

    fn photo_session() -> UiSession<Recorder> {
        let source = layer_core::color::source::rgba8_source([40, 30], |_, _| [200; 4]);
        let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
            Document::new("photo", 200, 150), [800, 600], Platform::Gtk).unwrap();
        s.place_layer_source("Photo", Arc::unwrap_or_clone(source), None).unwrap();
        invoke(&mut s, CommandId::ApplyTransform);
        s.frame(1, 1).unwrap();
        s
    }

    #[test]
    fn clear_erases_through_hard_soft_and_inverted_selections_in_one_step_each() {
        let mut s = session(Platform::Gtk);
        let layer = s.engine.document().active_layer;
        let extent = s.engine.document().target_extent(layer);
        let hard = rectangle([100., 120., 300., 260.]);
        let mut inverted = hard.clone();
        inverted.inverted = true;
        let soft = soft(extent, [400, 400, 480, 460]);
        let full = layer_core::Rect { min: Point::default(), max: Point { x: extent[0] as f32, y: extent[1] as f32 } };
        for (selection, command, bounds, erased_outside) in [
            (hard.clone(), CommandId::ClearSelected, hard.bounds(), false),
            (hard.clone(), CommandId::ClearOutside, full, true),
            (inverted.clone(), CommandId::ClearSelected, full, true),
            (inverted, CommandId::ClearOutside, hard.bounds(), false),
            (soft.clone(), CommandId::ClearSelected, soft.bounds(), false),
        ] {
            select(&mut s, selection.clone());
            let before = s.engine.document().clone();
            assert!(s.command(command).enabled, "{command:?}");
            invoke(&mut s, command);
            let operations = submitted(&mut s);
            let [(target, operation)] = &operations[..] else { panic!("one operation: {operations:?}") };
            assert_eq!(*target, layer);
            assert!(erase(operation), "{:?}", operation.kind);
            let coverage = operation.coverage.initial.as_ref().unwrap();
            assert_eq!(coverage.inverted, erased_outside, "{command:?}");
            assert_eq!(coverage.shape, selection.shape, "soft coverage is kept");
            assert_eq!(operation.bounds(extent), bounds, "{command:?} rewrites only the pages it can change");
            assert_eq!(s.engine.document().selection, before.selection, "clearing keeps the selection");
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.document().layers, before.layers, "{command:?} is one undo step");
            invoke(&mut s, CommandId::Redo);
            invoke(&mut s, CommandId::Undo);
        }
    }

    #[test]
    fn clear_explains_why_it_is_unavailable() {
        let mut s = session(Platform::Gtk);
        let id = s.engine.document().active_layer.0;
        let reason = |s: &UiSession<Recorder>| {
            let reasons = [CommandId::ClearSelected, CommandId::ClearOutside].map(|c| s.command_disabled_reason(c));
            assert_eq!(reasons[0], reasons[1]);
            reasons[0].clone()
        };
        assert_eq!(reason(&s).as_deref(), Some("Make a selection first"));
        select(&mut s, rectangle([10., 10., 90., 90.]));
        assert_eq!(reason(&s), None);
        s.dispatch(UiAction::Layer { action: LayerAction::AlphaLock { id, value: true } }).unwrap();
        assert_eq!(reason(&s).as_deref(), Some("Alpha lock keeps transparency; unlock the layer first"));
        assert_eq!(
            s.dispatch(UiAction::Invoke { command: CommandId::ClearSelected }).unwrap_err(),
            "Alpha lock keeps transparency; unlock the layer first"
        );
        s.dispatch(UiAction::Layer { action: LayerAction::AlphaLock { id, value: false } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id, value: true } }).unwrap();
        assert_eq!(reason(&s).as_deref(), Some("The active layer is locked"));
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id, value: false } }).unwrap();
        s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
        assert_eq!(reason(&s).as_deref(), Some(super::notices::drawing_refusal_text(layer_core::DrawingRefusal::Paper)));
        s.dispatch(UiAction::SelectLayer { id }).unwrap();
        let selection = s.engine.document().selection.clone();
        s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id, replace: false } }).unwrap();
        assert!(s.engine.document().active_mask);
        assert_eq!(reason(&s).as_deref(), Some("Masks aren't cleared this way; return to the layer's artwork first"));
        s.dispatch(UiAction::Layer { action: LayerAction::Select { id, mask: false } }).unwrap();
        select(&mut s, selection.unwrap());
        invoke(&mut s, CommandId::QuickMask);
        assert_eq!(reason(&s).as_deref(), Some("Quick Mask edits the selection; leave it to clear artwork"));
        invoke(&mut s, CommandId::ReturnToArtwork);
        s.dispatch(UiAction::Layer { action: LayerAction::FillSelection }).unwrap();
        invoke(&mut s, CommandId::ScaleRotate);
        assert_eq!(reason(&s).as_deref(), Some("Apply or cancel the transform first"));
    }

    #[test]
    fn clearing_a_placed_photo_keeps_its_original_and_clear_entire_layer_discards_it() {
        let mut s = photo_session();
        let layer = s.engine.document().active_layer;
        let source = s.engine.document().layer(layer).unwrap().source.clone().unwrap();
        select(&mut s, rectangle([20., 20., 60., 50.]));
        let before = s.engine.document().layers.clone();
        invoke(&mut s, CommandId::ClearSelected);
        let operations = submitted(&mut s);
        assert!(matches!(&operations[..], [(target, op)] if *target == layer && erase(op)));
        let cleared = s.engine.document().layer(layer).unwrap();
        assert!(Arc::ptr_eq(cleared.source.as_ref().unwrap(), &source), "the raster clears over the original");
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().layers, before);
        assert_eq!(CommandId::ClearLayer.label(), "Clear Entire Layer");
        assert!(crate::customization::tool_choice(ToolbarControl::Command { command: CommandId::ClearLayer })
            .description
            .contains("placed photo"));
        invoke(&mut s, CommandId::ClearLayer);
        assert!(s.engine.document().layer(layer).unwrap().source.is_none());
        assert!(s.state.settings.keys(&CommandId::ClearLayer.shortcut_id()).is_empty(), "Clear Entire Layer stays unbound");
    }

    #[test]
    fn revert_to_original_discards_photo_edits_in_one_step_and_keeps_the_rest() {
        let mut s = photo_session();
        let id = s.engine.document().active_layer;
        let reason = |s: &UiSession<Recorder>| s.command_disabled_reason(CommandId::RevertToOriginal);
        assert_eq!(reason(&s).as_deref(), Some("This photo has no edits"));
        let source = s.engine.document().layer(id).unwrap().source.clone().unwrap();
        select(&mut s, rectangle([20., 20., 60., 50.]));
        invoke(&mut s, CommandId::ClearSelected);
        submitted(&mut s);
        s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id: id.0, replace: false } }).unwrap();
        assert_eq!(reason(&s).as_deref(), Some("Return to the layer's artwork first"));
        s.dispatch(UiAction::Layer { action: LayerAction::Select { id: id.0, mask: false } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Blend { id: id.0, value: 2 } }).unwrap();
        s.set_layer_opacity(Some(id.0), 0.5).unwrap();
        let edited = s.engine.document().layer(id).unwrap().clone();
        assert!(!edited.pending_operations.is_empty() || !edited.raster.is_empty());
        assert_eq!(reason(&s), None);
        let edit_menu = s.application_menu(ApplicationMenu::Edit);
        let labels = |sections: &[Vec<ContextMenuItem>]| sections.iter().flatten().map(|i| i.label.clone()).collect::<Vec<_>>();
        let edit = labels(&edit_menu.sections);
        let rasterize = edit.iter().position(|l| l == "Rasterize Source…").unwrap();
        assert_eq!(edit[rasterize + 1], "Revert to Original Photo");
        let layer_menu = s.layer_menu(id.0, false).unwrap();
        let settings = layer_menu.sections.iter().flatten().find(|i| i.label == "Layer Settings").unwrap();
        let settings = labels(&settings.sections);
        let rasterize = settings.iter().position(|l| l == "Rasterize Source…").unwrap();
        assert_eq!(settings[rasterize + 1], "Revert to Original Photo");

        invoke(&mut s, CommandId::RevertToOriginal);
        let reverted = s.engine.document().layer(id).unwrap().clone();
        let mut expected = edited.clone();
        expected.raster = Default::default();
        expected.pending_operations.clear();
        assert_eq!(reverted, expected, "only the edits go; placement, mask, opacity and blend stay");
        assert!(Arc::ptr_eq(reverted.source.as_ref().unwrap(), &source), "the original is shared, not copied");
        assert_eq!(reason(&s).as_deref(), Some("This photo has no edits"));
        assert_eq!(
            s.dispatch(UiAction::Invoke { command: CommandId::RevertToOriginal }).unwrap_err(),
            "This photo has no edits"
        );
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().layer(id), Some(&edited), "one undo step");
        invoke(&mut s, CommandId::Redo);
        assert_eq!(s.engine.document().layer(id), Some(&expected));
        invoke(&mut s, CommandId::Undo);

        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: id.0, value: true } }).unwrap();
        assert_eq!(reason(&s).as_deref(), Some("The active layer is locked"));
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: id.0, value: false } }).unwrap();
        invoke(&mut s, CommandId::ScaleRotate);
        assert_eq!(reason(&s).as_deref(), Some("Apply or cancel the transform first"));
        invoke(&mut s, CommandId::CancelTransform);
        invoke(&mut s, CommandId::QuickMask);
        assert_eq!(reason(&s).as_deref(), Some("Return to the artwork first"));
        invoke(&mut s, CommandId::ReturnToArtwork);
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }).unwrap();
        assert_eq!(reason(&s).as_deref(), Some("Select a placed photo layer"));
        s.dispatch(UiAction::SelectLayer { id: id.0 }).unwrap();
        let mut rasterized = s.engine.document().layer(id).unwrap().clone();
        let mut converted = (*source).clone();
        converted.kind = layer_core::color::source::SourceKind::Rasterized;
        rasterized.source = Some(Arc::new(converted));
        s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(rasterized))).unwrap();
        assert_eq!(reason(&s).as_deref(), Some("A rasterized photo has no original to return to"));
    }

    #[test]
    fn copy_and_cut_to_a_new_layer_share_the_photo_and_sit_above_its_clipping_stack() {
        for cut in [false, true] {
            let command = if cut { CommandId::CutSelectionToLayer } else { CommandId::CopySelectionToLayer };
            let mut s = photo_session();
            let base = s.engine.document().active_layer;
            s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: true } }).unwrap();
            let clipped = s.engine.document().active_layer;
            s.dispatch(UiAction::SelectLayer { id: base.0 }).unwrap();
            let selection = rectangle([30., 25., 90., 80.]);
            select(&mut s, selection.clone());
            let before = s.engine.document().clone();
            invoke(&mut s, command);
            let doc = s.engine.document();
            let copy = doc.layer(doc.active_layer).unwrap();
            assert_ne!(copy.id, base);
            let index = |id| doc.layers.iter().position(|l| l.id == id).unwrap();
            assert_eq!(index(copy.id) + 1, index(clipped), "{command:?} goes directly above the clipping stack");
            assert!(!copy.properties.clipped && copy.mask.is_none());
            assert_eq!(copy.properties.parent, before.layer(base).unwrap().properties.parent);
            assert!(Arc::ptr_eq(
                copy.source.as_ref().unwrap(),
                before.layer(base).unwrap().source.as_ref().unwrap()
            ));
            assert!(doc.selection.is_none(), "the selection moves into the new layer");
            let copy = copy.id;
            let operations = submitted(&mut s);
            assert_eq!(operations.len(), 1 + usize::from(cut));
            assert!(operations.iter().all(|(_, op)| erase(op)));
            assert_eq!(operations[0].0, copy);
            assert!(operations[0].1.coverage.initial.as_ref().unwrap().inverted, "the copy keeps only the selection");
            if cut {
                assert_eq!(operations[1].0, base);
                assert!(!operations[1].1.coverage.initial.as_ref().unwrap().inverted, "Cut erases it from the source");
            }
            assert!(s.command(CommandId::Reselect).enabled);
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.document().layers, before.layers, "{command:?} is one undo step");
            assert_eq!(s.engine.document().selection, before.selection);
            invoke(&mut s, CommandId::Redo);
            invoke(&mut s, CommandId::Reselect);
            assert_eq!(s.engine.document().selection, Some(selection), "Reselect restores the consumed selection");
        }
    }

    #[test]
    fn copy_to_a_new_layer_without_a_selection_duplicates_and_cut_explains() {
        let mut s = session(Platform::Gtk);
        let count = s.engine.document().layers.len();
        assert_eq!(
            s.command_disabled_reason(CommandId::CutSelectionToLayer).as_deref(),
            Some("Make a selection first")
        );
        assert!(s.command(CommandId::CopySelectionToLayer).enabled);
        let source = s.engine.document().layer(s.engine.document().active_layer).unwrap().clone();
        invoke(&mut s, CommandId::CopySelectionToLayer);
        let doc = s.engine.document();
        assert_eq!(doc.layers.len(), count + 1);
        let copy = doc.layer(doc.active_layer).unwrap();
        assert_eq!(&*copy.name, format!("{} copy", source.name));
        assert_eq!(copy.raster, source.raster, "a duplicate shares every pixel");
        assert!(submitted(&mut s).is_empty());
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().layers.len(), count);
    }

    #[test]
    fn copy_and_cut_to_a_new_layer_refuse_layers_without_pixels_and_masks() {
        let mut s = session(Platform::Gtk);
        let paint = s.engine.document().active_layer.0;
        select(&mut s, rectangle([10., 10., 90., 90.]));
        let reasons = |s: &UiSession<Recorder>| {
            [CommandId::CopySelectionToLayer, CommandId::CutSelectionToLayer].map(|c| s.command_disabled_reason(c))
        };
        assert_eq!(reasons(&s), [None, None]);
        s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
        assert_eq!(reasons(&s)[0].as_deref(), Some("The paper can't be copied to a layer"));
        s.dispatch(UiAction::SelectLayer { id: paint }).unwrap();
        s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }).unwrap();
        select(&mut s, rectangle([10., 10., 90., 90.]));
        assert_eq!(reasons(&s)[1].as_deref(), Some("An effect layer has no pixels of its own"));
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
        assert_eq!(
            reasons(&s)[0].as_deref(),
            Some(super::notices::drawing_refusal_text(layer_core::DrawingRefusal::Group))
        );
        s.dispatch(UiAction::SelectLayer { id: paint }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: paint, value: true } }).unwrap();
        assert_eq!(reasons(&s), [None, Some("The active layer is locked".into())], "copying reads a locked layer");
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: paint, value: false } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::AlphaLock { id: paint, value: true } }).unwrap();
        assert_eq!(reasons(&s)[1].as_deref(), Some("Alpha lock keeps transparency; unlock the layer first"));
        s.dispatch(UiAction::Layer { action: LayerAction::AlphaLock { id: paint, value: false } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::MaskSelection { id: paint, hide: false } }).unwrap();
        select(&mut s, rectangle([10., 10., 90., 90.]));
        assert!(s.engine.document().active_mask);
        assert_eq!(reasons(&s)[0].as_deref(), Some("Return to the layer's artwork first"));
        let layers = s.engine.document().layers.clone();
        assert!(s.dispatch(UiAction::Invoke { command: CommandId::CopySelectionToLayer }).is_err());
        assert_eq!(s.engine.document().layers, layers);
    }

    #[test]
    fn a_new_effect_takes_soft_and_inverted_selections_as_its_mask_in_one_step() {
        let mut s = session(Platform::Gtk);
        let extent = s.engine.document().target_extent(s.engine.document().active_layer);
        let mut inverted = rectangle([100., 100., 300., 300.]);
        inverted.inverted = true;
        let insert = |s: &mut UiSession<Recorder>| {
            s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }).unwrap();
            let doc = s.engine.document();
            doc.layer(doc.active_layer).unwrap().clone()
        };
        for selection in [soft(extent, [200, 240, 400, 360]), inverted] {
            select(&mut s, selection.clone());
            let before = s.engine.document().clone();
            let effect = insert(&mut s);
            let mask = effect.mask.as_ref().expect("the selection becomes the effect's mask");
            assert_eq!(mask.initial.as_ref(), Some(&selection));
            assert_eq!(mask.default_coverage, f32::from(selection.inverted));
            assert!(mask.enabled && !mask.inverted);
            assert!(s.engine.document().selection.is_none(), "the mask consumes the selection");
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.document().layers, before.layers, "one undo step");
            assert_eq!(s.engine.document().selection, before.selection);
        }
        invoke(&mut s, CommandId::Deselect);
        assert!(insert(&mut s).mask.is_none(), "without a selection the effect covers everything");
    }

    #[test]
    fn selection_actions_are_reachable_from_menus_and_search() {
        let s = session(Platform::Gtk);
        let edit = s.application_menu(ApplicationMenu::Edit);
        let select = s.application_menu(ApplicationMenu::Select);
        let layer = s.application_menu(ApplicationMenu::Layer);
        let labels = |menu: &ContextMenu| {
            fn walk(sections: &[Vec<ContextMenuItem>], out: &mut Vec<String>) {
                for item in sections.iter().flatten() {
                    out.push(item.label.clone());
                    walk(&item.sections, out);
                }
            }
            let mut out = Vec::new();
            walk(&menu.sections, &mut out);
            out
        };
        for command in [CommandId::ClearSelected, CommandId::ClearOutside, CommandId::ClearLayer] {
            assert!(labels(&edit).contains(&command.label().to_string()), "Edit › {command:?}");
        }
        for command in [
            CommandId::ClearSelected,
            CommandId::ClearOutside,
            CommandId::CopySelectionToLayer,
            CommandId::CutSelectionToLayer,
        ] {
            assert!(labels(&select).contains(&command.label().to_string()), "Select › {command:?}");
        }
        let new = layer.sections[0][0].sections.concat();
        for command in [CommandId::CopySelectionToLayer, CommandId::CutSelectionToLayer] {
            assert!(new.iter().any(|i| i.action == Some(UiAction::Invoke { command })), "Layer › New › {command:?}");
        }
        assert_eq!(s.state.settings.keys(&CommandId::ClearOutside.shortcut_id()), [], "Clear Outside has no default");
    }

    #[test]
    fn delete_prefers_text_fields_then_the_polygon_then_a_guide_then_clear() {
        let mut s = session(Platform::Gtk);
        s.set_viewport([1000., 1000.], [1000, 1000]).unwrap();
        s.frame(1, 1).unwrap();
        let layer = s.engine.document().active_layer;
        let send = |s: &mut UiSession<Recorder>, phase, p: [f32; 2]| {
            pen_at(s, 1, phase, p);
            s.frame(1, 1).unwrap();
        };
        let pixels = |s: &UiSession<Recorder>| s.engine.document().layer(layer).unwrap().raster.identity();
        let press = |s: &mut UiSession<Recorder>, name: &str, editing: bool| {
            let reply = key(s, name, true, false, editing);
            key(s, name, false, false, editing);
            reply
        };
        select(&mut s, rectangle([100., 100., 400., 400.]));
        let selection = s.engine.document().selection.clone();
        let untouched = pixels(&s);

        invoke(&mut s, CommandId::PolygonSelect);
        for p in [[500., 500.], [700., 500.]] {
            send(&mut s, PenPhase::Down, p);
            send(&mut s, PenPhase::Up, p);
        }
        assert_eq!(s.layer_interaction.path.len(), 2);
        assert!(press(&mut s, "Delete", false).handled);
        assert_eq!(s.layer_interaction.path.len(), 1, "Delete removes the polygon's last point");
        assert!(press(&mut s, "BackSpace", false).handled);
        assert!(s.layer_interaction.path.is_empty());
        assert_eq!((pixels(&s), &s.engine.document().selection), (untouched, &selection));

        invoke(&mut s, CommandId::Ruler);
        send(&mut s, PenPhase::Down, [600., 200.]);
        send(&mut s, PenPhase::Move, [800., 300.]);
        send(&mut s, PenPhase::Up, [800., 300.]);
        assert_eq!(s.engine.document().rulers.len(), 1);
        assert!(press(&mut s, "Delete", false).handled);
        assert!(s.engine.document().rulers.is_empty(), "a selected guide takes Delete from the Ruler tool");
        assert_eq!(pixels(&s), untouched);
        assert!(!s.command(CommandId::DeleteRuler).enabled);
        assert!(press(&mut s, "Delete", false).handled);
        assert_ne!(pixels(&s), untouched, "with no guide selected, Delete clears the selected pixels");
        invoke(&mut s, CommandId::Undo);

        invoke(&mut s, CommandId::Ruler);
        send(&mut s, PenPhase::Down, [600., 600.]);
        send(&mut s, PenPhase::Up, [800., 700.]);
        invoke(&mut s, CommandId::Move);
        assert!(s.command(CommandId::DeleteRuler).enabled);
        press(&mut s, "BackSpace", false);
        assert!(s.engine.document().rulers.is_empty(), "Move also deletes the selected guide");
        assert_eq!(pixels(&s), untouched);

        invoke(&mut s, CommandId::Ruler);
        send(&mut s, PenPhase::Down, [600., 600.]);
        send(&mut s, PenPhase::Up, [800., 700.]);
        invoke(&mut s, CommandId::Brush);
        assert!(s.command(CommandId::DeleteRuler).enabled, "the guide stays selected");
        press(&mut s, "Delete", false);
        assert_eq!(s.engine.document().rulers.len(), 1, "painting tools leave guides alone");
        assert_ne!(pixels(&s), untouched, "Delete clears the selected pixels");
        invoke(&mut s, CommandId::Undo);
        assert_eq!(pixels(&s), untouched);

        assert!(!press(&mut s, "Delete", true).handled, "a focused text field keeps Delete");
        s.state.customization.header_editing = true;
        press(&mut s, "Delete", false);
        s.state.customization.header_editing = false;
        assert_eq!(pixels(&s), untouched, "title bar editing keeps Delete");
        invoke(&mut s, CommandId::Deselect);
        assert!(press(&mut s, "Delete", false).handled, "a disabled Clear still consumes the key");
        assert_eq!(pixels(&s), untouched);
    }
}
