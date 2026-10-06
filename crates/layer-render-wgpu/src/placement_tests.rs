use super::*;
use layer_core::{
    Affine, Document, RasterOperation, RasterOperationKind, CoverageSnapshot,
    authored::{Artwork, Occurrence, OccurrenceContent, OccurrenceHandle, PaintSource,
        CoverageHandle, SourceTarget, RecordChange},
    color::{SampleDepth, source::*},
};

pub(crate) fn paint_document(extent: [u32; 2], name: &str) -> Document {
    let mut artwork = Artwork::new(extent).unwrap();
    crate::test_support::add_paint(&mut artwork, name, extent);
    Document::from_artwork(artwork).unwrap()
}
pub(crate) fn occurrence_id(doc: &Document) -> OccurrenceHandle { doc.scene().order()[0] }
pub(crate) fn occurrence(doc: &Document) -> &Occurrence { doc.scene().occurrence(occurrence_id(doc)).unwrap() }
pub(crate) fn occurrence_mut(doc: &mut Document) -> &mut Occurrence {
    let h = occurrence_id(doc); doc.artwork.occurrences.get_mut(h).unwrap()
}
pub(crate) fn target(doc: &Document) -> SourceTarget { doc.scene().source_target(occurrence_id(doc)).unwrap() }
pub(crate) fn paint(doc: &Document) -> &PaintSource { doc.scene().paint_source(occurrence_id(doc)).unwrap() }
pub(crate) fn paint_mut(doc: &mut Document) -> &mut PaintSource {
    let OccurrenceContent::Paint(h) = occurrence(doc).content else { panic!("Paint fixture") };
    doc.artwork.paint.get_mut(h).unwrap()
}
pub(crate) fn set_source(doc: &mut Document, image: Arc<SourceImage>) {
    let source = paint_mut(doc); source.domain = image.extent; source.base = Some(layer_core::authored::PaintBase::new((image).into()));
}
pub(crate) fn set_mask(doc: &mut Document, mut mask: CoverageSnapshot) {
    let source = RecordChange::insert(&doc.artwork.coverage, mask.source);
    mask.use_.source = source.handle;
    let mut owner = occurrence(doc).clone(); owner.mask = Some(mask.use_);
    let owner = RecordChange::replace(&doc.artwork.occurrences, occurrence_id(doc), Some(owner)).unwrap();
    doc.apply(layer_core::Edit::Batch(vec![layer_core::Edit::Coverage(source), layer_core::Edit::Occurrence(owner)])).unwrap();
}
pub(crate) fn mask_snapshot(doc: &Document) -> CoverageSnapshot {
    let (use_, source) = doc.scene().mask(occurrence_id(doc)).unwrap();
    CoverageSnapshot { target: use_.source, source: source.clone(), selection: None, use_: use_.clone() }
}
pub(crate) fn append_paint(doc: &mut Document, name: &str, source: PaintSource) -> (OccurrenceHandle, SourceTarget) {
    let source = RecordChange::insert(&doc.artwork.paint, source);
    let target = SourceTarget::Paint(source.handle);
    let owner = RecordChange::insert(&doc.artwork.occurrences, Occurrence::new(OccurrenceContent::Paint(source.handle), name));
    let id = owner.handle;
    let root = doc.composition().result;
    let mut stack = doc.artwork.stacks.get(root).unwrap().clone(); stack.entries.push(id);
    let stack = RecordChange::replace(&doc.artwork.stacks, root, Some(stack)).unwrap();
    doc.apply(layer_core::Edit::Batch(vec![layer_core::Edit::Paint(source), layer_core::Edit::Occurrence(owner), layer_core::Edit::Stack(stack)])).unwrap();
    (id, target)
}
pub(crate) fn reveal_all(domain: [u32; 2], offset: [i64; 2]) -> CoverageSnapshot {
    CoverageSnapshot::reveal_all(CoverageHandle::from_index(0), domain, offset)
}
fn batch(target: SourceTarget) -> DabBatch {
    crate::test_support::dab_batch(target, crate::tests::test_style(BrushExecution::Dry), Rect::from_extent([128; 2]))
}
fn submit(r: &mut WgpuRasterizer, doc: &Document, dabs: &[Dab], batches: &[DabBatch], reset: bool) {
    r.submit(FramePacket { dabs, dab_batches: batches, reset_layers: reset, ..packet(doc.scene(), [128; 2]) }).unwrap();
}


