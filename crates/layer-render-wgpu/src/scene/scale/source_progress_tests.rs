use super::*;
use layer_render::ViewState;

pub(crate) const EXTENT: [u32; 2] = [2560, 2048];

pub(crate) fn color(x: u32, y: u32, seed: u8) -> [u8; 4] {
    [20 + seed + (x / PAGE_SIZE) as u8 * 13, 30 + (y / PAGE_SIZE) as u8 * 11,
        40 + seed + ((x / PAGE_SIZE + 10 * (y / PAGE_SIZE)) % 160) as u8, 255]
}

pub(crate) fn photo(seed: u8) -> Document {
    let mut doc = Document::new(PortableId::random(), EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Cache progress".into(), paper: "Paper".into() });
    let ink = doc.scene().order()[0];
    set_root_entries(&mut doc, vec![ink]);
    composition_mut(&mut doc).color.depth = SampleDepth::F32;
    composition_mut(&mut doc).blend = layer_core::BlendSpace::Linear;
    paint_mut(&mut doc, 0).base = Some(PaintBase::new(rgba8_source(EXTENT, |x, y| color(x, y, seed)).into()));
    doc
}

fn present(r: &WgpuRasterizer, presenter: &mut ViewportPresenter, view: ViewState) -> Vec<[f32; 4]> {
    let (texture, target) = create_target(&r.device, [view.width_px, view.height_px], wgpu::TextureFormat::Rgba32Float, "source progress presentation");
    presenter.present(r, &target, view, [0.; 4]).unwrap();
    pixels(r, &texture)
}

#[test]
fn value_equal_sources_with_distinct_storage_require_published_frame_backup() {
    let doc = document_at([4; 2]);
    let base = paint_at(&doc, 0).base.as_ref().unwrap();
    let storage = Arc::new((**base.image.storage()).clone());
    assert_eq!(storage.as_ref(), base.image.storage().as_ref());
    assert!(!Arc::ptr_eq(&storage, base.image.storage()));
    let frame = crate::artwork::Frame::new(packet(doc.scene(), [4; 2]), Default::default());
    assert!(!frame.paint_sources_changed(doc.scene()));
    let mut replacement = doc.clone();
    let mut replaced = base.clone();
    replaced.image = layer_core::authored::Image::with_id(base.image.id(), storage);
    assert_eq!(&replaced, base);
    paint_mut(&mut replacement, 0).base = Some(replaced);
    assert!(frame.paint_sources_changed(replacement.scene()));
}

#[test]
fn reduced_photo_larger_than_decoded_cache_publishes_only_after_prefixes_finish() {
    let doc = photo(0);
    let mut previous = doc.clone();
    paint_mut(&mut previous, 0).base = Some(PaintBase::new(rgba8_source([2048; 2], |x, y| color(x, y, 27)).into()));
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    assert_ne!(r.adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    r.source_tiles.get_mut().admit(0);
    assert_eq!(r.source_tiles.borrow().admitted_bytes()[0], 64 * u64::from(PAGE_SIZE).pow(2) * 16);
    let view = ViewState { width_px: 320, height_px: 256, document_to_surface: [0.125, 0., 0., 0.125, 0., 0.] };
    crate::test_support::submit_settled(&mut r, FramePacket { view, blend_space: layer_core::BlendSpace::Linear, ..packet(previous.scene(), EXTENT) });
    let mut presenter = crate::test_support::float_presenter(&r);
    present(&r, &mut presenter, view);
    let published = present(&r, &mut presenter, view);
    let presented_storage = |r: &WgpuRasterizer| r.presentation_views().map(|views|
        views.map(|view| (view.texture().format(), view.texture().size(), view.texture().mip_level_count())));
    let previous_storage = presented_storage(&r);
    let target = source_at(&doc, 0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut prefix = 0;
    let mut partial = false;
    let mut settled = false;
    let mut presentation_change = None;
    for step in 0..256 {
        assert!(std::time::Instant::now() < deadline, "photo progress timed out at {prefix}/80 pages");
        r.poll_pending(view).unwrap();
        let frame = FramePacket { view, composite_all: step == 0, blend_space: layer_core::BlendSpace::Linear, ..packet(doc.scene().with_owner(doc.owner, 1), EXTENT) };
        if r.can_submit() && r.raster_dependencies_ready(frame, None) { crate::test_support::submit_ready(&mut r, frame).unwrap(); }
        r.wait_idle().unwrap();
        if let Some(source) = r.scene.as_ref().and_then(|scene| scene.scale_sources.entries.get(&target)) {
            let completed = source.levels.values().map(|level| level.valid.len()).max().unwrap_or(0);
            assert!(completed >= prefix, "completed source pages regressed");
            prefix = completed;
            partial |= prefix > 0 && prefix < 80;
        }
        if r.object_deferred {
            let shown = present(&r, &mut presenter, view);
            assert_eq!(shown.len(), published.len());
            if presentation_change.is_none() && let Some((index, (actual, expected))) = shown.iter().zip(&published).enumerate().find(|(_, (a, b))| a != b) {
                eprintln!("retained presentation storage: before={previous_storage:?} pending={:?}", presented_storage(&r));
                presentation_change = Some((step, prefix, index, *actual, *expected));
            }
        }
        if !r.has_pending_work() && !r.has_pending_submission() { settled = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let levels = r.scene.as_ref().and_then(|scene| scene.scale_sources.entries.get(&target)).map(|source|
        source.levels.iter().map(|(level, cached)| (*level, cached.valid.len())).collect::<Vec<_>>());
    eprintln!("progress display level={:?} size={:?} source_levels={levels:?}", r.scale_display.as_ref().map(|cache| cache.plan.level), r.scale_display.as_ref().map(|cache| cache.plan.size));
    assert!(settled && partial && prefix == 80, "source prefixes must finish: settled={settled} partial={partial} pages={prefix}");
    eprintln!("source progress: settled={settled} partial={partial} pages={prefix} pending_presentation_change={presentation_change:?}");
    let actual = display_pixels(&r);
    let plan = r.scale_display.as_ref().unwrap().plan;
    assert_eq!((plan.level, plan.size), (3, [320, 256]));
    assert_eq!(actual.len(), 320 * 256);
    for (index, pixel) in actual.iter().enumerate() {
        let rgba = color((index as u32 % 320) * 8, (index as u32 / 320) * 8, 0);
        for c in 0..4 {
            let expected = if c == 3 { 1. } else { doc.composition().color.space.decode(f64::from(rgba[c]) / 255.) as f32 };
            assert!((pixel[c] - expected).abs() < 2e-6, "reduced pixel {index} channel {c}: {pixel:?} vs {expected}");
        }
    }
    assert!(presentation_change.is_none(), "pending source preparation replaced the published frame: {presentation_change:?}");
}
