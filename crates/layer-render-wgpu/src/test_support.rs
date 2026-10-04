use super::*;
use layer_render::{DabBatchKind, DabStyle, FramePacket, ViewState};

pub(crate) fn add_paint(artwork: &mut layer_core::authored::Artwork, name: impl Into<Arc<str>>, domain: [u32; 2]) -> (OccurrenceHandle, SourceTarget) {
    use layer_core::authored::*;
    let source = artwork.paint.insert(PortableId::random(), PaintSource {
        domain, raster: Default::default(), original: None, operations: Arc::default(),
    }).unwrap();
    let occurrence = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Paint(source), name)).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    (occurrence, SourceTarget::Paint(source))
}

pub(crate) fn corrupt_tile(blob: layer_core::raster::TileBlob) -> layer_core::raster::RasterTile {
    use layer_core::{authored::*, raster::*, raster_storage::*};
    struct Chunk(Arc<[u8]>);
    impl TileChunk for Chunk {
        fn len(&self) -> usize { self.0.len() }
        fn poll(&self) -> Result<Option<Arc<[u8]>>, String> { Ok(Some(self.0.clone())) }
        fn resident_bytes(&self) -> usize { self.0.len() }
        fn evict(&self) {}
    }
    let tile=RasterTile::backed(blob);let mut artwork=Artwork::new([256;2]).unwrap();
    let (_, SourceTarget::Paint(source))=add_paint(&mut artwork,"Corrupt backing",[256;2]) else {unreachable!()};
    let scalar=tile.descriptor().channels==1;
    use layer_core::color::{SampleDepth,SampleType};
    artwork.compositions.get_mut(artwork.root).unwrap().color.depth=match (tile.descriptor().sample,tile.descriptor().bits_per_channel) {
        (SampleType::Unsigned,8)=>SampleDepth::U8,(SampleType::Unsigned,16)=>SampleDepth::U16,
        (SampleType::Float,16)=>SampleDepth::F16,(SampleType::Float,32)=>SampleDepth::F32,_=>panic!("fixture depth"),
    };
    let raster=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:if scalar {RasterPlane::Mask}else{RasterPlane::Color},coordinate:[0;2]},tile.clone())].into(),watercolor:None});
    if scalar {
        let coverage=artwork.coverage.next_handle();let mut mask=layer_core::CoverageSnapshot::reveal_all(coverage,[256;2],Default::default());mask.source.raster=raster;
        artwork.coverage.insert(PortableId::random(),mask.source).unwrap();let owner=artwork.occurrences.iter().next().unwrap().0;artwork.occurrences.get_mut(owner).unwrap().mask=Some(mask.use_);
    } else {artwork.paint.get_mut(source).unwrap().raster=raster;}

    let editor=layer_core::Editor::new(layer_core::Document::from_artwork(artwork).unwrap());
    let mut spill=prepare_external_spill(&editor.retained_tiles()).unwrap().unwrap();spill.bytes[0]^=1;
    let bytes=Arc::from(spill.bytes.clone());spill.commit(Arc::new(Chunk(bytes))).unwrap();tile
}

pub(crate) fn view(extent: [u32; 2]) -> ViewState {
    ViewState {
        width_px: extent[0],
        height_px: extent[1],
        document_to_surface: [1., 0., 0., 1., 0., 0.],
    }
}

