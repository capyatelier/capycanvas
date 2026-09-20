use super::*;
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
        let layer = Layer::paint(LayerId(1), "batched restore");
        r.ensure_document_metadata([9504, 6336], &[layer]).unwrap();
        let first = data(color, 33, &[RasterPlane::Color, RasterPlane::Wetness], 1);
        let before = r.metrics.native_restore_submissions;
        r.restore_raster(LayerId(1), &RasterData::default(), &first).unwrap();
        assert_eq!(r.metrics.native_restore_submissions - before, 66, "cold decodes and scalars keep their early submissions");
        let original = live_pixels(&r);
        assert_eq!(original.len(), 66);
        let before = r.metrics.native_restore_submissions;
        r.restore_raster(LayerId(1), &RasterData::default(), &first).unwrap();
        assert_eq!(r.metrics.native_restore_submissions - before, 36, "cached colors use 16+16+1; scalars are unchanged");
        assert_eq!(original, live_pixels(&r));
        let before = r.metrics.native_restore_submissions;
        r.restore_raster(LayerId(1), &first, &first).unwrap();
        assert_eq!(r.metrics.native_restore_submissions, before, "unchanged backing needs no uploads");

        let second = data(color, 33, &[RasterPlane::Color, RasterPlane::Wetness], 100);
        r.restore_raster(LayerId(1), &first, &second).unwrap();
        assert_ne!(original, live_pixels(&r));
        r.restore_raster(LayerId(1), &second, &first).unwrap();
        assert_eq!(original, live_pixels(&r), "undo must preserve exact working pixels");
        r.restore_raster(LayerId(1), &first, &second).unwrap();
        let previous_pixels = live_pixels(&r);

        // Several valid candidate batches reach the queue before a later digest
        // fails. None may replace a live page, and retry must remain usable.
        let mut corrupt = data(color, 33, &[RasterPlane::Color, RasterPlane::Wetness], 200);
        let mut bad = blob(color, RasterPlane::Wetness, 232);
        bad.digest[0] ^= 1;
        corrupt.tiles.insert(TileKey { plane: RasterPlane::Wetness, coordinate: [10, 2] }, RasterTile::backed(bad));
        let before = r.metrics.native_restore_submissions;
        assert!(r.restore_raster(LayerId(1), &second, &corrupt).is_err());
        assert!(r.metrics.native_restore_submissions > before, "failure must occur after an earlier batch was submitted");
        assert_eq!(previous_pixels, live_pixels(&r), "failed candidates changed live artwork");
        r.restore_raster(LayerId(1), &second, &first).unwrap();
        assert_eq!(original, live_pixels(&r));

        // Removing a tile must not replace untouched GPU pages or submit work.
        let retained = r.paint_layers[0].pages.iter().find(|p| p.coordinate == [1, 0]).unwrap().primary.texture.clone();
        let mut removed = first.clone();
        removed.tiles.remove(&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] });
        let before = r.metrics.native_restore_submissions;
        r.restore_raster(LayerId(1), &first, &removed).unwrap();
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
    r.ensure_document_metadata([9504, 6336], &[Layer::paint(LayerId(1), "mixed restore")]).unwrap();
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
            RasterTile::backed(blob(color, RasterPlane::Color, if i < 8 { i + 1 } else { i + 100 })),
        )).collect(),
        watercolor: None,
    };
    let before = r.metrics.native_restore_submissions;
    r.restore_raster(LayerId(1), &RasterData::default(), &mixed).unwrap();
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
    let mut layer = Layer::paint(LayerId(1), "masked restore");
    layer.mask = Some(layer_core::LayerMask::reveal_all(LayerId(2), layer_core::Point::default()));
    r.ensure_document_metadata([9504, 6336], &[layer]).unwrap();
    let first = data(color, 33, &[RasterPlane::Mask], 1);
    let before = r.metrics.native_restore_submissions;
    r.restore_raster(LayerId(2), &RasterData::default(), &first).unwrap();
    assert_eq!(r.metrics.native_restore_submissions - before, 33);
    assert_eq!(r.layer_masks.pages.len(), 33);
    for (key, tile) in &first.tiles {
        let expected = tile.wait_backing().unwrap().decode().unwrap();
        let actual = crate::layer_tests::page_bytes(&r, &r.layer_masks.pages[&(LayerId(2), key.coordinate)].texture);
        for (actual, expected) in actual.chunks_exact(4).zip(expected.chunks_exact(2)) {
            assert_eq!(f32::from_le_bytes(actual.try_into().unwrap()),
                f32::from(u16::from_le_bytes(expected.try_into().unwrap())) / 65535.);
        }
    }
}

#[test]
#[ignore = "paired physical GPU restore benchmark; release and serial"]
fn native_restore_submission_latency() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    assert_ne!(r.adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    r.ensure_document_metadata([9504, 6336], &[Layer::paint(LayerId(1), "restore timing")]).unwrap();
    eprintln!("restore_adapter {:?}", r.adapter_info());
    let blobs: Vec<_> = (0..128).map(|i| Arc::new(blob(color, RasterPlane::Color, i + 1))).collect();
    let targets: Vec<_> = (0..128).map(|_| create_color_target(&r.device, [PAGE_SIZE; 2], "restore candidate").0).collect();
    let requests: Vec<_> = blobs.iter().zip(&targets).map(|(blob, working)|
        crate::native_tiles::NativeTileRestore { blob, working, space: color.space, destination: color.space }).collect();
    let summary = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        [v[v.len()/2], v[(v.len()*95).div_ceil(100)-1], v[(v.len()*99).div_ceil(100)-1]]
    };
    for round in 0..2 {
        for count in [1, 4, 64, 128] {
            // Zero selects the actual production policy: batch cache hits,
            // submit cold decodes early. Sixteen retains the rejected variant.
            for batch in if round == 0 { [1, 16, 0] } else { [0, 16, 1] } {
                let mut cpu = Vec::new();
                let mut completed = Vec::new();
                let mut drains = Vec::new();
                for frame in 0..150 {
                    let before = r.metrics.native_restore_submissions;
                    let uploads = r.metrics.source_upload_submissions;
                    let start = std::time::Instant::now();
                    if batch == 0 {
                        let mut pending = Vec::new();
                        for request in &requests[..count] {
                            r.restore_native_color_tile(&mut pending, request.blob.clone(), request.working.clone()).unwrap();
                        }
                        r.restore_native_raster_batch(&mut pending).unwrap();
                    } else {
                        for chunk in requests[..count].chunks(batch) { r.restore_native_tiles(chunk).unwrap(); }
                    }
                    let encoded = start.elapsed().as_secs_f64() * 1000.;
                    r.wait_idle().unwrap();
                    let complete = start.elapsed().as_secs_f64() * 1000.;
                    if frame >= 30 {
                        let expected = if batch == 0 { if count <= 64 { count.div_ceil(16) } else { count } }
                            else { count.div_ceil(batch) };
                        assert_eq!(r.metrics.native_restore_submissions - before, expected as u64);
                        cpu.push(encoded); completed.push(complete);
                        drains.push(r.metrics.source_upload_submissions - uploads);
                    }
                }
                eprintln!("restore_result round={round} tiles={count} batch={batch} submissions={} samples=120 cpu_ms={:.4?} completed_ms={:.4?} upload_drains={:?}..{:?} upload_peak_bytes={}",
                    if batch == 0 { if count <= 64 { count.div_ceil(16) } else { count } } else { count.div_ceil(batch) },
                    summary(cpu), summary(completed), drains.iter().min(), drains.iter().max(), r.metrics.source_upload_peak_bytes);
            }
        }
    }
}
