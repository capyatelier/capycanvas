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
