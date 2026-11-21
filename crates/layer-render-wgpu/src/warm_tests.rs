use super::*;
use crate::test_support::{complete, packet, submit_settled, PassCounts};
use layer_core::{MemoryClass, MemoryOwnerKind as Kind};

fn reduced(doc: &layer_core::Document) -> FramePacket<'_> {
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view = crate::test_support::view([512, 384]);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    frame
}

fn parked() -> (layer_core::Document, WgpuRasterizer) {
    let doc = crate::scene::scale::tests::document_at([2048, 1536]);
    let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { budget_bytes: Some(4 << 30), ..Default::default() }).unwrap();
    submit_settled(&mut r, reduced(&doc));
    (doc, r)
}

#[test]
fn a_parked_drawing_keeps_only_its_presented_display_within_the_cap() {
    let (doc, mut r) = parked();
    let presented = r.presentation_views().unwrap()[0].clone();
    let active = r.warm_bytes();
    assert!(r.raster_tiers.raster.used() > 0 && r.scene.as_ref().unwrap().retained_scale_bytes()[0] > 0);
    assert!(r.park_warm(&doc, reduced(&doc).view, false), "a settled reduced view fits the warm cap");
    complete(&r);
    assert!(r.warm_bytes() <= r.warm_tab_bytes() && r.warm_bytes() < active, "warm {} of {active} active, cap {}", r.warm_bytes(), r.warm_tab_bytes());
    assert_eq!(r.raster_tiers.raster.used(), r.pinned_paint_bytes(), "a parked drawing keeps only the layer pages it cannot evict");
    assert_eq!(r.presentation_views().unwrap()[0], &presented, "the presented display survives parking");
    let rows = r.factory.memory_budget().snapshot().rows;
    for kind in [Kind::Scene, Kind::RasterPool, Kind::Staging, Kind::DecodedSamples] {
        let row = rows[kind as usize][MemoryClass::Visible as usize];
        assert_eq!((row.pinned_gpu_bytes, row.pinned_ram_bytes), (0, 0), "{kind:?} pins nothing while parked");
    }
    assert_eq!(r.memory.display_owner().working(), Default::default(), "a parked display reserves no working memory");
}

#[test]
fn a_resumed_drawing_presents_its_retained_display_without_composing() {
    let (doc, mut r) = parked();
    let presented = r.presentation_views().unwrap()[0].clone();
    assert!(r.park_warm(&doc, reduced(&doc).view, false));
    assert!(r.resume_retained(&doc), "the parked drawing is unchanged");
    let before = PassCounts::snapshot();
    submit_settled(&mut r, FramePacket { composite_all: false, ..reduced(&doc) });
    assert!(r.showing_drawing());
    assert_eq!(r.presentation_views().unwrap()[0], &presented, "the retained display is the presented one");
    assert_eq!(PassCounts::snapshot().delta(before).composite_commits, 0, "an unchanged view composes nothing after resuming");
}

fn pressure(r: &WgpuRasterizer) -> layer_core::MemoryOwner {
    let budget = r.factory.memory_budget();
    let device = budget.snapshot().devices[0];
    let active = budget.register_owner(Kind::Other, MemoryClass::Visible, Some(device.device));
    active.report(layer_core::MemoryReport { demand_gpu_bytes: device.limit_bytes, ..Default::default() });
    budget.flush_reports();
    active
}

#[test]
fn memory_pressure_evicts_a_parked_drawing_before_any_active_demand() {
    let (doc, mut r) = parked();
    let budget = r.factory.memory_budget().clone();
    let active = pressure(&r);
    assert!(!r.park_warm(&doc, reduced(&doc).view, false), "a drawing parked under pressure is not kept");
    drop(active);
    budget.flush_reports();
    assert!(r.resume_retained(&doc));
    assert!(r.park_warm(&doc, reduced(&doc).view, false));
    assert!(r.keeps_warm());
    let active = pressure(&r);
    assert!(!r.keeps_warm(), "an active demand for the whole device takes the parked drawing's display");
    drop(active);
    budget.flush_reports();
    assert!(r.keeps_warm());
    assert!(r.resume_retained(&doc));
}

