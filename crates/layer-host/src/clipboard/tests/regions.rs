use super::*;
use layer_core::{
    CoverageSource, MaskUse, Occurrence, OccurrenceContent, OccurrenceHandle,
    PaintBase, PaintSource, PortableId, SceneScope, SourceTarget, Stack,
};

fn rectangle(x: f32, y: f32, width: f32, height: f32) -> Selection {
    Selection::polygon([[x, y], [x + width, y], [x + width, y + height], [x, y + height]]
        .map(|[x, y]| Point { x, y }).to_vec()).unwrap()
}

fn pixel(source: &SourceImage, x: usize, y: usize) -> [u8; 4] {
    rows(source)[y][x * 4..x * 4 + 4].try_into().unwrap()
}

#[track_caller]
fn close(actual: [u8; 4], expected: [u8; 4]) {
    assert!(actual.into_iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2), "{actual:?} != {expected:?}");
}

fn same_records(actual: &layer_core::Artwork, expected: &layer_core::Artwork) {
    macro_rules! same { ($($store:ident),+) => { $(
        assert_eq!(actual.$store.iter().collect::<Vec<_>>(), expected.$store.iter().collect::<Vec<_>>(), stringify!($store));
    )+ }; }
    same!(compositions, stacks, occurrences, paint, objects, coverage, effects, selections, guides, outputs);
}

