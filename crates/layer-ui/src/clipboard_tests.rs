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
            objects: None,
        }
    }

    fn translation(s: &UiSession<Recorder>) -> [f32; 2] {
        let doc = s.engine.document();
        let offset = doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap().offset;
        [offset[0] as f32, offset[1] as f32]
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
        let paper = occurrence_handle(2).unwrap();
        s.layer_edit(s.engine.document().select_occurrence_edit(paper).unwrap()).unwrap();
        assert_eq!(reason(&s, CommandId::Copy).as_deref(), Some("An effect layer has no pixels of its own"));
        assert!(s.command(CommandId::CopyMerged).enabled, "Copy Merged ignores the active layer");
        let paint = s.engine.document().scene().order()[0];
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
    fn paste_keeps_a_visible_position_and_otherwise_centres_in_one_step() {
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
        assert_eq!(translation(&s), [20., 25.], "centred in the view");
        invoke(&mut s, CommandId::Undo);
        for (rotation, expected) in [(0., Some([21., 26.])), (0.3, None)] {
            s.state.camera.rotation = rotation;
            s.state.camera.center_on([30.5, 30.5]);
            s.paste_clip(&hidden, PasteMode::Paste).unwrap();
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
        let object = doc.object_layer_children(handle).expect("Paste Into makes an image layer")[0];
        assert_eq!(doc.working.objects, [object].into());
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
        let object_name = |s: &UiSession<Recorder>| {
            let doc = s.engine.document();
            doc.scene().object(*doc.working.objects.iter().next().unwrap()).unwrap().name.clone()
        };
        let layer_name = |s: &UiSession<Recorder>| {
            let doc = s.engine.document();
            doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap().name.clone()
        };
        s.place_layer_source(&long, source(), None).unwrap();
        assert!(bounded(&object_name(&s)), "place");
        invoke(&mut s, CommandId::CancelTransform);
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(vec![(long.clone(), source())], PasteMode::InPlace, &context).unwrap();
        assert!(bounded(&object_name(&s)), "paste in place");
        let mut copied = clip([8, 8], [4, 4], DocumentColor::default());
        copied.name = long.clone();
        s.paste_clip(&copied, PasteMode::Paste).unwrap();
        assert!(bounded(&layer_name(&s)), "paste");
        select(&mut s, Some(rectangle([10., 10., 50., 50.])));
        s.paste_clip(&copied, PasteMode::Into).unwrap();
        assert!(bounded(&object_name(&s)), "paste into");
        let kept = "é".repeat(layer_core::MAX_NAME_BYTES / 2);
        let mut object = layer_core::ImageObject::new(layer_core::Image::new(Arc::new(source())), kept.as_str());
        object.affine = layer_core::Affine64([1., 0., 0., 1., 4., 4.]);
        copied.objects = Some(Arc::new(clipboard::ObjectClip { objects: vec![object] }));
        s.paste_clip(&copied, PasteMode::Paste).unwrap();
        assert_eq!(object_name(&s).as_ref(), kept, "a copied image keeps the name the drawing admitted");
        assert!(s.place_layer_source("\u{7} ", source(), None).is_err(), "a name of only spaces and controls is refused");
        assert!(s.import_layer_source(&long, source()).is_err(), "import keeps names literally and refuses ones past the limit");
    }

    #[test]
    fn external_paste_in_place_centres_at_full_size_without_handles() {
        let mut s = clip_session();
        let image = || vec![("Clipboard image".to_string(), Arc::unwrap_or_clone(rgba8_source([40, 20], |_, _| [1; 4])))];
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(image(), PasteMode::InPlace, &context).unwrap();
        assert!(!s.objects.placing());
        let centre = s.state.camera.surface_to_document64(s.state.camera.work_area_center().map(f64::from));
        let doc = s.engine.document();
        let object = *doc.working.objects.iter().next().unwrap();
        assert_eq!(doc.scene().object(object).unwrap().affine, layer_core::Affine64([1., 0., 0., 1., centre[0] - 20., centre[1] - 10.]));
        let context = s.image_placement_context(None, None).unwrap();
        s.paste_layer_sources(image(), PasteMode::Paste, &context).unwrap();
        assert!(s.objects.placing(), "an image from another app opens the placement handles");
        invoke(&mut s, CommandId::CancelTransform);
        let context = s.image_placement_context(None, None).unwrap();
        invoke(&mut s, CommandId::AddLayer);
        assert!(s.paste_layer_sources(image(), PasteMode::InPlace, &context).is_err(), "a stale context is refused");
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
        assert_eq!(CommandId::PasteImage.label().as_ref(), "Paste");
        assert_eq!(serde_json::to_value(CommandId::PasteImage).unwrap(), "paste_image", "the persisted id is kept");
        assert!(KeyChord::new("c", Modifiers { command: true, shift: true, alt: false }).available(Platform::Web));
        let edit = s.application_menu(ApplicationMenu::Edit);
        let labels: Vec<_> = edit.sections[2].iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["Cut", "Copy", "Copy Pixels", "Copy Merged", "Paste", "Paste as New Image", "Paste in Place", "Paste Into"]);

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
        let mut object = layer_core::ImageObject::new(copied.source.clone().into(), "Original");
        object.affine = layer_core::Affine64([1., 0., 0., 1., -10., 23.]);
        copied.objects = Some(Arc::new(super::super::clipboard::ObjectClip { objects: vec![object.clone()] }));
        let objects = copied.document(&localization).unwrap();
        let kept = objects.artwork.objects.iter().next().unwrap().2;
        assert_eq!(kept.affine, layer_core::Affine64([1., 0., 0., 1., 0., 0.]));
        assert_eq!(kept.image.extent, object.image.extent);
        assert_eq!(kept.image.interpretation, object.image.interpretation);
        assert_eq!(kept.image.tiles.values().next().unwrap().owner_identity(), object.image.tiles.values().next().unwrap().owner_identity());
        assert_eq!(objects.working.objects.len(), 1);
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
        let objects: Vec<_> = document.artwork.objects.iter().map(|(_, _, object)| (object.name.as_ref(), object.affine.0)).collect();
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
