use super::*;
use crate::test_support::{complete, dab_batch, packet, submit_published, submit_settled};
use layer_core::{MemoryBudget, MemoryClass, MemoryFacts};
use std::sync::atomic::Ordering;

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
    r.publish_memory(true);
    budget.flush_reports();
    r.refresh_memory_limits();
}

pub(crate) fn legacy_scene_renderer(color: layer_core::color::DocumentColor) -> WgpuRasterizer {
    renderer(color, layer_core::DeviceMemoryFacts {budget_bytes:Some(64 << 30),..Default::default()}).unwrap()
}

pub(crate) fn legacy_scene_frame(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    scene_frame_with_budget(r, frame, crate::scene::scale::CACHE_BYTES);
}

pub(crate) fn scene_frame_with_budget(r: &mut WgpuRasterizer, frame: FramePacket<'_>, bytes: u64) {
    r.begin_frame_memory(frame);
    let mut demand=r.memory.demand.get();
    demand[0]=bytes;
    r.memory.demand.set(demand);
    r.publish_memory(true);
    r.factory.memory_budget().flush_reports();
    r.refresh_memory_limits();
    assert_eq!(r.memory_limit(Kind::Scene),bytes);
}

pub(crate) fn renderer_for_restore(document: &layer_core::Document) -> WgpuRasterizer {
    let mut r = renderer(document.composition().color,
        layer_core::DeviceMemoryFacts {budget_bytes: Some(4 << 30), ..Default::default()}).unwrap();
    r.prepare_capture_memory(document.scene(), document.composition().size);
    let mut demand = r.memory.demand.get();
    demand[2] = 44 << 20;
    demand[3] = 64 << 20;
    r.memory.demand.set(demand);
    r.publish_memory(true);
    r.factory.memory_budget().flush_reports();
    r.refresh_memory_limits();
    r
}

pub(crate) fn declare_source_budget(r: &mut WgpuRasterizer, frame: FramePacket<'_>, slots: usize, uploads: u64, mips: u64) {
    r.memory.invalidate_limits();
    r.refresh_memory_limits();
    let applied = r.memory.applied_limits.get().expect("fixture memory limits were applied");
    let overhead = applied[4].saturating_sub(r.source_tiles.borrow().admitted_bytes()[0]);
    r.memory_demand(frame);
    r.prepare_moving_projection(frame).unwrap();
    let mut demand = r.memory.demand.get();
    demand[2] = mips;
    demand[3] = (slots as u64).saturating_mul(scene::sources::FLOAT_TILE_BYTES).saturating_add(overhead);
    r.memory.demand.set(demand);
    r.publish_memory(true);
    let mut staging = r.memory.staging.published.get();
    staging.demand_gpu_bytes = staging.demand_gpu_bytes.max(uploads);
    r.memory.staging.publish(staging, true);
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
fn leased_source_pins_do_not_reduce_an_ample_scene_grant() {
    let doc = document();
    let mut r = legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), [512; 2]);
    frame.composite_all = false;
    submit_settled(&mut r, frame);
    declare_source_budget(&mut r, frame, SOURCE_SLOTS + 4, WORKING_BYTES, SOURCE_POOL_BYTES - WORKING_BYTES);
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
    r.publish_memory(true);
    r.factory.memory_budget().flush_reports();
    r.begin_frame_memory(frame);
    let demand = r.memory.display_demand();
    let pins = r.memory.part_pins();
    assert!(pins[3] > demand[3], "foreign source leases exceed modelled source demand");
    assert_eq!(r.memory.scene_limit(), demand[0], "demand={demand:?}, pins={pins:?}");
    let work = r.metrics.composited_pixels;
    frame.view.document_to_surface[4] = -32.;
    submit_settled(&mut r, frame);
    assert_eq!(r.metrics.composited_pixels, work, "one pan samples settled resident pixels");
    assert_eq!(leases.len(), 2);
}

