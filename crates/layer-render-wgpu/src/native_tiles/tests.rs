use super::*;
use crate::{READBACK_TIMEOUT, WgpuRasterizer, layer_tests::page_bytes};
use layer_core::color::RgbSpace;

fn texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native writeback test"),
        size: wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
fn upload(r: &WgpuRasterizer, texture: &wgpu::Texture, bytes: &[u8]) {
    r.queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256 * texture.format().block_copy_size(None).unwrap()),
            rows_per_image: None,
        },
        texture.size(),
    );
}
fn working_bytes(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}
fn submit(
    r: &WgpuRasterizer,
    encoder: &NativeTileEncoder,
    status: &NativeEncodeStatus,
    batches: &[NativeTileBatch],
    reset: bool,
) {
    let mut commands = r.device.create_command_encoder(&Default::default());
    if reset {
        status.reset(&mut commands);
    }
    {
        let mut pass = commands.begin_compute_pass(&Default::default());
        for batch in batches {
            encoder.encode(&mut pass, batch);
        }
    }
    r.queue.submit([commands.finish()]);
}
fn read_status(
    r: &WgpuRasterizer,
    status: &NativeEncodeStatus,
) -> Result<NativeEncodingStats, GpuRasterError> {
    let read = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("native encoding test status"),
        size: STATUS_BYTES,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut commands = r.device.create_command_encoder(&Default::default());
    commands.copy_buffer_to_buffer(status.buffer(), 0, &read, 0, STATUS_BYTES);
    r.queue.submit([commands.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    read.slice(..)
        .map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = read.get_mapped_range(..).unwrap();
    let result = NativeEncodeStatus::decode(&bytes);
    drop(bytes);
    read.unmap();
    result
}
fn format(depth: SampleDepth) -> wgpu::TextureFormat {
    if depth == SampleDepth::U8 {
        wgpu::TextureFormat::Rgba8Uint
    } else {
        wgpu::TextureFormat::Rgba16Uint
    }
}
fn code(bytes: &[u8], component: usize, depth: SampleDepth) -> u32 {
    if depth == SampleDepth::U8 {
        u32::from(bytes[component])
    } else {
        u32::from(u16::from_le_bytes(
            bytes[component * 2..component * 2 + 2].try_into().unwrap(),
        ))
    }
}
fn reference(
    pixel: [f32; 4],
    depth: SampleDepth,
    space: RgbSpace,
    alpha: AlphaAssociation,
) -> [u32; 4] {
    let max = depth.maximum() as f64;
    let coverage = (f64::from(pixel[3]) * max).round() as u32;
    if coverage == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    out[3] = coverage;
    for c in 0..3 {
        // Reference follows the declared Float32 unassociation, then uses Float64
        // transfer/rounding independently of the GPU table and analytic estimate.
        let limit = if alpha == AlphaAssociation::Straight {
            pixel[3]
        } else {
            1.
        };
        let mut value = pixel[c].clamp(0., limit);
        if alpha == AlphaAssociation::Straight {
            value /= pixel[3];
        }
        out[c] = (space.encode(f64::from(value)).clamp(0., 1.) * max).round() as u32;
    }
    out
}
fn assert_pixels(
    actual: &[u8],
    pixels: &[[f32; 4]],
    depth: SampleDepth,
    space: RgbSpace,
    alpha: AlphaAssociation,
    region: [u32; 4],
    sentinel: u32,
) {
    let mut errors = [0; 4];
    let mut first = None;
    for (i, (bytes, pixel)) in actual
        .chunks_exact(4 * depth.bytes())
        .zip(pixels)
        .enumerate()
    {
        let x = i as u32 % 256;
        let y = i as u32 / 256;
        let inside = x >= region[0]
            && x < region[0] + region[2]
            && y >= region[1]
            && y < region[1] + region[3];
        let expected = if inside {
            reference(*pixel, depth, space, alpha)
        } else {
            [sentinel; 4]
        };
        for c in 0..4 {
            let got = code(bytes, c, depth);
            errors[c] = errors[c].max(got.abs_diff(expected[c]));
            if got != expected[c] && first.is_none() {
                first = Some((i, c, got, expected[c], *pixel));
            }
        }
    }
    assert_eq!(
        errors, [0; 4],
        "{depth:?} {space:?} {alpha:?} first={first:?}"
    );
}

#[test]
fn native_writeback_codes_boundaries_and_partial_tiles() { writeback_corpus(false); }

#[test]
fn native_in_place_writeback_codes_boundaries_and_partial_tiles() { writeback_corpus(true); }

fn writeback_corpus(in_place: bool) {
    let r = if in_place { WgpuRasterizer::new_native_headless(Default::default()).unwrap() } else { WgpuRasterizer::new_headless().unwrap() };
    let encoder = if in_place { NativeTileEncoder::validated_in_place(&r.device) } else { NativeTileEncoder::new(&r.device) };
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = if in_place { working.clone() } else { texture(&r, wgpu::TextureFormat::Rgba32Float) };
    let outputs = [
        texture(&r, format(SampleDepth::U8)),
        texture(&r, format(SampleDepth::U16)),
    ];
    let mut tables = transfer::Tables::default();
    let mut cases = 0;
    for space in RgbSpace::ALL {
        let transfer = tables.prepare(&r.device, space).unwrap();
        for (depth, encoded) in [SampleDepth::U8, SampleDepth::U16]
            .into_iter()
            .zip(&outputs)
        {
            let max = depth.maximum();
            for alpha in [
                AlphaAssociation::Straight,
                AlphaAssociation::PremultipliedLinear,
            ] {
                for mode in 0..10 {
                    let pixels: Vec<[f32; 4]> = (0..65536u32)
                        .map(|i| {
                            let coverage = match mode {
                                0..=4 => {
                                    [1., 1. / max as f32, 2. / max as f32, 17. / max as f32, 0.]
                                        [mode]
                                }
                                5..=7 => {
                                    let boundary = (f64::from(i % max) + 0.5) / f64::from(max);
                                    let rounded = boundary as f32;
                                    [rounded.next_down(), rounded, rounded.next_up()][mode - 5]
                                }
                                8 => 0.25 / max as f32,
                                _ => 1.,
                            };
                            let mut p = [0.; 4];
                            p[3] = coverage;
                            for c in 0..3 {
                                let n = i.wrapping_mul([1, 101, 237][c]) & max;
                                let linear = if mode == 9 {
                                    let boundary =
                                        space.decode((f64::from(n) + 0.5) / f64::from(max)) as f32;
                                    [boundary.next_down(), boundary, boundary.next_up()][c]
                                } else {
                                    space.decode(f64::from(n) / f64::from(max)) as f32
                                };
                                p[c] = if alpha == AlphaAssociation::Straight {
                                    linear * coverage
                                } else {
                                    linear
                                };
                            }
                            if i < 8 && mode == 8 {
                                p[0] = f32::MAX;
                            }
                            p
                        })
                        .collect();
                    let bytes = working_bytes(&pixels);
                    upload(&r, &working, &bytes);
                    upload(&r, &canonical, &bytes);
                    let seed = vec![0x39; 256 * 256 * 4 * depth.bytes()];
                    upload(&r, encoded, &seed);
                    let region = if mode == 4 {
                        [13, 7, 229, 231]
                    } else {
                        [0, 0, 256, 256]
                    };
                    let batch = encoder
                        .prepare(
                            &r.device,
                            &[NativeTileRequest {
                                working: &working,
                                canonical: &canonical,
                                encoded,
                                transfer,
                                depth,
                                alpha,
                                region,
                            }],
                            &status,
                        )
                        .unwrap();
                    submit(&r, &encoder, &status, &[batch], true);
                    read_status(&r, &status).unwrap();
                    assert_pixels(
                        &page_bytes(&r, encoded),
                        &pixels,
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
                    if !in_place { assert!(page_bytes(&r, &working) == bytes, "writeback modified working source"); }
                    let canonical_bytes = page_bytes(&r, &canonical);
                    for (i, encoded) in canonical_bytes.chunks_exact(16).enumerate() {
                        let x = i as u32 % 256;
                        let y = i as u32 / 256;
                        if x < region[0]
                            || x >= region[0] + region[2]
                            || y < region[1]
                            || y >= region[1] + region[3]
                        {
                            assert!(
                                encoded == &bytes[i * 16..i * 16 + 16],
                                "canonical changed outside region at {i}"
                            );
                            continue;
                        }
                        let codes = reference(pixels[i], depth, space, alpha);
                        let coverage = codes[3] as f32 / max as f32;
                        for c in 0..4 {
                            let actual =
                                f32::from_le_bytes(encoded[c * 4..c * 4 + 4].try_into().unwrap());
                            let expected = if c == 3 {
                                coverage
                            } else {
                                let decoded =
                                    space.decode(f64::from(codes[c]) / f64::from(max)) as f32;
                                if alpha == AlphaAssociation::Straight {
                                    decoded * coverage
                                } else {
                                    decoded
                                }
                            };
                            assert!(
                                (actual - expected).abs()
                                    <= 0.00000024 * expected.abs().max(0.0000001),
                                "canonical {depth:?} {space:?} {alpha:?} pixel={i} component={c} actual={actual} expected={expected}"
                            );
                        }
                    }
                    cases += 1;
                }
            }
        }
    }
    eprintln!(
        "NATIVE_WRITEBACK in_place={in_place} cases={cases} pixels={} max_code_error=0",
        cases * 65536
    );
}

#[test]
fn native_restore_writeback_capture_round_trip_preserves_committed_codes() {
    use crate::raster::TileCapture;
    use layer_core::raster::RasterTile;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let encoder = NativeTileEncoder::with_device(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let encoded8 = texture(&r, wgpu::TextureFormat::Rgba8Uint);
    let encoded16 = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    for space in RgbSpace::ALL {
        let transfer = r.prepare_native_transfer(space).unwrap();
        assert_eq!(transfer, r.prepare_native_transfer(space).unwrap());
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let maximum = depth.maximum();
            for association in [
                AlphaAssociation::Straight,
                AlphaAssociation::PremultipliedLinear,
            ] {
                let encoded = if depth == SampleDepth::U8 {
                    &encoded8
                } else {
                    &encoded16
                };
                let pixels: Vec<_> = (0..65536u32)
                    .map(|i| {
                        let a = [0, 1, 2, 17, maximum / 2, maximum][i as usize % 6] as f32
                            / maximum as f32;
                        let rgb: [f32; 3] = std::array::from_fn(|c| {
                            space.decode(
                                f64::from(i.wrapping_mul([1, 101, 237][c]) & maximum)
                                    / f64::from(maximum),
                            ) as f32
                                * a
                        });
                        [rgb[0], rgb[1], rgb[2], a]
                    })
                    .collect();
                upload(&r, &working, &working_bytes(&pixels));
                let request = NativeTileRequest {
                    working: &working,
                    encoded,
                    canonical: &canonical,
                    transfer: &transfer,
                    depth,
                    alpha: association,
                    region: [0, 0, 256, 256],
                };
                let descriptor = request.descriptor();
                let batch = encoder.prepare(&r.device, &[request], &status).unwrap();
                let mut first = None;
                for cycle in 0..4 {
                    submit(&r, &encoder, &status, std::slice::from_ref(&batch), true);
                    let ticket = RasterTile::pending(descriptor);
                    let capture = r
                        .capture_tiles(
                            &[TileCapture {
                                source: crate::raster::CaptureSource::Texture(encoded),
                                tile: ticket.clone(),
                            }],
                            Some(&status),
                        )
                        .unwrap();
                    capture.finish().unwrap();
                    let backing = ticket.wait_backing().unwrap();
                    let bytes = backing.decode().unwrap();
                    if let Some(first) = &first {
                        assert!(
                            first == &bytes,
                            "native code drift {space:?} {depth:?} {association:?} cycle={cycle}"
                        );
                    } else {
                        first = Some(bytes);
                    }
                    r.restore_native_tiles(&[NativeTileRestore {
                        blob: &backing,
                        space,
                        destination: space,
                        working: &working,
                    }])
                    .unwrap();
                    let restored = page_bytes(&r, &working);
                    let canonical_bytes = page_bytes(&r, &canonical);
                    let mut max_error = 0f32;
                    for (a, b) in restored
                        .chunks_exact(4)
                        .zip(canonical_bytes.chunks_exact(4))
                    {
                        let a = f32::from_le_bytes(a.try_into().unwrap());
                        let b = f32::from_le_bytes(b.try_into().unwrap());
                        max_error = max_error.max((a - b).abs());
                    }
                    assert!(max_error <= 0.00000024, "canonical mismatch: {max_error}");
                }
            }
        }
    }
}

#[test]
fn hdr_half_publication_preserves_finite_codes_subnormals_and_canonical_cache() {
    use layer_core::color::{DocumentColor, f16};
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F16 }).unwrap();
    let encoder = NativeTileEncoder::new(&r.device);
    let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let output = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    for alpha in [1., 0.5, 1./65536., 0.] {
        let pixels: Vec<_> = (0..65536u32).map(|code| {
            let v=f16::from_bits(code as u16).to_f32();
            let v=if v.is_finite() {v} else {0.};
            [v*alpha, -v*alpha, 4.*alpha, alpha]
        }).collect();
        upload(&r, &working, &working_bytes(&pixels));
        let request = NativeTileRequest { working: &working, encoded: &output, canonical: &canonical, transfer: &transfer,
            depth: SampleDepth::F16, alpha: AlphaAssociation::Straight, region: [0,0,256,256] };
        let batch=encoder.prepare(&r.device, &[request], &status).unwrap();
        submit(&r, &encoder, &status, &[batch], true);
        assert_eq!(read_status(&r,&status).unwrap().clipped_pixels,0);
        let actual=page_bytes(&r,&output);
        let cache=page_bytes(&r,&canonical);
        for (i, ((bytes,linear),pixel)) in actual.chunks_exact(8).zip(cache.chunks_exact(16)).zip(&pixels).enumerate() {
            let straight=if alpha>0. { [pixel[0]/alpha,pixel[1]/alpha,pixel[2]/alpha,alpha] } else { [0.;4] };
            let expected=layer_core::color::hdr::encode_pixel(straight).unwrap();
            for c in 0..4 {
                let bits=u16::from_le_bytes(bytes[c*2..c*2+2].try_into().unwrap());
                // Signed zero of premultiplied RGB is not an artwork distinction.
                if expected[c]&32767 != 0 { assert_eq!(bits,expected[c],"pixel={i} channel={c} alpha={alpha}"); }
                else { assert_eq!(bits&32767,0); }
                let decoded=f32::from_le_bytes(linear[c*4..c*4+4].try_into().unwrap());
                let value=f16::from_bits(bits).to_f32()*if c<3 {alpha} else {1.};
                assert_eq!(decoded,value,"canonical pixel={i} channel={c}");
            }
        }
    }
    for pixel in [[65505.,0.,0.,1.],[-65505.,0.,0.,1.],[f32::NAN,0.,0.,1.],[1.,0.,0.,-0.1]] {
        upload(&r,&working,&working_bytes(&vec![pixel;65536]));
        let batch=encoder.prepare(&r.device,&[NativeTileRequest { working:&working, encoded:&output, canonical:&canonical, transfer:&transfer,
            depth:SampleDepth::F16,alpha:AlphaAssociation::Straight,region:[0,0,256,256] }],&status).unwrap();
        submit(&r,&encoder,&status,&[batch],true);
        assert!(read_status(&r,&status).is_err());
    }
}

#[test]
fn float32_publication_retains_precision_range_and_rejects_unassociation_overflow() {
    use layer_core::color::DocumentColor;
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F32 }).unwrap();
    let encoder = NativeTileEncoder::new(&r.device);
    let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let output = texture(&r, wgpu::TextureFormat::Rgba32Uint);
    for region in [[0,0,256,256], [5,7,11,13]] {
        let pixels: Vec<_> = (0..65536).map(|i| {
            let a = [1., 0.5, 1./65536.][i%3];
            [1.0000001*a, -100000.125*a, 1e30*a, a]
        }).collect();
        upload(&r, &working, &working_bytes(&pixels));
        let batch = encoder.prepare(&r.device, &[NativeTileRequest { working: &working, encoded: &output, canonical: &canonical, transfer: &transfer,
            depth: SampleDepth::F32, alpha: AlphaAssociation::Straight, region }], &status).unwrap();
        submit(&r, &encoder, &status, &[batch], true);
        assert_eq!(read_status(&r,&status).unwrap().clipped_pixels, 0);
        let actual = page_bytes(&r, &output);
        for y in region[1]..region[1]+region[3] { for x in region[0]..region[0]+region[2] {
            let i = (y*256+x) as usize;
            let p = pixels[i];
            let expected = [p[0]/p[3],p[1]/p[3],p[2]/p[3],p[3]];
            assert_eq!(&actual[i*16..][..16], working_bytes(&[expected]));
        }}
    }
    for pixel in [[f32::MAX,0.,0.,0.125],[f32::NAN,0.,0.,1.],[0.,0.,0.,1.1]] {
        upload(&r, &working, &working_bytes(&vec![pixel;65536]));
        let batch = encoder.prepare(&r.device, &[NativeTileRequest { working: &working, encoded: &output, canonical: &canonical, transfer: &transfer,
            depth: SampleDepth::F32, alpha: AlphaAssociation::Straight, region: [0,0,256,256] }], &status).unwrap();
        submit(&r,&encoder,&status,&[batch],true);
        assert!(read_status(&r,&status).is_err());
    }
}
