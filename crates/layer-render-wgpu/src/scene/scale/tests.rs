use crate::test_support::float_pixels as pixels;
use super::*;
use crate::test_support::{dab_batch, packet};
use layer_core::color::{SampleDepth, source::*};
use layer_core::{DefaultBrushPreset, Document, CoverageSnapshot};
use layer_core::authored::*;

#[path = "presentation_tests.rs"]
mod presentation;

#[path = "effect_tests.rs"]
mod effects;
#[path = "color_effect_tests.rs"]
mod color_effects;

#[path = "refinement_tests.rs"]
mod refinement;

fn assert_settled(r: &mut WgpuRasterizer, frame: FramePacket<'_>, reference: &[[f32; 4]]) {
    let revision = r.artwork_revision;
    let pages = page_coordinates(PixelRect::full(frame.document_extent)).count();
    let idle = FramePacket { composite_all: false, reset_layers: false, dabs: &[], dab_batches: &[], restore_rasters: &[], ..frame };
    for step in 0..=pages + 1 {
        if !r.has_pending_work() { break; }
        assert!(step < pages + 1, "refinement must finish within one visit per native page");
        r.wait_idle().unwrap();
        let work = r.metrics.composited_pixels;
        r.submit(idle).unwrap();
        assert!(r.metrics.composited_pixels - work <= 4 * u64::from(PAGE_SIZE).pow(2));
        assert_eq!(r.artwork_revision, revision, "idle refinement does not edit artwork");
    }
    let cache = r.scale_display.as_ref().unwrap();
    let error = quality(&display_pixels(r), reference, cache.plan);
    assert!(error[2] < 2e-5, "settled display {error:?}");
    if let Some(overview) = &cache.overview {
        let error = quality(&window_pixels(r, overview.pixels.root().unwrap(), overview.plan), reference, overview.plan);
        assert!(error[2] < 2e-5, "settled overview {error:?}");
    }
    assert_presentation_mip(r);

    let work = r.metrics.composited_pixels;
    r.submit(idle).unwrap();
    assert_eq!(r.metrics.composited_pixels, work, "completed refinement has no further work");
}

fn document() -> Document {
    document_at([517, 259])
}
pub(super) fn document_at(extent: [u32; 2]) -> Document {
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let ink = doc.scene().order()[0];
    set_root_entries(&mut doc, vec![ink]);
    let paint = primary_paint(&doc);
    doc.artwork.paint.get_mut(paint).unwrap().original = Some(rgba8_source(extent, |x, y| [(x / 3) as u8, (y / 2) as u8, 80, 255]));
    doc
}

fn composition_mut(doc: &mut Document) -> &mut layer_core::authored::Composition { let root = doc.artwork.root; doc.artwork.compositions.get_mut(root).unwrap() }
fn set_attachment(occurrence: &mut Occurrence, enabled: bool) {
    occurrence.attachment = if !enabled { Attachment::None }
        else if occurrence.kind() == LayerKind::Effect { Attachment::Effect } else { Attachment::Clip };
}
fn set_attachment_at(doc: &mut Document, index: usize, enabled: bool) {
    set_attachment(occurrence_mut(doc, index), enabled);
    reindex(doc);
}
fn occurrence_at(doc: &Document, index: usize) -> &Occurrence { doc.artwork.occurrences.get(doc.scene().order()[index]).unwrap() }
fn occurrence_mut(doc: &mut Document, index: usize) -> &mut Occurrence { let handle = doc.scene().order()[index]; doc.artwork.occurrences.get_mut(handle).unwrap() }
fn source_at(doc: &Document, index: usize) -> SourceTarget { match occurrence_at(doc, index).content { OccurrenceContent::Paint(paint) => SourceTarget::Paint(paint), _ => unreachable!() } }
fn paint_at(doc: &Document, index: usize) -> &PaintSource { let SourceTarget::Paint(paint) = source_at(doc,index) else { unreachable!() }; doc.artwork.paint.get(paint).unwrap() }
fn paint_mut(doc: &mut Document, index: usize) -> &mut PaintSource { let SourceTarget::Paint(paint) = source_at(doc,index) else { unreachable!() }; doc.artwork.paint.get_mut(paint).unwrap() }
fn copy_paint(doc: &mut Document, index: usize) -> OccurrenceHandle {
    let mut occurrence = occurrence_at(doc,index).clone();
    let source=paint_at(doc,index).clone();
    let paint = doc.artwork.paint.insert(PortableId::random(), source).unwrap();
    if let Some(mask)=&mut occurrence.mask {
        let source=doc.artwork.coverage.get(mask.source).unwrap().clone();
        mask.source=doc.artwork.coverage.insert(PortableId::random(),source).unwrap();
    }
    occurrence.content = OccurrenceContent::Paint(paint);
    doc.artwork.occurrences.insert(PortableId::random(), occurrence).unwrap()
}
fn paint_occurrence(doc: &mut Document, name: &str, original: Option<Arc<SourceImage>>) -> OccurrenceHandle {
    let paint = doc.artwork.paint.insert(PortableId::random(), PaintSource { domain: doc.composition().size, raster: Default::default(), original, operations: Arc::default() }).unwrap();
    doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Paint(paint), name)).unwrap()
}
fn stack_occurrence(doc: &mut Document, name: &str, entries: Vec<OccurrenceHandle>) -> OccurrenceHandle {
    let stack = doc.artwork.stacks.insert(PortableId::random(), Stack { entries }).unwrap();
    doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Stack(stack), name)).unwrap()
}
fn set_root_entries(doc: &mut Document, entries: Vec<OccurrenceHandle>) { let stack=doc.composition().result; doc.artwork.stacks.get_mut(stack).unwrap().entries=entries; reindex(doc); }

fn swap_entries(doc:&mut Document,a:usize,b:usize){ let stack=doc.composition().result;doc.artwork.stacks.get_mut(stack).unwrap().entries.swap(a,b);reindex(doc); }

fn primary_paint(doc: &Document) -> PaintHandle { doc.artwork.paint.iter().next().unwrap().0 }
fn primary_target(doc: &Document) -> SourceTarget { SourceTarget::Paint(primary_paint(doc)) }
fn reindex(doc: &mut Document) {
    let handle = doc.composition().result;
    let edit = layer_core::Edit::Stack(RecordChange { handle, id: doc.artwork.stacks.id(handle).unwrap(), value: doc.artwork.stacks.get(handle).cloned() });
    doc.apply(edit).unwrap();
}
fn insert_occurrence(doc: &mut Document, handle: OccurrenceHandle, position: usize) {
    let stack = doc.composition().result;
    doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(position, handle);
    reindex(doc);
}
fn remove_occurrence(doc: &mut Document, handle: OccurrenceHandle) {
    let stack = doc.composition().result;
    doc.artwork.stacks.get_mut(stack).unwrap().entries.retain(|entry| *entry != handle);
    reindex(doc);
}
fn effect_occurrence(doc: &mut Document, effect: layer_core::EffectInstance, name: &str) -> OccurrenceHandle {
    let definition = doc.artwork.definitions.insert(PortableId::random(), Definition { program: effect.program }).unwrap();
    let application = doc.artwork.effects.insert(PortableId::random(), EffectApplication { definition, values: effect.values}).unwrap();
    doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(application), name)).unwrap()
}
pub(super) fn add_fill(doc: &mut Document, color: layer_core::color::RgbColor) -> OccurrenceHandle {
    let mut effect = layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("solid_color").unwrap().program());
    effect.set("color", layer_core::EffectValue::Color(color)).unwrap();
    let fill = effect_occurrence(doc, effect, "Fill");
    let stack = doc.composition().result;
    let position = doc.artwork.stacks.get(stack).unwrap().entries.len();
    insert_occurrence(doc, fill, position);
    fill
}
fn effect_handle(doc: &Document, handle: OccurrenceHandle) -> EffectHandle {
    match doc.artwork.occurrences.get(handle).unwrap().content { OccurrenceContent::Effect(effect) => effect, _ => unreachable!() }
}
pub(super) fn set_effect_value(doc: &mut Document, handle: OccurrenceHandle, key: &str, value: layer_core::EffectValue) {
    let effect = effect_handle(doc, handle);
    let application = doc.artwork.effects.get(effect).unwrap();
    let program = &doc.artwork.definitions.get(application.definition).unwrap().program;
    let parameter = program.parameters.iter().position(|parameter| parameter.key.as_ref() == key).unwrap();
    doc.artwork.effects.get_mut(effect).unwrap().values[parameter] = value;
}
pub(super) fn coverage_mask(doc: &mut Document, handle: OccurrenceHandle, translation: layer_core::Point, initial: Option<layer_core::Selection>) -> CoverageHandle {
    let source = doc.artwork.coverage.insert(PortableId::random(), CoverageSource { domain: doc.scene().local_extent(handle), raster: Default::default(), initial, default_coverage: 1., operations: Arc::default() }).unwrap();
    doc.artwork.occurrences.get_mut(handle).unwrap().mask = Some(MaskUse { source, enabled: true, linked: true, inverted: false, translation, placement: layer_core::Projective::IDENTITY });
    reindex(doc);
    source
}

#[test]
fn native_material_reduction_preserves_small_hdr_corrections() {
    let mut doc = document_at([256, 256]);
    composition_mut(&mut doc).color.depth = SampleDepth::F32;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let base = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
    let source = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
    let output = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
    let mut pigment = vec![[0., 0., 0., 1.]; 256 * 256];
    for (i, value) in [1048576., 1048576.125, 1048576.25, 1048576.375].into_iter().enumerate() {
        pigment[(i / 2) * 256 + i % 2][0] = value;
    }
    let mut appearance = pigment.clone();
    appearance[0][0] += 0.125;
    let expected = [0, 1, 256, 257].into_iter().map(|i|
        f64::from(appearance[i][0]) - f64::from(pigment[i][0])).sum::<f64>() / 4.;
    let bytes = |values: &[[f32; 4]]| values.iter().flatten().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    crate::test_support::upload_page(&r, &base, &bytes(&pigment));
    crate::test_support::upload_page(&r, &source, &bytes(&appearance));
    let binding = Commands::binding(&r, &source.create_view(&Default::default()),
        &base.create_view(&Default::default()), &output.create_view(&Default::default()));
    let mut commands = Commands::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let mut values = [0; 20];
    values[..8].copy_from_slice(&[0, 0, 1, 1, 256, 256, 2, 128]);
    commands.reduce(&mut r, &mut encoder, values, &binding, "HDR material correction oracle").unwrap();
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    let actual = pixels(&r, &output)[0][0];
    assert!((f64::from(actual) - expected).abs() < 1e-6, "HDR correction {actual}, expected {expected}");
}

#[test]
fn idle_display_converges_to_exact_composition_after_edits() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document();
        let extent = doc.composition().size;
        paint_mut(&mut doc,0).original = Some(layer_core::color::source::rgba8_source(extent, |x, y|
            [if (x / 3 + y / 2) % 2 == 0 { 40 } else { 220 }, 128, 70, 255]));
        let top = paint_occurrence(&mut doc, "correlated coverage", Some(layer_core::color::source::rgba8_source(extent, |x, y|
            [180, 20, 100, if (x / 3 + y / 2) % 2 == 0 { 40 } else { 220 }])));
        insert_occurrence(&mut doc, top, 0);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        for (step, opacity) in [0.8, 0.3, 0.9].into_iter().enumerate() {
            occurrence_mut(&mut doc,0).opacity = opacity;
            let mut frame = packet(doc.scene(), extent);
            frame.blend_space = space;
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            r.submit(frame).unwrap();
            exact.submit(frame).unwrap();
            let reference = pixels(&exact, crate::test_support::document_texture(&exact));
            let before = quality(&display_pixels(&r), &reference, r.scale_display.as_ref().unwrap().plan);
            assert!(before[2] > 1e-4, "fixture must need refinement: {before:?}");
            if step == 0 {
                r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
                assert!(r.has_pending_work(), "the next edit interrupts partial refinement");
                continue;
            }
            assert_settled(&mut r, frame, &reference);
        }
    }
}