#[test]
fn a_revoked_warm_display_cannot_spend_disjoint_page_retirement_credit() {
    let (doc, mut r) = parked();
    assert!(r.park_warm(&doc, reduced(&doc).view, false));
    complete(&r);
    let _active = pressure(&r);
    let (charged, window) = (r.display_bytes(), r.scene_tiers.windows.used());
    assert!(r.memory.retention_grant() == 0 && window > 0 && charged >= window, "setup: a revoked display of {charged} bytes presents a {window} byte window on a {} byte grant", r.memory.retention_grant());
    let live = r.warm_bytes();
    let _retiring = r.scene_tiers.retiring.charge(2 * charged);
    assert!(!r.keeps_warm(), "pages retiring outside the display tier cover a revoked display of {charged} bytes");
    assert_eq!(r.warm_bytes(), live, "pages retiring outside the display tier change a parked drawing's size");
    let _released = [r.retired_display_bytes.charge(charged), r.source_tiles.borrow().retired_tier().charge(charged)];
    assert_eq!(r.warm_bytes(), live, "scratch and decoded tiles the display has released count as its live size");
}

#[test]
fn a_parked_drawing_keeps_the_halves_nearest_its_last_dab_and_its_next_dab_builds_none() {
    use crate::scene::scale::tests::contact::{layered_fit, named, pencil_frame, pencil_lift};
    let doc = crate::memory::memory_tests::layered_raster_document(8, 16);
    let (mut r, frame) = layered_fit(&doc);
    let layer = named(&doc, "Layer 5");
    let target = doc.scene().source_target(layer).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r, Some(layer));
    submit_settled(&mut r, frame);
    let at = [1800., 2048.];
    for step in 0..3 { pencil_frame(&mut r, frame, target, 1, at, 12., step); }
    pencil_lift(&mut r, frame, target, 1, at, 12.);
    submit_settled(&mut r, frame);
    let halves = r.scale_display.as_ref().unwrap().half_pages();
    assert!(r.park_warm(&doc, frame.view, false), "warm {} cap {}", r.warm_bytes(), r.warm_tab_bytes());
    let kept = r.scale_display.as_ref().unwrap().half_pages();
    let under = |(_, cell): &(u32, PixelRect)| cell.min_x() as f32 <= at[0] && at[0] < cell.max_x() as f32 && cell.min_y() as f32 <= at[1] && at[1] < cell.max_y() as f32;
    assert!(kept.iter().filter(|half| under(half)).count() == 2, "both halves of the last dab's cell stay warm: kept {kept:?} of {halves:?}");
    assert!(kept.len() < halves.len(), "halves far from the last dab are released: kept {} of {}", kept.len(), halves.len());
    assert!(r.resume_retained(&doc));
    let counts = pencil_frame(&mut r, frame, target, 2, at, 12., 0);
    assert_eq!(counts.fold_inputs, 0, "the first dab after resuming builds no halves: {counts:?}");
    let level = r.scale_display.as_ref().unwrap().plan.level;
    assert!(r.metrics.frame_composited_pages.contains(&(level, [at[0] as u32 / PAGE_SIZE, at[1] as u32 / PAGE_SIZE])), "the first dab after resuming shows on its first frame: {counts:?}");
}

#[test]
fn a_parked_drawings_size_excludes_display_memory_other_renderers_hold() {
    let (doc, mut r) = parked();
    assert!(r.park_warm(&doc, reduced(&doc).view, false));
    complete(&r);
    let parked = r.warm_bytes();
    let held = r.device.transient_display.charge(32 << 20);
    assert_eq!(r.warm_bytes(), parked, "a held canvas on the shared device tier is not this drawing's warm memory");
    drop(held);
}