fn frames_until(owner: &mut NativeHost, ready: impl Fn(&layer_core::Document) -> bool) {
    let started = std::time::Instant::now();
    loop {
        let now = started.elapsed().as_nanos() as u64;
        owner.prepare_canvas_frame(now, now, true).unwrap();
        if owner.startup.canvas_ready && ready(owner.session.engine().document()) { break; }
        assert!(started.elapsed() < std::time::Duration::from_secs(30), "canvas startup timed out");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn raw(owner: &mut NativeHost, target: SourceTarget) -> Arc<SourceImage> {
    frames_until(owner, |_| true);
    let gpu = crate::tasks::gpu(&mut owner.session).unwrap().snapshot_gpu();
    let scene = owner.session.engine().scene_snapshot();
    let extent = scene.view().composition().size;
    let mut renderer = gpu.capture_scene(scene, SceneScope::Raw(target), Default::default()).unwrap();
    Arc::new(renderer.write_clip([0, 0, extent[0], extent[1]], None, true, 1 << 24).unwrap().0.unwrap())
}

fn layered(selection: Selection, grouped: bool, objects: bool) -> (NativeHost, Vec<OccurrenceHandle>) {
    let mut document = layer_core::Document::new(PortableId::random(), 64, 48,
        layer_core::DocumentNames { paint: "Red".into(), paper: "Paper".into() });
    crate::test_support::hide_paper(&mut document);
    let first = document.working.occurrence.unwrap();
    crate::test_support::active_source_mut(&mut document).base = Some(PaintBase::new(photo([64, 48], |_, _| [220, 30, 10, 255]).into()));
    let paint = document.artwork.paint.insert(PortableId::random(), PaintSource {
        domain: [40, 40], base: Some(PaintBase::new(photo([40, 40], |_, _| [10, 200, 40, 255]).into())),
        color_mode: Default::default(), raster: Default::default(), operations: Default::default(),
    }).unwrap();
    let mut occurrence = Occurrence::new(OccurrenceContent::Paint(paint), "Green");
    occurrence.offset = [4, -3];
    let second = document.artwork.occurrences.insert(PortableId::random(), occurrence).unwrap();
    let root = document.composition().result;
    document.artwork.stacks.get_mut(root).unwrap().entries.insert(0, second);
    let mut document = layer_core::Document::from_artwork(document.artwork).unwrap();
    let mut members = vec![first, second];
    if objects {
        let object = layer_core::ImageObject::new(photo([64, 48], |_, _| [15, 30, 210, 255]).into());
        let (id, edit) = document.create_object_layer_edit("Photo", object, None, 0).unwrap();
        document.apply(edit).unwrap();
        members.push(id);
    }
    let selected = if grouped {
        let stack = document.artwork.stacks.insert(PortableId::random(), Stack { entries: members.clone() }).unwrap();
        let mut folder = Occurrence::new(OccurrenceContent::Stack(stack), "Folder");
        folder.offset = [3, 2];
        let folder = document.artwork.occurrences.insert(PortableId::random(), folder).unwrap();
        let root = document.artwork.stacks.get_mut(root).unwrap();
        root.entries.retain(|id| !members.contains(id));
        root.entries.insert(0, folder);
        vec![folder]
    } else { members.clone() };
    let mut document = layer_core::Document::from_artwork(document.artwork).unwrap();
    document.working.occurrence = Some(selected[0]);
    document.working.target = document.scene().source_target(selected[0]);
    document.working.layer_selection = selected.into_iter().collect();
    document.working.selection = Some(selection);
    let mut owner = host(document);
    owner.session.frame(0, 0).unwrap();
    (owner, members)
}

#[test]
fn selected_regions_keep_separate_layers_and_group_offsets() {
    for grouped in [false, true] {
        let (mut owner, _) = layered(rectangle(12., 10., 16., 12.), grouped, true);
        let clip = copy(&mut owner, CommandId::Copy);
        assert_eq!((clip.origin, clip.source.extent), ([12, 10], [16, 12]));
        let copied = clip.layers.as_ref().unwrap();
        let document = clip.document(owner.session.localization()).unwrap();
        let paints = document.scene().order().iter().copied().filter(|id| document.scene().paint_source(*id).is_some()).collect::<Vec<_>>();
        assert_eq!(paints.len(), 3);
        assert_eq!(copied.roots.len(), if grouped { 1 } else { 3 });
        for id in paints {
            let source = document.scene().paint_source(id).unwrap();
            assert_eq!(document.layer_offset(id), [0, 0]);
            assert_eq!(source.domain, [16, 12]);
            let source = source.base.as_ref().unwrap().image.storage();
            let expected = match document.scene().occurrence(id).unwrap().name.as_ref() {
                "Red" => [220, 30, 10, 255], "Green" => [10, 200, 40, 255], "Photo" => [15, 30, 210, 255],
                name => panic!("Unexpected layer {name}"),
            };
            close(pixel(source, 0, 0), expected);
            close(pixel(source, 15, 11), expected);
        }
        owner.session.paste_clip(&clip, PasteMode::InPlace).unwrap();
        let document = owner.session.engine().document();
        for root in &document.working.layer_selection {
            for id in document.layer_subtrees(&[*root]) {
                if document.scene().paint_source(id).is_some() { assert_eq!(document.layer_offset(id), [12, 10]); }
            }
        }
    }
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
fn selected_regions_apply_soft_and_inverted_coverage_to_each_layer() {
    let pixels = layer_core::SelectionPixels::bytes([64, 48], [0, 0, 64, 48], vec![0x80ff_0080; 16 * 48]).unwrap();
    for inverted in [false, true] {
        let mut selection = Selection::pixels(Arc::new(pixels.clone()));
        selection.inverted = inverted;
        let (mut owner, _) = layered(selection, false, false);
        let clip = copy(&mut owner, CommandId::Copy);
        let document = clip.document(owner.session.localization()).unwrap();
        for id in document.scene().order() {
            let Some(source) = document.scene().paint_source(*id) else { continue; };
            let source = source.base.as_ref().unwrap().image.storage();
            let expected = if inverted { [127, 255, 0, 127] } else { [128, 0, 255, 128] };
            for (x, alpha) in expected.into_iter().enumerate() {
                assert!(pixel(source, x + 12, 12)[3].abs_diff(alpha) <= 2, "inverted={inverted}, x={x}");
            }
        }
    }
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

fn pending_copy(owner: &mut NativeHost, command: CommandId) -> (u32, ClipTask) {
    owner.dispatch(UiAction::Invoke { command }).unwrap();
    let id = owner.session.state().requests.iter().find(|r| matches!(r.kind,
        HostRequestKind::Document { request: DocumentRequest::Copy { .. } })).unwrap().id;
    (id, ClipTask::capture(&mut owner.session, id).unwrap())
}

#[test]
fn multi_layer_cut_waits_for_publication_and_undo_restores_objects_and_all_pixels() {
    let (mut owner, members) = layered(rectangle(12., 10., 16., 12.), true, true);
    let original = owner.session.engine().document().artwork.clone();
    let (id, task) = pending_copy(&mut owner, CommandId::Cut);
    let _ = task.run("failed".into(), Default::default()).unwrap();
    same_records(&owner.session.engine().document().artwork, &original);
    owner.session.complete_document_request(id, Err("Clipboard refused the image".into())).unwrap();
    same_records(&owner.session.engine().document().artwork, &original);
    let _ = copy(&mut owner, CommandId::Cut);
    frames_until(&mut owner, |document| members.iter().all(|id| document.scene().paint_source(*id).is_some()));
    for id in &members {
        let target = owner.session.engine().document().scene().source_target(*id).unwrap();
        assert!(matches!(target, SourceTarget::Paint(_)));
        let cut = raw(&mut owner, target);
        assert_eq!(pixel(&cut, 18, 16)[3], 0);
        assert_eq!(pixel(&cut, 9, 7)[3], 255);
    }
    owner.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    same_records(&owner.session.engine().document().artwork, &original);
    drop(owner);
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

fn masked(value: f32, selection: Option<Selection>, moved: bool, inverted: bool, locked: bool) -> NativeHost {
    let base = photo_host(None);
    let mut document = base.session.engine().document().clone();
    drop(base);
    let active = document.working.occurrence.unwrap();
    let mask = document.artwork.coverage.insert(PortableId::random(), CoverageSource {
        domain: [64, 48], raster: Default::default(), default_coverage: value, operations: Default::default(),
    }).unwrap();
    let occurrence = document.artwork.occurrences.get_mut(active).unwrap();
    occurrence.offset = if moved { [7, 3] } else { [0; 2] };
    occurrence.locked = locked;
    occurrence.mask = Some(MaskUse { source: mask, enabled: true, linked: !moved, inverted,
        offset: if moved { [-5, -4] } else { [0; 2] } });
    let mut document = layer_core::Document::from_artwork(document.artwork).unwrap();
    document.working.occurrence = Some(active);
    document.working.layer_selection = [active].into();
    document.working.target = Some(SourceTarget::Coverage(mask));
    document.working.inspect_mask = Some(active);
    document.working.selection = selection;
    let mut owner = host(document);
    owner.session.frame(0, 0).unwrap();
    owner
}

#[test]
fn focused_mask_copy_is_opaque_raw_gray_and_internal_paste_preserves_it() {
    for (gray, moved, inverted) in [(0., false, false), (0.25, true, true), (0.5, false, true), (1., false, false)] {
        let mut source = masked(gray, None, moved, inverted, false);
        let clip = copy(&mut source, CommandId::Copy);
        let expected = (gray * 255.).round() as u8;
        close(pixel(&clip.source, 20, 20), [expected, expected, expected, 255]);
        let png = layer_color::photo::read_photo(std::io::Cursor::new(clip.png.to_vec()), Default::default()).unwrap();
        close(pixel(&png, 20, 20), [expected, expected, expected, 255]);
        let mut destination = masked(0.8, None, false, false, false);
        let original = destination.session.engine().document().artwork.clone();
        destination.session.paste_clip(&clip, PasteMode::InPlace).unwrap();
        let target = destination.session.engine().document().working.target.unwrap();
        close(pixel(&raw(&mut destination, target), 20, 20), [expected, expected, expected, 255]);
        assert_eq!(destination.session.engine().document().scene().order().len(), 2);
        destination.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        same_records(&destination.session.engine().document().artwork, &original);
    }
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
fn external_mask_paste_respects_selection_alpha_and_unlinked_inverted_placement() {
    let mut owner = masked(0.5, Some(rectangle(12., 10., 16., 12.)), true, true, false);
    let original = owner.session.engine().document().artwork.clone();
    let image = photo([64, 48], |x, _| if x < 20 { [64, 64, 64, 128] } else { [255, 255, 255, 0] });
    let context = owner.session.image_placement_context(None, None).unwrap();
    owner.session.paste_layer_sources(vec![("Mask".into(), (*image).clone())], PasteMode::InPlace, &context).unwrap();
    let target = owner.session.engine().document().working.target.unwrap();
    let result = raw(&mut owner, target);
    close(pixel(&result, 16, 16), [96, 96, 96, 255]);
    close(pixel(&result, 24, 16), [128, 128, 128, 255]);
    close(pixel(&result, 8, 8), [128, 128, 128, 255]);
    owner.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    same_records(&owner.session.engine().document().artwork, &original);
    let image = photo([8, 4], |_, _| [200, 200, 200, 255]);
    let context = owner.session.image_placement_context(None, None).unwrap();
    owner.session.paste_layer_sources(vec![("Centered mask".into(), (*image).clone())], PasteMode::Paste, &context).unwrap();
    let result = raw(&mut owner, target);
    close(pixel(&result, 16, 14), [200, 200, 200, 255]);
    close(pixel(&result, 23, 17), [200, 200, 200, 255]);
    close(pixel(&result, 15, 14), [128, 128, 128, 255]);
    close(pixel(&result, 24, 17), [128, 128, 128, 255]);
    owner.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    same_records(&owner.session.engine().document().artwork, &original);
    drop(owner);
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
fn mask_cut_erases_only_selected_coverage_and_locked_masks_still_copy() {
    let mut owner = masked(0.5, Some(rectangle(12., 10., 16., 12.)), true, true, false);
    let original = owner.session.engine().document().artwork.clone();
    let clip = copy(&mut owner, CommandId::Cut);
    close(pixel(&clip.source, 5, 5), [128, 128, 128, 255]);
    let target = owner.session.engine().document().working.target.unwrap();
    let result = raw(&mut owner, target);
    close(pixel(&result, 18, 16), [0, 0, 0, 255]);
    close(pixel(&result, 8, 8), [128, 128, 128, 255]);
    owner.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    same_records(&owner.session.engine().document().artwork, &original);
    let mut locked = masked(0.5, None, false, false, true);
    let clip = copy(&mut locked, CommandId::Copy);
    close(pixel(&clip.source, 10, 10), [128, 128, 128, 255]);
    assert!(locked.session.paste_clip(&clip, PasteMode::Paste).is_err());
    assert!(locked.dispatch(UiAction::Invoke { command: CommandId::Cut }).is_err());
    assert!(!locked.session.state().requests.iter().any(|r| matches!(r.kind,
        HostRequestKind::Document { request: DocumentRequest::Copy { cut: true, .. } })));
    drop((owner, locked));
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
fn mask_paste_multiplies_transparency_with_soft_and_inverted_selection_coverage() {
    let pixels = layer_core::SelectionPixels::bytes([64, 48], [0, 0, 64, 48], vec![0x80ff_0080; 16 * 48]).unwrap();
    for inverted in [false, true] {
        let mut selection = Selection::pixels(Arc::new(pixels.clone()));
        selection.inverted = inverted;
        let mut owner = masked(0.5, Some(selection), false, false, false);
        let image = photo([64, 48], |_, _| [0, 0, 0, 128]);
        let context = owner.session.image_placement_context(None, None).unwrap();
        owner.session.paste_layer_sources(vec![("Soft mask".into(), (*image).clone())], PasteMode::InPlace, &context).unwrap();
        let target = owner.session.engine().document().working.target.unwrap();
        let result = raw(&mut owner, target);
        let expected = if inverted { [96, 64, 128, 96] } else { [96, 128, 64, 96] };
        for (x, gray) in expected.into_iter().enumerate() { close(pixel(&result, x + 12, 12), [gray, gray, gray, 255]); }
    }
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
fn hdr_mask_paste_clamps_source_gray_before_blending_transparency() {
    let owner = masked(0.5, None, false, false, false);
    let mut document = owner.session.engine().document().clone();
    drop(owner);
    crate::test_support::composition_mut(&mut document).color.depth = SampleDepth::F32;
    let mut owner = host(document);
    frames_until(&mut owner, |_| true);
    let mut builder = SourceBuilder::new([64, 48], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::F32, profile: ColorProfile::default(), profile_assumed: false,
    }, 1 << 24).unwrap();
    let row = (0..64).flat_map(|x| {
        let gray = if x < 32 { 2.0f32 } else { -0.5 };
        [gray, gray, gray, 0.5].into_iter().flat_map(f32::to_le_bytes)
    }).collect::<Vec<_>>();
    for _ in 0..48 { builder.push_row(&row).unwrap(); }
    let context = owner.session.image_placement_context(None, None).unwrap();
    owner.session.paste_layer_sources(vec![("HDR mask".into(), builder.finish().unwrap())], PasteMode::InPlace, &context).unwrap();
    frames_until(&mut owner, |_| true);
    let clip = copy(&mut owner, CommandId::Copy);
    assert_eq!(clip.source.interpretation.depth, SampleDepth::U16);
    let pixels = rows(&clip.source);
    for (x, expected) in [(16usize, 49151u16), (48, 16384)] {
        for channel in 0..4 {
            let index = (x * 4 + channel) * 2;
            let actual = u16::from_le_bytes(pixels[16][index..index + 2].try_into().unwrap());
            let expected = if channel == 3 { u16::MAX } else { expected };
            assert!(actual.abs_diff(expected) <= 2, "x={x}, channel={channel}: {actual} != {expected}");
        }
    }
    drop(owner);
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
fn mask_clip_preserves_gray_between_wide_gamut_and_hdr_drawings() {
    use layer_core::color::{DocumentColor, RgbSpace};
    fn colored(value: f32, color: DocumentColor) -> NativeHost {
        let owner = masked(value, None, false, false, false);
        let mut document = owner.session.engine().document().clone();
        drop(owner);
        crate::test_support::composition_mut(&mut document).color = color;
        let mut builder = SourceBuilder::new([64, 48], SourceInterpretation {
            channels: SourceChannels::Rgba, depth: color.depth, profile: ColorProfile::Builtin(color.space), profile_assumed: false,
        }, 1 << 24).unwrap();
        let pixel = match color.depth {
            SampleDepth::U8 => vec![25, 50, 75, 255],
            SampleDepth::U16 => [6400u16, 12800, 19200, 65535].into_iter().flat_map(u16::to_le_bytes).collect(),
            SampleDepth::F32 => [0.1f32, 0.2, 0.3, 1.].into_iter().flat_map(f32::to_le_bytes).collect(),
            depth => panic!("Unsupported test depth {depth:?}"),
        };
        for _ in 0..48 { builder.push_row(&pixel.repeat(64)).unwrap(); }
        let target = document.scene().source_target(document.working.occurrence.unwrap()).unwrap();
        let SourceTarget::Paint(paint) = target else { panic!("Expected a paint layer"); };
        document.artwork.paint.get_mut(paint).unwrap().base = Some(PaintBase {
            image: Arc::new(builder.finish().unwrap()).into(), offset: [0; 2], policy: layer_core::PaintBasePolicy::WorkingPixels,
        });
        let mut owner = host(document);
        owner.session.frame(0, 0).unwrap();
        owner
    }
    fn sample(source: &SourceImage) -> [f64; 4] {
        let bytes = rows(source);
        match source.interpretation.depth {
            SampleDepth::U8 => std::array::from_fn(|i| f64::from(bytes[16][16 * 4 + i]) / 255.),
            SampleDepth::U16 => std::array::from_fn(|i| {
                let index = (16 * 4 + i) * 2;
                f64::from(u16::from_le_bytes(bytes[16][index..index + 2].try_into().unwrap())) / 65535.
            }),
            depth => panic!("A mask clip must use unsigned coverage samples, got {depth:?}"),
        }
    }
    for (space, depth, destination) in [
        (RgbSpace::AdobeRgb, SampleDepth::U16, DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::F32 }),
        (RgbSpace::ProPhoto, SampleDepth::F32, DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U8 }),
    ] {
        let mut source = colored(0.375, DocumentColor { space, depth });
        let clip = copy(&mut source, CommandId::Copy);
        assert_eq!(clip.color, DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 });
        for (actual, expected) in sample(&clip.source).into_iter().zip([0.375, 0.375, 0.375, 1.]) {
            assert!((actual - expected).abs() <= 2. / 65535., "{space:?}/{depth:?}: {actual} != {expected}");
        }
        let mut target = colored(0.8, destination);
        target.session.paste_clip(&clip, PasteMode::InPlace).unwrap();
        let target_id = target.session.engine().document().working.target.unwrap();
        let _ = raw(&mut target, target_id);
        let pasted = copy(&mut target, CommandId::Copy);
        for (actual, expected) in sample(&pasted.source).into_iter().zip([0.375, 0.375, 0.375, 1.]) {
            assert!((actual - expected).abs() <= 2. / 255., "{destination:?}: {actual} != {expected}");
        }
    }
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
fn selected_region_with_attached_blur_does_not_reveal_pixels_outside_the_selection() {
    for effect in [None, Some("gaussian_blur")] {
        let (mut source, members) = layered(rectangle(12., 10., 16., 12.), true, false);
        let working = source.session.engine().document().working.clone();
        if let Some(effect) = effect {
            source.dispatch(UiAction::Effect { action: layer_ui::EffectAction::InsertAttached {
                effect: effect.into(), owner: layer_ui::occurrence_token(members[0]), epoch: source.session.state().document_file.epoch,
            } }).unwrap();
        }
        let mut document = source.session.engine().document().clone();
        document.working = working;
        drop(source);
        let mut source = host(document);
        frames_until(&mut source, |_| true);
        let clip = copy(&mut source, CommandId::Copy);
        let mut destination = layer_core::Document::new(PortableId::random(), 64, 48,
            layer_core::DocumentNames { paint: "Blank".into(), paper: "Paper".into() });
        crate::test_support::hide_paper(&mut destination);
        let mut destination = host(destination);
        destination.session.paste_clip(&clip, PasteMode::InPlace).unwrap();
        frames_until(&mut destination, |_| true);
        let result = copy(&mut destination, CommandId::CopyMerged);
        assert_eq!(pixel(&result.source, 11, 16)[3], 0, "{effect:?} must not expose unselected pixels on the left");
        assert_eq!(pixel(&result.source, 28, 16)[3], 0, "{effect:?} must not expose unselected pixels on the right");
        assert_eq!(pixel(&result.source, 20, 9)[3], 0, "{effect:?} must not expose unselected pixels above");
        assert_eq!(pixel(&result.source, 20, 22)[3], 0, "{effect:?} must not expose unselected pixels below");
        assert_eq!(pixel(&result.source, 20, 16)[3], 255);
        drop((source, destination));
    }
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
fn selected_group_generator_copies_and_cuts_only_selected_pixels() {
    let (source, mut members) = layered(rectangle(12., 10., 16., 12.), true, false);
    let mut document = source.session.engine().document().clone();
    drop(source);
    let group = document.working.occurrence.unwrap();
    let working = document.working.clone();
    let generator = *document.scene().order().iter().find(|id| document.scene().occurrence(**id).unwrap().name.as_ref() == "Paper").unwrap();
    let occurrence = document.artwork.occurrences.get_mut(generator).unwrap();
    occurrence.visible = true;
    occurrence.name = "Generated white".into();
    let root = document.composition().result;
    document.artwork.stacks.get_mut(root).unwrap().entries.retain(|id| *id != generator);
    let OccurrenceContent::Stack(stack) = document.scene().occurrence(group).unwrap().content else { panic!("Expected a group"); };
    document.artwork.stacks.get_mut(stack).unwrap().entries.push(generator);
    let mut document = layer_core::Document::from_artwork(document.artwork).unwrap();
    document.working = working;
    members.push(generator);
    let mut owner = host(document);
    frames_until(&mut owner, |_| true);
    let original = owner.session.engine().document().artwork.clone();
    let copied = copy(&mut owner, CommandId::Copy);
    let copied_document = copied.document(owner.session.localization()).unwrap();
    let generator_copy = *copied_document.scene().order().iter().find(|id| copied_document.scene().occurrence(**id).unwrap().name.as_ref() == "Generated white").unwrap();
    let source = copied_document.scene().paint_source(generator_copy).unwrap();
    assert_eq!(source.domain, [16, 12]);
    close(pixel(source.base.as_ref().unwrap().image.storage(), 8, 6), [255; 4]);
    let _ = copy(&mut owner, CommandId::Cut);
    frames_until(&mut owner, |document| members.iter().all(|id| document.scene().paint_source(*id).is_some()));
    let target = owner.session.engine().document().scene().source_target(generator).unwrap();
    let pixels = raw(&mut owner, target);
    assert_eq!(pixel(&pixels, 18, 16)[3], 0);
    close(pixel(&pixels, 9, 7), [255; 4]);
    owner.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    same_records(&owner.session.engine().document().artwork, &original);
    owner.session.paste_clip(&copied, PasteMode::InPlace).unwrap();
    let pasted = owner.session.engine().document();
    let root = pasted.working.occurrence.unwrap();
    let children = pasted.layer_subtrees(&[root]);
    assert_eq!(children.iter().filter(|id| pasted.scene().paint_source(**id).is_some()).count(), 3);
    for id in children { if pasted.scene().paint_source(id).is_some() { assert_eq!(pasted.layer_offset(id), [12, 10]); } }
    drop(owner);
    layer_render_wgpu::finish_shader_compiler_shutdown();
}