#[test]
fn blend_space_changes_refresh_branches_and_source_representations() {
    use layer_core::{Affine, BlendSpace};
    let mut doc = document();
    let extent = doc.composition().size;
    let entries=(0..12).map(|i| { let handle=copy_paint(&mut doc,0); doc.artwork.occurrences.get_mut(handle).unwrap().opacity=0.15+i as f32*0.04; handle }).collect();
    set_root_entries(&mut doc,entries);
    occurrence_mut(&mut doc,0).blend = layer_core::LayerBlend::SoftLight;
    let color = layer_core::color::RgbColor::from_linear(doc.composition().color.space, [0.17, 0.39, 0.81, 0.7]).unwrap();
    add_fill(&mut doc, color);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut fresh = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let shifted = Affine([1., 0., 0., 1., 8., -4.]);
    for (step, (space, placement, level)) in [
        (BlendSpace::Linear, Affine::IDENTITY, 3),
        (BlendSpace::Perceptual, Affine::IDENTITY, 3),
        (BlendSpace::Linear, Affine::IDENTITY, 3),
        (BlendSpace::Perceptual, shifted, 3),
        (BlendSpace::Perceptual, Affine::IDENTITY, 3),
        (BlendSpace::Perceptual, Affine::IDENTITY, 0),
        (BlendSpace::Linear, Affine::IDENTITY, 0),
    ].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(placement);
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
        frame.composite_all = matches!(step, 3 | 4);
        let scale = 1. / (1 << level) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(frame).unwrap();
        fresh.submit(FramePacket { reset_layers: true, ..frame }).unwrap();
        exact.submit(FramePacket { composite_all: true, ..frame }).unwrap();
        let actual = display_pixels(&r);
        let expected = display_pixels(&fresh);
        let error = crate::test_support::max_error(&actual, &expected);
        assert!(error < 2e-5, "step={step} {space:?} {placement:?} level={level}: {error}");
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.graph.storage_bytes() > 0);
        let sources = &r.scene.as_ref().unwrap().scale_sources;
        let encoding = if placement == Affine::IDENTITY { space } else { BlendSpace::Linear };
        assert_eq!(sources.entries[&source_at(&doc,0)].blend_space, encoding);
        if encoding == BlendSpace::Perceptual {
            assert!(sources.complete_texture(doc.scene(), source_at(&doc,0), extent, level).is_none());
        }
        if level == 0 {
            let reference = pixels(&exact, crate::test_support::document_texture(&exact));
            let error = crate::test_support::max_error(&actual, &reference);
            assert!(error < 2e-5, "native {space:?}: {error}");
        }
        let actual = r.readback_srgb_rgba8().unwrap();
        let expected = exact.readback_srgb_rgba8().unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn clipped_contacts_do_not_require_unmaterialized_source_levels() {
    let doc = Document::new(PortableId::random(), 517, 259, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let expected = display_pixels(&r);
    let dab = crate::tests::test_dab([10000., 10000.], [0.8, 0.2, 0.1, 1.], 1.);
    let batch = dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    for kind in [DabBatchKind::Persistent, DabBatchKind::Preview] {
        let batch = DabBatch { kind, ..batch.clone() };
        r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        assert_eq!(display_pixels(&r), expected);
        assert!(r.paint_layers.iter().all(|layer| layer.pages.is_empty()));
        assert!(r.preview_pages.is_empty());

    }
}

#[test]
fn native_graph_admits_a_full_4k_view_with_bounded_scratch() {
    let mut doc = document_at([65, 33]);
    composition_mut(&mut doc).size[0] = 4096;
    composition_mut(&mut doc).size[1] = 4096;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    for count in [1, 32] {
        while doc.scene().order().len() <= count {
            let layer = copy_paint(&mut doc,0);
            doc.artwork.occurrences.get_mut(layer).unwrap().opacity = 0.35;
            insert_occurrence(&mut doc, layer, 0);
        }
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.view.width_px = doc.composition().size[0];
        frame.view.height_px = doc.composition().size[1];
        assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(0));
        r.submit(frame).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let scene = r.scene.as_ref().unwrap();
        assert_eq!(cache.plan.level, 0);
        assert!(cache.graph.root.is_some());
        assert!(scene.scale_sources.entries.values().all(|s| !s.levels.contains_key(&0)));
        let bytes = resident_bytes_with_pool(cache, scene);
        assert!(bytes <= crate::scene::scale::CACHE_BYTES, "layers={count}, bytes={bytes}");
        assert!(scene.used.iter().all(|used| !used));
    }
}

#[test]
fn reduced_photo_stacks_use_bounded_working_tiles() {
    let doc = reduced_photo_stack();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.width_px = 1500;
    frame.view.height_px = 1000;
    frame.view.document_to_surface = [0.1578, 0., 0., 0.1578, 0., 0.];
    assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(2));
    r.submit(frame).unwrap();
    let cache = r.scale_display.as_ref().unwrap();
    let scene = r.scene.as_ref().unwrap();
    assert!(cache.output.is_empty() && cache.pixels.root().is_some(), "working images stay tile-sized");
    assert!(scene.pool.iter().all(|p| p.texture.width() == PAGE_SIZE && p.texture.height() == PAGE_SIZE));
    let bytes = resident_bytes_with_pool(cache, scene);
    assert!(bytes <= crate::scene::scale::CACHE_BYTES, "resident bytes={bytes}");

}

#[test]
fn partial_reduced_views_admit_the_visible_window() {
    let mut doc = document_at([65, 33]);
    composition_mut(&mut doc).size[0] = 9504;
    composition_mut(&mut doc).size[1] = 6336;
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    for detail in [1, 2, 4] {
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.view.width_px = 960;
        frame.view.height_px = 640;
        let scale = 1. / (1 << detail) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, -1600. * scale, -900. * scale];
        assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(detail));
        let plan = view_plan(frame, detail, Evaluation::Display).unwrap();
        assert!(plan.bounds.area() < PixelRect::full(plan.extent).area());
        assert!(allocation(&r, plan, frame, None).into_iter().sum::<u64>() <= crate::scene::scale::CACHE_BYTES);
    }
}

#[test]
fn rotated_reduced_stacks_admit_bounded_source_working_storage() {
    let doc = reduced_photo_stack();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.width_px = 1600; frame.view.height_px = 1000;
    let (s, c) = std::f32::consts::FRAC_PI_4.sin_cos();
    let scale = 0.307;
    frame.view.document_to_surface = [scale * c, scale * s, -scale * s, scale * c,
        800. - scale * (c * doc.composition().size[0] as f32 - s * doc.composition().size[1] as f32) * 0.5,
        500. - scale * (s * doc.composition().size[0] as f32 + c * doc.composition().size[1] as f32) * 0.5];
    assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(1));
    r.submit(frame).unwrap();
    let cache = r.scale_display.as_ref().unwrap();
    let scene = r.scene.as_ref().unwrap();
    let bytes = resident_bytes_with_pool(cache, scene);
    assert!(bytes <= crate::scene::scale::CACHE_BYTES, "resident bytes={bytes}");

    assert!(scene.used.iter().all(|used| !used));
}

#[test]
fn streamed_sources_match_cached_pixels_through_masks_paint_and_admission_changes() {
    let mut doc = document_at([2053, 1541]);
    let extent = doc.composition().size;
    for _ in 0..63 {
        let layer = copy_paint(&mut doc,0);
        doc.artwork.occurrences.get_mut(layer).unwrap().opacity = 0.35;
        doc.artwork.occurrences.get_mut(layer).unwrap().blend = layer_core::LayerBlend::SoftLight;
        insert_occurrence(&mut doc, layer, 0);
    }
    let owner=doc.scene().order()[0];
    coverage_mask(&mut doc,owner, Default::default(), Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 280., y: 210. }, layer_core::Point { x: 1600., y: 260. },
        layer_core::Point { x: 1700., y: 1200. }, layer_core::Point { x: 330., y: 1100. },
    ]).unwrap()));
    let mut streamed = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut cached = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    for state in 0..6 {
        for i in 0..doc.scene().order().len() { occurrence_mut(&mut doc,i).visible = state != 3 || i < 2; }
        occurrence_mut(&mut doc,0).mask.as_mut().unwrap().inverted = state == 2;
        let dab = crate::tests::test_dab([420. + state as f32 * 7., 310.], [0.8, 0.2, 0.1, 1.], 0.7);
        let mut batch = dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        if state < 4 { batch.kind = DabBatchKind::Preview; }
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = (2..=4).contains(&state);
        frame.view.width_px = extent[0]; frame.view.height_px = extent[1];
        frame.view.document_to_surface = [0.5, 0., 0., 0.5, 0., 0.];
        if state == 1 || state >= 4 {
            frame.dabs = std::slice::from_ref(&dab);
            frame.dab_batches = std::slice::from_ref(&batch);
        }
        streamed.submit(frame).unwrap();
        let full = streamed.scale_display.as_ref().unwrap();
        assert_eq!(full.streamed_sources, state != 3, "state {state}");
        let full_plan = full.plan;
        let complete = display_pixels(&streamed);
        frame.view.width_px = 192; frame.view.height_px = 128;
        let [x, y] = if state == 2 { [-910., -660.] } else { [-160., -110.] };
        frame.view.document_to_surface = [0.5, 0., 0., 0.5, x, y];
        cached.submit(frame).unwrap();
        let window = cached.scale_display.as_ref().unwrap();
        assert!(!window.streamed_sources, "cached oracle state {state}");
        let actual = display_pixels(&cached);
        let origin = [window.plan.bounds.min_x() >> 1, window.plan.bounds.min_y() >> 1];
        for (i, pixel) in actual.iter().enumerate() {
            let x = origin[0] + i as u32 % window.plan.size[0];
            let y = origin[1] + i as u32 / window.plan.size[0];
            let expected = complete[(y * full_plan.size[0] + x) as usize];
            assert!(pixel.iter().zip(expected).all(|(a, b)| (a - b).abs() < 2e-5),
                "state {state}, [{x}, {y}]: {pixel:?} != {expected:?}");
        }
        let scene = streamed.scene.as_ref().unwrap();
        let bytes = resident_bytes_with_pool(full, scene);
        assert!(bytes <= crate::scene::scale::CACHE_BYTES, "state {state}, bytes {bytes}");
        assert!(scene.used.iter().all(|used| !used));

        assert_presentation_mip(&streamed);
    }
}

#[test]
fn streamed_sources_reuse_valid_finer_pages_without_native_decoding() {
    let doc = document_at([1027, 773]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let expected = display_pixels(&r);
    let mut cache = r.scale_display.take().unwrap();
    let mut scene = r.scene.take().unwrap();
    let mut commands = scene.scale_commands.take().unwrap();
    let source = scene.scale_sources.entries.get_mut(&source_at(&doc,0)).unwrap();
    assert!(source.levels.contains_key(&2));
    source.levels.remove(&3);
    cache.streamed_sources = true;
    for missing in [None, Some([4, 3])] {
        let source = scene.scale_sources.entries.get_mut(&source_at(&doc,0)).unwrap();
        if let Some(tile) = missing { assert!(source.levels.get_mut(&2).unwrap().valid.remove(&tile)); }
        let updates = source.updates;
        cache.graph = Default::default();
        cache.valid.clear();
        scene.begin_frame();
        commands.begin();
        cache.prepare_graph(&r, frame, &scene, &commands, None).unwrap();
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        cache.render(&mut scene, &mut r, frame, PixelRect::full(frame.document_extent),
            &mut Encoding { encoder: &mut encoder, commands: &mut commands }, None).unwrap();
        commands.flush(&mut r, &mut encoder).unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        assert_eq!(scene.scale_sources.entries[&source_at(&doc,0)].updates, updates + u64::from(missing.is_some()),
            "only gaps in retained finer pages may require native reduction");
        let actual = pixels(&r, cache.texture());
        assert!(actual.iter().zip(&expected).all(|(a, b)| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-5)));
    }
    scene.scale_commands = Some(commands);
    r.scene = Some(scene);
    r.scale_display = Some(cache);
    assert_presentation_mip(&r);
}

#[test]
fn source_window_growth_remains_admitted_during_rotated_pans() {
    let doc = reduced_photo_stack();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    for step in 0..24 {
        let angle = step as f32 / 23. * std::f32::consts::TAU;
        let (s, c) = angle.sin_cos();
        let [x, y] = [(0.5 + 0.4 * c) * doc.composition().size[0] as f32, (0.5 + 0.4 * s) * doc.composition().size[1] as f32];
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.view.width_px = 1600; frame.view.height_px = 1000;
        frame.view.document_to_surface = [0.5 * c, 0.5 * s, -0.5 * s, 0.5 * c,
            800. - 0.5 * (c * x - s * y), 500. - 0.5 * (s * x + c * y)];
        assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(1), "step {step}");
        r.submit(frame).unwrap();
        assert!(r.scale_display.is_some());
        let cache = r.scale_display.as_ref().unwrap();
        let scene = r.scene.as_ref().unwrap();
        let bytes = resident_bytes_with_pool(cache, scene);
        assert!(bytes <= crate::scene::scale::CACHE_BYTES, "step {step}: {bytes}");
    }
}

#[test]
fn retained_windows_stream_sources_when_source_requirements_grow() {
    let mut doc = document_at([65, 33]);
    composition_mut(&mut doc).size[0] = 3584; composition_mut(&mut doc).size[1] = 3584;
    paint_mut(&mut doc,0).domain = [3584;2];
    for _ in 0..15 {
        let layer = copy_paint(&mut doc,0);
        doc.artwork.occurrences.get_mut(layer).unwrap().visible = false;
        let position=doc.scene().order().len(); insert_occurrence(&mut doc, layer, position);
    }
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut full = packet(doc.scene(), doc.composition().size);
    full.view.width_px = doc.composition().size[0];
    full.view.height_px = doc.composition().size[1];
    full.view.document_to_surface = [0.5, 0., 0., 0.5, 0., 0.];
    let previous = view_plan(full, 1, Evaluation::Display).unwrap();
    assert_eq!(previous.bounds, PixelRect::full(previous.extent));
    assert_eq!(request(&r, full).ok().map(|q| q.plan.level), Some(1));
    let old = Cache::new(&r, Request { plan: previous, evaluation: Evaluation::Display }, doc.scene().order().len());
    for i in 0..doc.scene().order().len() { occurrence_mut(&mut doc,i).visible = true; }
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.width_px = 192;
    frame.view.height_px = 128;
    frame.view.document_to_surface = [0.5, 0., 0., 0.5, 0., 0.];
    assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(1));
    assert!(allocation(&r, previous, frame, None).into_iter().sum::<u64>() > crate::scene::scale::CACHE_BYTES);
    let (selected, _) = Cache::select(Some(old), &r, frame, request(&r, frame).unwrap(), false);
    assert_eq!(selected.plan, previous);
    assert!(selected.streamed_sources);
    assert!(allocation_for(&r, selected.plan, frame, None, true, None).into_iter().sum::<u64>() <= crate::scene::scale::CACHE_BYTES);
}

