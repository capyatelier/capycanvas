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

#[test]
fn transient_display_bytes_stay_in_the_ledger_while_another_thread_frees_others() {
    let budget = MemoryBudget::with_facts(MemoryFacts { override_bytes: Some(64 << 30), ..Default::default() });
    let device = budget.register_device(layer_core::DeviceMemoryFacts { unified: true, ..Default::default() });
    let owner = budget.register_owner(Kind::Other, MemoryClass::Pinned, Some(device));
    let tier = Arc::new(crate::residency::Tier::default());
    let ledger = || { budget.flush_reports(); budget.snapshot().rows[Kind::Other as usize].iter().map(|row| row.gpu_bytes).sum::<u64>() };
    let undercounted = (0..1000).filter(|_| {
        let (retire, retired) = std::sync::mpsc::channel::<crate::present_transition::Charge>();
        let poll = std::thread::spawn(move || retired.into_iter().for_each(drop));
        let kept: Vec<_> = (0..64).filter_map(|page| {
            let charge = crate::present_transition::Charge::new(MIB, &tier, Some(owner.clone()));
            if page % 4 == 0 { Some(charge) } else { retire.send(charge).unwrap(); None }
        }).collect();
        let (counted, charged) = (ledger(), tier.used());
        drop(retire);
        poll.join().unwrap();
        drop(kept);
        counted < charged
    }).count();
    assert_eq!((undercounted, ledger()), (0, 0), "the ledger counted fewer transient display bytes than were still charged in {undercounted} of 1000 retirements, or kept counting freed bytes");
}

pub(crate) fn renderer(color: layer_core::color::DocumentColor, facts: layer_core::DeviceMemoryFacts) -> Result<WgpuRasterizer, GpuRasterError> {
    let budget = MemoryBudget::with_facts(MemoryFacts { host_default_bytes: Some(2 << 30),
        override_bytes: Some(2 << 30), ..Default::default() });
    renderer_in_budget(color, facts, budget)
}
pub(crate) fn renderer_in_budget(color: layer_core::color::DocumentColor, facts: layer_core::DeviceMemoryFacts, budget: MemoryBudget) -> Result<WgpuRasterizer, GpuRasterError> {
    WgpuRasterizer::new_native_headless_in(color, facts, budget).map(|renderer| *renderer)
}

pub(crate) fn pin_every_free_byte(r: &mut WgpuRasterizer) -> Vec<crate::residency::Reservation> {
    let (mut pinned, mut chunk) = (Vec::new(), 1u64 << 32);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while chunk >= 64 << 10 {
        assert!(std::time::Instant::now() < deadline, "the free room must drain");
        match r.admit_raster_working(chunk, crate::frame_demand::Allowance::Fresh) {
            Ok(held) => pinned.push(held),
            Err(_) if r.lane_waits() => { r.refresh_memory_limits(); complete(r); r.device.retirement_handle().flush().unwrap(); },
            Err(_) => chunk /= 2,
        }
    }
    pinned
}

pub(crate) fn lower_memory_to_floor(r: &mut WgpuRasterizer) {
    let budget = r.factory.memory_budget().clone();
    while budget.ram_limit_bytes() > 384 << 20 {
        let limit = budget.ram_limit_bytes();
        budget.update_ram(budget.facts(), MemoryBudget::now_ms());
        budget.out_of_memory();
        assert!(budget.ram_limit_bytes() < limit, "each out-of-memory incident lowers the limit");
    }
    r.publish_memory();
    budget.flush_reports();
    r.refresh_memory_limits();
}

pub(crate) fn evict_to_grant(r: &mut WgpuRasterizer) {
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let excess = r.raster_excess();
    r.evict_raster(excess, &[], &[], &mut encoder);
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
    for (owner, kind) in [(&mut r.memory.display, Kind::Scene), (&mut r.memory.raster, Kind::RasterPool), (&mut r.memory.staging, Kind::Staging)] {
        owner.handle = budget.register_owner(kind, MemoryClass::Visible, Some(device));
        owner.handle.report(owner.published.get());
    }
    r.device.source_samples = Arc::new(layer_core::raster::DecodedTileCache::new(0));
    r.memory.samples = r.device.source_samples.report_to(|| budget.register_owner(Kind::DecodedSamples, MemoryClass::Visible, Some(device)));
    r.device.source_samples.set_demand(SOURCE_WORKING_BYTES, DECODED_SAMPLE_BYTES);
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
    evict_to_grant(&mut r);
    assert_eq!(evictable(&r), keep, "pinned color must not consume the optional allowance twice");
    assert!(r.paint_layers[0].pages.iter().any(|page| page.primary.allocation_identity() == pinned && page.stroke_start.is_some()));
    complete(&r);
    r.refresh_storage_metrics();
    r.publish_memory();
    budget.flush_reports();
    let _drain = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    r.refresh_memory_limits();
    evict_to_grant(&mut r);
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
    let overview = cache.presentation().unwrap().views(&r.empty_view)[1].clone();
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
    assert_eq!(cache.presentation().unwrap().views(&r.empty_view)[1], &overview);
    assert_eq!(pixels(&r), before);
}


fn device_bytes_after_the_lane_and_authored_display_work(waits: bool) -> (u64, u64, String) {
    let doc = layer_core::Document::new(layer_core::PortableId::random(), 1024, 1024,
        layer_core::DocumentNames {paint: "Small canvas".into(), paper: "Paper".into()});
    let budget = MemoryBudget::with_facts(MemoryFacts {host_default_bytes: Some(8 << 30), override_bytes: Some(8 << 30), ..Default::default()});
    let mut r = renderer_in_budget(doc.composition().color, layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}, budget.clone()).unwrap();
    let frame = FramePacket {view: crate::test_support::view([1024; 2]), ..packet(doc.scene(), [1024; 2])};
    crate::test_support::submit_settled(&mut r, frame);
    let unit = r.memory.working_limit();
    if waits {
        assert!(r.admit_raster_working(u64::MAX / 2,crate::frame_demand::Allowance::Fresh).is_err(), "the lane cannot fit the device");
    }
    let stopped = r.memory.stop().is_some();
    let display = r.admit_display(None, crate::residency::Class::Authored, unit,crate::frame_demand::Allowance::Fresh);
    assert_eq!(display.is_ok(), !stopped, "a waiting lane holds every display admission back, so nothing draws on the reservations it borrows");
    r.publish_memory(); budget.flush_reports(); r.sync_raster_report();
    let room = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used());
    let raster = r.admit_raster_working(room,crate::frame_demand::Allowance::Fresh).expect("the lane takes the room its ledger shows");
    r.publish_memory(); budget.flush_reports();
    let device = budget.snapshot().devices[0];
    let detail = format!("unit {unit} room {room} display used {} raster used {} physical {} limit {}", r.scene_tiers.display.used(), r.raster_tiers.raster.used(), device.physical_bytes, device.limit_bytes);
    drop((display, raster));
    (device.physical_bytes, device.limit_bytes, detail)
}

#[test]
fn authored_display_work_beside_a_lane_that_never_waited_stays_within_the_device() {
    let (physical, limit, detail) = device_bytes_after_the_lane_and_authored_display_work(false);
    assert!(physical <= limit, "{detail}");
}

#[test]
fn a_lane_admitted_after_it_waited_leaves_the_displays_working_reservations_backed() {
    let (physical, limit, detail) = device_bytes_after_the_lane_and_authored_display_work(true);
    assert!(physical <= limit, "the lane borrows the display's working reservations, so the ledger must not commit more than the device holds: {detail}");
}

#[test]
fn a_lane_that_no_release_can_wake_refuses_its_edit_once_instead_of_polling() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits(); r.sync_raster_report();
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    let room = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r);
    let _held = r.raster_tiers.admitted.admit(crate::residency::Class::Authored, 64 * MIB).unwrap();
    assert!(r.admit_raster_working(room,crate::frame_demand::Allowance::Fresh).is_err(), "the lane needs the room the held bytes take");
    assert!(!r.lane_waits(), "a caller's own reservation has no release to wait for");
    assert!(!matches!(r.next_frame(), layer_render::FrameDemand::WaitUntil(_)), "the lane asks for no polling frame");
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), Some(GpuRasterError::StrokeMemory), "the edit is refused at once");
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), None, "and only once");
}

#[test]
fn a_refused_lane_keeps_its_refusal_until_it_is_delivered_without_admitting_again() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits(); r.sync_raster_report();
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    let room = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r);
    let _held = r.raster_tiers.admitted.admit(crate::residency::Class::Authored, 64 * MIB).unwrap();
    assert!(r.admit_raster_working(room,crate::frame_demand::Allowance::Fresh).is_err(), "the lane needs the room the held bytes take");
    let before = crate::test_support::PassCounts::snapshot();
    assert!(r.admit_raster_working(room,crate::frame_demand::Allowance::Fresh).is_err(), "an undelivered refusal answers the next request");
    r.end_frame_memory(); r.end_frame_memory();
    assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).raster_admissions, 0, "a refused lane is not admitted again before its refusal is delivered");
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), Some(GpuRasterError::StrokeMemory), "frames that do not deliver the refusal keep it");
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), None, "and it is delivered once");
}

#[test]
fn a_lane_waits_on_the_victims_its_search_retires_and_rechecks_once_when_they_drain() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    let (send, woken) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = send.send(()); }));
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    let notice = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let victim = r.scene_tiers.display.heap().lease(&notice);
    let charge = r.scene_tiers.display.charge(64 * MIB);
    victim.offer(64 * MIB, 0);
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits(); r.sync_raster_report();
    let wanted = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r) + 16 * MIB;
    let refused = r.admit_raster_working(wanted,crate::frame_demand::Allowance::Fresh).err().expect("the lane needs room the display victim holds");
    assert!(victim.evicted(), "the lane's search marks the display victim");
    assert!(refused.waiting().is_some_and(|batch| !batch.settled()), "the refusal names the batch its search started");
    assert!(r.lane_waits(), "the stopped stroke sleeps on its own reclaim");
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), None, "a pending reclaim is no refusal");
    let other = r.admit_display(None, crate::residency::Class::Visible, 1,crate::frame_demand::Allowance::Fresh).err().expect("other work waits behind the stopped lane");
    assert!(other.external() && other.waiting().is_some_and(|batch| !batch.settled()), "other work waits on the lane's end, not on its reclaim");
    assert!(r.admit_raster_working(wanted,crate::frame_demand::Allowance::Fresh).is_err() && victim.evicted(), "the stopped lane does not search again while its reclaim is pending");
    let query = crate::scene::sources::Inputs::default().accept_query(&mut r, None,crate::residency::Class::Visible,crate::frame_demand::Allowance::Fresh).err().expect("a query waits behind the stopped lane");
    assert!(matches!(&query, GpuRasterError::Waiting(waiting) if waiting.need().external() && waiting.need().waiting().is_some_and(|end| !end.settled())),
        "a refused query carries the lane end it waits on: {query:?}");
    while woken.try_recv().is_ok() {}
    let hold = victim.take_batch().expect("the marked victim carries its batch");
    let edge = r.device.retirement_handle();
    edge.run(move || drop((charge, hold)));
    edge.flush().unwrap();
    assert!(woken.try_recv().is_ok(), "the drained victim wakes the lane once its charge has dropped");
    assert!(woken.try_recv().is_err(), "the lane is woken once");
    assert!(!r.lane_waits(), "a settled reclaim leaves the stroke runnable for one recheck");
    let rechecked = r.admit_raster_working(wanted,crate::frame_demand::Allowance::Fresh).map(drop);
    let refused = r.take_lane_failure(layer_core::StrokeId(1));
    assert!(rechecked.is_ok() != refused.is_some(), "the recheck either grants the lane or refuses its edit once: {rechecked:?} {refused:?}");
    assert!(!r.lane_waits() && r.take_lane_failure(layer_core::StrokeId(1)).is_none(), "the recheck ends the wait");
    assert!(other.waiting().is_some_and(crate::residency::Batch::settled), "the lane's end releases the work its stop held back");
}

