use layer_render::CanvasRenderer;
use crate::{WgpuRasterizer, snapshot::{CaptureControl, SnapshotGpu}};
use layer_core::{ArtworkSample, ArtworkSampleRequest, ArtworkSource, Document, DocumentNames, Point};
use layer_core::color::{DocumentColor, SampleDepth};
use layer_core::authored::*;
use std::sync::Arc;
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};

pub(super) fn gpu() -> SnapshotGpu {
    static GPU: std::sync::OnceLock<SnapshotGpu> = std::sync::OnceLock::new();
    GPU.get_or_init(|| WgpuRasterizer::new_native_headless(Default::default()).unwrap().snapshot_gpu()).clone()
}

fn document(extent: [u32; 2], pixel: impl Fn(u32, u32) -> [f32; 4]) -> Document {
    document_in(extent, layer_core::color::RgbSpace::Srgb, pixel)
}

pub(super) fn document_in(extent: [u32; 2], space: layer_core::color::RgbSpace, pixel: impl Fn(u32, u32) -> [f32; 4]) -> Document {
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().color = DocumentColor { depth: SampleDepth::F32, space };
    let paper = doc.scene().children(None)[1];
    doc.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    let mut bytes = Vec::with_capacity(256 * 256 * 16);
    for y in 0..256 {
        for x in 0..256 {
            let mut rgba = pixel(x, y);
            if rgba[3] != 0. { for i in 0..3 { rgba[i] /= rgba[3]; } }
            bytes.extend(rgba.into_iter().flat_map(f32::to_le_bytes));
        }
    }
    let mut data = RasterData::default();
    data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] },
        RasterTile::backed(TileBlob::encode(doc.composition().color.paint_descriptor(), &bytes).unwrap()));
    paint_mut(&mut doc).raster = RasterRevision::backed(data);
    doc
}

pub(super) fn paint_id(doc: &Document) -> PaintHandle {
    doc.scene().order().iter().find_map(|&h| match doc.scene().occurrence(h)?.content {OccurrenceContent::Paint(p)=>Some(p),_=>None}).unwrap()
}
pub(super) fn paint_mut(doc: &mut Document) -> &mut PaintSource {let h=paint_id(doc);doc.artwork.paint.get_mut(h).unwrap()}
pub(super) fn paint_occurrence(doc: &Document) -> OccurrenceHandle {doc.scene().source_owner(SourceTarget::Paint(paint_id(doc))).unwrap()}
pub(super) fn refresh(doc: &mut Document) {
    let mut indexed=Document::from_artwork(doc.artwork.clone()).unwrap();
    indexed.owner=doc.owner;indexed.revision=doc.revision;indexed.working=doc.working.clone();*doc=indexed;
}
pub(super) fn insert_effect(doc: &mut Document, effect: layer_core::EffectInstance, index: usize) -> OccurrenceHandle {
    let definition=doc.artwork.definitions.insert(PortableId::random(),Definition {program:effect.program}).unwrap();
    let application=doc.artwork.effects.insert(PortableId::random(),EffectApplication {definition,values:effect.values,domain:doc.composition().size}).unwrap();
    let h=doc.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(application),"Effect")).unwrap();
    let stack=doc.composition().result;doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(index,h);refresh(doc);h
}
pub(super) fn effect_draft(doc: &Document, occurrence: OccurrenceHandle) -> layer_core::EffectInstance {
    let view=doc.scene().effect(occurrence).unwrap();let application=doc.scene().effect_application(occurrence).unwrap();
    layer_core::EffectInstance {program:doc.artwork.definitions.get(application.definition).unwrap().program.clone(),values:view.values.to_vec()}
}
pub(super) fn set_effect(doc: &mut Document, occurrence: OccurrenceHandle, draft: layer_core::EffectInstance) {
    let h=doc.scene().effect_handle(occurrence).unwrap();let definition=doc.artwork.effects.get(h).unwrap().definition;
    doc.artwork.definitions.get_mut(definition).unwrap().program=draft.program;doc.artwork.effects.get_mut(h).unwrap().values=draft.values;
}
pub(super) fn add_group(doc: &mut Document, children: Vec<OccurrenceHandle>, index: usize) -> OccurrenceHandle {
    let root=doc.composition().result;doc.artwork.stacks.get_mut(root).unwrap().entries.retain(|h|!children.contains(h));
    let stack=doc.artwork.stacks.insert(PortableId::random(),Stack {entries:children}).unwrap();
    let h=doc.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Stack(stack),"Group")).unwrap();
    doc.artwork.stacks.get_mut(root).unwrap().entries.insert(index,h);refresh(doc);h
}