#[test]
fn view_windows_reuse_overlap_and_preserve_global_sampling() {
    let doc = document_at([2053, 1541]);
    let extent = doc.composition().size;
    for detail in [0, 1, 2, 4] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut whole = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;

        exact.set_complete_display_allowance(256 << 20);
        let mut frame = packet(doc.scene(), extent);
        exact.submit(frame).unwrap();
        let oracle = pixels(&exact, crate::test_support::document_texture(&exact));
        frame.composite_all = false;
        frame.view.width_px = 192;
        frame.view.height_px = 128;
        let mut presenter = crate::test_support::float_presenter(&r);
        let mut reference_presenter = crate::test_support::float_presenter(&whole);
        let surface = |renderer: &WgpuRasterizer, presenter: &mut crate::present::ViewportPresenter, view| {
            let (texture, target) = create_color_target(&renderer.device, [192, 128], "composition viewport oracle");
            presenter.present(renderer, &target, view, [0.2; 4]).unwrap();
            pixels(renderer, &texture)
        };
        let views = [
            [1., 0., 0., 1., -300., -200.],
            [1., 0., 0., 1., -310., -210.],
            [1., 0., 0., 1., -750., -410.],
            [0., 1., -1., 0., 900., -510.],
            [-1.5, 0., 0., 0.75, 1700., -300.],
            [1., 0., 0., 1., -1900., -1430.],
        ];
        let mut work = 0;
        let mut revision = None;
        for (i, transform) in views.into_iter().enumerate() {
            let transform = transform.map(|n| n / (1 << detail) as f32);
            frame.view.document_to_surface = transform;
            r.submit(frame).unwrap();
            let current = r.canvas_preview_revision();
            assert!(revision.is_none_or(|previous| previous == current), "camera motion preserves artwork revision");
            revision = Some(current);
            let cache = r.scale_display.as_ref().unwrap();
            assert_eq!(cache.plan.level, display_mips::view_level(transform, 4).unwrap());
            assert!(cache.graph.root.is_some());
            if detail == 0 { assert!(cache.plan.bounds.area() < PixelRect::full(extent).area() / 2); }
            if let Some(overview) = &cache.overview { assert!(overview.plan.level > cache.plan.level); }

            assert!(quality(&display_pixels(&r), &oracle, cache.plan)[2] < 2e-5, "view={i}");
            assert_presentation_mip(&r);
            if i == 1 { assert_eq!(r.metrics.composited_pixels, work); }
            if i == 2 { assert!(r.metrics.composited_pixels - work < cache.plan.size.map(u64::from).into_iter().product()); }
            work = r.metrics.composited_pixels;
            let actual = surface(&r, &mut presenter, frame.view);
            let scale = 1. / (1 << cache.plan.level) as f32;
            let mut full = packet(doc.scene(), extent);
            full.view.width_px = extent[0];
            full.view.height_px = extent[1];
            full.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            whole.submit(full).unwrap();
            let complete = whole.scale_display.as_ref().unwrap();
            assert_eq!(complete.plan.bounds, PixelRect::full(extent));
            assert_eq!(complete.plan.level, cache.plan.level);
            assert!(quality(&display_pixels(&whole), &oracle, complete.plan)[2] < 2e-5);
            let reference = surface(&whole, &mut reference_presenter, frame.view);
            let error = crate::test_support::max_error(&actual, &reference);
            assert!(error < 0.0001, "window presentation detail={detail} view={i} plan={:?} error={error}", cache.plan);
        }
        let dab = crate::tests::test_dab([1980., 1480.], [0.9, 0.2, 0.1, 1.], 0.7);
        let batch = dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        frame.dabs = std::slice::from_ref(&dab);
        frame.dab_batches = std::slice::from_ref(&batch);
        r.submit(frame).unwrap();
        assert!(r.canvas_preview_revision() > revision.unwrap(), "painting advances artwork revision");
        exact.submit(frame).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let oracle = pixels(&exact, crate::test_support::document_texture(&exact));
        assert!(quality(&display_pixels(&r), &oracle, cache.plan)[2] < 2e-5);
        if let Some(overview) = &cache.overview {
            assert!(quality(&pixels(&r, overview.texture()), &oracle, overview.plan)[2] < 2e-5);
        }
        assert_presentation_mip(&r);
    }
}

#[test]
fn small_placed_source_remains_visible_beyond_its_local_extent() {
    let mut doc = document_at([256, 256]);
    let extent = [2048, 1536];
    composition_mut(&mut doc).size[0] = extent[0]; composition_mut(&mut doc).size[1] = extent[1];
    occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([1., 0., 0., 1., 896., 640.]));
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.385, 0., 0., 0.385, 0., 0.];
    r.submit(frame).unwrap();
    let display = display_pixels(&r);
    assert!(display.iter().any(|p| p[3] > 0.99 && p[2] > 0.05), "placed source contributes to presentation");
    let exact = r.readback_srgb_rgba8().unwrap();
    let center = ((768 * extent[0] + 1024) * 4) as usize;
    assert_eq!(&exact[center..center + 4], &[42, 64, 80, 255]);
}

#[test]
fn placed_sources_and_masks_compose_in_document_scale_and_keep_exact_queries() {
    let source_extent = [517, 259];
    let extent = [389, 277];
    let mut doc = document_at(source_extent);
    composition_mut(&mut doc).size[0] = extent[0];
    composition_mut(&mut doc).size[1] = extent[1];
    occurrence_mut(&mut doc,0).opacity = 0.71;
    let owner=doc.scene().order()[0];
    let mask_source=coverage_mask(&mut doc,owner, layer_core::Point { x: 17., y: -11. }, Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 30., y: 10. }, layer_core::Point { x: 390., y: 10. },
        layer_core::Point { x: 390., y: 200. }, layer_core::Point { x: 30., y: 200. },
    ]).unwrap()));
    doc.artwork.coverage.get_mut(mask_source).unwrap().default_coverage=0.63;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for (pose, placement) in [
        [0.5, 0., 0., 0.5, 16., 32.], [-0.5, 0., 0., 0.5, 320., 32.],
        [0.6, 0.2, -0.1, 0.5, 30., 4.], [0.35, 0.1, 0.2, 0.75, -17., 21.],
    ].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine(placement));
        occurrence_mut(&mut doc,0).mask.as_mut().unwrap().inverted = pose % 2 != 0;
        for enabled in [true, false] {
            occurrence_mut(&mut doc,0).mask.as_mut().unwrap().enabled = enabled;
            for level in [0, 1, 2, 3] {
                let scale = 1. / (1 << level) as f32;
                let mut frame = packet(doc.scene(), extent);
                frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
                r.submit(frame).unwrap();
                exact.submit(frame).unwrap();
                let cache = r.scale_display.as_ref().unwrap();
                assert_eq!(cache.plan.level, level);

                let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
                assert!(error[0] < 0.004 && error[1] < 0.04, "pose={pose} level={level}: {error:?}");
                assert_presentation_mip(&r);
                let actual = r.readback_srgb_rgba8().unwrap();
                let expected = exact.readback_srgb_rgba8().unwrap();
                assert!(actual == expected, "exact placed export pose={pose} level={level} mask={enabled}");
            }
        }
    }
}

#[test]
fn source_windows_keep_overlap_and_sample_global_coordinates() {
    let source_extent = [4099, 2053];
    let mut doc = document_at(source_extent);
    let extent = [513, 257];
    composition_mut(&mut doc).size[0] = extent[0]; composition_mut(&mut doc).size[1] = extent[1];
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut updates = 0;
    let mut previous = None;
    for (step, offset) in [[-2050., -1000.], [-2100., -1000.], [-2350., -1000.], [-2100., -1000.], [-3575., -1790.], [700., 400.]].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine::translation(layer_core::Point { x: offset[0], y: offset[1] }));
        let mut frame = packet(doc.scene(), extent);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        let cache = r.scale_display.as_ref().unwrap();

        let sources = &r.scene.as_ref().unwrap().scale_sources;
        assert!(sources.storage_bytes() < 8 << 20, "small output retains bounded source windows: {}", sources.storage_bytes());
        let source = &sources.entries[&source_at(&doc,0)];
        let images: Vec<_> = source.levels.values().map(|l| l.image.texture.clone()).collect();
        if step == 1 || step == 3 {
            assert_eq!(source.updates, updates, "motion inside the retained window reuses source pixels");
            assert_eq!(previous.as_ref().unwrap(), &images, "covered motion retains the same textures");
        }
        previous = Some(images);
        updates = source.updates;
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
        assert!(error[0] < 0.004 && error[1] < 0.04, "step={step}: {error:?}");
        assert_presentation_mip(&r);
    }
    occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([0.5, 0., 0., 0.5, -100., -100.]));
    let frame = packet(doc.scene(), extent);
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
    assert!(error[0] < 0.004 && error[1] < 0.04, "native placement after a partial source: {error:?}");
}

#[test]
fn cold_identity_sources_prepare_adjacent_detail_without_redecoding() {
    let doc = document_at([1025, 769]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.width_px = 512; frame.view.height_px = 512;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let source = &r.scene.as_ref().unwrap().scale_sources.entries[&source_at(&doc,0)];
    let updates = source.updates;
    assert_eq!(updates, 20);
    assert!(source.levels.contains_key(&2) && source.levels.contains_key(&3));
    let finer = source.levels[&2].image.texture.clone();
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    r.submit(frame).unwrap();
    let source = &r.scene.as_ref().unwrap().scale_sources.entries[&source_at(&doc,0)];
    assert_eq!(source.updates, updates);
    assert_eq!(source.levels[&2].image.texture, finer);
    assert!(!source.levels.contains_key(&1), "scale changes must not repeatedly promote optional detail");
}

#[test]
fn optional_source_detail_yields_to_unallocated_required_images() {
    let mut doc = document_at([1024, 1024]);
    composition_mut(&mut doc).size[0] = 128; composition_mut(&mut doc).size[1] = 128;
    occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([0.125, 0., 0., 0.125, 0., 0.]));
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.submit(packet(doc.scene(), doc.composition().size)).unwrap();
    let mut scene = r.scene.take().unwrap();
    let source = &scene.scale_sources.entries[&source_at(&doc,0)];
    assert_eq!(source.updates, 16, "cold detail and its required level decode each native page once");
    assert!(source.levels.contains_key(&1) && source.levels.contains_key(&2));
    let second = copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(second).unwrap().opacity = 0.5;
    insert_occurrence(&mut doc, second, 0);
    let frame = packet(doc.scene(), doc.composition().size);
    scene.scale_sources.prepare(&r, frame, &[]);
    let requested = r.scale_display.as_ref().unwrap().source_levels(&r, frame, &scene);
    let budget = requested.values().flat_map(|levels| levels.values()).map(|p| p.level_bytes(p.level)).sum();
    assert_eq!(scene.scale_sources.retain_levels(&requested, budget), budget);
    assert!(scene.scale_sources.entries.values().all(|s| !s.levels.contains_key(&1)));
    let mut commands = Commands::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for &handle in doc.scene().order() {
        let OccurrenceContent::Paint(paint)=doc.artwork.occurrences.get(handle).unwrap().content else {continue}; let target=SourceTarget::Paint(paint);
        if let Some(levels) = requested.get(&target) {
            for &plan in levels.values() {
                scene.prepare_scale_color(&mut commands, &mut r, frame, &mut encoder, handle,
                    SourceRequest { plan, required: plan.bounds.into(), covered: PixelRect::EMPTY }).unwrap();
            }
        }
    }
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    assert_eq!(scene.scale_sources.storage_bytes(), budget);
    assert!(requested.iter().all(|(id, levels)| levels.keys().all(|level|
        scene.scale_sources.entries[id].levels.contains_key(level))));
}

