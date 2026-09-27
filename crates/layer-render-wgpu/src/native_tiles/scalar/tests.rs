use super::*;
use crate::{
    WgpuRasterizer,
    layer_tests::page_bytes,
    raster::{CaptureSource, TileCapture},
};
use layer_core::raster::RasterTile;

fn texture(r: &WgpuRasterizer) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scalar native test"),
        size: wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn upload(r: &WgpuRasterizer, t: &wgpu::Texture, values: &[f32]) {
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    r.queue.write_texture(
        t.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(1024),
            rows_per_image: None,
        },
        t.size(),
    );
}
fn buffer(r: &WgpuRasterizer, depth: SampleDepth) -> wgpu::Buffer {
    r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("packed native scalar fixture"),
        size: 65536 * depth.bytes() as u64,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
fn submit(
    r: &WgpuRasterizer,
    encoder: &NativeScalarEncoder,
    status: &NativeEncodeStatus,
    batch: &NativeScalarBatch,
) {
    let mut commands = r.device.create_command_encoder(&Default::default());
    status.reset(&mut commands);
    {
        let mut pass = commands.begin_compute_pass(&Default::default());
        encoder.encode(&mut pass, batch);
    }
    r.queue.submit([commands.finish()]);
}
fn capture(
    r: &WgpuRasterizer,
    buffer: &wgpu::Buffer,
    descriptor: PixelDescriptor,
    status: &NativeEncodeStatus,
) -> Vec<u8> {
    let tile = RasterTile::pending(descriptor);
    r.capture_tiles(
        &[TileCapture {
            source: CaptureSource::Packed(buffer),
            tile: tile.clone(),
        }],
        Some(status),
    )
    .unwrap()
    .finish()
    .unwrap();
    tile.wait_backing().unwrap().decode().unwrap()
}
fn code(bytes: &[u8], i: usize, depth: SampleDepth) -> u32 {
    if depth == SampleDepth::U8 {
        bytes[i] as u32
    } else {
        u16::from_le_bytes(bytes[i * 2..i * 2 + 2].try_into().unwrap()) as u32
    }
}

#[test]
fn scalar_writeback_preserves_every_code_half_neighbors_and_partial_packed_words() { scalar_corpus(false); }
#[test]
fn native_in_place_scalar_writeback_preserves_every_code_half_neighbors_and_partial_packed_words() { scalar_corpus(true); }
fn scalar_corpus(in_place: bool) {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let encoder = if in_place { NativeScalarEncoder::validated_in_place(&r.device) } else { NativeScalarEncoder::with_device(&r.device) };
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r);
    let canonical = if in_place { working.clone() } else { texture(&r) };
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let encoded = buffer(&r, depth);
        let maximum = depth.maximum();
        for pattern in 0..4 {
            let values: Vec<f32> = (0..65536u32)
                .map(|i| {
                    let c = i % (maximum + 1);
                    if pattern == 0 {
                        c as f32 / maximum as f32
                    } else {
                        let half = ((c.min(maximum - 1) as f64 + 0.5) / maximum as f64) as f32;
                        match pattern {
                            1 => half.next_down(),
                            2 => half,
                            _ => half.next_up(),
                        }
                    }
                })
                .collect();
            upload(&r, &working, &values);
            for region in [
                [0, 0, 256, 256],
                [1, 7, 253, 243],
                [3, 255, 1, 1],
                [255, 0, 1, 256],
                [17, 31, 0, 71],
            ] {
                r.queue
                    .write_buffer(&encoded, 0, &vec![0xa5; encoded.size() as usize]);
                upload(&r, &working, &values);
                if !in_place { upload(&r, &canonical, &vec![-7.; 65536]); }
                let request = NativeScalarRequest {
                    working: &working,
                    encoded: &encoded,
                    canonical: &canonical,
                    depth,
                    region,
                };
                let descriptor = request.descriptor();
                let batch = encoder.prepare(&r.device, &[request], &status, &mut Default::default()).unwrap();
                submit(&r, &encoder, &status, &batch);
                let bytes = capture(&r, &encoded, descriptor, &status);
                let canonical_bytes = page_bytes(&r, &canonical);
                for (i, value) in values.iter().enumerate() {
                    let x = i as u32 % 256;
                    let y = i as u32 / 256;
                    let inside = x >= region[0]
                        && x < region[0] + region[2]
                        && y >= region[1]
                        && y < region[1] + region[3];
                    let expected = if inside {
                        (f64::from(*value) * maximum as f64).round() as u32
                    } else if depth == SampleDepth::U8 {
                        0xa5
                    } else {
                        0xa5a5
                    };
                    assert_eq!(
                        code(&bytes, i, depth),
                        expected,
                        "depth {depth:?}, pattern {pattern}, region {region:?}, pixel {i}"
                    );
                    let actual =
                        f32::from_le_bytes(canonical_bytes[i * 4..i * 4 + 4].try_into().unwrap());
                    if inside {
                        assert_eq!(actual, expected as f32 * reciprocal(depth), "canonical pixel {i}");
                    } else {
                        assert_eq!(actual, if in_place { *value } else { -7. },
                            "untouched canonical pixel {i}");
                    }
                }
            }
        }
    }
}

