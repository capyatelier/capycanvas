//! Live window seams, partial damage and resource ceilings against full-image
//! composition. Edited images use an arithmetic tolerance, not byte parity.
use super::image_windows::{effect, add_effect, document, set_entries, set_mask, insert_effect};
use super::*;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::{Document, CoverageSnapshot, Selection, authored::*};

fn packet(document: &Document, extent: [u32; 2]) -> FramePacket<'_> {
    FramePacket {
        selection_overlays: Some(&document.working.selection_overlays),
        inspect_mask: document.working.inspect_mask,
        view: ViewState { width_px: extent[0], height_px: extent[1], ..test_view() },
        ..crate::test_support::packet(document.scene(), extent)
    }
}
fn pixels(r: &WgpuRasterizer) -> Vec<u8> {
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
}
fn close(actual: &[u8], reference: &[u8]) {
    assert_eq!(actual.len(), reference.len());
    let mut maximum = 0f32;
    for (i, (a, b)) in actual
        .chunks_exact(4)
        .zip(reference.chunks_exact(4))
        .enumerate()
    {
        let a = f32::from_le_bytes(a.try_into().unwrap());
        let b = f32::from_le_bytes(b.try_into().unwrap());
        let error = (a - b).abs();
        assert!(error <= 3e-6, "sample {i}: {a} != {b}");
        maximum = maximum.max(error);
    }
    eprintln!("window composite maximum absolute channel error: {maximum}");
}

#[test]
fn completed_filter_windows_release_cached_texture_references() {
    let extent = [777, 533];
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor::default()).unwrap();
    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(8 << 20);
    let mut document = document(extent, DocumentColor::default());
    let entries = vec![effect(&mut document, false, false), effect(&mut document, true, false)];
    set_entries(&mut document, entries);
    r.submit(packet(&document, extent)).unwrap();
    r.wait_idle().unwrap();
    assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), 0);
    if let Some(report) = r.device.generate_allocator_report() {
        let retained: Vec<_> = report.allocations.iter().filter(|allocation| matches!(allocation.name.as_str(),
            "effect source cache" | "effect result cache" | "effect mask cache" |
            "reusable effect intermediate"
        )).collect();
        assert!(retained.is_empty(), "completed filter windows still own allocations: {retained:?}");
    }
}

#[test]
fn native_live_windows_match_full_filters_masks_clips_and_reconfiguration() {
    let extent = [777, 533];
    const CAP: u64 = 16 * 1024 * 1024;
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let mut r =
                WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            for clipped in [false, true] {
                let mut document = document(extent, DocumentColor { space, depth });
                let first = effect(&mut document, false, false);
                let second = effect(&mut document, false, false);
                document.artwork.occurrences.get_mut(first).unwrap().opacity = 0.63;
                document.artwork.occurrences.get_mut(first).unwrap().attachment = if clipped { layer_core::Attachment::Effect } else { layer_core::Attachment::None };
                document.artwork.occurrences.get_mut(second).unwrap().attachment = if clipped { layer_core::Attachment::Effect } else { layer_core::Attachment::None };
                let mut mask = CoverageSnapshot::reveal_all(document.artwork.coverage.next_handle(), extent, Point { x: 7., y: -9. });
                mask.source.default_coverage = 0.;
                mask.source.initial = Some(Selection::polygon(vec![Point { x: 0., y: 0. }, Point { x: 760., y: 99. }, Point { x: 440., y: 533. }]).unwrap());
                let inside = effect(&mut document, true, false);
                let outside = effect(&mut document, true, false);
                set_entries(&mut document, vec![first, second, inside, outside]);
                set_mask(&mut document, first, mask);
                for group in [false, true] {
                    if group {
                        let stack = document.artwork.stacks.insert(PortableId::random(), Stack { entries: vec![first, second, inside] }).unwrap();
                        let mut occurrence = Occurrence::new(OccurrenceContent::Stack(stack), "isolated group");
                        occurrence.opacity = 0.79;
                        let group = document.artwork.occurrences.insert(PortableId::random(), occurrence).unwrap();
                        set_entries(&mut document, vec![group, outside]);
                        document.working.inspect_mask = Some(first);
                    }
                    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(u64::MAX);
                    r.submit(packet(&document, extent)).unwrap();
                    let expected = pixels(&r);
                    let full_cache = r.scene.as_ref().unwrap().image_cache_bytes();
                    let before = r.metrics().image_window_submissions;
                    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(CAP);
                    r.submit(packet(&document, extent)).unwrap();
                    close(&pixels(&r), &expected);
                    assert!(r.metrics().image_window_submissions > before + 1);
                    assert!(r.metrics().image_window_peak_bytes <= CAP + 96 * document.scene().order().len() as u64);
                    assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), 0,
                        "completed filter windows must release temporary pixels");
                    eprintln!(
                        "{space:?} {depth:?} clipped={clipped} group={group}: full cache={full_cache}, retained window={}, peak window={}, cap={CAP}",
                        r.scene.as_ref().unwrap().image_cache_bytes(),
                        r.metrics().image_window_peak_bytes
                    );
                    // A second partial window frame cannot reuse a previous
                    // window as if it held the entire document.
                    let composed = r.metrics().composited_pixels;
                    compose_region(&mut r, &document, extent, PixelRect::new(253, 251, 279, 283));
                    assert!(r.metrics().composited_pixels - composed < u64::from(extent[0]) * u64::from(extent[1]),
                        "releasing temporary pixels must preserve damage metadata");
                    close(&pixels(&r), &expected);
                    // Returning to the ordinary cache must repopulate all of it.
                    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(u64::MAX);
                    r.submit(packet(&document, extent)).unwrap();
                    close(&pixels(&r), &expected);
                }
            }
        }
    }
}

