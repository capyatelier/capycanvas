use super::*;
use crate::test_support::{complete, dab_batch, packet, submit_published, submit_settled};
use layer_core::{MemoryBudget, MemoryClass, MemoryFacts};

#[test]
fn texture_storage_counts_every_mip_and_depth() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for (dimension, layers, expected) in [
        (wgpu::TextureDimension::D2, 1, 176),
        (wgpu::TextureDimension::D2, 2, 352),
        (wgpu::TextureDimension::D3, 4, 592),
    ] {
        let texture = r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mip storage test"),
            size: wgpu::Extent3d { width: 4, height: 2, depth_or_array_layers: layers },
            mip_level_count: 3,
            sample_count: 1,
            dimension,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        assert_eq!(texture_bytes(&texture), expected);
    }
    let image = display_mips::Image::with_mips(&r, display_mips::Plan::at([8, 4], 0), 3);
    assert_eq!(image.storage_bytes(), 1_049_264);
}

pub(crate) fn renderer(color: layer_core::color::DocumentColor, facts: layer_core::DeviceMemoryFacts) -> Result<WgpuRasterizer, GpuRasterError> {
    let budget = MemoryBudget::with_facts(MemoryFacts { host_default_bytes: Some(2 << 30),
        override_bytes: Some(2 << 30), ..Default::default() });
    renderer_in_budget(color, facts, budget)
}
pub(crate) fn renderer_in_budget(color: layer_core::color::DocumentColor, facts: layer_core::DeviceMemoryFacts, budget: MemoryBudget) -> Result<WgpuRasterizer, GpuRasterError> {
    let hardware = WgpuRasterizer::new_native_headless(color)?;
    let factory = crate::renderer_factory::fixture_factory(&hardware.factory, budget, facts);
    let device = factory.pipeline_device(color);
    let mut renderer = WgpuRasterizer::from_factory_inner(factory, device, Initialization::Headless)?;
    renderer.initialize_native(color)?;
    Ok(*renderer)
}

pub(crate) fn cold_color_pressure(r: &mut WgpuRasterizer) {
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    let device = budget.snapshot().devices[0].device;
    r.memory.raster.handle = budget.register_owner(Kind::RasterPool, MemoryClass::Warm, Some(device));
    r.publish_memory();
    budget.flush_reports();
    r.refresh_memory_limits();
}

pub(crate) fn evict_to_grant(r: &mut WgpuRasterizer, batches: &[layer_render::DabBatch], tiles: &[Vec<BrushTile>]) {
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let excess = r.raster_excess();
    r.evict_raster(excess, batches, tiles, &mut encoder);
    encoder.submit(&r.queue);
}

pub(crate) fn legacy_scene_renderer(color: layer_core::color::DocumentColor) -> WgpuRasterizer {
    renderer(color, layer_core::DeviceMemoryFacts {budget_bytes:Some(64 << 30),..Default::default()}).unwrap()
}

pub(crate) fn legacy_scene_frame(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    r.begin_frame_memory(frame);
    r.publish_memory();
    r.factory.memory_budget().flush_reports();
    r.refresh_memory_limits();
}

pub(crate) fn display_frame_with_budget(r: &mut WgpuRasterizer, frame: FramePacket<'_>, bytes: u64) -> MemoryOwner {
    legacy_scene_frame(r, frame);
    let budget = r.factory.memory_budget().clone();
    let device = budget.snapshot().devices[0];
    let pinned = r.memory.display.handle.reported().pinned_gpu_bytes;
    let held = device.limit_bytes.saturating_sub(device.pinned_bytes).saturating_sub(bytes.saturating_sub(pinned));
    let other = budget.register_owner(Kind::Other, MemoryClass::Pinned, Some(device.device));
    other.report(MemoryReport { gpu_bytes: held, pinned_gpu_bytes: held, ..Default::default() });
    budget.flush_reports();
    r.memory.invalidate_limits();
    r.refresh_memory_limits();
    assert_eq!(r.memory.display_grant(), bytes.max(r.memory.display.handle.reported().pinned_gpu_bytes));
    other
}

pub(crate) fn renderer_for_restore(document: &layer_core::Document) -> WgpuRasterizer {
    let mut r = renderer(document.composition().color,
        layer_core::DeviceMemoryFacts {budget_bytes: Some(4 << 30), ..Default::default()}).unwrap();
    r.prepare_capture_memory(document.scene(), document.composition().size);
    r.memory.demand.set(Demand {mips: 44 << 20, sources: [64 << 20; 2], ..r.memory.demand.get()});
    r.publish_memory();
    r.factory.memory_budget().flush_reports();
    r.refresh_memory_limits();
    r
}

pub(crate) fn declare_source_budget(r: &mut WgpuRasterizer, frame: FramePacket<'_>, slots: usize, uploads: u64, mips: u64) {
    r.memory.invalidate_limits();
    r.refresh_memory_limits();
    let overhead = r.source_tiles.borrow().non_slot_bytes();
    r.memory_demand(frame);
    r.prepare_moving_projection(frame).unwrap();
    let sources = (slots as u64).saturating_mul(scene::sources::NORMALIZED_PAGE_BYTES).saturating_add(overhead);
    r.memory.demand.set(Demand {mips, sources: [sources; 2], ..r.memory.demand.get()});
    r.publish_memory();
    let mut staging = r.memory.staging.published.get();
    staging.demand_gpu_bytes = staging.demand_gpu_bytes.max(uploads);
    r.memory.staging.publish(staging);
    r.factory.memory_budget().flush_reports();
    r.refresh_memory_limits();
}

pub(crate) fn isolate(r: &mut WgpuRasterizer) -> MemoryBudget {
    let budget = MemoryBudget::with_facts(MemoryFacts { host_default_bytes: Some(2 << 30),
        override_bytes: Some(2 << 30), ..Default::default() });
    let device = budget.register_device(layer_core::DeviceMemoryFacts { unified: true, ..Default::default() });
    for (owner, kind) in [(&mut r.memory.display, Kind::Scene), (&mut r.memory.samples, Kind::DecodedSamples),
        (&mut r.memory.raster, Kind::RasterPool), (&mut r.memory.staging, Kind::Staging)] {
        owner.handle = budget.register_owner(kind, MemoryClass::Visible, Some(device));
        owner.handle.report(owner.published.get());
    }
    budget
}

fn pixels(r: &WgpuRasterizer) -> Vec<u8> {
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
}

fn document() -> layer_core::Document {
    layer_core::Document::new(layer_core::PortableId::random(), 512, 512,
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() })
}

#[test]
fn a_paint_only_view_does_not_reserve_its_hierarchy_in_the_raster_pool() {
    let doc = layer_core::Document::new(layer_core::PortableId::random(), 9504, 6336,
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let mut r = renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { budget_bytes: Some(4 << 30), ..Default::default() }).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view = crate::test_support::view([1024, 768]);
    frame.view.document_to_surface = [0.1, 0., 0., 0.1, 0., 0.];
    let idle = r.paint_memory_bytes();
    r.begin_frame_memory(frame);
    r.publish_memory();
    r.factory.memory_budget().flush_reports();
    r.refresh_memory_limits();
    assert_eq!(r.paint_memory_bytes(), idle);
    let raster = r.memory.raster.handle.reported();
    assert!(raster.demand_gpu_bytes <= WORKING_BYTES, "empty raster demand: {raster:?}");
    assert!(r.memory.display_grant() >= CACHE_BYTES,
        "the hierarchy and scene cache fit beside an empty raster pool");
}

#[test]
fn leased_source_pins_do_not_reduce_an_ample_scene_grant() {
    let doc = document();
    let mut r = legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), [512; 2]);
    frame.composite_all = false;
    submit_settled(&mut r, frame);
    declare_source_budget(&mut r, frame, SOURCE_SLOTS + 4, WORKING_BYTES, 1024 * MIB - WORKING_BYTES);
    let source = layer_core::color::source::rgba8_source([PAGE_SIZE * 2, PAGE_SIZE],
        |x, y| [x as u8, y as u8, 93, 255]);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let mut leases = Vec::new();
    for x in 0..2 {
        let prepared = crate::prepare_image_tile(&source, [x, 0], r.document_color().space,
            &r.device.source_samples, None).unwrap();
        let tile = scene::Scene::decode_prepared(&mut r, &mut encoder, &source, [x, 0], prepared).unwrap();
        leases.push(r.source_tiles.borrow().lease(&tile.view).unwrap());
    }
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    complete(&r);
    r.publish_memory();
    r.factory.memory_budget().flush_reports();
    r.begin_frame_memory(frame);
    let demand = r.memory.demand.get().wanted(0);
    let pins = r.memory.demand.get().pinned(r.display_usage().pinned);
    assert!(pins[3] > demand[3], "foreign source leases exceed modelled source demand");
    assert!(r.memory.display_grant() >= total(std::array::from_fn(|part| demand[part].max(pins[part]))), "demand={demand:?}, pins={pins:?}");
    let work = r.metrics.composited_pixels;
    frame.view.document_to_surface[4] = -32.;
    submit_settled(&mut r, frame);
    assert_eq!(r.metrics.composited_pixels, work, "one pan samples settled resident pixels");
    assert_eq!(leases.len(), 2);
}

#[test]
fn a_color_cache_grant_keeps_unpinned_pages_beside_pinned_stroke_pages() {
    let (mut doc, mut r, _data) = durable_color_renderer();
    let extent = doc.composition().size;
    let target = doc.active_target().unwrap();
    let layer_core::SourceTarget::Paint(paint) = target else { unreachable!() };
    let dab = crate::tests::test_dab([64.; 2], [0.7, 0.2, 0.1, 1.], 1.);
    let mut style = crate::tests::test_style(BrushExecution::Dry);
    style.opacity = 0.5;
    let batch = layer_render::DabBatch { stroke_start: true, stroke_end: false,
        ..dab_batch(target, style, dab.bounds()) };
    doc.artwork.paint.get_mut(paint).unwrap().raster = layer_core::raster::RasterRevision::pending();
    submit_published(&mut r, FramePacket { dabs: &[dab], dab_batches: &[batch], ..packet(doc.scene(), extent) });
    complete(&r);
    let pinned = r.paint_layers[0].pages.iter().find(|page| page.stroke_start.is_some()).unwrap().primary.allocation_identity();
    let page_bytes = r.paint_layers[0].pages[0].primary.storage_bytes();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    r.refresh_storage_metrics();
    r.publish_memory();
    budget.flush_reports();
    let _pressure = budget.admit_import(budget.import_headroom_bytes().checked_sub(16 << 20).unwrap()).unwrap();
    budget.flush_reports();
    let evictable = |r: &WgpuRasterizer| r.paint_layers[0].pages.iter().filter(|page| page.stroke_start.is_none()).count();
    let optional = (evictable(&r) as u64 * page_bytes + r.raster_buffers.retained.used()).saturating_sub(r.raster_excess());
    let keep = (optional / page_bytes) as usize;
    assert!(keep > 0 && keep < evictable(&r), "the actual grant retains some backed color: {optional}");
    evict_to_grant(&mut r, &[], &[]);
    assert_eq!(evictable(&r), keep, "pinned color must not consume the optional allowance twice");
    assert!(r.paint_layers[0].pages.iter().any(|page| page.primary.allocation_identity() == pinned && page.stroke_start.is_some()));
    complete(&r);
    r.refresh_storage_metrics();
    r.publish_memory();
    budget.flush_reports();
    let _drain = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    r.refresh_memory_limits();
    evict_to_grant(&mut r, &[], &[]);
    assert_eq!(evictable(&r), 0);
    assert!(r.paint_layers[0].pages.iter().any(|page| page.primary.allocation_identity() == pinned && page.stroke_start.is_some()));
}

#[test]
fn lowered_budget_mid_stroke_keeps_authored_tiles_and_every_presentable_frame() {
    let mut a = document();
    let mut b = a.clone();
    let mut reference = WgpuRasterizer::new_native_headless(a.composition().color).unwrap();
    let mut bounded = WgpuRasterizer::new_native_headless(b.composition().color).unwrap();
    let budget = isolate(&mut bounded);
    submit_settled(&mut reference, packet(a.scene(), [512; 2]));
    submit_settled(&mut bounded, packet(b.scene(), [512; 2]));
    let target = a.active_target().unwrap();
    let layer_core::SourceTarget::Paint(paint) = target else { panic!() };
    for (phase, x) in [96., 256., 416.].into_iter().enumerate() {
        if phase == 1 {
            budget.flush_reports();
            budget.set_ram_override(Some(384 << 20));
        }
        let dab = crate::tests::test_dab([x, 256.], [0.8, 0.1, 0.3, 1.], 1.);
        let batch = layer_render::DabBatch { stroke_start: phase == 0, stroke_end: phase == 2,
            ..dab_batch(target, crate::tests::test_style(BrushExecution::Dry), dab.bounds()) };
        for (r, doc) in [(&mut reference, &mut a), (&mut bounded, &mut b)] {
            r.shader_input();
            doc.artwork.paint.get_mut(paint).unwrap().raster = layer_core::raster::RasterRevision::pending();
            submit_published(r, FramePacket { dabs: &[dab], dab_batches: std::slice::from_ref(&batch),
                ..packet(doc.scene(), [512; 2]) });
            complete(r);
            doc.artwork.paint.get(paint).unwrap().raster.wait_data().unwrap();
            r.after_present();
        }
        assert_eq!(pixels(&bounded), pixels(&reference));
        let tiles = |doc: &layer_core::Document| doc.artwork.paint.get(paint).unwrap().raster.wait_data().unwrap()
            .tiles.iter().map(|(key, tile)| (*key, tile.wait_backing().unwrap().content_digest().unwrap())).collect::<Vec<_>>();
        assert_eq!(tiles(&b), tiles(&a));
        assert!(!tiles(&b).is_empty());
    }
    assert_eq!(budget.ram_limit_bytes(), 384 << 20);
}

#[test]
fn raster_transfer_cache_keeps_the_parent_ceiling_and_can_shrink() {
    let r = legacy_scene_renderer(Default::default());
    let buffers = (0..3).map(|_| r.raster_buffers.take(&r.device, 32 * MIB)).collect::<Vec<_>>();
    let newest = buffers[1..].to_vec();
    for buffer in buffers { r.raster_buffers.put(buffer); }
    assert_eq!(r.raster_buffers.retained.used(), RASTER_BUFFER_BYTES);
    let retained = (0..2).map(|_| r.raster_buffers.take(&r.device, 32 * MIB)).collect::<Vec<_>>();
    assert_eq!(retained, newest);
    drop(newest);
    for buffer in retained { r.raster_buffers.put(buffer); }
    r.raster_tiers.raster.heap().evict(44 * MIB, crate::residency::Class::Fallback);
    r.raster_buffers.release_evicted();
    assert!(r.raster_buffers.retained.used() <= 20 * MIB);
}

