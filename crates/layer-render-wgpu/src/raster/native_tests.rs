use super::*;
use crate::native_tiles::{NativeTileEncoder, NativeTileRequest, NativeTransfer};
use layer_core::color::{SampleDepth, RgbSpace};

fn texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native capture fixture"),
        size: wgpu::Extent3d {
            width: PAGE_SIZE,
            height: PAGE_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | if matches!(
                format,
                wgpu::TextureFormat::Rgba8UnormSrgb | wgpu::TextureFormat::R8Unorm
            ) {
                wgpu::TextureUsages::empty()
            } else {
                wgpu::TextureUsages::STORAGE_BINDING
            },
        view_formats: &[],
    })
}
fn upload(r: &WgpuRasterizer, t: &wgpu::Texture, bytes: &[u8]) {
    r.queue.write_texture(
        t.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(PAGE_SIZE * t.format().block_copy_size(None).unwrap()),
            rows_per_image: None,
        },
        t.size(),
    );
}
fn descriptor(depth: SampleDepth) -> PixelDescriptor {
    PixelDescriptor {
            sample: layer_core::color::SampleType::Unsigned,
        channels: 4,
        bits_per_channel: depth.bits(),
        encoding: TransferEncoding::Profile,
        alpha: AlphaAssociation::Straight,
    }
}

#[test]
fn mixed_native_capture_is_exact_across_chunks_and_later_writes() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let native8 = texture(&r, wgpu::TextureFormat::Rgba8Uint);
    let native16 = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    let coverage = texture(&r, wgpu::TextureFormat::R8Unorm);
    let stored8: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE)
        .flat_map(|i| [i as u8, (i >> 8) as u8, (i * 113) as u8, (i * 73) as u8])
        .collect();
    let stored16: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE)
        .flat_map(|i| {
            [
                i as u16,
                (i * 113) as u16,
                (i * 317) as u16,
                (i * 73) as u16,
            ]
        })
        .flat_map(u16::to_le_bytes)
        .collect();
    let mask: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE).map(|i| (i * 73) as u8).collect();
    upload(&r, &native8, &stored8);
    upload(&r, &native16, &stored16);
    upload(&r, &coverage, &mask);
    let status = NativeEncodeStatus::new(&r.device);
    let mut copies: Vec<_> = (0..35)
        .map(|_| TileCapture {
            source: crate::raster::CaptureSource::Texture(&native16),
            tile: RasterTile::pending(descriptor(SampleDepth::U16)),
        })
        .collect();
    copies.insert(
        31,
        TileCapture {
            source: crate::raster::CaptureSource::Texture(&native8),
            tile: RasterTile::pending(descriptor(SampleDepth::U8)),
        },
    );
    copies.push(TileCapture {
        source: crate::raster::CaptureSource::Texture(&coverage),
        tile: RasterTile::pending(PixelDescriptor::COVERAGE8),
    });
    let capture = r.capture_tiles(&copies, Some(&status)).unwrap();
    assert_eq!(capture.chunks.len(), 2);
    assert_eq!(capture.staging_bytes, 20 * 1024 * 1024 + STATUS_BYTES);
    assert!(copies.iter().all(|c| c.tile.try_backing().is_none()));
    // Captures own the submitted snapshot even if the next edit overwrites GPU
    // storage before the worker maps or compresses the previous revision.
    upload(&r, &native16, &vec![0; stored16.len()]);
    std::thread::spawn(move || capture.finish())
        .join()
        .unwrap()
        .unwrap();
    for copy in &copies {
        let blob = copy.tile.wait_backing().unwrap();
        assert_eq!(blob.descriptor, copy.tile.descriptor());
        let expected = if matches!(copy.source, CaptureSource::Texture(t) if t == &native16) {
            &stored16
        } else if matches!(copy.source, CaptureSource::Texture(t) if t == &native8) {
            &stored8
        } else {
            &mask
        };
        assert!(
            blob.decode().unwrap() == *expected,
            "capture changed native samples"
        );
    }
    assert_eq!(r.raster_buffers.working.load(Ordering::Relaxed), 0);
    assert!(r.raster_buffers.bytes.load(Ordering::Relaxed) <= 64 * 1024 * 1024);
}