#[test]
fn native_live_global_limit_rejects_before_document_or_submission_changes() {
    let extent = [333, 291];
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    })
    .unwrap();
    let mut document = document(extent, DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    let adjustment = effect(&mut document, false, true);
    let generator = effect(&mut document, true, false);
    set_entries(&mut document, vec![adjustment, generator]);
    r.submit(packet(&document, extent)).unwrap();
    let before = pixels(&r);
    let texture = r.scale_display.as_ref().map(|c| c.texture().clone());
    let metrics = r.metrics();
    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(1024);
    // Includes a resize and reset: rejection must precede both.
    let error = r
        .submit(FramePacket {
            reset_layers: true,
            ..packet(&document, [6000, 4000])
        })
        .unwrap_err();
    assert!(error.to_string().contains("Document-wide"));
    assert_eq!(r.document_extent, extent);
    assert_eq!(r.scale_display.as_ref().map(|c| c.texture().clone()), texture);
    assert_eq!(r.metrics(), metrics);
    assert_eq!(
        pixels(&r),
        before,
        "rejection must preserve the exact current composite"
    );
    // Removing the unsupported adjustment leaves the renderer usable.
    set_entries(&mut document, vec![generator]);
    r.submit(packet(&document, extent)).unwrap();
    assert!(pixels(&r).iter().any(|b| *b != 0));
}

#[test]
fn native_live_window_halos_follow_paint_undo_redo_and_recreated_renderer() {
    use layer_core::raster::RasterRevision;
    let extent = [777, 533];
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    const CAP: u64 = 8 * 1024 * 1024;
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(CAP);
    let mut document = document(extent, color);
    let (_, target) = crate::test_support::add_paint(&mut document.artwork, "retouch", extent);
    let paint_owner = document.artwork.stacks.get(document.composition().result).unwrap().entries[0];
    let first = effect(&mut document, false, false);
    let second = effect(&mut document, false, false);
    set_entries(&mut document, vec![first, second, paint_owner]);
    let SourceTarget::Paint(paint) = target else { unreachable!() };
    let original = document.artwork.paint.get(paint).unwrap().raster.clone();
    r.submit(packet(&document, extent)).unwrap();
    let before = pixels(&r);
    document.artwork.paint.get_mut(paint).unwrap().raster = RasterRevision::pending();
    let dabs = [test_dab([255., 256.], [0.13, 0.72, 0.41, 0.37], 0.5)];
    let batches = [crate::test_support::dab_batch(
        target,
        test_style(BrushExecution::Dry),
        Rect { min: Point { x: 235., y: 236. }, max: Point { x: 275., y: 276. } },
    )];
    r.submit(FramePacket {
        dabs: &dabs,
        dab_batches: &batches,
        composite_all: false,
        ..packet(&document, extent)
    })
    .unwrap();
    let edited = document.artwork.paint.get(paint).unwrap().raster.clone();
    let backing = edited.wait_data().unwrap();
    assert!(!backing.tiles.is_empty());
    let incremental = pixels(&r);
    assert_ne!(incremental, before);
    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(u64::MAX);
    r.submit(packet(&document, extent)).unwrap();
    close(&incremental, &pixels(&r));
    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(CAP);
    document.artwork.paint.get_mut(paint).unwrap().raster = original;
    r.submit(FramePacket {
        composite_all: false,
        ..packet(&document, extent)
    })
    .unwrap();
    assert_eq!(pixels(&r), before, "undo restores the exact empty artwork");
    document.artwork.paint.get_mut(paint).unwrap().raster = edited;
    r.submit(FramePacket {
        composite_all: false,
        ..packet(&document, extent)
    })
    .unwrap();
    close(&pixels(&r), &incremental);
    assert!(Arc::ptr_eq(
        &document.artwork.paint.get(paint).unwrap().raster.wait_data().unwrap(),
        &backing
    ));
    let mut replacement = WgpuRasterizer::new_native_headless(color).unwrap();
    replacement.native_edit.as_mut().unwrap().image_pixel_bytes = Some(CAP);
    replacement.submit(packet(&document, extent)).unwrap();
    close(&pixels(&replacement), &incremental);
    assert!(Arc::ptr_eq(
        &document.artwork.paint.get(paint).unwrap().raster.wait_data().unwrap(),
        &backing
    ));
    // Metadata changes still invalidate the complete adjustment, even when a
    // caller supplies only a small paint rectangle in the same frame.
    document.artwork.occurrences.get_mut(first).unwrap().opacity = 0.13;
    compose_region(&mut r, &document, extent, PixelRect::new(259, 261, 263, 265));
    let changed = pixels(&r);
    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(u64::MAX);
    r.submit(packet(&document, extent)).unwrap();
    close(&changed, &pixels(&r));
}