pub(crate) fn packet(scene: SceneView<'_>, extent: [u32; 2]) -> FramePacket<'_> {
    FramePacket {
        commit_rasters: true,
        restore_rasters: &[],
        time_seconds: 0.,
        view: view(extent),
        document_extent: extent,
        scene,
        selection_visibility: None,
        inspect_mask: None,
        dabs: &[],
        dab_batches: &[],
        reset_layers: false,
        composite_all: true,
        blend_space: Default::default(),
    }
}

pub(crate) fn dab_batch(target: SourceTarget, style: DabStyle, damage: layer_core::Rect) -> DabBatch {
    DabBatch {
        material_update: 0,
        stroke_id: layer_core::StrokeId(1),
        target,
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
    let cache = r.scale_display.as_ref().unwrap();
    assert_eq!(cache.plan.level, 0);
    assert_eq!(cache.plan.bounds, PixelRect::full(r.document_extent));
    cache.texture()
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

pub(crate) fn pen(sequence: u64, phase: layer_engine::PenPhase, [x, y]: [f32; 2], flags: layer_engine::SampleFlags) -> layer_engine::PenEvent {
    use layer_engine::{PenEvent, ToolKind};
    PenEvent {
        device_id: 1,
        sequence,
        timestamp_ns: sequence * 8_000_000,
        view_revision: 0,
        surface_position: layer_core::Point { x, y },
        pressure: 1.,
        tilt_radians: [0.; 2],
        twist_radians: 0.,
        distance: 0.,
        phase,
        tool: ToolKind::Pen,
        flags,
    }
}

pub(crate) fn depth_source(extent: [u32; 2], depth: layer_core::color::SampleDepth, space: layer_core::color::RgbSpace,
    max_bytes: usize, pixel: impl Fn(u32, u32) -> [f32; 4]) -> Arc<layer_core::color::source::SourceImage> {
    use layer_core::color::{SampleDepth, ColorProfile, source::*};
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba, depth, profile: ColorProfile::Builtin(space), profile_assumed: false,
    }, max_bytes).unwrap();
    for y in 0..extent[1] {
        let mut row = Vec::new();
        for x in 0..extent[0] {
            for value in pixel(x, y) {
                match depth {
                    SampleDepth::U8 => row.push((value.clamp(0., 1.) * 255.).round() as u8),
                    SampleDepth::U16 => row.extend_from_slice(&((value.clamp(0., 1.) * 65535.).round() as u16).to_le_bytes()),
                    SampleDepth::F16 => row.extend_from_slice(&layer_core::color::f16::from_f32(value).to_bits().to_le_bytes()),
                    SampleDepth::F32 => row.extend_from_slice(&value.to_le_bytes()),
                }
            }
        }
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}

pub(crate) fn floats(bytes: &[u8]) -> Vec<[f32; 4]> {
    bytes
        .chunks_exact(16)
        .map(|p| std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap())))
        .collect()
}

pub(crate) fn float_pixels(r: &WgpuRasterizer, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
    floats(&crate::layer_tests::page_bytes(r, texture))
}

pub(crate) fn float_presenter(r: &WgpuRasterizer) -> ViewportPresenter {
    ViewportPresenter::for_surface(r, wgpu::TextureFormat::Rgba32Float, SdrSurfaceColor::ExtendedLinearSrgb).unwrap()
}

pub(crate) fn max_error(a: &[[f32; 4]], b: &[[f32; 4]]) -> f32 {
    a.iter().flatten().zip(b.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0., f32::max)
}

pub(crate) fn max_error_bytes(a: &[u8], b: &[u8]) -> f32 {
    a.chunks_exact(4).zip(b.chunks_exact(4))
        .map(|(a, b)| (f32::from_le_bytes(a.try_into().unwrap()) - f32::from_le_bytes(b.try_into().unwrap())).abs())
        .fold(0., f32::max)
}

pub(crate) fn staged_renderer(reference: &WgpuRasterizer, color: layer_core::color::DocumentColor) -> WgpuRasterizer {
    let mut renderer = WgpuRasterizer::from_wgpu_native_staged(
        reference.adapter.clone(), reference.device().clone(), reference.queue.clone(), color).unwrap();
    renderer.finish_startup_cache();
    renderer
}

pub(crate) fn wait_startup(renderer: &mut WgpuRasterizer, deadline: std::time::Instant,
    ready: impl Fn(StartupProgress) -> bool, message: std::fmt::Arguments<'_>) {
    while !ready(renderer.poll_startup().unwrap()) {
        assert!(std::time::Instant::now() < deadline, "{message}");
        std::thread::sleep(Duration::from_millis(1));
    }
}