fn sample(doc: &Document, source: ArtworkSource, position: [f32; 2], width: u32) -> Result<ArtworkSample, String> {
    pollster::block_on(gpu().artwork_sample(ArtworkSampleRequest::new(doc, source, position, width), CaptureControl::default()))
}

fn close(actual: ArtworkSample, expected: [f64; 4]) {
    let ArtworkSample::Color(actual) = actual else { panic!("Expected color, got {actual:?}"); };
    for (index, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
        let scale = if index == 3 { expected.abs().max(1e-38) } else { expected.abs().max(1.) };
        assert!((f64::from(actual) - expected).abs() <= 2e-5 * scale, "{actual} != {expected}");
    }
}

fn pixel(x: u32, y: u32) -> [f32; 4] {
    let alpha = match (x + 2 * y) % 4 { 0 => 0., 1 => 0.125, 2 => 0.5, _ => 1. };
    [(x as f32 / 7. - 2.) * alpha, (y as f32 / 11.) * alpha, 4. * alpha, alpha]
}

fn oracle(extent: [u32; 2], contact: [f32; 2], width: u32) -> [f64; 4] {
    let center = contact.map(|p| p.floor() as i64);
    let mut sum = [0f64; 4];
    let mut count = 0.;
    for y in 0..extent[1] {
        for x in 0..extent[0] {
            let dx = i64::from(x) - center[0];
            let dy = i64::from(y) - center[1];
            if 4 * (dx * dx + dy * dy) > i64::from(width).pow(2) { continue; }
            count += 1.;
            for (sum, value) in sum.iter_mut().zip(pixel(x, y)) { *sum += f64::from(value); }
        }
    }
    [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3], sum[3] / count]
}

#[test]
fn artwork_sample_circles_match_independent_f64_alpha_weighted_hdr_oracle() {
    let extent = [111, 107];
    let doc = document(extent, pixel);
    for width in [1, 5, 15, 51, 101] {
        for contact in [[53.99, 51.1], [1.9, 2.7], [110.1, 106.9]] {
            close(sample(&doc, ArtworkSource::Visible, contact, width).unwrap(), oracle(extent, contact, width));
        }
    }
}

#[test]
fn artwork_sample_distinguishes_outside_transparency_and_cancelled_capture() {
    let doc = document([7, 5], |_, _| [0.; 4]);
    assert_eq!(sample(&doc, ArtworkSource::Visible, [3., 2.], 5).unwrap(), ArtworkSample::Empty);
    for point in [[-0.01, 2.], [7., 2.], [3., 5.]] {
        assert_eq!(sample(&doc, ArtworkSource::Visible, point, 5).unwrap(), ArtworkSample::Outside);
    }
    let control = CaptureControl::default();
    control.cancel();
    assert!(pollster::block_on(gpu().artwork_sample(ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [0.; 2], 5), control)).is_err());
}

