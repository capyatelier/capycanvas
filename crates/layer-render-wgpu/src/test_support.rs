use super::*;
use layer_render::{DabBatchKind, DabStyle, FramePacket, ViewState};

pub(crate) fn prepare_raster<T>(r:&mut WgpuRasterizer,mut operation:impl FnMut(&mut WgpuRasterizer)->Result<T,GpuRasterError>)->Result<T,GpuRasterError> {
    let retained=|r:&WgpuRasterizer|r.paint_layers.iter().flat_map(|layer|layer.pages.iter().map(|page|page.active().texture.clone())
        .chain(layer.watercolor_wetness_pages.iter().map(|page|page.active().texture.clone())))
        .chain(r.layer_masks.pages.values().map(|page|page.texture.clone())).collect::<Vec<_>>();
    let original=retained(r);let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);
    loop {
        match operation(r) {
            Err(GpuRasterError::DeferredObjectWork)=> {
                assert_eq!(retained(r),original,"pending preparation published live pages");
                assert!(std::time::Instant::now()<deadline,"raster preparation did not complete");
                r.wait_idle().unwrap();std::thread::sleep(std::time::Duration::from_millis(1));
            },result=>return result,
        }
    }
}

pub(crate) fn add_paint(artwork: &mut layer_core::authored::Artwork, name: impl Into<Arc<str>>, domain: [u32; 2]) -> (OccurrenceHandle, SourceTarget) {
    use layer_core::authored::*;
    let source = artwork.paint.insert(PortableId::random(), PaintSource { color_mode: Default::default(),
        domain, raster: Default::default(), base: None, operations: Arc::default(),
    }).unwrap();
    let occurrence = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Paint(source), name)).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    let at = above_bottom_paper(artwork);
    artwork.stacks.get_mut(stack).unwrap().entries.insert(at, occurrence);
    (occurrence, SourceTarget::Paint(source))
}
pub(crate) fn above_bottom_paper(artwork: &layer_core::authored::Artwork) -> usize {
    let entries = &artwork.stacks.get(artwork.compositions.get(artwork.root).unwrap().result).unwrap().entries;
    let paper = entries.last().and_then(|h| match artwork.occurrences.get(*h)?.content {
        layer_core::authored::OccurrenceContent::Effect(effect) => artwork.effects.get(effect), _ => None,
    }).is_some_and(|effect| layer_core::paper::is_paper(&effect.program));
    entries.len() - usize::from(paper)
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

/// Stores `selection` in a mask through the renderer's mask Coverage
/// operation. Stored tiles keep precedence over the materialized pages.
pub(crate) fn materialize_mask(source: &mut layer_core::authored::CoverageSource, selection: layer_core::Selection, color: layer_core::color::DocumentColor) {
    use layer_core::{authored::*, raster::*, CoverageSnapshot, RasterOperation, RasterOperationKind};
    let mut artwork = Artwork::new(source.domain).unwrap();
    artwork.compositions.get_mut(artwork.root).unwrap().color = color;
    let (owner, _) = add_paint(&mut artwork, "Mask owner", source.domain);
    let mut coverage = CoverageSnapshot::reveal_all(artwork.coverage.next_handle(), source.domain, [0, 0]);
    coverage.source.default_coverage = source.default_coverage;
    coverage.selection = Some(selection);
    let operation = RasterOperation { placement: layer_core::Affine::IDENTITY, coverage, kind: RasterOperationKind::Coverage };
    let damage = operation.bounds(source.domain);
    let raster = RasterRevision::pending();
    let pending = CoverageSource { raster: raster.clone(), operations: Arc::new(vec![operation]), ..source.clone() };
    let handle = artwork.coverage.insert(PortableId::random(), pending).unwrap();
    artwork.occurrences.get_mut(owner).unwrap().mask = Some(MaskUse { source: handle, enabled: true, linked: true, inverted: false, offset: [0, 0] });
    let document = layer_core::Document::from_artwork(artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let batch = DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0,
        ..dab_batch(SourceTarget::Coverage(handle), crate::tests::test_style(BrushExecution::Dry), damage) };
    r.submit(FramePacket { dab_batches: std::slice::from_ref(&batch), reset_layers: true, view: view([64; 2]),
        ..packet(document.scene(), source.domain) }).unwrap();
    let mut tiles = raster.wait_data().unwrap().tiles.clone();
    if let Some(Ok(stored)) = source.raster.try_data() { tiles.extend(stored.tiles.iter().map(|(key, tile)| (*key, tile.clone()))); }
    source.raster = RasterRevision::backed(RasterData { tiles, watercolor: None });
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
        selection_overlays: None,
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

/// The averaged inputs a reduced-resolution dry prediction evaluates against,
/// gathered from `neighborhood` as encoding does.
pub(crate) fn dry_display(r: &mut WgpuRasterizer, batch: &DabBatch, neighborhood: [&wgpu::TextureView; 9], output: &PageSurface)
    -> Option<crate::dry_material::DisplayJob> {
    if !r.dry_display_gathers(batch) { return None; }
    if r.dry_display_inputs.is_empty() {
        let inputs = crate::dry_material::DisplayInputs::new(&r.device);
        r.dry_display_inputs.push(inputs);
    }
    let inputs = &r.dry_display_inputs[0];
    Some(crate::dry_material::DisplayJob { gather: inputs.gather(r, output), inputs: inputs.inputs(r, neighborhood) })
}

/// Dry kernels as a device without read-write storage builds them.
pub(crate) fn write_only_dry_material(r: &mut WgpuRasterizer) {
    r.pipelines.dry_material = crate::dry_material::Pipelines::new(&r.device,
        [&r.target_layout, &r.material_layout, &r.advanced_texture_layout], false, &crate::dry_material::output_layouts(&r.device, false));
}

pub(crate) fn complete(r: &WgpuRasterizer) {
    r.device
        .poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(READBACK_TIMEOUT) })
        .unwrap();
}