#[test]
fn native_gpu_failure_rejects_every_capture_before_backing_and_retry_succeeds() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let encoded = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    let transfer = NativeTransfer::new(&r.device, RgbSpace::ProPhoto).unwrap();
    let encoder = NativeTileEncoder::new(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let batch = encoder
        .prepare(
            &r.device,
            &[NativeTileRequest {
                working: &working,
                canonical: &canonical,
                encoded: &encoded,
                transfer: &transfer,
                depth: SampleDepth::U16,
                alpha: AlphaAssociation::Straight,
                region: [0, 0, PAGE_SIZE, PAGE_SIZE],
            }],
            &status,
        )
        .unwrap();
    for fail in [true, false] {
        let pixels: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE)
            .flat_map(|i| {
                let code = i as f64 / 65535.;
                let value = RgbSpace::ProPhoto.decode(code) as f32;
                [
                    if fail && i == PAGE_SIZE * PAGE_SIZE - 1 {
                        f32::NAN
                    } else {
                        value
                    },
                    value,
                    value,
                    1.,
                ]
            })
            .flat_map(f32::to_le_bytes)
            .collect();
        upload(&r, &working, &pixels);
        let mut commands = r.device.create_command_encoder(&Default::default());
        status.reset(&mut commands);
        {
            let mut pass = commands.begin_compute_pass(&Default::default());
            encoder.encode(&mut pass, &batch);
        }
        r.queue.submit([commands.finish()]);
        let copies: Vec<_> = (0..33)
            .map(|_| TileCapture {
                source: crate::raster::CaptureSource::Texture(&encoded),
                tile: RasterTile::pending(descriptor(SampleDepth::U16)),
            })
            .collect();
        let capture = r.capture_tiles(&copies, Some(&status)).unwrap();
        assert_eq!(capture.chunks.len(), 2);
        // Reusing/resetting the GPU status for the next publication must not
        // erase the captured failure before its worker consumes it.
        let mut commands = r.device.create_command_encoder(&Default::default());
        status.reset(&mut commands);
        r.queue.submit([commands.finish()]);
        let result = std::thread::spawn(move || capture.finish()).join().unwrap();
        if fail {
            assert!(result.is_err());
            assert!(
                copies
                    .iter()
                    .all(|c| matches!(c.tile.try_backing(), Some(Err(_))))
            );
        } else {
            result.unwrap();
            for copy in &copies {
                let raw = copy.tile.wait_backing().unwrap().decode().unwrap();
                for (i, pixel) in raw.chunks_exact(8).enumerate() {
                    for c in 0..4 {
                        let code = u16::from_le_bytes(pixel[c * 2..c * 2 + 2].try_into().unwrap());
                        assert_eq!(code, if c == 3 { 65535 } else { i as u16 });
                    }
                }
            }
        }
    }
    let cancelled = [TileCapture {
        source: crate::raster::CaptureSource::Texture(&encoded),
        tile: RasterTile::pending(descriptor(SampleDepth::U16)),
    }];
    drop(r.capture_tiles(&cancelled, Some(&status)).unwrap());
    assert!(matches!(cancelled[0].tile.try_backing(), Some(Err(_))));
}

#[test]
fn native_capture_preflights_descriptors_tickets_and_actual_staging_budget() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let native8 = texture(&r, wgpu::TextureFormat::Rgba8Uint);
    let native16 = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    let make = |texture, depth| TileCapture {
        source: crate::raster::CaptureSource::Texture(texture),
        tile: RasterTile::pending(descriptor(depth)),
    };
    assert!(r.capture_tiles(&[], None).is_err());
    let mut copies = vec![
        make(&native16, SampleDepth::U16),
        make(&native8, SampleDepth::U16),
    ];
    assert!(r.capture_tiles(&copies, None).is_err());
    assert!(copies.iter().all(|c| c.tile.try_backing().is_none()));
    copies[1] = make(&native16, SampleDepth::U16);
    copies[1].tile = copies[0].tile.clone();
    assert!(r.capture_tiles(&copies, None).is_err());
    // 252.5 MiB of raw payload would fit, but these boundaries need sixteen
    // 16 MiB chunks plus one 512 KiB chunk. Reject before allocating staging.
    let mut mixed = Vec::new();
    for _ in 0..16 {
        mixed.extend((0..31).map(|_| make(&native16, SampleDepth::U16)));
        mixed.push(make(&native8, SampleDepth::U8));
    }
    mixed.push(make(&native16, SampleDepth::U16));
    assert_eq!(
        mixed.iter().map(|c| c.byte_len().unwrap()).sum::<u64>(),
        252 * 1024 * 1024 + 512 * 1024
    );
    assert!(r.capture_tiles(&mixed, None).is_err());
    assert!(mixed.iter().all(|c| c.tile.try_backing().is_none()));
    assert_eq!(r.raster_buffers.bytes.load(Ordering::Relaxed), 0);
}

