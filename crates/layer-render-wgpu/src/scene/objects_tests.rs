use super::*;
use crate::test_support::{packet, float_pixels};
use layer_core::authored::*;
use layer_core::authored::Image as AuthoredImage;
use crate::scene::Output;
use layer_core::color::{SampleDepth, source::*};

fn image(extent: [u32; 2], color: [u8; 4]) -> AuthoredImage {
    let mut builder = SourceBuilder::new(extent, SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 16 * 1024 * 1024).unwrap();
    for _ in 0..extent[1] { builder.push_row(&color.repeat(extent[0] as usize)).unwrap(); }
    AuthoredImage::new(Arc::new(builder.finish().unwrap()))
}

fn document(extent: [u32; 2], objects: Vec<ImageObject>) -> (layer_core::Document, OccurrenceHandle, Vec<ImageObjectHandle>) {
    let mut artwork = Artwork::new(extent).unwrap();
    let handles: Vec<_> = objects.into_iter().map(|object| artwork.objects.insert(PortableId::random(), object).unwrap()).collect();
    let collection = artwork.object_layers.insert(PortableId::random(), ObjectLayer { children: handles.clone() }).unwrap();
    let owner = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Objects(collection), "Images")).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(owner);
    (layer_core::Document::from_artwork(artwork).unwrap(), owner, handles)
}

fn native(r: &mut WgpuRasterizer, doc: &layer_core::Document, output: Output) -> Vec<[f32; 4]> {
    let extent = doc.composition().size;
    let (texture, _) = create_color_target(&r.device, extent, "object native reference");
    let mut scene = r.scene.take().unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for _ in 0..5000 {
        let result=scene.capture_region_prepared(r,packet(doc.scene(),extent),&texture,PixelRect::full(extent),output,&mut encoder);
        match result {Ok(())=>break,Err(GpuRasterError::DeferredObjectWork)=>{},Err(error)=>panic!("{error}")}
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    }
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    r.scene = Some(scene);
    float_pixels(r, &texture)
}

fn submit_ready(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    r.submit(frame).unwrap();
    for _ in 0..2000 {
        r.wait_idle().unwrap();
        if !r.has_pending_work() { return; }
        std::thread::sleep(std::time::Duration::from_millis(1));
        r.submit(FramePacket { composite_all: false, reset_layers: false, ..frame }).unwrap();
    }
    panic!("object display preparation must settle");
}

fn canonical_request(r: &WgpuRasterizer, scene: &mut Scene, doc: &layer_core::Document, owner: OccurrenceHandle, window: ObjectWindow) -> super::objects::CollectionJob {
    let content = scene.object_spatial.content(doc.scene(), owner).unwrap();
    scene.collection_job(r, packet(doc.scene(), doc.composition().size), owner, content, window)
}

#[test]
fn object_only_native_and_display_composite_ordered_children_and_raw_queries() {
    let mut front = ImageObject::new(image([8; 2], [0, 0, 255, 128]), "Blue");
    front.affine = Affine64([1., 0., 0., 1., 4., 4.]);
    let back = ImageObject::new(image([8; 2], [255, 0, 0, 255]), "Red");
    let (mut doc, owner, _) = document([16; 2], vec![front, back]);
    assert!(doc.scene().source_target(owner).is_none());
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
    let native_pixels = native(&mut r, &doc, Output::Artwork(None));
    let display = r.scale_display.as_ref().unwrap().texture().clone();
    assert!(crate::test_support::max_error(&native_pixels, &float_pixels(&r, &display)) < 2e-5);
    let alpha = 128. / 255.;
    assert!(crate::test_support::max_error(&native_pixels[5 * 16 + 5..5 * 16 + 6], &[[1. - alpha, 0., alpha, 1.]]) < 2e-5);
    let coverage = doc.artwork.coverage.next_handle();
    let mut mask = layer_core::CoverageSnapshot::reveal_all(coverage, [16; 2], Default::default());
    mask.source.default_coverage = 0.5;
    doc.artwork.coverage.insert(PortableId::random(), mask.source).unwrap();
    let occurrence = doc.artwork.occurrences.get_mut(owner).unwrap();
    occurrence.opacity = 0.25; occurrence.mask = Some(mask.use_); occurrence.visible = false;
    let scoped = SceneScope::RawObjects(owner);
    let raw_frame = packet(doc.scene().with_scope(&scoped), doc.composition().size);
    let (target, _) = create_color_target(&r.device, [16; 2], "raw hidden images");
    let mut scene = r.scene.take().unwrap();
    let mut captured = false;
    for _ in 0..5000 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = scene.capture_region_prepared(&mut r, raw_frame, &target, PixelRect::full([16; 2]), Output::Objects(owner), &mut encoder);
        if let Err(error) = &result { assert!(matches!(error, GpuRasterError::DeferredObjectWork), "{error:?}"); }
        r.uploads.finish(&encoder); encoder.submit(&r.queue); r.wait_idle().unwrap();
        if result.is_ok() { captured = true; break; }
    }
    r.scene = Some(scene);
    assert!(captured, "hidden raw images complete through the deferred query path");
    assert!(crate::test_support::max_error(&native_pixels, &float_pixels(&r, &target)) < 2e-5);
}

#[test]
fn object_affine_preflight_rejects_unsupported_sampling_without_changing_the_scene() {
    let (doc, _, children) = document([64; 2], vec![ImageObject::new(image([8; 2], [255; 4]), "Image")]);
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let view = packet(doc.scene(), [64; 2]).view;
    let before = doc.artwork.clone();
    let unsupported = Affine64([0.0001, 0., 0., 0.0001, 0., 0.]);
    assert!(r.preflight_image_object_affine(doc.scene(), children[0], unsupported, view).is_err());
    assert_eq!(doc.artwork, before);
    let far = Affine64([1., 0., 0., 1., 1e15, -1e15]);
    r.preflight_image_object_affine(doc.scene(), children[0], far, view).unwrap();
}

#[test]
fn object_damage_excludes_unchanged_children() {
    let shared = image([8; 2], [255; 4]);
    let mut distant = ImageObject::new(shared.clone(), "Unchanged"); distant.affine.0[4] = 600.;
    let (mut doc, _, children) = document([768, 256], vec![ImageObject::new(shared, "Moving"), distant]);
    let before = doc.scene().snapshot(layer_core::EvaluationContext::default());
    doc.artwork.objects.get_mut(children[0]).unwrap().affine.0[4] = 32.;
    let damage = edited_damage(before.view(), doc.scene(), [768, 256]).unwrap();
    assert!(!damage.is_empty());
    assert!(damage.regions.iter().all(|region| region.max_x() < 100), "unchanged child bounds remain reusable");
}