#[test]
fn a_private_capture_refused_beside_a_live_stroke_leaves_the_strokes_lane_alone() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits(); r.sync_raster_report();
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    let full = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r) + 64 * MIB;
    let _held = r.raster_tiers.admitted.charge(full);
    let texture = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
    let tile = layer_core::raster::RasterTile::pending(layer_core::raster::RasterPlane::Color.descriptor(r.document_color()));
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    let refused = r.encode_private_tiles(vec![(texture, tile, layer_core::color::LayerColorMode::FullColor)], &mut encoder);
    assert!(refused.is_err(), "the private capture finds no room");
    assert!(!r.lane_exhausted() && !r.lane_waits(), "a private capture's miss neither ends nor holds the live stroke's lane");
}

#[test]
fn a_lane_rechecking_after_its_reclaim_drains_spends_the_search_its_wait_saved() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    let notice = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let first = r.scene_tiers.display.heap().lease(&notice);
    let charge = r.scene_tiers.display.charge(8 * MIB);
    first.offer(8 * MIB, 0);
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits(); r.sync_raster_report();
    let wanted = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r) + 16 * MIB;
    assert!(r.admit_raster_working(wanted, crate::frame_demand::Allowance::Fresh).is_err() && first.evicted() && r.lane_waits(), "the lane's one search marks the only victim and waits on it");
    let later = r.scene_tiers.display.heap().lease(&notice);
    let held = r.scene_tiers.display.charge(64 * MIB);
    later.offer(64 * MIB, 0);
    let hold = first.take_batch().expect("the marked victim carries its batch");
    let edge = r.device.retirement_handle();
    edge.run(move || drop((charge, hold)));
    edge.flush().unwrap();
    assert!(!r.lane_waits(), "the drained reclaim leaves the lane one recheck");
    let _ = r.admit_raster_working(wanted, crate::frame_demand::Allowance::Fresh);
    assert!(!later.evicted(), "a recheck after the reclaim it waited on spends no second search, whatever allowance its caller passes");
    drop(held);
}

#[test]
fn a_frame_held_on_its_lane_reclaim_neither_prepares_nor_submits_when_the_host_forces_a_draw() {
    let doc = document();
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    r.resize_surface(512, 512).unwrap();
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    let notice = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let victim = r.scene_tiers.display.heap().lease(&notice);
    let charge = r.scene_tiers.display.charge(64 * MIB);
    victim.offer(64 * MIB, 0);
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits(); r.sync_raster_report();
    let wanted = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r) + 16 * MIB;
    assert!(r.admit_raster_working(wanted,crate::frame_demand::Allowance::Fresh).is_err() && r.lane_waits(), "the stroke's lane sleeps on its reclaim");
    let mut attached = crate::AttachedRenderer(Some(Box::new(r)));
    let frame = packet(doc.scene(), [512; 2]);
    let (before, submissions) = (crate::test_support::PassCounts::snapshot(), attached.0.as_ref().unwrap().metrics.submissions);
    for _ in 0..3 { assert!(attached.submit_when_ready(frame, None).is_pending(), "a forced draw does not take the held frame's turn"); }
    let r = attached.0.as_mut().unwrap();
    assert_eq!((crate::test_support::PassCounts::snapshot().delta(before).display_rooms, r.metrics.submissions), (0, submissions), "the held frame is neither prepared again nor submitted");
    drop(r.admit_capture_window(u64::MAX / 4, u64::MAX / 4, 0, crate::frame_demand::Allowance::Fresh));
    assert!(r.lane_waits(), "a capture window refused behind the held lane leaves the stroke's wait in place");
    let hold = victim.take_batch().expect("the marked victim carries its batch");
    let edge = r.device.retirement_handle();
    edge.run(move || drop((charge, hold)));
    edge.flush().unwrap();
    assert!(!r.lane_waits(), "the drained reclaim ends the hold");
    let before = crate::test_support::PassCounts::snapshot();
    let _ = attached.submission_turn(frame, None);
    assert_ne!(crate::test_support::PassCounts::snapshot().delta(before).display_rooms, 0, "the next draw takes the frame's turn once the reclaim settles");
}

#[test]
fn a_close_passes_its_room_only_for_the_stroke_that_drew_and_within_its_closing_footprint() {
    let doc = document();
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    let presented = crate::present_transition::Charge::new(8 * MIB, &r.device.transient_display, r.device.transient_memory.clone());
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits(); r.sync_raster_report();
    let wanted = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r) + 4 * MIB;
    let batch = dab_batch(doc.active_target().unwrap(), crate::tests::test_style(BrushExecution::Liquify), layer_core::Rect::EMPTY);
    let ink = DabBatch {stroke_id: layer_core::StrokeId(1), stroke_start: true, dab_count: 1, ..batch.clone()};
    r.drawn.record(FramePacket {dab_batches: std::slice::from_ref(&ink), ..packet(doc.scene(), [512; 2])});
    for (stroke, case) in [(layer_core::StrokeId(2), "a stroke that drew nothing"), (layer_core::StrokeId(1), "the drawn stroke asking past its closing footprint")] {
        let end = DabBatch {stroke_id: stroke, stroke_start: false, stroke_end: true, dab_count: 0, ..batch.clone()};
        r.memory.claim(Some(stroke));
        r.begin_frame_memory(FramePacket {dab_batches: std::slice::from_ref(&end), ..packet(doc.scene(), [512; 2])});
        assert!(r.admit_raster_working(wanted, crate::frame_demand::Allowance::Fresh).is_err(), "{case}: a close is not admitted past its room");
        r.end_frame_memory();
        let _ = r.take_lane_failure(stroke);
        r.memory.claim(None);
    }
    drop(presented);
}

#[test]
fn a_shown_fill_or_transform_commit_whose_capture_window_misses_with_nothing_evictable_completes_within_its_footprint() {
    use layer_core::{Affine, RasterOperation, RasterOperationKind};
    use layer_engine::{CanvasEngine, ViewTransform, input_queue};
    const EXTENT: [u32; 2] = [1024, 1024];
    fn settle(engine: &mut CanvasEngine<WgpuRasterizer>, target: SourceTarget) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while engine.has_pending_input() || engine.has_pending_document_edits() || engine.document().scene().raster(target).unwrap().try_data().is_none() {
            assert!(std::time::Instant::now() < deadline, "the commit lands: {:?}", engine.backend().outstanding_terms());
            engine.render_frame().unwrap();
            engine.backend_mut().wait_idle().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    fn show(engine: &mut CanvasEngine<WgpuRasterizer>, shown: impl Fn(&WgpuRasterizer) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !shown(engine.backend()) {
            assert!(std::time::Instant::now() < deadline, "the preview is presented");
            engine.render_frame().unwrap();
            engine.backend_mut().wait_idle().unwrap();
        }
    }
    for (apply, scale) in [(false, 0.5), (true, 0.5), (true, 1.)] {
        let document = layer_core::Document::new(layer_core::PortableId::random(), EXTENT[0], EXTENT[1],
            layer_core::DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
        let target = document.active_target().unwrap();
        let mut gpu = renderer(document.composition().color, layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
        gpu.finish_startup_catalog();
        gpu.prepare_startup(&document, &layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), true).unwrap();
        crate::test_support::wait_startup(&mut gpu, std::time::Instant::now() + std::time::Duration::from_secs(60), |progress| progress.complete, format_args!("fill and transform startup"));
        let view = layer_render::ViewState {width_px: 512, height_px: 512, document_to_surface: [scale, 0., 0., scale, 0., 0.]};
        let mut engine = CanvasEngine::new(gpu, document, input_queue(8).1, view, ViewTransform {revision: 0, surface_to_document: [1. / scale, 0., 0., 1. / scale, 0., 0.]}).unwrap();
        let fill = |engine: &mut CanvasEngine<WgpuRasterizer>, color: [f32; 4]| RasterOperation {placement: Affine::IDENTITY,
            coverage: layer_core::CoverageSnapshot::reveal_all(engine.allocate_coverage_handle(), EXTENT, [0; 2]),
            kind: RasterOperationKind::Fill {color, texture: Default::default(), alpha_locked: false, edges: None, underfill: None}};
        let first = fill(&mut engine, [0., 0., 1., 1.]);
        engine.append_raster_operation(target, first).unwrap();
        settle(&mut engine, target);
        let shown = fill(&mut engine, [1., 0., 0., 1.]);
        if apply {
            engine.set_transform_preview(Some(layer_render::TransformPreview {transaction: 1, target, moving: false, selection: None,
                transform: layer_core::ImageTransform::affine(Affine([0.5, 0., 0., 1., 256., 0.]))})).unwrap();
            show(&mut engine, |r| r.transforms.as_ref().is_some_and(|transforms| transforms.has_preview()));
        } else {
            engine.set_fill_preview(Some(layer_render::FillPreview {transaction: 1, revision: 0, target, operation: shown.clone()}));
            show(&mut engine, |r| r.fill_preview_ready().is_some());
        }
        let case = format!("{} at {}%", if apply { "transform Apply" } else { "fill" }, scale * 100.);
        let (checkpoint, refusals, before) = (engine.checkpoint(), engine.metrics().stroke_memory_refusals, engine.document().artwork.clone());
        let pinned = pin_every_free_byte(engine.backend_mut());
        if apply { assert!(engine.commit_transform(None).unwrap()); } else { engine.set_fill_preview(None); engine.append_raster_operation(target, shown).unwrap(); }
        settle(&mut engine, target);
        assert_eq!(engine.metrics().stroke_memory_refusals, refusals, "{case}: a commit whose capture window misses is never refused, so the painter is not told");
        assert!(engine.document().artwork != before && engine.document().scene().raster(target).unwrap().wait_data().is_ok(), "{case}: the commit publishes its pixels");
        assert!(engine.undo().unwrap() && engine.checkpoint() == checkpoint, "{case}: the commit is one history entry");
        drop(pinned);
    }
}

#[test]
fn a_close_whose_lane_waits_on_its_own_reclaim_takes_its_turn_while_the_reclaim_drains() {
    let doc = document();
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    r.resize_surface(512, 512).unwrap();
    let stroke = layer_core::StrokeId(1);
    let batch = dab_batch(doc.active_target().unwrap(), crate::tests::test_style(BrushExecution::Dry), layer_core::Rect::EMPTY);
    let ink = DabBatch {stroke_id: stroke, stroke_start: true, dab_count: 1, ..batch.clone()};
    r.drawn.record(FramePacket {dab_batches: std::slice::from_ref(&ink), ..packet(doc.scene(), [512; 2])});
    let notice = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let victim = r.scene_tiers.display.heap().lease(&notice);
    let charge = r.scene_tiers.display.charge(64 * MIB);
    victim.offer(64 * MIB, 0);
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits(); r.sync_raster_report();
    let wanted = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r) + 16 * MIB;
    r.memory.claim(Some(stroke));
    assert!(r.admit_raster_working(wanted, crate::frame_demand::Allowance::Fresh).is_err() && r.lane_waits(), "the stroke's lane sleeps on its own reclaim");
    r.memory.claim(None);
    let end = DabBatch {stroke_id: stroke, stroke_start: false, stroke_end: true, dab_count: 0, ..batch};
    let close = FramePacket {dab_batches: std::slice::from_ref(&end), ..packet(doc.scene(), [512; 2])};
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let ready = loop {
        match r.submission_turn(close, None) {
            layer_render::SubmissionTurn::Ready(ready) => break ready,
            layer_render::SubmissionTurn::Refused(refusal, error) => panic!("the close is refused: {refusal:?} {error:?}"),
            layer_render::SubmissionTurn::Held => {},
        }
        assert!(std::time::Instant::now() < deadline, "a close whose lane waits on its own reclaim takes its turn instead of waiting for the reclaim");
        complete(&r);
    };
    assert!(victim.evicted() && victim.take_batch().is_some_and(|hold| { let edge = r.device.retirement_handle(); edge.run(move || drop((charge, hold))); edge.flush().is_ok() }),
        "the reclaim the close stepped past is still draining and finishes in the background");
    r.submit_ready(ready, close).unwrap();
    assert!(r.take_refused_stroke().is_none() && !r.lane_waits() && r.take_lane_failure(stroke).is_none(), "the close is never refused and leaves the lane free");
}

#[test]
fn a_permit_from_an_earlier_turn_cannot_submit_the_packet_that_replaced_it() {
    let doc = document();
    let mut attached = crate::AttachedRenderer(Some(Box::new(renderer(Default::default(), Default::default()).unwrap())));
    attached.resize_surface(512, 512).unwrap();
    let (first, replacement) = (packet(doc.scene(), [512; 2]), FramePacket {composite_all: true, ..packet(doc.scene(), [512; 2])});
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let stale = loop {
        if let layer_render::SubmissionTurn::Ready(ready) = attached.submission_turn(first, None) { break ready; }
        assert!(std::time::Instant::now() < deadline, "the first packet becomes ready");
        complete(attached.0.as_ref().unwrap());
    };
    let _ = attached.submission_turn(replacement, None);
    let submissions = attached.0.as_ref().unwrap().metrics.submissions;
    assert!(attached.submit_ready(stale, replacement).is_err(), "a permit outlived by a later turn is refused");
    assert_eq!(attached.0.as_ref().unwrap().metrics.submissions, submissions, "the replacement is not submitted on the stale permit");
}

#[test]
fn a_permit_for_one_packet_cannot_submit_another_in_the_same_turn() {
    let doc = document();
    let mut attached = crate::AttachedRenderer(Some(Box::new(renderer(Default::default(), Default::default()).unwrap())));
    attached.resize_surface(512, 512).unwrap();
    let (prepared, other) = (packet(doc.scene(), [512; 2]), FramePacket {reset_layers: true, ..packet(doc.scene(), [512; 2])});
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let ready = loop {
        if let layer_render::SubmissionTurn::Ready(ready) = attached.submission_turn(prepared, None) { break ready; }
        assert!(std::time::Instant::now() < deadline, "the prepared packet becomes ready");
        complete(attached.0.as_ref().unwrap());
    };
    let submissions = attached.0.as_ref().unwrap().metrics.submissions;
    assert!(attached.submit_ready(ready, other).is_err(), "a permit names the packet its turn prepared");
    assert_eq!(attached.0.as_ref().unwrap().metrics.submissions, submissions, "another packet is not submitted on that permit");
}

#[test]
fn a_detached_renderer_never_offers_a_turn_it_cannot_submit() {
    let doc = document();
    let mut detached = crate::AttachedRenderer(None);
    assert!(!matches!(detached.submission_turn(packet(doc.scene(), [512; 2]), None), layer_render::SubmissionTurn::Ready(_)), "a renderer without a device holds or refuses the frame instead of granting a turn");
}

#[test]
fn a_lost_device_refuses_the_stroke_at_its_next_turn_once_with_the_loss() {
    let doc = document();
    let mut attached = crate::AttachedRenderer(Some(Box::new(renderer(Default::default(), Default::default()).unwrap())));
    attached.resize_surface(512, 512).unwrap();
    let frame = packet(doc.scene(), [512; 2]);
    attached.0.as_mut().unwrap().live_stroke = Some(layer_core::StrokeId(1));
    attached.0.as_ref().unwrap().renderer_factory().on_device_lost()("Canvas GPU stopped (Unknown): test".into());
    attached.poll_pending(frame.view).unwrap();
    let lost = GpuRasterError::WaitFailed("Canvas GPU stopped (Unknown): test".into());
    assert!(matches!(attached.submission_turn(frame, None), layer_render::SubmissionTurn::Refused(_, error) if error == lost), "the stroke waiting at its turn is refused with the loss");
    attached.poll_pending(frame.view).unwrap();
    assert!(!matches!(attached.submission_turn(frame, None), layer_render::SubmissionTurn::Refused(..)), "the loss is delivered once");
}

#[test]
fn a_capture_window_on_a_spent_allowance_misses_without_a_second_search_and_refuses_only_its_operation() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits();
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    let before = crate::test_support::PassCounts::snapshot();
    drop(r.admit_capture_window(u64::MAX / 4, u64::MAX / 4, 0, crate::frame_demand::Allowance::Spent));
    assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).raster_admissions, 1, "a window whose owner spent its reclaim tries once without searching");
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(2)), None, "another operation is not refused for this capture window");
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), Some(GpuRasterError::StrokeMemory), "a window that is not a funded close is refused as its operation's own outcome");
}

