use super::*;
use crate::test_support::{page_texture as texture, upload_page as upload};
use layer_core::color::SampleDepth;

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
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let native8 = texture(&r, wgpu::TextureFormat::Rgba8Uint);
    let native16 = texture(&r, wgpu::TextureFormat::Rgba16Uint);
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
    let coverage = wgpu::util::DeviceExt::create_buffer_init(&*r.device, &wgpu::util::BufferInitDescriptor {
        label: Some("packed coverage fixture"),
        contents: &mask,
        usage: wgpu::BufferUsages::COPY_SRC,
    });
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
        source: crate::raster::CaptureSource::Packed(&coverage),
        tile: RasterTile::pending(PixelDescriptor::COVERAGE8),
    });
    let capture = r.capture_tiles(&copies, Some(&status)).unwrap();
    assert_eq!(capture.chunks.len(), 2);
    assert_eq!(
        capture.chunks.iter().chain(&capture.validation).map(|c| c.buffer.size()).sum::<u64>(),
        20 * 1024 * 1024 + STATUS_BYTES
    );
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