#[test]
fn after_present_trims_free_storage_but_keeps_presentation_and_root_overview() {
    let doc = document();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let budget = isolate(&mut r);
    submit_settled(&mut r, packet(doc.scene(), [512; 2]));
    let before = pixels(&r);
    let cache = r.scale_display.as_ref().unwrap();
    let overview = cache.presentation().unwrap().views()[1].clone();
    let pinned = r.composition_memory(r.scene.as_deref())[1];
    assert!(pinned > 0);
    r.raster_buffers.put(r.raster_buffers.take(&r.device, 32 << 20));
    let retained = r.raster_buffers.retained.used();
    assert_eq!(retained, 32 << 20);
    budget.set_ram_override(Some(384 << 20));
    budget.flush_reports();
    let _import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    r.shader_input();
    r.after_present();
    assert_eq!(r.raster_buffers.retained.used(), retained);
    r.shader_idle(true, true);
    r.after_present();
    complete(&r);
    assert!(r.raster_buffers.retained.used() < retained);
    let cache = r.scale_display.as_ref().unwrap();
    assert_eq!(r.composition_memory(r.scene.as_deref())[1], pinned);
    assert_eq!(cache.presentation().unwrap().views()[1], &overview);
    assert_eq!(pixels(&r), before);
}


#[test]
fn first_frame_preparation_publishes_demand_and_uses_the_next_grant() {
    let doc = layer_core::Document::new(layer_core::PortableId::random(), 8192, 8192,
        layer_core::DocumentNames {paint: "Large canvas".into(), paper: "Paper".into()});
    let budget = MemoryBudget::with_facts(MemoryFacts {host_default_bytes: Some(64 << 30), override_bytes: Some(64 << 30), ..Default::default()});
    let mut r = renderer_in_budget(doc.composition().color, layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}, budget.clone()).unwrap();
    r.publish_memory();
    budget.flush_reports();
    let before = r.memory.display_grant();
    let frame = FramePacket {view: crate::test_support::view([8192; 2]), ..packet(doc.scene(), [8192; 2])};
    r.raster_dependencies_ready(frame, None);
    assert!(r.artwork_frame.is_none());
    budget.flush_reports();
    assert!(r.memory.display_grant() > before);
    let display = r.memory.display.handle.reported();
    assert_eq!(display.demand_ram_bytes, display.ram_bytes);
    assert!(r.memory.sample_grant() > 0);
    r.raster_dependencies_ready(frame, None);
    assert_eq!(r.display_memory_limits()[0], r.memory.display_grant());
    assert!(r.display_memory_limits()[2] >= u64::from(PAGE_SIZE).pow(2) * 16);
    assert!(r.artwork_frame.is_none());
}

#[test]
fn the_first_showing_frame_uses_the_active_drawings_display_grant() {
    let doc = layer_core::Document::new(layer_core::PortableId::random(), 4096, 4096,
        layer_core::DocumentNames {paint: "Large canvas".into(), paper: "Paper".into()});
    let mut r = renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts {budget_bytes: Some(2 << 30), ..Default::default()}).unwrap();
    let mut frame = packet(doc.scene(), [4096; 2]);
    frame.view = crate::test_support::view([1024; 2]);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_published(&mut r, frame);
    assert!(r.has_presentable_frame());
    let cache = r.scale_display.as_ref().expect("the first artwork frame has a display cache");
    assert!(cache.evaluation == scene::scale::Evaluation::Display,
        "first artwork frame Scene grant: {} bytes", r.memory.display_grant());
    assert_eq!(cache.plan.level, 2);
}

#[test]
fn raster_readiness_preserves_the_installed_transform_preview() {
    let doc = document();
    let mut r = renderer(doc.composition().color, layer_core::DeviceMemoryFacts {budget_bytes: Some(2 << 30), ..Default::default()}).unwrap();
    let mut preview = layer_render::TransformPreview {transaction: 1, target: doc.active_target().unwrap(),
        moving: true, selection: None,
        transform: layer_core::ImageTransform::affine(layer_core::Affine::translation(layer_core::Point {x: 32., y: 0.}))};
    r.set_transform_preview(Some(&preview)).unwrap();
    r.raster_dependencies_ready(packet(doc.scene(), [512; 2]), None);
    assert_eq!(r.transform_preview.as_ref(), Some(&preview));
    let installed = preview.clone();
    preview.transaction = 2;
    preview.transform = layer_core::ImageTransform::affine(layer_core::Affine::translation(layer_core::Point {x: 64., y: 0.}));
    r.raster_dependencies_ready(packet(doc.scene(), [512; 2]), Some(&preview));
    assert_eq!(r.transform_preview.as_ref(), Some(&installed));
}


#[test]
fn snapshot_capture_keeps_its_authored_memory_after_the_editor_closes() {
    let budget = MemoryBudget::with_facts(MemoryFacts {host_default_bytes: Some(2 << 30), ..Default::default()});
    let editor = layer_core::Editor::new(document().with_memory(layer_core::DocumentMemory::new(budget.clone(), None)));
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let capture = r.snapshot_gpu().capture(editor.capture(1, Default::default()).unwrap(), Default::default()).unwrap();
    drop(editor);
    budget.flush_reports();
    assert!(budget.snapshot().ram_pinned_bytes > 0);
    drop(capture);
    budget.flush_reports();
    assert_eq!(budget.snapshot().ram_physical_bytes, 0);
}

#[test]
fn preparation_demand_preserves_the_shared_sample_inventory() {
    let doc = document();
    let r = renderer(doc.composition().color, layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let candidate = r.color_candidate_staged(doc.composition().color).unwrap();
    let owner = r.memory.samples.handle.clone();
    owner.report(MemoryReport {ram_bytes: 64 << 20, pinned_ram_bytes: 32 << 20,
        demand_ram_bytes: 96 << 20, ..Default::default()});
    candidate.memory_demand(packet(doc.scene(), [512; 2]));
    r.factory.memory_budget().flush_reports();
    let actual = owner.reported();
    assert_eq!(actual.ram_bytes, 64 << 20);
    assert_eq!(actual.pinned_ram_bytes, 32 << 20);
    assert_eq!(actual.demand_ram_bytes, 96 << 20);
}


#[test]
fn drained_headroom_keeps_a_working_window_beside_the_retained_large_view() {
    let extent = [4352, 512];
    let mut doc = layer_core::Document::new(layer_core::PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
    let paint = doc.artwork.paint.iter().next().unwrap().0;
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(
        layer_core::color::source::rgba8_source(extent, |x, y| [(x % 251) as u8, (y % 239) as u8, 128, 255]).into()));
    let mut r = renderer(doc.composition().color, layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let frame = packet(doc.scene(), extent);
    submit_settled(&mut r, frame);
    let expected = pixels(&r);
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    r.publish_memory();
    budget.flush_reports();
    let presentation = r.composition_memory(r.scene.as_deref())[1];
    let window = r.memory.working_limit();
    assert!(presentation > window);
    let _import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert_eq!(budget.import_headroom_bytes(), 0);
    r.begin_frame_memory(frame);
    assert!(r.memory.display_grant() >= presentation + window);
    submit_settled(&mut r, frame);
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level, 0);
    assert_eq!(pixels(&r), expected);
}

#[test]
fn an_import_draining_headroom_preserves_image_and_effect_frame_working_memory() {
    use layer_core::authored::{Image, ImageObject, Occurrence, OccurrenceContent, PortableId};
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let fixed = r.scene_tiers.usage().working;
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    let plain = layer_core::Document::new(PortableId::random(), 128, 128,
        layer_core::DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
    r.memory.demand_region(plain.scene(), [128; 2], PixelRect::full([128; 2]));
    r.publish_memory();
    budget.flush_reports();
    let floor = r.memory.working_limit();
    let plain_import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert_eq!(budget.import_headroom_bytes(), 0);
    let sources = r.source_tiles.borrow().non_slot_bytes();
    let cells = r.changed_cells.as_ref().map_or(0, changed_cells::ChangedCells::storage_bytes);
    assert!(r.memory.display.handle.reported().pinned_gpu_bytes <= floor + 2 * WORKING_BYTES + sources + cells + fixed);
    drop(plain_import);
    let mut artwork = plain.artwork;
    let source = layer_core::color::source::rgba8_source([128; 2], |_, _| [32, 64, 128, 255]);
    let paint = artwork.paint.iter().next().unwrap().0;
    artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(source.clone().into()));
    let object = artwork.objects.insert(PortableId::random(), ImageObject::new(Image::new(layer_core::color::source::rgba8_source([64; 2], |_, _| [16, 32, 64, 255])))).unwrap();
    let occurrence = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Objects(object), "Photo")).unwrap();
    artwork.stacks.get_mut(artwork.compositions.get(artwork.root).unwrap().result).unwrap().entries.insert(0, occurrence);
    let mut doc = layer_core::Document::from_artwork(artwork).unwrap();
    crate::artwork_sample_tests::insert_effect(&mut doc, layer_core::EffectInstance::new(crate::tests::fixture("invert").program()), 0);
    let editor = layer_core::Editor::new(doc.with_memory(layer_core::DocumentMemory::new(budget.clone(), None)));
    let frame = packet(editor.document().scene(), [128; 2]);
    r.raster_dependencies_ready(frame, None);
    r.publish_memory();
    budget.flush_reports();
    let _import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert_eq!(budget.import_headroom_bytes(), 0);
    submit_published(&mut r, frame);
    complete(&r);
    assert!(r.has_presentable_frame());
    let bytes = pixels(&r);
    assert_eq!(bytes.len(), 128 * 128 * 16);
    let rgba = |pixel: &[u8]| std::array::from_fn::<_, 4, _>(|channel| f32::from_le_bytes(pixel[channel*4..channel*4+4].try_into().unwrap()));
    assert!(bytes.chunks_exact(16).all(|pixel| (rgba(pixel)[3] - 1.).abs() < 1e-6));
    assert!(bytes.chunks_exact(16).all(|pixel| rgba(pixel)[0] > 0.5));
    assert!(rgba(&bytes[..16])[0] > rgba(&bytes[bytes.len()-16..])[0]);
    r.set_transform_preview(Some(&layer_render::TransformPreview {
        transaction: 1, target: layer_core::SourceTarget::Paint(paint), moving: true, selection: None,
        transform: layer_core::ImageTransform::affine(layer_core::Affine::translation(layer_core::Point {x: 12., y: 0.})),
    })).unwrap();
    submit_published(&mut r, frame);
    complete(&r);
    assert!(r.has_presentable_frame());
    let moved = pixels(&r);
    assert!(moved.chunks_exact(16).all(|pixel| (rgba(pixel)[3] - 1.).abs() < 1e-6));
    assert_ne!(moved, bytes);
}

#[test]
fn a_restore_drops_backed_color_before_refusing_working_memory() {
    let (doc, mut r, data) = durable_color_renderer();
    let target = doc.active_target().unwrap();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 * MIB));
    r.publish_memory();
    budget.flush_reports();
    let pressure = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    budget.flush_reports();
    let [scene, raster] = [r.memory.display_grant(), r.memory.raster_grant()];
    let page = r.paint_layers[0].pages[0].primary.storage_bytes();
    let restore = (scene + raster) / 2 / page * page;
    assert!(restore > 0 && restore + r.color_memory_bytes() > scene + raster);
    let result = r.admit_preparation_bytes(restore, 0);
    assert!(result.is_ok() || result.as_ref().is_err_and(|error| error.is_preparation_pending()),
        "backed color must not cause terminal restore refusal: {result:?}");
    assert_eq!(r.color_memory_bytes(), 0);
    assert_eq!(r.native_backing(target).unwrap().tiles.len(), data.tiles.len());
    complete(&r);
    assert!(r.admit_preparation_bytes(restore, 0).is_ok());
    drop(pressure);
}

#[test]
fn pressure_retires_every_blend_scratch_page_in_one_pass_over_the_pages() {
    let side = [8, 8];
    let extent = side.map(|side| PAGE_SIZE * side);
    let doc = layer_core::Document::new(layer_core::PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let target = doc.active_target().unwrap();
    let mut r = renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    submit_settled(&mut r, packet(doc.scene(), extent));
    let dabs = (0..side[0] * side[1]).map(|index| crate::tests::test_dab(
        [((index % side[0]) * PAGE_SIZE + PAGE_SIZE / 2) as f32, ((index / side[0]) * PAGE_SIZE + PAGE_SIZE / 2) as f32], [0.7, 0.2, 0.1, 1.], 1.)).collect::<Vec<_>>();
    let batch = layer_render::DabBatch { stroke_start: true, stroke_end: false,
        ..dab_batch(target, crate::tests::test_style(BrushExecution::Wet), layer_core::Rect { min: layer_core::Point { x: 0., y: 0. },
            max: layer_core::Point { x: extent[0] as f32, y: extent[1] as f32 } }) };
    submit_published(&mut r, FramePacket { dabs: &dabs, dab_batches: std::slice::from_ref(&batch), ..packet(doc.scene(), extent) });
    let end = layer_render::DabBatch { stroke_start: false, stroke_end: true, ..batch };
    submit_published(&mut r, FramePacket { dabs: &dabs, dab_batches: std::slice::from_ref(&end), ..packet(doc.scene(), extent) });
    complete(&r);
    r.retire_stroke_sources();
    r.restore_preflight = None;
    let scratch = |r: &WgpuRasterizer| r.paint_layers[0].pages.iter().filter(|page| page.secondary.is_some()).count() as u64;
    let pages = r.paint_layers[0].pages.len() as u64;
    let k = scratch(&r);
    assert!(k > 2 * SOURCE_SLOTS as u64, "the fixture retires several batches: {k} of {pages} pages hold scratch");
    let (retired,_leases)=r.evict_color(u64::MAX,&[],&[]);
    assert_eq!(scratch(&r),0,"one trim must release every optional scratch page");
    assert_eq!(retired.len() as u64,k);
}

#[test]
fn a_restore_drops_an_evictable_hierarchy_before_refusing_working_memory() {
    let doc = layer_core::Document::new(layer_core::PortableId::random(), 4096, 4096,
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let mut r = renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    r.factory.memory_budget().set_ram_override(Some(8 << 30));
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view = crate::test_support::view([512; 2]);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_settled(&mut r, frame);
    complete(&r);
    let hierarchy = |r: &WgpuRasterizer| r.scene.as_ref().unwrap().retained_scale_bytes()[0];
    assert!(hierarchy(&r) > 256 * MIB);
    let presented = r.scale_display.as_ref().unwrap().resident_bytes();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 * MIB));
    r.publish_memory();
    budget.flush_reports();
    r.refresh_memory_limits();
    let result = r.admit_preparation_bytes(64 * MIB, 0);
    assert!(result.is_ok() || result.as_ref().is_err_and(|error| error.is_preparation_pending()),
        "evictable hierarchy must not cause terminal restore refusal: {result:?}");
    assert!(r.scene_tiers.display.used() <= r.memory.retention_grant() + r.scene_tiers.retiring.used(),
        "the hierarchy above the lowered grant is released: {} of {}", r.scene_tiers.display.used(), r.memory.retention_grant());
    assert!(hierarchy(&r) < 256 * MIB);
    assert_eq!(r.scale_display.as_ref().unwrap().resident_bytes(), presented, "the presented picture stays");
    assert!(r.has_presentable_frame());
    complete(&r);
    assert!(r.admit_preparation_bytes(64 * MIB, 0).is_ok());
}