#[test]
fn source_retention_reserves_images_that_composition_allocates_later() {
    for material in [false, true] {
        let mut doc = document_at([33, 17]);
        composition_mut(&mut doc).size[0] = 4096; composition_mut(&mut doc).size[1] = 4096;
        if material {
            use layer_core::raster::*;
            let plane = RasterPlane::WatercolorWetness;
            let tile = RasterTile::backed(TileBlob::encode(plane.descriptor(doc.composition().color), &vec![255; 256 * 256]).unwrap());
            let mut data = RasterData { watercolor: Some(RasterWatercolor {
                wet_edge: 0.9, burnt_edge: 0.6, edge_width: 8.,
            }), ..Default::default() };
            for y in 0..16 {
                for x in 0..16 {
                    data.tiles.insert(TileKey { plane, coordinate: [x, y] }, tile.clone());
                }
            }
            paint_mut(&mut doc,0).domain=doc.composition().size;
            paint_mut(&mut doc,0).raster = RasterRevision::backed(data);
            occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine::translation(layer_core::Point { x: 32., y: 32. }));
        }
        let front = copy_paint(&mut doc,0);
        doc.artwork.occurrences.get_mut(front).unwrap().opacity = 0.5;
        insert_occurrence(&mut doc, front, 0);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        if material {
            r.submit(frame).unwrap();
            r.wait_idle().unwrap();
            r.scale_display = None;
            r.scene = None;
            frame.reset_layers = false;
        }
        let cache = Cache::new(&r, Request { plan: display_mips::Plan::at(frame.document_extent, 2), evaluation: Evaluation::Display }, doc.scene().order().len());
        assert!(cache.output.is_empty() && cache.pixels.next().is_none());
        let source_budget = cache.source_budget(&r, frame, &Commands::new(&r), None);
        r.scale_display = Some(cache);
        r.submit(frame).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.pixels.root().is_some() && cache.pixels.next().is_some());
        let commands = r.scene.as_ref().unwrap().scale_commands.as_ref().unwrap();
        let scene = r.scene.as_ref().unwrap();
        let material_bytes = scene.pool.iter().map(PageSurface::storage_bytes).sum::<u64>();
        let total = source_budget + cache.storage_bytes() + commands.storage_bytes() + material_bytes;
        eprintln!("material={material}: prospective_source={source_budget}, later_display={}, later_material={material_bytes}, total={total}", cache.storage_bytes());
        assert!(total <= crate::scene::scale::CACHE_BYTES, "prospective sources and later outputs/material must stay admitted: {total}");
    }
}

#[test]
fn source_windows_derive_across_origins_and_fill_only_missing_pages() {
    let doc = document_at([1027, 773]);
    let extent = doc.composition().size;
    let id = source_at(&doc,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    r.submit(frame).unwrap();
    let reference = pixels(&r, r.scale_display.as_ref().unwrap().texture());
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let mut scene = r.scene.take().unwrap();
    let full = PixelRect::full(extent);
    let window = PixelRect::new(256, 256, extent[0], extent[1]);
    for (fine, coarse) in [(full, window), (window, full)] {
        let mut commands = Commands::new(&r);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        scene.prepare_scale_color(&mut commands, &mut r, frame, &mut encoder, doc.scene().order()[0], SourceRequest {
            plan: display_mips::Plan::window(extent, 1, fine), required: fine.into(), covered: PixelRect::EMPTY,
        }).unwrap();
        scene.scale_sources.entries.get_mut(&id).unwrap().levels.remove(&2);
        let plan = display_mips::Plan::window(extent, 2, coarse);
        let updates = scene.scale_sources.entries[&id].updates;
        scene.scale_sources.ensure_level(&mut commands, &mut r, &mut encoder, id, plan).unwrap();
        let complete = scene.scale_sources.image(id, 2).valid.len();
        assert_eq!(complete, page_coordinates(fine.intersect(coarse)).count());
        assert_eq!(scene.scale_sources.entries[&id].updates, updates);
        scene.prepare_scale_color(&mut commands, &mut r, frame, &mut encoder, doc.scene().order()[0], SourceRequest {
            plan, required: coarse.into(), covered: PixelRect::EMPTY,
        }).unwrap();
        assert_eq!(scene.scale_sources.entries[&id].updates - updates, (page_coordinates(coarse).count() - complete) as u64);
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        let actual = pixels(&r, &scene.scale_sources.image(id, 2).image.texture);
        assert!(quality(&actual, &reference, plan)[2] < 1e-6);
        assert_eq!(scene.scale_sources.complete_texture(doc.scene(), source_at(&doc,0), extent, 2).is_some(), coarse == full);
    }
}

#[test]
fn placement_crossing_identity_keeps_the_prepared_source() {
    let extent = [1024, 512];
    let mut doc = document_at(extent);
    let id = source_at(&doc,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.prepare_moving_layer(Some(doc.scene().order()[0]));
    let mut prepared = None;
    let mut prepared_level = None;
    for (step, (x, scale)) in [(10., 1.), (0., 1.), (0., 0.9), (0., 0.9), (0., 1.), (-10., 1.), (0., 1.)].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([scale, 0., 0., scale, x, 0.]));
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = layer_core::BlendSpace::Perceptual;
        frame.time_seconds = step as f32 * 0.1;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        r.submit(frame).unwrap();
        let source = &r.scene.as_ref().unwrap().scale_sources.entries[&id];
        let level = *prepared_level.get_or_insert_with(|| source_level(r.scale_display.as_ref().unwrap().plan.level,
            &layer_core::target_geometry(doc.scene(),id),extent));
        let current = (source.updates, source.levels[&level].image.texture.clone());
        assert_eq!(prepared.get_or_insert_with(|| current.clone()), &current,
            "moving an unchanged photo through its original pose must reuse its pixels");
        assert!(!r.has_pending_work(), "an unfinished placement must not schedule exact refinement");
        let work = r.metrics.composited_pixels;
        frame.composite_all = false;
        r.submit(frame).unwrap();
        assert_eq!(r.metrics.composited_pixels, work, "an unchanged placement pose must not refine");
    }
    r.prepare_moving_layer(None);
    occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine::IDENTITY);
    let mut frame = packet(doc.scene(), extent);
    frame.blend_space = layer_core::BlendSpace::Perceptual;
    frame.composite_all = false;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    r.submit(frame).unwrap();
    assert!(r.has_pending_work(), "finishing placement must allow exact refinement");
    let mut fresh = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    fresh.submit(frame).unwrap();
    let actual = display_pixels(&r);
    let expected = display_pixels(&fresh);
    let describe = |r: &WgpuRasterizer| r.scene.as_ref().unwrap().scale_sources.entries[&id].levels.iter()
        .map(|(level, image)| (*level, r.scene.as_ref().unwrap().scale_sources.entries[&id].accepts(image), image.valid.len())).collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    let error = actual.iter().zip(&expected).flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
        .fold(0f32, f32::max);
    eprintln!("display reduction histories max={error}; cached {:?}; fresh {:?}", describe(&r), describe(&fresh));
    assert!(error < 1e-6, "cached versus fresh display reduction: {error}");
}

#[test]
fn deferred_placement_samples_the_final_surface_without_a_canvas_image() {
    let mut doc = document_at([1025, 513]);
    add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
    let extent = [641, 385];
    composition_mut(&mut doc).size[0] = extent[0]; composition_mut(&mut doc).size[1] = extent[1];
    occurrence_mut(&mut doc,0).opacity = 0.71;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut exact_presenter = crate::test_support::float_presenter(&exact);
    let mut presenter = crate::test_support::float_presenter(&r);
    let render = |r: &WgpuRasterizer, presenter: &mut crate::ViewportPresenter, view: layer_render::ViewState| {
        let (texture, target) = create_color_target(&r.device, [view.width_px, view.height_px], "deferred presentation oracle");
        presenter.present(r, &target, view, [1.; 4]).unwrap();
        pixels(r, &texture)
    };
    for placement in [[1., 0., 0., 1., -153.25, -51.5], [0.5, 0.1, -0.15, 0.6, 30., 5.], [-0.6, 0.1, 0.15, 0.5, 570., 7.]] {
        occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine(placement));
        for camera in [[0.25, 0., 0., 0.25, 8.25, 7.5], [0.19, 0., 0., 0.19, 8.25, 7.5], [0.13, 0., 0., 0.13, 8.25, 7.5],
            [0.17, 0.075, -0.075, 0.17, 37.5, 6.25], [0.14, -0.06, 0.02, 0.24, 8.25, 42.5]] {
            let mut frame = packet(doc.scene(), extent);
            frame.view.width_px = 192; frame.view.height_px = 128;
            frame.view.document_to_surface = camera;
            let inverse = layer_core::Affine(camera).inverse().unwrap();
            let inside = |x, y| {
                let p = inverse.map(layer_core::Point { x, y });
                p.x >= 0. && p.y >= 0. && p.x < extent[0] as f32 && p.y < extent[1] as f32
            };
            r.submit(frame).unwrap();
            exact.submit(frame).unwrap();
            assert!(r.scale_display.as_ref().unwrap().output.is_empty());
            assert!(r.scale_display.as_ref().unwrap().pixels.next().is_none());
            let actual = render(&r, &mut presenter, frame.view);
            let mut high = frame.view;
            high.width_px *= 8; high.height_px *= 8;
            high.document_to_surface = high.document_to_surface.map(|n| n * 8.);
            let reference = render(&exact, &mut exact_presenter, high);
            let point = render(&exact, &mut exact_presenter, frame.view);
            let expected: Vec<[f32; 4]> = (0..frame.view.height_px).flat_map(|y| (0..frame.view.width_px).map(move |x| (x, y))).map(|(x, y)| {
                if !inside(x as f32 + 0.5, y as f32 + 0.5) {
                    return point[(y * frame.view.width_px + x) as usize];
                }
                let mut value = [0.; 4];
                let mut count = 0.;
                for yy in y * 8..(y + 1) * 8 { for xx in x * 8..(x + 1) * 8 {
                    if !inside((xx as f32 + 0.5) / 8., (yy as f32 + 0.5) / 8.) { continue; }
                    for c in 0..4 { value[c] += reference[(yy * high.width_px + xx) as usize][c]; }
                    count += 1.;
                }}
                value.map(|c| c / count)
            }).collect();
            let mut errors: Vec<_> = actual.iter().zip(&expected).map(|(a,b)| a.iter().zip(b).map(|(a,b)| (a-b).abs()).fold(0., f32::max)).collect();
            errors.sort_by(f32::total_cmp);
            let mean = errors.iter().sum::<f32>() / errors.len() as f32;
            let p99 = errors[errors.len() * 99 / 100];
            assert!(mean < 0.004 && p99 < 0.04);
        }
    }
}

#[test]
fn deferred_placement_navigator_preserves_coarse_artwork() {
    let mut doc = document_at([1025, 513]);
    add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
    composition_mut(&mut doc).size[0] = 641; composition_mut(&mut doc).size[1] = 385;
    occurrence_mut(&mut doc,0).opacity = 0.71;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let overview = |r: &WgpuRasterizer, size: [u32; 2]| {
        let mut presenter = crate::ViewportPresenter::for_overview_surface(
            r, wgpu::TextureFormat::Rgba32Float, crate::SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        presenter.set_overviews(r, &[crate::OverviewPlacement {
            bounds: [0., 0., size[0] as f32, size[1] as f32], clip: None,
            work_area: [[-1000.; 2]; 4], outline_linear: [0.; 3], background_linear: [1.; 3], scale: 1., opacity: 1.,
        }]);
        let (texture, target) = create_color_target(&r.device, size, "placement navigator oracle");
        presenter.present_overviews(r, &target, size).unwrap();
        pixels(r, &texture)
    };
    for (step, placement) in [[1., 0., 0., 1., -153.25, -51.5], [0.5, 0.1, -0.15, 0.6, 30., 5.], [-0.6, 0.1, 0.15, 0.5, 570., 7.]].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine(placement));
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.time_seconds = step as f32 * 0.1;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 8.25, 7.5];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        assert!(r.scale_display.as_ref().unwrap().placed.is_some());
        for size in [[128, 77], [64, 39], [32, 19], [16, 10], [7, 4]] {
            let actual = overview(&r, size);
            let high = size.map(|n| n * 8);
            let reference = overview(&exact, high);
            let mut errors = Vec::new();
            for y in 0..size[1] { for x in 0..size[0] {
                let mut expected = [0.; 4];
                for yy in y * 8..(y + 1) * 8 { for xx in x * 8..(x + 1) * 8 {
                    for c in 0..4 { expected[c] += reference[(yy * high[0] + xx) as usize][c] / 64.; }
                }}
                errors.push(actual[(y * size[0] + x) as usize].iter().zip(expected).map(|(a,b)| (a-b).abs()).fold(0., f32::max));
            }}
            errors.sort_by(f32::total_cmp);
            let mean = errors.iter().sum::<f32>() / errors.len() as f32;
            let p99 = errors[errors.len() * 99 / 100];
            assert!(mean < 0.03 && p99 < 0.25, "navigator placement={placement:?} size={size:?}: mean={mean} p99={p99}");
        }
    }
}