#[test]
#[ignore = "hardware native capture submission, worker compression and staging benchmark"]
fn native_capture_workloads() {
    use std::time::Instant;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.prepare_source_backing().unwrap();
    while !r.raster_ready() {
        std::thread::sleep(Duration::from_millis(1));
    }
    let status = NativeEncodeStatus::new(&r.device);
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let encoded = texture(
            &r,
            if depth == SampleDepth::U8 {
                wgpu::TextureFormat::Rgba8Uint
            } else {
                wgpu::TextureFormat::Rgba16Uint
            },
        );
        for noise in [false, true] {
            let bytes: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE * 4)
                .flat_map(|i| {
                    let mut value = i;
                    if noise {
                        value ^= value >> 16;
                        value = value.wrapping_mul(0x7feb352d);
                        value ^= value >> 15;
                        value = value.wrapping_mul(0x846ca68b);
                        value ^= value >> 16;
                    }
                    value.to_le_bytes().into_iter().take(depth.bytes())
                })
                .collect();
            upload(&r, &encoded, &bytes);
            r.queue.submit([]);
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
            for count in [1, 16, 64] {
                let mut times = Vec::new();
                let mut staging = 0;
                let mut compressed = 0;
                for iteration in 0..70 {
                    let start = Instant::now();
                    let copies: Vec<_> = (0..count)
                        .map(|_| TileCapture {
                            source: crate::raster::CaptureSource::Texture(&encoded),
                            tile: RasterTile::pending(descriptor(depth)),
                        })
                        .collect();
                    let capture = r.capture_tiles(&copies, Some(&status)).unwrap();
                    staging = capture.staging_bytes;
                    let worker = r.raster.as_ref().unwrap().worker.as_ref().unwrap();
                    worker.submit(vec![capture]).unwrap();
                    let cpu = start.elapsed().as_secs_f64() * 1000.;
                    compressed = copies
                        .iter()
                        .map(|c| c.tile.wait_backing().unwrap().resident_bytes())
                        .sum::<usize>();
                    while worker.pending.load(Ordering::Acquire) != 0 {
                        std::thread::yield_now();
                    }
                    let complete = start.elapsed().as_secs_f64() * 1000.;
                    if iteration == 0 {
                        eprintln!(
                            "NATIVE_CAPTURE_COLD depth={depth:?} noise={noise} tiles={count} cpu_ms={cpu:.4} host_backed_ms={complete:.4}"
                        );
                        assert!(copies[0].tile.wait_backing().unwrap().decode().unwrap() == bytes);
                    }
                    if iteration >= 20 {
                        times.push([cpu, complete]);
                    }
                }
                let mut percentiles = [[0.; 2]; 2];
                for axis in 0..2 {
                    times.sort_by(|a, b| a[axis].total_cmp(&b[axis]));
                    percentiles[axis] = [times[47][axis], times[49][axis]];
                }
                eprintln!(
                    "NATIVE_CAPTURE_BENCH depth={depth:?} noise={noise} tiles={count} samples=50 cpu_p95_p99={:.4?} host_backed_p95_p99={:.4?} staging_bytes={staging} cached_staging_bytes={} worker_scratch_bytes={} compressed_bytes={compressed}",
                    percentiles[0],
                    percentiles[1],
                    r.raster_buffers.bytes.load(Ordering::Relaxed),
                    r.raster_buffers.working.load(Ordering::Relaxed)
                );
            }
        }
    }
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|l| l.starts_with("VmHWM:") || l.starts_with("VmRSS:"))
        {
            eprintln!("{line}");
        }
    }
}

#[test]
fn staging_prefill_does_not_prevent_reusing_small_capture_buffers() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.prepare_source_backing().unwrap();
    while !r.raster_ready() {
        std::thread::sleep(Duration::from_millis(1));
    }
    let pool = &r.raster_buffers;
    assert_eq!(pool.bytes.load(Ordering::Relaxed), 64 * 1024 * 1024);
    let status = pool.take(&r.device, STATUS_BYTES);
    let small = pool.take(&r.device, PAGE_BYTES);
    pool.put(status.clone());
    pool.put(small.clone());
    let again_status = pool.take(&r.device, STATUS_BYTES);
    let again_small = pool.take(&r.device, PAGE_BYTES);
    assert_eq!(status, again_status);
    assert_eq!(small, again_small);
    pool.put(again_status);
    pool.put(again_small);
    assert!(pool.bytes.load(Ordering::Relaxed) <= 64 * 1024 * 1024);
}