#[test]
fn ram_pressure_takes_a_parked_drawings_decoded_samples() {
    let (doc, mut r) = parked();
    assert!(r.park_warm(&doc, reduced(&doc).view, false));
    complete(&r);
    let held = r.device.source_samples.stats().physical_bytes;
    let budget = r.factory.memory_budget().clone();
    let active = budget.register_owner(Kind::Other, MemoryClass::Visible, None);
    active.report(layer_core::MemoryReport { demand_ram_bytes: budget.ram_limit_bytes(), ..Default::default() });
    budget.flush_reports();
    assert!(!r.keeps_warm() || r.device.source_samples.stats().physical_bytes == 0,
        "an active demand for all RAM leaves {held} decoded bytes in a parked drawing that stays warm");
    drop(active);
}

#[test]
fn a_drawing_resumed_at_another_zoom_and_size_presents_what_a_cold_renderer_does() {
    let (doc, mut r) = parked();
    assert!(r.park_warm(&doc, reduced(&doc).view, false));
    assert!(r.resume_retained(&doc));
    let mut zoomed = FramePacket { composite_all: false, ..reduced(&doc) };
    zoomed.view = crate::test_support::view([640, 360]);
    zoomed.view.document_to_surface = [0.5, 0., 0., 0.5, -96., -48.];
    submit_settled(&mut r, zoomed);
    let mut cold = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { budget_bytes: Some(4 << 30), ..Default::default() }).unwrap();
    submit_settled(&mut cold, FramePacket { composite_all: true, ..zoomed });
    let pixels = |r: &WgpuRasterizer| ViewportPresenter::for_surface(r, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb).unwrap()
        .readback_rgba8(r, zoomed.view, [0.; 4]).unwrap().bytes;
    let (warm, cold) = (pixels(&r), pixels(&cold));
    assert!(cold.chunks(4).any(|pixel| pixel != &cold[..4]), "the zoomed view shows the drawing");
    let differing = warm.iter().zip(&cold).filter(|(a, b)| a != b).count();
    assert!(warm.len() == cold.len() && differing == 0, "{differing} of {} bytes differ from a cold renderer after resuming at another zoom", cold.len());
}

fn layered_4k(layers: u8) -> layer_core::Document {
    use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
    let extent=[3840,2160];
    let tiles=extent.map(|side:u32|side.div_ceil(PAGE_SIZE));
    let mut doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    for layer in 0..layers {
        let (_,layer_core::SourceTarget::Paint(paint))=crate::test_support::add_paint(&mut doc.artwork,format!("Layer {layer}"),extent) else {unreachable!()};
        let data=RasterData {tiles:(0..tiles[1]).flat_map(|y|(0..tiles[0]).map(move|x|[x,y])).map(|[x,y]| {
            let bytes=[layer.wrapping_mul(29).wrapping_add((x as u8).wrapping_mul(7)),(y as u8).wrapping_mul(13).wrapping_add(layer),((x+y) as u8).wrapping_mul(5).wrapping_add(layer),128].repeat(PAGE_SIZE.pow(2) as usize);
            (TileKey {plane:RasterPlane::Color,coordinate:[x,y]},RasterTile::backed(TileBlob::encode(doc.composition().color.paint_descriptor(),&bytes).unwrap()))
        }).collect(),watercolor:None};
        doc.artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(data);
    }
    layer_core::Document::from_artwork(doc.artwork).unwrap()
}