#[test]
fn raster_gradients_preserve_authored_stops_and_opacity() {
    let extent=[128;2];
    let mut doc=paint_document(extent,"gradient stops");
    set_source(&mut doc,rgba8_source(extent,|_,_|[255;4]));
    let operation=RasterOperation {
        placement:Affine::IDENTITY,
        coverage:reveal_all(extent,[0,0]),
        kind:RasterOperationKind::Gradient {
            start:Point{x:16.,y:64.},end:Point{x:112.,y:64.},
            gradient:layer_core::GradientDefinition {
                stops:[[1.,0.,0.,1.],[0.,0.,1.,1.]].into_iter().enumerate().map(|(i,rgba)|layer_core::GradientStop {
                    position:i as f32,color:layer_core::color::RgbColor::from_linear(layer_core::color::RgbSpace::Srgb,rgba).unwrap(),
                }).collect(),interpolation:layer_core::ColorMixSpace::LinearRgb,
            },
            shape:layer_core::GradientShape::Linear,reverse:false,opacity:0.5,alpha_locked:false,
        },
    };
    let command=DabBatch {kind:DabBatchKind::RasterOperation(0),dab_count:0,damage:operation.bounds(extent),..batch(target(&doc))};
    paint_mut(&mut doc).operations=Arc::new(vec![operation]);
    for blend in layer_core::BlendSpace::ALL {
        paint_mut(&mut doc).raster=layer_core::raster::RasterRevision::pending();
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.submit(FramePacket {dab_batches:std::slice::from_ref(&command),reset_layers:true,blend_space:blend,..packet(doc.scene(),extent)}).unwrap();
        paint(&doc).raster.wait_data().unwrap();
        let pixels=r.readback_srgb_rgba8().unwrap();
        for x in [4usize,32,64,96,124] {
            let t=((x as f32+0.5-16.)/96.).clamp(0.,1.);
            let expected=[1.-0.5*t,0.5,0.5+0.5*t].map(|v|(layer_core::color::RgbSpace::Srgb.encode(f64::from(v))*255.).round() as u8);
            let actual=&pixels[(64*128+x)*4..][..4];
            assert!(actual[..3].iter().zip(expected).all(|(a,b)|a.abs_diff(b)<=2) && actual[3]==255,"{blend:?} x={x}: {actual:?} vs {expected:?}");
        }
    }
}

