//! Large replay must retain pixels and staging uploads across submission boundaries.
use super::*;

#[test]
fn an_intermediate_step_whose_submission_is_retired_fails() {
    let extent = [128; 2];
    let document = paint_document(extent, "retired step");
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let frame = packet(document.scene(), extent);
    crate::test_support::submit_settled(&mut r, frame);
    let ink = dab([0.1, 0.3, 0.8, 1.]);
    let stroke = DabBatch { stroke_end: false, ..crate::test_support::dab_batch(target(&document), crate::tests::test_style(BrushExecution::Dry), ink.bounds()) };
    let step = FramePacket { dabs: std::slice::from_ref(&ink), dab_batches: std::slice::from_ref(&stroke), commit_rasters: false, composite_all: false, ..frame };
    crate::test_support::submit_ready(&mut r, step).unwrap();
    r.wait_idle().unwrap();
    r.device.initialize_poll_owner().unwrap();
    crate::pipeline_device::DeviceRecipes::from(&r.device).poll_binding().retire_lost(||{});
    let submissions = r.metrics.submissions;
    assert!(r.submit(step).is_err(), "an intermediate step that was never submitted reports success");
    assert_eq!(r.metrics.submissions, submissions);
}

#[test]
#[ignore = "4K multilayer hardware replay; run serially in release mode"]
fn multilayer_4k_fill_replay_matches_incremental_submissions() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let extent = [4096, 4096];
    let mut artwork = layer_core::authored::Artwork::new(extent).unwrap();
    let mut owners = Vec::new();
    let mut batches = Vec::new();
    for id in 1..=7 {
        let (owner, target) = crate::test_support::add_paint(&mut artwork, "replay fixture", extent);
        let SourceTarget::Paint(source) = target else { unreachable!() };
        let operation = RasterOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage: reveal_all(extent, [0, 0]),
            kind: RasterOperationKind::Fill { edges: None, underfill: None, texture: Default::default(),
                color: [0.1 + (id % 3) as f32 * 0.3, 0.25, 0.55, 0.2],
                alpha_locked: false,
            },
        };
        batches.push(DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0,
            damage: operation.bounds(extent), ..batch(target) });
        artwork.paint.get_mut(source).unwrap().operations = Arc::new(vec![operation]);
        owners.push(owner);
    }
    let full = Document::from_artwork(artwork).unwrap();
    for count in 1..=owners.len() {
        let mut partial = full.artwork.clone();
        let root = partial.compositions.get(partial.root).unwrap().result;
        partial.stacks.get_mut(root).unwrap().entries = owners[..count].to_vec();
        let partial = Document::from_artwork(partial).unwrap();
        crate::test_support::submit_ready(&mut r, FramePacket {
            view: view(),
            dab_batches: &batches[count - 1..count],
            reset_layers: count == 1,
            ..packet(partial.scene(), extent)
        }).unwrap();
        r.wait_idle().unwrap();
    }
    let incremental = r.readback_srgb_rgba8().unwrap();
    assert!(incremental.as_chunks::<4>().0.iter().all(|pixel| pixel[3] > 0));
    for telemetry in [false, true] {
        r.set_telemetry_enabled(telemetry);
        crate::test_support::submit_ready(&mut r, FramePacket {
            view: view(),
            dab_batches: &batches,
            reset_layers: true,
            ..packet(full.scene(), extent)
        })
        .unwrap();
        r.wait_idle().unwrap();
        let replay = r.readback_srgb_rgba8().unwrap();
        assert_eq!(replay.len(), incremental.len());
        assert_eq!(
            replay.iter().zip(&incremental).position(|(a, b)| a != b),
            None,
            "Restoring all layers together must preserve every incremental pixel (telemetry={telemetry})"
        );
    }
}