#[test]
fn a_color_cache_grant_keeps_unpinned_pages_beside_pinned_stroke_pages() {
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
    let extent = [PAGE_SIZE * 8, PAGE_SIZE * 4];
    let mut doc = layer_core::Document::new(layer_core::PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let target = doc.active_target().unwrap();
    let layer_core::SourceTarget::Paint(paint) = target else { unreachable!() };
    let mut data = RasterData::default();
    for y in 0..4 {
        for x in 0..8 {
            let bytes = [31 + x as u8 * 17, 53 + y as u8 * 29, 109, 255].repeat(PAGE_SIZE.pow(2) as usize);
            data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate: [x, y] },
                RasterTile::backed(TileBlob::encode(doc.composition().color.paint_descriptor(), &bytes).unwrap()));
        }
    }
    doc.artwork.paint.get_mut(paint).unwrap().raster = RasterRevision::backed(data);
    let mut r = renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    submit_settled(&mut r, packet(doc.scene(), extent));
    let dab = crate::tests::test_dab([64.; 2], [0.7, 0.2, 0.1, 1.], 1.);
    let mut style = crate::tests::test_style(BrushExecution::Dry);
    style.opacity = 0.5;
    let batch = layer_render::DabBatch { stroke_start: true, stroke_end: false,
        ..dab_batch(target, style, dab.bounds()) };
    doc.artwork.paint.get_mut(paint).unwrap().raster = RasterRevision::pending();
    submit_published(&mut r, FramePacket { dabs: &[dab], dab_batches: &[batch], ..packet(doc.scene(), extent) });
    complete(&r);
    let pinned = r.paint_layers[0].pages.iter().find(|page| page.stroke_start.is_some()).unwrap().primary.texture.clone();
    let page_bytes = r.paint_layers[0].pages[0].primary.storage_bytes();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    r.refresh_storage_metrics();
    r.publish_memory(true);
    budget.flush_reports();
    let _pressure = budget.admit_import(budget.import_headroom_bytes().checked_sub(16 << 20).unwrap()).unwrap();
    r.refresh_memory_limits();
    let optional = r.native_edit.as_ref().unwrap().color_cache_bytes;
    let keep = (optional / page_bytes) as usize;
    let evictable = |r: &WgpuRasterizer| r.paint_layers[0].pages.iter().filter(|page| page.stroke_start.is_none()).count();
    assert!(keep > 0 && keep < evictable(&r), "the actual grant retains some backed color: {optional}");
    for _ in 0..3 { drop(r.trim_native_color_cache(&[], &[])); }
    assert_eq!(evictable(&r), keep, "pinned color must not consume the optional allowance twice");
    assert!(r.paint_layers[0].pages.iter().any(|page| page.primary.texture == pinned && page.stroke_start.is_some()));
    r.refresh_storage_metrics();
    r.publish_memory(true);
    budget.flush_reports();
    let _drain = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    r.refresh_memory_limits();
    assert_eq!(r.native_edit.as_ref().unwrap().color_cache_bytes, 0);
    for _ in 0..3 { drop(r.trim_native_color_cache(&[], &[])); }
    assert_eq!(evictable(&r), 0);
    assert!(r.paint_layers[0].pages.iter().any(|page| page.primary.texture == pinned && page.stroke_start.is_some()));
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
    r.raster_buffers.set_retained_limit(u64::MAX);
    let buffers = (0..3).map(|_| r.raster_buffers.take(&r.device, 32 * MIB)).collect::<Vec<_>>();
    let newest = buffers[1..].to_vec();
    for buffer in buffers { r.raster_buffers.put(buffer); }
    assert_eq!(r.raster_buffers.bytes.load(Ordering::Acquire), RASTER_BUFFER_BYTES);
    let retained = (0..2).map(|_| r.raster_buffers.take(&r.device, 32 * MIB)).collect::<Vec<_>>();
    assert_eq!(retained, newest);
    drop(newest);
    for buffer in retained { r.raster_buffers.put(buffer); }
    r.raster_buffers.set_retained_limit(20 * MIB);
    assert!(r.raster_buffers.trim_unused() > 0);
    assert!(r.raster_buffers.bytes.load(Ordering::Acquire) <= 20 * MIB);
}