#[test]
fn a_capture_window_refused_behind_an_ended_lane_leaves_the_edit_refusal_to_be_delivered() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits();
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    assert!(r.admit_raster_working(u64::MAX / 2, crate::frame_demand::Allowance::Fresh).is_err());
    assert!(r.lane_exhausted(), "nothing can free the room, so the edit's lane ends");
    drop(r.admit_capture_window(u64::MAX / 4, u64::MAX / 4, 0, crate::frame_demand::Allowance::Fresh));
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), Some(GpuRasterError::StrokeMemory), "a private capture window cannot clear the refusal the edit has not been told about");
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), None, "the refusal is delivered once");
}

#[test]
fn a_stroke_lane_that_nothing_can_free_refuses_its_edit_once_and_ends_the_wait() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    let (send, woken) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = send.send(()); }));
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits();
    let wait = |r: &mut WgpuRasterizer, bytes: u64| r.admit_raster_working(bytes,crate::frame_demand::Allowance::Fresh).map(drop);
    assert!(wait(&mut r, u64::MAX / 2).is_err());
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), None, "only an edit's lane is refused");
    r.end_frame_memory(); r.end_frame_memory();
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    let presented = crate::present_transition::Charge::new(8 * MIB, &r.device.transient_display, r.device.transient_memory.clone());
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.sync_raster_report();
    let wanted = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r) + 4 * MIB;
    assert!(wait(&mut r, wanted).is_err());
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), Some(GpuRasterError::StrokeMemory), "a presented canvas stays charged while it is shown and frees nothing the lane could wait for, so the edit is refused");
    drop(presented);
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.sync_raster_report();
    while woken.try_recv().is_ok() {}
    drop(r.admit_capture_window(MIB, u64::MAX / 2, 0, crate::frame_demand::Allowance::Fresh));
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), None, "a window that fits after its larger first try refuses nothing");
    for _ in 1..3 {
        assert!(wait(&mut r, u64::MAX / 2).is_err());
        assert!(r.lane_exhausted() && !r.lane_waits(), "a refused lane is delivered, not waited on");
        while woken.try_recv().is_ok() {}
        assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), Some(GpuRasterError::StrokeMemory), "with nothing left to free, the edit is refused");
        assert!(woken.try_recv().is_ok(), "ending the wait wakes the work its stop held back");
        assert!(r.take_lane_failure(layer_core::StrokeId(1)).is_none() && !r.lane_waits(), "the refusal ends the wait");
    }
}