#[test]
fn deriving_partial_sources_preserves_completed_texels_between_refreshed_regions() {
    let doc = document_at([769, 769]);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    let mut scene = r.scene.take().unwrap();
    let id = source_at(&doc,0);
    let expected = pixels(&r, &scene.scale_sources.image(id, 3).image.texture);
    let mut commands = Commands::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for tile in [[0, 0], [3, 3]] {
        scene.scale_sources.entries.get_mut(&id).unwrap().levels.get_mut(&3).unwrap().valid.remove(&tile);
        scene.prepare_scale_color(&mut commands, &mut r, frame, &mut encoder, doc.scene().order()[0],
            SourceRequest { plan: display_mips::Plan::at(extent, 1), required: page_rect(tile).into(), covered: PixelRect::EMPTY }).unwrap();
    }
    scene.scale_sources.ensure_level(&mut commands, &mut r, &mut encoder, id, display_mips::Plan::at(extent, 3)).unwrap();
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    let actual = pixels(&r, &scene.scale_sources.image(id, 3).image.texture);
    assert!(actual.iter().flatten().zip(expected.iter().flatten()).all(|(a,b)| (a-b).abs() < 1e-6));
    assert_eq!(scene.scale_sources.image(id, 3).valid.len(), 16);
}

#[test]
fn placed_compact_prediction_keeps_the_most_magnified_source_axis() {
    let mut doc = document();
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    for placement in [[2., 0., 0., 1., -300., -20.], [-2., 0., 0., 0.25, 600., 80.]] {
        occurrence_mut(&mut doc,0).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine(placement));
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(FramePacket { composite_all: true, ..frame }).unwrap();
        let baseline = display_pixels(&r);
        let mut dab = crate::tests::test_dab([200., 110.], [0.9, 0.02, 0.1, 1.], 1.);
        dab.radii = [32.; 2];
        let mut batch = dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        batch.kind = DabBatchKind::Preview;
        batch.style.brush_to_layer = occurrence_at(&doc,0).placement.as_affine().unwrap().inverse().unwrap();
        r.submit(FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame }).unwrap();
        assert_eq!(r.preview_level, 1);
        assert_ne!(display_pixels(&r), baseline);
        r.submit(frame).unwrap();
        let actual = display_pixels(&r);
        let changed: Vec<_> = actual.iter().zip(&baseline).enumerate().filter(|(_, (a,b))| a != b).collect();
        let error = crate::test_support::max_error(&actual, &baseline);
        assert!(changed.is_empty(), "pose={placement:?} differing={} max={error} first={:?}", changed.len(), changed.first());
    }
}