#[test]
fn a_restore_is_refused_or_admitted_after_one_budget_rebalance() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    r.publish_memory();
    budget.flush_reports();
    r.refresh_memory_limits();
    let import = budget.admit_import(budget.import_headroom_bytes() - (8 << 20)).unwrap();
    r.refresh_memory_limits();
    let restore = r.memory.display_grant() + r.memory.raster_grant() + (8 << 20);
    assert!(restore < budget.ram_limit_bytes() / 2);
    for _ in 0..2 {
        assert!(matches!(r.admit_preparation_bytes(restore, 0), Err(GpuRasterError::SourceWorkingSetExceeded)));
    }
    assert_eq!(r.memory.preparation.get(), 0);
    drop(import);
    let first = r.admit_preparation_bytes(restore, 0);
    assert!(first.is_ok() || matches!(first, Err(GpuRasterError::NotReady)), "the released restore must publish its need: {first:?}");
    assert!(r.admit_preparation_bytes(restore, 0).is_ok());
}

#[test]
fn an_effect_analysis_refused_under_import_pressure_is_admitted_once_the_pressure_releases() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    r.publish_memory();
    budget.flush_reports();
    r.refresh_memory_limits();
    let import = budget.admit_import(budget.import_headroom_bytes() - (8 << 20)).unwrap();
    r.refresh_memory_limits();
    let bytes = r.device.resource_limit.load(std::sync::atomic::Ordering::Acquire) + (16 << 20);
    assert!(bytes < budget.ram_limit_bytes() / 2);
    assert!(r.reserve_effect_analysis(bytes).is_err());
    drop(import);
    let lease = r.reserve_effect_analysis(bytes);
    assert!(lease.is_ok(), "the released analysis must publish its need: {:?}", lease.map(|_| ()));
}

#[test]
fn ample_budget_keeps_scene_retention_and_returns_unused_source_allowance() {
    use layer_core::{authored::{Image, ImageObject, Occurrence, OccurrenceContent, PortableId}, color::source::SourceImage};
    let extent = [4096, 3072];
    let mut artwork = layer_core::Document::new(PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames {paint: "Ink".into(), paper: "Paper".into()}).artwork;
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    let mut single = None;
    for index in 0..28 {
        let tile = layer_core::color::source::rgba8_source([PAGE_SIZE; 2], |_, _| [index, 100, 150, 255]);
        let blob = tile.tiles.values().next().unwrap().clone();
        let tiles = (0..extent[1] / PAGE_SIZE).flat_map(|y| (0..extent[0] / PAGE_SIZE)
            .map(move |x| [x, y])).map(|coordinate| (coordinate, blob.clone())).collect();
        let source = Arc::new(SourceImage {extent, tiles, ..(*tile).clone()});
        let object = artwork.objects.insert(PortableId::random(), ImageObject::new(Image::new(source))).unwrap();
        let owner = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Objects(object), "Photo")).unwrap();
        artwork.stacks.get_mut(stack).unwrap().entries.insert(0, owner);
        if index == 0 { single = Some(layer_core::Document::from_artwork(artwork.clone()).unwrap()); }
    }
    let full = layer_core::Document::from_artwork(artwork).unwrap();
    let budget = MemoryBudget::with_facts(MemoryFacts {host_default_bytes: Some(1 << 40), override_bytes: Some(1 << 40), ..Default::default()});
    let mut r = renderer_in_budget(full.composition().color, layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 40), ..Default::default()}, budget).unwrap();
    for document in [single.as_ref().unwrap(), &full] {
        let frame = packet(document.scene(), extent);
        r.begin_frame_memory(frame);
        r.prepare_moving_projection(frame).unwrap();
        let requested = crate::object_image_mips::MovingImages::demand(&r.moving_projection.as_ref().unwrap().requests, [frame.view.width_px,frame.view.height_px]).unwrap();
        let mips = requested;
        let windows = scene::Scene::capture_image_bound(document.scene(), PixelRect::full(extent));
        assert!(r.memory.display_grant() >= CACHE_BYTES + OBJECT_CACHE_BYTES + mips);
        assert_eq!(scene::Scene::window_plan(document.scene(), extent, r.image_pixel_budget(0),
            r.device.limits().max_texture_dimension_2d, None).unwrap().is_none(), windows <= r.image_pixel_budget(0));
        assert_eq!(r.memory.mip_budget(), mips);
        assert_eq!(r.memory.sample_grant(), DECODED_SAMPLE_BYTES);
        assert_eq!(r.memory.demand.get().mips, mips);
    }
    let plain = layer_core::Document::new(PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
    let frame = packet(plain.scene(), extent);
    r.begin_frame_memory(frame);
    r.prepare_moving_projection(frame).unwrap();
    assert!(r.memory.display_grant() >= CACHE_BYTES + OBJECT_CACHE_BYTES);
    assert_eq!(r.memory.mip_budget(), 0);
    assert_eq!(r.memory.sample_grant(), DECODED_SAMPLE_BYTES);
}

#[test]
fn a_tablet_budget_below_the_photo_keeps_publishing_strokes_and_their_ink() {
    use crate::test_support::pen;
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue};
    const EXTENT: [u32; 2] = [6000, 4000];
    const SIZE: [u32; 2] = [2560, 1600];
    const FRAMES: u32 = 120;
    let budget = MemoryBudget::with_facts(MemoryFacts { total_bytes: Some(5648 << 20), available_bytes: Some(2183 << 20), tablet: true, ..Default::default() });
    let mut document = layer_core::Document::new(layer_core::PortableId::random(), EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Photo".into(), paper: "Paper".into() });
    let layer_core::SourceTarget::Paint(photo) = document.working.target.unwrap() else { panic!() };
    let source = crate::test_support::depth_source(EXTENT, layer_core::color::SampleDepth::U8, layer_core::color::RgbSpace::Srgb, 128 << 20,
        |x, y| [x as f32 / EXTENT[0] as f32, y as f32 / EXTENT[1] as f32, ((x ^ y) & 63) as f32 / 255. + 0.5, 1.]);
    document.artwork.paint.get_mut(photo).unwrap().base = Some(layer_core::authored::PaintBase::new(source.into()));
    let (owner, target) = crate::test_support::add_paint(&mut document.artwork, "Ink", EXTENT);
    let stack = document.artwork.compositions.get(document.artwork.root).unwrap().result;
    let entries = &mut document.artwork.stacks.get_mut(stack).unwrap().entries;
    entries.retain(|h| *h != owner);
    entries.insert(0, owner);
    let paper = *entries.last().unwrap();
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    let mut document = layer_core::Document::from_artwork(document.artwork).unwrap();
    document.working.target = Some(target);
    document.working.occurrence = Some(owner);
    let mut brush = layer_core::default_brush(layer_core::CONTACT_BRUSH_PRESETS.into_iter().find(|p| *p as u32 == 2).unwrap());
    brush.diameter = 16.;
    let mut gpu = renderer_in_budget(document.composition().color, layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }, budget).unwrap();
    gpu.prepare_startup(&document, &brush, false).unwrap();
    gpu.finish_startup_catalog();
    crate::test_support::wait_startup(&mut gpu, std::time::Instant::now() + std::time::Duration::from_secs(60), |progress| progress.complete, format_args!("startup"));
    let (mut input, consumer) = input_queue(128);
    let scale = (SIZE[0] as f32 / EXTENT[0] as f32).min(SIZE[1] as f32 / EXTENT[1] as f32);
    let [tx, ty] = [0, 1].map(|axis| (SIZE[axis] as f32 - EXTENT[axis] as f32 * scale) * 0.5);
    let view = layer_render::ViewState { width_px: SIZE[0], height_px: SIZE[1], document_to_surface: [scale, 0., 0., scale, tx, ty] };
    let mut engine = CanvasEngine::new(gpu, document, consumer, view,
        ViewTransform { revision: 0, surface_to_document: [1. / scale, 0., 0., 1. / scale, -tx / scale, -ty / scale] }).unwrap();
    engine.set_instant_feedback(InstantFeedbackConfig { enabled: true, use_platform_prediction: false, prediction_horizon_micros: 16_000, ..Default::default() }).unwrap();
    engine.render_frame_at(0).unwrap();
    engine.set_brush(brush).unwrap();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = engine.backend().device().create_texture(&wgpu::TextureDescriptor {
        label: Some("test presentation"), size: wgpu::Extent3d { width: SIZE[0], height: SIZE[1], depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut presenter = ViewportPresenter::for_surface(engine.backend(), format, SdrSurfaceColor::Srgb).unwrap();
    fn frame(engine: &mut CanvasEngine<WgpuRasterizer>, presenter: &mut ViewportPresenter, target: &wgpu::TextureView, clock: u64) -> bool {
        let revision = engine.backend().canvas_preview_revision();
        engine.render_frame_at(clock).unwrap();
        presenter.present(engine.backend(), target, engine.view(), [0.12, 0.12, 0.12, 1.]).unwrap();
        engine.backend_mut().wait_idle().unwrap();
        engine.backend().canvas_preview_revision() != revision
    }
    fn settle(engine: &mut CanvasEngine<WgpuRasterizer>, presenter: &mut ViewportPresenter, target: &wgpu::TextureView, clock: &mut u64) {
        for _ in 0..600 {
            if engine.backend().raster_ready() && !crate::scene::scale::tests::publication_pending(engine.backend()) && !engine.has_pending_document_edits() && !engine.has_pending_input() { return; }
            *clock += 4_166_667;
            frame(engine, presenter, target, *clock);
        }
        panic!("the canvas never settled");
    }
    let mut clock = 1_000_000_000u64;
    let mut revision = 0;
    settle(&mut engine, &mut presenter, &view, &mut clock);
    let mut published = Vec::new();
    for contact in 0..2 {
        let mut advanced = Vec::new();
        for step in 0..FRAMES {
            if contact == 0 && step == 4 {
                revision = 1;
                engine.set_view(layer_render::ViewState { width_px: SIZE[0], height_px: SIZE[1], document_to_surface: [scale, 0., 0., scale, tx + 48., ty] },
                    ViewTransform { revision, surface_to_document: [1. / scale, 0., 0., 1. / scale, -(tx + 48.) / scale, -ty / scale] });
            }
            for sample in 0..2 {
                clock += 4_166_667;
                let angle = (contact * FRAMES + step) as f32 / FRAMES as f32 * 1.3 + sample as f32 * 0.005;
                let phase = if step == 0 && sample == 0 { PenPhase::Down } else if step + 1 == FRAMES && sample == 1 { PenPhase::Up } else { PenPhase::Move };
                input.push(PenEvent { timestamp_ns: clock, view_revision: revision,
                    ..pen(clock, phase, [1280. + 900. * angle.cos(), 800. + 500. * angle.sin()], SampleFlags::PRIMARY) }).unwrap();
            }
            advanced.push(frame(&mut engine, &mut presenter, &view, clock));
        }
        settle(&mut engine, &mut presenter, &view, &mut clock);
        published.push(advanced);
    }
    let pattern: Vec<String> = published.iter().map(|frames| frames.iter().map(|advanced| if *advanced { '+' } else { '.' }).collect()).collect();
    assert!(pattern.iter().all(|frames| frames.contains('+') && !frames.trim_start_matches('.').contains("....")),
        "every stroke must keep publishing once its photo tiles decode: {pattern:?}");
    let revision_before_tap = engine.backend().canvas_preview_revision();
    for (offset, phase) in [(0., PenPhase::Down), (6., PenPhase::Move), (12., PenPhase::Up)] {
        clock += 1_388_889;
        input.push(PenEvent { timestamp_ns: clock, view_revision: revision, ..pen(clock, phase, [260. + offset, 160. + offset / 2.], SampleFlags::PRIMARY) }).unwrap();
    }
    frame(&mut engine, &mut presenter, &view, clock);
    settle(&mut engine, &mut presenter, &view, &mut clock);
    assert_ne!(engine.backend().canvas_preview_revision(), revision_before_tap, "a tap reports its ink once it publishes");
    let shown = crate::layer_tests::page_bytes(engine.backend(), &texture);
    engine.backend_mut().document_damage.push(PixelRect::full(EXTENT));
    settle(&mut engine, &mut presenter, &view, &mut clock);
    clock += 4_166_667;
    frame(&mut engine, &mut presenter, &view, clock);
    assert!(shown == crate::layer_tests::page_bytes(engine.backend(), &texture), "a full redraw changed the settled canvas, so deferred frames lost damage");
}

#[test]
fn changed_resident_color_restores_lazily_under_a_tight_cache_without_losing_history() {
    use layer_core::{Affine, Point, RasterOperation, RasterOperationKind, Selection};
    use layer_engine::{CanvasEngine, ViewTransform, input_queue};
    use crate::test_support::{add_paint, flush_engine, submit_settled};
    const EXTENT: [u32; 2] = [2752, 2064];
    const SIZE: [u32; 2] = [1376, 1032];
    let mut document = layer_core::Document::new(layer_core::PortableId::random(), EXTENT[0], EXTENT[1],
        layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let original = document.active_target().unwrap();
    let (owner, temporary) = add_paint(&mut document.artwork, "Temporary paint", EXTENT);
    let stack = document.artwork.compositions.get(document.artwork.root).unwrap().result;
    let entries = &mut document.artwork.stacks.get_mut(stack).unwrap().entries;
    entries.retain(|id| *id != owner);
    entries.insert(0, owner);
    let mut document = layer_core::Document::from_artwork(document.artwork).unwrap();
    document.working.target = Some(temporary);
    document.working.occurrence = Some(owner);
    let selection = Selection::polygon(vec![Point { x: 0., y: 0. }, Point { x: EXTENT[0] as f32, y: 0. },
        Point { x: EXTENT[0] as f32, y: EXTENT[1] as f32 }, Point { x: 0., y: EXTENT[1] as f32 }]).unwrap();
    document.working.selection = Some(selection.clone());
    let mut gpu = renderer(document.composition().color,
        layer_core::DeviceMemoryFacts { budget_bytes: Some(512 << 20), ..Default::default() }).unwrap();
    gpu.finish_startup_catalog();
    gpu.prepare_startup(&document, &layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), true).unwrap();
    crate::test_support::wait_startup(&mut gpu, std::time::Instant::now() + std::time::Duration::from_secs(60),
        |progress| progress.complete, format_args!("selected paint transform startup"));
    let scale = 0.49;
    let [tx, ty] = [0, 1].map(|axis| (SIZE[axis] as f32 - EXTENT[axis] as f32 * scale) * 0.5);
    let view = layer_render::ViewState { width_px: SIZE[0], height_px: SIZE[1],
        document_to_surface: [scale, 0., 0., scale, tx, ty] };
    let mut engine = CanvasEngine::new(gpu, document, input_queue(8).1, view,
        ViewTransform { revision: 0, surface_to_document: [1. / scale, 0., 0., 1. / scale, -tx / scale, -ty / scale] }).unwrap();
    let publish = |engine: &mut CanvasEngine<WgpuRasterizer>| {
        flush_engine(engine);
        let snapshot = engine.document().clone();
        submit_settled(engine.backend_mut(), FramePacket { view, ..packet(snapshot.scene(), EXTENT) });
        assert!(engine.backend().has_presentable_frame());
    };
    for target in [original, temporary] {
        let mut coverage = layer_core::CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(), EXTENT, [0; 2]);
        coverage.source.default_coverage = 0.;
        coverage.selection = Some(selection.clone());
        engine.append_raster_operation(target, RasterOperation { placement: Affine::IDENTITY, coverage,
            kind: RasterOperationKind::Fill { color: [0., 0., 1., 1.], texture: Default::default(),
                alpha_locked: false, edges: None, underfill: None } }).unwrap();
        publish(&mut engine);
    }
    let before = engine.document().clone();
    let mut presenter = ViewportPresenter::for_renderer(engine.backend(), wgpu::TextureFormat::Rgba8UnormSrgb);
    pollster::block_on(presenter.prepare(engine.backend())).unwrap();
    let (texture, surface) = crate::create_target(engine.backend().device(), SIZE,
        wgpu::TextureFormat::Rgba8UnormSrgb, "selected paint transform viewport");
    let capture = |presenter: &mut ViewportPresenter, engine: &CanvasEngine<WgpuRasterizer>| {
        presenter.present(engine.backend(), &surface, view, [1.; 4]).unwrap();
        crate::layer_tests::page_bytes(engine.backend(), &texture)
    };
    let baseline = capture(&mut presenter, &engine);
    for x in [SIZE[0] / 8, SIZE[0] / 2, SIZE[0] * 7 / 8] {
        let offset = ((SIZE[1] / 2 * SIZE[0] + x) * 4) as usize;
        assert_eq!(&baseline[offset..offset + 4], &[0, 0, 255, 255]);
    }
    engine.set_transform_preview(Some(layer_render::TransformPreview { transaction: 1, target: temporary,
        moving: false, selection: Some(selection), transform: layer_core::ImageTransform::affine(Affine([
            0.5, 0., 0., 1., EXTENT[0] as f32 * 0.25, 0.,
        ])) })).unwrap();
    publish(&mut engine);
    assert_eq!(engine.document().artwork, before.artwork);
    assert_eq!(capture(&mut presenter, &engine), baseline);
    assert!(engine.commit_transform(None).unwrap());
    publish(&mut engine);
    assert_ne!(engine.document().artwork.paint, before.artwork.paint);
    assert_eq!(capture(&mut presenter, &engine), baseline);
    assert!(engine.undo().unwrap());
    publish(&mut engine);
    assert_eq!(engine.document().artwork.coverage.iter().collect::<Vec<_>>(), before.artwork.coverage.iter().collect::<Vec<_>>());
    let mut restored = engine.document().artwork.clone();
    restored.coverage = before.artwork.coverage.clone();
    assert_eq!(restored, before.artwork);
    assert_eq!(engine.document().working.selection, before.working.selection);
    assert_eq!(capture(&mut presenter, &engine), baseline);
    let original_owner = engine.document().scene().source_owner(original).unwrap();
    engine.set_layer_opacity(original_owner, 0.).unwrap();
    publish(&mut engine);
    assert_eq!(capture(&mut presenter, &engine), baseline);
}

#[test]
fn blank_paint_outputs_wait_for_raster_capacity_before_the_frame_is_accepted() {
    let extent=[8192;2];
    let mut doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let target=doc.active_target().unwrap();
    let layer_core::SourceTarget::Paint(paint)=target else {unreachable!()};
    doc.artwork.paint.get_mut(paint).unwrap().raster=layer_core::raster::RasterRevision::pending();
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {unified:true,..Default::default()}).unwrap();
    r.ensure_document_metadata(extent,doc.scene()).unwrap();
    let dabs=(0..64).map(|n|crate::tests::test_dab([(n%32*PAGE_SIZE+128) as f32,(n/32*PAGE_SIZE+128) as f32],
        [1.,0.,1.,1.],1.)).collect::<Vec<_>>();
    let damage=dabs.iter().copied().map(Dab::bounds).fold(layer_core::Rect::EMPTY,layer_core::Rect::union);
    let batch=DabBatch {dab_count:dabs.len() as u32,..dab_batch(target,crate::tests::test_style(BrushExecution::Dry),damage)};
    let mut frame=FramePacket {dabs:&dabs,dab_batches:std::slice::from_ref(&batch),..packet(doc.scene(),extent)};
    frame.view.width_px=512;frame.view.height_px=512;
    r.begin_frame_memory(frame);
    let budget=r.factory.memory_budget().clone();budget.set_ram_override(Some(384*MIB));
    r.refresh_storage_metrics();r.publish_memory();budget.flush_reports();r.refresh_memory_limits();
    let pressure=budget.admit_import(budget.import_headroom_bytes()).unwrap();r.refresh_memory_limits();
    let ready=r.raster_dependencies_ready(frame,None);
    r.refresh_storage_metrics();
    let physical=r.raster_buffers.memory_bytes()[1]+r.paint_memory_bytes();
    let grant=r.memory.raster_grant();
    assert!(physical<=grant,"unaccepted Raster physical={physical}, own grant={grant}");
    assert!(r.paint_layers.iter().all(|layer|layer.pages.is_empty()&&layer.coverage_pages.is_empty()),
        "a refused output plan cannot allocate color or coverage");
    assert!(!ready,"64 new color pages and their coverage exceed the drained Raster working capacity");
    assert!(r.take_restore_failure().is_none(),"capacity refusal must retain the frame, not publish a raster error");
    drop(pressure);budget.set_ram_override(Some(2<<30));budget.flush_reports();r.refresh_memory_limits();
    submit_published(&mut r,frame);complete(&r);
    let data=doc.artwork.paint.get(paint).unwrap().raster.wait_data().unwrap();
    let coordinates=data.tiles.keys().filter(|key|key.plane==layer_core::raster::RasterPlane::Color)
        .map(|key|key.coordinate).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(coordinates,(0..64).map(|n|[n%32,n/32]).collect(),"every retained dab must reach authored backing");
}