#[test]
fn offset_photo_incremental_composition_matches_rebuild_with_alpha_edges() {
    let size = [1024, 768];
    let mut photo = paint_document([128; 2], "moving alpha photo");
    set_source(&mut photo, rgba8_source(size, |x, y| {
        [(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, ((x / 7 + y / 11) % 256) as u8]
    }));
    occurrence_mut(&mut photo).opacity = 0.63;
    let source = paint(&photo).clone();
    let (behind, _) = append_paint(&mut photo, "behind", source);
    let behind = photo.artwork.occurrences.get_mut(behind).unwrap();
    behind.opacity = 0.78;
    behind.offset = [140, 87];
    let canvas = [1031, 777]; // Both partial edge tiles and full interior tiles.
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut rebuilt = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut source_misses = None;
    for (step, offset) in [[10, 19], [-71, -45], [403, 33], [100, -38]].into_iter().enumerate() {
        occurrence_mut(&mut photo).offset = offset;
        let mut images = Vec::new();
        for (r, full) in [(&mut r, false), (&mut rebuilt, true)] {
            if full { r.scale_display = None; }
            r.submit(FramePacket {
                view: ViewState { width_px: canvas[0], height_px: canvas[1], ..view() },
                reset_layers: step == 0,
                ..packet(photo.scene(), canvas)
            }).unwrap();
            images.push(page_bytes(&r, crate::test_support::document_texture(&r)));
        }
        let misses = r.metrics().source_tile_misses;
        if let Some(before) = source_misses { assert_eq!(misses, before, "moving reuses source pixels"); }
        else { source_misses = Some(misses); }
        let maximum = crate::test_support::max_error_bytes(&images[0], &images[1]);
        assert!(maximum < 0.0001, "offset {step}: live pixel error {maximum}");
    }
    for (opacity, mode, mask) in [(1., DabMode::Paint, false), (0.63, DabMode::Paint, false),
        (1., DabMode::Erase, false), (0.63, DabMode::Erase, true)] {
        occurrence_mut(&mut photo).opacity = opacity;
        if mask {
            occurrence_mut(&mut photo).offset = [120, -38];
            set_mask(&mut photo, reveal_all(size, [10, 20]));
        }
        let local = |target| {
            let offset = photo.scene().target_offset(target);
            let mut ink = dab([0.1, 0.7, 0.2, 0.45]);
            ink.center = Point { x: 230. - offset[0] as f32, y: 180. - offset[1] as f32 };
            ink.radii = [32.; 2];
            ink.contact = [1., 0., 0., 0.];
            ink
        };
        let mut stroke = batch(target(&photo));
        stroke.style = preset_style(layer_core::DefaultBrushPreset::GPen);
        stroke.kind = DabBatchKind::Preview;
        stroke.style.mode = mode;
        let ink = local(stroke.target);
        stroke.damage = ink.bounds();
        let mut baseline = None;
        for prediction in [false, true, false] {
            let mut images = Vec::new();
            for (r, full) in [(&mut r, false), (&mut rebuilt, true)] {
                if full { r.scale_display = None; }
                let before = r.metrics.composited_pixels;
                r.submit(FramePacket {
                    view: ViewState { width_px: canvas[0], height_px: canvas[1], ..view() },
                    dabs: if prediction { std::slice::from_ref(&ink) } else { &[] },
                    dab_batches: if prediction { std::slice::from_ref(&stroke) } else { &[] },
                    composite_all: full || baseline.is_none(),
                    ..packet(photo.scene(), canvas)
                }).unwrap();
                if prediction && !full {
                    assert!(r.metrics.composited_pixels - before < u64::from(canvas[0]) * u64::from(canvas[1]),
                        "an offset contact must not rebuild the entire canvas");
                }
                images.push(page_bytes(&r, crate::test_support::document_texture(&r)));
            }
            let maximum = crate::test_support::max_error_bytes(&images[0], &images[1]);
            assert!(maximum < 0.0001, "prediction={prediction}, {mode:?}, opacity={opacity}: {maximum}");
            if let Some(before) = &baseline {
                if prediction { assert!(&images[0] != before, "prediction changes live pixels"); }
                else { assert!(&images[0] == before, "cancel restores live pixels exactly"); }
            } else { baseline = Some(images.remove(0)); }
        }
        if mask {
            stroke.target = SourceTarget::Coverage(occurrence(&photo).mask.as_ref().unwrap().source);
            stroke.kind = DabBatchKind::Persistent;
            let ink = local(stroke.target);
            stroke.damage = ink.bounds();
            let mut images = Vec::new();
            for (r, full) in [(&mut r, false), (&mut rebuilt, true)] {
                if full { r.scale_display = None; }
                r.submit(FramePacket {
                    view: ViewState { width_px: canvas[0], height_px: canvas[1], ..view() },
                    dabs: std::slice::from_ref(&ink),
                    dab_batches: std::slice::from_ref(&stroke),
                    composite_all: full,
                    ..packet(photo.scene(), canvas)
                }).unwrap();
                images.push(page_bytes(&r, crate::test_support::document_texture(&r)));
            }
            assert!(images[0] != baseline.unwrap(), "mask painting changes live pixels");
            let maximum = crate::test_support::max_error_bytes(&images[0], &images[1]);
            assert!(maximum < 0.0001, "offset mask damage matches rebuilt composition: {maximum}");
        }
    }
}

#[test]
fn offset_photo_mask_linking_preserves_position_and_apply_preserves_pixels() {
    let size = [600, 400];
    let mut layer = paint_document([128; 2], "offset masked photo");
    set_source(&mut layer, rgba8_source(size, |_, _| [255; 4]));
    occurrence_mut(&mut layer).offset = [-120, 0];
    let mut mask = reveal_all(size, [0, 0]);
    mask.source.default_coverage = 0.;
    crate::test_support::materialize_mask(&mut mask.source,
        Selection::polygon(vec![
            Point { x: 150., y: 0. },
            Point { x: 200., y: 0. },
            Point { x: 200., y: 400. },
            Point { x: 150., y: 400. },
        ])
        .unwrap(),
        layer.composition().color,
    );
    set_mask(&mut layer, mask);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &layer, &[], &[], true);
    let original = r.readback_srgb_rgba8().unwrap();
    assert_eq!(pixel(&mut r, 40, 30), [255; 4]);
    assert_eq!(pixel(&mut r, 20, 30), [0; 4]);
    let owner = occurrence(&layer).clone();
    occurrence_mut(&mut layer).mask.as_mut().unwrap().set_linked(false, &owner).unwrap();
    submit(&mut r, &layer, &[], &[], false);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        original,
        "unlink does not move coverage"
    );
    occurrence_mut(&mut layer).offset[0] += 10;
    submit(&mut r, &layer, &[], &[], false);
    assert_eq!(pixel(&mut r, 35, 30), [255; 4]);
    assert_eq!(pixel(&mut r, 85, 30), [0; 4]);
    let unlinked = r.readback_srgb_rgba8().unwrap();
    let owner = occurrence(&layer).clone();
    occurrence_mut(&mut layer).mask.as_mut().unwrap().set_linked(true, &owner).unwrap();
    submit(&mut r, &layer, &[], &[], false);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        unlinked,
        "relink does not move coverage"
    );
    occurrence_mut(&mut layer).offset[0] += 10;
    submit(&mut r, &layer, &[], &[], false);
    assert_eq!(pixel(&mut r, 35, 30), [0; 4]);
    assert_eq!(pixel(&mut r, 85, 30), [255; 4]);
    let before_apply = r.readback_srgb_rgba8().unwrap();
    let mut mask = mask_snapshot(&layer);
    mask.use_.offset = layer_core::offsets::checked_sub(layer.scene().target_origin(SourceTarget::Coverage(mask.use_.source)), layer.scene().target_origin(target(&layer))).unwrap();
    mask.use_.linked = false;
    occurrence_mut(&mut layer).mask = None;
    let op = RasterOperation {
        placement: layer_core::Affine::IDENTITY,
        coverage: mask,
        kind: RasterOperationKind::ApplyMask,
    };
    let command = DabBatch {
        kind: DabBatchKind::RasterOperation(0),
        dab_count: 0,
        damage: op.bounds(size),
        ..batch(target(&layer))
    };
    Arc::make_mut(&mut paint_mut(&mut layer).operations).push(op);
    paint_mut(&mut layer).raster = layer_core::raster::RasterRevision::pending();
    submit(&mut r, &layer, &[], &[command], false);
    paint(&layer).raster.wait_data().unwrap();
    Arc::make_mut(&mut paint_mut(&mut layer).operations).clear();
    submit(&mut r, &layer, &[], &[], false);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        before_apply,
        "applying an offset mask does not change the visible result"
    );
    assert_eq!(paint(&layer).base.as_ref().unwrap().image.storage().extent, size);
}