#[test]
fn affine_edits_clear_vacated_bounds_and_keep_shared_source_residency() {
    let shared = image([16; 2], [255, 0, 0, 255]);
    let mut second = ImageObject::new(shared.clone(), "Second");
    second.affine.0[4] = 48.;
    let (mut doc, _, children) = document([768, 256], vec![ImageObject::new(shared, "First"), second]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
    let misses = r.source_cache_work()[1];
    for position in [96., 144., 192., 0.] {
        doc.artwork.objects.get_mut(children[0]).unwrap().affine.0[4] = position;
        let work = r.metrics.composited_pixels;
        submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
        assert!(r.metrics.composited_pixels - work < 768 * 256, "object movement does not replay the full frame");
        assert_eq!(r.source_cache_work()[1], misses, "affine edits share immutable decoded source tiles");
        let expected = native(&mut r, &doc, Output::Artwork(None));
        let actual = float_pixels(&r, &r.scale_display.as_ref().unwrap().texture().clone());
        assert!(crate::test_support::max_error(&actual, &expected) < 2e-5, "old and new object bounds repaint");
    }
}

#[test]
fn object_read_mapping_preserves_f64_relative_coordinates_and_requested_density() {
    let mut object = ImageObject::new(image([2; 2], [255, 0, 0, 255]), "Far image");
    object.affine.0[4] = 1_048_576.125;
    let (doc, owner, _) = document([16; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [16; 2];
    let mut scene = Scene::new(&r);
    for side in [0.25, 0.5, 1., 2.] {
        let request = canonical_request(&r, &mut scene, &doc, owner, ObjectWindow { origin: [1_048_576.125, 0.], side, size: [8; 2] });
        let mut cache = super::super::object_cache::ObjectCache::default();
        let view = loop {
            if let Some(view) = cache.resolve_collection(&r, doc.scene(), &request).unwrap() { break view; }
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            cache.advance(&mut r, &mut scene, &mut encoder, true).unwrap();
            r.uploads.finish(&encoder); encoder.submit(&r.queue); r.wait_idle().unwrap();
        };
        let pixels = float_pixels(&r, view.texture());
        assert!(pixels[0][0] > 0. && pixels[0][3] > 0.);
        assert!(pixels.iter().all(|pixel| pixel.iter().all(|v| v.is_finite())));
    }
}
#[test]
fn private_canonical_jobs_yield_and_cancel_before_publication() {
    let mut object = ImageObject::new(image([512; 2], [255, 0, 0, 255]), "Reduced image");
    object.affine = Affine64([1. / 16., 0., 0., 1. / 16., 0., 0.]);
    let (mut doc, owner, children) = document([64; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [64; 2];
    let mut compositor=Scene::new(&r);
    let mut cache = super::super::object_cache::ObjectCache::default();
    let request = canonical_request(&r, &mut compositor, &doc, owner, ObjectWindow { origin: [0.; 2], side: 1., size: [16; 2] });
    assert!(cache.resolve_collection(&r,doc.scene(),&request).unwrap().is_none());
    let mut complete = None;
    for _ in 0..300 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        r.prepare_moving_images(packet(doc.scene(), [64; 2]), &mut encoder).unwrap();
        let passes = encoder.pass_count();
        let taps = cache.advance(&mut r, &mut compositor, &mut encoder, true).unwrap();
        let limit = super::super::object_cache::INTERACTIVE_TAPS + crate::object_sampling::DISPATCH_TAPS;
        assert!(taps <= limit && (encoder.pass_count() - passes) * super::super::object_cache::PASS_TAPS <= limit, "canonical refinement has a bounded per-frame workload");
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        if !cache.pending() {
            complete = cache.resolve_collection(&r,doc.scene(),&request).unwrap();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let view = complete.expect("canonical work completes across bounded frames");
    let pixels = float_pixels(&r, view.texture());
    assert!(pixels[8 * 16 + 8][0] > 0.999);
    assert!(cache.bytes() < 64 * 1024);
    let reused = cache.resolve_collection(&r,doc.scene(),&request).unwrap().unwrap();
    assert_eq!(view, reused, "multiple consumers share the completed result within one submission");
    assert!(!cache.pending());
    let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    cache.flush_retired(&encoder); r.uploads.finish(&encoder); r.last_submission = Some(encoder.submit(&r.queue)); r.wait_idle().unwrap();
    assert!(cache.resolve_collection(&r,doc.scene(),&request).unwrap().is_none(), "a query cache releases encoded results");
    doc.artwork.objects.get_mut(children[0]).unwrap().affine.0[4] = 32.;
    cache.retain(doc.scene(),layer_core::BlendSpace::Linear,r.device.working_space());
    assert!(!cache.pending(), "obsolete affine work cannot publish");
    assert!(cache.bytes() < 64 * 1024);
}
#[test]
fn object_masks_attached_effects_and_clipping_follow_both_stack_evaluators() {
    let (mut doc, owner, _) = document([64; 2], vec![ImageObject::new(image([16; 2], [255, 0, 0, 255]), "Base")]);
    let mut clip_image = ImageObject::new(image([16; 2], [0, 0, 255, 128]), "Clip");
    clip_image.affine.0[4] = 8.; clip_image.affine.0[5] = 8.;
    let child = doc.artwork.objects.insert(PortableId::random(), clip_image).unwrap();
    let collection = doc.artwork.object_layers.insert(PortableId::random(), ObjectLayer { children: vec![child] }).unwrap();
    let mut clip = Occurrence::new(OccurrenceContent::Objects(collection), "Clipped image"); clip.attachment = layer_core::Attachment::Clip;
    let clip = doc.artwork.occurrences.insert(PortableId::random(), clip).unwrap();
    let instance = layer_core::EffectInstance::new(crate::tests::fixture("invert").program());
    let application = doc.artwork.effects.insert(PortableId::random(), EffectApplication::new(instance.program, instance.values, [64; 2])).unwrap();
    let mut effect = Occurrence::new(OccurrenceContent::Effect(application), "Invert"); effect.attachment = layer_core::Attachment::Effect;
    let effect = doc.artwork.occurrences.insert(PortableId::random(), effect).unwrap();
    let coverage = doc.artwork.coverage.next_handle();
    let mut mask = layer_core::CoverageSnapshot::reveal_all(coverage, [64; 2], Default::default()); mask.source.default_coverage = 0.5;
    doc.artwork.coverage.insert(PortableId::random(), mask.source).unwrap();
    doc.artwork.occurrences.get_mut(owner).unwrap().mask = Some(mask.use_);
    doc.artwork.occurrences.get_mut(owner).unwrap().opacity = 0.25;
    let stack = doc.composition().result;
    doc.artwork.stacks.get_mut(stack).unwrap().entries = vec![clip, effect, owner];
    let doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
    let expected = native(&mut r, &doc, Output::Artwork(None));
    let actual = float_pixels(&r, &r.scale_display.as_ref().unwrap().texture().clone());
    assert!(crate::test_support::max_error(&actual, &expected) < 2e-5);
    let pixel = actual[10 * 64 + 10];
    assert!((pixel[3] - 0.125).abs() < 2e-5, "outer properties apply once: {pixel:?}");
    assert!(pixel[0] < 2e-5 && pixel[2] > 0.12, "local effect precedes clipping: {pixel:?}");
    assert_eq!(actual[20 * 64 + 20][3], 0., "clipped image cannot expand the base shape");
}

#[test]
fn coarse_linear_object_node_includes_source_outside_its_output_cell() {
    let mut object = ImageObject::new(image([8; 2], [255, 0, 0, 255]), "Thin reduced image");
    object.affine.0[4] = 200.; object.affine.0[5] = 200.;
    let (doc, _, _) = document([8192; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), [8192; 2]);
    frame.view.width_px = 1; frame.view.height_px = 1;
    frame.view.document_to_surface = [1. / 256., 0., 0., 1. / 256., -1., -1.];
    let window = PixelRect::new(256, 256, 512, 512);
    r.document_extent = [8192; 2];
    r.scale_display = Some(super::super::scale::Cache::new(&r, super::super::scale::Request {
        plan: display_mips::Plan::window([8192; 2], 8, window), evaluation: super::super::scale::Evaluation::Display,
    }, 1));
    let mut scene = r.scene.take().unwrap();
    let mut settled = false;
    for iteration in 0..2000 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        if r.prepare_moving_images(frame, &mut encoder).unwrap() { scene.invalidate_object_previews([8192; 2]); }
        let composed = scene.compose(&mut r, frame, &[], if iteration == 0 { window } else { PixelRect::EMPTY }, &mut encoder, None);
        if let Err(error) = &composed { assert!(matches!(error, GpuRasterError::DeferredObjectWork), "{error:?}"); }
        r.uploads.finish(&encoder); r.last_submission = Some(encoder.submit(&r.queue)); r.wait_idle().unwrap();
        if composed.is_ok() && !scene.objects_pending() && !r.moving_images.pending() && !r.image_decode_waiting() { settled = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(settled);
    r.scene = Some(scene);
    let cache = r.scale_display.as_ref().unwrap();
    assert_eq!(cache.plan.level, 8);
    assert!(cache.evaluation == super::super::scale::Evaluation::Display);
    let pixels = float_pixels(&r, cache.texture());
    assert_eq!(cache.plan.size, [1; 2]);
    let index = 0;
    let weight: f64 = (0..8).map(|i| 1. - ((f64::from(i) + 0.5 - 184.).abs() / 256.)).sum();
    let expected = (weight * weight / (256. * 256.)) as f32;
    assert!(expected > 0.);
    assert!((pixels[index][0] - expected).abs() < 2e-7, "coarse tent includes pixels beyond the allocation cell: {:?}, {expected}", pixels[index]);
    assert!((pixels[index][3] - expected).abs() < 2e-7);
}

#[test]
fn nearest_reduced_object_display_keeps_level_zero_aliases_after_idle() {
    let mut builder = SourceBuilder::new([256; 2], SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 1024 * 1024).unwrap();
    for y in 0..256 { builder.push_row(&(0..256).flat_map(|x| {
        let value = if (x + y) % 2 == 0 { 255 } else { 0 }; [value, value, value, 255]
    }).collect::<Vec<_>>()).unwrap(); }
    let mut object = ImageObject::new(AuthoredImage::new(Arc::new(builder.finish().unwrap())), "Nearest checker");
    object.interpolation = ImageInterpolation::Nearest;
    object.affine = Affine64([0.25, 0., 0., 0.25, 0., 0.]);
    let (doc, _, _) = document([64; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), [64; 2]);
    frame.view.width_px = 8; frame.view.height_px = 8;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_ready(&mut r, frame);
    for _ in 0..3 { r.submit(FramePacket { composite_all: false, ..frame }).unwrap(); }
    let cache = r.scale_display.as_ref().unwrap();
    let pixels = float_pixels(&r, cache.texture());
    assert!(pixels.iter().all(|pixel| pixel[..3] == [1.; 3] && pixel[3] == 1.), "Nearest never uses averaged source or native-result levels");
    assert!(!r.has_pending_work());
}

#[test]
fn simultaneous_photo_results_bound_batches_and_survive_deferred_retries() {
    let shared = image([4000, 3000], [255, 0, 0, 255]);
    let mut objects = vec![ImageObject::new(shared.clone(), "Shared one"), ImageObject::new(shared, "Shared two"),
        ImageObject::new(image([4000, 3000], [0, 255, 0, 255]), "Green"),
        ImageObject::new(image([4000, 3000], [0, 0, 255, 255]), "Blue")];
    for object in &mut objects { object.affine = Affine64([1. / 7., 0., 0., 1. / 7., 0., 0.]); }
    let (doc, owner, _) = document([768; 2], objects);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [768; 2];
    let mut compositor=Scene::new(&r);
    let mut cache = super::super::object_cache::ObjectCache::default();
    let requests: Vec<_> = [[0., 0.], [256., 0.], [0., 256.], [256., 256.]].into_iter()
        .map(|origin| canonical_request(&r, &mut compositor, &doc, owner, ObjectWindow { origin, side: 1., size: [256; 2] })).collect();
    for request in &requests { assert!(cache.resolve_collection(&r, doc.scene(), request).unwrap().is_none()); }
    let mut complete = false;
    for _ in 0..4000 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        r.drain_image_decode(&mut encoder).unwrap();
        let before = encoder.pass_count();
        cache.advance(&mut r, &mut compositor, &mut encoder, true).unwrap();
        let passes = encoder.pass_count() - before;
        cache.advance(&mut r, &mut compositor, &mut encoder, true).unwrap();
        assert_eq!(encoder.pass_count() - before, passes, "a cache has only one batch in flight");
        assert!(passes <= 24, "sampling and source preparation remain bounded: {passes}");
        assert!(cache.bytes() <= 64 * 1024 * 1024);
        if let Some(report) = r.device.generate_allocator_report() {
            assert!(report.allocations.iter().filter(|allocation| allocation.name == "image-object sampling parameters").count() <= 1);
        }
        r.uploads.finish(&encoder); r.last_submission = Some(encoder.submit(&r.queue)); r.wait_idle().unwrap();
        if !cache.pending() { complete = true; break; }
    }
    assert!(complete, "all four private results make bounded progress");
    let views: Vec<_> = requests.iter().map(|request| cache.resolve_collection(&r, doc.scene(), request).unwrap().unwrap()).collect();
    cache.reset_used();
    for (request, view) in requests.iter().zip(&views) {
        assert_eq!(&cache.resolve_collection(&r, doc.scene(), request).unwrap().unwrap(), view, "a deferred frame retains every completed result");
    }
    let (destination, _) = create_color_target(&r.device, [256; 2], "retained object result consumer");
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for view in &views { encoder.copy_texture_to_texture(view.texture().as_image_copy(), destination.as_image_copy(),
        wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 }); }
    cache.flush_retired(&encoder);
    assert!(cache.bytes() >= 4 * 256 * 256 * 16, "encoded consumers retain charged results through completion");
    r.uploads.finish(&encoder); r.last_submission = Some(encoder.submit(&r.queue)); r.wait_idle().unwrap();
    assert!(cache.bytes() < 64 * 1024);
}
#[test]
fn tablet_fit_photo_scene_completes_many_private_windows_within_cache_budget() {
    let shared = image([4000, 3000], [255, 0, 0, 255]);
    let mut objects = vec![ImageObject::new(shared.clone(), "Shared one"), ImageObject::new(shared, "Shared two"),
        ImageObject::new(image([4000, 3000], [0, 255, 0, 255]), "Green"),
        ImageObject::new(image([4000, 3000], [0, 0, 255, 255]), "Blue")];
    for object in &mut objects { object.affine = Affine64([0.55, 0., 0., 0.55, 80., 80.]); object.interpolation = ImageInterpolation::Nearest; }
    let (doc, _, _) = document([4248, 2832], objects);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), [4248, 2832]);
    frame.view.width_px = 1440; frame.view.height_px = 960;
    let scale = 1440. / 4248.; frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
    let mut ready = false; let mut peak = 0;
    for iteration in 0..10000 {
        r.submit(FramePacket { composite_all: iteration == 0, reset_layers: iteration == 0, ..frame }).unwrap();
        let scene = r.scene.as_ref().unwrap();
        let bytes = scene.object_results.bytes(); peak = peak.max(bytes);
        assert!(bytes <= 64 * 1024 * 1024, "complete and pending windows share one cache allowance: {bytes}");
        r.wait_idle().unwrap();
        if !r.has_pending_work() { ready = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(ready, "a photo viewport must progress beyond its first 64 completed object windows");
    let display = r.scale_display.as_ref().unwrap();
    assert!(display.plan.level > 0);
    let pixels = float_pixels(&r, display.texture());
    let x = 400 >> display.plan.level; let y = 400 >> display.plan.level;
    let pixel = pixels[(y * display.plan.size[0] + x) as usize];
    assert!(pixel[0] > 0.999 && pixel[3] > 0.999, "ordered shared front images publish a complete pose: {pixel:?}");
    eprintln!("tablet Fit private-cache peak={peak}");
}

#[test]
fn tablet_fit_shared_photos_with_paint_above_remain_admitted_during_strokes() {
    let extent = [4248, 2832];
    let shared = image(extent, [180, 90, 40, 255]);
    let objects = [[955.8, 863.76], [1295.64, 637.2], [615.96, 637.2], [955.8, 410.64]].into_iter().map(|[x,y]| {
        let mut object = ImageObject::new(shared.clone(), "Photo");
        object.affine = Affine64([0.55, 0., 0., 0.55, x, y]); object
    }).collect();
    let (mut doc, owner, _) = document(extent, objects);
    doc.artwork.occurrences.get_mut(owner).unwrap().opacity = 0.35;
    let (_, base) = crate::test_support::add_paint(&mut doc.artwork, "Original", extent);
    let SourceTarget::Paint(base) = base else { unreachable!() };
    doc.artwork.paint.get_mut(base).unwrap().base = Some(PaintBase { image: shared, offset: [0;2], policy: PaintBasePolicy::SourceProfile });
    let (paint, target) = crate::test_support::add_paint(&mut doc.artwork, "Paint", extent);
    let stack = doc.composition().result;
    let entries = &mut doc.artwork.stacks.get_mut(stack).unwrap().entries;
    entries.retain(|h|*h != paint); entries.insert(0,paint);
    let doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let zoom = 0.15970339;
    let view = layer_render::ViewState { width_px:1920,height_px:1200,document_to_surface:[zoom,0.,0.,zoom,670.,260.] };
    let frame = FramePacket { view, composite_all:false,..packet(doc.scene(),extent) };
    r.submit(FramePacket { reset_layers:true,composite_all:true,..frame }).unwrap();
    let mut ready = false;
    let mut scratch_peak = 0;
    for _ in 0..50000 {
        r.wait_idle().unwrap();
        scratch_peak = scratch_peak.max(r.scene.as_ref().unwrap().pool.len());
        assert!(scratch_peak <= 64,"cold retry scratch stays bounded: {scratch_peak} tiles");
        if !r.has_pending_work() { ready = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
        r.submit(frame).unwrap();
    }
    assert!(ready,"shared photo cold preparation settles");
    let mut ink = crate::layer_tests::dab([0.2,0.6,0.9,1.]);
    ink.radii = [512.;2]; ink.contact = [1.,0.,0.,0.];
    let style = crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
    for step in 0..160 {
        ink.center = layer_core::Point {x:2100.+(step as f32*0.1).cos()*1200.,y:1400.+(step as f32*0.1).sin()*800.};
        let mut batch = crate::test_support::dab_batch(target,style.clone(),ink.bounds());
        batch.stroke_start = step == 0; batch.stroke_end = step ==159;
        r.submit(FramePacket {dabs:std::slice::from_ref(&ink),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        r.wait_idle().unwrap();
        assert!(r.scene.as_ref().unwrap().object_results.bytes() <= 64*1024*1024);
        scratch_peak = scratch_peak.max(r.scene.as_ref().unwrap().pool.len());
        assert!(scratch_peak <= 64,"stroke scratch stays bounded: {scratch_peak} tiles");
    }
    ready = false;
    let start = std::time::Instant::now();
    let mut checkpoint = start;
    let mut previous = None;
    while start.elapsed() < std::time::Duration::from_secs(120) {
        r.submit(frame).unwrap(); r.wait_idle().unwrap();
        let scene = r.scene.as_ref().unwrap();
        assert!(scene.object_results.bytes() <= 64*1024*1024);
        scratch_peak = scratch_peak.max(scene.pool.len());
        assert!(scratch_peak <= 64);
        if !r.has_pending_work() { ready = true; break; }
        if checkpoint.elapsed() >= std::time::Duration::from_secs(5) {
            let progress = (scene.object_results.work_progress(),r.metrics.composited_pixels);
            assert_ne!(previous,Some(progress),"canonical refinement must advance: {progress:?}");
            eprintln!("post-stroke elapsed={:?} progress={progress:?}",start.elapsed());
            previous = Some(progress); checkpoint = std::time::Instant::now();
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(ready,"shared photo post-stroke refinement settles");
    eprintln!("shared photo stroke scratch peak={scratch_peak} tiles; settle={:?}",start.elapsed());
}

#[test]
fn live_object_read_defers_without_inline_decode_then_returns_canonical_pixels() {
    let mut object = ImageObject::new(image([512; 2], [255, 0, 0, 255]), "Reference");
    object.affine = Affine64([1. / 7., 0., 0., 1. / 7., 0., 0.]);
    let (doc, owner, _) = document([128; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [128; 2];
    let mut scene = Scene::new(&r);
    let (target, _) = create_color_target(&r.device, [64; 2], "prepared reference read");
    let frame = packet(doc.scene(), [128; 2]);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let before = r.device.source_samples.stats();
    let result = scene.capture_region_prepared(&mut r, frame, &target, PixelRect::full([64; 2]), Output::Objects(owner), &mut encoder);
    assert!(matches!(result, Err(GpuRasterError::DeferredObjectWork)));
    assert_eq!(r.device.source_samples.stats(), before, "the initiating read cannot decompress source pixels");
    assert!(!r.image_decode_waiting(), "initial admission schedules only metadata");
    assert_eq!(encoder.pass_count(), 0);
    r.uploads.finish(&encoder); r.last_submission = Some(encoder.submit(&r.queue)); r.wait_idle().unwrap();
    let mut complete = false;
    for _ in 0..500 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = scene.capture_region_prepared(&mut r, frame, &target, PixelRect::full([64; 2]), Output::Objects(owner), &mut encoder);
        if let Err(error) = &result { assert!(matches!(error, GpuRasterError::DeferredObjectWork), "{error:?}"); }
        assert!(encoder.pass_count() < 40, "live reads advance bounded GPU work");
        r.uploads.finish(&encoder); r.last_submission = Some(encoder.submit(&r.queue)); r.wait_idle().unwrap();
        if result.is_ok() { complete = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(complete);
    let pixels = float_pixels(&r, &target);
    assert!((pixels[32 * 64 + 32][0] - 1.).abs() < 2e-5);
    assert!((pixels[32 * 64 + 32][3] - 1.).abs() < 2e-5);
}

#[test]
fn cold_object_reads_advance_every_finished_tile_and_the_idle_budget_per_attempt() {
    let mut object = ImageObject::new(image([2048; 2], [255, 0, 0, 255]), "Reference");
    object.affine = Affine64([1. / 16., 0., 0., 1. / 16., 0., 0.]);
    let (doc, owner, _) = document([128; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [128; 2];
    let mut scene = Scene::new(&r);
    let (target, _) = create_color_target(&r.device, [128; 2], "prepared reference read");
    let frame = packet(doc.scene(), [128; 2]);
    let mut attempts = 0;
    loop {
        attempts += 1;
        assert!(attempts < 32, "a read of 64 source tiles and 2^24 taps completes in a few host polls");
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = scene.capture_region_prepared(&mut r, frame, &target, PixelRect::full([128; 2]), Output::Objects(owner), &mut encoder);
        if let Err(error) = &result { assert!(matches!(error, GpuRasterError::DeferredObjectWork), "{error:?}"); }
        r.uploads.finish(&encoder); r.last_submission = Some(encoder.submit(&r.queue)); r.wait_idle().unwrap();
        if result.is_ok() { break; }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    assert!((float_pixels(&r, &target)[64 * 128 + 64][0] - 1.).abs() < 2e-5);
}

fn cold_query_renderer(doc: &layer_core::Document) -> WgpuRasterizer {
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = doc.composition().size;
    r.artwork_frame = Some(Arc::new(crate::artwork::Frame::new(packet(doc.scene(), r.document_extent), Default::default())));
    r
}

fn hide_objects(doc: &mut layer_core::Document, owner: OccurrenceHandle) {
    let mut occurrence = doc.artwork.occurrences.get(owner).unwrap().clone();
    occurrence.visible = false;
    doc.apply(layer_core::Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, owner, Some(occurrence)).unwrap())).unwrap();
}

#[test]
fn cold_composite_color_sample_retries_its_frame_before_accepting_the_next_query() {
    use layer_render::{ColorSampleRequest, ColorSampleSource, ColorSampleArea};
    let mut object = ImageObject::new(image([512; 2], [255, 0, 0, 255]), "Red");
    object.affine = Affine64([1. / 7., 0., 0., 1. / 7., 0., 0.]);
    let (mut doc, owner, _) = document([128; 2], vec![object]);
    let mut r = cold_query_renderer(&doc);
    let request = ColorSampleRequest { request_id: 100, source: ColorSampleSource::Composite, position: [32; 2], area: ColorSampleArea::Average5 };
    let before = r.device.source_samples.stats();
    assert!(r.request_color_sample(request).unwrap());
    assert!(r.color_sample_pending());
    assert_eq!(r.device.source_samples.stats(), before);
    assert!(!r.request_color_sample(ColorSampleRequest {request_id:101,..request}).unwrap());
    hide_objects(&mut doc, owner);
    r.artwork_frame = Some(Arc::new(crate::artwork::Frame::new(packet(doc.scene(), r.document_extent), Default::default())));
    let deadline = std::time::Instant::now() + crate::READBACK_TIMEOUT;
    let first = loop {
        if let Some(sample) = r.take_color_sample() { break sample.unwrap(); }
        assert!(std::time::Instant::now() < deadline, "cold composite color query must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(first.request_id, 100);
    assert!(crate::test_support::max_error(&[first.rgba], &[[1., 0., 0., 1.]]) < 2e-5);
    assert!(r.request_color_sample(ColorSampleRequest {request_id:101,..request}).unwrap());
    let second = loop {
        if let Some(sample) = r.take_color_sample() { break sample.unwrap(); }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(second.request_id, 101);
    assert_eq!(second.rgba, [0.; 4]);
    assert!(!r.color_sample_pending());
}

#[test]
fn cold_composite_region_classification_progresses_beyond_its_capture_cache() {
    use layer_render::{RegionRequest, RegionSource};
    let extent = [257, 8449];
    let mut object = ImageObject::new(image([1; 2], [255, 0, 0, 255]), "Red");
    object.interpolation = ImageInterpolation::Nearest;
    object.affine = Affine64([f64::from(extent[0]), 0., 0., f64::from(extent[1]), 0., 0.]);
    let (doc, _, _) = document(extent, vec![object]);
    let mut r = cold_query_renderer(&doc);
    let before = r.device.source_samples.stats();
    assert!(r.request_region(RegionRequest {request_id:200,source:RegionSource::Composite,position:[0;2],tolerance:0.,contiguous:false,selection:None,refinement:Default::default(),limit:None,enclosure:None}).unwrap());
    assert_eq!(r.device.source_samples.stats(), before);
    let deadline = std::time::Instant::now() + crate::READBACK_TIMEOUT;
    let result = loop {
        if let Some(result) = r.take_region() { break result.unwrap(); }
        assert!(std::time::Instant::now() < deadline, "classification must retain completed batches while cold windows advance");
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(result.request_id, 200);
    assert_eq!(result.pixels.coverage_bounds(), [0, 0, extent[0], extent[1]]);
    let count = result.pixels.pixels_per_word();
    let stride = extent[0].div_ceil(count);
    for y in 0..extent[1] { for x in 0..extent[0] {
        let word = result.pixels.words()[(y * stride + x / count) as usize];
        assert_eq!((word >> ((x % count) * 4)) & 15, 4, "coverage at {x},{y}");
    }}
    assert!(!r.region_pending());
}

#[test]
fn cancelling_a_cold_object_region_never_publishes_into_its_replacement() {
    use layer_render::{RegionRequest, RegionSource};
    let (mut doc, owner, _) = document([32; 2], vec![ImageObject::new(image([16; 2], [255, 0, 0, 255]), "Red")]);
    let mut r = cold_query_renderer(&doc);
    let request = RegionRequest {request_id:300,source:RegionSource::Composite,position:[8;2],tolerance:0.,contiguous:true,selection:None,refinement:Default::default(),limit:None,enclosure:None};
    assert!(r.request_region(request.clone()).unwrap());
    assert!(r.region_pending());
    r.cancel_region();
    assert!(!r.region_pending());
    hide_objects(&mut doc, owner);
    r.artwork_frame = Some(Arc::new(crate::artwork::Frame::new(packet(doc.scene(), r.document_extent), Default::default())));
    let result = crate::test_support::receive_request(&mut r, RegionRequest {request_id:301,..request});
    assert_eq!(result.request_id, 301);
    assert_eq!(result.pixels.coverage_bounds(), [0, 0, 32, 32]);
    for _ in 0..10 { assert!(r.take_region().is_none()); }
}

#[test]
fn overlapping_collection_consumes_children_with_bounded_residency_and_preserves_order() {
    let colors=[[211,37,83,32],[29,163,61,57],[73,43,197,91]];
    let images=colors.map(|color|image([256;2],color));
    let objects=(0..80).map(|index| {
        let mut object=ImageObject::new(images[index%3].clone(),"Translucent image");
        object.interpolation=ImageInterpolation::Nearest;object
    }).collect();
    let (doc,owner,_)=document([256;2],objects);
    for blend in [layer_core::BlendSpace::Linear,layer_core::BlendSpace::Perceptual] {
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();r.document_extent=[256;2];
        let mut scene=Scene::new(&r);
        let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
        let frame=FramePacket {blend_space:blend,..packet(doc.scene(),[256;2])};
        let request=scene.collection_job(&r,frame,owner,content,ObjectWindow {origin:[0.;2],side:1.,size:[256;2]});
        let mut cache=super::super::object_cache::ObjectCache::default();
        assert!(cache.resolve_collection(&r,doc.scene(),&request).unwrap().is_none());
        let start=std::time::Instant::now();let mut peak=0;let view=loop {
            assert!(start.elapsed()<std::time::Duration::from_secs(40),"ordered collection must make bounded progress");
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.drain_image_decode(&mut encoder).unwrap();let taps=cache.advance(&mut r,&mut scene,&mut encoder,true).unwrap();
            peak=peak.max(cache.bytes());assert!(peak<16*1024*1024,"one child plus isolated prefix: {peak}");
            let limit=super::super::object_cache::INTERACTIVE_TAPS+crate::object_sampling::DISPATCH_TAPS;
            assert!(taps<=limit && encoder.pass_count()*super::super::object_cache::PASS_TAPS<=limit+16*super::super::object_cache::PASS_TAPS,"sampling work and passes per batch are bounded");
            r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
            if let Some(view)=cache.resolve_collection(&r,doc.scene(),&request).unwrap() {break view;}
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        let pixels=float_pixels(&r,view.texture());
        let mut expected=[0f64;4];
        for index in (0..80).rev() {
            let alpha=f64::from(colors[index%3][3])/255.;
            for axis in 0..3 {
                let value=f64::from(colors[index%3][axis])/255.;
                let value=if blend==layer_core::BlendSpace::Linear {layer_core::color::RgbSpace::Srgb.decode(value)} else {value};
                expected[axis]=value*alpha+expected[axis]*(1.-alpha);
            }
            expected[3]=alpha+expected[3]*(1.-alpha);
        }
        for (actual,expected) in pixels[128*256+128].iter().zip(expected) {assert!((f64::from(*actual)-expected).abs()<0.00002,"{actual} != {expected}");}
        assert!(!cache.pending());
        eprintln!("overlapping collection {blend:?} peak={peak} elapsed={:?}",start.elapsed());
    }
}

#[test]
fn unfinished_collection_cancels_changed_content_and_display_visibility_but_keeps_raw_queries() {
    let mut object=ImageObject::new(image([256;2],[171,37,83,190]),"Nearest");object.interpolation=ImageInterpolation::Nearest;
    let (mut doc,owner,handles)=document([256;2],vec![object.clone(),object]);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();r.document_extent=[256;2];
    let mut scene=Scene::new(&r);scene.object_display=true;
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    let request=scene.collection_job(&r,packet(doc.scene(),[256;2]),owner,content,ObjectWindow {origin:[0.;2],side:1.,size:[256;2]});
    let mut cache=super::super::object_cache::ObjectCache::default();cache.resolve_collection(&r,doc.scene(),&request).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());cache.advance(&mut r,&mut scene,&mut encoder,true).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
    doc.artwork.occurrences.get_mut(owner).unwrap().visible=false;
    cache.retain(doc.scene(),request.blend,request.context);assert!(!cache.pending(),"hidden display owner cancels unfinished work");
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());cache.flush_retired(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
    let mut raw=request.clone();raw.display=false;
    cache.resolve_collection(&r,doc.scene(),&raw).unwrap();cache.retain(doc.scene(),raw.blend,raw.context);assert!(cache.pending(),"raw objects include hidden owners");
    let collection=doc.artwork.occurrences.get(owner).unwrap().content.clone();
    let OccurrenceContent::Objects(collection)=collection else {unreachable!()};
    doc.artwork.object_layers.get_mut(collection).unwrap().children.reverse();
    cache.retain(doc.scene(),raw.blend,raw.context);assert!(!cache.pending(),"reordering cancels the captured child cursor");
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    raw=scene.collection_job(&r,packet(doc.scene(),[256;2]),owner,content,ObjectWindow {origin:[0.;2],side:1.,size:[256;2]});raw.display=false;
    cache.resolve_collection(&r,doc.scene(),&raw).unwrap();
    doc.artwork.objects.get_mut(handles[0]).unwrap().affine.0[4]=0.5;
    cache.retain(doc.scene(),raw.blend,raw.context);assert!(!cache.pending(),"pose changes cancel a captured source mapping");
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    raw=scene.collection_job(&r,packet(doc.scene(),[256;2]),owner,content,ObjectWindow {origin:[0.;2],side:1.,size:[256;2]});raw.display=false;
    cache.resolve_collection(&r,doc.scene(),&raw).unwrap();
    cache.retain(doc.scene(),layer_core::BlendSpace::Perceptual,raw.context);assert!(!cache.pending(),"blend changes retire the previous color domain");
    cache.resolve_collection(&r,doc.scene(),&raw).unwrap();
    cache.retain(doc.scene(),raw.blend,layer_core::color::RgbSpace::DisplayP3);assert!(!cache.pending(),"working RGB changes retire the previous interpretation");
}

#[test]
fn collection_nearest_preserves_f64_source_floor_at_affine_boundaries() {
    let source=AuthoredImage::new(rgba8_source([256;2],|x,y|[if x%2==0 {211} else {29},if y%2==0 {37} else {163},83,255]));
    let mut object=ImageObject::new(source,"Nearest boundary");object.interpolation=ImageInterpolation::Nearest;
    object.affine=Affine64([0.55,0.,0.,0.55,80.,13.]);
    let inverse=object.affine.inverse().unwrap();
    let (doc,owner,_)=document([256;2],vec![object]);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let pixels=native(&mut r,&doc,Output::Objects(owner));
    for y in 0..256 {for x in 0..256 {
        let source=inverse.map([x as f64+0.5,y as f64+0.5]).map(|v|v.floor() as i64);
        let expected=if source.iter().all(|v|(0..256).contains(v)) {
            let color=[if source[0]%2==0 {211.} else {29.},if source[1]%2==0 {37.} else {163.},83.];
            [layer_core::color::RgbSpace::Srgb.decode(color[0]/255.) as f32,layer_core::color::RgbSpace::Srgb.decode(color[1]/255.) as f32,layer_core::color::RgbSpace::Srgb.decode(color[2]/255.) as f32,1.]
        } else {[0.;4]};
        for (actual,expected) in pixels[y*256+x].iter().zip(expected) {assert!((*actual-expected).abs()<0.00002,"pixel {x},{y}: {actual} != {expected}");}
    }}
}

#[test]
fn deferred_collection_construction_discards_unencoded_prefix_and_reuses_scratch() {
    let mut object=ImageObject::new(image([256;2],[171,37,83,190]),"Nearest");object.interpolation=ImageInterpolation::Nearest;
    let (doc,owner,_)=document([256;2],vec![object]);
    let r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut scene=Scene::new(&r);scene.object_query=true;
    for _ in 0..12 {
        scene.alloc(&r,wgpu::Color::TRANSPARENT);
        assert!(matches!(scene.object_tile(&r,packet(doc.scene(),[256;2]),owner,[0;2]),Err(GpuRasterError::DeferredObjectWork)));
        assert!(scene.jobs.is_empty());assert!(scene.source_jobs.is_empty());assert!(scene.used.iter().all(|used|!*used));
        assert_eq!(scene.pool.len(),1,"abandoned composition prefix remains reusable");
        assert!(scene.object_results.pending(),"private collection progress remains scheduled");
    }
}

fn tablet_scene(distinct: bool, count: usize, blur: bool) -> (layer_core::Document, OccurrenceHandle, Vec<ImageObjectHandle>, layer_render::ViewState) {
    let extent = [4248, 2832];
    let photo = AuthoredImage::new(rgba8_source(extent, |x, y| [(x * 7 % 256) as u8, (y * 5 % 256) as u8, ((x ^ y) % 256) as u8, 255]));
    let objects = (0..count).map(|index| {
        let phase = index as f64 / count as f64 * std::f64::consts::TAU;
        let source = if distinct { AuthoredImage::new(rgba8_source(extent, move |x, y| [((x + index as u32 * 31) % 256) as u8, (y % 256) as u8, 90, 255])) } else { photo.clone() };
        let mut object = ImageObject::new(source, "Image");
        object.affine = Affine64([0.55, 0., 0., 0.55, f64::from(extent[0]) * (0.225 + 0.08 * phase.cos()), f64::from(extent[1]) * (0.225 + 0.08 * phase.sin())]);
        object
    }).collect();
    let (mut doc, owner, handles) = document(extent, objects);
    doc.artwork.occurrences.get_mut(owner).unwrap().opacity = 0.35;
    let (base, target) = crate::test_support::add_paint(&mut doc.artwork, "Photo", extent);
    let SourceTarget::Paint(target) = target else { unreachable!() };
    doc.artwork.paint.get_mut(target).unwrap().base = Some(PaintBase { image: photo, offset: [0; 2], policy: PaintBasePolicy::SourceProfile });
    let stack = doc.composition().result;
    let mut entries = vec![owner, base];
    if blur {
        let instance = layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program());
        let mut application = EffectApplication::new(instance.program, instance.values, extent);
        let sigma = application.program.parameters.iter().position(|parameter| parameter.key.as_ref() == "sigma").unwrap();
        application.values[sigma] = layer_core::EffectValue::Number(8.);
        let application = doc.artwork.effects.insert(PortableId::random(), application).unwrap();
        let mut effect = Occurrence::new(OccurrenceContent::Effect(application), "Blur"); effect.attachment = layer_core::Attachment::Effect;
        entries.insert(0, doc.artwork.occurrences.insert(PortableId::random(), effect).unwrap());
    }
    doc.artwork.stacks.get_mut(stack).unwrap().entries = entries;
    let doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
    let zoom = (1920. / 4248f32).min(1200. / 2832.);
    let view = layer_render::ViewState { width_px: 1920, height_px: 1200,
        document_to_surface: [zoom, 0., 0., zoom, (1920. - 4248. * zoom) / 2., (1200. - 2832. * zoom) / 2.] };
    (doc, owner, handles, view)
}

fn settle_frames(r: &mut WgpuRasterizer, frame: FramePacket<'_>, limit: usize) -> usize {
    r.submit(FramePacket { composite_all: true, reset_layers: true, ..frame }).unwrap();
    for submitted in 1..=limit {
        r.wait_idle().unwrap();
        if !r.has_pending_work() { return submitted; }
        r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
    }
    panic!("object scene must settle within {limit} submissions");
}

#[test]
fn moving_blurred_object_layer_publishes_every_pose_without_queueing_canonical_work() {
    for distinct in [false, true] {
        let (mut doc, owner, handles, view) = tablet_scene(distinct, 4, true);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        settle_frames(&mut r, FramePacket { view, ..packet(doc.scene(), [4248, 2832]) }, 2000);
        layer_render::CanvasRenderer::prepare_moving_layer(&mut r, Some(owner));
        let original = doc.scene().object(handles[0]).unwrap().affine;
        for step in 0..24 {
            let mut affine = original; affine.0[4] += 200. * (f64::from(step) * 0.1).sin(); affine.0[5] += 150. * (f64::from(step) * 0.1).cos();
            doc.apply(doc.set_image_object_affine_edit(handles[0], affine).unwrap()).unwrap();
            let (revision, composited) = (r.composite_revision, r.metrics.composited_pixels);
            r.submit(FramePacket { view, composite_all: true, time_seconds: f32::from(step as u8) / 60., ..packet(doc.scene(), [4248, 2832]) }).unwrap();
            r.wait_idle().unwrap();
            assert!(r.composite_revision != revision && !r.object_deferred, "pose {step} publishes a fresh preview (distinct={distinct})");
            let full = r.scale_display.as_ref().unwrap().plan.size.into_iter().map(u64::from).product::<u64>();
            assert!(step == 0 || (r.metrics.composited_pixels - composited) * 2 < full, "an advancing frame clock without animated effects keeps bounded object damage");
            assert!(!r.scene.as_ref().unwrap().object_results.pending(), "a moving layer never starts canonical work for a superseded pose");
        }
        layer_render::CanvasRenderer::prepare_moving_layer(&mut r, None);
        settle_frames(&mut r, FramePacket { view, ..packet(doc.scene(), [4248, 2832]) }, 2000);
        let display = r.scale_display.as_ref().unwrap();
        assert!(display.plan.level > 0);
        let bytes = r.scene.as_ref().unwrap().object_results.bytes();
        assert!(bytes <= 64 * 1024 * 1024, "settled canonical windows stay within one private allowance: {bytes}");
    }
}

#[test]
fn cold_tablet_object_previews_and_canonical_results_settle_in_bounded_submissions() {
    let (doc, _, _, mut view) = tablet_scene(false, 4, false);
    let zoom = 0.1597f32;
    view.document_to_surface = [zoom, 0., 0., zoom, (1920. - 4248. * zoom) / 2., (1200. - 2832. * zoom) / 2.];
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let passes = r.metrics.command_passes;
    settle_frames(&mut r, FramePacket { view, ..packet(doc.scene(), [4248, 2832]) }, 4000);
    assert!(r.metrics.command_passes - passes < 4000, "exact canonical tent windows need one dispatch per source tile and child part plus boundary denominators: {}", r.metrics.command_passes - passes);
    let (doc, _, _, view) = tablet_scene(true, 4, false);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let submissions = settle_frames(&mut r, FramePacket { view, ..packet(doc.scene(), [4248, 2832]) }, 4000);
    assert_eq!(r.moving_images.completed_images, 4);
    eprintln!("cold tablet submissions={submissions}");
    assert!(submissions < 400, "four cold 12MP previews and canonical windows settle in {submissions} submissions");
}

#[test]
fn moving_collection_preview_composes_ordered_children_across_source_batches() {
    let colors: Vec<[u8; 4]> = (0..10u8).map(|index| [20 + index * 23, 200 - index * 17, 60 + index * 9, 40 + index * 7]).collect();
    let images: Vec<_> = colors.iter().map(|color| image([64; 2], *color)).collect();
    let objects = (0..20).map(|index| {
        let mut object = ImageObject::new(images[index % 10].clone(), "Translucent");
        object.affine = Affine64([1., 0., 0., 1., f64::from((index % 5) as u32 * 4), f64::from((index / 5) as u32 * 4)]);
        object
    }).collect();
    let (doc, owner, _) = document([128; 2], objects);
    for blend in [layer_core::BlendSpace::Linear, layer_core::BlendSpace::Perceptual] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        layer_render::CanvasRenderer::prepare_moving_layer(&mut r, Some(owner));
        let mut frame = FramePacket { blend_space: blend, ..packet(doc.scene(), [128; 2]) };
        frame.view.width_px = 32; frame.view.height_px = 32; frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        settle_frames(&mut r, frame, 2000);
        assert!(!r.scene.as_ref().unwrap().object_results.pending(), "a moving layer composes previews without canonical work");
        let display = r.scale_display.as_ref().unwrap();
        assert_eq!(display.plan.level, 2);
        let pixels = float_pixels(&r, display.texture());
        let mut expected = [0f64; 4];
        for index in (0..20).rev() {
            let color = colors[index % 10];
            let alpha = f64::from(color[3]) / 255.;
            for axis in 0..3 {
                let value = f64::from(color[axis]) / 255.;
                let value = if blend == layer_core::BlendSpace::Linear { layer_core::color::RgbSpace::Srgb.decode(value) } else { value };
                expected[axis] = value * alpha + expected[axis] * (1. - alpha);
            }
            expected[3] = alpha + expected[3] * (1. - alpha);
        }
        let covered = pixels[(10 * display.plan.size[0] + 10) as usize];
        for (actual, expected) in covered.iter().zip(expected) { assert!((f64::from(*actual) - expected).abs() < 2e-3, "{blend:?}: {actual} != {expected}"); }
        assert_eq!(pixels[(30 * display.plan.size[0] + 30) as usize], [0.; 4]);
    }
}

#[test]
fn moving_nearest_objects_publish_level_zero_poses_with_and_without_effects() {
    let mut builder = SourceBuilder::new([64; 2], SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 1024 * 1024).unwrap();
    for y in 0..64 { builder.push_row(&(0..64).flat_map(|x| { let value = if (x + y) % 2 == 0 { 255 } else { 0 }; [value, value, value, 255] }).collect::<Vec<_>>()).unwrap(); }
    let checker = AuthoredImage::new(Arc::new(builder.finish().unwrap()));
    for blur in [false, true] {
        let mut object = ImageObject::new(checker.clone(), "Nearest checker");
        object.interpolation = ImageInterpolation::Nearest;
        object.affine = Affine64([0.75, 0., 0., 0.75, 64., 64.]);
        let (mut doc, owner, handles) = document([256; 2], vec![object]);
        if blur {
            let instance = layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program());
            let application = doc.artwork.effects.insert(PortableId::random(), EffectApplication::new(instance.program, instance.values, [256; 2])).unwrap();
            let mut effect = Occurrence::new(OccurrenceContent::Effect(application), "Blur"); effect.attachment = layer_core::Attachment::Effect;
            let effect = doc.artwork.occurrences.insert(PortableId::random(), effect).unwrap();
            let stack = doc.composition().result;
            doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(0, effect);
            doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
        }
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let view = layer_render::ViewState { width_px: 64, height_px: 64, document_to_surface: [0.25, 0., 0., 0.25, 0., 0.] };
        settle_frames(&mut r, FramePacket { view, ..packet(doc.scene(), [256; 2]) }, 2000);
        layer_render::CanvasRenderer::prepare_moving_layer(&mut r, Some(owner));
        for step in 0..12 {
            let mut affine = doc.scene().object(handles[0]).unwrap().affine; affine.0[4] += 1.37; affine.0[5] -= 0.61;
            doc.apply(doc.set_image_object_affine_edit(handles[0], affine).unwrap()).unwrap();
            let revision = r.composite_revision;
            r.submit(FramePacket { view, composite_all: false, ..packet(doc.scene(), [256; 2]) }).unwrap();
            r.wait_idle().unwrap();
            assert!(r.composite_revision != revision && !r.object_deferred, "Nearest pose {step} publishes (blur={blur})");
            assert!(!r.scene.as_ref().unwrap().object_results.pending(), "Nearest motion queues no canonical work");
            if !blur {
                let pixels = float_pixels(&r, r.scale_display.as_ref().unwrap().texture());
                assert!(pixels.iter().all(|pixel| [[0., 0., 0., 1.], [1.; 4], [0.; 4]].contains(pixel)), "Nearest previews never average source texels");
                assert!(pixels.iter().any(|pixel| *pixel == [1.; 4]) && pixels.iter().any(|pixel| *pixel == [0., 0., 0., 1.]));
            }
        }
    }
}

#[test]
fn overlapping_canonical_windows_sample_only_their_uncovered_content() {
    let (doc, owner, _, _) = tablet_scene(false, 4, false);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [4248, 2832];
    let mut scene = Scene::new(&r);
    scene.object_spatial.prepare(doc.scene());
    let content = scene.object_spatial.content(doc.scene(), owner).unwrap();
    let evaluate = |r: &mut WgpuRasterizer, scene: &mut Scene, origin: [f64; 2]| {
        let frame = packet(doc.scene(), [4248, 2832]);
        let mut request = scene.collection_job(r, frame, owner, content.clone(), ObjectWindow { origin, side: 4., size: [256; 2] });
        request.live = true; request.display = true;
        let mut taps = 0;
        for _ in 0..2000 {
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            r.drain_image_decodes(&mut encoder).unwrap();
            if let Some(pieces) = scene.canonical_cover(r, frame, &content, &request).unwrap() {
                r.uploads.finish(&encoder); encoder.submit(&r.queue);
                return (taps, pieces.into_iter().map(|(_, bounds)| bounds).collect::<Vec<_>>());
            }
            let mut cache = std::mem::take(&mut scene.object_results);
            taps += cache.advance(r, scene, &mut encoder, false).unwrap();
            scene.object_results = cache;
            r.uploads.finish(&encoder); encoder.submit(&r.queue); r.wait_idle().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("canonical window completes");
    };
    let (first, pieces) = evaluate(&mut r, &mut scene, [600., 400.]);
    assert_eq!(pieces.len(), 1);
    let (shifted, pieces) = evaluate(&mut r, &mut scene, [1112., 400.]);
    assert!(pieces.len() == 2 && pieces.iter().all(|bounds| !bounds.intersect(DocRect { min: [1112, 400], max: [2136, 1424] }).is_empty()));
    let mut empty = Scene::new(&r);
    let (fresh, _) = evaluate(&mut r, &mut empty, [1112., 400.]);
    assert!(shifted * 3 < fresh * 2, "half of the shifted window is reused: {shifted} of {fresh} taps (first {first})");
    let (covered, _) = evaluate(&mut r, &mut scene, [856., 400.]);
    assert_eq!(covered, 0, "a window inside completed results samples nothing");
}

#[test]
fn eight_distinct_sources_settle_in_few_frames_with_a_tablet_sized_source_cache() {
    let (doc, _, _, mut view) = tablet_scene(true, 8, false);
    view.document_to_surface = [0.1597, 0., 0., 0.1597, 670.09, 445.26];
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.source_tiles.get_mut().admit(400 * 4 * (1 << 20));
    let frames = settle_frames(&mut r, FramePacket { view, ..packet(doc.scene(), [4248, 2832]) }, 4000);
    assert!(frames < 400, "decoding stays ahead of eight sources' mip and canonical work: {frames} frames");
}