fn parks_a_4k_drawing_of_32_layers(total_gib: u64, [width, height]: [u32; 2], zoom: f32, [x, y]: [f32; 2]) {
    use crate::scene::scale::tests::contact::{named, pencil_frame, pencil_lift};
    let doc = layered_4k(32);
    let budget = layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts { total_bytes: Some(total_gib << 30), available_bytes: Some(total_gib << 29), tablet: true, ..Default::default() });
    let mut r = crate::memory::memory_tests::renderer_in_budget(doc.composition().color, layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }, budget).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.composite_all = false;
    (frame.view.width_px, frame.view.height_px) = (width, height);
    frame.view.document_to_surface = [zoom, 0., 0., zoom, x, y];
    let layer = named(&doc, "Layer 5");
    let target = doc.scene().source_target(layer).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r, Some(layer));
    submit_settled(&mut r, FramePacket { composite_all: true, ..frame });
    let at = [1800., 1000.];
    for step in 0..3 { pencil_frame(&mut r, frame, target, 1, at, 12., step); }
    pencil_lift(&mut r, frame, target, 1, at, 12.);
    submit_settled(&mut r, frame);
    let presented = r.presentation_views().unwrap()[0].clone();
    assert!(r.park_warm(&doc, frame.view, true), "warm {} MiB over the {} MiB cap", r.warm_bytes() >> 20, r.warm_tab_bytes() >> 20);
    complete(&r);
    assert!(r.warm_bytes() <= r.warm_tab_bytes() && r.keeps_warm(), "warm {} MiB after in-flight work landed, cap {} MiB", r.warm_bytes() >> 20, r.warm_tab_bytes() >> 20);
    assert!(r.resume_retained(&doc));
    r.poll_startup().unwrap();
    r.poll_pending(frame.view).unwrap();
    assert!(r.can_submit() && matches!(r.submit_when_ready(frame, None), std::task::Poll::Ready(Ok(_))), "the resumed drawing submits its first frame");
    assert!(r.showing_drawing() && r.presentation_views().unwrap()[0] == &presented, "the first frame after resuming presents the retained display");
}

#[test]
fn a_4k_drawing_of_32_layers_parks_within_a_4_gb_tablets_cap_and_presents_on_its_first_frame() {
    parks_a_4k_drawing_of_32_layers(4, [1920, 1200], 0.4285, [137.3, 137.2]);
}

#[test]
fn a_4k_drawing_of_32_layers_parks_within_a_12_gb_tablets_cap_and_presents_on_its_first_frame() {
    parks_a_4k_drawing_of_32_layers(12, [2880, 1800], 0.6455, [253.2, 253.64]);
}

#[test]
fn a_parked_drawing_keeps_the_next_level_only_where_its_display_samples_it() {
    for (zoom, sampled) in [(0.25, false), (0.14, true)] {
        let doc = crate::scene::scale::tests::document_at([2048, 1536]);
        let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
            layer_core::DeviceMemoryFacts { budget_bytes: Some(4 << 30), ..Default::default() }).unwrap();
        let mut frame = reduced(&doc);
        frame.view.document_to_surface = [zoom, 0., 0., zoom, 0., 0.];
        submit_settled(&mut r, frame);
        let views = r.presentation_views().unwrap();
        assert!(views[2] != views[0], "the settled display has its next level at zoom {zoom}");
        assert!(r.park_warm(&doc, frame.view, false));
        let views = r.presentation_views().unwrap();
        assert_eq!(views[2] != views[0], sampled, "a parked display at zoom {zoom} keeps the next level only when it samples it");
    }
}

#[test]
fn a_parked_drawing_with_the_navigator_shown_resumes_with_its_navigator_image_on_the_first_frame() {
    for shown in [true, false] {
        let (doc, mut r) = parked();
        let image = r.navigator.view().cloned().expect("the settled drawing has a Navigator image");
        assert!(r.park_warm(&doc, reduced(&doc).view, shown));
        complete(&r);
        assert_eq!(r.navigator.view().is_some(), shown, "a parked drawing keeps its Navigator image only while the Navigator is shown");
        assert!(r.resume_retained(&doc));
        r.poll_startup().unwrap();
        r.poll_pending(reduced(&doc).view).unwrap();
        let frame = FramePacket { composite_all: false, ..reduced(&doc) };
        assert!(r.can_submit() && matches!(r.submit_when_ready(frame, None), std::task::Poll::Ready(Ok(_))), "the resumed drawing submits its first frame");
        if shown { assert_eq!(r.navigator.view(), Some(&image), "the first frame after resuming shows the parked Navigator image"); }
    }
}