#[test]
fn offset_photo_edits_and_restores_tiles_outside_canvas_bounds() {
    use layer_core::raster::RasterRevision;
    let size = [900, 700];
    let source = rgba8_source(size, |_, _| [255; 4]);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = paint_document([128; 2], "offset editable photo");
    set_source(&mut layer, source.clone());
    occurrence_mut(&mut layer).offset = [-560, -440];
    submit(&mut r, &layer, &[], &[], true);
    let before = paint(&layer).raster.clone();
    paint_mut(&mut layer).raster = RasterRevision::pending();
    let mut ink = dab([1., 0., 0., 1.]);
    ink.center = Point { x: 624., y: 504. };
    ink.radii = [24.; 2];
    let mut stroke = batch(target(&layer));
    stroke.damage = ink.bounds();
    stroke.style.selection = Some(Arc::new(
        Selection::polygon(vec![
            Point { x: 610., y: 490. },
            Point { x: 650., y: 490. },
            Point { x: 650., y: 540. },
            Point { x: 610., y: 540. },
        ])
        .unwrap(),
    ));
    submit(&mut r, &layer, &[ink], &[stroke], false);
    let data = paint(&layer).raster.wait_data().unwrap();
    assert!(data.tiles.keys().any(|key| key.coordinate == [2, 1]));
    assert_eq!(pixel(&mut r, 64, 64), [255, 0, 0, 255]);
    assert_eq!(
        pixel(&mut r, 45, 64),
        [255; 4],
        "selection clips in source coordinates"
    );
    let after = paint(&layer).raster.clone();
    paint_mut(&mut layer).raster = before;
    submit(&mut r, &layer, &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [255; 4]);
    paint_mut(&mut layer).raster = after;
    submit(&mut r, &layer, &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [255, 0, 0, 255]);
    // Recreate the renderer to exercise cold restoration, not just resident history.
    let mut reopened = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut reopened, &layer, &[], &[], true);
    assert_eq!(pixel(&mut reopened, 64, 64), [255, 0, 0, 255]);
}

