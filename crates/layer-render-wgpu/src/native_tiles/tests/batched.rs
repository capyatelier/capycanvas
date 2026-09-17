use super::*;

#[test]
fn color_dispatch_slots_and_partial_tail_match_native_reference() {
    slots(false);
}
#[test]
fn native_in_place_color_dispatch_slots_and_partial_tail_match_native_reference() {
    slots(true);
}
fn slots(in_place: bool) {
    let r = if in_place {
        WgpuRasterizer::new_native_headless(Default::default()).unwrap()
    } else {
        WgpuRasterizer::new_headless().unwrap()
    };
    let encoder = if in_place {
        NativeTileEncoder::validated_in_place(&r.device)
    } else {
        NativeTileEncoder::new(&r.device)
    };
    let status = NativeEncodeStatus::new(&r.device);
    let working: Vec<_> = (0..3)
        .map(|_| texture(&r, wgpu::TextureFormat::Rgba32Float))
        .collect();
    let canonical: Vec<_> = if in_place {
        working.clone()
    } else {
        (0..3)
            .map(|_| texture(&r, wgpu::TextureFormat::Rgba32Float))
            .collect()
    };
    for space in [RgbSpace::Srgb, RgbSpace::ProPhoto] {
        let transfer = NativeTransfer::new(&r.device, space).unwrap();
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let encoded: Vec<_> = (0..3).map(|_| texture(&r, format(depth))).collect();
            let seed = vec![0x39; 65536 * 4 * depth.bytes()];
            for alpha in [
                AlphaAssociation::Straight,
                AlphaAssociation::PremultipliedLinear,
            ] {
                let pixels: Vec<Vec<[f32; 4]>> = (0..3)
                    .map(|slot| {
                        (0..65536u32)
                            .map(|i| {
                                let coverage =
                                    [1. / 65535., 0.0625, 0.37, 1.][(i as usize + slot) % 4];
                                let mut pixel = [0.; 4];
                                for c in 0..3 {
                                    let code = (i.wrapping_mul([1, 101, 237][c])
                                        + slot as u32 * 193)
                                        & 65535;
                                    let value = space.decode(f64::from(code) / 65535.) as f32;
                                    pixel[c] = if alpha == AlphaAssociation::Straight {
                                        value * coverage
                                    } else {
                                        value
                                    };
                                }
                                pixel[3] = coverage;
                                pixel
                            })
                            .collect()
                    })
                    .collect();
                let originals: Vec<_> = pixels.iter().map(|p| working_bytes(p)).collect();
                for region in [[0, 0, 256, 256], [1, 3, 253, 251]] {
                    let requests: Vec<_> = (0..3)
                        .map(|i| NativeTileRequest {
                            working: &working[i],
                            encoded: &encoded[i],
                            canonical: &canonical[i],
                            transfer: &transfer,
                            depth,
                            alpha,
                            region,
                        })
                        .collect();
                    let mut together = Vec::new();
                    for grouped in [true, false] {
                        for i in 0..3 {
                            upload(&r, &working[i], &originals[i]);
                            upload(&r, &canonical[i], &originals[i]);
                            upload(&r, &encoded[i], &seed);
                        }
                        let batches = if grouped {
                            vec![encoder.prepare(&r.device, &requests, &status).unwrap()]
                        } else {
                            (0..3)
                                .map(|i| {
                                    encoder
                                        .prepare(&r.device, &requests[i..i + 1], &status)
                                        .unwrap()
                                })
                                .collect()
                        };
                        submit(&r, &encoder, &status, &batches, true);
                        read_status(&r, &status).unwrap();
                        for i in 0..3 {
                            let bytes = page_bytes(&r, &encoded[i]);
                            assert_pixels(
                                &bytes,
                                &pixels[i],
                                depth,
                                space,
                                alpha,
                                region,
                                if depth == SampleDepth::U8 {
                                    0x39
                                } else {
                                    0x3939
                                },
                            );
                            let canonical_bytes = page_bytes(&r, &canonical[i]);
                            if !in_place {
                                assert!(
                                    page_bytes(&r, &working[i]) == originals[i],
                                    "source changed at slot {i}"
                                );
                            }
                            if grouped {
                                together.push((bytes, canonical_bytes));
                            } else {
                                assert!(
                                    together[i] == (bytes, canonical_bytes),
                                    "grouping changed native/canonical samples at slot {i}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