#[test]
fn artwork_sample_layer_content_ignores_mask_opacity_and_places_pixels() {
    let mut doc = document([20, 20], |x, _| [x as f32 / 20., 0.25, 2., 1.]);
    let owner=paint_occurrence(&doc);let id=SourceTarget::Paint(paint_id(&doc));
    let coverage=doc.artwork.coverage.next_handle();
    let mut mask=layer_core::CoverageSnapshot::reveal_all(coverage,[20,20],Point::default());mask.source.default_coverage=0.5;
    doc.artwork.coverage.insert(PortableId::random(),mask.source).unwrap();
    let occurrence=doc.artwork.occurrences.get_mut(owner).unwrap();occurrence.opacity=0.25;occurrence.mask=Some(mask.use_);
    occurrence.placement=layer_core::LayerPlacement::from_affine(layer_core::Affine::translation(Point {x:3.,y:2.}));
    close(sample(&doc, ArtworkSource::Source(id), [8., 5.], 1).unwrap(), [0.25, 0.25, 2., 1.]);
    close(sample(&doc, ArtworkSource::Visible, [8., 5.], 1).unwrap(), [0.25, 0.25, 2., 0.125]);
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().size=[64;2];
    let group=add_group(&mut doc,vec![owner],0);doc.artwork.occurrences.get_mut(group).unwrap().translation=Point {x:17.,y:23.};
    let occurrence=doc.artwork.occurrences.get_mut(owner).unwrap();occurrence.visible=false;occurrence.opacity=0.4;occurrence.blend=layer_core::LayerBlend::Multiply;occurrence.attachment = layer_core::Attachment::Clip;
    let mut raw=gpu().capture_scene(doc.snapshot(),SceneScope::Raw(id),CaptureControl::default()).unwrap();
    let pixel=raw.read_region([25,28,1,1]).unwrap()[0];
    for (actual,expected) in pixel.into_iter().zip([0.25,0.25,2.,1.]) {assert!((actual-expected).abs()<2e-5,"raw hidden source {pixel:?}");}
    assert_eq!(doc.scene().target_offset(id),Point {x:17.,y:23.});
}

#[test]
fn artwork_sample_normalizes_large_finite_premultiplied_values_before_summing() {
    let doc = document([101, 101], |_, _| [1e35, -2e35, 3e35, 0.5]);
    close(sample(&doc, ArtworkSource::Visible, [50., 50.], 101).unwrap(), [2e35, -4e35, 6e35, 0.5]);
}

pub(super) fn doubled_effect() -> layer_core::EffectInstance {
    let mut program=(*crate::tests::fixture("exposure").program()).clone();
    program.wgsl="fn double_color(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(2.*c.rgb,c.a);}".into();
    program.entry="double_color".into();program.passes=Arc::new([]);
    layer_core::EffectInstance::new(Arc::new(program))
}

#[test]
fn artwork_sample_effect_input_excludes_active_and_upper_adjustments_and_baseline_restores_original() {
    let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 1.]);
    insert_effect(&mut doc,doubled_effect(),0);
    let active=insert_effect(&mut doc,doubled_effect(),0);
    let effect=doc.scene().effect_handle(active).unwrap();let mut original=doc.artwork.effects.get(effect).unwrap().clone();
    let mut program=(*crate::tests::fixture("exposure").program()).clone();program.wgsl="fn identity_color(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return c;}".into();program.entry="identity_color".into();program.passes=Arc::new([]);
    original.definition=doc.artwork.definitions.insert(PortableId::random(),Definition {program:Arc::new(program)}).unwrap();
    insert_effect(&mut doc,doubled_effect(),0);
    close(sample(&doc,ArtworkSource::Visible,[8.;2],1).unwrap(),[1.,2.,4.,1.]);
    close(sample(&doc,ArtworkSource::EffectInput(active),[8.;2],1).unwrap(),[0.25,0.5,1.,1.]);
    close(sample(&doc,ArtworkSource::EffectBaseline(EffectBaseline {occurrence:active,effect,application:original}),[8.;2],1).unwrap(),[0.5,1.,2.,1.]);
}

#[test]
fn artwork_sample_reference_uses_saved_membership_instead_of_visible_upper_layers() {
    let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 1.]);
    let owner=paint_occurrence(&doc);doc.artwork.occurrences.get_mut(owner).unwrap().reference=true;
    insert_effect(&mut doc,doubled_effect(),0);
    close(sample(&doc, ArtworkSource::Reference, [8.; 2], 1).unwrap(), [0.125, 0.25, 0.5, 1.]);
    close(sample(&doc, ArtworkSource::Visible, [8.; 2], 1).unwrap(), [0.25, 0.5, 1., 1.]);
}