#[test]
fn offset_photo_samples_full_source_across_tiles_without_creating_raster() {
    let size = [1537, 1025];
    let canvas = [384, 256];
    let alpha = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= size[0] as i32 || y >= size[1] as i32 {
            return 0.;
        }
        f32::from((x / 71 + y / 53) % 3 != 0)
    };
    let mut builder = SourceBuilder::new(
        size,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: Default::default(),
            profile_assumed: false,
        },
        32 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..size[1] {
        let row: Vec<_> = (0..size[0])
            .flat_map(|x| {
                [
                    65535u16,
                    65535,
                    65535,
                    (alpha(x as i32, y as i32) * 65535.) as u16,
                ]
                .into_iter()
                .flat_map(u16::to_le_bytes)
            })
            .collect();
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    let digests: Vec<_> = source.tiles.values().map(|t| t.content_digest().unwrap()).collect();
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = paint_document([128; 2], "offset photo");
    set_source(&mut layer, source.clone());
    for (n, offset) in [[30, 10], [-1050, -700], [-700, 15], [0, 0]].into_iter().enumerate() {
        occurrence_mut(&mut layer).offset = offset;
        r.submit(FramePacket {
            reset_layers: n == 0,
            ..packet(layer.scene(), canvas)
        })
        .unwrap();
        let pixels = r.readback_srgb_rgba8().unwrap();
        for y in 0..canvas[1] {
            for x in 0..canvas[0] {
                let expected = alpha(x as i32 - offset[0] as i32, y as i32 - offset[1] as i32) * 255.;
                let actual = pixels[((y * canvas[0] + x) * 4 + 3) as usize] as f32;
                assert_eq!(actual, expected, "offset={offset:?} at={x},{y}");
            }
        }
        assert!(paint(&layer).raster.is_empty());
        assert!(
            r.paint_layers.iter().all(|p| p.pages.is_empty()),
            "an offset must not create paint backing"
        );
        assert_eq!(
            source.tiles.values().map(|t| t.content_digest().unwrap()).collect::<Vec<_>>(),
            digests
        );
    }
}

#[test]
fn watercolor_prediction_and_commit_cover_an_offset_source_larger_than_canvas() {
    watercolor_prediction_and_commit_with_canvas(256, false);
}

#[test]
fn watercolor_prediction_and_commit_in_canvas_control() {
    watercolor_prediction_and_commit_with_canvas(512, false);
}

#[test]
fn watercolor_prediction_and_commit_at_reduced_zoom() {
    watercolor_prediction_and_commit_with_canvas(256, true);
}

fn watercolor_prediction_and_commit_with_canvas(canvas: u32, reduced: bool) {
    watercolor_prediction_and_commit_with_renderer(WgpuRasterizer::new_native_headless(Default::default()).unwrap(), canvas, reduced);
}

