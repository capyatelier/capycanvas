use super::*;
use crate::{WgpuRasterizer, layer_tests::page_bytes};

fn texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("canonical promotion fixture"),
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
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
fn upload(r: &WgpuRasterizer, t: &wgpu::Texture, bytes: &[u8]) {
    r.queue.write_texture(
        t.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256 * t.format().block_copy_size(None).unwrap()),
            rows_per_image: None,
        },
        t.size(),
    );
}
fn floats(values: impl Iterator<Item = f32>) -> Vec<u8> {
    values.flat_map(f32::to_le_bytes).collect()
}

#[test]
fn promotion_preserves_float32_bits_and_pixels_outside_each_region() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let promoter = NativePromoter::with_device(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    for format in [
        wgpu::TextureFormat::Rgba32Float,
        wgpu::TextureFormat::R32Float,
    ] {
        let channels = (format.block_copy_size(None).unwrap() / 4) as usize;
        let canonical = texture(&r, format);
        let working = texture(&r, format);
        let original = floats((0..65536 * channels).map(|i| -0.5 + (i % 65536) as f32 / 65535.));
        let replacement =
            floats((0..65536 * channels).map(|i| 0.12345 + (i % 65536) as f32 / 65535.));
        upload(&r, &canonical, &replacement);
        for region in [
            [0, 0, 256, 256],
            [3, 5, 63, 65],
            [255, 0, 1, 256],
            [0, 255, 256, 1],
            [256, 256, 0, 0],
        ] {
            for invalid in [false, true] {
                upload(&r, &working, &original);
                r.queue.write_buffer(status.buffer(), 0, &u32::from(invalid).to_le_bytes());
                let batch = promoter
                    .prepare(
                        &r.device,
                        &[NativePromotion {
                            canonical: &canonical,
                            working: &working,
                            region,
                        }],
                        &status, &mut Default::default(),
                    )
                    .unwrap();
                assert_eq!(batch.is_empty(), region[2] == 0 || region[3] == 0);
                let mut commands = r.device.create_command_encoder(&Default::default());
                promoter.encode(&mut commands, &batch);
                r.queue.submit([commands.finish()]);
                let actual = page_bytes(&r, &working);
                let [x, y, width, height] = region;
                for py in 0..256 {
                    for px in 0..256 {
                        let begin = (py * 256 + px) as usize * channels * 4;
                        let range = begin..begin + channels * 4;
                        let changed =
                            !invalid && px >= x && py >= y && px < x + width && py < y + height;
                        let expected = if changed { &replacement } else { &original };
                        assert_eq!(
                            actual[range.clone()],
                            expected[range],
                            "{format:?} {region:?} invalid={invalid} at {px},{py}"
                        );
                    }
                }
            }
        }
    }
}

