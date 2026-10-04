use super::*;
use crate::layer_tests::placement::{paint_document,target,set_mask,reveal_all};
fn restore_document(color: DocumentColor) -> layer_core::Document {
    let mut document = paint_document([9504,6336],"native restore");
    let root = document.artwork.root;
    document.artwork.compositions.get_mut(root).unwrap().color = color;
    document
}
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};

fn blob(color: DocumentColor, plane: RasterPlane, seed: u32) -> TileBlob {
    let descriptor = plane.descriptor(color);
    let pixel: Vec<_> = (0..descriptor.channels).flat_map(|channel| {
        let code = if channel == 3 { color.depth.maximum() }
            else { (seed * 251 + u32::from(channel) * 71) & color.depth.maximum() };
        (code as u16).to_le_bytes().into_iter().take(color.depth.bytes())
    }).collect();
    TileBlob::encode(descriptor, &pixel.repeat(PAGE_SIZE.pow(2) as usize)).unwrap()
}

fn data(color: DocumentColor, count: u32, planes: &[RasterPlane], seed: u32) -> RasterData {
    RasterData {
        tiles: planes.iter().flat_map(|&plane| (0..count).map(move |i| (
            TileKey { plane, coordinate: [i % 11, i / 11] },
            RasterTile::backed(blob(color, plane, seed + i)),
        ))).collect(),
        watercolor: None,
    }
}

fn live_pixels(r: &WgpuRasterizer) -> Vec<(RasterPlane, [u32; 2], Vec<u8>)> {
    let layer = &r.paint_layers[0];
    layer.pages.iter().map(|p| (RasterPlane::Color, p.coordinate,
        crate::layer_tests::page_bytes(r, &p.primary.texture)))
        .chain(layer.material_pages.iter().map(|p| (RasterPlane::Wetness, p.coordinate,
            crate::layer_tests::page_bytes(r, &p.wetness.texture)))).collect()
}

#[test]
fn native_raster_restore_batches_keep_pixels_reuse_and_late_failure_atomicity() {
    for color in [DocumentColor::default(), DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 }] {
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        r.source_tiles.get_mut().admit(0);
        let document = restore_document(color);
        let source = target(&document);
        r.ensure_document_metadata([9504, 6336], document.scene()).unwrap();
        let first = data(color, 33, &[RasterPlane::Color, RasterPlane::Wetness], 1);
        let before = r.metrics.native_restore_submissions;
        r.restore_raster(source, &RasterData::default(), &first).unwrap();
        assert_eq!(r.metrics.native_restore_submissions - before, 66, "cold decodes and scalars keep their early submissions");
        let original = live_pixels(&r);
        assert_eq!(original.len(), 66);
        let before = r.metrics.native_restore_submissions;
        r.restore_raster(source, &RasterData::default(), &first).unwrap();
        assert_eq!(r.metrics.native_restore_submissions - before, 36, "cached colors use 16+16+1; scalars are unchanged");
        assert_eq!(original, live_pixels(&r));
        let before = r.metrics.native_restore_submissions;
        r.restore_raster(source, &first, &first).unwrap();
        assert_eq!(r.metrics.native_restore_submissions, before, "unchanged backing needs no uploads");

        let second = data(color, 33, &[RasterPlane::Color, RasterPlane::Wetness], 100);
        r.restore_raster(source, &first, &second).unwrap();
        assert_ne!(original, live_pixels(&r));
        r.restore_raster(source, &second, &first).unwrap();
        assert_eq!(original, live_pixels(&r), "undo must preserve exact working pixels");
        r.restore_raster(source, &first, &second).unwrap();
        let previous_pixels = live_pixels(&r);

        // Several valid candidate batches reach the queue before a later digest
        // fails. None may replace a live page, and retry must remain usable.
        let mut corrupt = data(color, 33, &[RasterPlane::Color, RasterPlane::Wetness], 200);
        let bad = blob(color, RasterPlane::Wetness, 232);
        let bad=crate::test_support::corrupt_tile(bad);
        corrupt.tiles.insert(TileKey { plane: RasterPlane::Wetness, coordinate: [10, 2] }, bad);
        let before = r.metrics.native_restore_submissions;
        assert!(r.restore_raster(source, &second, &corrupt).is_err());
        assert!(r.metrics.native_restore_submissions > before, "failure must occur after an earlier batch was submitted");
        assert_eq!(previous_pixels, live_pixels(&r), "failed candidates changed live artwork");
        r.restore_raster(source, &second, &first).unwrap();
        assert_eq!(original, live_pixels(&r));

        // Removing a tile must not replace untouched GPU pages or submit work.
        let retained = r.paint_layers[0].pages.iter().find(|p| p.coordinate == [1, 0]).unwrap().primary.texture.clone();
        let mut removed = first.clone();
        removed.tiles.remove(&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] });
        let before = r.metrics.native_restore_submissions;
        r.restore_raster(source, &first, &removed).unwrap();
        assert_eq!(r.metrics.native_restore_submissions, before);
        assert_eq!(r.paint_layers[0].pages.len(), 32);
        assert_eq!(r.paint_layers[0].pages.iter().find(|p| p.coordinate == [1, 0]).unwrap().primary.texture, retained);
        assert!(r.metrics.source_upload_peak_bytes <= 16 * 1024 * 1024);
    }
}

