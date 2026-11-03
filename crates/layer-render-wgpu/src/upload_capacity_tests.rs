use super::*;
use crate::test_support::{dab_batch, packet};
use crate::tests::{test_dab, test_style};
use std::sync::atomic::Ordering;

#[derive(Debug, PartialEq)]
struct UploadState {
    dabs: wgpu::Buffer,
    styles: wgpu::Buffer,
    binding: wgpu::BindGroup,
    dab_capacity: u64,
    style_capacity: usize,
    dab_staging_capacity: usize,
    style_staging_capacity: usize,
    dab_bytes: Vec<u8>,
    style_bytes: Vec<u8>,
    style_valid: bool,
}

fn state(r: &WgpuRasterizer) -> UploadState {
    UploadState {
        dabs: r.dab_buffer.clone(), styles: r.style_buffer.clone(), binding: r.style_bind_group.clone(),
        dab_capacity: r.dab_capacity_bytes, style_capacity: r.style_capacity,
        dab_staging_capacity: r.dab_upload.capacity(), style_staging_capacity: r.style_upload.capacity(),
        dab_bytes: dab_bytes(&r.dab_upload).to_vec(), style_bytes: r.style_upload.clone(),
        style_valid: r.style_upload_valid.load(Ordering::Acquire),
    }
}