#[test]
fn native_live_animated_windows_refresh_with_empty_paint_damage_and_keep_frozen_time() {
    let extent = [777, 533];
    let mut document = document(extent, DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 });
    let mut program = with_time_controls((*super::image_windows::program(true, false).program).clone());
    program.wgsl = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let a=.37;return vec4<f32>(vec3<f32>(fract(p.x/37.+fx_time(b)/7.),fract(p.y/29.),.27)*a,a);}".into();
    let generator = add_effect(&mut document, layer_core::EffectInstance::new(Arc::new(program)));
    let adjustment = effect(&mut document, false, false);
    set_entries(&mut document, vec![adjustment, generator]);
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    })
    .unwrap();
    const CAP: u64 = 8 * 1024 * 1024;
    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(CAP);
    r.submit(packet(&document, extent)).unwrap();
    let before = pixels(&r);
    r.submit(FramePacket {
        time_seconds: 3.,
        composite_all: false,
        ..packet(&document, extent)
    })
    .unwrap();
    let animated = pixels(&r);
    assert_ne!(animated, before);
    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(u64::MAX);
    r.submit(FramePacket {
        time_seconds: 3.,
        ..packet(&document, extent)
    })
    .unwrap();
    close(&pixels(&r), &animated);
    let OccurrenceContent::Effect(application) = document.artwork.occurrences.get(generator).unwrap().content else { unreachable!() };
    let definition = document.artwork.effects.get(application).unwrap().definition;
    for (key, value) in [("animate", layer_core::EffectValue::Toggle(false)), ("time", layer_core::EffectValue::Number(3.))] {
        let index = document.artwork.definitions.get(definition).unwrap().program.parameters.iter().position(|p|p.key.as_ref()==key).unwrap();
        document.artwork.effects.get_mut(application).unwrap().values[index] = value;
    }
    r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(CAP);
    r.submit(FramePacket {
        time_seconds: 7.,
        ..packet(&document, extent)
    })
    .unwrap();
    close(&pixels(&r), &animated);
    let submissions = r.metrics().image_window_submissions;
    r.submit(FramePacket {
        time_seconds: 8.,
        composite_all: false,
        ..packet(&document, extent)
    })
    .unwrap();
    assert_eq!(r.metrics().image_window_submissions, submissions);
    close(&pixels(&r), &animated);
}