#[test]
fn artwork_sample_effect_input_respects_isolated_group_and_clipped_base() {
    for clipped in [false, true] {
        let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 0.5]);
        let paint=paint_occurrence(&doc);let active=insert_effect(&mut doc,doubled_effect(),0);let upper=insert_effect(&mut doc,doubled_effect(),0);
        doc.artwork.occurrences.get_mut(active).unwrap().attachment = if clipped { layer_core::Attachment::Effect } else { layer_core::Attachment::None };doc.artwork.occurrences.get_mut(upper).unwrap().attachment = if clipped { layer_core::Attachment::Effect } else { layer_core::Attachment::None };
        add_group(&mut doc,vec![upper,active,paint],0);
        let paper=doc.scene().children(None)[1];doc.artwork.occurrences.get_mut(paper).unwrap().visible=true;
        close(sample(&doc, ArtworkSource::EffectInput(active), [8.; 2], 1).unwrap(), [0.25, 0.5, 1., 0.5]);
    }
}

#[test]
fn artwork_sample_effect_input_follows_group_roots_instead_of_descendant_storage_order() {
    for clipped in [false,true] {
        let mut doc=document([16,16],|_,_|[0.125,0.25,0.5,0.5]);
        let child=paint_occurrence(&doc);let lower=add_group(&mut doc,vec![child],0);
        let target=insert_effect(&mut doc,doubled_effect(),0);doc.artwork.occurrences.get_mut(target).unwrap().attachment = if clipped { layer_core::Attachment::Effect } else { layer_core::Attachment::None };
        let source=document([16,16],|_,_|[0.5,0.125,0.25,1.]);
        let paint=doc.artwork.paint.insert(PortableId::random(),source.artwork.paint.get(paint_id(&source)).unwrap().clone()).unwrap();
        let upper_child=doc.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),"Upper child")).unwrap();
        let upper_effect=insert_effect(&mut doc,doubled_effect(),0);let upper=add_group(&mut doc,vec![upper_effect,upper_child],0);
        let root=doc.composition().result;doc.artwork.stacks.get_mut(root).unwrap().entries=vec![upper,target,lower];refresh(&mut doc);
        for _ in 0..2 {
            close(sample(&doc,ArtworkSource::Visible,[8.;2],1).unwrap(),[1.,0.25,0.5,1.]);
            close(sample(&doc,ArtworkSource::EffectInput(target),[8.;2],1).unwrap(),[0.25,0.5,1.,0.5]);
            refresh(&mut doc);
        }
    }
}

#[test]
fn artwork_sample_tiny_covered_alpha_preserves_representable_extended_color() {
    let doc = document([5, 5], |_, _| [1., -0.5, 0.25, 1e-30]);
    close(sample(&doc, ArtworkSource::Visible, [2.; 2], 5).unwrap(), [1e30, -5e29, 2.5e29, 1e-30]);
}

#[test]
fn artwork_sample_reads_frozen_backing_after_live_document_pixels_change() {
    let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 1.]);
    let request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [8.; 2], 5);
    let changed=document([16,16],|_,_|[0.75,0.5,0.25,1.]);
    paint_mut(&mut doc).raster=changed.artwork.paint.get(paint_id(&changed)).unwrap().raster.clone();
    assert!(!request.matches_artwork(&doc));
    close(pollster::block_on(gpu().artwork_sample(request, CaptureControl::default())).unwrap(), [0.125, 0.25, 0.5, 1.]);
    close(sample(&doc, ArtworkSource::Visible, [8.; 2], 5).unwrap(), [0.75, 0.5, 0.25, 1.]);
}