#[test]
fn preview_outputs_the_raster_cannot_hold_keep_the_frame_unready_instead_of_deferring_its_submission() {
    let extent=[8192;2];
    let doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let target=doc.active_target().unwrap();
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {unified:true,..Default::default()}).unwrap();
    r.ensure_document_metadata(extent,doc.scene()).unwrap();
    let preview=|dabs:&[Dab]| {
        let damage=dabs.iter().copied().map(Dab::bounds).fold(layer_core::Rect::EMPTY,layer_core::Rect::union);
        DabBatch {dab_count:dabs.len() as u32,kind:DabBatchKind::Preview,..dab_batch(target,crate::tests::test_style(BrushExecution::Dry),damage)}
    };
    let frame=|dabs,batches| {let mut frame=FramePacket {dabs,dab_batches:batches,..packet(doc.scene(),extent)};frame.view.width_px=512;frame.view.height_px=512;frame};
    let first=[crate::tests::test_dab([128.;2],[1.,0.,1.,1.],1.)];
    let warm=[preview(&first)];
    crate::test_support::submit_ready(&mut r,frame(&first,&warm)).unwrap();r.wait_idle().unwrap();
    let held=r.preview_pages.len();
    let dabs=(0..64).map(|n|crate::tests::test_dab([(n%32*PAGE_SIZE+128) as f32,(n/32*PAGE_SIZE+PAGE_SIZE+128) as f32],
        [1.,0.,1.,1.],1.)).collect::<Vec<_>>();
    let batches=[preview(&dabs)];
    let frame=frame(&dabs,&batches);
    r.begin_frame_memory(frame);
    let budget=r.factory.memory_budget().clone();budget.set_ram_override(Some(384*MIB));
    r.refresh_storage_metrics();r.publish_memory();budget.flush_reports();r.refresh_memory_limits();
    let pressure=budget.admit_import(budget.import_headroom_bytes()).unwrap();r.refresh_memory_limits();
    if r.raster_dependencies_ready(frame,None) {
        assert!(!r.submit(frame).is_err_and(|error|error.is_preparation_pending()),"a ready frame deferred its preview outputs during submission");
    }
    assert_eq!(r.preview_pages.len(),held,"a refused preview allocates no pages");
    drop(pressure);budget.set_ram_override(Some(2<<30));budget.flush_reports();r.refresh_memory_limits();
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    assert!(r.preview_pages.len()>=64,"the released raster holds the preview");
}

fn durable_color_renderer() -> (layer_core::Document, WgpuRasterizer, Arc<layer_core::raster::RasterData>) {
    use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
    let extent=[PAGE_SIZE*8;2];
    let mut doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let target=doc.active_target().unwrap();let layer_core::SourceTarget::Paint(paint)=target else {unreachable!()};
    let bytes=[255,0,0,255].repeat(PAGE_SIZE.pow(2) as usize);
    let blob=Arc::new(TileBlob::encode(doc.composition().color.paint_descriptor(),&bytes).unwrap());
    let data=Arc::new(RasterData {tiles:(0..8).flat_map(|y|(0..8).map(move|x|[x,y]))
        .map(|coordinate|(TileKey {plane:RasterPlane::Color,coordinate},RasterTile::backed_shared(blob.clone()))).collect(),watercolor:None});
    doc.artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed((*data).clone());
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {unified:true,..Default::default()}).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.view.width_px=512;frame.view.height_px=512;
    submit_settled(&mut r,frame);complete(&r);
    let data=r.native_backing(target).unwrap().clone();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    for coordinate in data.tiles.keys().map(|key|key.coordinate) {
        if r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter().any(|page|page.coordinate==coordinate) {continue;}
        let expected=r.raster_version_for(doc.scene(),target,RasterPlane::Color,coordinate).unwrap();
        let mut page=r.create_page(coordinate,"durable color cache fixture");page.primary_needs_clear=false;
        let receipt=encoder.submission();let write=page.primary.initialize(&receipt).unwrap();
        {let _pass=encoder.color_pass("durable color cache fixture",&write.value().view,wgpu::LoadOp::Clear(wgpu::Color {r:1.,g:0.,b:0.,a:1.}));}
        write.encoded(expected).publish(&encoder.submission());
        r.paint_layers.iter_mut().find(|layer|layer.id==target).unwrap().pages.push(page);
    }
    encoder.submit(&r.queue);complete(&r);
    r.refresh_storage_metrics();r.publish_memory();r.factory.memory_budget().flush_reports();
    assert_eq!(r.color_memory_bytes(),64*MIB);
    assert_eq!(r.pinned_paint_bytes(),r.paint_memory_bytes()-64*MIB,"all fixture color is durable and optional");
    (doc,r,data)
}

#[test]
fn durable_color_accounting_compares_only_changed_pages() {
    let (_doc,r,_data)=durable_color_renderer();
    let before=crate::test_support::PassCounts::snapshot();
    let pinned=r.pinned_paint_bytes();
    let visits=crate::test_support::PassCounts::snapshot().delta(before).paint_page_visits;
    assert_eq!(pinned,r.paint_memory_bytes()-64*MIB);
    assert_eq!(visits,0,"durable color accounting compared {visits} unchanged pages one by one");
}

#[test]
fn stroke_frames_compare_only_the_pages_they_write() {
    let extent=[PAGE_SIZE*16,PAGE_SIZE];
    let doc=crate::scene::scale::tests::document_at(extent);
    let target=doc.active_target().unwrap();
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    submit_settled(&mut r,frame);
    let style=crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
    let mut visits=Vec::new();
    for step in 0..16 {
        let mut dab=crate::tests::test_dab([(step*PAGE_SIZE+PAGE_SIZE/2) as f32,(PAGE_SIZE/2) as f32],[0.9,0.1,0.3,1.],0.8);dab.radii=[40.;2];
        let batch=layer_render::DabBatch {stroke_start:step==0,..dab_batch(target,style.clone(),dab.bounds())};
        let before=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame}).unwrap();
        complete(&r);
        visits.push(crate::test_support::PassCounts::snapshot().delta(before).paint_page_visits);
    }
    let late=visits[8..].iter().copied().max().unwrap();let early=visits[..4].iter().copied().max().unwrap();
    assert!(late<=early,"a stroke writing one page a frame compared {late} pages a frame after 8 frames and {early} at its start: {visits:?}");
}

#[test]
fn a_raster_above_its_grant_sheds_by_the_next_frame_start() {
    let (doc,mut r,_data)=durable_color_renderer();
    let resident=|r:&WgpuRasterizer|r.raster_tiers.raster.used().saturating_sub(r.raster_buffers.transfer.used());
    let budget=r.factory.memory_budget().clone();let snapshot=budget.snapshot();
    let other=budget.register_owner(Kind::Other,MemoryClass::Pinned,Some(snapshot.devices[0].device));
    let held=snapshot.devices[0].limit_bytes.saturating_sub(snapshot.devices[0].pinned_bytes);
    other.report(MemoryReport {gpu_bytes:held,pinned_gpu_bytes:held,..Default::default()});
    assert!(resident(&r)>r.memory.raster_grant(),"the competing pin leaves the raster above its grant: resident {} grant {}",resident(&r),r.memory.raster_grant());
    let mut frame=packet(doc.scene(),[PAGE_SIZE*8;2]);frame.view.width_px=512;frame.view.height_px=512;
    r.begin_frame_memory(frame);
    assert!(resident(&r)<=r.memory.raster_grant(),"raster stayed above its grant past the next frame start: resident {} grant {}",resident(&r),r.memory.raster_grant());
    drop(other);
}

