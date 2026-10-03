use layer_render::CanvasRenderer;
use crate::{WgpuRasterizer, snapshot::{CaptureControl, SnapshotGpu}};
use layer_core::{ArtworkSample, ArtworkSampleRequest, ArtworkSource, Document, DocumentNames, LayerMask, Point};
use layer_core::color::{DocumentColor, SampleDepth};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};

pub(super) fn gpu() -> SnapshotGpu {
    static GPU: std::sync::OnceLock<SnapshotGpu> = std::sync::OnceLock::new();
    GPU.get_or_init(|| WgpuRasterizer::new_native_headless(Default::default()).unwrap().snapshot_gpu()).clone()
}

fn document(extent: [u32; 2], pixel: impl Fn(u32, u32) -> [f32; 4]) -> Document {
    document_in(extent, layer_core::color::RgbSpace::Srgb, pixel)
}

pub(super) fn document_in(extent: [u32; 2], space: layer_core::color::RgbSpace, pixel: impl Fn(u32, u32) -> [f32; 4]) -> Document {
    let mut doc = Document::new("Sample", extent[0], extent[1], DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    doc.color = DocumentColor { depth: SampleDepth::F32, space };
    doc.layers[1].visible = false;
    for _ in 0..100 { doc.allocate_layer_id(); }
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
        RasterTile::backed(TileBlob::encode(doc.color.paint_descriptor(), &bytes).unwrap()));
    doc.layers[0].raster = RasterRevision::backed(data);
    doc
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
    let id = doc.layers[0].id;
    doc.layers[0].opacity = 0.25;
    let mut mask = LayerMask::reveal_all(layer_core::LayerId(99), Point::default());
    mask.default_coverage = 0.5;
    doc.layers[0].mask = Some(mask);
    doc.layers[0].properties.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine::translation(Point { x: 3., y: 2. }));
    close(sample(&doc, ArtworkSource::LayerContent(id), [8., 5.], 1).unwrap(), [0.25, 0.25, 2., 1.]);
    close(sample(&doc, ArtworkSource::Visible, [8., 5.], 1).unwrap(), [0.25, 0.25, 2., 0.125]);
}

#[test]
fn artwork_sample_normalizes_large_finite_premultiplied_values_before_summing() {
    let doc = document([101, 101], |_, _| [1e35, -2e35, 3e35, 0.5]);
    close(sample(&doc, ArtworkSource::Visible, [50., 50.], 101).unwrap(), [2e35, -4e35, 6e35, 0.5]);
}

pub(super) fn doubled_effect(id: u64) -> layer_core::Layer {
    let mut program = (*crate::tests::fixture("exposure").program()).clone();
    program.wgsl = "fn double_color(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(2.*c.rgb,c.a);}".into();
    program.entry = "double_color".into();
    program.passes = std::sync::Arc::new([]);
    let mut layer = layer_core::Layer::paint(layer_core::LayerId(id), "Double");
    layer.kind = layer_core::LayerKind::Effect;
    layer.effect = Some(std::sync::Arc::new(layer_core::EffectInstance::new(std::sync::Arc::new(program))));
    layer
}

#[test]
fn artwork_sample_effect_input_excludes_active_and_upper_adjustments_and_baseline_restores_original() {
    let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 1.]);
    let lower = doubled_effect(10);
    let active = doubled_effect(11);
    let mut original = active.clone();
    original.visible = false;
    doc.layers.insert(0, lower);
    doc.layers.insert(0, active.clone());
    doc.layers.insert(0, doubled_effect(12));
    close(sample(&doc, ArtworkSource::Visible, [8.; 2], 1).unwrap(), [1., 2., 4., 1.]);
    close(sample(&doc, ArtworkSource::EffectInput(active.id), [8.; 2], 1).unwrap(), [0.25, 0.5, 1., 1.]);
    close(sample(&doc, ArtworkSource::EffectBaseline(Box::new(original)), [8.; 2], 1).unwrap(), [0.5, 1., 2., 1.]);
}

#[test]
fn artwork_sample_reference_uses_saved_membership_instead_of_visible_upper_layers() {
    let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 1.]);
    doc.reference_layers.insert(doc.layers[0].id);
    doc.layers.insert(0, doubled_effect(10));
    close(sample(&doc, ArtworkSource::Reference, [8.; 2], 1).unwrap(), [0.125, 0.25, 0.5, 1.]);
    close(sample(&doc, ArtworkSource::Visible, [8.; 2], 1).unwrap(), [0.25, 0.5, 1., 1.]);
}