#[test]
fn artwork_sample_uses_captured_animation_clock_and_explicit_batch_phase() {
    let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 1.]);
    let fill = doc.scene().effect_handle(doc.scene().children(None)[1]).unwrap();
    let mut program = (*crate::tests::fixture("domain_warp").program()).clone();
    program.wgsl = "fn query_phase(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(fx_time(b),.25,.5,1.);}".into();
    program.entry = "query_phase".into();
    program.passes = std::sync::Arc::new([]);
    let mut effect = layer_core::EffectInstance::new(std::sync::Arc::new(program));
    effect.set("animate", layer_core::EffectValue::Toggle(true)).unwrap();
    effect.set("speed", layer_core::EffectValue::Number(1.)).unwrap();
    let occurrence=insert_effect(&mut doc,effect,0);let handle=doc.scene().effect_handle(occurrence).unwrap();
    let mut live=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    live.submit(layer_render::FramePacket {time_seconds:2.,reset_layers:true,..crate::test_support::packet(doc.scene(),[16,16])}).unwrap();
    let frozen_gpu=live.snapshot_gpu();let mut request=ArtworkSampleRequest::new(&doc,ArtworkSource::Visible,[8.;2],1);
    request.set_context(live.evaluation_context());
    let mut draft=effect_draft(&doc,occurrence);draft.set("speed",layer_core::EffectValue::Number(2.)).unwrap();set_effect(&mut doc,occurrence,draft);
    for elapsed in [2.,3.] {live.submit(layer_render::FramePacket {time_seconds:elapsed,..crate::test_support::packet(doc.scene(),[16,16])}).unwrap();}
    assert_eq!(live.evaluation_context().phases.as_slice(),&[(handle,4.),(fill,0.)]);
    let mut replacement=crate::test_support::staged_renderer(&live,doc.composition().color);
    replacement.seed_evaluation_context(live.evaluation_context());
    for (elapsed,phase) in [(3.,4.),(4.,6.)] {
        replacement.submit(layer_render::FramePacket {time_seconds:elapsed,..crate::test_support::packet(doc.scene(),[16,16])}).unwrap();
        assert_eq!(replacement.evaluation_context().phases.as_slice(),&[(handle,phase),(fill,0.)]);
    }
    close(pollster::block_on(frozen_gpu.artwork_sample(request.clone(),CaptureControl::default())).unwrap(),[2.,0.25,0.5,1.]);
    request.set_context(EvaluationContext {elapsed:2.,phases:vec![(handle,6.)].into()});
    close(pollster::block_on(frozen_gpu.artwork_sample(request,CaptureControl::default())).unwrap(),[6.,0.25,0.5,1.]);
}

