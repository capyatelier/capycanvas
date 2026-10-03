use crate::{WgpuRasterizer, effect_analysis::Lease, snapshot::CaptureControl};
use layer_core::{ArtworkQuery, ArtworkSource, EffectInstance, EffectValue, Layer, LayerKind};
use std::sync::Arc;

fn document() -> layer_core::Document {
    let mut document = crate::artwork_sample_tests::document_in([31, 23], layer_core::color::RgbSpace::Srgb,
        |x, y| [0.01 + x as f32 / 255., 0.02 + y as f32 / 255., 0.08, 1.]);
    let mut layer = Layer::paint(document.allocate_layer_id(), "Shadows");
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(EffectInstance::new(crate::tests::fixture("shadows_highlights").program())));
    document.active_layer = layer.id;
    document.layers.insert(0, layer);
    document
}
fn retained(renderer: &WgpuRasterizer) -> u64 { *renderer.device.analysis_memory.lock().unwrap() }

#[test]
fn analysis_lease_reserve_refusal_split_and_shared_drop_account_exactly() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let baseline = retained(&renderer);
    let mut lease = Lease::reserve(&renderer.device, 4096).unwrap();
    assert_eq!(retained(&renderer), baseline + 4096);
    assert!(Lease::reserve(&renderer.device, u64::MAX).is_err());
    assert_eq!(retained(&renderer), baseline + 4096);
    let output = lease.split(1024);
    let handle = output.clone();
    drop(lease);
    assert_eq!(retained(&renderer), baseline + 1024);
    drop(output);
    assert_eq!(retained(&renderer), baseline + 1024);
    drop(handle);
    assert_eq!(retained(&renderer), baseline);
}

#[test]
fn analysis_guide_reuses_one_resource_for_100_own_edits_and_snapshot_handles_retain_lease() {
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let baseline = retained(&renderer);
    let mut document = document();
    let target = document.active_layer;
    let query = |document: &layer_core::Document| ArtworkQuery::new(document, ArtworkSource::EffectInput(target));
    let candidate = pollster::block_on(renderer.snapshot_gpu().effect_analysis(query(&document), CaptureControl::default())).unwrap();
    assert_eq!(candidate.entries.len(), 1);
    let identity = Arc::as_ptr(&candidate.entries[0]) as usize;
    let bytes = candidate.entries[0].resource.buffer.size();
    assert_eq!(bytes, 16 + 31 * 23 * 16);
    renderer.apply_effect_analysis(candidate);
    assert_eq!(retained(&renderer), baseline + bytes);
    for amount in 0..100 {
        Arc::make_mut(document.layers[0].effect.as_mut().unwrap()).set("shadows", EffectValue::Number(amount as f32)).unwrap();
        let candidate = pollster::block_on(renderer.snapshot_gpu().effect_analysis(query(&document), CaptureControl::default())).unwrap();
        assert_eq!(candidate.entries.len(), 1);
        assert_eq!(Arc::as_ptr(&candidate.entries[0]) as usize, identity, "own amount {amount}");
        renderer.apply_effect_analysis(candidate);
        assert_eq!(retained(&renderer), baseline + bytes);
    }
    let capture = renderer.snapshot_gpu();
    renderer.effect_analyses.clear();
    assert_eq!(retained(&renderer), baseline + bytes);
    let second = capture.clone();
    drop(capture);
    assert_eq!(retained(&renderer), baseline + bytes);
    drop(second);
    assert_eq!(retained(&renderer), baseline);
    eprintln!("P25_GUIDE_OBSERVER edits=100 unique_published_resources=1 reused_candidates=100 retained_bytes={bytes} final_bytes={baseline}");
}

#[test]
fn analysis_cancelled_and_invalid_requests_leave_no_lease() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let baseline = retained(&renderer);
    let document = document();
    let control = CaptureControl::default();
    control.cancel();
    let query = ArtworkQuery::new(&document, ArtworkSource::EffectInput(document.active_layer));
    assert!(pollster::block_on(renderer.snapshot_gpu().effect_analysis(query, control)).is_err());
    assert_eq!(retained(&renderer), baseline);
    let query = ArtworkQuery::new(&document, ArtworkSource::Visible);
    assert!(pollster::block_on(renderer.snapshot_gpu().effect_analysis(query, CaptureControl::default())).is_err());
    assert_eq!(retained(&renderer), baseline);
}

#[test]
fn analysis_in_flight_cancellation_releases_reserved_and_unpublished_storage() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let baseline = retained(&renderer);
    let mut document = document();
    document.width = 769;
    document.height = 513;
    let query = ArtworkQuery::new(&document, ArtworkSource::EffectInput(document.active_layer));
    let control = CaptureControl::default();
    let worker_control = control.clone();
    let gpu = renderer.snapshot_gpu();
    let worker = std::thread::spawn(move || pollster::block_on(gpu.effect_analysis(query, worker_control)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let peak = loop {
        let bytes = retained(&renderer);
        if bytes > baseline { break bytes; }
        assert!(!worker.is_finished(), "guide completed without an observable reservation");
        assert!(std::time::Instant::now() < deadline, "guide reservation did not start");
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    let cancelled = std::time::Instant::now();
    control.cancel();
    assert!(worker.join().unwrap().is_err());
    assert_eq!(retained(&renderer), baseline);
    assert!(renderer.effect_analyses.is_empty());
    eprintln!("P25_GUIDE_CANCEL observed_reserved_bytes={peak} release_ms={:.3}", cancelled.elapsed().as_secs_f64() * 1000.);
}

#[test]
fn analysis_failed_source_publication_releases_all_storage() {
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileKey};
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let baseline = retained(&renderer);
    let mut document = document();
    let failed = RasterTile::pending(document.color.paint_descriptor());
    failed.publish(Err("analysis source publication failed".into())).unwrap();
    let mut data = RasterData::default();
    data.tiles.insert(TileKey {plane: RasterPlane::Color, coordinate: [0, 0]}, failed);
    document.layers.iter_mut().find(|layer| layer.kind == LayerKind::Paint).unwrap().raster = RasterRevision::backed(data);
    let query = ArtworkQuery::new(&document, ArtworkSource::EffectInput(document.active_layer));
    let result = pollster::block_on(renderer.snapshot_gpu().effect_analysis(query, CaptureControl::default()));
    assert!(matches!(result, Err(ref error) if error.contains("analysis source publication failed")));
    assert_eq!(retained(&renderer), baseline);
    assert!(renderer.effect_analyses.is_empty());
}