#[test]
fn a_stroke_lane_waits_on_a_held_canvas_already_retiring_and_is_granted_once_it_lands() {
    let mut r = renderer(Default::default(), layer_core::DeviceMemoryFacts {budget_bytes: Some(1 << 30), ..Default::default()}).unwrap();
    let (send, woken) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = send.send(()); }));
    r.live_stroke = Some(layer_core::StrokeId(1)); r.memory.claim(r.live_stroke);
    let (format, size) = (wgpu::TextureFormat::Rgba8Unorm, [2048, 2048]);
    let encoder = r.device.create_command_encoder(&Default::default());
    let held = crate::HeldCanvas::new(&r.device, size, format, crate::SdrSurfaceColor::Srgb, Some(crate::HeldCanvas::texture(&r.device, size, format)), None);
    held.retain_until_complete(&r.device, &encoder);
    r.publish_memory(); r.factory.memory_budget().flush_reports(); r.refresh_memory_limits(); r.sync_raster_report();
    let wanted = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used()) + lendable(&r) + 4 * MIB;
    assert!(r.admit_raster_working(wanted,crate::frame_demand::Allowance::Fresh).is_err());
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), Some(GpuRasterError::StrokeMemory), "a canvas that is still shown frees nothing, so the edit is refused at once");
    drop(held);
    let refused = r.admit_raster_working(wanted,crate::frame_demand::Allowance::Fresh).err().expect("the retiring canvas still holds the room the lane needs");
    assert!(refused.waiting().is_some_and(|batch| !batch.settled()), "the refusal waits on the canvas retirement already in flight");
    assert!(r.lane_waits());
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), None, "bytes about to free refuse no stroke");
    while woken.try_recv().is_ok() {}
    r.queue.submit([encoder.finish()]);
    r.device.wait_queue().unwrap();
    r.device.retirement_handle().flush().unwrap();
    assert!(woken.try_recv().is_ok(), "the landed retirement wakes the lane");
    assert!(!r.lane_waits());
    assert!(r.admit_raster_working(wanted,crate::frame_demand::Allowance::Fresh).is_ok(), "the recheck grants the lane the bytes the canvas freed");
    assert_eq!(r.take_lane_failure(layer_core::StrokeId(1)), None);
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
fn a_shared_sample_cache_stays_exact_in_the_ledger_while_one_renderer_decodes_and_another_trims() {
    use layer_core::raster::TileBlob;
    const TILE: usize = 8 << 16;
    let doc = document();
    let r = renderer(doc.composition().color, layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let candidate = r.color_candidate_staged(doc.composition().color).unwrap();
    let budget = r.factory.memory_budget().clone();
    let descriptor = layer_core::color::DocumentColor {depth: layer_core::color::SampleDepth::U16, ..Default::default()}.paint_descriptor();
    let tiles: Vec<_> = (0..64u8).map(|value| Arc::new(TileBlob::encode(descriptor, &vec![value; TILE]).unwrap())).collect();
    let decoded = std::cell::RefCell::new(Vec::new());
    let decode = |renderer: &WgpuRasterizer, tiles: &[Arc<TileBlob>]| tiles.iter().map(|tile| {
        let samples = renderer.device.source_samples.decode_on_worker(tile).unwrap();
        decoded.borrow_mut().push(Arc::downgrade(&samples));
        samples
    }).collect::<Vec<_>>();
    let check = |step: &str| {
        budget.flush_reports();
        let counted = budget.snapshot().rows[Kind::DecodedSamples as usize].iter().map(|row| row.ram_bytes).sum::<u64>();
        let live = decoded.borrow().iter().filter_map(std::sync::Weak::upgrade).map(|samples| samples.capacity() as u64).sum::<u64>();
        assert_eq!(counted, live, "after {step} the ledger counts {counted} decoded sample bytes while {live} are alive");
    };
    r.device.source_samples.set_limit(48 * TILE);
    let held = decode(&r, &tiles[..40]);
    check("one renderer decodes");
    drop(decode(&candidate, &tiles[40..56]));
    check("the other renderer decodes past the limit");
    drop(held);
    candidate.device.source_samples.set_limit(40 * TILE);
    candidate.device.source_samples.trim_unused(usize::MAX);
    check("the other renderer trims");
    drop(decode(&r, &tiles[56..]));
    check("one renderer evicts unread samples");
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

fn wet_stroke_on_every_page() -> WgpuRasterizer {
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
    r.restore_preflight = None;
    r
}

#[test]
fn pressure_retires_every_blend_scratch_page_in_one_pass_over_the_pages() {
    let mut r = wet_stroke_on_every_page();
    let scratch = |r: &WgpuRasterizer| r.paint_layers[0].pages.iter().filter(|page| page.secondary.is_some()).count() as u64;
    let pages = r.paint_layers[0].pages.len() as u64;
    assert!(scratch(&r) > 2 * SOURCE_SLOTS as u64, "the fixture retires several batches: {} of {pages} pages hold scratch", scratch(&r));
    r.retire_stroke_sources();
    let k = scratch(&r);
    assert_eq!(k, WARM_SPARE_PAGES as u64, "a retired stroke keeps only the warm spares");
    let (retired,_leases)=r.evict_color(u64::MAX,&[],&[]);
    assert_eq!(scratch(&r),0,"one trim must release every optional scratch page");
    assert_eq!(retired.len() as u64,k);
}

#[test]
fn a_modified_pages_spare_is_evictable_and_not_pinned() {
    let mut r = wet_stroke_on_every_page();
    r.retire_stroke_sources();
    let pinned = r.pinned_paint_bytes();
    let unpinned = r.paint_memory_bytes() - pinned;
    let (retired, _leases) = r.evict_color(u64::MAX, &[], &[]);
    assert_eq!(retired.len(), WARM_SPARE_PAGES, "only the warm spares of the modified pages are evictable");
    assert_eq!(retired.iter().map(PageSurface::storage_bytes).sum::<u64>(), unpinned, "pinned paint counts spares that eviction offers");
    drop(retired);
    assert_eq!(r.pinned_paint_bytes(), pinned, "evicting the spares changed pinned paint");
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
    assert!(first.is_ok() || matches!(first, Err(GpuRasterError::NotReady|GpuRasterError::Waiting(_))), "the released restore must publish its need: {first:?}");
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
    let pressured=frame(&dabs,&batches);
    r.begin_frame_memory(pressured);
    let budget=r.factory.memory_budget().clone();budget.set_ram_override(Some(384*MIB));
    r.refresh_storage_metrics();r.publish_memory();budget.flush_reports();r.refresh_memory_limits();
    let pressure=budget.admit_import(budget.import_headroom_bytes()).unwrap();r.refresh_memory_limits();
    assert!(!r.raster_dependencies_ready(pressured,None),"the preview that cannot fit must hold for its terminal result");
    assert_eq!(r.preview_pages.len(),held,"a refused preview allocates no pages");
    let error=crate::test_support::submit_ready(&mut r,pressured).unwrap_err();
    assert_eq!(error,GpuRasterError::StrokeMemory,"a prepared terminal result must not defer during submission");
    assert_eq!(r.take_refused_stroke(),Some(layer_render::Refusal {stroke:pressured.operation(),drawn:0}));
    assert!(r.take_refused_stroke().is_none(),"the refused operation delivers one notice");
    assert_eq!(r.preview_pages.len(),held,"the terminal refusal allocates no pages");
    drop(pressure);budget.set_ram_override(Some(2<<30));budget.flush_reports();r.refresh_memory_limits();
    let batches=[DabBatch {stroke_id:layer_core::StrokeId(2),..preview(&dabs)}];
    crate::test_support::submit_ready(&mut r,frame(&dabs,&batches)).unwrap();
    assert!(r.preview_pages.len()>=64,"the released raster holds the preview");
}

pub(crate) fn durable_color_renderer() -> (layer_core::Document, WgpuRasterizer, Arc<layer_core::raster::RasterData>) {
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
fn a_memory_pass_walks_the_paint_pages_once() {
    let (_doc,mut r,_data)=durable_color_renderer();
    let walks=|r:&mut WgpuRasterizer,pass:&dyn Fn(&mut WgpuRasterizer)| {
        let before=crate::test_support::PassCounts::snapshot();pass(r);crate::test_support::PassCounts::snapshot().delta(before).pinned_walks
    };
    let held=std::cell::RefCell::new(None);
    let admission=walks(&mut r,&|r|*held.borrow_mut()=Some(r.admit_raster_working(MIB,crate::frame_demand::Allowance::Fresh).unwrap()));
    let commit=walks(&mut r,&|r|r.commit_raster_working(held.borrow_mut().take().unwrap()));
    let refresh=walks(&mut r,&|r|{r.memory.invalidate_limits();r.refresh_memory_limits();});
    assert_eq!([admission,commit,refresh],[1;3],"an admission, a commit and a grant pass each walk the paint pages once");
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
    assert!(r.admit_raster_working(headroom,crate::frame_demand::Allowance::Fresh).is_ok(),"the retained buffer must make room for authored working memory");
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
    assert!(r.admit_display(None,crate::residency::Class::Visible,bytes,crate::frame_demand::Allowance::Fresh).is_ok(),
        "a retention share of {share} must not refuse required work with free device headroom");
}

#[test]
fn an_over_grant_display_admission_shrinks_the_raster_limit_before_the_raster_admits() {
    let mut r=legacy_scene_renderer(Default::default());
    r.memory.display.handle.report(MemoryReport {demand_gpu_bytes:1,..Default::default()});
    let headroom=r.memory.raster.handle.gpu_working_limit();
    let held=r.admit_display(None,crate::residency::Class::Visible,64*MIB,crate::frame_demand::Allowance::Fresh).expect("required work may exceed the grant");
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
    let raster=r.admit_raster_working(512*MIB,crate::frame_demand::Allowance::Fresh).expect("the raster holds most of the device");
    r.memory.display.handle.report(MemoryReport {demand_gpu_bytes:1,..Default::default()});
    r.scene_tiers.display.set_limit(u64::MAX,0);
    let (send,woken)=std::sync::mpsc::channel();
    r.preparation_wake=Some(Arc::new(move||{let _=send.send(());}));
    r.watch_ledger();
    let physical=|r:&WgpuRasterizer|{
        let grants=r.factory.memory_budget().snapshot().devices.iter().map(|device|device.granted_bytes).sum::<u64>();
        let resident=r.telemetry().resident_bytes;
        assert!(resident<=grants+64*MIB,"physical {resident} exceeds grants {grants} + 64 MiB");
    };
    let refused=r.admit_display(None,crate::residency::Class::Visible,384*MIB,crate::frame_demand::Allowance::Fresh).err().expect("the ledger refuses what the device cannot hold");
    physical(&r);
    let wait=crate::frame_demand::UnitWait::parked(refused,crate::frame_demand::Allowance::Fresh);
    assert!(wait.eligible(&r).is_none(),"the refused unit waits and registers for the release it needs");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"a refused unit requests no frame before a release");
    r.commit_raster_working(raster);
    woken.recv_timeout(std::time::Duration::from_secs(1)).expect("the raster release wakes the refused unit");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"one release wakes the unit once");
    let allowance=wait.eligible(&r).expect("the woken unit is eligible again");
    let held=r.admit_display(None,crate::residency::Class::Visible,384*MIB,allowance).expect("the woken unit fits");
    physical(&r);
    drop(held);
}

#[test]
fn a_held_raster_head_whose_room_drains_on_the_poll_thread_is_woken() {
    let mut r=renderer(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()}).unwrap();
    let (send,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=send.send(());}));
    r.live_stroke=Some(layer_core::StrokeId(1));r.memory.claim(r.live_stroke);
    let transient=crate::present_transition::Charge::new(768*MIB,&r.device.transient_display,r.device.transient_memory.clone());
    let retirement=r.device.transient_display.heap().retire();
    r.publish_memory();r.factory.memory_budget().flush_reports();r.refresh_memory_limits();
    let refused=r.admit_raster_working(512*MIB,crate::frame_demand::Allowance::Fresh).err().expect("the retiring display bytes hold the room the raster head needs");
    assert!(refused.waiting().is_some_and(|batch|!batch.settled()));
    assert!(r.lane_waits() && r.take_lane_failure(layer_core::StrokeId(1)).is_none(),"the claimed head waits for its retiring bytes");
    assert_eq!(r.next_frame(),layer_render::FrameDemand::Wait,"the renderer waits for the retiring bytes to drain");
    let edge=r.device.retirement_handle();
    edge.flush().unwrap();
    while woken.try_recv().is_ok() {}
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"no wake before the retirement drains");
    edge.run(move||{drop(transient);drop(retirement);});
    edge.flush().unwrap();
    let woke=woken.try_recv().is_ok();
    assert!(r.memory.raster.handle.gpu_working_limit()>=r.raster_tiers.raster.used()+512*MIB,"the drained bytes leave room for the head");
    assert!(woke,"the bytes that drain on the poll thread wake the held raster head");
    assert!(woken.try_recv().is_err(),"the sealed retirement wakes its head once");
    assert!(!r.lane_waits());
    assert!(r.admit_raster_working(512*MIB,crate::frame_demand::Allowance::Fresh).is_ok(),"the claimed head takes the released room");
    assert!(r.take_lane_failure(layer_core::StrokeId(1)).is_none());
}

#[test]
fn a_refused_authored_raster_head_stops_lower_display_admissions_and_takes_the_display_overdraft() {
    let mut r=renderer(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()}).unwrap();
    r.memory.display.handle.report(MemoryReport {demand_gpu_bytes:1,..Default::default()});
    r.scene_tiers.display.set_limit(u64::MAX,0);
    let notice=Arc::new(std::sync::atomic::AtomicBool::new(false));
    let retained=r.scene_tiers.display.heap().lease(&notice);
    let mut held=Some(r.admit_display(None,crate::residency::Class::Visible,640*MIB,crate::frame_demand::Allowance::Fresh).expect("required work may exceed the grant"));
    retained.offer(640*MIB,0);
    let frame=|r:&mut WgpuRasterizer|{r.publish_memory();r.factory.memory_budget().flush_reports();r.refresh_memory_limits();};
    frame(&mut r);
    assert!(r.admit_raster_working(512*MIB,crate::frame_demand::Allowance::Fresh).is_err(),"the display overdraft holds the room the raster head needs");
    assert!(r.admit_display(None,crate::residency::Class::Visible,MIB,crate::frame_demand::Allowance::Fresh).is_err(),"a lower class is not admitted while a higher head is blocked on memory");
    let mut admitted=None;
    for _ in 0..4 {
        frame(&mut r);
        if retained.evicted() {held=None;drop(retained.take_batch());}
        if let Ok(reservation)=r.admit_raster_working(512*MIB,crate::frame_demand::Allowance::Fresh) {admitted=Some(reservation);break;}
    }
    assert!(admitted.is_some(),"the refused authored head takes the room of display bytes above the grant, evicted={}",retained.evicted());
    assert!(r.admit_display(None,crate::residency::Class::Visible,MIB,crate::frame_demand::Allowance::Fresh).is_ok(),"the stop ends once the head is admitted");
    drop(held);
}

#[test]
fn a_refused_display_unit_searches_for_room_once_per_batch_and_rechecks_without_searching() {
    use crate::residency::Class;
    use crate::frame_demand::UnitWait;
    let r=renderer(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()}).unwrap();
    r.scene_tiers.display.set_limit(64*MIB,0);
    let scans=|admit:&mut dyn FnMut()|{let before=crate::test_support::PassCounts::snapshot();admit();crate::test_support::PassCounts::snapshot().delta(before).room_scans};
    let attempt=|wait:&mut UnitWait|->Option<crate::residency::Reservation> {
        let allowance=wait.eligible(&r)?;
        r.admit_display(None,Class::Prefetch,32*MIB,allowance).map_err(|need|*wait=UnitWait::parked(need,allowance)).ok()
    };
    for victims in [false,true] {
        let notice=Arc::new(std::sync::atomic::AtomicBool::new(false));
        let victim=r.scene_tiers.display.heap().lease(&notice);
        let held=r.admit_display(None,Class::Prefetch,48*MIB,crate::frame_demand::Allowance::Fresh).expect("the first unit fits");
        if victims {victim.offer(48*MIB,0);}
        let mut wait=UnitWait::default();
        assert_eq!(scans(&mut ||for _ in 0..16 {assert!(attempt(&mut wait).is_none());}),1,"a refused unit searches once and then waits on what that search found: victims={victims}");
        assert_eq!(matches!(wait,UnitWait::Reclaim(..)),victims,"a search that marks victims waits on their batch, and one that finds none parks");
        drop(held);drop(victim.take_batch());
        let mut admitted=None;
        assert_eq!(scans(&mut ||admitted=attempt(&mut wait)),0,"the release revives the unit for one check that does not search again: victims={victims}");
        assert!(admitted.is_some(),"the revived unit is admitted: victims={victims}");
    }
}