fn fixture() -> (WgpuRasterizer, layer_core::Document) {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    assert_ne!(r.adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let document = layer_core::Document::new(layer_core::authored::PortableId::random(), 64, 64,
        layer_core::DocumentNames { paint: "Upload bounds".into(), paper: "Paper".into() });
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    r.prepare_uploads(packet(document.scene(), [64; 2]), &mut [], &mut encoder).unwrap();
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    crate::test_support::complete(&r);
    assert!(r.style_upload_valid.load(Ordering::Acquire));
    (r, document)
}

#[test]
fn actual_device_upload_limits_refuse_before_buffer_or_cache_changes() {
    let (mut r, _) = fixture();
    let limits = r.device.limits();
    let dab_limit = limits.max_buffer_size.min(limits.max_storage_buffer_binding_size);
    let style_limit = (limits.max_buffer_size / r.style_stride).min(u64::from(u32::MAX) / r.style_stride + 1);
    let dab_growth = r.dab_capacity_bytes as usize / mem::size_of::<DabGpu>() + 1;
    for (dabs, styles) in [
        (dab_limit as usize / mem::size_of::<DabGpu>() + 1, r.style_capacity + 1),
        (dab_growth, style_limit as usize + 1),
    ] {
        let before = state(&r);
        let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let error = r.ensure_upload_capacity(dabs, styles).unwrap_err();
        assert!(matches!(error, GpuRasterError::UploadLimit { .. }), "{error}");
        assert_eq!(state(&r), before);
        assert!(pollster::block_on(scope.pop()).is_none());
    }
}

#[test]
fn reordered_duplicate_dabs_refuse_before_cpu_staging_or_gpu_growth() {
    let (mut r, document) = fixture();
    let target = document.active_target().unwrap();
    let dabs = [test_dab([24.; 2], [0.2, 0.4, 0.6, 1.], 1.)];
    let mut style = test_style(BrushExecution::Dry);
    style.rendering.blend_mode = BrushBlendMode::Multiply;
    let batches = [dab_batch(target, style, dabs[0].bounds())];
    assert!(r.reorders_dabs(&batches[0]));
    let limit = r.device.limits().max_buffer_size.min(r.device.limits().max_storage_buffer_binding_size);
    let repeated = limit as usize / mem::size_of::<DabGpu>();
    let mut tiles = [vec![BrushTile { coordinate: [0; 2], local: PixelRect::full([64; 2]), dabs: 0..1, indices: vec![0; repeated] }]];
    let before = state(&r);
    let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    let error = r.prepare_uploads(FramePacket { dabs: &dabs, dab_batches: &batches, ..packet(document.scene(), [64; 2]) }, &mut tiles, &mut encoder).unwrap_err();
    assert!(matches!(error, GpuRasterError::UploadLimit { .. }), "{error}");
    assert_eq!(state(&r), before);
    assert_eq!(tiles[0][0].dabs, 0..1);
    assert_eq!(tiles[0][0].indices.len(), repeated);
    assert!(pollster::block_on(scope.pop()).is_none());
}

#[test]
fn invalid_dab_batch_ranges_refuse_before_upload_state_changes() {
    let (mut r, document) = fixture();
    let dabs = [test_dab([24.; 2], [0.2, 0.4, 0.6, 1.], 1.)];
    for (first_dab, dab_count) in [(u32::MAX, 1), (1, 1), (0, u32::MAX)] {
        let mut batch = dab_batch(document.active_target().unwrap(), test_style(BrushExecution::Dry), dabs[0].bounds());
        batch.first_dab = first_dab; batch.dab_count = dab_count;
        let batches = [batch];
        let mut tiles = [Vec::new()];
        let before = state(&r);
        let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        let error = r.prepare_uploads(FramePacket { dabs: &dabs, dab_batches: &batches, ..packet(document.scene(), [64; 2]) }, &mut tiles, &mut encoder).unwrap_err();
        assert!(matches!(error, GpuRasterError::InvalidDabRange), "{error}");
        assert_eq!(state(&r), before);
        assert!(pollster::block_on(scope.pop()).is_none());
    }
}

type TileState=([u32;2],PixelRect,std::ops::Range<u32>,Vec<u32>);
fn tile_state(tiles: &[Vec<BrushTile>]) -> Vec<Vec<TileState>> {
    tiles.iter().map(|row| row.iter().map(|tile|
        (tile.coordinate, tile.local, tile.dabs.clone(), tile.indices.clone())).collect()).collect()
}

#[test]
fn mismatched_batch_tile_slices_refuse_before_upload_or_tile_changes() {
    let (mut r, document) = fixture();
    let dabs = [test_dab([24.; 2], [0.2, 0.4, 0.6, 1.], 1.)];
    let batches = [dab_batch(document.active_target().unwrap(), test_style(BrushExecution::Dry), dabs[0].bounds())];
    let tile = BrushTile { coordinate: [0; 2], local: PixelRect::full([64; 2]), dabs: 0..1, indices: vec![0, 0] };
    for mut tiles in [Vec::new(), vec![vec![tile.clone()], vec![tile.clone()]]] {
        let before = state(&r);
        let original_tiles = tile_state(&tiles);
        let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        let error = r.prepare_uploads(FramePacket { dabs: &dabs, dab_batches: &batches, ..packet(document.scene(), [64; 2]) }, &mut tiles, &mut encoder).unwrap_err();
        assert!(matches!(error, GpuRasterError::InvalidDabRange), "{error}");
        assert_eq!(state(&r), before);
        assert_eq!(tile_state(&tiles), original_tiles);
        assert!(pollster::block_on(scope.pop()).is_none());
    }
}

#[test]
fn invalid_reordered_indices_refuse_before_growth_or_prior_tile_rewrites() {
    let (mut r, document) = fixture();
    let dabs = [test_dab([24.; 2], [0.2, 0.4, 0.6, 1.], 1.)];
    let mut style = test_style(BrushExecution::Dry);
    style.rendering.blend_mode = BrushBlendMode::Multiply;
    let batches = [dab_batch(document.active_target().unwrap(), style, dabs[0].bounds())];
    assert!(r.reorders_dabs(&batches[0]));
    let growth = r.dab_capacity_bytes as usize / mem::size_of::<DabGpu>() + 1;
    for invalid in [1, u32::MAX] {
        let mut tiles = [vec![
            BrushTile { coordinate: [0; 2], local: PixelRect::full([64; 2]), dabs: 0..1, indices: vec![0; growth] },
            BrushTile { coordinate: [1, 0], local: PixelRect::full([64; 2]), dabs: 0..1, indices: vec![0, invalid] },
        ]];
        let before = state(&r);
        let original_tiles = tile_state(&tiles);
        let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        let error = r.prepare_uploads(FramePacket { dabs: &dabs, dab_batches: &batches, ..packet(document.scene(), [64; 2]) }, &mut tiles, &mut encoder).unwrap_err();
        assert!(matches!(error, GpuRasterError::InvalidDabRange), "{error}");
        assert_eq!(state(&r), before);
        assert_eq!(tile_state(&tiles), original_tiles);
        assert!(pollster::block_on(scope.pop()).is_none());
    }
}


#[test]
fn upload_memory_reuses_completed_chunks_and_releases_discarded_pins() {
    let (r, _) = fixture();
    let mut uploads = Uploads::new(&r.device, 4096);
    let target = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("upload memory accounting"), size: 256,
        usage: wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
    });
    for _ in 0..4 {
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        uploads.write(&mut encoder, &target, &[17; 256]).unwrap();
        uploads.finish(&encoder);
        assert_eq!(uploads.memory_bytes(), [4096, 4096]);
        assert_eq!(uploads.trim_unused(0), 0);
        encoder.submit(&r.queue);
        crate::test_support::complete(&r);
        assert_eq!(uploads.memory_bytes(), [4096, 0]);
    }
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    uploads.write(&mut encoder, &target, &[23; 256]).unwrap();
    uploads.finish(&encoder);
    assert_eq!(uploads.memory_bytes(), [4096, 4096]);
    drop(encoder);
    assert_eq!(uploads.memory_bytes(), [4096, 0]);
    assert_eq!(uploads.trim_unused(0), 4096);
    assert_eq!(uploads.memory_bytes(), [0, 0]);
}