pub(crate) fn watercolor_prediction_and_commit_with_renderer(mut r: WgpuRasterizer, canvas: u32, reduced: bool) {
    let mut layer = paint_document([128; 2], "larger offset watercolor source");
    let source = rgba8_source([512; 2], |_, _| [160, 170, 180, 255]);
    set_source(&mut layer, source.clone());
    occurrence_mut(&mut layer).offset = [-128, 0];
    let send = |r: &mut WgpuRasterizer, layer: &Document, dabs: &[Dab], batches: &[DabBatch], reset| {
        let mut frame = crate::test_support::packet(layer.scene(), [canvas; 2]);
        if reduced {
            frame.composite_all = false;
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            frame.view.width_px = canvas / 8; frame.view.height_px = canvas / 8;
        }
        r.submit(FramePacket { dabs, dab_batches: batches, reset_layers: reset,
            ..frame }).unwrap();
    };
    let read = |r: &mut WgpuRasterizer| {
        if reduced {
            assert!(r.scale_display.as_ref().unwrap().plan.level > 0);
            crate::scene::scale::tests::display_pixels(r).into_iter().flatten().map(|v| (v.clamp(0., 1.) * 255.).round() as u8).collect::<Vec<_>>()
        } else { r.readback_srgb_rgba8().unwrap() }
    };
    send(&mut r, &layer, &[], &[], true);
    let original = read(&mut r);
    let mut ink = dab([0.1, 0.3, 0.8, 1.]);
    ink.center = Point { x: 303., y: 64. };
    ink.radii = [12.; 2];
    ink.material = [0.8, 1., 1., 1.];
    let mut stroke = batch(target(&layer));
    stroke.style = preset_style(layer_core::DefaultBrushPreset::WatercolorWash);
    stroke.damage = ink.bounds();
    let mut preview = stroke.clone(); preview.kind = DabBatchKind::Preview;
    send(&mut r, &layer, &[ink], &[preview.clone()], false);
    let prediction = read(&mut r);
    let preview_storage = r.native_edit.as_ref().unwrap().storage_bytes();
    assert_ne!(prediction, original);
    assert!(paint(&layer).raster.is_empty());
    assert!(r.paint_layers.iter().find(|p| p.id == target(&layer)).unwrap().watercolor.is_none());
    assert!(r.preview_watercolor_wetness_pages.iter().any(|page| page.coordinate == [1, 0]));
    let mut next = preview.clone(); next.first_dab = 1; next.stroke_start = false;
    send(&mut r, &layer, &[ink, ink], &[preview, next], false);
    assert_eq!(r.native_edit.as_ref().unwrap().storage_bytes(), preview_storage, "same-footprint prediction reuses native storage");
    assert!(paint(&layer).raster.is_empty());
    send(&mut r, &layer, &[], &[], false);
    assert_eq!(read(&mut r), original, "cancel restores source pixels");
    assert!(r.preview_watercolor_wetness_pages.is_empty());
    assert_eq!(r.native_edit.as_ref().unwrap().storage_bytes(), preview_storage, "cancel retains bounded reusable prediction storage");
    assert!(r.paint_layers.iter().find(|p| p.id == target(&layer)).unwrap().watercolor.is_none());
    paint_mut(&mut layer).raster = layer_core::raster::RasterRevision::pending();
    send(&mut r, &layer, &[ink], &[stroke.clone()], false);
    let accepted = read(&mut r);
    let mut fresh = WgpuRasterizer::new_native_headless(r.document_color()).unwrap();
    let mut fresh_layer = layer.clone();
    paint_mut(&mut fresh_layer).raster = Default::default();
    send(&mut fresh, &fresh_layer, &[], &[], true);
    paint_mut(&mut fresh_layer).raster = layer_core::raster::RasterRevision::pending();
    send(&mut fresh, &fresh_layer, &[ink], &[stroke.clone()], false);
    assert_eq!(read(&mut fresh), accepted, "fresh persistent oracle excludes preview/cancel reuse");
    let maximum = accepted.iter().zip(&prediction).map(|(a,b)|a.abs_diff(*b)).max().unwrap();
    assert!(maximum <= 1, "prediction matches committed material: maximum={maximum}, edge_after_stroke={}", stroke.style.rendering.edge_after_stroke);
    let data = paint(&layer).raster.wait_data().unwrap();
    assert!(data.watercolor.is_some());
    for plane in [layer_core::raster::RasterPlane::Color, layer_core::raster::RasterPlane::WatercolorWetness] {
        assert!(data.tiles.contains_key(&layer_core::raster::TileKey { plane, coordinate: [1, 0] }));
    }
    assert!(std::sync::Arc::ptr_eq(paint(&layer).base.as_ref().unwrap().image.storage(), &source));
    send(&mut r, &layer, &[], &[], true);
    assert_eq!(read(&mut r), accepted, "backing restore preserves pigment and wetness");
}

#[test]
fn layer_tiles_beside_the_largest_offset_keep_their_exact_pixels() {
    let source = rgba8_source([300, 20], |x, y| [(x * 37 % 256) as u8, (y * 11 + x) as u8, (x * x % 256) as u8, 255]);
    let tiles = |offset: i64| {
        let mut layer = paint_document([64; 2], "far photo");
        set_source(&mut layer, source.clone());
        occurrence_mut(&mut layer).offset = [offset, 3];
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        submit(&mut r, &layer, &[], &[], true);
        let mut scene = crate::scene::Scene::new(&r);
        let first = u32::try_from(offset.div_euclid(i64::from(PAGE_SIZE))).unwrap();
        (first..first + 3).map(|x| {
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            let tile = scene.layer_tile_for_query(&mut r, layer.scene(), target(&layer), [x, 0], &mut encoder).unwrap();
            r.uploads.finish(&encoder);
            encoder.submit(&r.queue);
            crate::layer_tests::page_bytes(&r, &tile.texture)
        }).collect::<Vec<_>>()
    };
    let far = layer_core::offsets::MAX_OFFSET - 5;
    assert_eq!(tiles(far), tiles(far.rem_euclid(i64::from(PAGE_SIZE))));
}