#[test]
fn an_over_grant_display_admission_marks_undemanded_victims_before_its_overdraft() {
    let r=legacy_scene_renderer(Default::default());
    r.memory.display.handle.report(MemoryReport {demand_gpu_bytes:1,..Default::default()});
    let notice=Arc::new(std::sync::atomic::AtomicBool::new(false));
    let victim=r.scene_tiers.display.heap().lease(&notice);victim.offer(8*MIB,0);
    let held=r.admit_display(None,crate::residency::Class::Visible,64*MIB,crate::frame_demand::Allowance::Fresh).expect("required work may exceed the grant");
    assert!(victim.evicted(),"an over-grant admission evicts what no current demand protects before it takes the overdraft");
    drop(held);
}

#[test]
fn display_bytes_above_the_grant_keep_their_ledger_reservation_across_the_grant_pass() {
    let mut r=legacy_scene_renderer(Default::default());
    let held=r.admit_display(None,crate::residency::Class::Visible,2<<30,crate::frame_demand::Allowance::Fresh).expect("required work may exceed the grant");
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
    assert!(r.admit_display(None,crate::residency::Class::Prefetch,MIB,crate::frame_demand::Allowance::Fresh).is_err(),"prefetch took device headroom beyond the display grant");
    assert!(r.admit_display(None,crate::residency::Class::Visible,MIB,crate::frame_demand::Allowance::Fresh).is_ok());
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
    let format = crate::display_precision::DisplayPrecision::for_scene(frame.scene, false).format();
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
    assert!(r.admit_display(None, crate::residency::Class::Prefetch, bytes,crate::frame_demand::Allowance::Fresh).is_err(), "prefetch took the presented and held cells' floor");
    assert!(r.admit_display(None, crate::residency::Class::Visible, bytes,crate::frame_demand::Allowance::Fresh).is_ok(), "required work may use the floor");
}

#[test]
fn the_display_floor_follows_the_view_rather_than_the_largest_request() {
    let doc = document();
    let mut r = legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), [512; 2]);
    frame.view = crate::test_support::view([1024, 768]);
    r.begin_frame_memory(frame);
    let _authored = r.admit_display(None, crate::residency::Class::Authored, MIB,crate::frame_demand::Allowance::Fresh).unwrap();
    let bytes = optional_headroom(&r, presented_floor(frame));
    assert!(r.admit_display(None, crate::residency::Class::Prefetch, bytes,crate::frame_demand::Allowance::Fresh).is_err(), "a small authored request shrank the floor below the view's cells");
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
    let refused=r.admit_display(None,crate::residency::Class::Visible,1,crate::frame_demand::Allowance::Fresh).err().expect("the full tier refuses the unit");
    let wait=crate::frame_demand::UnitWait::parked(refused,crate::frame_demand::Allowance::Fresh);
    assert!(wait.eligible(&r).is_none(),"the refused unit's wait registers for the release");
    drop(held);
    receive.recv_timeout(std::time::Duration::from_secs(1)).expect("the refused display unit's wait subscribes to release");
    assert!(wait.eligible(&r).is_some(),"the released charge revives the unit");
}

#[test]
fn an_import_s_reclaim_is_answered_only_once_the_pass_that_served_it_has_released_its_retirements() {
    use std::{future::Future, task::{Context, Waker}};
    let doc = crate::scene::scale::tests::document_at([2048, 1536]);
    let mut r = renderer(doc.composition().color, layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view = crate::test_support::view([512, 384]);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_settled(&mut r, frame);
    r.set_preparation_wake(Arc::new(|| {}));
    let budget = r.factory.memory_budget().clone();
    let retained = r.scene_tiers.display.charge(64 * MIB);
    r.publish_memory();
    budget.flush_reports();
    let ledger = budget.snapshot();
    let import = budget.admit_import(ledger.ram_limit_bytes - ledger.ram_physical_bytes + MIB).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut waiting = Box::pin(import.covering(&cancel));
    let mut context = Context::from_waker(Waker::noop());
    assert!(waiting.as_mut().poll(&mut context).is_pending(), "the import waits on the display's reclaim");
    assert!(r.memory.reclaim_unanswered(), "the import asks the display to reclaim");
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.apply_frame_grants(&mut encoder);
    assert!(r.memory.reclaim_unanswered(), "the reclaim is not answered while the encoder carrying its retirements is unsubmitted");
    encoder.submit(&r.queue);
    complete(&r);
    r.device.retirement_handle().flush().unwrap();
    r.refresh_memory_limits();
    assert!(!r.memory.reclaim_unanswered(), "the reclaim is answered once its retirements are released");
    drop(retained);
}

#[test]
fn a_reclaim_settled_before_a_grant_pass_is_answered_with_the_display_store_the_pass_holds() {
    let doc = crate::scene::scale::tests::document_at([2048, 1536]);
    let mut r = renderer(doc.composition().color, layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view = crate::test_support::view([512, 384]);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_settled(&mut r, frame);
    assert!(r.scene.as_ref().unwrap().store_metadata_bytes() > 0, "the shown display keeps an indexed store");
    let budget = r.factory.memory_budget().clone();
    let shown = |r: &WgpuRasterizer| { budget.flush_reports(); r.memory.display.handle.reported().ram_bytes };
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.apply_frame_grants(&mut encoder);
    r.memory.reclaiming.replace(Some((r.memory.display.handle.reclaim_epoch(), crate::residency::Search::open([]).seal())));
    r.apply_frame_grants(&mut encoder);
    assert!(r.memory.reclaiming.borrow().is_none(), "the pass answers the settled reclaim");
    let answered = shown(&r);
    r.publish_memory();
    assert_eq!(answered, shown(&r), "the answer to the reclaim published a display report without the store the grant pass had taken out");
}

#[test]
fn a_unit_whose_room_returns_between_its_refusal_and_its_park_enters_without_another_release() {
    let r=legacy_scene_renderer(Default::default());
    r.scene_tiers.display.set_limit(r.scene_tiers.display.used()+8,0);
    let held=r.scene_tiers.display.charge(8);
    let refused=r.admit_display(None,crate::residency::Class::Visible,1,crate::frame_demand::Allowance::Spent).err().expect("the full tier refuses the unit");
    drop(held);
    let wait=crate::frame_demand::UnitWait::parked(refused,crate::frame_demand::Allowance::Spent);
    assert_eq!(wait.observe(&r),crate::frame_demand::Turn::Enter(crate::frame_demand::Allowance::Spent),
        "the release that landed after the failed attempt is the gain the parked unit waits for");
}

#[test]
fn a_joint_need_limited_by_its_second_tier_enters_when_only_that_tier_releases() {
    let r=legacy_scene_renderer(Default::default());
    let [first,second]=[0;2].map(|_|{let tier=Arc::new(crate::residency::Tier::default());tier.set_limit(8,0);tier});
    let held=second.charge(8);
    let class=crate::residency::Class::Prefetch;
    let need=first.wanting(class,4).with(second.blocked(class,4).expect("the second tier is full"));
    let wait=crate::frame_demand::UnitWait::parked(need,crate::frame_demand::Allowance::Spent);
    assert_eq!(wait.observe(&r),crate::frame_demand::Turn::Dormant,"the joint need waits while its second tier is full");
    drop(held);
    assert_eq!(wait.observe(&r),crate::frame_demand::Turn::Enter(crate::frame_demand::Allowance::Spent),
        "room on the limiting second tier revives the joint need");
}

fn raster_headroom(r: &WgpuRasterizer) -> u64 {
    r.memory.raster.handle.gpu_working_limit().saturating_sub(r.raster_tiers.raster.used())
}

fn lendable(r: &WgpuRasterizer) -> u64 {
    r.publish_memory();
    r.memory.lane.borrow().lendable
}

#[test]
fn raster_admission_counts_pages_and_retained_buffers_from_creation() {
    let mut r = legacy_scene_renderer(Default::default());
    let headroom = raster_headroom(&r);
    let page = r.create_page([0, 0], "unpublished raster page");
    assert!(r.admit_raster_working(headroom + lendable(&r),crate::frame_demand::Allowance::Fresh).is_err(), "a page counts from its creation, before any layer holds it");
    let headroom = raster_headroom(&r);
    r.refresh_memory_limits();
    let held = r.raster_tiers.raster.used();
    r.raster_buffers.put(r.device.create_buffer(&wgpu::BufferDescriptor { label: Some("retained raster buffer"), size: 64 << 10,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }));
    assert_eq!(r.raster_tiers.raster.used() - held, 64 << 10, "a retained buffer counts while the pool holds it");
    assert!(r.admit_raster_working(headroom,crate::frame_demand::Allowance::Fresh).is_ok(), "the retained buffer yields to authored working memory");
    assert_eq!(r.raster_buffers.retained.used(), 0);
    drop(page);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_refused_raster_admission_is_not_rearmed_by_an_unrelated_page_release() {
    let mut r = legacy_scene_renderer(Default::default());
    let page = r.create_page([0, 0], "released raster page");
    let (send, receive) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = send.send(()); }));
    let stroke=layer_core::StrokeId(1);
    r.live_stroke=Some(stroke);r.memory.claim(r.live_stroke);
    let room = r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used());
    let wanted=room+lendable(&r)+1;
    assert!(r.admit_raster_working(wanted,crate::frame_demand::Allowance::Fresh).is_err());
    assert!(r.lane_exhausted() && !r.lane_waits(),"an unretiring page is no producer for this head");
    while receive.try_recv().is_ok() {}
    let before=crate::test_support::PassCounts::snapshot();
    drop(page);
    assert!(receive.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"the unrelated release must not wake the refused head");
    assert!(r.admit_raster_working(wanted,crate::frame_demand::Allowance::Fresh).is_err(),"the undelivered refusal is unchanged");
    assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).raster_admissions,0,"the head must not admit again until its refusal is delivered");
    assert_eq!(r.take_lane_failure(stroke),Some(GpuRasterError::StrokeMemory));
    assert_eq!(r.take_lane_failure(stroke),None,"the operation receives its refusal once");
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
    let cells = display_floor(frame, 0, WORKING_BYTES, crate::display_precision::DisplayPrecision::for_scene(frame.scene, false)) - WORKING_BYTES - IN_FLIGHT_DECODE_BYTES;
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
        r.admit_display(None, class, bytes,crate::frame_demand::Allowance::Fresh).ok()
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
fn a_streamed_lookahead_decode_queues_behind_a_contact_decode_requested_after_it() {
    use crate::residency::Class;
    let source = decoded_source(2);
    let mut r = decoded_renderer(&source);
    let declared = |r: &WgpuRasterizer, x: u32| {let mut inputs = scene::sources::Inputs::default();inputs.image(&source, tile(x), r.document_color().space);inputs};
    let lookahead = declared(&r, 0).acquire_in(&r, Class::Fallback, &Default::default()).unwrap();
    assert!(!lookahead.prepare(&mut r).unwrap(), "the lookahead step's decode is still outstanding");
    let contact = declared(&r, 1).acquire(&r, Class::Authored).unwrap();
    assert!(!contact.prepare(&mut r).unwrap(), "the contact's decode is still outstanding");
    let queue = r.image_decode_queue(r.document_color().space).unwrap();
    let classes = [0, 1].map(|x| queue.awaits_image(&source, tile(x)));
    assert_eq!(classes, [Some(Class::Fallback), Some(Class::Authored)], "a stream's lookahead decode is requested at the stream's class, below the contact's");
}