#[test]
fn after_present_trims_free_storage_but_keeps_presentation_and_root_overview() {
    let doc = document();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let budget = isolate(&mut r);
    submit_settled(&mut r, packet(doc.scene(), [512; 2]));
    let before = pixels(&r);
    let cache = r.scale_display.as_ref().unwrap();
    let overview = cache.coarse_view().clone();
    let pinned = cache.presentation_bytes();
    assert!(pinned > 0);
    r.raster_buffers.set_retained_limit(32 << 20);
    r.raster_buffers.put(r.raster_buffers.take(&r.device, 32 << 20));
    let retained = r.raster_buffers.bytes.load(Ordering::Acquire);
    assert_eq!(retained, 32 << 20);
    budget.set_ram_override(Some(384 << 20));
    budget.flush_reports();
    let _import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    r.shader_input();
    r.after_present();
    assert_eq!(r.raster_buffers.bytes.load(Ordering::Acquire), retained);
    r.shader_idle(true, true);
    r.after_present();
    complete(&r);
    assert!(r.raster_buffers.bytes.load(Ordering::Acquire) < retained);
    let cache = r.scale_display.as_ref().unwrap();
    assert_eq!(cache.presentation_bytes(), pinned);
    assert_eq!(cache.coarse_view(), &overview);
    assert_eq!(pixels(&r), before);
}


#[test]
fn first_frame_preparation_publishes_demand_and_uses_the_next_grant() {
    let doc = layer_core::Document::new(layer_core::PortableId::random(), 8192, 8192,
        layer_core::DocumentNames {paint: "Large canvas".into(), paper: "Paper".into()});
    let budget = MemoryBudget::with_facts(MemoryFacts {host_default_bytes: Some(64 << 30), override_bytes: Some(64 << 30), ..Default::default()});
    let mut r = renderer_in_budget(doc.composition().color, layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}, budget.clone()).unwrap();
    r.publish_memory(true);
    budget.flush_reports();
    let before = r.memory_limit(Kind::Scene);
    let frame = FramePacket {view: crate::test_support::view([512; 2]), ..packet(doc.scene(), [8192; 2])};
    r.raster_dependencies_ready(frame, None);
    assert!(r.artwork_frame.is_none());
    budget.flush_reports();
    assert!(r.memory_limit(Kind::Scene) > before);
    let display = r.memory.display.handle.reported();
    assert_eq!(display.demand_ram_bytes, display.ram_bytes);
    assert!(r.ram_limit(Kind::DecodedSamples) > 0);
    r.raster_dependencies_ready(frame, None);
    assert_eq!(r.display_memory_limits()[0], r.memory_limit(Kind::Scene));
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
        "first artwork frame Scene grant: {} bytes", r.memory_limit(Kind::Scene));
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
    r.publish_memory(true);
    budget.flush_reports();
    let presentation = r.memory.pinned.get()[0];
    let window = r.memory.working_limit();
    assert!(presentation > window);
    let _import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert_eq!(budget.import_headroom_bytes(), 0);
    r.begin_frame_memory(frame);
    assert!(r.memory_limit(Kind::Scene) >= presentation + window);
    submit_settled(&mut r, frame);
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level, 0);
    assert_eq!(pixels(&r), expected);
}

