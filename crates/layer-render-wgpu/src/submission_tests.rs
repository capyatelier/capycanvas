//! Large replay must retain pixels and staging uploads across submission boundaries.
use super::*;

#[test]
#[ignore = "4K multilayer hardware replay; run serially in release mode"]
fn multilayer_4k_fill_replay_matches_incremental_submissions() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let extent = [4096, 4096];
    let layers: Vec<_> = (1..=7)
        .map(|id| {
            let mut layer = Layer::paint(LayerId(id), "replay fixture");
            layer.operations.push(LayerOperation {
                after_stroke: 0,
                coverage: LayerMask::reveal_all(LayerId(100 + id), Point::default()),
                kind: LayerOperationKind::Fill {
                    color: [0.1 + (id % 3) as f32 * 0.3, 0.25, 0.55, 0.2],
                    alpha_locked: false,
                },
            });
            layer
        })
        .collect();
    let batches: Vec<_> = layers
        .iter()
        .map(|layer| DabBatch {
            kind: DabBatchKind::LayerOperation(0),
            dab_count: 0,
            damage: layer.operations[0].bounds(extent),
            ..batch(layer.id.0)
        })
        .collect();
    for count in 1..=layers.len() {
        r.submit(FramePacket {
            view: view(),
            document_extent: extent,
            layers: &layers[..count],
            dabs: &[],
            dab_batches: &batches[count - 1..count],
            reset_layers: count == 1,
            time_seconds: 0.,
            composite_all: true,
        })
        .unwrap();
        r.wait_idle().unwrap();
    }
    let incremental = r.readback_srgb_rgba8().unwrap();
    assert!(incremental.chunks_exact(4).all(|pixel| pixel[3] > 0));
    for telemetry in [false, true] {
        r.set_telemetry_enabled(telemetry);
        r.submit(FramePacket {
            view: view(),
            document_extent: extent,
            layers: &layers,
            dabs: &[],
            dab_batches: &batches,
            reset_layers: true,
            time_seconds: 0.,
            composite_all: true,
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