#[test]
fn failed_frame_keeps_clock_rate_and_frozen_phase_ownership() {
    let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 1.]);
    let fill = doc.scene().effect_handle(doc.scene().children(None)[1]).unwrap();
    let mut program = (*crate::tests::fixture("domain_warp").program()).clone();
    program.wgsl = "fn query_phase(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(fx_time(b),.25,.5,1.);}".into();
    program.entry = "query_phase".into();
    program.passes = Arc::new([]);
    let mut effect = layer_core::EffectInstance::new(Arc::new(program));
    effect.set("animate", layer_core::EffectValue::Toggle(true)).unwrap();
    effect.set("speed", layer_core::EffectValue::Number(1.)).unwrap();
    let occurrence = insert_effect(&mut doc, effect, 0);
    let handle = doc.scene().effect_handle(occurrence).unwrap();
    let mut live = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    live.submit(layer_render::FramePacket { time_seconds:2., reset_layers:true,
        ..crate::test_support::packet(doc.scene(), [16; 2]) }).unwrap();
    let previous = live.evaluation_context();
    assert_eq!(previous.phases.as_slice(), &[(handle, 2.), (fill, 0.)]);
    let frozen = doc.snapshot_with_context(previous.clone());
    let original = effect_draft(&doc, occurrence);
    let mut changed = original.clone();
    changed.set("speed", layer_core::EffectValue::Number(4.)).unwrap();
    set_effect(&mut doc, occurrence, changed);
    let style = layer_render::DabStyle::for_brush(&layer_core::BrushSnapshot::default(), layer_core::StrokeTool::Brush);
    let mut batch = crate::test_support::dab_batch(SourceTarget::Paint(paint_id(&doc)), style, layer_core::Rect::from_extent([16; 2]));
    batch.dab_count = 1;
    let error = live.submit(layer_render::FramePacket { time_seconds:3., composite_all:true, dab_batches:&[batch],
        ..crate::test_support::packet(doc.scene(), [16; 2]) }).unwrap_err();
    assert!(matches!(error, crate::GpuRasterError::InvalidDabRange));
    assert_eq!(live.evaluation_context(), previous);
    assert!(Arc::ptr_eq(&live.evaluation_context().phases, &previous.phases));
    set_effect(&mut doc, occurrence, original);
    live.submit(layer_render::FramePacket { time_seconds:4., composite_all:true,
        ..crate::test_support::packet(doc.scene(), [16; 2]) }).unwrap();
    assert_eq!(live.evaluation_context().phases.as_slice(), &[(handle, 4.), (fill, 0.)]);
    assert_eq!(frozen.context.phases.as_slice(), &[(handle, 2.), (fill, 0.)]);
    let stable = live.evaluation_context();
    live.submit(layer_render::FramePacket { time_seconds:4., composite_all:true,
        ..crate::test_support::packet(doc.scene(), [16; 2]) }).unwrap();
    assert!(Arc::ptr_eq(&stable.phases, &live.evaluation_context().phases));
    let mut changed = effect_draft(&doc, occurrence);
    changed.set("speed", layer_core::EffectValue::Number(4.)).unwrap();
    set_effect(&mut doc, occurrence, changed);
    live.submit(layer_render::FramePacket { time_seconds:10., composite_all:true,
        ..crate::test_support::packet(doc.scene(), [16; 2]) }).unwrap();
    assert_eq!(live.evaluation_context().phases.as_slice(), &[(handle, 10.), (fill, 0.)]);
    live.submit(layer_render::FramePacket { time_seconds:20., composite_all:true,
        ..crate::test_support::packet(frozen.view(), [16; 2]) }).unwrap();
    assert_eq!(live.evaluation_context().phases.as_slice(), &[(handle, 2.), (fill, 0.)]);
    assert_eq!(frozen.context, previous);
    let frame = live.artwork_frame.as_ref().unwrap();
    let request = ArtworkSampleRequest { query:layer_core::ArtworkQuery::from_snapshot(frame.scene.clone(), ArtworkSource::Visible, None),
        position:[8.; 2], width:1 };
    close(pollster::block_on(live.snapshot_gpu().artwork_sample(request, CaptureControl::default())).unwrap(), [2.,0.25,0.5,1.]);
}

#[test]
fn artwork_sample_nonlinear_layer_content_returns_document_primary_linear_color() {
    for space in [layer_core::color::RgbSpace::Srgb, layer_core::color::RgbSpace::DisplayP3, layer_core::color::RgbSpace::AdobeRgb, layer_core::color::RgbSpace::ProPhoto] {
        let mut doc = document_in([20, 20], space, |x, _| [x as f32 / 40., 0.25, 1., 0.5]);
        let owner=paint_occurrence(&doc);let id=SourceTarget::Paint(paint_id(&doc));
        let map = layer_core::Projective::rect_to_quad(layer_core::Rect::from_extent([20, 20]),
            [[0., 0.], [24., 0.], [18., 20.], [0., 20.]].map(|[x, y]| Point { x, y })).unwrap();
        doc.artwork.occurrences.get_mut(owner).unwrap().placement = layer_core::LayerPlacement::from_projective(map);
        let source = map.inverse().unwrap().map(Point { x: 8.5, y: 8.5 }).unwrap();
        close(sample(&doc, ArtworkSource::Source(id), [8., 8.], 1).unwrap(),
            [f64::from(source.x - 0.5) / 20., 0.5, 2., 0.5]);
    }
}