#[test]
fn cached_restore_prefix_is_copied_before_cold_decodes_reuse_slots() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    r.source_tiles.get_mut().admit(0);
    let document = restore_document(color);
    let source = target(&document);
    r.ensure_document_metadata([9504, 6336],document.scene()).unwrap();
    let target = create_color_target(&r.device, [PAGE_SIZE; 2], "cache warmup").0;
    let cached: Vec<_> = (1..=64).map(|seed| Arc::new(blob(color, RasterPlane::Color, seed))).collect();
    for blob in &cached {
        r.restore_native_tiles(&[crate::native_tiles::NativeTileRestore {
            blob, working: &target, space: color.space, destination: color.space,
        }]).unwrap();
    }
    let mixed = RasterData {
        tiles: (0..80).map(|i| (
            TileKey { plane: RasterPlane::Color, coordinate: [i / 25, i % 25] },
            if i < 8 { RasterTile::backed_shared(cached[i as usize].clone()) }
                else { RasterTile::backed(blob(color, RasterPlane::Color, i + 100)) },
        )).collect(),
        watercolor: None,
    };
    let before = r.metrics.native_restore_submissions;
    r.restore_raster(source, &RasterData::default(), &mixed).unwrap();
    assert_eq!(r.metrics.native_restore_submissions - before, 73, "one cached prefix, then 72 cold submissions");
    for page in &r.paint_layers[0].pages {
        let index = page.coordinate[0] * 25 + page.coordinate[1];
        let seed = if index < 8 { index + 1 } else { index + 100 };
        let expected: [f32; 4] = std::array::from_fn(|channel| if channel == 3 { 1. } else {
            color.space.decode(f64::from((seed * 251 + channel as u32 * 71) & 65535) / 65535.) as f32
        });
        for pixel in crate::layer_tests::page_bytes(&r, &page.primary.texture).chunks_exact(16) {
            for channel in 0..4 {
                let actual = f32::from_le_bytes(pixel[channel * 4..channel * 4 + 4].try_into().unwrap());
                assert!((actual - expected[channel]).abs() < 2e-7, "tile {index}, channel {channel}");
            }
        }
    }
}

#[test]
fn native_mask_restoration_keeps_existing_scalar_path() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let mut document = restore_document(color);
    set_mask(&mut document,reveal_all([9504,6336],layer_core::Point::default()));
    let source = SourceTarget::Coverage(document.scene().mask(document.scene().order()[0]).unwrap().0.source);
    r.ensure_document_metadata([9504, 6336],document.scene()).unwrap();
    let first = data(color, 33, &[RasterPlane::Mask], 1);
    let before = r.metrics.native_restore_submissions;
    r.restore_raster(source, &RasterData::default(), &first).unwrap();
    assert_eq!(r.metrics.native_restore_submissions - before, 33);
    assert_eq!(r.layer_masks.pages.len(), 33);
    for (key, tile) in &first.tiles {
        let expected = tile.wait_backing().unwrap().decode().unwrap();
        let actual = crate::layer_tests::page_bytes(&r, &r.layer_masks.pages[&(source, key.coordinate)].texture);
        for (actual, expected) in actual.chunks_exact(4).zip(expected.chunks_exact(2)) {
            assert_eq!(f32::from_le_bytes(actual.try_into().unwrap()),
                f32::from(u16::from_le_bytes(expected.try_into().unwrap())) / 65535.);
        }
    }
}