fn consecutive_camera_frames_retire_durable_color_after_the_raster_grant_shrinks() {
    let (doc,mut r,data)=durable_color_renderer();
    let extent=[PAGE_SIZE*8;2];
    let target=doc.active_target().unwrap();let layer_core::SourceTarget::Paint(paint)=target else {unreachable!()};
    let mut frame=packet(doc.scene(),extent);frame.view.width_px=512;frame.view.height_px=512;
    let budget=r.factory.memory_budget().clone();budget.set_ram_override(Some(384*MIB));
    r.publish_memory();budget.flush_reports();
    let pressure=budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert!(r.memory.raster_grant()<64*MIB);
    r.refresh_memory_limits();
    for step in 1..=1 {
        frame.composite_all=false;frame.reset_layers=false;frame.view.document_to_surface[4]=-(step*16) as f32;
        assert!(r.raster_dependencies_ready(frame,None),"the nearest durable camera unit stays ready");
        complete(&r);
        r.refresh_storage_metrics();
        let physical=r.raster_buffers.memory_bytes()[1]+r.paint_memory_bytes();let grant=r.memory.raster_grant();
        assert!(physical<=grant,"camera frame {step}: Raster physical={physical}, own grant={grant}");
        assert!(Arc::ptr_eq(r.native_backing(target).unwrap(),&data));
        assert_eq!(doc.artwork.paint.get(paint).unwrap().raster.wait_data().unwrap().tiles.len(),64);
    }
    drop(pressure);budget.set_ram_override(Some(2<<30));budget.flush_reports();
    submit_published(&mut r,frame);complete(&r);
    let shown=r.readback_srgb_rgba8().unwrap();
    assert!(shown.as_chunks::<4>().0.iter().all(|pixel|*pixel==[255,0,0,255]),"evicting optional color must preserve the authored picture");
}

#[test]
fn a_raster_admission_evicts_retained_buffers_before_it_is_refused() {
    let (_doc,mut r,_data)=durable_color_renderer();
    r.memory.invalidate_limits();r.refresh_memory_limits();
    let headroom=r.memory.raster.handle.gpu_working_limit().saturating_sub(r.raster_tiers.raster.used());
    r.raster_buffers.put(r.raster_buffers.take(&r.device,8*MIB));
    assert_eq!(r.raster_buffers.retained.used(),8*MIB);
    assert!(r.admit_raster_working(headroom).is_ok(),"the retained buffer must make room for authored working memory");
    assert_eq!(r.raster_buffers.retained.used(),0);
}

#[test]
fn lowered_owner_grants_release_optional_retention_within_one_frame() {
    for owner in ["Display","Decoded","Raster"] {
        match owner {
            "Display"=>crate::object_image_mips::tests::assert_optional_display_grant_decrease(),
            "Decoded"=>a_demanded_decoded_slot_survives_a_grant_shrink_while_undemanded_ones_go_first(),
            "Raster"=>consecutive_camera_frames_retire_durable_color_after_the_raster_grant_shrinks(),
            _=>unreachable!(),
        }
    }
}

#[test]
fn changed_cells_are_charged_once() {
    let mut r = legacy_scene_renderer(Default::default());
    r.refresh_memory_limits();
    let cells = r.changed_cells.as_ref().map_or(0, changed_cells::ChangedCells::storage_bytes);
    assert!(cells > 0);
    r.refresh_storage_metrics();
    let raster = r.raster_memory_report().gpu_bytes;
    let held = r.scene_tiers.display.used();
    drop(r.changed_cells.take());
    r.refresh_memory_limits();
    r.refresh_storage_metrics();
    assert_eq!(held - r.scene_tiers.display.used(), cells);
    assert_eq!(r.raster_memory_report().gpu_bytes, raster);
}

#[test]
fn display_working_admission_uses_device_headroom_instead_of_a_retention_share() {
    let mut r=legacy_scene_renderer(Default::default());
    r.memory.display.handle.report(MemoryReport {demand_gpu_bytes:1,..Default::default()});
    let share=r.memory.display.handle.grant().gpu_bytes;
    r.refresh_memory_limits();
    let bytes=share.saturating_add(r.scene_tiers.display.used()).saturating_add(1);
    assert!(r.admit_display(None,crate::residency::Class::Visible,bytes).is_ok(),
        "a retention share of {share} must not refuse required work with free device headroom");
}

#[test]
fn an_over_grant_display_admission_shrinks_the_raster_limit_before_the_raster_admits() {
    let mut r=legacy_scene_renderer(Default::default());
    r.memory.display.handle.report(MemoryReport {demand_gpu_bytes:1,..Default::default()});
    let headroom=r.memory.raster.handle.gpu_working_limit();
    let held=r.admit_display(None,crate::residency::Class::Visible,64*MIB).expect("required work may exceed the grant");
    let covered=r.scene_tiers.display.used();
    assert!(r.memory.raster.handle.gpu_working_limit().saturating_add(covered)<=headroom,
        "the ledger must take the {covered} display bytes above the grant from the raster before the raster admits");
    assert!(r.memory.display.handle.grant().gpu_bytes>=covered,"the display grant covers what it admitted");
    r.refresh_memory_limits();
    assert_eq!(r.memory.display.handle.working().gpu_bytes,0,"the next display report replaces the overdraft");
    drop(held);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_ledger_refused_display_admission_waits_without_frames_and_wakes_once_when_the_raster_releases() {
    let mut r=renderer(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()}).unwrap();
    let raster=r.admit_raster_working(512*MIB).expect("the raster holds most of the device");
    r.memory.display.handle.report(MemoryReport {demand_gpu_bytes:1,..Default::default()});
    r.scene_tiers.display.set_limit(u64::MAX,0);
    let (send,woken)=std::sync::mpsc::channel();
    r.preparation_wake=Some(Arc::new(move||{let _=send.send(());}));
    let physical=|r:&WgpuRasterizer|{
        let grants=r.factory.memory_budget().snapshot().devices.iter().map(|device|device.granted_bytes).sum::<u64>();
        let resident=r.telemetry().resident_bytes;
        assert!(resident<=grants+64*MIB,"physical {resident} exceeds grants {grants} + 64 MiB");
    };
    assert!(r.admit_display(None,crate::residency::Class::Visible,384*MIB).is_err(),"the ledger refuses what the device cannot hold");
    physical(&r);
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"a refused unit requests no frame before a release");
    r.commit_raster_working(raster);
    woken.recv_timeout(std::time::Duration::from_secs(1)).expect("the raster release wakes the refused unit");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"one release wakes the unit once");
    let held=r.admit_display(None,crate::residency::Class::Visible,384*MIB).expect("the woken unit fits");
    physical(&r);
    drop(held);
}

#[test]
fn an_over_grant_display_admission_marks_undemanded_victims_before_its_overdraft() {
    let r=legacy_scene_renderer(Default::default());
    r.memory.display.handle.report(MemoryReport {demand_gpu_bytes:1,..Default::default()});
    let notice=Arc::new(std::sync::atomic::AtomicBool::new(false));
    let victim=r.scene_tiers.display.heap().lease(&notice);victim.offer(8*MIB,0);
    let held=r.admit_display(None,crate::residency::Class::Visible,64*MIB).expect("required work may exceed the grant");
    assert!(victim.evicted(),"an over-grant admission evicts what no current demand protects before it takes the overdraft");
    drop(held);
}

#[test]
fn display_bytes_above_the_grant_keep_their_ledger_reservation_across_the_grant_pass() {
    let mut r=legacy_scene_renderer(Default::default());
    let held=r.admit_display(None,crate::residency::Class::Visible,2<<30).expect("required work may exceed the grant");
    r.refresh_memory_limits();
    let grants=r.factory.memory_budget().snapshot().devices.iter().map(|device|device.granted_bytes).sum::<u64>();
    let resident=r.telemetry().resident_bytes;
    assert!(resident<=grants+64*MIB,"physical {resident} exceeds grants {grants} + 64 MiB once the grant pass ends the overdraft");
    drop(held);
}

#[test]
fn an_optional_display_admission_stays_within_the_grant() {
    let r=legacy_scene_renderer(Default::default());
    r.memory.display.handle.report(MemoryReport {demand_gpu_bytes:1,..Default::default()});
    assert!(r.admit_display(None,crate::residency::Class::Prefetch,MIB).is_err(),"prefetch took device headroom beyond the display grant");
    assert!(r.admit_display(None,crate::residency::Class::Visible,MIB).is_ok());
}

#[test]
fn a_tablet_raster_demand_beside_the_display_floor_is_not_reduced_by_the_display_cache_target() {
    let doc = document();
    let budget = MemoryBudget::with_facts(MemoryFacts { host_default_bytes: Some(1 << 30), override_bytes: Some(1 << 30), ..Default::default() });
    let mut r = renderer_in_budget(doc.composition().color, layer_core::DeviceMemoryFacts { budget_bytes: Some(1 << 30), ..Default::default() }, budget).unwrap();
    submit_settled(&mut r, packet(doc.scene(), doc.composition().size));
    complete(&r);
    let budget = r.factory.memory_budget().clone();
    let device = budget.snapshot().devices[0];
    let raster = r.memory.raster.handle.reported();
    let room = device.limit_bytes.saturating_sub(device.pinned_bytes).saturating_sub(r.scene_tiers.display.floor()).saturating_sub(64 * MIB);
    let wanted = raster.pinned_gpu_bytes.saturating_add(room);
    r.memory.raster.handle.report(MemoryReport { demand_gpu_bytes: wanted, ..raster });
    budget.flush_reports();
    assert!(r.memory.raster_grant() >= wanted, "the display's cache target reduced the raster's working demand: wanted {wanted}, granted {}, display {:?}, device {device:?}",
        r.memory.raster_grant(), r.memory.display.handle.reported());
}

fn presented_floor(frame: FramePacket<'_>) -> u64 {
    let format = crate::display_precision::DisplayPrecision::for_scene(frame.scene).format();
    let page = u64::from(PAGE_SIZE).pow(2) * u64::from(format.block_copy_size(None).unwrap());
    let visible = display_mips::view_bounds(frame.view, frame.document_extent, 0).unwrap();
    let level = display_mips::view_level(frame.view.document_to_surface, display_mips::MAX_LEVEL).unwrap();
    let side = PAGE_SIZE << level;
    let cells = u64::from(visible.max_x().div_ceil(side) - visible.min_x() / side) * u64::from(visible.max_y().div_ceil(side) - visible.min_y() / side);
    2 * cells * page
}

fn optional_headroom(r: &WgpuRasterizer, floor: u64) -> u64 {
    let free = r.scene_tiers.display.limit().saturating_sub(r.scene_tiers.display.used());
    assert!(free > floor, "the test grant {free} must exceed the presented floor {floor}");
    free - floor + 1
}

#[test]
fn the_display_floor_holds_across_frames_without_a_required_request() {
    let doc = document();
    let mut r = legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), [512; 2]);
    frame.view = crate::test_support::view([1024, 768]);
    r.begin_frame_memory(frame);
    r.begin_frame_memory(frame);
    let bytes = optional_headroom(&r, presented_floor(frame));
    assert!(r.admit_display(None, crate::residency::Class::Prefetch, bytes).is_err(), "prefetch took the presented and held cells' floor");
    assert!(r.admit_display(None, crate::residency::Class::Visible, bytes).is_ok(), "required work may use the floor");
}

#[test]
fn the_display_floor_follows_the_view_rather_than_the_largest_request() {
    let doc = document();
    let mut r = legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), [512; 2]);
    frame.view = crate::test_support::view([1024, 768]);
    r.begin_frame_memory(frame);
    let _authored = r.admit_display(None, crate::residency::Class::Authored, MIB).unwrap();
    let bytes = optional_headroom(&r, presented_floor(frame));
    assert!(r.admit_display(None, crate::residency::Class::Prefetch, bytes).is_err(), "a small authored request shrank the floor below the view's cells");
}

#[test]
fn the_display_tier_is_the_sum_of_its_charges_across_a_frame() {
    let mut r = legacy_scene_renderer(Default::default());
    r.refresh_memory_limits();
    let base = r.scene_tiers.display.used();
    let held = r.scene_tiers.display.charge(3 * MIB);
    let lease = r.reserve_effect_analysis(5 * MIB).unwrap();
    r.refresh_memory_limits();
    assert_eq!(r.scene_tiers.display.used(), base + 8 * MIB, "a direct charge and an analysis lease are both counted");
    drop((held, lease));
    r.refresh_memory_limits();
    assert_eq!(r.scene_tiers.display.used(), base);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn refused_display_admission_is_woken_by_a_released_charge() {
    let mut r=legacy_scene_renderer(Default::default());
    let (send,receive)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=send.send(());}));
    r.scene_tiers.display.set_limit(r.scene_tiers.display.used()+8,0);
    let held=r.scene_tiers.display.charge(8);
    while receive.try_recv().is_ok() {}
    assert!(r.admit_display(None,crate::residency::Class::Visible,1).is_err());
    drop(held);
    receive.recv_timeout(std::time::Duration::from_secs(1)).expect("the refused display unit must subscribe to release");
}

fn raster_headroom(r: &mut WgpuRasterizer) -> u64 {
    let (mut fits, mut refused) = (0u64, 1u64 << 44);
    while refused - fits > 1 {
        let bytes = fits + (refused - fits) / 2;
        if r.admit_raster_working(bytes).is_ok() { fits = bytes; } else { refused = bytes; }
    }
    fits
}

#[test]
fn raster_admission_counts_pages_and_retained_buffers_from_creation() {
    let mut r = legacy_scene_renderer(Default::default());
    let headroom = raster_headroom(&mut r);
    let page = r.create_page([0, 0], "unpublished raster page");
    assert!(r.admit_raster_working(headroom).is_err(), "a page counts from its creation, before any layer holds it");
    let headroom = raster_headroom(&mut r);
    r.refresh_memory_limits();
    let held = r.raster_tiers.raster.used();
    r.raster_buffers.put(r.device.create_buffer(&wgpu::BufferDescriptor { label: Some("retained raster buffer"), size: 64 << 10,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }));
    assert_eq!(r.raster_tiers.raster.used() - held, 64 << 10, "a retained buffer counts while the pool holds it");
    assert!(r.admit_raster_working(headroom).is_ok(), "the retained buffer yields to authored working memory");
    assert_eq!(r.raster_buffers.retained.used(), 0);
    drop(page);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn refused_raster_admission_is_woken_by_a_released_page() {
    let mut r = legacy_scene_renderer(Default::default());
    let page = r.create_page([0, 0], "released raster page");
    let (send, receive) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = send.send(()); }));
    while receive.try_recv().is_ok() {}
    r.raster_tiers.raster.update_limit(r.memory.raster.handle.gpu_working_limit());
    let room = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used());
    assert!(r.admit_raster_working(room + 1).is_err());
    drop(page);
    receive.recv_timeout(std::time::Duration::from_secs(1)).expect("a release that makes the refused raster unit fit must wake it");
}