#[test]
fn artwork_sample_effect_input_respects_isolated_group_and_clipped_base() {
    for clipped in [false, true] {
        let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 0.5]);
        let mut group = layer_core::Layer::paint(layer_core::LayerId(20), "Isolated");
        group.kind = layer_core::LayerKind::Group;
        group.properties.blend = layer_core::LayerBlend::Normal;
        doc.layers[0].properties.parent = Some(group.id);
        let mut active = doubled_effect(11);
        active.properties.parent = Some(group.id);
        active.properties.clipped = clipped;
        let active_id = active.id;
        let mut upper = doubled_effect(12);
        upper.properties.parent = Some(group.id);
        upper.properties.clipped = clipped;
        doc.layers.insert(0, active);
        doc.layers.insert(0, upper);
        doc.layers.insert(0, group);
        doc.layers.last_mut().unwrap().visible = true;
        close(sample(&doc, ArtworkSource::EffectInput(active_id), [8.; 2], 1).unwrap(), [0.25, 0.5, 1., 0.5]);
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
    doc.layers[0].raster = document([16, 16], |_, _| [0.75, 0.5, 0.25, 1.]).layers[0].raster.clone();
    assert!(!request.matches_artwork(&doc));
    close(pollster::block_on(gpu().artwork_sample(request, CaptureControl::default())).unwrap(), [0.125, 0.25, 0.5, 1.]);
    close(sample(&doc, ArtworkSource::Visible, [8.; 2], 5).unwrap(), [0.75, 0.5, 0.25, 1.]);
}

#[test]
fn artwork_sample_uses_captured_animation_clock_and_explicit_batch_phase() {
    let mut doc = document([16, 16], |_, _| [0.125, 0.25, 0.5, 1.]);
    let mut program = (*crate::tests::fixture("domain_warp").program()).clone();
    program.wgsl = "fn query_phase(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(fx_time(b),.25,.5,1.);}".into();
    program.entry = "query_phase".into();
    program.passes = std::sync::Arc::new([]);
    let mut effect = layer_core::EffectInstance::new(std::sync::Arc::new(program));
    effect.set("animate", layer_core::EffectValue::Toggle(true)).unwrap();
    effect.set("speed", layer_core::EffectValue::Number(1.)).unwrap();
    let mut layer = layer_core::Layer::paint(layer_core::LayerId(10), "Phase");
    layer.kind = layer_core::LayerKind::Effect;
    layer.effect = Some(std::sync::Arc::new(effect));
    doc.layers.insert(0, layer);
    let mut live = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    live.submit(layer_render::FramePacket { time_seconds: 2., reset_layers: true,
        ..crate::test_support::packet(&doc.layers, [16, 16]) }).unwrap();
    let frozen_gpu = live.snapshot_gpu();
    let mut request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [8.; 2], 1);
    request.time = 2.;
    std::sync::Arc::make_mut(doc.layers[0].effect.as_mut().unwrap()).set("speed", layer_core::EffectValue::Number(2.)).unwrap();
    for elapsed in [2., 3.] {
        live.submit(layer_render::FramePacket { time_seconds: elapsed,
            ..crate::test_support::packet(&doc.layers, [16, 16]) }).unwrap();
    }
    close(pollster::block_on(frozen_gpu.artwork_sample(request.clone(), CaptureControl::default())).unwrap(), [2., 0.25, 0.5, 1.]);
    request.effect_times.push((layer_core::LayerId(10), 6.));
    close(pollster::block_on(frozen_gpu.artwork_sample(request, CaptureControl::default())).unwrap(), [6., 0.25, 0.5, 1.]);
}

#[test]
fn artwork_sample_nonlinear_layer_content_returns_document_primary_linear_color() {
    for space in [layer_core::color::RgbSpace::Srgb, layer_core::color::RgbSpace::DisplayP3, layer_core::color::RgbSpace::AdobeRgb, layer_core::color::RgbSpace::ProPhoto] {
        let mut doc = document_in([20, 20], space, |x, _| [x as f32 / 40., 0.25, 1., 0.5]);
        let id = doc.layers[0].id;
        let map = layer_core::Projective::rect_to_quad(layer_core::Rect::from_extent([20, 20]),
            [[0., 0.], [24., 0.], [18., 20.], [0., 20.]].map(|[x, y]| Point { x, y })).unwrap();
        doc.layers[0].properties.placement = layer_core::LayerPlacement::from_projective(map);
        let source = map.inverse().unwrap().map(Point { x: 8.5, y: 8.5 }).unwrap();
        close(sample(&doc, ArtworkSource::LayerContent(id), [8., 8.], 1).unwrap(),
            [f64::from(source.x - 0.5) / 20., 0.5, 2., 0.5]);
    }
}

#[test]
fn artwork_sample_does_not_unassociate_unrepresentable_individual_texels_before_average() {
    let mut doc = document([5, 5], |_, _| [0.; 4]);
    doc.blend_space = layer_core::BlendSpace::Linear;
    let mut layer = doubled_effect(10);
    let effect = std::sync::Arc::make_mut(layer.effect.as_mut().unwrap());
    let program = std::sync::Arc::make_mut(&mut effect.program);
    program.alpha = layer_core::EffectAlpha::Filter;
    program.wgsl = "fn weighted_color(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(1e10,5e9,2.5e9,select(1.,1e-30,p.x<2.));}".into();
    program.entry = "weighted_color".into();
    doc.layers.insert(0, layer);
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
        let tile = RasterTile::pending(doc.color.paint_descriptor());
        doc.layers[0].raster = if pending_tile {
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
        if pending_tile { tile.publish(Ok(TileBlob::encode(doc.color.paint_descriptor(), &vec![0; 256 * 256 * 16]).unwrap())).unwrap(); }
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
        let tile = RasterTile::pending(doc.color.paint_descriptor());
        doc.layers[0].raster = if pending_tile {
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
