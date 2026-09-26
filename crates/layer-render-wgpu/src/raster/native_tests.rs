use super::*;
use layer_core::color::SampleDepth;

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