fn resumed_at_a_zoom_that_samples_the_next_level(park: bool) -> (usize, bool) {
    let (doc, mut r) = parked();
    let level = r.scale_display.as_ref().unwrap().plan.level;
    if park {
        assert!(r.park_warm(&doc, reduced(&doc).view, false));
        let views = r.presentation_views().unwrap();
        assert!(views[2] == views[0], "setup: zoom 0.25 parks without the next level");
        assert!(r.resume_retained(&doc));
    }
    let mut zoomed = FramePacket { composite_all: false, ..reduced(&doc) };
    zoomed.view.document_to_surface = [0.14, 0., 0., 0.14, 0., 0.];
    submit_settled(&mut r, zoomed);
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level, level, "setup: zoom 0.14 keeps the parked level");
    let views = r.presentation_views().unwrap();
    let next = views[2] != views[0];
    let mut cold = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { budget_bytes: Some(4 << 30), ..Default::default() }).unwrap();
    submit_settled(&mut cold, FramePacket { composite_all: true, ..zoomed });
    let pixels = |r: &WgpuRasterizer| ViewportPresenter::for_surface(r, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb).unwrap()
        .readback_rgba8(r, zoomed.view, [0.; 4]).unwrap().bytes;
    let (warm, cold) = (pixels(&r), pixels(&cold));
    (warm.iter().zip(&cold).filter(|(a, b)| a != b).count(), next)
}

#[test]
fn a_drawing_resumed_where_its_display_samples_the_released_next_level_presents_what_a_cold_renderer_does() {
    let control = resumed_at_a_zoom_that_samples_the_next_level(false);
    let resumed = resumed_at_a_zoom_that_samples_the_next_level(true);
    assert_eq!(control, (0, true), "setup: an active drawing zoomed to 0.14 matches a cold renderer");
    assert_eq!(resumed, (0, true), "a drawing resumed at a zoom whose display samples the next level released at park never rebuilds it, and differs from a cold renderer (differing bytes, next level rebuilt)");
}

#[test]
fn the_first_dab_after_resuming_far_from_the_last_one_shows_within_three_frames() {
    use crate::scene::scale::tests::contact::{layered_fit, named, pencil_frame, pencil_lift};
    let doc = crate::memory::memory_tests::layered_raster_document(8, 16);
    let (_, frame) = layered_fit(&doc);
    let budget = layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts { total_bytes: Some(12 << 30), available_bytes: Some(6 << 30), tablet: true, ..Default::default() });
    let mut r = crate::memory::memory_tests::renderer_in_budget(doc.composition().color, layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }, budget).unwrap();
    let layer = named(&doc, "Layer 5");
    let target = doc.scene().source_target(layer).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r, Some(layer));
    submit_settled(&mut r, frame);
    let at = [3800., 3800.];
    for step in 0..3 { pencil_frame(&mut r, frame, target, 1, at, 12., step); }
    pencil_lift(&mut r, frame, target, 1, at, 12.);
    submit_settled(&mut r, frame);
    assert!(r.park_warm(&doc, frame.view, false), "setup: warm {} MiB cap {} MiB", r.warm_bytes() >> 20, r.warm_tab_bytes() >> 20);
    let far = [200., 200.];
    let covers = |(_, cell): &(u32, PixelRect)| cell.min_x() as f32 <= far[0] && far[0] < cell.max_x() as f32 && cell.min_y() as f32 <= far[1] && far[1] < cell.max_y() as f32;
    assert!(!r.scale_display.as_ref().unwrap().half_pages().iter().any(covers), "setup: the far cell's halves are released at park");
    assert!(r.resume_retained(&doc));
    let page = (r.scale_display.as_ref().unwrap().plan.level, [far[0] as u32 / PAGE_SIZE, far[1] as u32 / PAGE_SIZE]);
    let shown = (0..3).position(|step| {
        pencil_frame(&mut r, frame, target, 2, far, 12., step);
        let composited = r.metrics.frame_composited_pages.contains(&page);
        crate::test_support::settle_decodes(&mut r);
        composited
    });
    assert!(shown.is_some(), "the first dab far from the parked last dab shows within three frames once each frame's decodes have arrived");
}
