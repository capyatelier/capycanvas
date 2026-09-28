use super::*;
use layer_render::{DabBatchKind, DabStyle, FramePacket, ViewState};

pub(crate) fn view(extent: [u32; 2]) -> ViewState {
    ViewState {
        width_px: extent[0],
        height_px: extent[1],
        document_to_surface: [1., 0., 0., 1., 0., 0.],
        background_rgba_linear: [0.; 4],
    }
}

pub(crate) fn packet(layers: &[Layer], extent: [u32; 2]) -> FramePacket<'_> {
    FramePacket {
        restore_rasters: &[],
        time_seconds: 0.,
        view: view(extent),
        document_extent: extent,
        layers,
        dabs: &[],
        dab_batches: &[],
        reset_layers: false,
        composite_all: true,
        blend_space: Default::default(),
    }
}

pub(crate) fn dab_batch(layer_id: LayerId, style: DabStyle, damage: layer_core::Rect) -> DabBatch {
    DabBatch {
        material_update: 0,
        stroke_id: layer_core::StrokeId(1),
        layer_id,
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style,
        damage,
    }
}

pub(crate) fn complete(r: &WgpuRasterizer) {
    r.device
        .poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(READBACK_TIMEOUT) })
        .unwrap();
}

pub(crate) fn document_texture(r: &WgpuRasterizer) -> &wgpu::Texture {
    if let Some(cache) = &r.scale_display {
        assert_eq!(cache.plan.level, 0);
        assert_eq!(cache.plan.bounds, PixelRect::full(r.document_extent));
        cache.texture()
    } else { r.composite_texture.as_ref().unwrap() }
}

pub(crate) fn receive_request(r: &mut WgpuRasterizer, request: layer_render::RegionRequest) -> layer_render::RegionResult {
    assert!(r.request_region(request).unwrap());
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        if let Some(result) = r.take_region() {
            return result.unwrap();
        }
        assert!(std::time::Instant::now() < deadline, "region readback timed out");
        std::thread::yield_now();
    }
}

/// Solve H(u, v) = p for row-major H. None without a nearby preimage where w > 0.
pub(crate) fn preimage(h: [f64; 9], [x, y]: [f64; 2]) -> Option<[f64; 2]> {
    let [a, b, c, d] = [h[0] - x * h[6], h[1] - x * h[7], h[3] - y * h[6], h[4] - y * h[7]];
    let [e, f] = [x * h[8] - h[2], y * h[8] - h[5]];
    let det = a * d - b * c;
    let [u, v] = [(e * d - b * f) / det, (a * f - e * c) / det];
    (det != 0. && h[6] * u + h[7] * v + h[8] > 0. && u.abs() < 1e9 && v.abs() < 1e9).then_some([u, v])
}

pub(crate) fn page_texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test page"),
        size: wgpu::Extent3d { width: PAGE_SIZE, height: PAGE_SIZE, depth_or_array_layers: 1 },
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

pub(crate) fn upload_page(r: &WgpuRasterizer, texture: &wgpu::Texture, bytes: &[u8]) {
    r.queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(PAGE_SIZE * texture.format().block_copy_size(None).unwrap()),
            rows_per_image: None,
        },
        texture.size(),
    );
}
