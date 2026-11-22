use super::*;
use crate::{
    WgpuRasterizer,
    layer_tests::page_bytes,
    raster::{CaptureSource, TileCapture},
};
use layer_core::raster::RasterTile;

fn texture(r: &WgpuRasterizer) -> wgpu::Texture {
    crate::test_support::page_texture(r, wgpu::TextureFormat::R32Float)
}
fn upload(r: &WgpuRasterizer, t: &wgpu::Texture, values: &[f32]) {
    crate::test_support::upload_page(r, t, &values.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>());
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
    batch: &NativeScalarBatch,
) {
    let mut commands = r.device.create_command_encoder(&Default::default());
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
) -> Vec<u8> {
    let tile = RasterTile::pending(descriptor);
    r.capture_tiles(
        &[TileCapture {
            source: CaptureSource::Packed(buffer),
            tile: tile.clone(),
        }],
    )
    .unwrap()
    .finish()
    .unwrap();
    tile.wait_backing().unwrap().decode_on_worker().unwrap()
}
fn code(bytes: &[u8], i: usize, depth: SampleDepth) -> u32 {
    if depth == SampleDepth::U8 {
        bytes[i] as u32
    } else {
        u16::from_le_bytes(bytes[i * 2..i * 2 + 2].try_into().unwrap()) as u32
    }
}

#[test]
fn scalar_writeback_preserves_every_code_half_neighbors_and_partial_packed_words_with_and_without_in_place_storage() {
    for in_place in [false, true] { scalar_corpus(in_place); }
}
fn scalar_corpus(in_place: bool) {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let encoder = if in_place { NativeScalarEncoder::in_place(&r.device) } else { NativeScalarEncoder::with_device(&r.device) };
    pollster::block_on(r.compilation.prepare_all(&encoder.pipelines)).unwrap();
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
                let batch = encoder.prepare(&r.device, &[request], &mut Default::default()).unwrap();
                submit(&r, &encoder, &batch);
                let bytes = capture(&r, &encoded, descriptor);
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
                        "in_place={in_place}, depth {depth:?}, pattern {pattern}, region {region:?}, pixel {i}"
                    );
                    let actual =
                        f32::from_le_bytes(canonical_bytes[i * 4..i * 4 + 4].try_into().unwrap());
                    if inside {
                        assert_eq!(actual.to_bits(), (expected as f32 / maximum as f32).to_bits(), "in_place={in_place}: canonical pixel {i}");
                    } else {
                        assert_eq!(actual, if in_place { *value } else { -7. },
                            "in_place={in_place}: untouched canonical pixel {i}");
                    }
                }
            }
        }
    }
}

#[test]
fn invalid_coverage_stores_its_nearest_defined_value() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let encoder = NativeScalarEncoder::with_device(&r.device);
    pollster::block_on(r.compilation.prepare_all(&encoder.pipelines)).unwrap();
    let working = texture(&r);
    let canonical = texture(&r);
    let encoded = buffer(&r, SampleDepth::U8);
    let values = [f32::NAN, -f32::NAN, -0.1, -0., 1.1, f32::INFINITY, f32::NEG_INFINITY, 0.5];
    upload(&r, &working, &(0..65536).map(|i| values[i % values.len()]).collect::<Vec<_>>());
    let request = NativeScalarRequest { working: &working, encoded: &encoded, canonical: &canonical, depth: SampleDepth::U8, region: [0, 0, 256, 256] };
    let descriptor = request.descriptor();
    let batch = encoder.prepare(&r.device, &[request], &mut Default::default()).unwrap();
    submit(&r, &encoder, &batch);
    assert_eq!(capture(&r, &encoded, descriptor)[..values.len()], [0, 0, 0, 0, 255, 255, 0, 128]);
}

