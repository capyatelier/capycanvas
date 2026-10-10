mod clipboard_checks {
    use super::*;
    use layer_core::{
        Edit, LayerBlend, RasterOperationKind,
        color::{
            DocumentColor, RgbSpace, SampleDepth,
            source::rgba8_source,
        },
    };
    use std::sync::Arc;

    fn clip_session() -> UiSession<Recorder> {
        let mut s = UiSession::new(
            Recorder { tiled_sources: true, ..Default::default() },
            Document::new(PortableId::random(), 400, 300, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
            [800, 600],
            Platform::Gtk,
        )
        .unwrap();
        s.frame(1, 1).unwrap();
        s
    }

    fn pending(s: &UiSession<Recorder>) -> (u32, DocumentRequest) {
        s.state
            .requests
            .iter()
            .find_map(|r| match &r.kind {
                HostRequestKind::Document { request } => Some((r.id, request.clone())),
                _ => None,
            })
            .expect("a document request")
    }

    fn clip(extent: [u32; 2], origin: [i64; 2], color: DocumentColor) -> PixelClip {
        let source = Arc::unwrap_or_clone(rgba8_source(extent, |_, _| [90, 120, 200, 255]));
        PixelClip {
            nonce: "clip".into(),
            name: "Ink".into(),
            source: Arc::new(source),
            policy: layer_core::PaintBasePolicy::WorkingPixels,
            origin,
            color,
            blend: layer_core::BlendSpace::Linear.for_depth(color.depth),
            png: Arc::from(&b"png"[..]),
            layers: None,
        }
    }

    fn retain_image_layers(clip: &mut PixelClip, images: Vec<(Arc<str>, layer_core::ImageObject)>) {
        let mut artwork = layer_core::Artwork::new(clip.source.extent).unwrap();
        let composition = artwork.compositions.get_mut(artwork.root).unwrap();
        composition.color = clip.color;
        composition.blend = clip.blend;
        let mut document = Document::from_artwork(artwork).unwrap();
        let (roots, edit) = document.import_object_layers_edit(images, None, 0).unwrap();
        document.apply(edit).unwrap();
        clip.layers = Some(Arc::new(clipboard::LayerClip { scene: document.snapshot(), roots }));
    }

    fn translation(s: &UiSession<Recorder>) -> [f32; 2] {
        let doc = s.engine.document();
        let offset = doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap().offset;
        [offset[0] as f32, offset[1] as f32]
    }

    fn selected_pair(s: &mut UiSession<Recorder>) -> [OccurrenceHandle; 2] {
        let first = s.engine.document().working.occurrence.unwrap();
        invoke(s, CommandId::AddLayer);
        let second = s.engine.document().working.occurrence.unwrap();
        let mut working = s.engine.document().working.clone();
        working.layer_selection = [first, second].into();
        s.layer_edit(Edit::Working(working)).unwrap();
        [first, second]
    }

    fn focused_mask(s: &mut UiSession<Recorder>) -> SourceTarget {
        let id = occurrence_token(s.engine.document().working.occurrence.unwrap());
        layer(s, LayerAction::AddMask { id, replace: false });
        layer(s, LayerAction::Select { id, mask: true });
        s.engine.document().working.target.unwrap()
    }

    fn undo_while_raster_backing_is_pending(mask: bool) {
        let mut s = clip_session();
        if mask { focused_mask(&mut s); }
        select(&mut s, Some(rectangle([10., 20., 30., 40.])));
        let before = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        if mask {
            let copied = clip([20, 20], [10, 20], before.composition().color);
            s.paste_clip(&copied, PasteMode::Paste).unwrap();
        } else {
            layer(&mut s, LayerAction::FillSelection);
        }
        s.frame(2, 2).unwrap();
        let edited = s.engine.checkpoint();
        assert_ne!(edited, checkpoint);
        s.renderer_mut().settling = true;
        assert!(s.canvas_idle());
        assert!(key(&mut s, "z", true, true, false).handled);
        key(&mut s, "z", false, true, false);
        assert_eq!(s.engine.checkpoint(), edited, "history waits for the pending raster backing");
        s.renderer_mut().settling = false;
        s.frame(3, 3).unwrap();
        assert_eq!(s.engine.checkpoint(), checkpoint, "the accepted Undo runs after raster backing is ready");
        assert_live_artwork_eq(s.engine.document(), &before);
        s.frame(4, 4).unwrap();
        assert_eq!(s.engine.checkpoint(), checkpoint, "the shortcut runs once");
    }

    #[test]
    fn mask_paste_undo_waits_for_raster_backing() {
        undo_while_raster_backing_is_pending(true);
    }

    #[test]
    fn paint_undo_waits_for_raster_backing() {
        undo_while_raster_backing_is_pending(false);
    }

    fn shortcut(s: &mut UiSession<Recorder>, name: &str) {
        assert!(key(s, name, true, true, false).handled);
        key(s, name, false, true, false);
    }

    fn has_document_request(s: &UiSession<Recorder>) -> bool {
        s.state.requests.iter().any(|request| matches!(request.kind, HostRequestKind::Document { .. }))
    }

    #[test]
    fn queued_undo_undo_redo_preserves_history_order() {
        let mut s = clip_session();
        select(&mut s, Some(rectangle([10., 20., 30., 40.])));
        layer(&mut s, LayerAction::FillSelection);
        s.frame(2, 2).unwrap();
        let first = s.engine.document().clone();
        let first_checkpoint = s.engine.checkpoint();
        layer(&mut s, LayerAction::FillSelection);
        s.frame(3, 3).unwrap();
        let second_checkpoint = s.engine.checkpoint();
        assert_ne!(first_checkpoint, second_checkpoint);
        assert!(!s.engine.can_redo());
        s.renderer_mut().settling = true;
        for name in ["z", "z", "y"] { shortcut(&mut s, name); }
        assert_eq!(s.engine.checkpoint(), second_checkpoint);
        s.renderer_mut().settling = false;
        for tick in 4..8 { s.frame(tick, tick).unwrap(); }
        assert_eq!(s.engine.checkpoint(), first_checkpoint);
        assert_live_artwork_eq(s.engine.document(), &first);
        shortcut(&mut s, "y");
        s.frame(8, 8).unwrap();
        assert_eq!(s.engine.checkpoint(), second_checkpoint, "the remaining Redo is the second fill");
    }

    #[test]
    fn queued_paste_request_waits_for_the_preceding_undo() {
        let mut s = clip_session();
        let original = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        invoke(&mut s, CommandId::AddLayer);
        s.frame(2, 2).unwrap();
        let edited = s.engine.checkpoint();
        s.renderer_mut().settling = true;
        shortcut(&mut s, "z");
        shortcut(&mut s, "v");
        assert!(!has_document_request(&s), "Paste must not capture the pre-Undo document");
        assert_eq!(s.engine.checkpoint(), edited);
        s.renderer_mut().settling = false;
        for tick in 3..7 { s.frame(tick, tick).unwrap(); }
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_live_artwork_eq(s.engine.document(), &original);
        assert!(matches!(pending(&s).1, DocumentRequest::Paste { mode: PasteMode::Paste }));
        assert_eq!(s.state.requests.iter().filter(|request| matches!(request.kind, HostRequestKind::Document { .. })).count(), 1);
    }

    #[test]
    fn queued_clipboard_actions_do_not_cross_renderer_or_document_activation() {
        for replace_renderer in [false, true] {
            let mut s = clip_session();
            invoke(&mut s, CommandId::AddLayer);
            s.frame(2, 2).unwrap();
            let edited = s.engine.document().clone();
            let checkpoint = s.engine.checkpoint();
            s.renderer_mut().settling = true;
            shortcut(&mut s, "z");
            shortcut(&mut s, "v");
            assert!(!has_document_request(&s));
            if replace_renderer {
                s.replace_renderer(Recorder { tiled_sources: true, ..Default::default() }).unwrap();
            } else {
                s.inherit_window_state(&clip_session()).unwrap();
                s.renderer_mut().settling = false;
            }
            for tick in 3..7 { s.frame(tick, tick).unwrap(); }
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert_live_artwork_eq(s.engine.document(), &edited);
            assert!(!has_document_request(&s), "retired clipboard work cannot request Paste for a new owner");
        }
    }

    fn pen_contact_after_queued_undo(undo_during_contact: bool) {
        let mut s = clip_session();
        let original = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        let target = original.working.target;
        invoke(&mut s, CommandId::AddLayer);
        let removed = s.engine.document().working.occurrence.unwrap();
        s.frame(2, 2).unwrap();
        s.renderer_mut().settling = true;
        shortcut(&mut s, "z");
        pen_at(&mut s, 10, PenPhase::Down, [100., 100.]);
        if undo_during_contact { shortcut(&mut s, "z"); }
        pen_at(&mut s, 11, PenPhase::Up, [110., 100.]);
        s.renderer_mut().settling = false;
        for tick in 3..9 { s.frame(tick, tick).unwrap(); }
        assert!(s.engine.document().scene().occurrence(removed).is_none());
        assert_eq!(s.engine.document().working.target, target);
        assert_eq!(s.engine.metrics().committed_strokes, 1);
        shortcut(&mut s, "z");
        s.frame(9, 9).unwrap();
        assert_eq!(s.engine.checkpoint(), checkpoint, "Undoing the new stroke returns to the original layer");
        assert_live_artwork_eq(s.engine.document(), &original);
    }

    #[test]
    fn queued_pen_contact_starts_after_the_preceding_undo() {
        pen_contact_after_queued_undo(false);
    }

    #[test]
    fn undo_during_a_delayed_pen_contact_does_not_split_or_stall_it() {
        pen_contact_after_queued_undo(true);
    }

    #[test]
    fn closing_drawing_discards_deferred_commands_and_contacts() {
        let mut s = clip_session();
        invoke(&mut s, CommandId::AddLayer);
        s.frame(2, 2).unwrap();
        let checkpoint = s.engine.checkpoint();
        s.renderer_mut().settling = true;
        shortcut(&mut s, "z");
        pen_at(&mut s, 10, PenPhase::Down, [100., 100.]);
        pen_at(&mut s, 11, PenPhase::Up, [110., 100.]);
        s.request_session_close().unwrap();
        s.renderer_mut().settling = false;
        for tick in 3..7 { s.frame(tick, tick).unwrap(); }
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.engine.metrics().committed_strokes, 0);
        assert!(!s.wants_continuous_frames());
    }

    #[test]
    fn undo_after_pen_lift_waits_for_the_first_stroke_frame() {
        let mut s = clip_session();
        let original = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        pen_at(&mut s, 10, PenPhase::Down, [100., 100.]);
        pen_at(&mut s, 11, PenPhase::Up, [110., 100.]);
        shortcut(&mut s, "z");
        for tick in 2..8 { s.frame(tick, tick).unwrap(); }
        assert_eq!(s.engine.metrics().committed_strokes, 1);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_live_artwork_eq(s.engine.document(), &original);
    }

    fn contact_sizes_across_brush_change(deferred: bool, during_contact: bool) -> Vec<Vec<[f32; 2]>> {
        let mut s = clip_session();
        s.dispatch(UiAction::SetBrushSize { value: 19. }).unwrap();
        if deferred {
            invoke(&mut s, CommandId::AddLayer);
            s.frame(2, 2).unwrap();
            s.renderer_mut().settling = true;
            shortcut(&mut s, "z");
        }
        pen_at(&mut s, 10, PenPhase::Down, [100., 100.]);
        if during_contact { s.dispatch(UiAction::SetBrushSize { value: 43. }).unwrap(); }
        pen_at(&mut s, 11, PenPhase::Up, [110., 100.]);
        if !during_contact { s.dispatch(UiAction::SetBrushSize { value: 43. }).unwrap(); }
        pen_at(&mut s, 20, PenPhase::Down, [150., 100.]);
        pen_at(&mut s, 21, PenPhase::Up, [160., 100.]);
        s.renderer_mut().settling = false;
        let mut contacts = Vec::new();
        for tick in 3..13 {
            let before = s.engine.metrics().committed_strokes;
            let dabs = s.renderer_mut().recorded_dabs.len();
            s.frame(tick, tick).unwrap();
            if s.engine.metrics().committed_strokes != before {
                contacts.push(s.renderer_mut().recorded_dabs[dabs..].iter().map(|dab| dab.radii).collect());
            }
        }
        assert_eq!(contacts.len(), 2, "both contacts commit without stalling");
        assert_eq!(s.engine.configured_brush().diameter, 43.);
        assert_eq!(s.state.brush.diameter, 43.);
        contacts
    }

    #[test]
    fn queued_contact_keeps_the_brush_size_from_down_when_size_changes_during_contact() {
        let immediate = contact_sizes_across_brush_change(false, true);
        assert_ne!(immediate[0][0], immediate[1][0]);
        assert_eq!(contact_sizes_across_brush_change(true, true), immediate);
    }

    #[test]
    fn queued_contact_brush_size_changes_after_up_apply_to_the_next_contact() {
        let immediate = contact_sizes_across_brush_change(false, false);
        assert_ne!(immediate[0][0], immediate[1][0]);
        assert_eq!(contact_sizes_across_brush_change(true, false), immediate);
    }

    #[test]
    fn queued_contact_keeps_its_capture_coordinates_after_many_navigation_changes() {
        let draw = |deferred| {
            let mut s = clip_session();
            if deferred {
                invoke(&mut s, CommandId::AddLayer);
                s.frame(2, 2).unwrap();
            }
            s.renderer_mut().settling = true;
            if deferred { shortcut(&mut s, "z"); }
            pen_at(&mut s, 10, PenPhase::Down, [100., 100.]);
            pen_at(&mut s, 11, PenPhase::Up, [110., 100.]);
            let captured_camera = s.state.camera.clone();
            for _ in 0..128 { s.gesture([200., 200.], [201., 200.], 1., 0.).unwrap(); }
            let navigated_camera = s.state.camera.clone();
            assert_ne!(captured_camera, navigated_camera, "navigation stays responsive while the contact waits");
            s.renderer_mut().settling = false;
            for tick in 3..9 { s.frame(tick, tick).unwrap(); }
            assert_eq!(s.engine.metrics().committed_strokes, 1);
            assert_eq!(s.state.camera, navigated_camera, "replaying an old contact preserves the current view");
            s.renderer_mut().recorded_dabs.iter().map(|dab| dab.center).collect::<Vec<_>>()
        };
        let immediate = draw(false);
        assert!(!immediate.is_empty());
        assert_eq!(draw(true), immediate);
    }

    fn mixed_region_selection(s: &mut UiSession<Recorder>) -> [OccurrenceHandle; 3] {
        let paint = s.engine.document().working.occurrence.unwrap();
        let images = ["First photo", "Second photo"].map(|name| {
            let image = layer_core::ImageObject::new(rgba8_source([40, 30], |_, _| [255; 4]).into());
            let (layer, edit) = s.engine.document().create_object_layer_edit(name, image, None, 0).unwrap();
            s.layer_edit(edit).unwrap();
            layer
        });
        s.layer_edit(s.engine.document().select_occurrence_edit(images[1]).unwrap()).unwrap();
        let layers = [paint, images[0], images[1]];
        let mut working = s.engine.document().working.clone();
        working.layer_selection = layers.into();
        s.layer_edit(Edit::Working(working)).unwrap();
        select(s, Some(rectangle([10., 10., 20., 20.])));
        layers
    }

    #[test]
    fn copy_captures_the_active_layer_alone_before_its_properties() {
        let mut s = clip_session();
        let paint = s.engine.document().working.occurrence.unwrap();
        invoke(&mut s, CommandId::AddLayer);
        let doc = s.engine.document();
        let stack = RecordChange::insert(&doc.artwork.stacks, Stack { entries: vec![paint] });
        let mut group = Occurrence::new(OccurrenceContent::Stack(stack.handle), "Group");
        group.offset = [10, 20];
        let group = RecordChange::insert(&doc.artwork.occurrences, group);
        let root = doc.composition().result;
        let mut containing = doc.artwork.stacks.get(root).unwrap().clone();
        containing.entries.retain(|h| *h != paint);
        containing.entries.insert(0, group.handle);
        let mut occurrence = doc.scene().occurrence(paint).unwrap().clone();
        occurrence.offset = [7, 3];
        occurrence.blend = LayerBlend::Multiply;
        occurrence.attachment = layer_core::Attachment::None;
        occurrence.opacity = 0.4;
        occurrence.visible = false;
        let edit = Edit::Batch(vec![Edit::Stack(stack), Edit::Occurrence(group), Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, paint, Some(occurrence)).unwrap()), Edit::Stack(RecordChange::replace(&doc.artwork.stacks, root, Some(containing)).unwrap())]);
        s.layer_edit(edit).unwrap();
        s.layer_edit(s.engine.document().select_occurrence_edit(paint).unwrap()).unwrap();
        select(&mut s, Some(rectangle([30.2, 40., 130., 90.5])));
        assert!(s.command(CommandId::Copy).enabled, "{:?}", s.command_disabled_reason(CommandId::Copy));
        invoke(&mut s, CommandId::Copy);
        let (id, request) = pending(&s);
        assert!(matches!(request, DocumentRequest::Copy { merged: false, cut: false, pixels: false }));
        assert_eq!(s.command_disabled_reason(CommandId::Copy).as_deref(), Some("Wait for the current file operation"));
        let capture = s.capture_clipboard(id).unwrap();
        assert_eq!(capture.crop, [30, 40, 100, 51]);
        assert!(capture.coverage.is_some());
        assert!(capture.original.is_none());
        assert_eq!((capture.name.as_str(), capture.large), ("Current ink", false));
        let scene = capture.scene.view();
        let target = scene.source_target(paint).unwrap();
        assert_eq!(capture.scope, SceneScope::Raw(target), "only the active source, before occurrence properties");
        assert_eq!(scene.target_offset(target), [17, 23], "the group's offset is kept");
        let occurrence = scene.occurrence(paint).unwrap();
        assert!(!occurrence.visible && occurrence.opacity == 0.4 && occurrence.mask.is_none());
        assert!(scene.parent(paint).is_some());
        assert_eq!(occurrence.blend, LayerBlend::Multiply, "raw capture keeps authored properties untouched");
        s.complete_document_request(id, Ok(true)).unwrap();
        assert!(s.command(CommandId::Copy).enabled);

        invoke(&mut s, CommandId::CopyMerged);
        let (id, request) = pending(&s);
        assert!(matches!(request, DocumentRequest::Copy { merged: true, cut: false, pixels: false }));
        let merged = s.capture_clipboard(id).unwrap();
        assert_eq!(merged.scene.view().order().len(), s.engine.document().scene().order().len());
        assert_eq!(merged.scope, SceneScope::All);
        assert_eq!(merged.name, "Merged copy");
        s.complete_document_request(id, Ok(true)).unwrap();

        select(&mut s, None);
        invoke(&mut s, CommandId::Copy);
        let (id, _) = pending(&s);
        let whole = s.capture_clipboard(id).unwrap();
        assert_eq!(whole.crop, [0, 0, 400, 300], "without a selection the whole layer within the canvas");
        assert!(whole.coverage.is_none());
        s.complete_document_request(id, Ok(false)).unwrap();
    }

    #[test]
    fn select_all_on_an_untouched_photo_keeps_its_original_samples() {
        let mut s = clip_session();
        let photo = rgba8_source([400, 300], |x, y| [x as u8, y as u8, 9, 255]);
        s.import_layer_source("Photo", Arc::unwrap_or_clone(photo.clone())).unwrap();
        invoke(&mut s, CommandId::SelectAll);
        invoke(&mut s, CommandId::Copy);
        let (id, _) = pending(&s);
        let capture = s.capture_clipboard(id).unwrap();
        assert!(capture.coverage.is_none(), "Select All needs no coverage");
        assert!(capture.large == (400 * 300 > LARGE_CLIP_PIXELS));
        let original = capture.original.expect("the photo's own source");
        assert_eq!(*original, *s.engine.document().scene().paint_source(s.engine.document().working.occurrence.unwrap()).unwrap().base.as_ref().unwrap().image.storage().clone());
        s.complete_document_request(id, Ok(true)).unwrap();

        select(&mut s, Some(rectangle([10., 10., 50., 50.])));
        invoke(&mut s, CommandId::Copy);
        let (id, _) = pending(&s);
        assert!(s.capture_clipboard(id).unwrap().original.is_none(), "a selection composes the copy");
        s.complete_document_request(id, Ok(true)).unwrap();
    }

    #[test]
    fn copy_commands_explain_why_they_are_unavailable() {
        let mut s = clip_session();
        let reason = |s: &UiSession<Recorder>, command| s.command_disabled_reason(command);
        assert!(s.command(CommandId::Cut).enabled, "Cut removes whole layers without a pixel selection");
        assert_eq!(reason(&s, CommandId::PasteInto).as_deref(), Some("Make a selection to paste into"));
        assert!(s.command(CommandId::Copy).enabled && s.command(CommandId::CopyMerged).enabled);
        assert!(s.command(CommandId::PasteInPlace).enabled && s.command(CommandId::PasteImage).enabled);
        select(&mut s, Some(rectangle([500., 400., 600., 500.])));
        assert_eq!(reason(&s, CommandId::Copy).as_deref(), Some("The selection doesn't cover any of the canvas"));
        assert!(s.dispatch(UiAction::Invoke { command: CommandId::Copy }).is_err());
        assert!(s.state.requests.is_empty());
        select(&mut s, Some(rectangle([10., 10., 60., 60.])));
        assert!(s.command(CommandId::Cut).enabled && s.command(CommandId::PasteInto).enabled);
        let paint = s.engine.document().working.occurrence.unwrap();
        let paper = occurrence_handle(2).unwrap();
        s.layer_edit(s.engine.document().select_occurrence_edit(paper).unwrap()).unwrap();
        assert!(s.command(CommandId::Copy).enabled, "a fill layer supplies its own pixels");
        select(&mut s, None);
        insert_effect(&mut s, "gaussian_blur");
        select(&mut s, Some(rectangle([10., 10., 60., 60.])));
        assert_eq!(reason(&s, CommandId::Copy).as_deref(), Some("An effect layer has no pixels of its own"));
        assert!(s.command(CommandId::CopyMerged).enabled, "Copy Merged ignores the active layer");
        s.layer_edit(s.engine.document().select_occurrence_edit(paint).unwrap()).unwrap();
        let mut layer = s.engine.document().scene().occurrence(paint).unwrap().clone();
        layer.alpha_locked = true;
        s.layer_edit(Edit::Occurrence(RecordChange::replace(&s.engine.document().artwork.occurrences, paint, Some(layer)).unwrap())).unwrap();
        assert!(s.command(CommandId::Copy).enabled);
        assert_eq!(reason(&s, CommandId::Cut).as_deref(), Some("Alpha lock keeps transparency; unlock the layer first"));
        invoke(&mut s, CommandId::QuickMask);
        for command in [CommandId::Copy, CommandId::Cut, CommandId::CopyMerged] {
            assert_eq!(reason(&s, command).as_deref(), Some("Quick Mask edits the selection; leave it to copy artwork"));
        }
        for platform in Platform::ALL {
            for command in [CommandId::Copy, CommandId::Cut, CommandId::CopyMerged, CommandId::PasteInPlace, CommandId::PasteInto] {
                assert!(command.available_on(platform), "{command:?} on {platform:?}");
            }
            assert!(CommandId::PasteImage.available_on(platform));
        }
    }

    #[test]
    fn a_clip_is_document_pixels_only_where_the_colour_settings_match() {
        let color = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 };
        let clip = clip([4, 4], [0, 0], color);
        assert_eq!(clip.source_for(color).policy, layer_core::PaintBasePolicy::WorkingPixels);
        for other in [
            DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 },
            DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U8 },
        ] {
            let source = clip.source_for(other);
            assert_eq!(source.policy, layer_core::PaintBasePolicy::SourceProfile);
            assert_eq!(source.image.interpretation, clip.source.interpretation, "an explicit profile");
        }
        let mut original = clip.clone();
        original.policy = layer_core::PaintBasePolicy::SourceProfile;
        assert_eq!(original.source_for(color).policy, layer_core::PaintBasePolicy::SourceProfile, "a copied photo keeps its original");
    }

    #[test]
    fn paste_preserves_position_and_shown_position_centres_in_one_step() {
        let mut s = clip_session();
        let before = s.engine.document().scene().order().len();
        let visible = clip([20, 10], [100, 50], s.engine.document().composition().color);
        s.paste_clip(&visible, PasteMode::Paste).unwrap();
        assert_eq!(s.engine.document().scene().order().len(), before + 1);
        assert_eq!(translation(&s), [100., 50.]);
        assert!(!s.operation.placing(), "no handles");
        let doc = s.engine.document();
        let layer = doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap();
        assert_eq!(layer.name.as_ref(), "Ink");
        assert_eq!(doc.target_extent(doc.working.target.unwrap()), [400, 300]);
        assert_eq!(doc.scene().paint_source(doc.working.occurrence.unwrap()).unwrap().base.as_ref().unwrap().image.extent, [20, 10]);
        assert_eq!(doc.scene().paint_source(doc.working.occurrence.unwrap()).unwrap().base.as_ref().unwrap().policy, layer_core::PaintBasePolicy::WorkingPixels);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().scene().order().len(), before, "one undo step");

        s.state.camera.zoom = 8.;
        s.state.camera.center_on([30., 30.]);
        let hidden = clip([20, 10], [300, 250], s.engine.document().composition().color);
        s.paste_clip(&hidden, PasteMode::Paste).unwrap();
        assert_eq!(translation(&s), [300., 250.], "CSP Paste keeps even an off-screen position");
        invoke(&mut s, CommandId::Undo);
        s.paste_clip(&hidden, PasteMode::AtView).unwrap();
        assert_eq!(translation(&s), [20., 25.], "centred in the view");
        invoke(&mut s, CommandId::Undo);
        for (rotation, expected) in [(0., Some([21., 26.])), (0.3, None)] {
            s.state.camera.rotation = rotation;
            s.state.camera.center_on([30.5, 30.5]);
            s.paste_clip(&hidden, PasteMode::AtView).unwrap();
            let view = s.state.camera.surface_to_document64(s.state.camera.work_area_center().map(f64::from));
            let rounded = [(view[0] - 10.).round() as f32, (view[1] - 5.).round() as f32];
            assert_eq!(translation(&s), expected.unwrap_or(rounded), "paint rounds the F64 view centre, halves away from zero");
            assert_eq!(translation(&s), rounded);
            invoke(&mut s, CommandId::Undo);
        }
        s.state.camera.rotation = 0.;
        s.paste_clip(&hidden, PasteMode::InPlace).unwrap();
        assert_eq!(translation(&s), [300., 250.], "Paste in Place keeps the position");
        invoke(&mut s, CommandId::Undo);
        let wide = clip([600, 10], [0, 0], s.engine.document().composition().color);
        s.paste_clip(&wide, PasteMode::InPlace).unwrap();
        let doc = s.engine.document();
        assert_eq!(doc.target_extent(doc.working.target.unwrap()), [600, 300]);
        assert_eq!(doc.scene().paint_source(doc.working.occurrence.unwrap()).unwrap().base.as_ref().unwrap().image.extent, [600, 10]);
        invoke(&mut s, CommandId::Undo);
        s.state.settings.keymap = Some(crate::keymaps::KeymapRef { id: "photoshop".into(), revision: 11 });
        s.paste_clip(&hidden, PasteMode::Paste).unwrap();
        assert_eq!(translation(&s), [190., 145.], "Photoshop Paste centres in the document even when the view is panned");
    }

    #[test]
    fn paste_into_masks_a_new_image_layer_with_the_selection_in_one_step() {
        let mut s = clip_session();
        let selection = rectangle([110., 55., 115., 58.]);
        select(&mut s, Some(selection.clone()));
        let before = s.engine.document().clone();
        let clip = clip([20, 10], [100, 50], DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U8 });
        s.paste_clip(&clip, PasteMode::Into).unwrap();
        let doc = s.engine.document();
        let handle = doc.working.occurrence.unwrap();
        let layer = doc.scene().occurrence(handle).unwrap();
        let object = doc.scene().object_handle(handle).expect("Paste Into makes an image layer");
        assert_eq!(doc.selected_objects(), [object].into());
        let pasted = doc.scene().object(object).unwrap();
        assert_eq!(pasted.affine, layer_core::Affine64([1., 0., 0., 1., 103., 52.]), "the clip is centred on the selection");
        assert_eq!(pasted.image.interpretation, clip.source.interpretation, "the image keeps its own colour interpretation");
        let mask = layer.mask.as_ref().expect("a mask from the selection");
        let (source, default_coverage, consumed) = (mask.source, doc.artwork.coverage.get(mask.source).unwrap().default_coverage, doc.working.selection.is_none());
        assert_eq!(crate::session::test_support::stored_selection(&mut s, source).unwrap().shape, selection.shape);
        assert_eq!(default_coverage, 0.);
        assert!(consumed, "the selection became the mask");
        invoke(&mut s, CommandId::Undo);
        assert_live_artwork_eq(s.engine.document(), &before);
        assert_eq!(s.engine.document().working.selection, before.working.selection, "one undo step restores the selection");
        select(&mut s, None);
        assert!(s.paste_clip(&clip, PasteMode::Into).is_err());
    }

    #[test]
    fn placed_and_pasted_names_keep_to_the_shared_name_limit() {
        let mut s = clip_session();
        let long = format!("  {}\u{7}photo.png", "a".repeat(300));
        let source = || Arc::unwrap_or_clone(rgba8_source([8, 8], |_, _| [1, 2, 3, 255]));
        let bounded = |name: &str| name.chars().count() == layer_core::MAX_NAME_CHARS && name.chars().all(|c| c == 'a');
        let layer_name = |s: &UiSession<Recorder>| {
            let doc = s.engine.document();
            doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap().name.clone()
        };
        s.place_layer_source(&long, source(), None).unwrap();
        assert!(bounded(&layer_name(&s)), "place");
        invoke(&mut s, CommandId::CancelTransform);
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(vec![(long.clone(), source())], PasteMode::InPlace, &context).unwrap();
        assert!(bounded(&layer_name(&s)), "paste in place");
        let mut copied = clip([8, 8], [4, 4], DocumentColor::default());
        copied.name = long.clone();
        s.paste_clip(&copied, PasteMode::Paste).unwrap();
        assert!(bounded(&layer_name(&s)), "paste");
        select(&mut s, Some(rectangle([10., 10., 50., 50.])));
        s.paste_clip(&copied, PasteMode::Into).unwrap();
        assert!(bounded(&layer_name(&s)), "paste into");
        let kept = "é".repeat(layer_core::MAX_NAME_BYTES / 2);
        let mut object = layer_core::ImageObject::new(layer_core::Image::new(Arc::new(source())));
        object.affine = layer_core::Affine64([1., 0., 0., 1., 4., 4.]);
        retain_image_layers(&mut copied, vec![(kept.clone().into(), object)]);
        s.paste_clip(&copied, PasteMode::Paste).unwrap();
        assert_eq!(layer_name(&s).as_ref(), kept, "a copied image keeps the name the drawing admitted");
        assert!(s.place_layer_source("\u{7} ", source(), None).is_err(), "a name of only spaces and controls is refused");
        assert!(s.import_layer_source(&long, source()).is_err(), "import keeps names literally and refuses ones past the limit");
    }

    #[test]
    fn external_paste_keeps_full_size_and_in_place_uses_the_canvas_origin() {
        let mut s = clip_session();
        let image = || vec![("Clipboard image".to_string(), Arc::unwrap_or_clone(rgba8_source([40, 20], |_, _| [1; 4])))];
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(image(), PasteMode::InPlace, &context).unwrap();
        assert!(!s.objects.placing());
        let doc = s.engine.document();
        let object = doc.scene().object_handle(doc.working.occurrence.unwrap()).unwrap();
        assert_eq!(doc.scene().object(object).unwrap().affine, layer_core::Affine64([1., 0., 0., 1., 0., 0.]));
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(image(), PasteMode::Paste, &context).unwrap();
        assert!(s.objects.placing(), "an image from another app opens the placement handles");
        invoke(&mut s, CommandId::CancelTransform);
        let context = s.image_placement_context(None, None).unwrap();
        invoke(&mut s, CommandId::AddLayer);
        assert!(s.paste_layer_sources(image(), PasteMode::InPlace, &context).is_err(), "a stale context is refused");
    }

    #[test]
    fn external_images_paste_as_named_siblings_and_select_all_selects_canvas_pixels() {
        let mut s = clip_session();
        let image = |name: &str, extent| (name.to_string(), Arc::unwrap_or_clone(rgba8_source(extent, |_, _| [90, 120, 200, 255])));
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(vec![image("First", [8, 6])], PasteMode::InPlace, &context).unwrap();
        let before = s.engine.document().clone();
        let first = before.working.occurrence.unwrap();
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(vec![image("Wide", [9, 3]), image("Tall", [2, 11])], PasteMode::InPlace, &context).unwrap();
        let document = s.engine.document();
        let selected = document.working.layer_selection.clone();
        assert_eq!(selected.len(), 2);
        assert!(!selected.contains(&first));
        assert_eq!(document.selected_objects().len(), 2);
        assert_eq!(document.scene().occurrence(first), before.scene().occurrence(first));
        assert_eq!(document.scene().object_layer(first), before.scene().object_layer(first));
        let images = document.scene().children(None).iter().filter_map(|&id| {
            let object = document.scene().object_layer(id)?;
            assert_eq!(document.scene().parent(id), None);
            Some((document.scene().occurrence(id).unwrap().name.as_ref(), object.image.extent))
        }).collect::<Vec<_>>();
        assert_eq!(images, [("Wide", [9, 3]), ("Tall", [2, 11]), ("First", [8, 6])]);
        invoke(&mut s, CommandId::SelectAll);
        let document = s.engine.document();
        assert_eq!(document.working.layer_selection, selected);
        assert_eq!(document.working.selection.as_ref().unwrap().shape, rectangle([0., 0., 400., 300.]).shape);
        invoke(&mut s, CommandId::Copy);
        let (id, _) = pending(&s);
        let capture = s.capture_clipboard(id).unwrap();
        assert_eq!(capture.layers.as_ref().unwrap().roots.len(), 2);
        assert_eq!(capture.layer_captures().len(), 2);
        s.complete_document_request(id, Ok(false)).unwrap();
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().working.selection.is_none(), "Select All is its own undo step");
        invoke(&mut s, CommandId::Undo);
        assert_live_artwork_eq(s.engine.document(), &before);
    }

    #[test]
    fn object_layer_cut_uses_retained_layers_and_undo_restores_named_siblings() {
        let mut s = clip_session();
        let context = s.image_placement_context(None, None).unwrap();
        let images = ["First", "Second"].map(|name| (name.into(), Arc::unwrap_or_clone(rgba8_source([8, 6], |_, _| [255; 4])))).to_vec();
        s.paste_layer_sources(images, PasteMode::InPlace, &context).unwrap();
        let before = s.engine.document().clone();
        let selected = before.working.layer_selection.clone();
        invoke(&mut s, CommandId::Cut);
        let (id, _) = pending(&s);
        let capture = s.capture_clipboard(id).unwrap();
        assert_eq!(capture.layers.as_ref().unwrap().roots.len(), 2);
        assert!(capture.layer_captures().is_empty(), "whole object layers retain their original images");
        let extent = [capture.crop[2], capture.crop[3]];
        let copied = capture.finish("object-layers".into(), rgba8_source(extent, |_, _| [0; 4]), vec![]).unwrap();
        assert_live_artwork_eq(s.engine.document(), &before);
        s.complete_document_request(id, Ok(true)).unwrap();
        assert!(selected.iter().all(|&layer| s.engine.document().scene().occurrence(layer).is_none()));
        invoke(&mut s, CommandId::Undo);
        assert_live_artwork_eq(s.engine.document(), &before);
        s.paste_clip(&copied, PasteMode::InPlace).unwrap();
        let document = s.engine.document();
        assert_eq!(document.working.layer_selection.len(), 2);
        assert!(document.working.layer_selection.is_disjoint(&selected));
        let names = document.scene().order().iter().filter(|id| document.working.layer_selection.contains(id))
            .map(|id| document.scene().occurrence(*id).unwrap().name.as_ref()).collect::<Vec<_>>();
        assert_eq!(names, ["First", "Second"]);
        for id in &document.working.layer_selection { assert_eq!(document.scene().object_layer(*id).unwrap().image.extent, [8, 6]); }
        invoke(&mut s, CommandId::Undo);
        assert_live_artwork_eq(s.engine.document(), &before);
    }

    #[test]
    fn focused_object_layer_mask_takes_precedence_over_derived_object_selection() {
        let mut s = clip_session();
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(vec![("Photo".into(), Arc::unwrap_or_clone(rgba8_source([20, 10], |_, _| [255; 4])))], PasteMode::InPlace, &context).unwrap();
        let mask = focused_mask(&mut s);
        assert_eq!(s.engine.document().selected_objects().len(), 1);
        invoke(&mut s, CommandId::Copy);
        let (id, _) = pending(&s);
        let capture = s.capture_clipboard(id).unwrap();
        assert_eq!(capture.scope, SceneScope::Raw(mask));
        assert!(capture.layers.is_none());
        s.complete_document_request(id, Ok(true)).unwrap();
        for external in [false, true] {
            let before = s.engine.document().clone();
            let copied = clip([4, 4], [0, 0], before.composition().color);
            if external {
                let context = s.image_placement_context(None, None).unwrap();
                s.paste_layer_sources(vec![("Mask pixels".into(), (*copied.source).clone())], PasteMode::InPlace, &context).unwrap();
            } else {
                s.paste_clip(&copied, PasteMode::InPlace).unwrap();
            }
            s.frame(2, 2).unwrap();
            assert_eq!(s.engine.document().scene().order(), before.scene().order());
            assert_eq!(s.engine.document().artwork.objects.iter().collect::<Vec<_>>(), before.artwork.objects.iter().collect::<Vec<_>>());
            let operations = &s.renderer_mut().pending_operations;
            assert_eq!(operations.len(), 1);
            assert!(operations.iter().all(|(target, operation)| *target == mask && matches!(operation.kind, RasterOperationKind::Bake { .. })));
            invoke(&mut s, CommandId::Undo);
            assert_live_artwork_eq(s.engine.document(), &before);
        }
    }

    #[test]
    fn converted_layer_clips_keep_hierarchy_masks_originals_and_atomic_paste() {
        let mut source_session = clip_session();
        let first = source_session.engine.document().working.occurrence.unwrap();
        let original_pixels = rgba8_source([2, 1], |x, _| [31 + x as u8, 73, 127, 255]);
        source_session.import_layer_source("Original", Arc::unwrap_or_clone(original_pixels)).unwrap();
        let second = source_session.engine.document().working.occurrence.unwrap();
        let document = source_session.engine.document();
        let original = document.scene().paint_source(second).unwrap().base.as_ref().unwrap().image.storage().clone();
        let OccurrenceContent::Paint(handle) = document.scene().occurrence(first).unwrap().content else { unreachable!() };
        let mut paint = document.artwork.paint.get(handle).unwrap().clone();
        paint.base = Some(layer_core::PaintBase { image: rgba8_source([2, 1], |_, _| [64, 128, 192, 128]).into(), offset: [0; 2], policy: layer_core::PaintBasePolicy::WorkingPixels });
        source_session.layer_edit(Edit::Paint(RecordChange::replace(&document.artwork.paint, handle, Some(paint)).unwrap())).unwrap();
        layer(&mut source_session, LayerAction::AddMask { id: occurrence_token(second), replace: false });
        let document = source_session.engine.document();
        let mut masked = document.scene().occurrence(second).unwrap().clone();
        masked.opacity = 0.375;
        masked.blend = LayerBlend::Multiply;
        let mask = masked.mask.as_mut().unwrap();
        mask.offset = [7, 11]; mask.linked = false; mask.enabled = false; mask.inverted = true;
        source_session.layer_edit(Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, second, Some(masked)).unwrap())).unwrap();
        invoke(&mut source_session, CommandId::EditLayerContent);
        let edit = source_session.engine.document().group_layers_edit(&[first, second], LayerBlend::Normal, "Clipboard group").unwrap();
        source_session.layer_edit(edit).unwrap();
        let group = *source_session.engine.document().scene().order().iter().find(|h| source_session.engine.document().scene().occurrence(**h).unwrap().name.as_ref() == "Clipboard group").unwrap();
        let edit = source_session.engine.document().select_occurrence_edit(group).unwrap();
        source_session.layer_edit(edit).unwrap();
        let document = source_session.engine.document();
        let mut occurrence = document.scene().occurrence(group).unwrap().clone();
        occurrence.offset = [20, 30]; occurrence.opacity = 0.625;
        source_session.layer_edit(Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, group, Some(occurrence)).unwrap())).unwrap();
        invoke(&mut source_session, CommandId::Copy);
        let (id, _) = pending(&source_session);
        let capture = source_session.capture_clipboard(id).unwrap();
        let extent = [capture.crop[2], capture.crop[3]];
        let copied = capture.finish("convert-layers".into(), rgba8_source(extent, |_, _| [0; 4]), vec![]).unwrap();
        source_session.complete_document_request(id, Ok(true)).unwrap();
        let retained = copied.layers.as_ref().unwrap().clone();
        let source_artwork = retained.scene.artwork.clone();
        let colors = [DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 }, DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::F32 }];
        for color in colors {
            let mut converted = copied.clone();
            converted.convert_layers(color, 16 * 1024 * 1024, || false).unwrap();
            assert_eq!(converted.layers.as_ref().unwrap().scene.view().composition().color, color);
            assert!(Arc::ptr_eq(&converted.source, &copied.source));
            let mut document = Document::new(PortableId::random(), 400, 300, layer_core::DocumentNames { paint: "Destination".into(), paper: "Paper".into() });
            document.artwork.compositions.get_mut(document.artwork.root).unwrap().color = color;
            let mut destination = UiSession::new(Recorder { color, tiled_sources: true, ..Default::default() }, document, [800, 600], Platform::Gtk).unwrap();
            let before = destination.engine.document().clone();
            destination.paste_clip(&converted, PasteMode::InPlace).unwrap();
            let document = destination.engine.document();
            assert_eq!(document.composition().color, color);
            let group = document.working.occurrence.unwrap();
            assert_eq!(document.scene().occurrence(group).unwrap().name.as_ref(), "Clipboard group");
            assert_eq!(document.scene().occurrence(group).unwrap().opacity, 0.625);
            assert_eq!(document.layer_offset(group), [20, 30]);
            let children = document.scene().children(Some(group));
            assert_eq!(children.len(), 2);
            let masked = document.scene().occurrence(children[0]).unwrap();
            assert_eq!((masked.name.as_ref(), masked.opacity, masked.blend), ("Original", 0.375, LayerBlend::Multiply));
            let mask = masked.mask.as_ref().unwrap();
            assert_eq!((mask.offset, mask.linked, mask.enabled, mask.inverted), ([7, 11], false, false, true));
            assert_eq!(document.artwork.coverage.get(mask.source).unwrap().default_coverage, 1.);
            assert!(Arc::ptr_eq(document.scene().paint_source(children[0]).unwrap().base.as_ref().unwrap().image.storage(), &original));
            let working = document.scene().paint_source(children[1]).unwrap().base.as_ref().unwrap();
            assert_eq!((working.policy, working.image.interpretation.depth, &working.image.interpretation.profile), (layer_core::PaintBasePolicy::WorkingPixels, color.depth, &layer_core::color::ColorProfile::Builtin(color.space)));
            let mut row = vec![0; working.image.row_bytes()];
            working.image.rows().read(0, &mut row).unwrap();
            match color.depth {
                SampleDepth::U16 => assert_eq!(u16::from_le_bytes(row[6..8].try_into().unwrap()), 128 * 257),
                SampleDepth::F32 => assert!((f32::from_le_bytes(row[12..16].try_into().unwrap()) - 128. / 255.).abs() < 1e-6),
                _ => unreachable!(),
            }
            document.validate(Default::default()).unwrap();
            layer_color::validate_document_color(document).unwrap();
            invoke(&mut destination, CommandId::Undo);
            crate::session::test_support::assert_live_artwork_eq(destination.engine.document(), &before);

            let converted_layers = converted.layers.as_ref().unwrap().clone();
            if color.depth.is_float() {
                assert!(converted.convert_layers(DocumentColor::default(), 16 * 1024 * 1024, || false).is_err());
                assert!(Arc::ptr_eq(converted.layers.as_ref().unwrap(), &converted_layers));
            } else {
                converted.convert_layers(DocumentColor::default(), 16 * 1024 * 1024, || false).unwrap();
                assert_eq!(converted.layers.as_ref().unwrap().scene.view().composition().color, DocumentColor::default());
            }
        }
        for cancel_after in [0, 3] {
            let mut cancelled = copied.clone();
            let mut checks = 0;
            assert!(cancelled.convert_layers(colors[0], 16 * 1024 * 1024, || { checks += 1; checks > cancel_after }).is_err());
            assert!(Arc::ptr_eq(cancelled.layers.as_ref().unwrap(), &retained));
        }
        assert_eq!(copied.layers.as_ref().unwrap().scene.artwork, source_artwork);
    }

    #[test]
    fn whole_layer_clipboard_preserves_multiple_layers_and_cuts_only_after_publication() {
        let mut s = clip_session();
        let first = s.engine.document().working.occurrence.unwrap();
        invoke(&mut s, CommandId::AddLayer);
        let second = s.engine.document().working.occurrence.unwrap();
        let mut working = s.engine.document().working.clone(); working.layer_selection = [first, second].into();
        s.layer_edit(Edit::Working(working)).unwrap();
        invoke(&mut s, CommandId::Cut);
        let (id, _) = pending(&s); let capture = s.capture_clipboard(id).unwrap();
        assert_eq!(capture.layers.as_ref().unwrap().roots, [second, first]);
        assert!(s.engine.document().scene().occurrence(first).is_some());
        s.complete_document_request(id, Ok(false)).unwrap();
        assert!(s.engine.document().scene().occurrence(first).is_some(), "failed publication preserves layers");
        invoke(&mut s, CommandId::Cut);
        let (id, _) = pending(&s); let capture = s.capture_clipboard(id).unwrap();
        let copied = capture.finish("whole".into(), rgba8_source([400, 300], |_, _| [0; 4]), vec![]).unwrap();
        assert_eq!(copied.layers.as_ref().unwrap().scene.view().order().len(), 2, "the clip does not retain unselected layers or paper");
        s.complete_document_request(id, Ok(true)).unwrap();
        assert!(s.engine.document().scene().occurrence(first).is_none());
        assert!(s.engine.document().scene().occurrence(second).is_none());
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().scene().occurrence(first).is_some());
        let count = s.engine.document().scene().order().len();
        s.paste_clip(&copied, PasteMode::Paste).unwrap();
        assert_eq!(s.engine.document().scene().order().len(), count + 2);
        assert_eq!(s.engine.document().working.layer_selection.len(), 2);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().scene().order().len(), count);
        let opened = copied.document(s.localization()).unwrap();
        assert_eq!(opened.composition().size, [400, 300]);
        assert_eq!(opened.scene().order().len(), 3, "copied layers and transparent paper");
        assert!(opened.artwork.paint.iter().all(|(_, _, paint)| paint.base.is_none()));
        for platform in Platform::ALL {
            let mut pasted = UiSession::from_project(Recorder::default(), opened.clone(), None, [800, 600], platform).unwrap();
            assert!(pasted.state.document_file.modified, "copied authored layers need their own save on {platform:?}");
            assert!(!pasted.engine.can_undo());
            pasted.request_document_close().unwrap();
            let (id, request) = pending(&pasted);
            assert!(matches!(request, DocumentRequest::ConfirmClose { .. }));
            pasted.respond_document_close(id, CloseDecision::Cancel).unwrap();
            pasted.initialize_document_location(Some(DocumentLocation { uri: "private:copied-layers".into(), name: "Copied.capy".into() })).unwrap();
            assert!(!pasted.state.document_file.modified);
        }
    }

    #[test]
    fn selected_regions_keep_group_structure_offsets_and_masks_when_pasted() {
        let mut s = clip_session();
        let [first, second] = selected_pair(&mut s);
        layer(&mut s, LayerAction::AddMask { id: occurrence_token(second), replace: false });
        invoke(&mut s, CommandId::EditLayerContent);
        let edit = s.engine.document().group_layers_edit(&[first, second], LayerBlend::Normal, "Region group").unwrap();
        s.layer_edit(edit).unwrap();
        let group = s.engine.document().scene().parent(first).unwrap();
        let doc = s.engine.document();
        let mut occurrence = doc.scene().occurrence(group).unwrap().clone();
        occurrence.offset = [20, -10]; occurrence.opacity = 0.625;
        s.layer_edit(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, group, Some(occurrence)).unwrap())).unwrap();
        let doc = s.engine.document();
        let mut occurrence = doc.scene().occurrence(second).unwrap().clone();
        occurrence.offset = [-3, 7]; occurrence.blend = LayerBlend::Multiply; occurrence.opacity = 0.4;
        occurrence.mask.as_mut().unwrap().offset = [8, -2];
        s.layer_edit(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, second, Some(occurrence)).unwrap())).unwrap();
        let mask = s.engine.document().scene().occurrence(second).unwrap().mask.as_ref().unwrap().source;
        let mask_origin = s.engine.document().target_offset(SourceTarget::Coverage(mask));
        s.layer_edit(s.engine.document().select_occurrence_edit(group).unwrap()).unwrap();
        let mut working = s.engine.document().working.clone();
        working.layer_selection.insert(first);
        s.layer_edit(Edit::Working(working)).unwrap();
        let selection = rectangle([30., 40., 90., 80.]);
        select(&mut s, Some(selection.clone()));
        invoke(&mut s, CommandId::Copy);
        let (id, _) = pending(&s);
        let mut capture = s.capture_clipboard(id).unwrap();
        assert_eq!(capture.layers.as_ref().unwrap().roots, [group], "selected descendants are copied once");
        assert_eq!(capture.crop, [30, 40, 60, 40]);
        assert_eq!(capture.origin, [30, 40]);
        let captures = capture.layer_captures();
        assert_eq!(captures.len(), 2);
        for (layer, part) in captures {
            assert!([first, second].contains(&layer));
            assert_eq!(part.scope, SceneScope::Raw(s.engine.document().scene().source_target(layer).unwrap()));
            assert_eq!(part.coverage.as_deref(), Some(&selection));
            assert_eq!(part.crop, capture.crop);
            assert!(part.layers.is_none() && part.layer_captures().is_empty());
            capture.set_layer_source(layer, rgba8_source([60, 40], |_, _| [90, 120, 200, 128])).unwrap();
        }
        capture.finish_layer_sources().unwrap();
        assert!(capture.coverage.is_none(), "each layer's pixels already contain the selected coverage");
        assert!(capture.layer_captures().is_empty());
        let scene = capture.scene.view();
        for layer in [first, second] {
            let paint = scene.paint_source(layer).unwrap();
            assert_eq!(paint.domain, [60, 40]);
            assert_eq!(scene.layer_origin(Some(layer)), [30, 40]);
            assert_eq!(paint.base.as_ref().unwrap().image.extent, [60, 40]);
        }
        assert_eq!(scene.target_offset(SourceTarget::Coverage(mask)), mask_origin);
        let copied = capture.finish("regions".into(), rgba8_source([60, 40], |_, _| [0; 4]), vec![]).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        let mut destination = clip_session();
        let before = destination.engine.document().clone();
        destination.paste_clip(&copied, PasteMode::InPlace).unwrap();
        let doc = destination.engine.document();
        let pasted = doc.working.occurrence.unwrap();
        assert_eq!(doc.scene().occurrence(pasted).unwrap().name.as_ref(), "Region group");
        assert_eq!(doc.layer_offset(pasted), [20, -10]);
        let children = doc.scene().children(Some(pasted));
        assert_eq!(children.len(), 2);
        for &layer in children { assert_eq!(doc.layer_offset(layer), [30, 40]); }
        let masked = doc.scene().occurrence(children[0]).unwrap();
        assert_eq!((masked.blend, masked.opacity), (LayerBlend::Multiply, 0.4));
        assert_eq!(doc.target_offset(SourceTarget::Coverage(masked.mask.as_ref().unwrap().source)), mask_origin);
        doc.validate(Default::default()).unwrap();
        invoke(&mut destination, CommandId::Undo);
        assert_live_artwork_eq(destination.engine.document(), &before);
    }

    #[test]
    fn multi_layer_region_cut_waits_for_publication_and_undoes_in_one_step() {
        let mut s = clip_session();
        let layers = selected_pair(&mut s);
        select(&mut s, Some(rectangle([30., 40., 90., 80.])));
        let before = s.engine.document().clone();
        for published in [false, true] {
            invoke(&mut s, CommandId::Cut);
            let (id, _) = pending(&s);
            let capture = s.capture_clipboard(id).unwrap();
            assert_eq!(capture.layer_captures().len(), 2);
            assert_live_artwork_eq(s.engine.document(), &before);
            s.complete_document_request(id, Ok(published)).unwrap();
            s.frame(2, 2).unwrap();
            if !published {
                assert_live_artwork_eq(s.engine.document(), &before);
                assert!(s.renderer_mut().pending_operations.is_empty());
                continue;
            }
            assert_eq!(s.engine.document().scene().order().len(), before.scene().order().len());
            assert_eq!(s.engine.document().working, before.working);
            let operations = s.renderer_mut().pending_operations.clone();
            assert_eq!(operations.len(), 2);
            for layer in layers {
                let target = before.scene().source_target(layer).unwrap();
                assert!(operations.iter().any(|(actual, op)| *actual == target && matches!(op.kind, RasterOperationKind::Erase { alpha_locked: false })));
            }
            invoke(&mut s, CommandId::Undo);
            assert_live_artwork_eq(s.engine.document(), &before);
            assert_eq!(s.engine.document().working, before.working);
        }
    }

    #[test]
    fn region_copy_keeps_own_pixels_while_whole_layer_copy_keeps_adjustments() {
        let mut s = clip_session();
        let [first, second] = selected_pair(&mut s);
        s.dispatch(UiAction::Effect { action: EffectAction::InsertAttached {
            effect: "gaussian_blur".into(), owner: occurrence_token(first), epoch: s.state.document_file.epoch,
        } }).unwrap();
        let blur = s.engine.document().working.occurrence.unwrap();
        s.layer_edit(s.engine.document().select_occurrence_edit(second).unwrap()).unwrap();
        let mut working = s.engine.document().working.clone(); working.layer_selection = [first, second].into();
        s.layer_edit(Edit::Working(working)).unwrap();
        for regional in [false, true] {
            select(&mut s, regional.then(|| rectangle([30., 40., 90., 80.])));
            invoke(&mut s, CommandId::Copy);
            let (id, _) = pending(&s);
            let mut capture = s.capture_clipboard(id).unwrap();
            let extent = [capture.crop[2], capture.crop[3]];
            for (layer, part) in capture.layer_captures() {
                assert!([first, second].contains(&layer));
                assert!(matches!(part.scope, SceneScope::Raw(_)));
                capture.set_layer_source(layer, rgba8_source(extent, |_, _| [255; 4])).unwrap();
            }
            capture.finish_layer_sources().unwrap();
            let copied = capture.finish("effects".into(), rgba8_source(extent, |_, _| [255; 4]), vec![]).unwrap();
            let layers = copied.layers.as_ref().unwrap();
            let has_blur = layers.scene.artwork.effects.iter().any(|(_, _, effect)| effect.program.id.as_ref() == "gaussian_blur");
            assert_eq!(has_blur, !regional);
            assert_eq!(layers.roots.len(), if regional { 2 } else { 3 });
            Document::from_artwork(layers.scene.artwork.clone()).unwrap().validate(Default::default()).unwrap();
            assert!(s.engine.document().scene().effect(blur).is_some(), "copy never changes the source effects");
            s.complete_document_request(id, Ok(true)).unwrap();
        }
    }

    #[test]
    fn generators_and_their_groups_copy_regions_and_cut_atomically_after_capture() {
        for select_group in [false, true] {
            let mut s = clip_session();
            insert_effect(&mut s, "gradient_fill");
            let fill = s.engine.document().working.occurrence.unwrap();
            s.layer_edit(s.engine.document().group_layers_edit(&[fill], LayerBlend::Normal, "Fill group").unwrap()).unwrap();
            let group = s.engine.document().scene().parent(fill).unwrap();
            s.layer_edit(s.engine.document().select_occurrence_edit(if select_group { group } else { fill }).unwrap()).unwrap();
            select(&mut s, Some(rectangle([30., 40., 90., 80.])));
            let before = s.engine.document().clone();
            for command in [CommandId::Copy, CommandId::Cut] {
                assert!(s.command(command).enabled, "{command:?}: {:?}", s.command_disabled_reason(command));
                invoke(&mut s, command);
                let (id, _) = pending(&s);
                let mut capture = s.capture_clipboard(id).unwrap();
                let parts = capture.layer_captures();
                assert_eq!(parts.len(), 1);
                assert_eq!(parts[0].0, fill);
                assert!(matches!(&parts[0].1.scope, SceneScope::Members(ids) if ids.contains(&fill) && ids.contains(&group)));
                assert_eq!(capture.crop, [30, 40, 60, 40]);
                capture.set_layer_source(fill, rgba8_source([60, 40], |_, _| [128, 64, 32, 255])).unwrap();
                capture.finish_layer_sources().unwrap();
                let copied = capture.finish("fill-region".into(), rgba8_source([60, 40], |_, _| [0; 4]), vec![]).unwrap();
                let layers = copied.layers.as_ref().unwrap();
                assert!(layers.scene.artwork.effects.is_empty());
                assert_eq!(layers.scene.artwork.paint.len(), 1);
                Document::from_artwork(layers.scene.artwork.clone()).unwrap().validate(Default::default()).unwrap();
                s.complete_document_request(id, Ok(true)).unwrap();
                assert_live_artwork_eq(s.engine.document(), &before);
                if command == CommandId::Copy { continue; }
                assert!(s.conversion_busy());
                s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::Image(Some((rgba8_source([60, 40], |_, _| [128, 64, 32, 255]), [30, 40])))));
                s.frame(2, 2).unwrap();
                assert!(!s.conversion_busy());
                assert_eq!(s.engine.document().scene().occurrence(fill).unwrap().kind(), LayerKind::Paint);
                assert_eq!(s.engine.document().scene().parent(fill), Some(group));
                assert_eq!(s.engine.document().working.selection, before.working.selection);
                let operations = s.renderer_mut().pending_operations.clone();
                assert!(matches!(operations.as_slice(), [(_, layer_core::RasterOperation { kind: RasterOperationKind::Erase { alpha_locked: false }, .. })]));
                invoke(&mut s, CommandId::Undo);
                assert_live_artwork_eq(s.engine.document(), &before);
            }
        }
    }

    #[test]
    fn region_cut_of_image_and_paint_layers_restores_objects_on_undo() {
        let mut s = clip_session();
        let [_, first, second] = mixed_region_selection(&mut s);
        let before = s.engine.document().clone();
        invoke(&mut s, CommandId::Cut);
        let (id, _) = pending(&s);
        let capture = s.capture_clipboard(id).unwrap();
        for images in [first, second] { assert!(capture.layer_captures().iter().any(|(id, part)| *id == images && part.scope == SceneScope::RawObjects(images))); }
        s.complete_document_request(id, Ok(true)).unwrap();
        for (index, images) in [first, second].into_iter().enumerate() {
            assert!(s.conversion_busy());
            assert_live_artwork_eq(s.engine.document(), &before);
            let Some(layer_render::SnapshotRequest::Image(capture)) = s.engine.backend().snapshot_requests.last() else { panic!("an image layer capture"); };
            assert_eq!(capture.scope, SceneScope::RawObjects(images));
            s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::Image(Some((rgba8_source([40, 30], |_, _| [255; 4]), [0, 0])))));
            s.frame(2 + index as u64, 2 + index as u64).unwrap();
        }
        assert!(!s.conversion_busy());
        for images in [first, second] {
            assert!(s.engine.document().scene().object_layer(images).is_none());
            assert!(s.engine.document().scene().paint_source(images).is_some());
        }
        assert_eq!(s.engine.document().scene().order().len(), before.scene().order().len());
        let operations = s.renderer_mut().pending_operations.clone();
        assert_eq!(operations.len(), 3);
        assert!(operations.iter().all(|(_, op)| matches!(op.kind, RasterOperationKind::Erase { .. })), "image snapshots finish before live raster work");
        invoke(&mut s, CommandId::Undo);
        assert_live_artwork_eq(s.engine.document(), &before);
        assert_eq!(layer_core::WorkingState { generation: before.working.generation, ..s.engine.document().working.clone() }, before.working);
    }

    #[test]
    fn region_cut_never_publishes_partial_image_conversions() {
        for interruption in 0..3 {
            let mut s = clip_session();
            mixed_region_selection(&mut s);
            invoke(&mut s, CommandId::Cut);
            let (id, _) = pending(&s);
            s.capture_clipboard(id).unwrap();
            s.complete_document_request(id, Ok(true)).unwrap();
            s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::Image(Some((rgba8_source([40, 30], |_, _| [255; 4]), [0, 0])))));
            s.frame(2, 2).unwrap();
            assert!(s.conversion_busy());
            match interruption {
                0 => s.engine.backend_mut().snapshot_reply = Some(Err(layer_render::BackendError("capture failed"))),
                1 => {
                    let mut working = s.engine.document().working.clone();
                    working.selection = Some(rectangle([100., 120., 130., 140.]));
                    s.engine.apply_edit(Edit::Working(working)).unwrap();
                }
                _ => { assert!(s.cancel_conversion()); }
            }
            let before = s.engine.document().clone();
            let checkpoint = s.engine.checkpoint();
            s.frame(3, 3).unwrap();
            assert!(!s.conversion_busy());
            assert_live_artwork_eq(s.engine.document(), &before);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert!(s.renderer_mut().pending_operations.is_empty());
        }
    }

    #[test]
    fn group_region_cut_refuses_any_locked_or_alpha_locked_descendant() {
        for alpha in [false, true] {
            let mut s = clip_session();
            let layers = selected_pair(&mut s);
            s.layer_edit(s.engine.document().group_layers_edit(&layers, LayerBlend::Normal, "Group").unwrap()).unwrap();
            let group = s.engine.document().scene().parent(layers[0]).unwrap();
            let doc = s.engine.document();
            let mut occurrence = doc.scene().occurrence(layers[0]).unwrap().clone();
            occurrence.alpha_locked = alpha; occurrence.locked = !alpha;
            s.layer_edit(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, layers[0], Some(occurrence)).unwrap())).unwrap();
            s.layer_edit(s.engine.document().select_occurrence_edit(group).unwrap()).unwrap();
            select(&mut s, Some(rectangle([30., 40., 90., 80.])));
            assert!(s.command(CommandId::Copy).enabled);
            assert!(!s.command(CommandId::Cut).enabled);
            let before = s.engine.document().clone();
            assert!(s.dispatch(UiAction::Invoke { command: CommandId::Cut }).is_err());
            assert!(s.state.requests.is_empty());
            assert_live_artwork_eq(s.engine.document(), &before);
        }
    }

    #[test]
    fn pending_region_cuts_never_erase_a_changed_selection_or_layer_set() {
        for change_layers in [false, true] {
            let mut s = clip_session();
            let [first, second] = selected_pair(&mut s);
            select(&mut s, Some(rectangle([30., 40., 90., 80.])));
            invoke(&mut s, CommandId::Cut);
            let (id, _) = pending(&s);
            s.capture_clipboard(id).unwrap();
            let mut working = s.engine.document().working.clone();
            if change_layers { working.layer_selection = [second].into(); }
            else { working.selection = Some(rectangle([100., 120., 130., 140.])); }
            s.engine.apply_edit(Edit::Working(working)).unwrap();
            let before = s.engine.document().clone();
            s.complete_document_request(id, Ok(true)).unwrap();
            s.frame(2, 2).unwrap();
            assert_live_artwork_eq(s.engine.document(), &before);
            assert!(s.renderer_mut().pending_operations.is_empty());
            assert!(s.engine.document().scene().occurrence(first).is_some());
            assert!(s.state.notice.as_ref().is_some_and(|n| n.text.contains("copied but not erased")));
        }
    }

    #[test]
    fn full_size_external_paste_and_cursor_position_survive_delivery_delay() {
        let mut s = clip_session();
        let image = Arc::unwrap_or_clone(rgba8_source([800, 600], |_, _| [255; 4]));
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(vec![("Large".into(), image)], PasteMode::Paste, &context).unwrap();
        let doc = s.engine.document(); let object = doc.scene().object_layer(doc.working.occurrence.unwrap()).unwrap();
        assert_eq!(&object.affine.0[..4], &[1., 0., 0., 1.], "paste never shrinks to fit");
        invoke(&mut s, CommandId::CancelTransform);
        s.set_viewport([400., 300.], [800, 600]).unwrap();
        s.interaction.hover = Some([100., 125.]);
        let mut hover = event(&s, 2, PenPhase::Hover, 0.);
        hover.surface_position = Point { x: 200., y: 250. }; s.cursor_input(Some(hover));
        let expected = s.pointer64([200., 250.]);
        invoke(&mut s, CommandId::PasteAtCursor);
        hover.surface_position = Point { x: 600., y: 500. }; s.cursor_input(Some(hover));
        s.interaction.hover = Some([300., 250.]); s.state.camera.center_on([350., 250.]);
        let copied = clip([20, 10], [0; 2], s.engine.document().composition().color);
        let actual = s.clip_position(&copied, PasteMode::AtCursor);
        assert_eq!([actual.x, actual.y], [(expected[0] - 10.).round() as f32, (expected[1] - 5.).round() as f32]);
    }

    #[test]
    fn clipboard_gates_share_busy_locked_and_native_context_rules() {
        let mut s = clip_session();
        let mut working = s.engine.document().working.clone(); working.occurrence = None; working.target = None; working.layer_selection.clear();
        s.layer_edit(Edit::Working(working)).unwrap();
        assert!(!s.command(CommandId::Copy).enabled);
        assert!(s.command(CommandId::CopyMerged).enabled);
        s.state.settings_open = true;
        assert_eq!(s.native_paste_input().unwrap().regions, 0); assert!(s.state.requests.is_empty());
        s.state.settings_open = false; s.interaction.facts.popup_open = true;
        assert_eq!(s.native_paste_input().unwrap().regions, 0); assert!(s.state.requests.is_empty());
        s.interaction.facts.popup_open = false;
        assert_ne!(s.native_paste_input().unwrap().regions, 0);
        let (id, _) = pending(&s); s.complete_document_request(id, Ok(false)).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
        let doc = s.engine.document(); let group = doc.working.occurrence.unwrap(); let mut layer = doc.scene().occurrence(group).unwrap().clone(); layer.locked = true;
        s.layer_edit(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, group, Some(layer)).unwrap())).unwrap();
        assert!(!s.command(CommandId::PasteImage).enabled);
        assert!(s.command(CommandId::PasteAsNewImage).enabled);
    }

    #[test]
    fn paste_into_uses_the_active_group_and_restores_selection_on_undo() {
        let mut s = clip_session();
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
        let parent = s.engine.document().working.occurrence.unwrap();
        select(&mut s, Some(rectangle([50., 60., 150., 160.])));
        let copied = clip([20, 10], [0; 2], s.engine.document().composition().color);
        s.paste_clip(&copied, PasteMode::Into).unwrap();
        let doc = s.engine.document(); let pasted = doc.working.occurrence.unwrap();
        assert_eq!(doc.scene().parent(pasted), Some(parent));
        assert!(doc.scene().mask(pasted).is_some()); assert!(doc.working.selection.is_none());
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().working.selection.is_some());
        assert!(s.engine.document().scene().occurrence(pasted).is_none());
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
        let destination = s.engine.document().working.occurrence.unwrap();
        let edit = s.engine.document().select_occurrence_edit(parent).unwrap(); s.layer_edit(edit).unwrap();
        let context = s.image_placement_context(None, Some(ImageLayerDestination { target: destination, position: LayerDropPosition::Into })).unwrap();
        let source = Arc::unwrap_or_clone(rgba8_source([20, 10], |_, _| [255; 4]));
        s.paste_layer_sources(vec![("Explicit destination".into(), source)], PasteMode::Into, &context).unwrap();
        let doc = s.engine.document();
        assert_eq!(doc.scene().parent(doc.working.occurrence.unwrap()), Some(destination));
    }

    #[test]
    fn cut_erases_only_when_the_drawing_is_unchanged() {
        let mut s = clip_session();
        let paint = s.engine.document().working.occurrence.unwrap();
        select(&mut s, Some(rectangle([10., 10., 60., 60.])));
        invoke(&mut s, CommandId::Cut);
        let (id, request) = pending(&s);
        assert!(matches!(request, DocumentRequest::Copy { merged: false, cut: true, pixels: false }));
        s.capture_clipboard(id).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        s.frame(2, 2).unwrap();
        let erased = s.renderer_mut().pending_operations.clone();
        assert!(matches!(erased.as_slice(), [(layer, op)] if Some(*layer) == s.engine.document().scene().source_target(paint) && matches!(op.kind, RasterOperationKind::Erase { .. })));
        assert!(s.engine.document().working.selection.is_some(), "the selection stays");

        s.renderer_mut().pending_operations.clear();
        invoke(&mut s, CommandId::Cut);
        let (id, _) = pending(&s);
        s.capture_clipboard(id).unwrap();
        let mut occurrence = s.engine.document().scene().occurrence(paint).unwrap().clone(); occurrence.opacity = 0.5;
        s.layer_edit(Edit::Occurrence(RecordChange::replace(&s.engine.document().artwork.occurrences, paint, Some(occurrence)).unwrap())).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        s.frame(3, 3).unwrap();
        assert!(s.renderer_mut().pending_operations.is_empty(), "the drawing changed while cutting");
        assert!(s.state.notice.as_ref().is_some_and(|n| n.text.contains("copied but not erased")));

        invoke(&mut s, CommandId::Cut);
        let (id, _) = pending(&s);
        s.capture_clipboard(id).unwrap();
        s.complete_document_request(id, Ok(false)).unwrap();
        s.frame(4, 4).unwrap();
        assert!(s.renderer_mut().pending_operations.is_empty(), "a cancelled cut erases nothing");
    }

    #[test]
    fn mask_copy_captures_raw_coverage_and_cut_keeps_the_mask_record() {
        let mut s = clip_session();
        let target = focused_mask(&mut s);
        let owner = s.engine.document().working.occurrence.unwrap();
        let doc = s.engine.document();
        let mut occurrence = doc.scene().occurrence(owner).unwrap().clone();
        occurrence.offset = [20, 30];
        let mask = occurrence.mask.as_mut().unwrap();
        mask.offset = [7, -2]; mask.linked = false; mask.inverted = true; mask.enabled = false;
        s.layer_edit(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, owner, Some(occurrence)).unwrap())).unwrap();
        let selection = rectangle([10., 20., 30., 40.]);
        select(&mut s, Some(selection.clone()));
        for command in [CommandId::Copy, CommandId::CopyPixels, CommandId::Cut, CommandId::PasteImage] {
            assert!(s.command(command).enabled, "{command:?}: {:?}", s.command_disabled_reason(command));
        }
        invoke(&mut s, CommandId::Copy);
        let (id, _) = pending(&s);
        let capture = s.capture_clipboard(id).unwrap();
        assert_eq!(capture.scope, SceneScope::Raw(target));
        assert_eq!(capture.crop, [10, 20, 20, 20]);
        assert_eq!(capture.coverage.as_deref(), Some(&selection));
        assert!(capture.layers.is_none() && capture.original.is_none());
        assert_eq!(capture.scene.view().target_offset(target), [7, -2]);
        s.complete_document_request(id, Ok(true)).unwrap();
        let before = s.engine.document().clone();
        for published in [false, true] {
            invoke(&mut s, CommandId::Cut);
            let (id, _) = pending(&s);
            s.capture_clipboard(id).unwrap();
            s.complete_document_request(id, Ok(published)).unwrap();
            s.frame(2, 2).unwrap();
            if !published {
                assert_live_artwork_eq(s.engine.document(), &before);
                assert!(s.renderer_mut().pending_operations.is_empty());
                continue;
            }
            assert_eq!(s.engine.document().scene().occurrence(owner), before.scene().occurrence(owner));
            assert_eq!(s.engine.document().working.target, Some(target));
            let operations = s.renderer_mut().pending_operations.clone();
            assert_eq!(operations.len(), 1);
            assert_eq!(operations[0].0, target);
            assert!(matches!(operations[0].1.kind, RasterOperationKind::Erase { alpha_locked: false }));
            assert_eq!(operations[0].1.coverage.selection.as_ref(), Some(&selection.translated(Point { x: -7., y: 2. })));
            invoke(&mut s, CommandId::Undo);
            assert_live_artwork_eq(s.engine.document(), &before);
        }
    }

    #[test]
    fn mask_paste_preserves_focus_selection_offsets_and_layer_count_with_one_undo() {
        for linked in [false, true] {
            let mut s = clip_session();
            let target = focused_mask(&mut s);
            let owner = s.engine.document().working.occurrence.unwrap();
            let doc = s.engine.document();
            let mut occurrence = doc.scene().occurrence(owner).unwrap().clone();
            occurrence.offset = [20, 30]; occurrence.alpha_locked = true;
            let mask = occurrence.mask.as_mut().unwrap();
            mask.offset = [7, -2]; mask.linked = linked; mask.inverted = true; mask.enabled = false;
            s.layer_edit(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, owner, Some(occurrence)).unwrap())).unwrap();
            let selection = rectangle([110., 55., 115., 58.]);
            select(&mut s, Some(selection.clone()));
            let before = s.engine.document().clone();
            let mut copied = clip([20, 10], [100, 50], before.composition().color);
            if linked { copied.layers = Some(Arc::new(crate::session::clipboard::LayerClip { scene: before.snapshot(), roots: vec![owner] })); }
            assert!(s.command(CommandId::Cut).enabled, "alpha lock applies to paint, not mask coverage");
            s.paste_clip(&copied, PasteMode::Paste).unwrap();
            s.frame(2, 2).unwrap();
            let doc = s.engine.document();
            assert_eq!(doc.scene().order(), before.scene().order());
            assert_eq!(doc.working, before.working);
            assert_eq!(doc.scene().occurrence(owner), before.scene().occurrence(owner));
            assert!(!s.operation.active(), "mask paste does not enter image placement");
            let origin = before.target_offset(target);
            let operations = s.renderer_mut().pending_operations.clone();
            assert_eq!(operations.len(), 1);
            assert_eq!(operations[0].0, target);
            let operation = &operations[0].1;
            assert_eq!(operation.coverage.selection.as_ref(), Some(&selection.translated(Point { x: -origin[0] as f32, y: -origin[1] as f32 })));
            let RasterOperationKind::Bake { scene, scope, offset } = &operation.kind else { panic!("mask paste needs an image operation"); };
            assert_eq!(*scope, SceneScope::All);
            assert_eq!(*offset, Point { x: 100. - origin[0] as f32, y: 50. - origin[1] as f32 });
            assert_eq!(scene.view().composition().size, [20, 10]);
            s.engine.document().validate(Default::default()).unwrap();
            invoke(&mut s, CommandId::Undo);
            assert_live_artwork_eq(s.engine.document(), &before);
            assert_eq!(s.engine.document().working, before.working);
        }
    }

    #[test]
    fn external_mask_paste_centres_in_selection_and_rejects_changed_targets() {
        let mut s = clip_session();
        let target = focused_mask(&mut s);
        select(&mut s, Some(rectangle([100., 60., 160., 100.])));
        let before = s.engine.document().clone();
        let sources = || vec![("External".into(), Arc::unwrap_or_clone(rgba8_source([20, 10], |_, _| [60, 120, 180, 128])))];
        let context = s.image_placement_context(None, None).unwrap();
        s.state.camera.center_on([300., 250.]);
        s.paste_layer_sources(sources(), PasteMode::Paste, &context).unwrap();
        s.frame(2, 2).unwrap();
        assert_eq!(s.engine.document().scene().order(), before.scene().order());
        assert_eq!(s.engine.document().working, before.working);
        assert!(!s.operation.active());
        let operations = s.renderer_mut().pending_operations.clone();
        assert!(matches!(operations.as_slice(), [(actual, layer_core::RasterOperation { kind: RasterOperationKind::Bake { offset: Point { x: 120., y: 75. }, .. }, .. })] if *actual == target));
        invoke(&mut s, CommandId::Undo);
        assert_live_artwork_eq(s.engine.document(), &before);
        let context = s.image_placement_context(None, None).unwrap();
        invoke(&mut s, CommandId::EditLayerContent);
        let content = s.engine.document().clone();
        assert!(s.paste_layer_sources(sources(), PasteMode::Paste, &context).is_err());
        assert_live_artwork_eq(s.engine.document(), &content);
        let context = s.image_placement_context(None, None).unwrap();
        invoke(&mut s, CommandId::EditLayerMask);
        assert!(s.paste_layer_sources(sources(), PasteMode::Paste, &context).is_err());
        assert_live_artwork_eq(s.engine.document(), &content);
    }

    #[test]
    fn mask_clipboard_edits_reopen_with_exact_tiles_and_session_history() {
        use layer_core::package::{ImmutableBacking, session::{PreparedSession, open}};
        use std::sync::atomic::AtomicBool;
        let signature = |document: &Document| {
            let (_, _, source) = document.artwork.coverage.iter().next().unwrap();
            assert!(source.operations.is_empty());
            let pixels = source.raster.wait_data().unwrap().tiles.iter().map(|(key, tile)|
                (*key, tile.wait_backing().unwrap().decode().unwrap())).collect::<Vec<_>>();
            (source.domain, source.default_coverage, pixels)
        };
        let mut s = clip_session();
        let target = focused_mask(&mut s);
        let owner = s.engine.document().working.occurrence.unwrap();
        let doc = s.engine.document();
        let mut occurrence = doc.scene().occurrence(owner).unwrap().clone();
        let mask = occurrence.mask.as_mut().unwrap();
        mask.offset = [7, -2]; mask.linked = false; mask.inverted = true; mask.enabled = false;
        s.layer_edit(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, owner, Some(occurrence)).unwrap())).unwrap();
        select(&mut s, Some(rectangle([10., 20., 30., 40.])));
        let before = signature(s.engine.document());
        let before_checkpoint = s.engine.checkpoint();
        let copied = clip([20, 20], [10, 20], s.engine.document().composition().color);
        s.paste_clip(&copied, PasteMode::Paste).unwrap();
        assert!(s.engine.capture_artwork(0).is_err());
        assert!(s.capture_session().is_err());
        s.frame(2, 2).unwrap();
        let pasted = signature(s.engine.document());
        let pasted_checkpoint = s.engine.checkpoint();
        invoke(&mut s, CommandId::Cut);
        let (id, _) = pending(&s); s.capture_clipboard(id).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        assert!(s.engine.capture_artwork(0).is_err());
        assert!(s.capture_session().is_err());
        s.frame(3, 3).unwrap();
        let cut = signature(s.engine.document());
        let cut_checkpoint = s.engine.checkpoint();
        let capture = s.engine.capture_artwork(0).unwrap();
        let reopened = reopen_capture(&capture);
        assert_eq!(signature(&reopened), cut);
        let mask = reopened.artwork.occurrences.iter().find_map(|(_, _, layer)| layer.mask.as_ref()).unwrap();
        assert_eq!((mask.offset, mask.linked, mask.inverted, mask.enabled), ([7, -2], false, true, false));
        let cancel = AtomicBool::new(false);
        let capture = s.engine.capture_session(0).unwrap();
        let prepared = PreparedSession::prepare(&capture, serde_json::json!({}), &cancel).unwrap();
        let mut bytes = Vec::new(); prepared.write(&mut bytes, &cancel).unwrap();
        let backing = ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
        let mut restored = open(backing, Default::default(), &cancel).unwrap().editor;
        assert_eq!(restored.document().working, s.engine.document().working);
        assert_eq!(restored.document().working.target, Some(target));
        assert_eq!(signature(restored.document()), cut);
        assert_eq!(restored.checkpoint(), cut_checkpoint);
        for (expected, checkpoint) in [(&pasted, pasted_checkpoint), (&before, before_checkpoint)] {
            assert!(restored.undo().unwrap());
            assert_eq!(&signature(restored.document()), expected);
            assert_eq!(restored.checkpoint(), checkpoint);
        }
        for (expected, checkpoint) in [(&pasted, pasted_checkpoint), (&cut, cut_checkpoint)] {
            assert!(restored.redo().unwrap());
            assert_eq!(&signature(restored.document()), expected);
            assert_eq!(restored.checkpoint(), checkpoint);
        }
    }

    #[test]
    fn locked_masks_allow_copy_but_refuse_cut_and_paste_without_changing_artwork() {
        let mut s = clip_session();
        focused_mask(&mut s);
        let owner = s.engine.document().working.occurrence.unwrap();
        let doc = s.engine.document();
        let mut occurrence = doc.scene().occurrence(owner).unwrap().clone(); occurrence.locked = true;
        s.layer_edit(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, owner, Some(occurrence)).unwrap())).unwrap();
        let before = s.engine.document().clone();
        let copied = clip([20, 10], [0; 2], before.composition().color);
        for command in [CommandId::Copy, CommandId::CopyPixels, CommandId::CopyMerged] { assert!(s.command(command).enabled, "{command:?}"); }
        for command in [CommandId::Cut, CommandId::PasteImage, CommandId::PasteInPlace, CommandId::PasteAtView] {
            assert!(!s.command(command).enabled, "{command:?}");
            assert!(s.dispatch(UiAction::Invoke { command }).is_err());
        }
        assert!(s.paste_clip(&copied, PasteMode::Paste).is_err());
        assert!(s.state.requests.is_empty());
        assert_live_artwork_eq(s.engine.document(), &before);
        assert!(s.command(CommandId::PasteAsNewImage).enabled);
    }

    #[test]
    fn clipboard_stays_unavailable_while_editing_quick_or_saved_selections() {
        for saved in [false, true] {
            let mut s = clip_session();
            invoke(&mut s, if saved { CommandId::NewSelectionLayer } else { CommandId::QuickMask });
            let before = s.engine.document().clone();
            for command in [CommandId::Copy, CommandId::Cut, CommandId::CopyMerged, CommandId::PasteImage, CommandId::PasteInPlace] {
                assert!(!s.command(command).enabled, "{command:?}, saved={saved}");
                assert!(s.dispatch(UiAction::Invoke { command }).is_err());
            }
            let copied = clip([20, 10], [0; 2], before.composition().color);
            assert!(s.paste_clip(&copied, PasteMode::Paste).is_err());
            assert!(s.state.requests.is_empty());
            assert_live_artwork_eq(s.engine.document(), &before);
        }
    }

    #[test]
    fn insert_clipboard_keys_use_the_same_requests_and_respect_text_focus() {
        for preset in crate::keymaps::KEYMAP_PRESETS {
            for (name, command, shift, cut) in [
                ("Insert", true, false, Some(false)),
                ("Delete", false, true, Some(true)),
                ("Insert", false, true, None),
            ] {
                let mut s = clip_session();
                let mut settings = Settings::default();
                crate::keymaps::select(&mut settings, preset.id).unwrap();
                s.dispatch(UiAction::RestoreSettings { settings }).unwrap();
                let before = s.engine.document().clone();
                let input = |pressed, editing| UiInput::Key { key: name.into(), pressed, repeat: false,
                    modifiers: Modifiers { command, shift, alt: false }, editing, divider: None };
                assert!(!s.input(input(true, true)).unwrap().handled, "{}: text owns {name}", preset.id);
                s.input(input(false, true)).unwrap();
                assert!(s.state.requests.is_empty());
                assert!(s.input(input(true, false)).unwrap().handled, "{}: canvas handles {name}", preset.id);
                s.input(input(false, false)).unwrap();
                let request = pending(&s).1;
                assert!(match cut {
                    Some(cut) => matches!(request, DocumentRequest::Copy { merged: false, cut: actual, pixels: false } if actual == cut),
                    None => matches!(request, DocumentRequest::Paste { mode: PasteMode::Paste }),
                }, "{}: {name}: {request:?}", preset.id);
                assert_live_artwork_eq(s.engine.document(), &before);
            }
        }
    }

    #[test]
    fn clipboard_chords_and_menus() {
        let mut s = clip_session();
        for (command, chord) in [
            (CommandId::Copy, "Ctrl+C / Ctrl+Insert"),
            (CommandId::Cut, "Ctrl+X / Shift+Delete"),
            (CommandId::CopyMerged, "Ctrl+Shift+C"),
            (CommandId::PasteImage, "Ctrl+V / Shift+Insert"),
            (CommandId::PasteAtView, "Ctrl+Shift+V"),
            (CommandId::PasteAtCursor, "Ctrl+Alt+V"),
            (CommandId::PasteInto, "Ctrl+Alt+Shift+V"),
        ] {
            assert_eq!(s.command(command).shortcut, chord, "{command:?}");
        }
        assert_eq!(CommandId::PasteImage.label().as_ref(), "Paste");
        assert_eq!(serde_json::to_value(CommandId::PasteImage).unwrap(), "paste_image", "the persisted id is kept");
        assert!(KeyChord::new("c", Modifiers { command: true, shift: true, alt: false }).available(Platform::Web));
        let edit = s.application_menu(ApplicationMenu::Edit);
        let labels: Vec<_> = edit.sections[1].iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["Cut", "Copy", "Copy Merged", "Copy Pixels", "Paste", "Paste Special"]);
        let special = &edit.sections[1].last().unwrap().sections;
        let labels: Vec<_> = special.iter().flatten().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["Paste in Place", "Paste to Shown Position", "Paste at Cursor", "Paste Into", "Paste as New Image"]);

        assert!(!key(&mut s, "c", true, true, true).handled, "a focused text field keeps Ctrl+C");
        assert!(s.state.requests.is_empty());
        key(&mut s, "c", false, true, true);
        assert!(!key(&mut s, "v", true, true, true).handled, "a focused text field keeps Ctrl+V");
        assert!(s.state.requests.is_empty());
        key(&mut s, "v", false, true, true);
        assert!(key(&mut s, "c", true, true, false).handled);
        assert!(matches!(pending(&s).1, DocumentRequest::Copy { merged: false, cut: false, pixels: false }));

        for platform in [Platform::Mac, Platform::Windows] {
            let mut host = session(platform);
            let edit = host.application_menu(ApplicationMenu::Edit);
            assert!(edit.sections.iter().flatten().any(|item| item.label == "Paste"));
            assert!(edit.sections.iter().flatten().any(|item| item.label == "Copy"), "{platform:?}");
            host.frame(1, 1).unwrap();
        }
    }

    #[test]
    fn preset_paste_chords() {
        for (preset, id, chord) in [
            ("photoshop", "command.PasteInto", ("v", true, true)),
            ("photoshop", "command.PasteInPlace", ("v", false, true)),
            ("krita", "command.PasteAtCursor", ("v", true, false)),
            ("gimp", "command.PasteInPlace", ("v", true, false)),
            ("gimp", "command.PasteAsNewImage", ("v", false, true)),
            ("krita", "command.PasteAsNewImage", ("n", false, true)),
        ] {
            let keys = crate::keymaps::preset(preset).unwrap().keys_for(id).unwrap();
            assert_eq!(keys.len(), 1);
            assert_eq!((keys[0].key.as_str(), keys[0].command, keys[0].alt, keys[0].shift), (chord.0, true, chord.1, chord.2), "{preset}");
        }
    }

    #[test]
    fn new_image_retains_depth_samples_transparency_and_object_geometry() {
        use layer_core::color::{ColorProfile, source::{SourceBuilder, SourceChannels, SourceInterpretation}};
        let localization = Localizer::shared(UiLanguage::English);
        let color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
        let mut builder = SourceBuilder::new([2, 1], SourceInterpretation { channels: SourceChannels::Rgba,
            depth: color.depth, profile: ColorProfile::Builtin(color.space), profile_assumed: false }, 1024 * 1024).unwrap();
        builder.push_row(&[1, 0, 2, 0, 3, 0, 0, 0, 4, 0, 5, 0, 6, 0, 255, 255]).unwrap();
        let mut copied = clip([2, 1], [-10, 23], color);
        copied.source = Arc::new(builder.finish().unwrap());
        let document = copied.document(&localization).unwrap();
        assert_eq!(document.composition().size, [2, 1]);
        assert_eq!(document.composition().color, color);
        assert_eq!(document.composition().blend, copied.blend);
        let base = document.scene().paint_source(document.working.occurrence.unwrap()).unwrap().base.as_ref().unwrap();
        assert!(Arc::ptr_eq(base.image.storage(), &copied.source));
        assert_eq!(base.policy, copied.policy);
        assert_eq!(base.offset, [0, 0]);
        assert!(!document.scene().occurrence(document.scene().order()[1]).unwrap().visible);
        let mut object = layer_core::ImageObject::new(copied.source.clone().into());
        object.affine = layer_core::Affine64([1., 0., 0., 1., -10., 23.]);
        retain_image_layers(&mut copied, vec![("Original".into(), object.clone())]);
        let objects = copied.document(&localization).unwrap();
        let (handle, _, kept) = objects.artwork.objects.iter().next().unwrap();
        assert_eq!(objects.object_document_affine(handle).unwrap(), layer_core::Affine64([1., 0., 0., 1., 0., 0.]));
        assert_eq!(kept.image.extent, object.image.extent);
        assert_eq!(kept.image.interpretation, object.image.interpretation);
        assert_eq!(kept.image.tiles.values().next().unwrap().owner_identity(), object.image.tiles.values().next().unwrap().owner_identity());
        assert_eq!(objects.selected_objects().len(), 1);
        assert_eq!(objects.scene().order().len(), 2);
        assert!(objects.artwork.paint.is_empty());
        objects.validate(Default::default()).unwrap();
    }

    #[test]
    fn new_image_includes_every_external_image_without_scaling() {
        let sources = vec![("Wide".into(), Arc::unwrap_or_clone(rgba8_source([9, 3], |_, _| [9; 4]))),
            ("Tall".into(), Arc::unwrap_or_clone(rgba8_source([2, 11], |_, _| [7; 4])))];
        let document = clipboard_document(sources, Default::default(), &Localizer::shared(UiLanguage::English)).unwrap();
        assert_eq!(document.composition().size, [9, 11]);
        let objects: Vec<_> = document.scene().order().iter().filter_map(|&id| {
            let object = document.scene().object_layer(id)?;
            Some((document.scene().occurrence(id).unwrap().name.as_ref(), object.affine.0))
        }).collect();
        assert_eq!(objects, [("Wide", [1., 0., 0., 1., 0., 4.]), ("Tall", [1., 0., 0., 1., 3., 0.])]);
        assert!(clipboard_document(Vec::new(), Default::default(), &Localizer::shared(UiLanguage::English)).is_err());
    }

    #[test]
    fn paste_sizes_only_the_untouched_startup_drawing_automatically() {
        let mut startup = clip_session();
        startup.mark_startup_drawing();
        invoke(&mut startup, CommandId::PasteImage);
        assert!(matches!(pending(&startup).1, DocumentRequest::Paste { mode: PasteMode::NewImage }));
        let mut working = clip_session();
        working.mark_startup_drawing();
        invoke(&mut working, CommandId::AddLayer);
        invoke(&mut working, CommandId::PasteImage);
        assert!(matches!(pending(&working).1, DocumentRequest::Paste { mode: PasteMode::Paste }));
        let mut explicit = clip_session();
        invoke(&mut explicit, CommandId::PasteImage);
        assert!(matches!(pending(&explicit).1, DocumentRequest::Paste { mode: PasteMode::Paste }));
        for platform in Platform::ALL { assert!(CommandId::PasteAsNewImage.available_on(platform)); }
    }

    #[test]
    fn new_image_does_not_need_an_editable_destination() {
        let mut s = clip_session();
        let target = s.engine.document().working.occurrence.unwrap();
        let mut layer = s.engine.document().scene().occurrence(target).unwrap().clone();
        layer.locked = true;
        s.layer_edit(Edit::Occurrence(RecordChange::replace(&s.engine.document().artwork.occurrences, target, Some(layer)).unwrap())).unwrap();
        let before = s.engine.document().clone();
        invoke(&mut s, CommandId::PasteAsNewImage);
        let context = s.image_placement_context(None, None).unwrap();
        s.validate_image_placement(&context).unwrap();
        assert_eq!(*s.engine.document(), before);
    }

    #[test]
    fn the_selection_bar_offers_copy_on_every_host() {
        for platform in Platform::ALL {
            let mut s = UiSession::new(Recorder::default(), Document::new(PortableId::random(), 400, 300, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }), [800, 600], platform).unwrap();
            invoke(&mut s, CommandId::SelectAll);
            invoke(&mut s, CommandId::Move);
            let offered = s.state.canvas_bar.unwrap().items.iter().any(|i| i.menu == Some(CanvasBarMenu::Copy));
            assert!(offered, "{platform:?}");
        }
    }
}