/// Submit `frame` once its dependencies are ready, as the engine does.
pub(crate) fn submit_ready(r: &mut WgpuRasterizer, frame: FramePacket<'_>) -> Result<(), GpuRasterError> {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    let preview = r.transform_preview.clone();
    loop {
        r.poll_pending(frame.view)?;
        if r.can_submit() && r.raster_dependencies_ready(frame, preview.as_ref()) { return r.submit(frame); }
        assert!(std::time::Instant::now() < deadline, "frame dependencies must become ready");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(crate) fn submit_settled(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let idle = FramePacket { dabs: &[], dab_batches: &[], restore_rasters: &[],
        reset_layers: false, composite_all: false, ..frame };
    let mut first = true;
    loop {
        r.poll_pending(frame.view).unwrap();
        let next = if first { frame } else { idle };
        if r.can_submit() && r.raster_dependencies_ready(next, None) {
            r.submit(next).unwrap();
            first = false;
        }
        r.wait_idle().unwrap();
        if !first && !r.has_pending_work() && r.raster_ready() && !r.has_pending_submission() { return; }
        assert!(std::time::Instant::now() < deadline, "renderer work must settle");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(crate) fn query_ready<T>(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
    mut query: impl FnMut(&mut WgpuRasterizer, &mut crate::submission::CommandEncoder) -> Result<T, GpuRasterError>) -> T {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        r.drain_image_decodes(encoder).unwrap();
        match query(r, encoder) {
            Ok(value) => return value,
            Err(GpuRasterError::DeferredObjectWork) => {},
            Err(error) => panic!("{error}"),
        }
        scene::Scene::submit_chunk(r, encoder, "test query preparation").unwrap();
        assert!(std::time::Instant::now() < deadline, "query preparation must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(crate) fn thumbnail(r: &mut WgpuRasterizer, id: u64, target: layer_render::ThumbnailTarget) -> Vec<u8> {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    while !r.prepare_thumbnail_batch(target).unwrap() {
        complete(r);
        assert!(std::time::Instant::now() < deadline, "thumbnail preparation must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    r.request_thumbnail(id, target).unwrap();
    loop {
        complete(r);
        if let Some(image) = r.take_thumbnail() { return image.unwrap().bytes; }
        assert!(std::time::Instant::now() < deadline, "thumbnail readback must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
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
        .as_chunks::<16>().0.iter()
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
    a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter())
        .map(|(a, b)| (f32::from_le_bytes(*a) - f32::from_le_bytes(*b)).abs())
        .fold(0., f32::max)
}

pub(crate) fn staged_renderer(reference: &WgpuRasterizer, color: layer_core::color::DocumentColor) -> WgpuRasterizer {
    let mut renderer = WgpuRasterizer::from_wgpu_native_staged(
        reference.adapter.clone(), reference.device().clone(), reference.queue.clone(), color).unwrap();
    renderer.finish_startup_catalog();
    renderer
}

pub(crate) fn wait_startup(renderer: &mut WgpuRasterizer, deadline: std::time::Instant,
    ready: impl Fn(StartupProgress) -> bool, message: std::fmt::Arguments<'_>) {
    loop {
        let progress = renderer.poll_startup().unwrap();
        if ready(progress) { return; }
        assert!(std::time::Instant::now() < deadline, "{message}: {progress:?}, pending jobs: {}",
            renderer.startup.as_ref().map_or(0, |s| s.compiler.pending()));
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Both renderers' canonical composites agree in every working F32 sample.
pub(crate) fn assert_same_canonical(actual: &mut WgpuRasterizer, expected: &mut WgpuRasterizer) {
    let bits = |r: &mut WgpuRasterizer| canonical_pixels(r).into_iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>();
    let (actual, expected) = (bits(actual), bits(expected));
    let differing = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
    let first = actual.iter().zip(&expected).position(|(a, b)| a != b);
    assert!(actual.len() == expected.len() && differing == 0, "{differing} canonical samples differ, first at {first:?}");
}

fn canonical_pixels(r: &mut WgpuRasterizer) -> Vec<[f32; 4]> {
    let frame = r.artwork_frame.clone().unwrap();
    let extent = r.document_extent;
    let mut capture = crate::artwork::Capture::default();
    let mut pixels = vec![[0.; 4]; extent[0] as usize * extent[1] as usize];
    for coordinate in page_coordinates(PixelRect::full(extent)) {
        let region = page_rect(coordinate).intersect(PixelRect::full(extent));
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let tile = query_ready(r, &mut encoder, |r, encoder| capture.region(r, frame.packet(extent), region, [PAGE_SIZE; 2], encoder));
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        let samples = float_pixels(r, &tile.texture);
        for y in 0..region.height() {
            let row = (region.min_y() + y) as usize * extent[0] as usize + region.min_x() as usize;
            let local = y as usize * PAGE_SIZE as usize;
            pixels[row..row + region.width() as usize].copy_from_slice(&samples[local..local + region.width() as usize]);
        }
    }
    pixels
}