#[test]
fn artwork_sample_does_not_unassociate_unrepresentable_individual_texels_before_average() {
    let mut doc = document([5, 5], |_, _| [0.; 4]);
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend=layer_core::BlendSpace::Linear;
    let mut effect=doubled_effect();
    let program=Arc::make_mut(&mut effect.program);
    program.alpha = layer_core::EffectAlpha::Filter;
    program.wgsl = "fn weighted_color(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(1e10,5e9,2.5e9,select(1.,1e-30,p.x<2.));}".into();
    program.entry = "weighted_color".into();
    insert_effect(&mut doc,effect,0);
    assert!(sample(&doc, ArtworkSource::Visible, [0., 2.], 1).is_err());
    let mut count = 0.;
    let mut covered = 0.;
    for y in 0..5i32 {
        for x in 0..5i32 {
            if (x - 2).pow(2) + (y - 2).pow(2) > 6 { continue; }
            count += 1.;
            if x >= 2 { covered += 1.; }
        }
    }
    close(sample(&doc, ArtworkSource::Visible, [2., 2.], 5).unwrap(),
        [1e10 * count / covered, 5e9 * count / covered, 2.5e9 * count / covered, covered / count]);
}

#[test]
fn artwork_sample_cancellation_releases_pending_root_and_tile_before_publication() {
    let mut outcomes = Vec::new();
    for pending_tile in [false, true] {
        let mut doc = document([5, 5], |_, _| [0.; 4]);
        let pending = RasterRevision::pending();
        let tile = RasterTile::pending(doc.composition().color.paint_descriptor());
        paint_mut(&mut doc).raster = if pending_tile {
            let mut data = RasterData::default();
            data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, tile.clone());
            RasterRevision::backed(data)
        } else { pending.clone() };
        let request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [2.; 2], 5);
        let gpu = gpu();
        let control = CaptureControl::default();
        let worker_control = control.clone();
        let (started, entered) = std::sync::mpsc::channel();
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            let result = pollster::block_on(gpu.artwork_sample(request, worker_control));
            sender.send(result).unwrap();
        });
        entered.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let before_cancel = receiver.try_recv();
        control.cancel();
        let cancelled = receiver.recv_timeout(std::time::Duration::from_millis(500));
        if pending_tile { tile.publish(Ok(TileBlob::encode(doc.composition().color.paint_descriptor(), &vec![0; 256 * 256 * 16]).unwrap())).unwrap(); }
        else { pending.publish(Ok(RasterData::default())).unwrap(); }
        if cancelled.is_err() { let _ = receiver.recv_timeout(std::time::Duration::from_secs(2)); }
        worker.join().unwrap();
        assert_eq!(sample(&doc, ArtworkSource::Visible, [2.; 2], 5).unwrap(), ArtworkSample::Empty);
        outcomes.push((pending_tile, before_cancel, cancelled));
    }
    assert!(outcomes.iter().all(|(_, before, _)| matches!(before, Err(std::sync::mpsc::TryRecvError::Empty))), "pending backing must not become Empty: {outcomes:?}");
    assert!(outcomes.iter().all(|(_, _, result)| matches!(result, Ok(Err(error)) if error.contains("cancel"))), "cancelled workers stayed blocked or returned wrong results: {outcomes:?}");
}

#[test]
fn artwork_sample_pending_producer_failure_is_error_instead_of_empty() {
    for pending_tile in [false, true] {
        let mut doc = document([5, 5], |_, _| [0.; 4]);
        let pending = RasterRevision::pending();
        let tile = RasterTile::pending(doc.composition().color.paint_descriptor());
        paint_mut(&mut doc).raster = if pending_tile {
            let mut data = RasterData::default();
            data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, tile.clone());
            RasterRevision::backed(data)
        } else { pending.clone() };
        let request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [2.; 2], 5);
        if pending_tile { tile.publish(Err("query source readback failed".into())).unwrap(); }
        else { pending.publish(Err("query source readback failed".into())).unwrap(); }
        let result = pollster::block_on(gpu().artwork_sample(request, CaptureControl::default()));
        assert!(matches!(result, Err(ref error) if error.contains("query source readback failed")), "pending tile={pending_tile}: {result:?}");
    }
}