#[test]
fn groups_clipping_and_all_blends_share_exact_stack_semantics() {
    let extent = [33, 19];
    let mut doc = document_at(extent);
    let paper=*doc.scene().order().last().unwrap();
    let base=paint_occurrence(&mut doc,"solid",Some(rgba8_source(extent,|_,_|[50,170,80,117])));
    doc.artwork.occurrences.get_mut(base).unwrap().opacity=0.81;
    let clipped=paint_occurrence(&mut doc,"solid",Some(rgba8_source(extent,|_,_|[230,30,120,193])));
    let occurrence=doc.artwork.occurrences.get_mut(clipped).unwrap(); set_attachment(occurrence, true); occurrence.opacity=0.54;
    let clip_mask=coverage_mask(&mut doc,clipped,Default::default(),None);
    doc.artwork.coverage.get_mut(clip_mask).unwrap().default_coverage=0.42;
    doc.artwork.occurrences.get_mut(clipped).unwrap().mask.as_mut().unwrap().inverted=true;
    let inner=stack_occurrence(&mut doc,"inner",vec![clipped,base]);
    let occurrence=doc.artwork.occurrences.get_mut(inner).unwrap(); occurrence.opacity=0.71; occurrence.blend=layer_core::LayerBlend::Multiply;
    let outer=stack_occurrence(&mut doc,"outer",vec![inner]);
    doc.artwork.occurrences.get_mut(outer).unwrap().opacity=0.63;
    let group_mask=coverage_mask(&mut doc,outer,Default::default(),None);
    doc.artwork.coverage.get_mut(group_mask).unwrap().default_coverage=0.61;
    let behind=paint_occurrence(&mut doc,"solid",Some(rgba8_source(extent,|_,_|[170,210,70,230])));
    doc.artwork.occurrences.get_mut(behind).unwrap().opacity=0.79;
    set_root_entries(&mut doc,vec![outer,behind,paper]);
    let mut reduced = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for mode in layer_core::LayerBlend::ALL {
        occurrence_mut(&mut doc,0).blend = mode;
        occurrence_mut(&mut doc,2).blend = mode;
        for state in 0..4 {
            occurrence_mut(&mut doc,1).visible = state != 1;
            occurrence_mut(&mut doc,2).visible = state != 2;
            occurrence_mut(&mut doc,3).visible = state != 3;
            occurrence_mut(&mut doc,0).mask.as_mut().unwrap().enabled = state != 2;

            let mut frame = packet(doc.scene(), extent);
            frame.inspect_mask=(state==1).then_some(clipped);
            frame.composite_all = false;
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            reduced.submit(frame).unwrap();
            exact.submit(frame).unwrap();
            let cache = reduced.scale_display.as_ref().unwrap();
            let error = quality(&display_pixels(&reduced), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
            assert!(error[2] < 2e-5, "{mode:?} state={state}: {error:?}");
            assert_presentation_mip(&reduced);

            assert_settled(&mut reduced, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
    }
}

#[test]
fn cached_branches_recompose_logarithmic_work_and_preserve_untouched_regions() {
    let mut doc = document();
    let photo_source=paint_at(&doc,0).original.as_ref().unwrap().clone();
    let paper=*doc.scene().order().last().unwrap();
    let mut entries:Vec<_>=(0..32).map(|i| { let handle=copy_paint(&mut doc,0); doc.artwork.occurrences.get_mut(handle).unwrap().opacity=0.17+i as f32*0.02; handle }).collect();
    entries.push(paper);
    let empty=paint_occurrence(&mut doc,"empty paint target",None); entries.insert(15,empty); set_root_entries(&mut doc,entries);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    fn frame(scene: SceneView<'_>, extent: [u32; 2]) -> FramePacket<'_> {
        let mut p = packet(scene, extent);
        p.composite_all = false;
        p.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        p
    }
    r.submit(frame(doc.scene(), extent)).unwrap();
    exact.submit(frame(doc.scene(), extent)).unwrap();
    assert!(!r.scene.as_ref().unwrap().scale_sources.entries.contains_key(&source_at(&doc,15)));
    let record_bound = page_coordinates(PixelRect::full(extent)).count() as u32
        + doc.scene().order().len().next_power_of_two().ilog2() + 1;
    for (step, index) in [15, 0, 32, 15, 0].into_iter().enumerate() {
        let mut dab = crate::tests::test_dab([30. + step as f32 * 100., 97.], [0.9, 0.02, 0.1, 0.7], 1.);
        dab.radii = [21.; 2];
        let batch = dab_batch(source_at(&doc,index), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
            let batch = DabBatch { kind, ..batch.clone() };
            let p = FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame(doc.scene(), extent) };
            r.submit(p).unwrap();
            exact.submit(p).unwrap();
            let scene = r.scene.as_ref().unwrap();
            let records = scene.scale_commands.as_ref().unwrap().cursor;
            assert!(records <= record_bound, "32 photos, first paint on an empty layer and end edits must reuse the other branches: step={step} {kind:?}, {records} > {record_bound}");
            let cache = r.scale_display.as_ref().unwrap();
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
            assert!(error[0] < 0.003 && error[1] < 0.025, "step={step} kind={kind:?}: {error:?}");
            assert_presentation_mip(&r);
            assert!(resident_bytes(cache, scene) <= crate::scene::scale::CACHE_BYTES);
        }
    }
    for step in 0..4 {
        match step {
            0 => occurrence_mut(&mut doc,15).opacity = 0.91,
            1 => occurrence_mut(&mut doc,0).visible = false,
            2 => swap_entries(&mut doc,15,31),
            _ => paint_mut(&mut doc,15).original = Some(Arc::new((*photo_source).clone())),
        }
        r.submit(frame(doc.scene(), extent)).unwrap();
        exact.submit(frame(doc.scene(), extent)).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
        assert!(error[0] < 0.003 && error[1] < 0.025, "metadata step={step}: {error:?}");
    }
}

#[test]
fn layer_edits_reuse_balanced_branches_across_the_stack() {
    fn blends(node: &graph::Node, target: SourceTarget) -> Option<u32> {
        use graph::Expression;
        match node.as_ref() {
            Expression::Source { id, .. } => (*id == target).then_some(0),
            Expression::Opacity { input, .. } => blends(input, target),
            Expression::Combine { front, back, .. } => blends(front, target)
                .or_else(|| blends(back, target)).map(|n| n + 1),
            _ => None,
        }
    }
    let mut doc = document();
    let photo_source=paint_at(&doc,0).original.clone();
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for count in [2usize, 3, 4, 7, 8, 15, 16] {
        let mut entries:Vec<_>=(0..count).map(|i| { let handle=paint_occurrence(&mut doc,"photo",photo_source.clone()); doc.artwork.occurrences.get_mut(handle).unwrap().opacity=0.17+i as f32*0.02; handle }).collect();
        let empty=paint_occurrence(&mut doc,"paint",None); entries.insert(0,empty); set_root_entries(&mut doc,entries);
        let mut dab = crate::tests::test_dab([97., 97.], [0.9, 0.02, 0.1, 0.7], 1.);
        dab.radii = [21.; 2];
        let fill = add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
        for (space, paper) in layer_core::BlendSpace::ALL.into_iter().flat_map(|space| [0., 0.7].map(|paper| (space, paper))) {
            let color = layer_core::color::RgbColor::from_linear(doc.composition().color.space, [0.17, 0.39, 0.81, paper]).unwrap();
            set_effect_value(&mut doc, fill, "color", layer_core::EffectValue::Color(color));
            let mut frame = packet(doc.scene(), extent);
            frame.blend_space = space;
            frame.composite_all = false;
            frame.reset_layers = true;
            frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
            r.submit(frame).unwrap();
            exact.submit(frame).unwrap();
            frame.reset_layers = false;
            let bound = (count as u32 + 1 + u32::from(paper > 0.)).next_power_of_two().ilog2();
            for index in [0, count / 2, count] {
                let batch = dab_batch(source_at(&doc,index), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
                let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
                r.submit(stroke).unwrap();
                exact.submit(stroke).unwrap();
                let cache = r.scale_display.as_ref().unwrap();
                for index in 0..=count {
                    let target=source_at(&doc,index);
                    let work = blends(cache.graph.root.as_ref().unwrap(), target).unwrap();
                    assert!(work <= bound, "{count} static layers, {space:?}, paper={paper}, layer={:?}: {work} > {bound}", target);
                }
                let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
                assert!(error[0] < 0.003 && error[1] < 0.025, "{count} static layers, {space:?}, index={index}: {error:?}");
                assert_presentation_mip(&r);
            }
        }
    }
}

#[test]
fn identity_edits_recompose_only_damaged_pages() {
    let doc = document_at([1024, 768]);
    let dabs: Vec<_> = [[125., 125.], [893., 125.], [893., 637.], [125., 637.]].into_iter().map(|p| {
        let mut dab = crate::tests::test_dab(p, [1., 0., 0., 1.], 1.);
        dab.radii = [16.; 2];
        dab
    }).collect();
    let damage = dabs.iter().fold(layer_core::Rect::default(), |r, d| r.union(d.bounds()));
    let batch = DabBatch { dab_count: dabs.len() as u32,
        ..dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), damage) };
    for level in [0, 2] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.width_px = doc.composition().size[0];
        frame.view.height_px = doc.composition().size[1];
        let scale = 1. / (1 << level) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(frame).unwrap();
        frame.dabs = &dabs;
        frame.dab_batches = std::slice::from_ref(&batch);
        let work = r.metrics.composited_pixels;
        r.submit(frame).unwrap();
        assert_eq!(r.metrics.composited_pixels - work, 4 * u64::from(PAGE_SIZE >> level).pow(2));
        let incremental = display_pixels(&r);
        r.scale_display = None;
        r.submit(FramePacket { dabs: &[], dab_batches: &[], composite_all: true, ..frame }).unwrap();
        assert_eq!(display_pixels(&r), incremental);
    }
}

#[test]
fn placed_page_edge_edits_match_rebuilding_the_entire_display() {
    let mut doc = document_at([513, 513]);
    let moving = copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(moving).unwrap().opacity = 0.71;
    doc.artwork.occurrences.get_mut(moving).unwrap().placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([0.7, 0.7, -0.7, 0.7, 76.8, 77.8]));
    let front = copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(front).unwrap().opacity = 0.3;
    insert_occurrence(&mut doc, moving, 0);
    insert_occurrence(&mut doc, front, 0);
    let extent = doc.composition().size;
    let mut incremental = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut rebuilt = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    incremental.submit(frame).unwrap();
    rebuilt.submit(frame).unwrap();
    let original = display_pixels(&incremental);
    let mut dab = crate::tests::test_dab([254.5, 0.5], [1., 0., 0., 1.], 1.);
    dab.radii = [0.45; 2];
    let batch = dab_batch(source_at(&doc,1), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    frame.dabs = std::slice::from_ref(&dab);
    frame.dab_batches = std::slice::from_ref(&batch);
    rebuilt.scale_display = None;
    incremental.submit(frame).unwrap();
    rebuilt.submit(frame).unwrap();
    let expected = display_pixels(&rebuilt);
    let actual = display_pixels(&incremental);
    assert_ne!(actual, original);
    let error = crate::test_support::max_error(&actual, &expected);
    assert!(error < 1e-6, "a filtered source page reaches pixels beyond its mapped bounds: {error}");
}

#[test]
fn masks_refresh_coverage_properties_and_paint_without_exact_display() {
    let mut doc = document();
    let extent = doc.composition().size;
    let owner=doc.scene().order()[0];
    let mask_source=coverage_mask(&mut doc,owner, Default::default(), Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 258., y: 0. },
        layer_core::Point { x: 258., y: 259. }, layer_core::Point { x: 0., y: 259. },
    ]).unwrap()));
    doc.artwork.coverage.get_mut(mask_source).unwrap().default_coverage=0.;
    let mut reduced = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for state in 0..7 {
        let mask = occurrence_mut(&mut doc,0).mask.as_mut().unwrap();
        mask.enabled = state != 3;
        mask.inverted = state == 1;

        let mut frame = packet(doc.scene(), extent);
        frame.inspect_mask=(state==2).then_some(owner);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        let dab = crate::tests::test_dab([370., 129.], [1.; 4], 1.);
        let batch = dab_batch(SourceTarget::Coverage(mask_source), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        if state == 5 {
            frame.dabs = std::slice::from_ref(&dab);
            frame.dab_batches = std::slice::from_ref(&batch);
        }
        reduced.submit(frame).unwrap();
        exact.submit(FramePacket { composite_all: true, ..frame }).unwrap();
        let cache = reduced.scale_display.as_ref().unwrap();
        let error = quality(&display_pixels(&reduced), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
        assert!(error[0] < 0.001 && error[1] < 0.015, "moving mask state={state}: {error:?}");
        assert_presentation_mip(&reduced);
        assert_settled(&mut reduced, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        let a = reduced.readback_srgb_rgba8().unwrap();
        let b = exact.readback_srgb_rgba8().unwrap();
        assert_eq!(a, b, "mask state={state} exact query");

    }
}
pub(crate) fn display_pixels(r: &WgpuRasterizer) -> Vec<[f32; 4]> {
    pixels(r, &materialized_display(r).texture)
}
fn materialized_display(r: &WgpuRasterizer) -> Image {
    let cache = r.scale_display.as_ref().unwrap();
    if let Some(root) = &cache.placed {
        let [width, height] = cache.plan.size;
        let texels = [0, 0, width, height];
        let values = match root {
            Presentation::Placed(p) => p.value.record(cache.plan, texels).unwrap(),
            Presentation::Mapped(p) => {
                let mut values = p.values;
                let side = (1 << cache.plan.level) as f32;
                for row in values[..128].chunks_exact_mut(16) { for component in row[..8].chunks_exact_mut(4) {
                    let n = f32::from_le_bytes(component.try_into().unwrap()) * side;
                    component.copy_from_slice(&n.to_le_bytes());
                }}
                for (dst, n) in values[144..160].chunks_exact_mut(4).zip(texels) { dst.copy_from_slice(&n.to_le_bytes()); }
                for (dst, n) in values[208..216].chunks_exact_mut(4).zip(cache.plan.extent) { dst.copy_from_slice(&(n as f32 / side).to_le_bytes()); }
                values
            }
        };
        let uniforms = r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("materialized display query"), contents: &values,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let (texture, view) = create_color_target(&r.device, cache.plan.size, "materialized display query");
        let pass = &r.scene_pipelines.resample;
        let kept = match root { Presentation::Placed(_) => root.view(), Presentation::Mapped(_) => root.next() };
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        match root {
            Presentation::Placed(_) => {
                let binding = pass.binding(&r.device, &uniforms, 0, [root.view(), &view, kept]);
                pass.encode(&mut encoder, &binding, texels);
            }
            Presentation::Mapped(_) => {
                let binding = pass.mesh_binding(&r.device, &uniforms, &[root.view().clone(), kept.clone()]);
                pass.encode_mesh(&mut encoder, &binding, &view, texels, None, false);
            }
        }
        encoder.submit(&r.queue);
        Image { texture, view, plan: cache.plan }
    } else {
        let image = cache.pixels.root().unwrap();
        if image.plan == cache.plan { return image.clone(); }
        let output = Image::new(r, cache.plan, "display window query");
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {
            origin: wgpu::Origin3d { x: cache.plan.bounds.min_x() >> cache.plan.level, y: cache.plan.bounds.min_y() >> cache.plan.level, z: 0 },
            ..image.texture.as_image_copy()
        }, output.texture.as_image_copy(), wgpu::Extent3d { width: cache.plan.size[0], height: cache.plan.size[1], depth_or_array_layers: 1 });
        encoder.submit(&r.queue);
        output
    }
}
fn window_pixels(r: &WgpuRasterizer, image: &Image, plan: display_mips::Plan) -> Vec<[f32; 4]> {
    let pixels = pixels(r, &image.texture);
    let [x, y, width, height] = paint_transform::texel_rect(plan.bounds.window_local(image.plan.bounds), 1 << plan.level);
    (y..y + height).flat_map(|y| pixels[(y * image.plan.size[0] + x) as usize..(y * image.plan.size[0] + x + width) as usize].iter().copied()).collect()
}

fn assert_presentation_mip(r: &WgpuRasterizer) {
    let cache = r.scale_display.as_ref().unwrap();
    let (input, actual, plan) = if let Some(Presentation::Placed(root)) = &cache.placed {
        let source = &r.scene.as_ref().unwrap().scale_sources;
        (pixels(r, &source.image(root.value.id, root.value.plan.level).image.texture),
            pixels(r, &source.image(root.value.id, root.value.plan.level + 1).image.texture), root.value.plan)
    } else { (display_pixels(r), window_pixels(r, cache.pixels.next().unwrap(),
        display_mips::Plan::window(cache.plan.extent, cache.plan.level + 1, cache.plan.bounds)), cache.plan) };
    let size = plan.level_size(plan.level + 1);
    let side = 1 << plan.level;
    for y in 0..size[1] {
        for x in 0..size[0] {
            let mut sum = [0.; 4];
            let mut area = 0.;
            for yy in y * 2..((y + 1) * 2).min(plan.size[1]) {
                for xx in x * 2..((x + 1) * 2).min(plan.size[0]) {
                    let weight = (side.min(plan.bounds.width() - xx * side)
                        * side.min(plan.bounds.height() - yy * side))
                        as f32;
                    for c in 0..4 {
                        sum[c] += input[(yy * plan.size[0] + xx) as usize][c] * weight;
                    }
                    area += weight;
                }
            }
            for c in 0..4 {
                assert!(
                    (actual[(y * size[0] + x) as usize][c] - sum[c] / area).abs() < 1e-5,
                    "adjacent output level must include every changed region and weight partial cells"
                );
            }
        }
    }
}

pub(crate) fn quality(actual: &[[f32; 4]], exact: &[[f32; 4]], plan: display_mips::Plan) -> [f32; 3] {
    quality_linear(actual, exact, plan, |color| color)
}

fn linear_color(color: [f32; 4], space: layer_core::BlendSpace, rgb: layer_core::color::RgbSpace) -> [f32; 4] {
    let a = color[3];
    if space == layer_core::BlendSpace::Linear || a <= 0. { return color; }
    [rgb.decode(f64::from(color[0] / a)) as f32 * a,
     rgb.decode(f64::from(color[1] / a)) as f32 * a,
     rgb.decode(f64::from(color[2] / a)) as f32 * a, a]
}

fn quality_linear(actual: &[[f32; 4]], exact: &[[f32; 4]], plan: display_mips::Plan, linear: impl Fn([f32; 4]) -> [f32; 4]) -> [f32; 3] {
    let side = 1 << plan.level;
    let mut errors = Vec::new();
    for y in 0..plan.size[1] {
        for x in 0..plan.size[0] {
            let mut sum = [0.; 4];
            let mut count = 0.;
            for yy in plan.bounds.min_y() + y * side..(plan.bounds.min_y() + (y + 1) * side).min(plan.bounds.max_y()) {
                for xx in plan.bounds.min_x() + x * side..(plan.bounds.min_x() + (x + 1) * side).min(plan.bounds.max_x()) {
                    for c in 0..4 {
                        sum[c] += exact[(yy * plan.extent[0] + xx) as usize][c];
                    }
                    count += 1.;
                }
            }
            let a = linear(actual[(y * plan.size[0] + x) as usize]);
            let b = linear(sum.map(|v| v / count));
            for c in 0..4 { errors.push((a[c] - b[c]).abs()); }
        }
    }
    errors.sort_by(f32::total_cmp);
    [
        errors.iter().sum::<f32>() / errors.len() as f32,
        errors[errors.len() * 99 / 100],
        *errors.last().unwrap(),
    ]
}

#[test]
fn scaled_composition_preserves_exact_paint_and_replaces_full_display() {
    let mut doc = document();
    let paint = copy_paint(&mut doc,0);
    let OccurrenceContent::Paint(source)=doc.artwork.occurrences.get(paint).unwrap().content else {unreachable!()}; doc.artwork.paint.get_mut(source).unwrap().original=None;
    insert_occurrence(&mut doc, paint, 0);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut p = packet(doc.scene(), extent);
    p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(p).unwrap();
    exact.submit(p).unwrap();

    assert!(!r.scene.as_ref().unwrap().scale_sources.entries.contains_key(&source_at(&doc,0)));
    assert!(r.scale_display.as_ref().unwrap().storage_bytes() < 1 << 20);
    let original = display_pixels(&r);
    let reference = pixels(&exact, crate::test_support::document_texture(&exact));
    let plan = r.scale_display.as_ref().unwrap().plan;
    assert_eq!(plan.level, 3);
    assert_eq!(plan.bounds, PixelRect::full(extent));
    assert!(quality(&original, &reference, plan)[2] < 1e-5);
    let mut dab = crate::tests::test_dab([255., 129.], [0.9, 0.02, 0.1, 0.7], 1.);
    dab.radii = [45.; 2];
    let batch = dab_batch(
        source_at(&doc,0),
        crate::layer_tests::preset_style(DefaultBrushPreset::GPen),
        dab.bounds(),
    );
    for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
        let batch = DabBatch {
            kind,
            ..batch.clone()
        };
        let stroke = FramePacket {
            dabs: &[dab],
            dab_batches: &[batch],
            composite_all: false,
            ..p
        };
        r.submit(stroke).unwrap();
        exact.submit(stroke).unwrap();
        assert_presentation_mip(&r);
        if kind == DabBatchKind::Preview {
            assert_eq!(r.preview_level, 2);
            assert!(
                r.preview_pages
                    .iter()
                    .all(|p| p.primary.texture.width() == 64)
            );
            assert!(r.preview_coverage_pages.is_empty());
        }
        let coarse = r.scale_display.as_ref().unwrap();
        let error = quality(
            &pixels(&r, coarse.texture()),
            &pixels(&exact, crate::test_support::document_texture(&exact)),
            coarse.plan,
        );
        assert!(
            error[0] < 0.003,
            "large brush preview must remain close to exact reduction"
        );
        let a = r.readback_srgb_rgba8().unwrap();
        let b = exact.readback_srgb_rgba8().unwrap();
        assert_eq!(a, b, "display resolution must not change exact output");
    }
    r.submit(FramePacket {
        composite_all: false,
        ..p
    })
    .unwrap();
    let final_pixels = display_pixels(&r);
    assert_ne!(original, final_pixels);
    assert_settled(&mut r, p, &pixels(&exact, crate::test_support::document_texture(&exact)));
    p.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    r.submit(p).unwrap();
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level, 0);

    assert!(quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan)[2] < 1e-6);
}

#[test]
fn native_predictions_match_exact_composition_before_layer_opacity_and_blending() {
    let mut doc = document_at([257, 259]);
    let front = copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(front).unwrap().opacity = 0.37;
    occurrence_mut(&mut doc,0).opacity = 0.63;
    insert_occurrence(&mut doc, front, 0);
    let extent = doc.composition().size;
    for target in [0, 1] {
        for mode in [layer_render::DabMode::Paint, layer_render::DabMode::Erase] {
            let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            exact.test.reference = true;
            let mut frame = packet(doc.scene(), extent);
            frame.view.width_px = extent[0];
            frame.view.height_px = extent[1];
            r.submit(frame).unwrap();
            exact.submit(frame).unwrap();
            let original = display_pixels(&r);
            let mut dab = crate::tests::test_dab([253., 254.], [0.9, 0.02, 0.1, 0.7], 1.);
            dab.radii = [35.; 2];
            let mut style = crate::layer_tests::preset_style(DefaultBrushPreset::GPen);
            style.mode = mode;
            let batch = DabBatch { kind: DabBatchKind::Preview,
                ..dab_batch(source_at(&doc,target), style, dab.bounds()) };
            for prediction in [true, false] {
                let stroke = FramePacket {
                    dabs: if prediction { std::slice::from_ref(&dab) } else { &[] },
                    dab_batches: if prediction { std::slice::from_ref(&batch) } else { &[] },
                    composite_all: false, ..frame
                };
                r.submit(stroke).unwrap();
                exact.submit(stroke).unwrap();
                let actual = display_pixels(&r);
                let expected = pixels(&exact, crate::test_support::document_texture(&exact));
                let error = quality(&actual, &expected, r.scale_display.as_ref().unwrap().plan);
                assert!(error[2] < 1e-6, "target={target}, mode={mode:?}, preview={prediction}: {error:?}");
                if prediction { assert_ne!(actual, original); }
                else { assert_eq!(actual, original); }
            }
        }
    }
}