#[test]
fn a_step_that_reads_its_cohort_slot_is_not_promised_that_slot_for_its_missing_tile() {
    use crate::residency::Class;
    let layers = 16;
    let source = decoded_source(layers + 1);
    let mut r = decoded_renderer(&source);
    for x in 0..layers { decode_at(&mut r, &source, x, Class::Visible); }
    assert_eq!(r.source_tiles.borrow().live_arrays(), 1, "the fixture fills one source array");
    let declared = |r: &WgpuRasterizer, tiles: &[u32]| {let mut inputs = scene::sources::Inputs::default();for x in tiles {inputs.image(&source, tile(*x), r.document_color().space);}inputs};
    let mut cohort = scene::sources::Cohort::default();
    let held = declared(&r, &[0]).acquire_in(&r, Class::Visible, &cohort).unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    cohort.read(&held, &encoder.submission());
    drop(held);encoder.submit(&r.queue);complete(&r);
    let step = declared(&r, &[0, layers]);
    let promised = {let cache = r.source_tiles.borrow();step.uncovered_bytes(&cache, &mut cohort.spare_credit(&cache, [&step].into_iter()))};
    let acquired = step.acquire_in(&r, Class::Visible, &cohort).unwrap();
    let created = r.source_tiles.borrow().live_arrays() - 1;
    assert!(created * scene::sources::ARRAY_BYTES <= promised,
        "the step's own read pins the cohort slot, so its missing tile's {created} new array(s) must be promised, not {promised} bytes");
    drop(acquired);
}

#[test]
fn a_refused_stream_leaves_no_pin_on_the_slots_it_prepared_but_never_read() {
    use crate::residency::Class;
    let source = decoded_source(2);
    let r = decoded_renderer(&source);
    let cohort = scene::sources::Cohort::default();
    let before = r.source_tiles.borrow().pinned_bytes();
    let mut inputs = scene::sources::Inputs::default();
    inputs.image(&source, tile(0), r.document_color().space);
    let prepared = inputs.acquire_in(&r, Class::Visible, &cohort).unwrap();
    assert!(r.source_tiles.borrow().pinned_bytes() > before, "the stream pins the slot it prepared");
    drop(prepared);
    assert_eq!(r.source_tiles.borrow().pinned_bytes(), before, "the view keeps no pin on a slot its refused stream never read");
}

fn photo_document(source: &Arc<layer_core::color::source::SourceImage>) -> (layer_core::Document, layer_core::PaintBase) {
    let mut doc = layer_core::Document::new(layer_core::PortableId::random(), source.extent[0], source.extent[1], layer_core::DocumentNames { paint: "Photo".into(), paper: "Paper".into() });
    let target = doc.artwork.paint.iter().next().unwrap().0;
    let base = layer_core::PaintBase::new(source.clone().into());
    doc.artwork.paint.get_mut(target).unwrap().base = Some(base.clone());
    (layer_core::Document::from_artwork(doc.artwork).unwrap(), base)
}

fn queued_decodes(r: &mut WgpuRasterizer, base: &layer_core::PaintBase) -> Vec<crate::residency::Class> {
    let queue = r.image_decode_queue(r.document_color().space).unwrap();
    (0..16).filter_map(|x| queue.awaits_image(base.image.storage(), [x, 0])).collect()
}

#[test]
fn a_fit_cell_the_preview_admits_streams_its_decodes_at_the_view_class_once_the_view_joins() {
    use crate::residency::Class;
    let source = decoded_source(16);
    let (doc, base) = photo_document(&source);
    let mut r = decoded_renderer(&source);
    let queued = (0..120).find_map(|_| {
        crate::test_support::submit_ready(&mut r, fit_view(&doc, 1024)).unwrap();
        let queued = queued_decodes(&mut r, &base);
        (!queued.is_empty()).then_some(queued)
    }).unwrap_or_default();
    assert!(queued.iter().any(|class| *class <= Class::Visible) && !queued.iter().any(|class| matches!(class, Class::Fallback | Class::Navigator)),
        "the Preview admits a Fit cell at Fallback and the View joins it, so the cell's owed decodes run at Visible: {queued:?}");
}

#[test]
fn a_view_cell_decodes_ahead_at_prefetch_only_while_no_stroke_decode_waits() {
    use crate::residency::Class;
    let source = decoded_source(16);
    let (doc, base) = photo_document(&source);
    let mut r = decoded_renderer(&source);
    let held = layer_core::color::source::rgba8_source([PAGE_SIZE; 2], |_, _| [255, 128, 0, 255]);
    let mut artwork = layer_core::authored::Artwork::new([PAGE_SIZE; 2]).unwrap();
    let (_, layer_core::authored::SourceTarget::Paint(owner)) = crate::test_support::add_paint(&mut artwork, "Stroke source", [PAGE_SIZE; 2]) else { unreachable!() };
    artwork.paint.get_mut(owner).unwrap().base = Some(layer_core::PaintBase::new(held.clone().into()));
    let store = crate::test_support::GatedStore::new(&layer_core::Editor::new(layer_core::Document::from_artwork(artwork).unwrap()).retained_tiles(), false);
    let mut stroke = scene::sources::Inputs::default();
    stroke.image(&held, [0, 0], r.document_color().space);
    let stroke = stroke.acquire(&r, Class::Authored).unwrap();
    assert!(!stroke.prepare(&mut r).unwrap(), "the stroke's decode waits on its gated read");
    store.gate.entered();
    let owed = (0..120).find_map(|_| {
        crate::test_support::submit_ready(&mut r, fit_view(&doc, 1024)).unwrap();
        let queued = queued_decodes(&mut r, &base);
        (!queued.is_empty()).then_some(queued)
    }).unwrap_or_default();
    assert!(!owed.is_empty() && !owed.contains(&Class::Prefetch), "while the stroke's decode waits the view requests only its owed steps: {owed:?}");
    store.gate.release();
    let ahead = (0..120).find_map(|_| {
        crate::test_support::submit_ready(&mut r, fit_view(&doc, 1024)).unwrap();
        let queued = queued_decodes(&mut r, &base);
        queued.contains(&Class::Prefetch).then_some(queued)
    });
    assert!(ahead.is_some(), "once the stroke's decode is delivered the view decodes its next steps ahead at Prefetch, below every owed decode");
    drop(stroke);
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
    assert!(r.admit_display(None, crate::residency::Class::Navigator, bytes,crate::frame_demand::Allowance::Fresh).is_err(), "the grant is full for optional work");
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.retire_victims(None, &mut encoder);
    encoder.submit(&r.queue);
    complete(&r);
    r.device.retirement_handle().flush().unwrap();
    assert!(r.admit_display(None, crate::residency::Class::Navigator, bytes,crate::frame_demand::Allowance::Fresh).is_ok(), "the miss evicted undemanded decodes for the navigator");
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
    assert_eq!(r.scene_tiers.display.floor(), display_floor(frame, presented, unit, r.display_precision(frame.scene)), "the tier floor holds the same unit");
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

pub(crate) fn layered_raster_document(layers:u8,tiles:u32)->layer_core::Document {
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

#[test]
fn a_pen_down_after_idle_frames_composes_its_cell_from_the_tiles_the_visible_cells_decoded() {
    struct Row {layers:u8,tiles:u32,photo:Option<[u32;2]>,surface:[u32;2],from:Option<[f32;6]>,shown:[f32;6],at:[f32;2]}
    let actual=[1.,0.,0.,1.,-1024.,-1024.];
    let fit=|tiles:u32|{let scale=768./(PAGE_SIZE*tiles) as f32;[scale,0.,0.,scale,128.,0.]};
    let raster=|layers,tiles,from,shown,at|Row {layers,tiles,photo:None,surface:[1024,768],from,shown,at};
    let dab=[1500.,1400.];
    let photo=1600./2832.;
    let rows=[raster(1,8,None,actual,dab),raster(2,8,None,actual,dab),raster(1,12,None,actual,dab),raster(4,8,None,actual,dab),
        raster(1,8,None,[0.5,0.,0.,0.5,0.,-128.],dab),raster(1,8,None,fit(8),dab),raster(2,8,Some([1.,0.,0.,1.,0.,0.]),actual,dab),raster(1,31,None,fit(31),[3968.,3968.]),
        Row {layers:1,tiles:0,photo:Some([4248,2832]),surface:[2560,1600],from:None,shown:[photo,0.,0.,photo,(2560.-4248.*photo)/2.,0.],at:[2124.,1416.]}];
    let (mut table,mut failed)=(Vec::new(),false);
    for Row {layers,tiles,photo,surface,from,shown,at} in rows {
        let doc=photo.map_or_else(||layered_raster_document(layers,tiles),|extent| {
            let mut doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Photo".into(),paper:"Paper".into()});
            let layer_core::SourceTarget::Paint(paint)=doc.working.target.unwrap() else {unreachable!()};
            doc.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase::new(layer_core::color::source::rgba8_source(extent,|x,y|
                [(x/7) as u8,(y/5) as u8,((x+y)/3) as u8,255]).into()));
            doc
        });
        let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
        let name=format!("Layer {}",layers-1);
        let layer=if photo.is_some() {doc.working.occurrence.unwrap()}
            else {doc.scene().order().iter().copied().find(|handle|doc.scene().occurrence(*handle).unwrap().name.as_ref()==name).unwrap()};
        layer_render::CanvasRenderer::set_working_layer(&mut r,Some(layer));
        let mut frame=packet(doc.scene(),doc.composition().size);frame.composite_all=false;
        [frame.view.width_px,frame.view.height_px]=surface;
        if let Some(from)=from {frame.view.document_to_surface=from;settle_observing(&mut r,frame,|_|{});}
        frame.view.document_to_surface=shown;
        settle_observing(&mut r,frame,|_|{});
        for _ in 0..4 {crate::test_support::submit_ready(&mut r,frame).unwrap();complete(&r);}
        let (decoded,released)={let sources=r.source_tiles.borrow();(sources.misses,sources.evictions)};
        let (protected,floor,limit)=(r.release_unused_decodes(),r.scene_tiers.display.floor(),r.scene_tiers.display.limit());
        let [held,wanted,warm]=scene::sources::tests::pen_down_coverage(&r);
        let dab=crate::tests::test_dab(at,[0.9,0.1,0.3,1.],0.8);
        let batch=dab_batch(doc.scene().source_target(layer).unwrap(),crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        let level=r.scale_display.as_ref().unwrap().plan.level;
        let cell=at.map(|value|value as u32/(PAGE_SIZE<<level));
        let composed=r.metrics.frame_composited_pages.contains(&(level,cell));
        let decodes=r.source_tiles.borrow().misses-decoded;
        failed|=decodes!=0 || level==0 && !composed || protected>limit.max(floor) || wanted>9 || warm>9*scene::sources::NORMALIZED_PAGE_BYTES;
        table.push(format!("{layers} layers of {tiles}² tiles, photo {photo:?}, surface {surface:?} at {shown:?} after {from:?}: released {released}, protected {} of floor {} and display limit {} KiB, \
            working tiles held {held} of {wanted} ({} KiB), pen-down at {at:?} decodes {decodes}, cell {cell:?} at level {level} composed {composed}",
            protected>>10,floor>>10,limit>>10,warm>>10));
    }
    assert!(!failed,"a pen-down after idle frames must compose its cell from tiles the view already decoded, keeping at most the 3×3 working tiles around the pointer within the display limit:\n{}",table.join("\n"));
}

#[test]
fn a_settled_view_whose_working_tiles_exceed_the_room_stops_asking_for_frames() {
    let doc=layered_raster_document(2,16);
    let named=|doc:&layer_core::Document,name:&str|doc.scene().order().iter().copied().find(|handle|doc.scene().occurrence(*handle).unwrap().name.as_ref()==name).unwrap();
    let mut artwork=doc.artwork.clone();
    artwork.occurrences.get_mut(named(&doc,"Layer 1")).unwrap().visible=false;
    let doc=layer_core::Document::from_artwork(artwork).unwrap();
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()}).unwrap();
    let mut frame=packet(doc.scene(),doc.composition().size);frame.composite_all=false;
    frame.view.width_px=1024;frame.view.height_px=768;
    let scale=768./(PAGE_SIZE*16) as f32;frame.view.document_to_surface=[scale,0.,0.,scale,128.,0.];
    settle_observing(&mut r,frame,|_|{});
    let display=r.scene_tiers.display.clone();
    let room=2*scene::sources::NORMALIZED_PAGE_BYTES;
    let _held=display.charge(r.source_tiles.borrow().idle_room(display.limit(),display.used()).saturating_sub(room));
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(named(&doc,"Layer 1")));
    let mut frames=0;
    loop {
        crate::test_support::submit_ready(&mut r,frame).unwrap();complete(&r);r.wait_idle().unwrap();crate::test_support::settle_decodes(&mut r);frames+=1;
        if r.next_frame()!=layer_render::FrameDemand::Work {break;}
        assert!(frames<8,"the settled view still asks for frames after {frames}: {:?}",r.outstanding_terms());
    }
    for _ in 0..4 {
        r.wait_idle().unwrap();crate::test_support::settle_decodes(&mut r);r.poll_pending(frame.view).unwrap();
        assert_ne!(r.next_frame(),layer_render::FrameDemand::Work,"an idle settled view asks for a frame: {:?}",r.outstanding_terms());
    }
    let [held,wanted,bytes]=scene::sources::tests::pen_down_coverage(&r);
    assert!(held<wanted && bytes<=room,"the room of {} KiB holds {held} of {wanted} working tiles under the view, {} KiB",room>>10,bytes>>10);
}

