use super::*;
use layer_core::raster::{RasterData, RasterRevision};
use std::collections::BTreeSet;

fn capture(
    r: &WgpuRasterizer,
    target: LayerId,
    previous: &RasterData,
    changed: bool,
) -> (RasterRevision, Option<RasterCapture>) {
    let revision = RasterRevision::pending();
    let dirty = if changed {
        BTreeSet::from([[0, 0]])
    } else {
        BTreeSet::new()
    };
    let ticket = r
        .capture_raster(target, previous, &dirty, &revision)
        .unwrap();
    (revision, ticket)
}

#[test]
fn raster_capture_undo_redo_and_continued_paint_are_exact() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let layers = [Layer::paint(LayerId(1), "ink")];
    submit(
        &mut r,
        &layers,
        &[dab([0.02, 0.1, 0.3, 0.125])],
        &[batch(1)],
        true,
    );
    let first_pixels = r.readback_srgb_rgba8().unwrap();
    let (first, ticket) = capture(&r, LayerId(1), &RasterData::default(), true);
    let worker = std::thread::spawn(move || ticket.unwrap().finish().unwrap());
    // A second edit starts before CPU backing completes; the queued copy must
    // preserve the first revision rather than reading the current texture later.
    submit(
        &mut r,
        &layers,
        &[dab([0.7, 0.2, 0.01, 0.2])],
        &[batch(1)],
        false,
    );
    worker.join().unwrap();
    let first_data = first.wait_data().unwrap();
    let mut document = layer_core::Document::new("raster roundtrip", 128, 128);
    document.layers = layers.to_vec();
    document.layers[0].raster = first.clone();
    let project = layer_core::Project {
        document,
        assets: Default::default(),
    };
    let mut saved = Vec::new();
    project.write(&mut saved).unwrap();
    let reopened = layer_core::Project::read(saved.as_slice(), Default::default()).unwrap();
    let loaded = reopened.document.layers[0].raster.wait_data().unwrap();
    assert_eq!(loaded.tiles.len(), first_data.tiles.len());
    for (key, tile) in &loaded.tiles {
        assert_eq!(
            tile.wait_backing().unwrap().decode().unwrap(),
            first_data.tiles[key]
                .wait_backing()
                .unwrap()
                .decode()
                .unwrap()
        );
    }
    let (second, ticket) = capture(&r, LayerId(1), &first_data, true);
    ticket.unwrap().finish().unwrap();
    let second_data = second.wait_data().unwrap();
    let second_pixels = r.readback_srgb_rgba8().unwrap();
    assert_ne!(first_pixels, second_pixels);
    r.restore_raster(LayerId(1), &second_data, &first_data)
        .unwrap();
    submit(&mut r, &layers, &[], &[], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), first_pixels);
    r.restore_raster(LayerId(1), &first_data, &second_data)
        .unwrap();
    submit(&mut r, &layers, &[], &[], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), second_pixels);
    let (unchanged, ticket) = capture(&r, LayerId(1), &second_data, false);
    assert!(
        ticket.is_none(),
        "unchanged save must issue no GPU readback"
    );
    assert!(
        unchanged
            .wait_data()
            .unwrap()
            .tiles
            .iter()
            .all(|(key, tile)| tile.same_capture(&second_data.tiles[key]))
    );
    submit(
        &mut r,
        &layers,
        &[dab([0.1, 0.3, 0.4, 0.5])],
        &[batch(1)],
        false,
    );
    assert_ne!(r.readback_srgb_rgba8().unwrap(), second_pixels);
    // A new device reconstructs only stored pixels; no stroke exists in the file.
    drop(r);
    let mut r = WgpuRasterizer::new_headless().unwrap();
    submit(&mut r, &reopened.document.layers, &[], &[], true);
    r.restore_raster(LayerId(1), &RasterData::default(), &loaded)
        .unwrap();
    submit(&mut r, &reopened.document.layers, &[], &[], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), first_pixels);
    submit(
        &mut r,
        &reopened.document.layers,
        &[dab([0.3, 0.2, 0.1, 0.25])],
        &[batch(1)],
        false,
    );
    assert_ne!(r.readback_srgb_rgba8().unwrap(), first_pixels);
}

#[test]
fn raster_mask_capture_and_restore_preserve_exact_coverage() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut layer = Layer::paint(LayerId(1), "masked");
    layer.mask = Some(LayerMask::reveal_all(LayerId(9), Point::default()));
    let layers = [layer];
    submit(
        &mut r,
        &layers,
        &[dab([0.1, 0.2, 0.3, 1.])],
        &[batch(1)],
        true,
    );
    let (before, ticket) = capture(&r, LayerId(9), &RasterData::default(), true);
    if let Some(ticket) = ticket {
        ticket.finish().unwrap();
    }
    let mut erase = batch(9);
    erase.style.mode = DabMode::Erase;
    submit(&mut r, &layers, &[dab([1., 1., 1., 0.25])], &[erase], false);
    let masked = r.readback_srgb_rgba8().unwrap();
    let before = before.wait_data().unwrap();
    let (after, ticket) = capture(&r, LayerId(9), &before, true);
    ticket.unwrap().finish().unwrap();
    let after = after.wait_data().unwrap();
    r.restore_raster(LayerId(9), &after, &before).unwrap();
    submit(&mut r, &layers, &[], &[], false);
    assert_ne!(r.readback_srgb_rgba8().unwrap(), masked);
    r.restore_raster(LayerId(9), &before, &after).unwrap();
    submit(&mut r, &layers, &[], &[], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), masked);
}

#[test]
fn opaque_srgb_import_preserves_all_shadow_and_full_ramp_codes() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let id = AssetId::from("test:ramp");
    let pixels: Vec<u8> = (0..128 * 128)
        .flat_map(|i| {
            let c = (i % 256) as u8;
            [c, c, c, 255]
        })
        .collect();
    r.prepare_asset(
        &id,
        HostImage {
            width: 128,
            height: 128,
            stride: 512,
            format: PixelFormat::Rgba8Srgb,
            bytes: &pixels,
        },
    )
    .unwrap();
    let mut layer = Layer::paint(LayerId(1), "ramp");
    layer.asset = Some(id);
    submit(&mut r, &[layer], &[], &[], true);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), pixels);
}