#[test]
fn a_replaced_renderer_reports_none_of_the_display_its_successor_adopted() {
    let mut previous = legacy_scene_renderer(Default::default());
    let adopted = previous.scene_tiers.display.charge(64 * MIB);
    let factory = previous.factory.clone();
    let mut next = WgpuRasterizer::from_factory_inner(factory.clone(), factory.pipeline_device(Default::default()), crate::Initialization::Headless).unwrap();
    layer_render::CanvasRenderer::adopt_committed_pages(&mut *next, &mut previous);
    assert!(next.current_display_memory_report().gpu_bytes >= adopted.bytes(), "the successor reports the display it adopted");
    assert!(previous.current_display_memory_report().gpu_bytes < adopted.bytes(), "the replaced renderer still reports, and on retirement pins, the display it gave away");
    drop(adopted);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_retired_display_owner_ends_with_its_queue_while_adopted_state_shares_its_device() {
    let previous = legacy_scene_renderer(Default::default());
    let budget = previous.factory.memory_budget().clone();
    let scenes = |budget: &MemoryBudget| budget.snapshot().rows[Kind::Scene as usize].iter().map(|row| row.owners).sum::<usize>();
    assert_eq!(scenes(&budget), 1);
    let adopted = previous.device.clone();
    drop(previous);
    adopted.wait_queue().unwrap();
    assert_eq!(scenes(&budget), 0, "state that keeps a device clone must not keep the retired renderer's display owner and its retirement pins");
    drop(adopted);
}

#[test]
fn resident_presented_cells_leave_the_floor_to_refinement() {
    let doc = document();
    let mut r = legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), [512; 2]);
    frame.view = crate::test_support::view([256, 256]);
    submit_settled(&mut r, frame);
    r.begin_frame_memory(frame);
    let cells = display_floor(frame, 0, WORKING_BYTES) - WORKING_BYTES - IN_FLIGHT_DECODE_BYTES;
    let presented = r.composition_memory(r.scene.as_deref())[1];
    assert!(presented >= cells, "the settled view keeps its presented cells resident: presented {presented}, cells {cells}");
    let display = r.scene_tiers.display.clone();
    display.update_limit(display.used() + MIB + WORKING_BYTES + IN_FLIGHT_DECODE_BYTES);
    assert!(display.admit(crate::residency::Class::Refinement, MIB).is_ok(), "the floor reserved the resident presented cells a second time");
}

fn retained_photo_display() -> (layer_core::Document, WgpuRasterizer, layer_render::ViewState) {
    let mut doc = crate::tests::native_effects::empty_document([4096; 2], Default::default());
    crate::tests::native_effects::insert_source(&mut doc, "Photo", layer_core::color::source::rgba8_source([4096; 2], |x, y| [(x % 251) as u8, (y % 241) as u8, 96, 255]));
    let mut r = legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), [4096; 2]);
    frame.view = crate::test_support::view([256, 256]);
    for translation in [[0., 0.], [-2048., -2048.], [0., -2048.]] {
        frame.view.document_to_surface = [1., 0., 0., 1., translation[0], translation[1]];
        submit_settled(&mut r, frame);
    }
    complete(&r);
    let view = frame.view;
    (doc, r, view)
}

fn pin_the_free_device(r: &WgpuRasterizer) -> MemoryOwner {
    let budget = r.factory.memory_budget();
    let snapshot = budget.snapshot();
    let other = budget.register_owner(Kind::Other, MemoryClass::Pinned, Some(snapshot.devices[0].device));
    let held = snapshot.devices[0].limit_bytes.saturating_sub(snapshot.devices[0].pinned_bytes);
    other.report(MemoryReport { gpu_bytes: held, pinned_gpu_bytes: held, ..Default::default() });
    other
}

fn display_resident(r: &WgpuRasterizer) -> u64 { r.scene_tiers.display.used().saturating_sub(r.scene_tiers.retiring.used()) }

#[test]
fn retained_display_pages_above_the_display_grant_are_trimmed_by_the_next_frame() {
    let (doc, mut r, view) = retained_photo_display();
    let moved = FramePacket { view, ..packet(doc.scene(), [4096; 2]) };
    let [stored, pinned] = r.scene.as_ref().unwrap().retained_scale_bytes();
    assert!(stored > pinned, "the first view's pages stay retained beside the presented and held ones");
    let other = pin_the_free_device(&r);
    r.factory.memory_budget().flush_reports();
    let grant = r.memory.display.handle.grant().gpu_bytes;
    let used = r.scene_tiers.display.used();
    assert!(used > grant, "the competing pin leaves the display grant below its retained charges");
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, held) = std::sync::mpsc::channel::<()>();
    r.device.retirement_handle().run(move || { let _ = entered.send(()); let _ = held.recv_timeout(crate::READBACK_TIMEOUT); });
    ready.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    r.begin_frame_memory(moved);
    let [kept, _] = r.scene.as_ref().unwrap().retained_scale_bytes();
    assert!(kept < stored, "retention above the display grant survived the frame: stored {stored} kept {kept} pinned {pinned} grant {grant}");
    let target = stored.saturating_sub(used.saturating_sub(grant).saturating_sub(r.scene_tiers.display.heap().pending()));
    assert!(kept + u64::from(crate::PAGE_SIZE).pow(2) * 16 > target, "pages still retiring were trimmed for again: kept {kept} target {target}");
    r.begin_frame_memory(moved);
    assert_eq!(r.scene.as_ref().unwrap().retained_scale_bytes()[0], kept, "one frame releases every evictable page above the grant");
    release.send(()).unwrap();
    r.device.retirement_handle().flush().unwrap();
    drop(other);
}

#[test]
fn an_idle_display_above_a_lowered_grant_wakes_and_releases_its_retention() {
    let (doc, mut r, view) = retained_photo_display();
    let frame = FramePacket { view, ..packet(doc.scene(), [4096; 2]) };
    let [stored, _] = r.scene.as_ref().unwrap().retained_scale_bytes();
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = woken.clone();
    r.set_preparation_wake(Arc::new(move || { count.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }));
    assert_ne!(r.next_frame(), layer_render::FrameDemand::Work);
    let other = pin_the_free_device(&r);
    assert!(r.scene_tiers.display.used() > r.memory.retention_grant(), "the competing pin leaves the display above its grant");
    assert!(woken.load(std::sync::atomic::Ordering::SeqCst) > 0, "a lowered grant wakes the idle renderer");
    assert_eq!(r.next_frame(), layer_render::FrameDemand::Work, "retention above the grant is outstanding work");
    for _ in 0..8 {
        if r.next_frame() != layer_render::FrameDemand::Work { break; }
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        complete(&r);
    }
    let kept = r.scene.as_ref().unwrap().retained_scale_bytes()[0];
    assert!(kept < stored, "the idle display released its retention: stored {stored} kept {kept}");
    assert_ne!(r.next_frame(), layer_render::FrameDemand::Work, "an idle display does not spin on bytes it cannot release");
    drop(other);
}

#[test]
fn a_frame_that_cannot_submit_still_brings_the_display_under_a_lowered_grant() {
    let (_doc, mut r, view) = retained_photo_display();
    let other = pin_the_free_device(&r);
    assert!(display_resident(&r) > r.memory.retention_grant(), "the competing pin leaves the display above its grant");
    r.background_refinement = false;
    r.background_ready.store(false, std::sync::atomic::Ordering::Release);
    assert!(!r.can_submit(), "the frame cannot submit");
    r.poll_pending(view).unwrap();
    assert!(display_resident(&r) <= r.memory.retention_grant(), "a frame that cannot submit kept the display above its grant: resident {} grant {}",
        display_resident(&r), r.memory.retention_grant());
    drop(other);
}

#[test]
fn an_unchanged_idle_frame_creates_no_grant_encoder_and_syncs_no_report() {
    let (_doc, mut r, view) = retained_photo_display();
    r.poll_pending(view).unwrap();
    let before = crate::test_support::PassCounts::snapshot();
    for _ in 0..3 { r.poll_pending(view).unwrap(); }
    let counts = crate::test_support::PassCounts::snapshot().delta(before);
    assert_eq!([counts.grant_encoders, counts.report_syncs], [0, 0], "unchanged idle frames ran the grant pass");
}

#[test]
fn changed_frames_request_no_memory_sample_and_any_reader_gets_one() {
    let (doc, mut r, mut view) = retained_photo_display();
    for (enabled, translation) in [(false, [-1024., 0.]), (true, [-1024., -1024.])] {
        r.set_telemetry_enabled(enabled);
        view.document_to_surface = [1., 0., 0., 1., translation[0], translation[1]];
        let before = crate::test_support::PassCounts::snapshot();
        submit_settled(&mut r, FramePacket { view, ..packet(doc.scene(), [4096; 2]) });
        let requests = crate::test_support::PassCounts::snapshot().delta(before).memory_updates;
        assert_eq!(requests, 0, "telemetry {enabled}: frames requested {requests} memory sample refreshes");
    }
    r.set_telemetry_enabled(false);
    let deadline = std::time::Instant::now() + crate::READBACK_TIMEOUT;
    while r.telemetry().memory_budget.is_none() {
        assert!(std::time::Instant::now() < deadline, "a renderer stats reader never received a memory sample");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_recomposed_frame_with_unchanged_inputs_prepares_no_composition_graph() {
    let (doc, mut r, view) = retained_photo_display();
    let frame = FramePacket { view, composite_all: true, ..packet(doc.scene(), [4096; 2]) };
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    complete(&r);
    let before = crate::test_support::PassCounts::snapshot();
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).graph_prepares, 0, "a frame rebuilt a composition graph whose inputs did not change");
}

#[test]
fn a_submitted_frame_sets_its_display_room_once() {
    let (doc, mut r, view) = retained_photo_display();
    let frame = FramePacket { view, ..packet(doc.scene(), [4096; 2]) };
    for _ in 0..2 {
        r.poll_pending(view).unwrap();
        let before = crate::test_support::PassCounts::snapshot();
        let std::task::Poll::Ready(result) = r.submit_when_ready(frame, None) else { panic!("the settled view is ready") };
        result.unwrap();
        assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).display_rooms, 1, "a frame published and flushed its display room more than once");
        complete(&r);
    }
}

#[test]
fn telemetry_counts_each_composition_window_and_strip_once() {
    let extent = [1033, 517];
    let mut threshold = (*crate::tests::fixture("threshold").program()).clone();
    threshold.id = "unbundled threshold".into();
    let mut doc = crate::tests::native_effects::empty_document(extent, Default::default());
    crate::tests::native_effects::insert_effect(&mut doc, layer_core::EffectInstance::new(Arc::new(threshold)));
    crate::tests::native_effects::insert_source(&mut doc, "Photo", layer_core::color::source::rgba8_source(extent, |x, y| [(x % 251) as u8, (y % 241) as u8, 96, 255]));
    let mut r = legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene().with_owner(0, 0), extent);
    frame.view.width_px = 43;
    frame.view.height_px = 25;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, -152.5, -67.5];
    submit_published(&mut r, frame);
    let target = doc.scene().order().iter().find_map(|handle| doc.scene().source_target(*handle)).unwrap();
    let dab = crate::tests::test_dab([160., 70.], [0.9, 0.2, 0.1, 1.], 1.);
    let batch = layer_render::DabBatch { stroke_start: true, stroke_end: true, ..dab_batch(target, crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen), dab.bounds()) };
    submit_published(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame });
    r.wait_idle().unwrap();
    assert!(r.scale_display.as_ref().unwrap().charged_scratch_bytes() > 0, "a stroke composes the unbundled effect through scratch and the exact strip");
    let resident = |r: &mut WgpuRasterizer| { r.refresh_storage_metrics(); layer_render::CanvasRenderer::telemetry(r).resident_bytes };
    let [telemetry, display] = [resident(&mut r), r.scene_tiers.display.used()];
    drop(r.scale_display.take());
    assert_eq!(telemetry - resident(&mut r), display - r.scene_tiers.display.used(),
        "telemetry must release exactly the display charges of the windows, scratch and strip it drops");
}

#[test]
fn the_raster_report_counts_a_page_from_its_creation_without_a_storage_walk() {
    let r = legacy_scene_renderer(Default::default());
    let before = r.raster_memory_report().gpu_bytes;
    let page = r.create_page([0, 0], "unlisted raster page");
    assert_eq!(r.raster_memory_report().gpu_bytes - before, page.primary.storage_bytes(), "the raster owner reports a page while it exists, before any layer lists it");
    drop(page);
    assert_eq!(r.raster_memory_report().gpu_bytes, before);
}