fn settle_observing(r:&mut WgpuRasterizer,frame:FramePacket<'_>,mut observe:impl FnMut(&WgpuRasterizer)) {
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(120);
    let mut submitted=false;
    loop {
        r.poll_startup().unwrap();r.poll_pending(frame.view).unwrap();
        let next=if !submitted {frame}else {FramePacket {composite_all:false,..frame}};
        let preview=r.transform_preview.clone();
        crate::test_support::pace(r);
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
fn stroke_frames_build_every_half_of_their_dab_cells_and_none_outside_them() {
    let doc=layered_raster_document(8,8);
    let mut r=renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    let layer=working_layer(&doc);
    let target=doc.scene().source_target(layer).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(layer));
    let mut frame=packet(doc.scene(),doc.composition().size);frame.composite_all=false;
    frame.view.width_px=512;frame.view.height_px=512;frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    settle_observing(&mut r,frame,|_|{});
    let cell=PixelRect::new(0,0,1024,1024);
    let level=r.scale_display.as_ref().unwrap().plan.level;
    assert_eq!(level,2);
    let units=2*(cell.width()/(PAGE_SIZE<<level)*cell.height()/(PAGE_SIZE<<level)) as usize;
    let mut built=0;
    for step in 0..8 {
        let dab=crate::tests::test_dab([200.+8.*step as f32,200.],[0.9,0.1,0.3,1.],0.8);
        let batch=crate::test_support::dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        crate::test_support::complete(&r);
        let halves=r.scale_display.as_ref().unwrap().half_pages();
        assert!(halves.iter().all(|(_,region)|region.intersect(cell)==*region),"stroke frame {step} built halves outside its dab cell: {halves:?}");
        built=halves.iter().filter(|(actual,_)|*actual==level).count();
    }
    assert_eq!(built,units,"the stroke builds both halves at the level of the cell it reaches");
    assert!(r.scale_display.as_ref().unwrap().has_cell_halves(cell),"both reached halves have valid coverage");
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
        r.source_tiles.borrow().release_unused(0,0,|_|Default::default());r.source_tiles.borrow_mut().retire_evicted();
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

#[test]
fn a_gigapixel_fit_on_a_tablet_budget_selects_the_finest_level_whose_window_fits() {
    let side=131_072u32;
    let mut doc=layer_core::Document::new(layer_core::PortableId::random(),side,side,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let root=doc.artwork.root;
    doc.artwork.compositions.get_mut(root).unwrap().color.depth=layer_core::color::SampleDepth::F32;
    let r=tablet_renderer(doc.composition().color);
    assert!(r.device.limits().max_texture_dimension_2d>=8192,"the level-4 root passes the dimension check");
    let mut frame=packet(doc.scene(),[side;2]);
    frame.view.width_px=1024;frame.view.height_px=1024;frame.view.document_to_surface=[1./128.,0.,0.,1./128.,0.,0.];
    let request=crate::scene::scale::request(&r,frame).unwrap();
    let bytes=|level:u32|[level,level+1].map(|level|u64::from(side>>level).pow(2)*16).into_iter().sum::<u64>();
    let limit=r.memory.display_limit();
    assert!(bytes(4)>limit,"the level-4 root and next exceed the tablet's display room of {limit} bytes");
    assert!(request.plan.level>4 && request.plan.level<=7 && bytes(request.plan.level)<=limit && bytes(request.plan.level-1)>limit,
        "Fit selects the finest legal level whose root and next fit: level {} on {limit} bytes",request.plan.level);
}

fn tablet_open(doc:&layer_core::Document,size:[u32;2],scale:f32,mut observe:impl FnMut(&WgpuRasterizer))->(WgpuRasterizer,crate::test_support::PassCounts,u64) {
    let extent=doc.composition().size;
    let mut r=tablet_renderer(doc.composition().color);
    let mut frame=packet(doc.scene(),extent);frame.view.width_px=size[0];frame.view.height_px=size[1];
    frame.view.document_to_surface=[scale,0.,0.,scale,size[0] as f32/2.-scale*extent[0] as f32/2.,size[1] as f32/2.-scale*extent[1] as f32/2.];
    crate::test_support::count_source_preparations();
    let counts=crate::test_support::PassCounts::snapshot();
    let mut frames=0;
    settle_observing(&mut r,frame,|r|{frames+=1;observe(r);});
    (r,crate::test_support::PassCounts::snapshot().delta(counts),frames)
}

#[test]
fn a_tablet_budget_cold_open_at_fit_grants_each_cell_once_and_writes_each_present_tile_once() {
    let doc=layered_raster_document(28,16);
    let (r,counts,frames)=tablet_open(&doc,[1366;2],1./3.,|_|{});
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level,1,"a third of the 4096 x 4096 drawing opens on level 1");
    let (cells,present)=(8*8,28*16*16);
    let [written,distinct]=crate::test_support::source_preparations();
    assert_eq!([counts.unit_admissions,written,distinct],[cells,present,present],
        "each of the {cells} level-1 cells is granted once and writes each of its 112 layer tiles once, in {frames} frames: {counts:?}");
    assert!(counts.unit_acceptances<=cells+8,"{} parent attempts for {cells} grants in {frames} frames; each cell reuses the decoded slots and layer pages of the cells before it instead of asking for fresh room: {counts:?}",counts.unit_acceptances);
    let decodes=r.source_tiles.borrow().misses;
    assert!(decodes<=present+present/50,"{decodes} decodes of {present} present tiles");
}

#[test]
fn a_tablet_budget_zoomed_cold_open_decodes_a_tile_at_most_once_for_the_overview_and_once_for_the_view() {
    let doc=layered_raster_document(28,12);
    let (r,..)=tablet_open(&doc,[1920,1200],1.,|r| {r.source_tiles.borrow().release_ready();r.source_tiles.borrow_mut().retire_evicted();});
    let view=PixelRect::new(1536-960,1536-600,1536+960,1536+600);
    let visible=28*page_coordinates(view).count() as u64;
    let decodes=r.source_tiles.borrow().misses;
    assert!(decodes<=28*12*12+visible,"{decodes} decodes: the overview reads each of {} present tiles and the view its {visible} visible tiles at most once more",28*12*12);
}

#[test]
fn a_far_edge_panorama_stroke_keeps_its_source_arrays_and_settles_within_870_ticks_after_pen_up() {
    use layer_core::color::{SampleDepth,source::{SourceChannels,SourceImage,SourceInterpretation}};
    use layer_core::raster::{TILE_SIZE,TileBlob};
    use layer_engine::{CanvasEngine,PenEvent,PenPhase,SampleFlags,ToolKind,ViewTransform,input_queue};
    const VIEW:[u32;2]=[2880,1800];
    const SIZE:[u32;2]=[65536,16384];
    const EDGE:[f32;2]=[0.985,0.5];
    const FRAME_NS:u64=8_333_333;
    const PEN_NS:u64=5_000_000;
    const SETTLE:u64=870;
    let budget=MemoryBudget::with_facts(MemoryFacts {host_default_bytes:Some(1<<30),override_bytes:Some(1<<30),tablet:true,..Default::default()});
    let interpretation=SourceInterpretation {channels:SourceChannels::Rgb,depth:SampleDepth::U8,profile:Default::default(),profile_assumed:false};
    let descriptor=interpretation.descriptor();
    let coordinates=(0..SIZE[1]/TILE_SIZE).flat_map(|y|(0..SIZE[0]/TILE_SIZE).map(move|x|[x,y])).collect::<Vec<_>>();
    let workers=std::thread::available_parallelism().map_or(4,usize::from);
    let tiles=std::thread::scope(|scope| coordinates.chunks(coordinates.len().div_ceil(workers)).map(|chunk|scope.spawn(move|| {
        let mut tile:Vec<u8>=(0..TILE_SIZE).flat_map(|y|(0..TILE_SIZE).flat_map(move|x|[64+(x/8) as u8,144-(y/8) as u8,106+((x+y)/16) as u8])).collect();
        chunk.iter().map(|&[x,y]| {tile[..3].copy_from_slice(&[x as u8,y as u8,128]);([x,y],Arc::new(TileBlob::encode(descriptor,&tile).unwrap()))}).collect::<Vec<_>>()
    })).collect::<Vec<_>>().into_iter().flat_map(|worker|worker.join().unwrap()).collect());
    let image=Arc::new(SourceImage {extent:SIZE,resolution:None,interpretation,tiles});
    image.validate().unwrap();
    let mut doc=layer_core::Document::new(layer_core::PortableId::random(),SIZE[0],SIZE[1],layer_core::DocumentNames {paint:"Panorama".into(),paper:"Paper".into()});
    let target=doc.artwork.paint.iter().next().unwrap().0;
    doc.artwork.paint.get_mut(target).unwrap().base=Some(layer_core::PaintBase::new(image.into()));
    let working=doc.working.clone();
    let mut doc=layer_core::Document::from_artwork(doc.artwork).unwrap().with_memory(layer_core::DocumentMemory::new(budget.clone(),None));
    doc.working=working;doc.initialize_memory();budget.flush_reports();
    let mut brush=layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);brush.diameter=100.;
    let mut gpu=renderer_in_budget(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),unified:true,..Default::default()},budget).unwrap();
    gpu.prepare_startup(&doc,&brush,false).unwrap();gpu.finish_startup_catalog();
    crate::test_support::wait_startup(&mut gpu,std::time::Instant::now()+std::time::Duration::from_secs(60),|progress|progress.complete,format_args!("startup"));
    let camera=|scale:f32,anchor:[f32;2],pan:[f32;2],revision:u64| {
        let position=[0,1].map(|axis|VIEW[axis] as f32/2.-anchor[axis]*SIZE[axis] as f32*scale+pan[axis]);
        (layer_render::ViewState {width_px:VIEW[0],height_px:VIEW[1],document_to_surface:[scale,0.,0.,scale,position[0],position[1]]},
            ViewTransform {revision,surface_to_document:[1./scale,0.,0.,1./scale,-position[0]/scale,-position[1]/scale]})
    };
    let fit=VIEW[0] as f32/SIZE[0] as f32;
    let (view,transform)=camera(fit,[0.5;2],[0.;2],0);
    let (mut input,consumer)=input_queue(1024);
    let mut engine=CanvasEngine::new(gpu,doc,consumer,view,transform).unwrap();
    engine.set_brush(brush).unwrap();
    let tick=|engine:&mut CanvasEngine<WgpuRasterizer>,clock:&mut u64| {
        *clock+=FRAME_NS;
        crate::test_support::pace(engine.backend_mut());
        engine.backend_mut().poll_startup().unwrap();
        engine.render_frame_for(*clock,*clock+FRAME_NS).unwrap();
        crate::test_support::wait_until("the journey's pipelines compile",||match engine.backend().compilation.pending() {0=>Ok(()),pending=>Err(pending)});
        engine.backend_mut().wait_idle().unwrap();
        crate::test_support::settle_decodes(engine.backend_mut());
    };
    let settled=|engine:&CanvasEngine<WgpuRasterizer>|engine.backend().has_presentable_frame() && engine.backend().settled()
        && matches!(engine.frame_demand(),layer_render::FrameDemand::Wait) && !engine.has_pending_document_edits() && !engine.has_pending_input() && !engine.has_active_stroke();
    let drive=|engine:&mut CanvasEngine<WgpuRasterizer>,clock:&mut u64,ticks:u64|(1..=ticks).find(|_|{tick(engine,clock);settled(engine)});
    let mut clock=0;
    drive(&mut engine,&mut clock,1440);
    let mut revision=0;
    for (scale,anchor,pan,ticks) in [(fit,[0.5;2],[-106.,100.],360),(0.4,EDGE,[0.;2],600),(1.,EDGE,[0.;2],1200)] {
        revision+=1;let (view,transform)=camera(scale,anchor,pan,revision);engine.set_view(view,transform);
        drive(&mut engine,&mut clock,ticks);
    }
    let mut sequence=0;
    let mut contact=|engine:&mut CanvasEngine<WgpuRasterizer>,clock:&mut u64,events:u64,stationary:bool| {
        let (start,mut sent)=(*clock,0);
        while sent<events {
            while sent<((*clock-start)/PEN_NS+1).min(events) {
                let angle=std::f32::consts::TAU*(sent*PEN_NS) as f32/1e9;
                let phase=if sent==0 {PenPhase::Down}else if sent==events-1 {PenPhase::Up}else {PenPhase::Move};
                let offset=if stationary {[0.;2]}else {[VIEW[0] as f32*0.3*angle.cos(),VIEW[1] as f32*0.2*angle.sin()]};
                let pressure=if sent==events-1 {0.}else if stationary {1.}else {0.625-0.375*angle.cos()};
                sequence+=1;
                input.push(PenEvent {device_id:1,sequence,timestamp_ns:start+sent*PEN_NS,view_revision:revision,
                    surface_position:layer_core::Point {x:VIEW[0] as f32/2.+offset[0],y:VIEW[1] as f32/2.+offset[1]},pressure,
                    tilt_radians:[0.;2],twist_radians:0.,distance:0.,phase,tool:ToolKind::Pen,flags:SampleFlags::PRIMARY}).unwrap();
                sent+=1;
            }
            tick(engine,clock);
        }
    };
    let blank=engine.document().revision;
    contact(&mut engine,&mut clock,21,true);
    for _ in 0..960 {if engine.document().revision>blank {break;}tick(&mut engine,&mut clock);}
    let primed=engine.document().revision;
    assert!(primed>blank,"the priming dab commits");
    contact(&mut engine,&mut clock,251,false);
    let (created,live)=(crate::test_support::source_arrays(),engine.backend().source_tiles.borrow().live_arrays());
    let ticks=drive(&mut engine,&mut clock,SETTLE);
    let creates=crate::test_support::source_arrays()-created;
    let frees=live+creates-engine.backend().source_tiles.borrow().live_arrays();
    assert!(engine.document().revision>primed,"the stroke commits");
    let arrays=(2*crate::scene::sources::ARRAY_BYTES+2*40*crate::scene::sources::NORMALIZED_PAGE_BYTES).div_ceil(crate::scene::sources::ARRAY_BYTES)+1;
    assert!(ticks.is_some() && creates<=arrays,"after pen-up the panorama settled at tick {ticks:?} of {SETTLE} and created {creates} and freed {frees} 16 MiB source arrays from {live} live; \
        its producers hold a working set of at most {arrays} arrays instead of freeing and recreating them per region");
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
    let cache=r.scale_display.as_ref().unwrap();
    assert!(cache.plan.level>0 && cache.has_exact_window(),"the photo settles exact at the reduced view level");
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