/// Bakes whose filter images exceed the budget run in windows with their
/// halos, and store the same pixels as a bake of the whole layer.
#[test]
fn bakes_run_their_filters_in_bounded_windows_with_the_same_pixels() {
    use layer_core::{BlendSpace, Document, EffectInstance, EffectValue, MergeKind, SeparationFilters};
    use layer_engine::{CanvasEngine, ViewTransform, input_queue};
    const CAP: u64 = 16 * 1024 * 1024;
    let extent = [1100, 700];
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        for space in BlendSpace::ALL {
            let mut doc = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
            let root = doc.artwork.root;
            let composition = doc.artwork.compositions.get_mut(root).unwrap();
            composition.color = DocumentColor { space: RgbSpace::Srgb, depth };
            composition.blend = space;
            let photo = doc.scene().order()[0];
            let SourceTarget::Paint(paint) = doc.scene().source_target(photo).unwrap() else { unreachable!() };
            doc.artwork.paint.get_mut(paint).unwrap().original = Some(layer_core::color::source::rgba8_source(extent, |x, y| {
                [(x * 7 % 256) as u8, (y * 5 % 256) as u8, ((x ^ y) % 256) as u8, if (x / 97 + y / 61) % 3 == 0 { 140 } else { 255 }]
            }));
            let mut effect = EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
            effect.set("sigma", EffectValue::Number(9.)).unwrap();
            let blur = insert_effect(&mut doc, effect);
            doc.working.occurrence = Some(blur);
            doc.working.target = None;
            let bake = |cap: u64, separate: bool| {
                let gpu = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
                let (_producer, consumer) = input_queue(8);
                let mut engine = CanvasEngine::new(gpu, doc.clone(), consumer, test_view(), ViewTransform::IDENTITY).unwrap();
                let settle = |engine: &mut CanvasEngine<WgpuRasterizer>| {
                    engine.render_frame_at(0).unwrap();
                    while engine.has_pending_document_edits() {
                        engine.render_frame_at(0).unwrap();
                    }
                };
                settle(&mut engine);
                engine.backend_mut().native_edit.as_mut().unwrap().image_pixel_bytes = Some(cap);
                let before = engine.backend().metrics();
                let baked: Vec<_> = if separate {
                    let filters = SeparationFilters::new(layer_core::bundled_effect_catalog(), 7.5).unwrap();
                    let plan = engine.document().separation_plan(photo, &filters, ["Frequency Separation", "Low", "High"].map(std::sync::Arc::from)).unwrap();
                    let baked = plan.operations.iter().map(|(id, _)| *id).collect();
                    engine.insert_with_operations(plan.edits, plan.operations, None).unwrap();
                    baked
                } else {
                    let plan = engine.document().merge_plan(MergeKind::Down).unwrap();
                    let target = plan.target;
                    engine.insert_with_operations(plan.edits, vec![(target, plan.operation)], None).unwrap();
                    vec![target]
                };
                settle(&mut engine);
                let after = engine.backend().metrics();
                let pixels: Vec<Vec<u8>> = baked
                    .iter()
                    .flat_map(|id| {
                        let data = engine.document().scene().raster(*id).unwrap().wait_data().unwrap();
                        data.tiles.values().map(|t| t.wait_backing().unwrap().decode().unwrap()).collect::<Vec<_>>()
                    })
                    .collect();
                (pixels, after.image_window_submissions - before.image_window_submissions, after.image_window_peak_bytes)
            };
            for separate in [false, true].into_iter().filter(|s| !s || space == BlendSpace::Perceptual) {
                let what = format!("{depth:?} {space:?} {}", if separate { "Frequency Separation" } else { "Merge Down of a blur" });
                let (full, unbounded, _) = bake(u64::MAX, separate);
                assert_eq!(unbounded, 0, "{what}: a bake that fits composes its filters whole");
                let (windowed, windows, peak) = bake(CAP, separate);
                assert!(windows > 2, "{what}: {windows} windows");
                assert!(peak <= CAP, "{what}: {peak} bytes of filter images");
                assert_eq!(windowed.len(), full.len(), "{what}");
                for (tile, (a, b)) in windowed.iter().zip(&full).enumerate() {
                    let differ = a.iter().zip(b).filter(|(a, b)| a != b).count();
                    assert_eq!(differ, 0, "{what}: tile {tile} differs in {differ} bytes");
                }
            }
        }
    }
}

