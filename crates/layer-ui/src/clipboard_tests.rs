mod clipboard_checks {
    use super::*;
    use layer_core::{
        Edit, Layer, LayerBlend, LayerOperationKind,
        color::{
            DocumentColor, RgbSpace, SampleDepth,
            source::{SourceKind, rgba8_source},
        },
    };
    use std::sync::Arc;

    fn clip_session() -> UiSession<Recorder> {
        let mut s = UiSession::new(
            Recorder { tiled_sources: true, ..Default::default() },
            Document::new("clipboard", 400, 300),
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

    fn clip(extent: [u32; 2], origin: [u32; 2], color: DocumentColor) -> PixelClip {
        let mut source = Arc::unwrap_or_clone(rgba8_source(extent, |_, _| [90, 120, 200, 255]));
        source.kind = SourceKind::Rasterized;
        PixelClip {
            nonce: "clip".into(),
            name: "Ink".into(),
            source: Arc::new(source),
            origin,
            color,
            png: Arc::from(&b"png"[..]),
        }
    }

    fn translation(s: &UiSession<Recorder>) -> [f32; 2] {
        let doc = s.engine.document();
        let placement = doc.layer(doc.active_layer).unwrap().properties.placement;
        assert_eq!(placement.0[..4], [1., 0., 0., 1.], "pasted at full size");
        [placement.0[4], placement.0[5]]
    }

    #[test]
    fn copy_captures_the_active_layer_alone_before_its_properties() {
        let mut s = clip_session();
        let paint = s.engine.document().active_layer;
        invoke(&mut s, CommandId::AddLayer);
        let group = s.engine.allocate_layer_id();
        s.layer_edit(Edit::InsertLayer { index: 0, layer: Layer { kind: LayerKind::Group, ..Layer::paint(group, "Group") } }).unwrap();
        let mut layer = s.engine.document().layer(paint).unwrap().clone();
        layer.properties.parent = Some(group);
        layer.properties.offset = Point { x: 7., y: 3. };
        layer.properties.blend = LayerBlend::Multiply;
        layer.properties.clipped = false;
        layer.opacity = 0.4;
        layer.visible = false;
        s.layer_edit(Edit::ReplaceLayer(Box::new(layer))).unwrap();
        let mut moved = s.engine.document().layer(group).unwrap().clone();
        moved.properties.offset = Point { x: 10., y: 20. };
        s.layer_edit(Edit::ReplaceLayer(Box::new(moved))).unwrap();
        s.layer_edit(Edit::SetActiveLayer { id: paint }).unwrap();
        select(&mut s, Some(rectangle([30.2, 40., 130., 90.5])));
        assert!(s.command(CommandId::Copy).enabled, "{:?}", s.command_disabled_reason(CommandId::Copy));
        invoke(&mut s, CommandId::Copy);
        let (id, request) = pending(&s);
        assert!(matches!(request, DocumentRequest::Copy { merged: false, cut: false }));
        assert_eq!(s.command_disabled_reason(CommandId::Copy).as_deref(), Some("Wait for the current file operation"));
        let capture = s.capture_clipboard(id).unwrap();
        assert_eq!(capture.crop, [30, 40, 100, 51]);
        assert!(capture.coverage.is_some());
        assert!(capture.original.is_none());
        assert_eq!((capture.background, capture.name.as_str(), capture.large), ([0.; 4], "Current ink", false));
        let document = &capture.project.document;
        assert_eq!(document.layers.len(), 1);
        let copied = &document.layers[0];
        assert_eq!(copied.id, paint);
        assert!(copied.visible && copied.opacity == 1. && copied.mask.is_none());
        assert_eq!(copied.properties.parent, None);
        assert_eq!(copied.properties.offset, Point { x: 17., y: 23. }, "the group's offset is kept");
        assert_eq!(copied.properties.blend, LayerBlend::Normal);
        assert!(document.selection.is_none());
        s.complete_document_request(id, Ok(true)).unwrap();
        assert!(s.command(CommandId::Copy).enabled);

        invoke(&mut s, CommandId::CopyMerged);
        let (id, request) = pending(&s);
        assert!(matches!(request, DocumentRequest::Copy { merged: true, cut: false }));
        let merged = s.capture_clipboard(id).unwrap();
        assert_eq!(merged.project.document.layers.len(), s.engine.document().layers.len());
        assert_eq!(merged.background, s.engine.view().background_rgba_linear);
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
        assert_eq!(*original, *s.engine.document().layer(s.engine.document().active_layer).unwrap().source.clone().unwrap());
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
        assert_eq!(reason(&s, CommandId::Cut).as_deref(), Some("Make a selection first"));
        assert_eq!(reason(&s, CommandId::PasteInto).as_deref(), Some("Make a selection to paste into"));
        assert!(s.command(CommandId::Copy).enabled && s.command(CommandId::CopyMerged).enabled);
        assert!(s.command(CommandId::PasteInPlace).enabled && s.command(CommandId::PasteImage).enabled);
        select(&mut s, Some(rectangle([500., 400., 600., 500.])));
        assert_eq!(reason(&s, CommandId::Copy).as_deref(), Some("The selection doesn't cover any of the canvas"));
        assert!(s.dispatch(UiAction::Invoke { command: CommandId::Copy }).is_err());
        assert!(s.state.requests.is_empty());
        select(&mut s, Some(rectangle([10., 10., 60., 60.])));
        assert!(s.command(CommandId::Cut).enabled && s.command(CommandId::PasteInto).enabled);
        let paper = s.engine.document().layers.iter().find(|l| l.kind == LayerKind::Background).unwrap().id;
        s.layer_edit(Edit::SetActiveLayer { id: paper }).unwrap();
        assert_eq!(reason(&s, CommandId::Copy).as_deref(), Some("The paper has no pixels to copy"));
        assert!(s.command(CommandId::CopyMerged).enabled, "Copy Merged ignores the active layer");
        let paint = s.engine.document().layers[0].id;
        s.layer_edit(Edit::SetActiveLayer { id: paint }).unwrap();
        let mut layer = s.engine.document().layer(paint).unwrap().clone();
        layer.properties.alpha_locked = true;
        s.layer_edit(Edit::ReplaceLayer(Box::new(layer))).unwrap();
        assert!(s.command(CommandId::Copy).enabled);
        assert_eq!(reason(&s, CommandId::Cut).as_deref(), Some("Alpha lock keeps transparency; unlock the layer first"));
        invoke(&mut s, CommandId::QuickMask);
        for command in [CommandId::Copy, CommandId::Cut, CommandId::CopyMerged] {
            assert_eq!(reason(&s, command).as_deref(), Some("Quick Mask edits the selection; leave it to copy artwork"));
        }
        for platform in [Platform::Mac, Platform::Ios, Platform::Windows] {
            for command in [CommandId::Copy, CommandId::Cut, CommandId::CopyMerged, CommandId::PasteInPlace, CommandId::PasteInto] {
                assert!(!command.available_on(platform), "{command:?} on {platform:?}");
            }
            assert!(CommandId::PasteImage.available_on(platform));
        }
    }

    #[test]
    fn a_clip_is_document_pixels_only_where_the_colour_settings_match() {
        let color = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 };
        let clip = clip([4, 4], [0, 0], color);
        assert_eq!(clip.source_for(color).kind, SourceKind::Rasterized);
        for other in [
            DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 },
            DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U8 },
        ] {
            let source = clip.source_for(other);
            assert_eq!(source.kind, SourceKind::Original);
            assert_eq!(source.interpretation, clip.source.interpretation, "an explicit profile");
        }
        let mut original = clip.clone();
        let mut photo = (*original.source).clone();
        photo.kind = SourceKind::Original;
        original.source = Arc::new(photo);
        assert_eq!(original.source_for(color).kind, SourceKind::Original, "a copied photo keeps its original");
    }

    #[test]
    fn paste_keeps_a_visible_position_and_otherwise_centres_in_one_step() {
        let mut s = clip_session();
        let before = s.engine.document().layers.len();
        let visible = clip([20, 10], [100, 50], s.engine.document().color);
        s.paste_clip(&visible, PasteMode::Paste).unwrap();
        assert_eq!(s.engine.document().layers.len(), before + 1);
        assert_eq!(translation(&s), [100., 50.]);
        assert!(!s.operation.placing(), "no handles");
        let doc = s.engine.document();
        let layer = doc.layer(doc.active_layer).unwrap();
        assert_eq!(layer.name.as_ref(), "Ink");
        assert_eq!(layer.source.as_ref().unwrap().kind, SourceKind::Rasterized);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().layers.len(), before, "one undo step");

        s.state.camera.zoom = 8.;
        s.state.camera.center_on([30., 30.]);
        let hidden = clip([20, 10], [300, 250], s.engine.document().color);
        s.paste_clip(&hidden, PasteMode::Paste).unwrap();
        assert_eq!(translation(&s), [20., 25.], "centred in the view");
        invoke(&mut s, CommandId::Undo);
        s.paste_clip(&hidden, PasteMode::InPlace).unwrap();
        assert_eq!(translation(&s), [300., 250.], "Paste in Place keeps the position");
    }

    #[test]
    fn paste_into_masks_the_new_layer_with_the_selection_in_one_step() {
        let mut s = clip_session();
        let selection = rectangle([110., 55., 115., 58.]);
        select(&mut s, Some(selection.clone()));
        let before = s.engine.document().clone();
        let clip = clip([20, 10], [100, 50], DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U8 });
        s.paste_clip(&clip, PasteMode::Into).unwrap();
        let doc = s.engine.document();
        let layer = doc.layer(doc.active_layer).unwrap();
        assert_eq!(layer.source.as_ref().unwrap().kind, SourceKind::Original, "another colour mode");
        let mask = layer.mask.as_ref().expect("a mask from the selection");
        assert_eq!(mask.initial.as_ref().unwrap().shape, selection.shape);
        assert_eq!(mask.default_coverage, 0.);
        assert!(doc.selection.is_none(), "the selection became the mask");
        assert_eq!(translation(&s), [100., 50.]);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().layers, before.layers);
        assert_eq!(s.engine.document().selection, before.selection, "one undo step restores the selection");
        select(&mut s, None);
        assert!(s.paste_clip(&clip, PasteMode::Into).is_err());
    }

    #[test]
    fn external_paste_in_place_centres_at_full_size_without_handles() {
        let mut s = clip_session();
        let image = || vec![("Clipboard image".to_string(), Arc::unwrap_or_clone(rgba8_source([40, 20], |_, _| [1; 4])))];
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(image(), PasteMode::InPlace, &context).unwrap();
        assert!(!s.operation.placing());
        let [cx, cy] = s.state.camera.work_area_center();
        let centre = s.state.camera.input_transform().map(Point { x: cx, y: cy });
        assert_eq!(translation(&s), [(centre.x - 20.).round(), (centre.y - 10.).round()]);
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(image(), PasteMode::Paste, &context).unwrap();
        assert!(s.operation.placing(), "an image from another app opens the placement handles");
        invoke(&mut s, CommandId::CancelTransform);
        let context = s.image_placement_context(None, None).unwrap();
        invoke(&mut s, CommandId::AddLayer);
        assert!(s.paste_layer_sources(image(), PasteMode::InPlace, &context).is_err(), "a stale context is refused");
    }

    #[test]
    fn cut_erases_only_when_the_drawing_is_unchanged() {
        let mut s = clip_session();
        let paint = s.engine.document().active_layer;
        select(&mut s, Some(rectangle([10., 10., 60., 60.])));
        invoke(&mut s, CommandId::Cut);
        let (id, request) = pending(&s);
        assert!(matches!(request, DocumentRequest::Copy { merged: false, cut: true }));
        let capture = s.capture_clipboard(id).unwrap();
        assert_eq!(capture.progress, "Cutting…");
        s.complete_document_request(id, Ok(true)).unwrap();
        s.frame(2, 2).unwrap();
        let erased = s.renderer_mut().pending_operations.clone();
        assert!(matches!(erased.as_slice(), [(layer, op)] if *layer == paint && matches!(op.kind, LayerOperationKind::Erase { .. })));
        assert!(s.engine.document().selection.is_some(), "the selection stays");

        s.renderer_mut().pending_operations.clear();
        invoke(&mut s, CommandId::Cut);
        let (id, _) = pending(&s);
        s.capture_clipboard(id).unwrap();
        s.layer_edit(Edit::SetLayerOpacity { id: paint, opacity: 0.5 }).unwrap();
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
    fn clipboard_chords_and_menus() {
        let mut s = clip_session();
        for (command, chord) in [
            (CommandId::Copy, "Ctrl+C"),
            (CommandId::Cut, "Ctrl+X"),
            (CommandId::CopyMerged, "Ctrl+Shift+C"),
            (CommandId::PasteImage, "Ctrl+V"),
            (CommandId::PasteInPlace, "Ctrl+Shift+V"),
        ] {
            assert_eq!(s.command(command).shortcut, chord, "{command:?}");
        }
        assert_eq!(CommandId::PasteImage.label(), "Paste");
        assert_eq!(serde_json::to_value(CommandId::PasteImage).unwrap(), "paste_image", "the persisted id is kept");
        assert!(KeyChord::new("c", Modifiers { command: true, shift: true, alt: false }).available(Platform::Web));
        let edit = s.application_menu(ApplicationMenu::Edit);
        let labels: Vec<_> = edit.sections[2].iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["Cut", "Copy", "Copy Merged", "Paste", "Paste in Place", "Paste Into"]);

        assert!(!key(&mut s, "c", true, true, true).handled, "a focused text field keeps Ctrl+C");
        assert!(s.state.requests.is_empty());
        key(&mut s, "c", false, true, true);
        assert!(!key(&mut s, "v", true, true, true).handled, "a focused text field keeps Ctrl+V");
        assert!(s.state.requests.is_empty());
        key(&mut s, "v", false, true, true);
        assert!(key(&mut s, "c", true, true, false).handled);
        assert!(matches!(pending(&s).1, DocumentRequest::Copy { merged: false, cut: false }));

        let mut mac = session(Platform::Mac);
        let edit = mac.application_menu(ApplicationMenu::Edit);
        assert!(edit.sections.iter().flatten().any(|item| item.label == "Paste"));
        assert!(!edit.sections.iter().flatten().any(|item| item.label == "Copy"));
        mac.frame(1, 1).unwrap();
    }

    #[test]
    fn preset_paste_chords() {
        for (preset, id, chord) in [
            ("photoshop", "command.PasteInto", ("v", true, true)),
            ("gimp", "command.PasteInPlace", ("v", true, false)),
        ] {
            let keys = crate::keymaps::preset(preset).unwrap().keys_for(id).unwrap();
            assert_eq!(keys.len(), 1);
            assert_eq!((keys[0].key.as_str(), keys[0].command, keys[0].alt, keys[0].shift), (chord.0, true, chord.1, chord.2), "{preset}");
        }
    }

    #[test]
    fn the_mac_selection_bar_omits_the_pixel_clipboard_menu() {
        let mut mac = UiSession::new(Recorder::default(), Document::new("mac", 400, 300), [800, 600], Platform::Mac).unwrap();
        invoke(&mut mac, CommandId::SelectAll);
        invoke(&mut mac, CommandId::Move);
        assert!(!mac.state.canvas_bar.unwrap().items.iter().any(|i| i.menu == Some(CanvasBarMenu::Copy)));
    }
}