#[test]
fn compact_preview_weights_partial_edge_texels_and_retires_corrections() {
    let doc = document();
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut p = packet(doc.scene(), extent);
    p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(p).unwrap();
    let original = display_pixels(&r);
    for position in [[516., 258.], [100., 100.], [514., 256.]] {
        let mut dab = crate::tests::test_dab(position, [0.9, 0.02, 0.1, 0.7], 1.);
        dab.radii = [12.; 2];
        let batch = DabBatch {
            kind: DabBatchKind::Preview,
            ..dab_batch(
                source_at(&doc,0),
                crate::layer_tests::preset_style(DefaultBrushPreset::GPen),
                dab.bounds(),
            )
        };
        r.submit(FramePacket {
            dabs: &[dab],
            dab_batches: &[batch],
            composite_all: false,
            ..p
        })
        .unwrap();
        if let Some(page) = r.preview_page([2, 1]) {
            let compact = pixels(&r, &page.active().texture);
            let cache = r.scale_display.as_ref().unwrap();
            let layer = pixels(&r, &r.scene.as_ref().unwrap().scale_sources.image(source_at(&doc,0), cache.plan.level).image.texture);
            // The 517 × 259 document ends with a 5 × 3 block: these two
            // compact texels cover 4 × 3 and 1 × 3 original pixels.
            let last = layer.last().unwrap();
            for c in 0..4 {
                assert!((last[c] - (compact[0][c] * 4. + compact[1][c]) / 5.).abs() < 1e-6);
            }
        }
    }
    r.submit(FramePacket {
        composite_all: false,
        ..p
    })
    .unwrap();
    assert_eq!(
        display_pixels(&r),
        original,
        "discarding a corrected tail restores every old footprint"
    );
    assert!(r.preview_pages.is_empty());
}

#[test]
fn scaled_layer_cache_tracks_stack_changes_and_odd_edges_at_each_level() {
    let mut doc = document();
    let foreground = copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(foreground).unwrap().opacity = 0.37;
    insert_occurrence(&mut doc, foreground, 0);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for level in [1, 3, 2, 4] {
        let scale = 1. / (1u32 << level) as f32;
        for change in 0..5 {
            match change {
                1 => occurrence_mut(&mut doc,0).opacity = 0.12,
                2 => swap_entries(&mut doc,0,1),
                3 => paint_mut(&mut doc,0).original = None,
                4 => paint_mut(&mut doc,0).original = paint_at(&doc,1).original.clone(),
                _ => (),
            }
            let mut p = packet(doc.scene(), extent);
            p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            r.submit(p).unwrap();
            exact.submit(p).unwrap();
            let cache = r.scale_display.as_ref().unwrap();
            let error = quality(
                &pixels(&r, cache.texture()),
                &pixels(&exact, crate::test_support::document_texture(&exact)),
                cache.plan,
            );
            assert!(
                error[2] < 2e-5,
                "constant-alpha stack, level {level}, change {change}: {error:?}"
            );
        }
    }
}

#[test]
fn display_and_native_evaluation_share_the_display_cache() {
    let mut doc = document();
    let extent = doc.composition().size;
    let mut draft = crate::tests::image_windows::program(false, false);
    Arc::make_mut(&mut draft.program).resolution = layer_core::EffectResolution::Native;
    let effect=effect_occurrence(&mut doc,draft,"native windows");
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for filtered in [true, false, true, false] {
        remove_occurrence(&mut doc,effect);
        if filtered {
            insert_occurrence(&mut doc,effect,0);
        }
        let mut p = packet(doc.scene(), extent);
        p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(p).unwrap();
        exact.submit(p).unwrap();
        assert!(r.scale_display.is_some());

        assert_eq!(r.scale_display.as_ref().unwrap().evaluation == Evaluation::Native, filtered);
        assert_settled(&mut r, p, &pixels(&exact, crate::test_support::document_texture(&exact)));
        if !filtered {
            assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), 0);
        }
        let a = r.readback_srgb_rgba8().unwrap();
        let b = exact.readback_srgb_rgba8().unwrap();
        assert_eq!(a, b);
    }
}

/// Optional photographic oracle, without putting a licensed photo in the repo.
/// Supply a 2048 × 1536, row-major, RGBA8 sRGB crop in LAYER_DISPLAY_PHOTO_RGBA.
#[test]
#[ignore = "requires a local photographic RGBA fixture in LAYER_DISPLAY_PHOTO_RGBA"]
fn photographic_preview_and_committed_display_quality() {
    let extent = [2048, 1536];
    let bytes = std::fs::read(std::env::var("LAYER_DISPLAY_PHOTO_RGBA").unwrap()).unwrap();
    assert_eq!(bytes.len(), (extent[0] * extent[1] * 4) as usize);
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: Default::default(),
            profile_assumed: false,
        },
        32 << 20,
    )
    .unwrap();
    for row in bytes.chunks_exact(extent[0] as usize * 4) {
        builder.push_row(row).unwrap();
    }
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let photo = copy_paint(&mut doc,0);
    let OccurrenceContent::Paint(source)=doc.artwork.occurrences.get(photo).unwrap().content else {unreachable!()}; doc.artwork.paint.get_mut(source).unwrap().original=Some(Arc::new(builder.finish().unwrap()));
    insert_occurrence(&mut doc, photo, 1);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut p = packet(doc.scene(), extent);
    for zoom in [0.071382575, 0.16, 0.3, 0.6] {
    p.view.document_to_surface = [zoom, 0., 0., zoom, 0., 0.];
    for space in layer_core::BlendSpace::ALL {
      p.blend_space = space;
      for diameter in [50., 460., 1000., 2000.] {
        r.submit(FramePacket {
            reset_layers: true,
            ..p
        })
        .unwrap();
        exact
            .submit(FramePacket {
                reset_layers: true,
                ..p
            })
            .unwrap();
        let mut dab = crate::tests::test_dab([1055., 780.], [0.01, 0.02, 0.03, 0.7], 1.);
        dab.radii = [diameter / 2.; 2];
        let batch = dab_batch(
            source_at(&doc,0),
            crate::layer_tests::preset_style(DefaultBrushPreset::GPen),
            dab.bounds(),
        );
        for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
            let batch = DabBatch {
                kind,
                ..batch.clone()
            };
            let stroke = FramePacket {
                dabs: &[dab],
                dab_batches: &[batch],
                composite_all: false,
                ..p
            };
            r.submit(stroke).unwrap();
            exact.submit(stroke).unwrap();
            let cache = r.scale_display.as_ref().unwrap();
            let errors = quality_linear(
                &display_pixels(&r),
                &pixels(&exact, crate::test_support::document_texture(&exact)),
                cache.plan,
                |color| linear_color(color, space, doc.composition().color.space),
            );
            assert!(
                errors[0] < 0.003 && errors[1] < 0.03,
                "photographic display quality regressed"
            );
            let actual = r.readback_srgb_rgba8().unwrap();
            let expected = exact.readback_srgb_rgba8().unwrap();
            assert_eq!(
                actual, expected,
                "photographic exact output at {space:?} {diameter}px {kind:?}"
            );
        }
    }
    }
    }
}

#[test]
fn unchanged_navigation_derives_and_reuses_a_bounded_neighbor_level() {
    let mut doc = document();
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut misses = None;
    let mut composed = 0;
    for (index, level) in [2, 3, 2, 3, 2].into_iter().enumerate() {
        let scale = 1. / (1u32 << level) as f32;
        let mut p = packet(doc.scene(), extent);
        p.composite_all = index == 0;
        p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(p).unwrap();
        exact.submit(p).unwrap();
        let work = r.source_cache_work()[1];
        assert_eq!(
            *misses.get_or_insert(work),
            work,
            "zooming must reuse already-decoded content"
        );
        if index >= 2 {
            assert_eq!(
                r.metrics.composited_pixels, composed,
                "returning to a cached level needs no composition"
            );
        }
        composed = r.metrics.composited_pixels;
        assert_presentation_mip(&r);
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.storage_bytes() <= crate::scene::scale::CACHE_BYTES);
        let error = quality(
            &display_pixels(&r),
            &pixels(&exact, crate::test_support::document_texture(&exact)),
            cache.plan,
        );
        assert!(
            error[2] < 1e-5,
            "derived level must include correct partial-edge weights: {error:?}"
        );
    }
    assert!(r.scale_display.as_ref().unwrap().spare.is_some());
    // Changing artwork retires the spare and any native backing references it
    // holds. Returning to that level must not resurrect its stale composition.
    occurrence_mut(&mut doc,0).opacity = 0.3;
    for (index, scale) in [0.25, 0.125].into_iter().enumerate() {
        let mut p = packet(doc.scene(), extent);
        p.composite_all = index == 0;
        p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(p).unwrap();
        exact.submit(p).unwrap();
        if scale == 0.25 {
            assert!(r.scale_display.as_ref().unwrap().spare.is_none());
        }
        let cache = r.scale_display.as_ref().unwrap();
        let error = quality(
            &display_pixels(&r),
            &pixels(&exact, crate::test_support::document_texture(&exact)),
            cache.plan,
        );
        assert!(
            error[2] < 1e-5,
            "changed artwork must replace the retired neighbor: {error:?}"
        );
    }
    occurrence_mut(&mut doc,0).visible = false;
    for (index, scale) in [0.25, 0.125, 0.25, 0.125, 0.125].into_iter().enumerate() {
        if index == 4 {
            occurrence_mut(&mut doc,0).visible = true;
        }
        let mut p = packet(doc.scene(), extent);
        p.composite_all = index == 0 || index == 4;
        p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(p).unwrap();
        exact.submit(p).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let error = quality(
            &display_pixels(&r),
            &pixels(&exact, crate::test_support::document_texture(&exact)),
            cache.plan,
        );
        assert!(
            error[2] < 1e-5,
            "an empty retained level must remain reusable after layer visibility changes"
        );
    }
}

#[path = "transform_tests.rs"]
mod transforms;

fn reduced_photo_stack() -> Document {
    let mut doc = document_at([65, 33]);
    composition_mut(&mut doc).size[0] = 9504; composition_mut(&mut doc).size[1] = 6336;
    for _ in 0..4 {
        let layer = copy_paint(&mut doc,0);
        doc.artwork.occurrences.get_mut(layer).unwrap().opacity = 0.35;
        doc.artwork.occurrences.get_mut(layer).unwrap().blend = layer_core::LayerBlend::SoftLight;
        insert_occurrence(&mut doc, layer, 0);
    }
    doc
}

fn resident_bytes(cache: &crate::scene::scale::Cache, scene: &crate::scene::Scene) -> u64 {
    cache.storage_bytes() + scene.scale_sources.storage_bytes() + scene.scale_commands.as_ref().unwrap().storage_bytes()
}

fn resident_bytes_with_pool(cache: &crate::scene::scale::Cache, scene: &crate::scene::Scene) -> u64 {
    resident_bytes(cache, scene) + scene.pool.iter().map(|p| texture_bytes(&p.texture)).sum::<u64>()
}

#[test]
fn retained_outer_mesh_display_refines_to_exact_after_geometry_and_mask_changes() {
    use layer_core::{Affine,LayerPlacement,MeshMap,Point,Projective,Rect,Interpolation};
    let extent = [513,387];
    let mut doc = document_at(extent);
    let id = source_at(&doc,0);
    paint_mut(&mut doc,0).original = Some(rgba8_source(extent,|x,y|
        if (x/7+y/5)%2==0 {[220,31,90,255]} else {[25,180,210,128]}));
    let domain = Rect::from_extent(extent);
    let mesh = Arc::new(MeshMap::from_affine(domain,[2,2],Affine([0.7,0.02,-0.03,0.8,57.,21.])).unwrap()
        .move_node(4,Point {x:31.,y:-23.}).unwrap());
    let owner=doc.scene().order()[0];
    let mask_source=coverage_mask(&mut doc,owner,Point {x:7.,y:-3.}, Some(layer_core::Selection::polygon(vec![Point {x:20.,y:30.},Point {x:400.,y:30.},
        Point {x:400.,y:300.},Point {x:20.,y:300.}]).unwrap()));
    doc.artwork.coverage.get_mut(mask_source).unwrap().default_coverage=0.63;
    occurrence_mut(&mut doc,0).mask.as_mut().unwrap().placement=Projective([1.,0.02,0.,0.,1.,0.,0.0001,0.,1.]);
    let mut cached = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference=true;
    for (step,outer) in [Projective::IDENTITY,Projective([1.05,0.02,-16.,-0.02,1.04,-6.,0.00015,-0.0001,1.])].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).placement =LayerPlacement {outer,mesh:Some(mesh.clone()),interpolation:Interpolation::Linear};
        occurrence_mut(&mut doc,0).mask.as_mut().unwrap().inverted=step!=0;
        let mut frame=packet(doc.scene(),extent);
        frame.view.document_to_surface=[0.125,0.,0.,0.125,0.,0.];
        cached.submit(frame).unwrap();exact.submit(frame).unwrap();
        let owner_geometry=layer_core::target_geometry(doc.scene(),id);
        let mask_geometry=layer_core::target_geometry(doc.scene(),SourceTarget::Coverage(mask_source));
        let scene=cached.scene.as_ref().unwrap();
        let owner_mesh=scene.mesh_geometry(&owner_geometry).unwrap();
        assert!(Arc::ptr_eq(&owner_mesh,&scene.mesh_geometry(&mask_geometry).unwrap()),"owner and linked mask share tessellation and winning UV");
        let reference=pixels(&exact,crate::test_support::document_texture(&exact));
        assert_settled(&mut cached,frame,&reference);
        assert!(cached.scene.as_ref().unwrap().scale_sources.cache_info(id).is_some());
    }
}