#[test]
fn zero_pages_and_remembered_selection_pixels_are_charged_to_the_paint_tier() {
    let mut r = legacy_scene_renderer(Default::default());
    let fixed = r.reservoir.primary.storage_bytes() + r.reservoir.secondary.storage_bytes()
        + r.declared_scratch.iter().map(PageSurface::storage_bytes).sum::<u64>() + r.zero_pages.iter().map(texture_bytes).sum::<u64>();
    assert_eq!(r.raster_tiers.paint.used(), fixed, "the paint tier holds the immutable zero pages from creation");
    let pixels = Arc::new(layer_core::SelectionPixels::new([8, 1], [0, 0, 8, 1], vec![0x4444]).unwrap());
    let buffer = r.device.create_buffer(&wgpu::BufferDescriptor { label: Some("remembered selection coverage"), size: MIB,
        usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
    r.selection_clip.remember_pixels(&pixels, buffer);
    assert_eq!(r.raster_tiers.paint.used(), fixed + MIB, "remembered selection coverage is charged while the clip keeps it");
    drop(pixels);
    r.selection_clip.reset();
    assert_eq!(r.raster_tiers.paint.used(), fixed, "released coverage leaves the paint tier");
}

#[test]
fn liquify_maps_are_charged_to_the_paint_tier_when_created() {
    let r = legacy_scene_renderer(Default::default());
    let before = r.raster_tiers.paint.used();
    let liquify = crate::liquify::Liquify::new(&r);
    let map = u64::from(PAGE_SIZE * PAGE_SIZE) * u64::from(wgpu::TextureFormat::Rgba32Uint.block_copy_size(None).unwrap());
    let color = u64::from(PAGE_SIZE * PAGE_SIZE) * u64::from(r.device.working_format().block_copy_size(None).unwrap());
    assert_eq!(r.raster_tiers.paint.used() - before, 3 * map + 2 * color, "the empty and scratch maps are charged beside the color gathers");
    drop(liquify);
    assert_eq!(r.raster_tiers.paint.used(), before);
}

#[test]
fn a_transform_spare_page_is_charged_only_to_the_paint_tier() {
    let mut r = legacy_scene_renderer(Default::default());
    r.transforms.as_mut().unwrap().begin_frame();
    r.refresh_memory_limits();
    let [display, paint] = [r.scene_tiers.display.used(), r.raster_tiers.paint.used()];
    let page = r.create_page([0, 0], "transform spare page");
    let bytes = page.primary.storage_bytes();
    r.transforms.as_mut().unwrap().hold_spare(page);
    r.transforms.as_mut().unwrap().begin_frame();
    r.refresh_memory_limits();
    assert_eq!(r.raster_tiers.paint.used() - paint, bytes);
    assert_eq!(r.scene_tiers.display.used(), display, "a page that left the live preview is counted once, on the paint tier");
}

fn decoded_source(tiles: u32) -> Arc<layer_core::color::source::SourceImage> {
    layer_core::color::source::rgba8_source([tiles.min(16) * PAGE_SIZE, tiles.div_ceil(16) * PAGE_SIZE], |x, y| {
        let id = (y / PAGE_SIZE * 16 + x / PAGE_SIZE) as u8;
        [id.wrapping_mul(17), id.wrapping_mul(43), id.wrapping_mul(97), 255]
    })
}

fn decoded_renderer(source: &Arc<layer_core::color::source::SourceImage>) -> WgpuRasterizer {
    let r = renderer(Default::default(), layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    pollster::block_on(r.compilation.prepare_all(crate::startup::SourceKinds::for_source(source).pipelines(&r.scene_pipelines.source))).unwrap();
    r
}

fn tile(x: u32) -> [u32; 2] { [x % 16, x / 16] }

fn decode_at(r: &mut WgpuRasterizer, source: &Arc<layer_core::color::source::SourceImage>, x: u32, class: crate::residency::Class) {
    let mut requested = scene::sources::Inputs::default();
    requested.image(source, tile(x), r.document_color().space);
    let ((), accepted) = requested.accept(r, &mut (), |_, r, inputs| {
        let bytes = inputs.additional_bytes(&r.source_tiles.borrow());
        r.admit_display(None, class, bytes).ok()
    }, |_, _| Ok(()), |_, _, _| Ok(())).unwrap().expect("the decode fits the display grant");
    let read = scene::sources::accepted_image(&accepted, source, tile(x)).unwrap();
    if !read.pending.is_empty() {
        let pixels = crate::prepare_image_tile(source, tile(x), r.document_color().space, &r.device.source_samples, None).unwrap();
        for pending in &read.pending { pending.set_prepared(Some(pixels.clone())); }
    }
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    assert!(read.encode(r, &mut encoder, class).unwrap());
    accepted.track(&mut encoder);
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    complete(r);
}

fn resident(r: &WgpuRasterizer, source: &Arc<layer_core::color::source::SourceImage>, x: u32) -> bool {
    r.source_tiles.borrow().prepared_view(source, tile(x)).is_some()
}

fn demand_the_first_decode_above_more_recent_lower_class_ones(r: &mut WgpuRasterizer, frame: FramePacket<'_>,
    source: &Arc<layer_core::color::source::SourceImage>, tiles: u32) {
    r.begin_frame_memory(frame);
    for x in 0..tiles { decode_at(r, source, x, crate::residency::Class::Visible); }
    for _ in 0..2 {
        r.end_frame_memory();
        r.begin_frame_memory(frame);
        decode_at(r, source, 0, crate::residency::Class::Visible);
        for x in 1..tiles { decode_at(r, source, x, crate::residency::Class::Refinement); }
    }
    assert!((0..tiles).all(|x| resident(r, source, x)), "the fixture holds every decode before the grant changes");
}

#[test]
fn a_demanded_decoded_slot_survives_a_grant_shrink_while_undemanded_ones_go_first() {
    let doc = document();
    let source = decoded_source(24);
    let mut r = decoded_renderer(&source);
    let frame = packet(doc.scene(), doc.composition().size);
    demand_the_first_decode_above_more_recent_lower_class_ones(&mut r, frame, &source, 24);
    r.memory.display.handle.report(MemoryReport { demand_gpu_bytes: 1, ..Default::default() });
    r.factory.memory_budget().flush_reports();
    r.begin_frame_memory(frame);
    complete(&r);
    r.device.retirement_handle().flush().unwrap();
    assert!(resident(&r, &source, 0), "the slot a Visible unit demanded this frame survives the shrink");
    assert!((1..24).all(|x| !resident(&r, &source, x)), "the shrunk grant takes every slot no current demand at Fallback protects");
}

#[test]
fn the_end_of_a_frame_releases_the_decodes_its_trim_takes_from_a_lowered_grant() {
    let doc = document();
    let source = decoded_source(96);
    let mut r = decoded_renderer(&source);
    let frame = packet(doc.scene(), doc.composition().size);
    r.begin_frame_memory(frame);
    for x in 0..96 { decode_at(&mut r, &source, x, crate::residency::Class::Visible); }
    r.end_frame_memory();
    let other = pin_the_free_device(&r);
    assert!(display_resident(&r) > r.memory.retention_grant(), "the competing pin leaves the decodes above the display grant");
    r.fit_grants();
    assert!(display_resident(&r) <= r.memory.retention_grant(), "the frame ended with its trimmed decodes still resident: resident {} grant {}",
        display_resident(&r), r.memory.retention_grant());
    drop(other);
}

#[test]
fn navigator_and_restore_decodes_are_not_decoded_again_at_a_fixed_view_under_a_tablet_grant() {
    let doc = document();
    let source = decoded_source(192);
    let mut r = decoded_renderer(&source);
    let frame = packet(doc.scene(), doc.composition().size);
    let class = |x: u32| if x % 2 == 0 { crate::residency::Class::Navigator } else { crate::residency::Class::Authored };
    let mut misses = Vec::new();
    for _ in 0..3 {
        r.begin_frame_memory(frame);
        let before = r.source_tiles.borrow().misses;
        for x in 0..192 { decode_at(&mut r, &source, x, class(x)); }
        misses.push(r.source_tiles.borrow().misses - before);
        r.end_frame_memory();
    }
    assert_eq!(misses, [192, 0, 0], "a fixed view decodes each tile once; display {} of grant {}",
        r.scene_tiers.display.used(), r.memory.display.handle.grant().gpu_bytes);
}

#[test]
fn a_navigator_admission_at_a_full_grant_evicts_undemanded_decodes_while_a_visible_demanded_one_survives() {
    let doc = document();
    let source = decoded_source(9);
    let mut r = decoded_renderer(&source);
    let frame = packet(doc.scene(), doc.composition().size);
    demand_the_first_decode_above_more_recent_lower_class_ones(&mut r, frame, &source, 8);
    let full = r.scene_tiers.display.used() + r.scene_tiers.display.floor();
    r.memory.display.handle.report(MemoryReport { demand_gpu_bytes: full, ..Default::default() });
    r.factory.memory_budget().flush_reports();
    assert_eq!(r.memory.display.handle.grant().gpu_bytes, full);
    let mut requested = scene::sources::Inputs::default();
    requested.image(&source, [8, 0], r.document_color().space);
    let bytes = requested.additional_bytes(&r.source_tiles.borrow());
    assert!(r.admit_display(None, crate::residency::Class::Navigator, bytes).is_err(), "the grant is full for optional work");
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.retire_victims(None, &mut encoder);
    encoder.submit(&r.queue);
    complete(&r);
    r.device.retirement_handle().flush().unwrap();
    assert!(r.admit_display(None, crate::residency::Class::Navigator, bytes).is_ok(), "the miss evicted undemanded decodes for the navigator");
    assert!(resident(&r, &source, 0), "the Visible-demanded slot is not a navigator victim");
    assert!((1..8).any(|x| !resident(&r, &source, x)), "an undemanded decode went instead");
}

fn stacked_document(layers: usize) -> layer_core::Document {
    let mut doc = document();
    for layer in 0..layers { crate::test_support::add_paint(&mut doc.artwork, format!("Layer {layer}"), [512; 2]); }
    layer_core::Document::from_artwork(doc.artwork).unwrap()
}

#[test]
fn a_drained_display_pin_covers_one_full_stack_unit_of_decoded_inputs() {
    let doc = stacked_document(60);
    let mut r = renderer(doc.composition().color, layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    r.prepare_capture_memory(doc.scene(), doc.composition().size);
    let unit = 60 * scene::sources::NORMALIZED_PAGE_BYTES;
    let pinned = r.memory.demand.get().pinned(r.display_usage().pinned)[3];
    assert!(pinned >= unit, "the decoded-input pin {pinned} must hold one 60-layer page composite");
    let budget = r.factory.memory_budget().clone();
    let _drain = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    r.publish_memory();
    budget.flush_reports();
    r.refresh_memory_limits();
    assert!(r.source_tiles.borrow().free_slots() as u64 >= 60, "a drained ledger keeps room for one full-stack unit: {} pages", r.source_tiles.borrow().free_slots());
}

#[test]
fn a_capture_step_ends_the_demand_epoch_its_decodes_were_stamped_in() {
    let source = decoded_source(1);
    let mut r = decoded_renderer(&source);
    decode_at(&mut r, &source, 0, crate::residency::Class::Visible);
    assert_eq!(r.scene_tiers.display.heap().evict(u64::MAX, crate::residency::Class::Visible), 0, "a decode is demanded in the step that acquired it");
    let mut scene = scene::Scene::new(&r).unwrap();
    pollster::block_on(scene.prepare_capture_work_async(&mut r)).unwrap();
    assert!(r.scene_tiers.display.heap().evict(u64::MAX, crate::residency::Class::Visible) > 0, "the next capture step may evict the consumed decode");
}

#[test]
fn the_display_tier_floor_and_the_ledger_pin_hold_the_same_unit() {
    let doc = stacked_document(60);
    let mut r = legacy_scene_renderer(doc.composition().color);
    let frame = packet(doc.scene(), doc.composition().size);
    r.begin_frame_memory(frame);
    let unit = r.memory.unit();
    assert!(unit >= (60 + SCRATCH_PAGES) * scene::sources::NORMALIZED_PAGE_BYTES, "one full-stack page composite: {unit}");
    let usage = r.display_usage().pinned;
    assert_eq!(r.memory.demand.get().pinned(usage)[3] - usage[3], unit, "the ledger pin holds the unit");
    let presented = r.composition_memory(r.scene.as_deref())[1];
    assert_eq!(r.scene_tiers.display.floor(), display_floor(frame, presented, unit), "the tier floor holds the same unit");
}

#[test]
fn a_display_change_below_a_mebibyte_reaches_the_ledger_at_the_next_frame() {
    let doc = document();
    let mut r = legacy_scene_renderer(doc.composition().color);
    let frame = packet(doc.scene(), [512; 2]);
    submit_settled(&mut r, frame);
    r.begin_frame_memory(frame);
    let held = r.retired_display_bytes.charge(MIB / 2);
    r.begin_frame_memory(frame);
    assert_eq!(r.memory.display.handle.reported().pinned_gpu_bytes, r.current_display_memory_report().pinned_gpu_bytes, "the next frame reports the held display bytes as they are");
    drop(held);
}

#[test]
fn the_display_report_and_grant_pass_walk_no_composition_window() {
    let doc = document();
    let mut r = legacy_scene_renderer(doc.composition().color);
    let frame = packet(doc.scene(), [512; 2]);
    submit_settled(&mut r, frame);
    assert!(r.scale_display.is_some() && r.navigator.storage_bytes() > 0, "the fixture presents a view and a navigator");
    let counts = crate::test_support::PassCounts::snapshot();
    r.publish_memory();
    r.memory.invalidate_limits();
    r.refresh_memory_limits();
    r.begin_frame_memory(frame);
    assert_eq!(crate::test_support::PassCounts::snapshot().delta(counts).display_walks, 0, "the report reads window counters, not windows");
}

fn layered_raster_document(layers:u8,tiles:u32)->layer_core::Document {
    use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
    let extent=[PAGE_SIZE*tiles;2];
    let mut doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    for layer in 0..layers {
        let (_,layer_core::SourceTarget::Paint(paint))=crate::test_support::add_paint(&mut doc.artwork,format!("Layer {layer}"),extent) else {unreachable!()};
        let data=RasterData {tiles:(0..tiles).flat_map(|y|(0..tiles).map(move|x|[x,y])).map(|[x,y]| {
            let bytes=[layer.wrapping_mul(29).wrapping_add((x as u8).wrapping_mul(7)),(y as u8).wrapping_mul(13).wrapping_add(layer),((x+y) as u8).wrapping_mul(5).wrapping_add(layer),128].repeat(PAGE_SIZE.pow(2) as usize);
            (TileKey {plane:RasterPlane::Color,coordinate:[x,y]},RasterTile::backed(TileBlob::encode(doc.composition().color.paint_descriptor(),&bytes).unwrap()))
        }).collect(),watercolor:None};
        doc.artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(data);
    }
    layer_core::Document::from_artwork(doc.artwork).unwrap()
}

#[test]
fn a_settled_reduced_view_keeps_no_decoded_layer_tiles_after_its_units_commit() {
    let doc=layered_raster_document(8,4);
    let extent=doc.composition().size;
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.view.width_px=256;frame.view.height_px=256;
    frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    submit_settled(&mut r,frame);complete(&r);
    for _ in 0..2 {crate::test_support::submit_ready(&mut r,FramePacket {composite_all:false,..frame}).unwrap();complete(&r);}
    let sources=r.source_tiles.borrow();
    let decoded=sources.gpu_bytes()-sources.non_slot_bytes();
    assert!(sources.misses>=8*16,"every layer tile was decoded once: {}",sources.misses);
    assert!(decoded<=r.scene_tiers.display.floor(),"{} MiB of decoded layer tiles stay above the view floor after their units committed",decoded>>20);
}

fn settle_observing(r:&mut WgpuRasterizer,frame:FramePacket<'_>,mut observe:impl FnMut(&WgpuRasterizer)) {
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(120);
    let mut submitted=false;
    loop {
        r.poll_startup().unwrap();r.poll_pending(frame.view).unwrap();
        let next=if !submitted {frame}else {FramePacket {composite_all:false,..frame}};
        let preview=r.transform_preview.clone();
        if r.can_submit() && let std::task::Poll::Ready(result)=r.submit_when_ready(next,preview.as_ref()) {result.unwrap();submitted=true;observe(r);}
        r.wait_idle().unwrap();
        if submitted && r.next_frame()!=layer_render::FrameDemand::Work && r.settled() {return;}
        assert!(std::time::Instant::now()<deadline,"the view must settle");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn reduced_32_layer_view(doc:&layer_core::Document)->FramePacket<'_> {
    let mut frame=packet(doc.scene(),doc.composition().size);frame.view.width_px=512;frame.view.height_px=512;
    frame.view.document_to_surface=[1./3.,0.,0.,1./3.,0.,0.];frame
}

#[test]
fn a_surface_presenter_charges_its_presented_images_to_the_device() {
    let r=renderer(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    let budget=r.factory.memory_budget().clone();
    let pinned=|budget:&MemoryBudget| {budget.flush_reports();budget.snapshot().devices[0].pinned_bytes};
    let before=pinned(&budget);
    let mut presenter=crate::ViewportPresenter::for_surface(&r,wgpu::TextureFormat::Rgba8UnormSrgb,crate::SdrSurfaceColor::Srgb).unwrap();
    pollster::block_on(presenter.prepare(&r)).unwrap();
    let target=crate::create_target(&r.device,[800,600],wgpu::TextureFormat::Rgba8UnormSrgb,"surface image").0;
    presenter.present(&r,&target.create_view(&Default::default()),crate::test_support::view([800,600]),[1.;4]).unwrap();
    assert_eq!(pinned(&budget)-before,800*600*4*3,"three presented surface images stay pinned on the device");
    drop(presenter);
    assert_eq!(pinned(&budget),before);
}

#[test]
fn refinement_leases_decoded_inputs_only_for_the_cells_one_frame_composes() {
    const LAYERS:u8=32;
    let doc=layered_raster_document(LAYERS,6);
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    submit_settled(&mut r,reduced_32_layer_view(&doc));complete(&r);
    let mut artwork=doc.artwork.clone();
    let layer=doc.scene().order().iter().copied().find(|handle|doc.scene().occurrence(*handle).unwrap().name.as_ref()=="Layer 0").unwrap();
    artwork.occurrences.get_mut(layer).unwrap().opacity=0.5;
    let changed=layer_core::Document::from_artwork(artwork).unwrap();
    let mut leased=0;
    settle_observing(&mut r,reduced_32_layer_view(&changed),|r|leased=leased.max(r.source_tiles.borrow().leased_bytes()));
    let unit=u64::from(LAYERS)*scene::sources::NORMALIZED_PAGE_BYTES;
    assert!(leased<=8*unit,"refinement leased {} MiB of decoded inputs at once, {} cells of {LAYERS} layers; one frame composes a few",leased>>20,leased/unit);
}

#[test]
fn a_cold_32_layer_open_decodes_each_present_tile_once_and_reduces_no_layer() {
    let doc=layered_raster_document(32,6);
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    let (mut layer_pages,mut leased)=(std::collections::BTreeSet::new(),0);
    settle_observing(&mut r,reduced_32_layer_view(&doc),|r| {
        layer_pages.extend(crate::scene::scale::layer_pages(r));leased=leased.max(r.source_tiles.borrow().leased_bytes());
    });
    let present=32*36;
    assert!(layer_pages.is_empty(),"a cold exact open reduces no layer on its own: {layer_pages:?}");
    assert_eq!(r.source_tiles.borrow().misses,present,"each present tile is decoded once");
    let unit=32*scene::sources::NORMALIZED_PAGE_BYTES;
    let frame_budget=(8<<20)/(u64::from(PAGE_SIZE).pow(2)*32)+1;
    assert!(leased<=frame_budget*unit,"the open leased {} MiB of decoded inputs at once, {} cells of 32 layers",leased>>20,leased/unit);
}

#[test]
fn a_cold_32_layer_open_admits_decodes_by_working_set_instead_of_per_frame() {
    let doc=layered_raster_document(32,6);
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    let (mut seen,mut widest,mut heaviest,mut frames)=(0,0,0,0);
    settle_observing(&mut r,reduced_32_layer_view(&doc),|r| {
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(60);
        while let Some((_,queue))=&r.moving_decode {
            heaviest=heaviest.max(queue.working_bytes());
            if queue.decoded() {break;}
            assert!(std::time::Instant::now()<deadline,"admitted decodes must finish");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let misses=r.source_tiles.borrow().misses;
        widest=widest.max(misses-seen);seen=misses;frames+=1;
    });
    let bound=(crate::object_image_mips::decode_workers() as u64+128)*u64::from(PAGE_SIZE).pow(2)*4;
    assert_eq!(r.source_tiles.borrow().misses,32*36,"each present tile is decoded once");
    assert!(widest>18,"a frame whose workers finish early still started only {widest} decodes over {frames} frames");
    assert!(heaviest<=bound,"{} KiB of decodes were admitted at once; the working set is {} KiB",heaviest>>10,bound>>10);
}

fn working_layer(doc:&layer_core::Document)->layer_core::OccurrenceHandle {
    doc.scene().order().iter().copied().find(|handle|doc.scene().occurrence(*handle).unwrap().name.as_ref()=="Layer 3").unwrap()
}

#[test]
fn an_open_with_a_working_layer_builds_no_halves_and_commits_what_a_flat_open_commits() {
    let doc=layered_raster_document(8,4);
    let open=|working:Option<layer_core::OccurrenceHandle>| {
        let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
        layer_render::CanvasRenderer::set_working_layer(&mut r,working);
        let before=crate::test_support::PassCounts::snapshot();
        settle_observing(&mut r,reduced_32_layer_view(&doc),|_|{});
        (crate::test_support::PassCounts::snapshot().delta(before).composite_commits,r.scale_display.as_ref().unwrap().cached_branches())
    };
    let (flat,_)=open(None);
    let (working,halves)=open(Some(working_layer(&doc)));
    assert_eq!(halves,0,"an open builds no halves around the working layer");
    assert_eq!(working,flat,"an open with a working layer commits what a flat open commits");
}

#[test]
fn selecting_another_layer_builds_its_halves_only_in_idle_frames() {
    let doc=layered_raster_document(8,4);
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    let frame=FramePacket {composite_all:false,..reduced_32_layer_view(&doc)};
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(working_layer(&doc)));
    settle_observing(&mut r,frame,|_|{});
    let other=doc.scene().order().iter().copied().find(|handle|doc.scene().occurrence(*handle).unwrap().name.as_ref()=="Layer 6").unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(other));
    let mut panned=frame;
    for step in 1..=4 {
        panned.view.document_to_surface[4]=step as f32;
        let (misses,before)=(r.source_tiles.borrow().misses,crate::test_support::PassCounts::snapshot());
        crate::test_support::submit_ready(&mut r,panned).unwrap();
        crate::test_support::complete(&r);
        let work=crate::test_support::PassCounts::snapshot().delta(before);
        assert_eq!((r.source_tiles.borrow().misses-misses,work.unit_acceptances,work.composite_commits),(0,0,0),"a selection does no work in panning frame {step}");
    }
    assert_eq!(r.scale_display.as_ref().unwrap().cached_branches(),0,"no halves exist before an idle frame");
    settle_observing(&mut r,panned,|_|{});
    assert!(r.scale_display.as_ref().unwrap().cached_branches()>0,"idle frames build the selected layer's halves");
}

#[test]
fn stroke_frames_build_halves_only_within_their_dab_reach_and_their_budget() {
    let doc=layered_raster_document(8,8);
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    let layer=working_layer(&doc);
    let target=doc.scene().source_target(layer).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(layer));
    let mut frame=packet(doc.scene(),doc.composition().size);frame.composite_all=false;
    frame.view.width_px=512;frame.view.height_px=512;frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    settle_observing(&mut r,frame,|_|{});
    let cell=PixelRect::new(0,0,1024,1024);
    let units=1+(1<<20)/(3*u64::from(PAGE_SIZE).pow(2)) as usize;
    let mut built=0;
    for step in 0..8 {
        let dab=crate::tests::test_dab([200.+8.*step as f32,200.],[0.9,0.1,0.3,1.],0.8);
        let batch=crate::test_support::dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        crate::test_support::complete(&r);
        let halves=r.scale_display.as_ref().unwrap().half_pages();
        assert!(halves.iter().all(|(_,region)|region.intersect(cell)==*region),"stroke frame {step} built halves outside its dab reach: {halves:?}");
        let level_zero=halves.iter().filter(|(level,_)|*level==0).count();
        assert!(level_zero-built<=units,"stroke frame {step} built {} level-0 half units; its budget admits {units}",level_zero-built);
        built=level_zero;
    }
    assert!(built>0,"the stroke builds halves for the cell it reaches");
}

#[test]
fn opacity_edits_on_the_working_layer_read_only_that_layer_after_the_first() {
    let doc=layered_raster_document(8,4);
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    let layer=working_layer(&doc);
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(layer));
    settle_observing(&mut r,reduced_32_layer_view(&doc),|_|{});
    let mut artwork=doc.artwork.clone();
    for (opacity,bound) in [(0.5,2*8*16),(0.25,16)] {
        r.source_tiles.borrow().release_unused(0);r.source_tiles.borrow_mut().retire_evicted();
        artwork.occurrences.get_mut(layer).unwrap().opacity=opacity;
        let changed=layer_core::Document::from_artwork(artwork.clone()).unwrap();
        let before=r.source_tiles.borrow().misses;
        settle_observing(&mut r,reduced_32_layer_view(&changed),|_|{});
        let read=r.source_tiles.borrow().misses-before;
        assert!(read<=bound,"an opacity edit of {opacity} on the working layer read {read} tiles; the bound is {bound}");
    }
}

fn tablet_renderer(color:layer_core::color::DocumentColor)->WgpuRasterizer {
    renderer_in_budget(color,layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),unified:true,..Default::default()},
        MemoryBudget::with_facts(MemoryFacts {host_default_bytes:Some(1<<30),override_bytes:Some(1<<30),..Default::default()})).unwrap()
}

fn tablet_open(doc:&layer_core::Document,size:[u32;2],scale:f32)->(WgpuRasterizer,u64) {
    let extent=doc.composition().size;
    let mut r=tablet_renderer(doc.composition().color);
    let mut frame=packet(doc.scene(),extent);frame.view.width_px=size[0];frame.view.height_px=size[1];
    frame.view.document_to_surface=[scale,0.,0.,scale,size[0] as f32/2.-scale*extent[0] as f32/2.,size[1] as f32/2.-scale*extent[1] as f32/2.];
    let counts=crate::test_support::PassCounts::snapshot();
    settle_observing(&mut r,frame,|_|{});
    let attempts=crate::test_support::PassCounts::snapshot().delta(counts).unit_acceptances;
    (r,attempts)
}

#[test]
fn a_tablet_budget_cold_open_admits_each_exact_unit_once_and_decodes_each_present_tile_once() {
    let doc=layered_raster_document(28,16);
    let (r,attempts)=tablet_open(&doc,[1366;2],1./3.);
    let (units,present)=(16*16,28*16*16);
    assert!(attempts<=units+8,"{attempts} acceptance attempts for {units} exact units; an admitted unit keeps its inputs and a refused one waits for room");
    let decodes=r.source_tiles.borrow().misses;
    assert!(decodes<=present+present/50,"{decodes} decodes of {present} present tiles");
}

#[test]
fn a_tablet_budget_zoomed_cold_open_decodes_a_tile_at_most_once_for_the_overview_and_once_for_the_view() {
    let doc=layered_raster_document(28,12);
    let (r,_)=tablet_open(&doc,[1920,1200],1.);
    let view=PixelRect::new(1536-960,1536-600,1536+960,1536+600);
    let visible=28*page_coordinates(view).count() as u64;
    let decodes=r.source_tiles.borrow().misses;
    assert!(decodes<=28*12*12+visible,"{decodes} decodes: the overview reads each of {} present tiles and the view its {visible} visible tiles at most once more",28*12*12);
}

fn single_layer_document(tiles:u32)->layer_core::Document {
    use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
    let extent=[PAGE_SIZE*tiles;2];
    let mut artwork=layer_core::authored::Artwork::new(extent).unwrap();
    let descriptor=layer_core::color::DocumentColor::default().paint_descriptor();
    let (_,layer_core::SourceTarget::Paint(paint))=crate::test_support::add_paint(&mut artwork,"Photo",extent) else {unreachable!()};
    let data=RasterData {tiles:(0..tiles).flat_map(|y|(0..tiles).map(move|x|[x,y])).map(|[x,y]| {
        let bytes=[(x*29) as u8,(y*13) as u8,((x+y)*5) as u8,255].repeat(PAGE_SIZE.pow(2) as usize);
        (TileKey {plane:RasterPlane::Color,coordinate:[x,y]},RasterTile::backed(TileBlob::encode(descriptor,&bytes).unwrap()))
    }).collect(),watercolor:None};
    artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(data);
    layer_core::Document::from_artwork(artwork).unwrap()
}

fn fit_view(doc:&layer_core::Document,side:u32)->FramePacket<'_> {
    let extent=doc.composition().size;
    let mut frame=packet(doc.scene(),extent);frame.view.width_px=side;frame.view.height_px=side;
    let scale=side as f32/extent[0].max(extent[1]) as f32;
    frame.view.document_to_surface=[scale,0.,0.,scale,0.,0.];frame
}

#[test]
fn a_single_layer_photo_settles_with_no_retained_level_zero_exact_pages() {
    let doc=single_layer_document(8);
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    settle_observing(&mut r,fit_view(&doc,512),|_|{});
    assert!(crate::scene::scale::composite_pages_at(&r,1)>0,"the photo settles exact at the reduced levels");
    assert_eq!(crate::scene::scale::composite_pages_at(&r,0),0,"a single-layer photo keeps no level-0 exact pages once its cells reduce");
}

#[test]
fn a_unified_device_settles_with_its_retention_within_the_display_grant() {
    let doc=layered_raster_document(3,32);
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {unified:true,budget_bytes:Some(4<<30),..Default::default()}).unwrap();
    settle_observing(&mut r,fit_view(&doc,512),|_|{});
    let (used,grant)=(r.scene_tiers.display.used(),r.memory.display_grant());
    assert!(grant<r.memory.retention_grant(),"the view's target bounds the display grant below the ledger grant");
    assert!(used<=grant.saturating_add(r.scene_tiers.retiring.used()),"display retention {} MiB exceeds the display grant {} MiB",used>>20,grant>>20);
}