#[test]
fn native_u8_scalar_reads_equal_every_float_decode_of_their_codes() {
    use layer_core::color::RgbSpace;
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let codes: Vec<u8> = (0..65536u32).map(|i| i as u8).collect();
    let descriptor = PixelDescriptor { sample: layer_core::color::SampleType::Unsigned, channels: 1, bits_per_channel: 8,
        encoding: TransferEncoding::Linear, alpha: AlphaAssociation::None };
    let blob = std::sync::Arc::new(layer_core::raster::TileBlob::encode(descriptor, &codes).unwrap());
    let native = r.device.create_texture(&wgpu::TextureDescriptor { label: Some("native scalar oracle"),
        size: wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    crate::test_support::upload_page(&r, &native, &codes);
    let hardware = texture(&r);
    let module = r.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("native scalar oracle"), source: wgpu::ShaderSource::Wgsl(
        "@group(0) @binding(0) var input:texture_2d<f32>;\n@group(0) @binding(1) var output:texture_storage_2d<r32float,write>;\n\
        @compute @workgroup_size(8,8) fn main(@builtin(global_invocation_id) id:vec3<u32>) {textureStore(output,id.xy,textureLoad(input,id.xy,0));}".into()) });
    let pipeline = r.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: None, module: &module,
        entry_point: Some("main"), compilation_options: Default::default(), cache: None });
    let views = [native.create_view(&Default::default()), hardware.create_view(&Default::default())];
    let bind = r.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[0]) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[1]) }] });
    let mut commands = r.device.create_command_encoder(&Default::default());
    {
        let mut pass = commands.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline); pass.set_bind_group(0, &bind, &[]); pass.dispatch_workgroups(32, 32, 1);
    }
    r.queue.submit([commands.finish()]);
    let resident = texture(&r);
    crate::test_support::submit_encoded(&mut r, |r, encoder| upload_coverage(r, encoder, &resident, SampleDepth::U8, &codes, crate::residency::Class::Authored)).unwrap();
    let encoder = NativeScalarEncoder::with_device(&r.device);
    pollster::block_on(r.compilation.prepare_all(&encoder.pipelines)).unwrap();
    let (working, canonical, encoded) = (texture(&r), texture(&r), buffer(&r, SampleDepth::U8));
    upload(&r, &working, &codes.iter().map(|code| f32::from(*code) / 255.).collect::<Vec<_>>());
    let request = NativeScalarRequest { working: &working, encoded: &encoded, canonical: &canonical, depth: SampleDepth::U8, region: [0, 0, 256, 256] };
    let batch = encoder.prepare(&r.device, &[request], &mut Default::default()).unwrap();
    submit(&r, &encoder, &batch);
    let decoded = crate::test_support::submit_encoded(&mut r, |r, encoder| crate::test_support::query_ready(r, encoder,
        |r, encoder| r.backed_raster_tile(&blob, RgbSpace::Srgb, encoder)));
    let values = |bytes: Vec<u8>, stride: usize| bytes.chunks_exact(stride).map(|pixel| f32::from_le_bytes(pixel[..4].try_into().unwrap())).collect::<Vec<_>>();
    let reads = [("hardware R8Unorm", values(page_bytes(&r, &hardware), 4)), ("resident restore", values(page_bytes(&r, &resident), 4)),
        ("canonical write-back", values(page_bytes(&r, &canonical), 4)), ("source decode", values(page_bytes(&r, &decoded.texture), 16))];
    let differences: Vec<_> = reads.iter().map(|(name, read)| {
        let differing: Vec<_> = (0..256).filter(|&code| read[code].to_bits() != (code as f32 / 255.).to_bits()).collect();
        format!("{name}: {} codes, first {:?}", differing.len(), differing.first().map(|&code| (code, read[code].to_bits(), (code as f32 / 255.).to_bits())))
    }).collect();
    assert!(reads.iter().all(|(_, read)| (0..256).all(|code| read[code].to_bits() == (code as f32 / 255.).to_bits())), "{differences:#?}");
}