#[test]
fn a_near_unit_projective_photo_keeps_the_display_source_budget() {
    use layer_core::{LayerPlacement,Projective};
    let mut doc=document_at([33,17]);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.submit(packet(doc.scene(),doc.composition().size)).unwrap();
    composition_mut(&mut doc).size[0] =9504;composition_mut(&mut doc).size[1] =6336;
    paint_mut(&mut doc,0).domain=doc.composition().size;
    occurrence_mut(&mut doc,0).placement =LayerPlacement::from_projective(
        Projective([1.,0.,0.,0.,1.,0.,0.000001,0.,1.]));
    r.moving_layer=Some(doc.scene().order()[0]);
    let geometry=layer_core::target_geometry(doc.scene(),source_at(&doc,0));
    let mut frame=packet(doc.scene(),doc.composition().size);
    frame.view.document_to_surface=[0.1653409,0.,0.,0.1653409,0.,0.];
    let requested=request(&r,frame).unwrap();
    eprintln!("near-unit rate {} source level {} display level {} native {}",
        geometry.magnification(layer_core::Rect::from_extent(frame.document_extent)),
        source_level(requested.plan.level,&geometry,frame.document_extent),requested.plan.level,
        requested.evaluation==Evaluation::Native);
    assert!(requested.evaluation==Evaluation::Display,"a near-unit pose fits a reduced immutable source");
    assert!(source_level(requested.plan.level,&geometry,frame.document_extent)>0);
}

#[test]
fn a_large_bent_material_photo_keeps_the_display_source_budget() {
    use layer_core::{LayerPlacement,MeshMap,Point,Rect};
    use layer_core::raster::*;
    let mut doc=document_at([33,17]);
    let tile=RasterTile::backed(TileBlob::encode(RasterPlane::WatercolorWetness.descriptor(doc.composition().color),&vec![255;256*256]).unwrap());
    paint_mut(&mut doc,0).raster =RasterRevision::backed(RasterData {watercolor:Some(RasterWatercolor {wet_edge:0.9,burnt_edge:0.6,edge_width:8.}),
        tiles:BTreeMap::from([(TileKey {plane:RasterPlane::WatercolorWetness,coordinate:[0;2]},tile)]),..Default::default()});
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.submit(packet(doc.scene(),doc.composition().size)).unwrap();
    composition_mut(&mut doc).size[0] =9504;composition_mut(&mut doc).size[1] =6336;
    paint_mut(&mut doc,0).domain=doc.composition().size;
    occurrence_mut(&mut doc,0).placement =LayerPlacement {mesh:Some(Arc::new(MeshMap::identity(Rect::from_extent(doc.composition().size),[3;2]).unwrap()
        .move_node(5,Point {x:-120./0.1653409,y:-80./0.1653409}).unwrap())),..Default::default()};
    r.moving_layer=Some(doc.scene().order()[0]);
    let mut frame=packet(doc.scene(),doc.composition().size);frame.view.document_to_surface=[0.1653409,0.,0.,0.1653409,0.,0.];
    let requested=request(&r,frame).unwrap();
    let bytes=allocation_for(&r,requested.plan,frame,None,bounded(frame.scene),None).into_iter().sum::<u64>();
    let geometry=layer_core::target_geometry(doc.scene(),source_at(&doc,0));
    eprintln!("bent material photo: level={}, source={}, native={}, prospective={bytes}",requested.plan.level,source_level(requested.plan.level,&geometry,frame.document_extent),requested.evaluation==Evaluation::Native);
    assert!(requested.evaluation==Evaluation::Display,"the actual binned mesh and conservative reduced source fit the existing budget");
    assert!(bytes<=CACHE_BYTES);
    let scene=r.scene.as_ref().unwrap();let first=scene.mesh_geometry(&geometry).unwrap();
    let other=copy_paint(&mut doc,0);doc.artwork.occurrences.get_mut(other).unwrap().translation=Point {x:16.,y:8.};insert_occurrence(&mut doc,other,0);
    let second_geometry=layer_core::target_geometry(doc.scene(),source_at(&doc,0));
    Scene::geometry_bytes(doc.scene(),Some(scene));let second=scene.mesh_geometry(&second_geometry).unwrap();
    assert!(Arc::ptr_eq(&first,&scene.mesh_geometry(&geometry).unwrap()));
    assert!(Arc::ptr_eq(&second,&scene.mesh_geometry(&second_geometry).unwrap()));
    remove_occurrence(&mut doc,other);Scene::geometry_bytes(doc.scene(),Some(scene));
    assert_eq!(scene.mesh_geometry.borrow().len(),1,"retired owner geometry cannot accumulate across poses");
    let mut scene=r.scene.take().unwrap();
    let mapped=scene.material_coverage(&r,source_at(&doc,0),&geometry,&[]).0;
    assert!(!mapped.is_empty());
    assert!((mapped.max.x-mapped.min.x)*(mapped.max.y-mapped.min.y)<256.*256.*2.,"one wet tile keeps its mapped footprint: {mapped:?}");
    assert_eq!(scene.material_coverage(&r,source_at(&doc,0),&geometry,&[]).0,mapped);
    let frame=packet(doc.scene(),doc.composition().size);
    for tile in [[0,0],[1,0],[0,1]] {
        scene.placed_material_tile(&r,frame,doc.scene().order()[0],geometry.clone(),tile).unwrap();
    }
    assert_eq!(scene.mesh_geometry.borrow().len(),1,"material neighborhoods share the owner's world mesh");
    assert!(Arc::ptr_eq(&first,&scene.mesh_geometry(&geometry).unwrap()));
    assert!(scene.mesh_geometry.borrow().iter().map(|(_,mesh)|mesh.storage_bytes()).sum::<u64>()<=64*1024*1024);
}

#[test]
fn folded_display_across_positions_windows_matches_exact_paint() {
    let mut doc=document_at([2048,512]);
    occurrence_mut(&mut doc,0).placement =layer_core::LayerPlacement {mesh:Some(Arc::new(layer_core::MeshMap::identity(
        layer_core::Rect::from_extent(doc.composition().size),[3;2]).unwrap().move_node(4,layer_core::Point {x:1500.,y:0.}).unwrap())),..Default::default()};
    let mut cached=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();exact.test.reference=true;
    let mut frame=packet(doc.scene(),doc.composition().size);frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    cached.submit(frame).unwrap();exact.submit(frame).unwrap();
    let reference=pixels(&exact,crate::test_support::document_texture(&exact));
    assert_settled(&mut cached,frame,&reference);
}

#[test]
fn projective_compute_display_matches_analytic_bilinear_pigment_and_composition() {
    use layer_core::{ImageTransform,LayerPlacement,Projective};
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let source=crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba32Float);
    let colors:Vec<_>=(0..256*256).map(|i| {let a=if (i/256/5+i%256/7)%2==0 {0.25}else{0.75};
        [a*0.8,a*0.2,a*0.6,a]}).collect();
    crate::test_support::upload_page(&r,&source,&colors.iter().flatten().flat_map(|v|f32::to_le_bytes(*v)).collect::<Vec<_>>());
    let extent=[256;2];let plan=display_mips::Plan::at(extent,0);let texels=[3,5,113,83];
    let resample=&r.scene_pipelines.resample;
    for encode in [false,true] {
        let moved=ImageTransform {placement:LayerPlacement::from_projective(
            Projective([0.9,0.07,-16.,-0.05,1.1,-12.,0.001,-0.0002,1.])),..Default::default()};
        let values=crate::scene::resample::Resample::values(crate::scene::resample::Request {moved:&moved,kept:&ImageTransform::default(),
            clip:layer_core::Affine::IDENTITY,extent,texels,
            display:pixel_transform::DisplayLevel {side:1,extent,opacity:0.65,backdrop:[0.1,0.05,0.15,0.5],encode},
            target:plan,source:plan,max_lod:0,outside:0.,keep_source:false,identity:false}).unwrap();
        let uniforms=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:None,contents:&values,usage:wgpu::BufferUsages::UNIFORM});
        let compute=crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let fragment=crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let source=source.create_view(&Default::default());
        let compute_view=compute.create_view(&Default::default());let fragment_view=fragment.create_view(&Default::default());
        let binding=resample.binding(&r.device,&uniforms,0,[&source,&compute_view,&r.empty_view]);
        let reference=resample.mesh_binding(&r.device,&uniforms,&[source,r.empty_view.clone()]);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        resample.encode(&mut encoder,&binding,texels);
        resample.encode_mesh(&mut encoder,&reference,&fragment_view,texels,None,false);
        r.uploads.finish(&encoder);encoder.submit(&r.queue);
        let actual=pixels(&r,&compute);let expected=pixels(&r,&fragment);
        assert!(actual.iter().any(|p|p[3]>0.5),"projective display samples actual semitransparent pigment");
        let rows=pixel_transform::inverse_rows(&moved).unwrap().map(|row|row.map(f64::from));
        let mut maximum=[0f64;2];
        for y in 0..256 {for x in 0..256 {
            let index=y*256+x;
            if x<texels[0] as usize || y<texels[1] as usize || x>=(texels[0]+texels[2]) as usize || y>=(texels[1]+texels[3]) as usize {
                assert_eq!(actual[index],[0.;4]);assert_eq!(expected[index],[0.;4]);continue;
            }
            let [u,v]=crate::test_support::preimage(moved.placement.outer.0.map(f64::from),[x as f64+0.5,y as f64+0.5]).unwrap();
            let h=[x as f64+0.5,y as f64+0.5,1.];
            let dot=|row:usize| rows[row].into_iter().zip(h).map(|(a,b)|a*b).sum::<f64>();
            let size=|row:usize| rows[row].into_iter().zip(h).map(|(a,b)|(a*b).abs()).sum::<f64>();
            let w=dot(2);let round=4.*f64::from(f32::EPSILON);let minimum_w=w-round*size(2);
            assert!(minimum_w>0.);
            let coordinate_error=[u,v].into_iter().enumerate().map(|(axis,q)|
                (dot(axis)/w-q).abs()+round*(size(axis)+q.abs()*size(2))/minimum_w+q.abs()*f64::from(f32::EPSILON)).sum::<f64>();
            let [u,v]=[u-0.5,v-0.5];let [i,j]=[u.floor() as usize,v.floor() as usize];let [fx,fy]=[u.fract(),v.fract()];
            assert!(i+1<256 && j+1<256);
            let alpha=f64::from(colors[j*256+i][3])*(1.-fx)*(1.-fy)+f64::from(colors[j*256+i+1][3])*fx*(1.-fy)
                +f64::from(colors[(j+1)*256+i][3])*(1.-fx)*fy+f64::from(colors[(j+1)*256+i+1][3])*fx*fy;
            let opacity=alpha*0.65;let mut reference=[0f64;4];
            for (channel,value) in [0.8,0.2,0.6].into_iter().enumerate() {
                let value=if encode {r.device.working_space().encode(value)}else{value};
                reference[channel]=value*opacity+[0.1,0.05,0.15][channel]*(1.-opacity);
            }
            reference[3]=opacity+0.5*(1.-opacity);
            for (path,pixel) in [actual[index],expected[index]].into_iter().enumerate() {
                for channel in 0..4 {
                    let error=(f64::from(pixel[channel])-reference[channel]).abs();maximum[path]=maximum[path].max(error);
                    let color=if channel==3 {1.}else {let value=[0.8,0.2,0.6][channel];if encode {r.device.working_space().encode(value)}else{value}};
                    let paper=[0.1,0.05,0.15,0.5][channel];
                    let bound=0.5*0.65*(2./256.+coordinate_error)*(color-paper).abs()+2e-5;
                    assert!(error<bound,"sampler rounding at {x},{y} channel{channel}, path{path}: {error} exceeds {bound}");
                }
            }
        }}
        eprintln!("analytic projective compute/fragment max error {maximum:?}, encode {encode}");
        assert!(maximum.into_iter().all(|error|error<1./255.),"reduced U8 display is within one channel code of analytic bilinear pigment: {maximum:?}");
    }
}