#[test]
fn an_import_draining_headroom_preserves_image_and_effect_frame_working_memory() {
    use layer_core::authored::{Image, ImageObject, Occurrence, OccurrenceContent, PortableId};
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    let plain = layer_core::Document::new(PortableId::random(), 128, 128,
        layer_core::DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
    r.memory.demand_region(plain.scene(), [128; 2], PixelRect::full([128; 2]), DisplayDemand::default());
    r.publish_memory(true);
    budget.flush_reports();
    let floor = r.memory.floors.get()[0];
    let plain_import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert_eq!(budget.import_headroom_bytes(), 0);
    let sources = r.source_tiles.borrow().non_slot_bytes();
    assert!(r.memory.display.handle.reported().pinned_gpu_bytes <= floor + 2 * WORKING_BYTES + sources);
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
    r.publish_memory(true);
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
fn a_restore_is_refused_or_admitted_after_one_budget_rebalance() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    r.publish_memory(true);
    budget.flush_reports();
    r.refresh_memory_limits();
    let import = budget.admit_import(budget.import_headroom_bytes() - (8 << 20)).unwrap();
    r.refresh_memory_limits();
    let restore = 64 << 20;
    assert!(restore < budget.ram_limit_bytes());
    for _ in 0..2 {
        assert!(matches!(r.admit_preparation_bytes(restore, 0), Err(GpuRasterError::SourceWorkingSetExceeded)));
    }
    assert_eq!(r.memory.preparation.get(), 0);
    drop(import);
    assert!(matches!(r.admit_preparation_bytes(restore, 0), Err(GpuRasterError::DeferredObjectWork)));
    r.refresh_memory_limits();
    assert!(r.admit_preparation_bytes(restore, 0).is_ok());
}

#[test]
fn ample_budget_keeps_the_parent_cache_ceilings_for_overlapping_photos() {
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
        let requested = crate::object_image_mips::MovingImages::demand(&r.device, &r.moving_projection.as_ref().unwrap().requests).unwrap();
        let mips = requested.min(SOURCE_POOL_BYTES - WORKING_BYTES);
        let windows = scene::Scene::capture_image_bound(document.scene(), PixelRect::full(extent));
        assert_eq!(r.memory_limit(Kind::Scene), CACHE_BYTES + windows);
        assert_eq!(scene::scale::cache_bytes(&r), CACHE_BYTES);
        assert!(scene::Scene::window_plan(document.scene(), extent, r.native_edit.as_ref().unwrap().image_pixel_budget(0),
            r.device.limits().max_texture_dimension_2d, None).unwrap().is_none());
        assert_eq!(r.memory_limit(Kind::Objects), OBJECT_CACHE_BYTES);
        assert_eq!(r.memory_limit(Kind::Mips), mips);
        assert_eq!(r.memory_limit(Kind::DecodedSamples), (SOURCE_POOL_BYTES - mips).max(r.memory.part_pins()[3]));
        assert_eq!(r.ram_limit(Kind::DecodedSamples), DECODED_SAMPLE_BYTES);
        assert_eq!(r.source_tiles.borrow().mip_budget(), mips);
        assert_eq!(r.memory.demand.get()[2], mips);
    }
    let plain = layer_core::Document::new(PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
    let frame = packet(plain.scene(), extent);
    r.begin_frame_memory(frame);
    r.prepare_moving_projection(frame).unwrap();
    let pyramid = display_mips::Plan::at(extent, 0).pixel_bytes_through(display_mips::MAX_LEVEL);
    assert_eq!(r.memory_limit(Kind::Scene), CACHE_BYTES + pyramid);
    assert_eq!(scene::scale::cache_bytes(&r), CACHE_BYTES);
    assert_eq!(r.memory.scene_limit().saturating_sub(CACHE_BYTES), pyramid);
    assert_eq!(r.memory_limit(Kind::Objects), OBJECT_CACHE_BYTES);
    assert_eq!(r.memory_limit(Kind::Mips), 0);
    assert_eq!(r.memory_limit(Kind::DecodedSamples), SOURCE_POOL_BYTES);
    assert_eq!(r.ram_limit(Kind::DecodedSamples), DECODED_SAMPLE_BYTES);
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
            if engine.backend().raster_ready() && !engine.backend().object_deferred && !engine.has_pending_document_edits() && !engine.has_pending_input() { return; }
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
    assert!(!frame(&mut engine, &mut presenter, &view, clock), "the tap waits for its photo tiles");
    settle(&mut engine, &mut presenter, &view, &mut clock);
    assert_ne!(engine.backend().canvas_preview_revision(), revision_before_tap, "a deferred tap reports its ink once it publishes");
    let shown = crate::layer_tests::page_bytes(engine.backend(), &texture);
    engine.backend_mut().document_damage.push(PixelRect::full(EXTENT));
    settle(&mut engine, &mut presenter, &view, &mut clock);
    clock += 4_166_667;
    frame(&mut engine, &mut presenter, &view, clock);
    assert!(shown == crate::layer_tests::page_bytes(engine.backend(), &texture), "a full redraw changed the settled canvas, so deferred frames lost damage");
}