#[test]
fn retiring_a_stroke_keeps_only_warm_coverage_and_the_next_stroke_reuses_it() {
    let side = [8, 8];
    let extent = side.map(|side| PAGE_SIZE * side);
    let doc = layer_core::Document::new(layer_core::PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let target = doc.active_target().unwrap();
    let mut r = renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    submit_settled(&mut r, packet(doc.scene(), extent));
    let style = crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
    let bounds = layer_core::Rect { min: layer_core::Point { x: 0., y: 0. }, max: layer_core::Point { x: extent[0] as f32, y: extent[1] as f32 } };
    let dabs = (0..side[0] * side[1]).map(|index| crate::tests::test_dab(
        [((index % side[0]) * PAGE_SIZE + PAGE_SIZE / 2) as f32, ((index / side[0]) * PAGE_SIZE + PAGE_SIZE / 2) as f32], [0.1, 0.3, 0.8, 1.], 1.)).collect::<Vec<_>>();
    let stroke = |r: &mut WgpuRasterizer, id: u64| {
        for (stroke_start, stroke_end) in [(true, false), (false, true)] {
            let batch = layer_render::DabBatch { stroke_start, stroke_end, stroke_id: layer_core::StrokeId(id), dab_count: dabs.len() as u32, ..dab_batch(target, style.clone(), bounds) };
            submit_published(r, FramePacket { dabs: &dabs, dab_batches: std::slice::from_ref(&batch), ..packet(doc.scene(), extent) });
        }
        complete(r);
    };
    stroke(&mut r, 1);
    let coverage = |r: &WgpuRasterizer| r.paint_layers.iter().map(|layer| layer.coverage_pages.len()).sum::<usize>();
    assert!(coverage(&r) > WARM_COVERAGE_PAGES, "the fixture leaves more finished coverage than the warm pool: {}", coverage(&r));
    r.retire_stroke_sources();
    assert_eq!(coverage(&r), WARM_COVERAGE_PAGES, "retirement keeps only the warm coverage pool");
    assert!(r.paint_layers.iter().flat_map(|layer| &layer.pages).all(|page| page.stroke_start.is_none()));
    stroke(&mut r, 2);
    assert!(coverage(&r) >= WARM_COVERAGE_PAGES, "the next stroke allocates what the warm pool lacks");
}

#[test]
fn a_display_blocked_while_ram_is_exhausted_completes_once_another_owner_releases_it() {
    let extent = [4096, 4096];
    let budget = MemoryBudget::with_facts(MemoryFacts { host_default_bytes: Some(2 << 30), override_bytes: Some(2 << 30), ..Default::default() });
    let photo = layer_core::color::source::rgba8_source(extent, |x, y| [(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 199) as u8, 255]);
    let document = layer_color::photo_project(Arc::unwrap_or_clone(photo), Default::default(), "Photo".into(), layer_core::color::SampleDepth::U8).unwrap();
    let mut r = renderer_in_budget(document.composition().color, layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }, budget.clone()).unwrap();
    let view = layer_render::ViewState { width_px: 1024, height_px: 1024, document_to_surface: [0.25, 0., 0., 0.25, 0., 0.] };
    let frame = FramePacket { view, ..crate::test_support::packet(document.scene(), extent) };
    crate::test_support::submit_settled(&mut r, frame);
    let pressure = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap();
    let (signal, woken) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = signal.send(()); }));
    let class = crate::residency::Class::Navigator;
    let bytes = r.scene_tiers.display.headroom(class) + r.scene_tiers.display.yielding() + MIB;
    let blocked = r.scene_tiers.display.blocked(class, bytes).expect("with the RAM taken by another owner the display has no room for optional work");
    let wait = crate::frame_demand::UnitWait::parked(blocked, crate::frame_demand::Allowance::Spent);
    assert_eq!(wait.observe(&r), crate::frame_demand::Turn::Dormant);
    while woken.try_recv().is_ok() {}
    drop(pressure);
    assert!(woken.recv_timeout(std::time::Duration::from_secs(10)).is_ok(), "the room another owner returns wakes the renderer");
    let Some(allowance) = wait.eligible(&r) else { panic!("without a frame, the parked request is judged against the room the ledger now gives the display") };
    assert!(r.admit_display(None, class, bytes, allowance).is_ok(), "and the request is admitted");
}

#[test]
fn a_tablet_ellipse_at_fit_keeps_each_predicted_page_s_display_inputs() {
    use crate::test_support::pen;
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue};
    const EXTENT: [u32; 2] = [4248, 2832];
    const SIZE: [u32; 2] = [2004, 1316];
    const SAMPLES: u32 = 504;
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
    let mut document = layer_core::Document::from_artwork(document.artwork).unwrap();
    document.working.target = Some(target);
    document.working.occurrence = Some(owner);
    let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
    brush.diameter = 1024.;
    let mut gpu = renderer_in_budget(document.composition().color, layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }, budget).unwrap();
    gpu.prepare_startup(&document, &brush, false).unwrap();
    gpu.finish_startup_catalog();
    crate::test_support::wait_startup(&mut gpu, std::time::Instant::now() + std::time::Duration::from_secs(60), |progress| progress.complete, format_args!("startup"));
    let (mut input, consumer) = input_queue(1024);
    let scale = 0.33686f32;
    let [tx, ty] = [0, 1].map(|axis| (SIZE[axis] as f32 - EXTENT[axis] as f32 * scale) * 0.5);
    let view = layer_render::ViewState { width_px: SIZE[0], height_px: SIZE[1], document_to_surface: [scale, 0., 0., scale, tx, ty] };
    let mut engine = CanvasEngine::new(gpu, document, consumer, view,
        ViewTransform { revision: 0, surface_to_document: [1. / scale, 0., 0., 1. / scale, -tx / scale, -ty / scale] }).unwrap();
    engine.set_instant_feedback(InstantFeedbackConfig { use_platform_prediction: false, ..Default::default() }).unwrap();
    engine.render_frame_at(0).unwrap();
    engine.set_brush(brush).unwrap();
    let mut clock = 1_000_000_000u64;
    let settle = |engine: &mut CanvasEngine<WgpuRasterizer>, clock: &mut u64| {
        while !(engine.backend().raster_ready() && !crate::scene::scale::tests::publication_pending(engine.backend()) && !engine.has_pending_document_edits() && !engine.has_pending_input()) {
            *clock += 4_166_667;
            engine.render_frame_at(*clock).unwrap();
            engine.backend_mut().wait_idle().unwrap();
        }
    };
    settle(&mut engine, &mut clock);
    let mut frames = Vec::new();
    for contact in 0..2 {
        frames.clear();
        for first in (0..SAMPLES).step_by(9) {
            for sample in first..(first + 9).min(SAMPLES) {
                clock += 5_000_000;
                let angle = sample as f32 / SAMPLES as f32 * std::f32::consts::TAU;
                let phase = if sample == 0 { PenPhase::Down } else if sample + 1 == SAMPLES { PenPhase::Up } else { PenPhase::Move };
                input.push(PenEvent { timestamp_ns: clock, ..pen(clock, phase, [1002. + 543.6 * angle.cos(), 658. + 212. * angle.sin()], SampleFlags::PRIMARY) }).unwrap();
            }
            let shown = engine.backend().preview_footprint.clone();
            let before = crate::test_support::PassCounts::snapshot();
            engine.render_frame_at(clock).unwrap();
            engine.backend_mut().wait_idle().unwrap();
            let entered = engine.backend().preview_footprint.difference(&shown).count() as u64;
            frames.push((crate::test_support::PassCounts::snapshot().delta(before).display_gathers, entered));
        }
        if contact == 0 { settle(&mut engine, &mut clock); }
    }
    assert!(frames.iter().map(|(gathers, _)| gathers).sum::<u64>() > 0, "the prediction averages its pages at display resolution");
    let rebound: Vec<_> = frames.iter().enumerate().skip(3).filter(|(_, (gathers, entered))| gathers > entered).collect();
    assert!(rebound.is_empty(), "a page that stays in the prediction keeps its display inputs; (frame, (gathers, entered pages)): {rebound:?}");
}