#[test]
fn frequency_separation_at_sigma_85_bakes_discrete_gaussian_and_survives_history_and_archive() {
    use layer_core::{BlendSpace, Document, SeparationFilters};
    use layer_core::package::{self, codec::{PreparedPackage, OpenOutcome}};
    use std::sync::atomic::AtomicBool;
    use layer_engine::{CanvasEngine, ViewTransform, input_queue};
    let extent=[65,33];
    let mut document=Document::new(PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames{paint:"Original".into(),paper:"Paper".into()});
    let root=document.artwork.root;
    document.artwork.compositions.get_mut(root).unwrap().color=DocumentColor{space:RgbSpace::Srgb,depth:SampleDepth::U16};
    document.artwork.compositions.get_mut(root).unwrap().blend=BlendSpace::Perceptual;
    let original=document.scene().order()[0];
    let paper=document.scene().order()[1];
    document.artwork.occurrences.get_mut(paper).unwrap().visible=false;
    let SourceTarget::Paint(paint)=document.scene().source_target(original).unwrap() else {unreachable!()};
    document.artwork.paint.get_mut(paint).unwrap().original=Some(layer_core::color::source::rgba8_source(extent,|x,_|{
        let v=if x<32 {64}else{192};[v,v,v,255]
    }));
    let gpu=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let (_,consumer)=input_queue(8);
    let mut engine=CanvasEngine::new(gpu,document,consumer,crate::test_support::view(extent),ViewTransform::IDENTITY).unwrap();
    let finish=|engine:&mut CanvasEngine<WgpuRasterizer>| {
        engine.render_frame_at(0).unwrap();
        for _ in 0..1000 {
            if !engine.has_pending_document_edits(){return;}
            engine.render_frame_at(0).unwrap();
        }
        panic!("Frequency Separation did not publish its raster results");
    };
    finish(&mut engine);
    let before=engine.backend_mut().readback_srgb_rgba8().unwrap();
    let filters=SeparationFilters::new(layer_core::bundled_effect_catalog(),85.).unwrap();
    let plan=engine.document().separation_plan(original,&filters,
        ["Frequency Separation","Low","High"].map(Arc::from)).unwrap();
    let SourceTarget::Paint(low_target)=plan.operations[0].0 else {unreachable!()};
    let low_id=plan.edits.iter().find_map(|edit| match edit {
        layer_core::Edit::Paint(change) if change.handle==low_target=>Some(change.id),_=>None,
    }).unwrap();
    engine.insert_with_operations(plan.edits,plan.operations,None).unwrap();finish(&mut engine);
    let low=engine.document().artwork.paint.get(low_target).unwrap().raster.wait_data().unwrap();
    let tile=low.tiles.values().next().unwrap().wait_backing().unwrap().decode().unwrap();
    let weights=(-255..=255).map(|k|(-0.5*(f64::from(k)/85.).powi(2)).exp()).collect::<Vec<_>>();
    let sum=weights.iter().sum::<f64>();
    for x in [0,1,31,32,63,64] {
        let expected=(-255..=255).zip(&weights).map(|(k,w)|{
            let v=if (x+k).clamp(0,64)<32 {64.}else{192.};v/255.*w/sum
        }).sum::<f64>();
        let offset=(16*256+x as usize)*8;
        let actual=f64::from(u16::from_le_bytes(tile[offset..offset+2].try_into().unwrap()))/65535.;
        assert!((actual-expected).abs()<2e-5,"Low x={x}: {actual} != {expected}");
    }
    let separated=engine.backend_mut().readback_srgb_rgba8().unwrap();
    assert!(before.iter().zip(&separated).all(|(a,b)|a.abs_diff(*b)<=1),"Low and High reconstruct the original");
    assert!(engine.undo().unwrap());finish(&mut engine);
    assert_eq!(engine.backend_mut().readback_srgb_rgba8().unwrap(),before);
    assert!(engine.redo().unwrap());finish(&mut engine);
    assert_eq!(engine.backend_mut().readback_srgb_rgba8().unwrap(),separated);
    let cancelled=AtomicBool::new(false);
    let capture=engine.capture_artwork(0).unwrap();
    let prepared=PreparedPackage::prepare(&capture,None,&cancelled).unwrap();
    let mut archive=Vec::new();prepared.write(&mut archive,&cancelled).unwrap();
    let source=package::transport::ChunkedBytes::new(archive.chunks(package::MAX_RANGE_BYTES).map(Arc::from).collect()).unwrap();
    let backing=package::ImmutableBacking::new(Arc::new(source)).unwrap();
    let OpenOutcome::Candidate{artwork,..}=package::codec::open(backing,Default::default(),&cancelled).unwrap() else {panic!("editable package")};
    let reopened=Document::from_artwork(artwork).unwrap();
    assert_eq!(reopened.composition().color.depth,SampleDepth::U16);
    let low=reopened.artwork.paint.resolve(low_id).unwrap();
    let low_owner=reopened.scene().source_owner(SourceTarget::Paint(low)).unwrap();
    assert_eq!(reopened.scene().occurrence(low_owner).unwrap().name.as_ref(),"Low");
    let gpu=WgpuRasterizer::new_native_headless(reopened.composition().color).unwrap();
    let (_,consumer)=input_queue(8);
    let mut fresh=CanvasEngine::new(gpu,reopened,consumer,crate::test_support::view(extent),ViewTransform::IDENTITY).unwrap();
    finish(&mut fresh);assert_eq!(fresh.backend_mut().readback_srgb_rgba8().unwrap(),separated);
}

fn compose_region(r: &mut WgpuRasterizer, document: &Document, extent: [u32; 2], damage: PixelRect) {
    let mut scene = r.scene.take().unwrap();
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    scene.compose(r, FramePacket { composite_all: false, ..packet(document, extent) },
        &[], damage, &mut encoder, None).unwrap();
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    r.scene = Some(scene);
}
