use super::*;

fn document() -> Document {
    let mut artwork = Artwork::new([32; 2]).unwrap();
    let mut owners = Vec::new();
    for (color, offset) in [([255, 0, 0, 255], [0., 0.]), ([0, 0, 255, 128], [12., 8.])] {
        let extent = [256; 2];
        let mut builder = SourceBuilder::new(extent, SourceInterpretation {
            channels: SourceChannels::Rgba, depth: SampleDepth::U8,
            profile: Default::default(), profile_assumed: false,
        }, 1024 * 1024).unwrap();
        for _ in 0..extent[1] {builder.push_row(&color.repeat(extent[0] as usize)).unwrap();}
        let mut object = ImageObject::new(layer_core::authored::Image::new(Arc::new(builder.finish().unwrap())));
        object.affine = Affine64([1. / 16., 0., 0., 1. / 16., offset[0], offset[1]]);
        let handle=artwork.objects.insert(PortableId::random(), object).unwrap();
        owners.push(artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Reduced image")).unwrap());
    }
    let mut effect = EffectInstance::new(crate::tests::fixture("gaussian_blur").program());
    effect.set("sigma", layer_core::EffectValue::Number(1.5)).unwrap();
    let application = artwork.effects.insert(PortableId::random(), EffectApplication::new(effect.program, effect.values, [32; 2])).unwrap();
    let effect = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(application), "Blur")).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    owners.insert(0,effect);artwork.stacks.get_mut(stack).unwrap().entries=owners;
    Document::from_artwork(artwork).unwrap()
}

#[test]
fn deferred_object_snapshot_retries_preserve_filtered_windows_and_histogram() {
    let doc = document();
    let mut live = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    live.ensure_document_metadata([32; 2], doc.scene()).unwrap();
    let (texture, _) = create_color_target(&live.device, [32; 2], "native object reference");
    let mut scene = live.scene.take().unwrap_or_else(|| scene::Scene::new(&live));
    let mut encoder = submission::CommandEncoder::new(&live.device, &Default::default());
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
    loop {
        match scene.capture_region_prepared(&mut live, packet(doc.scene(), [32; 2]), &texture,
            PixelRect::full([32; 2]), scene::Output::Artwork(None), &mut encoder) {
            Ok(())=>break,
            Err(GpuRasterError::DeferredObjectWork)=>{},
            Err(error)=>panic!("{error}"),
        }
        assert!(std::time::Instant::now()<deadline,"Native object reference completes");
        live.uploads.finish(&encoder);encoder.submit(&live.queue);live.wait_idle().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        encoder=submission::CommandEncoder::new(&live.device,&Default::default());
    }
    live.uploads.finish(&encoder);encoder.submit(&live.queue);live.scene=Some(scene);
    let expected = crate::test_support::float_pixels(&live, &texture);
    let mut reader = capture(doc).unwrap();
    assert!(reader.renderer.snapshot_worker);
    let actual = reader.read_region([0, 0, 32, 32]).unwrap();
    assert!(crate::test_support::max_error(&expected, &actual) < 3e-5);
    let window = reader.read_region([9, 7, 11, 13]).unwrap();
    let expected_window = (7..20).flat_map(|y| expected[y * 32 + 9..y * 32 + 20].iter().copied()).collect::<Vec<_>>();
    assert!(crate::test_support::max_error(&expected_window, &window) < 3e-5);
    let mut histogram = layer_core::color::histogram::Histogram::new(reader.color());
    histogram.add(&expected).unwrap();
    assert_eq!(reader.histogram().unwrap(), histogram);
}

#[test]
fn cancelled_deferred_object_drain_keeps_output_unpublished() {
    let doc = document();
    let mut reader = capture(doc).unwrap();
    let (r, packet, region, mut encoder) = reader.prepare_region_gpu([0, 0, 32, 32], 0).unwrap();
    let (texture, _) = create_color_target(&r.device, [32; 2], "cancelled object capture");
    let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
    assert!(matches!(scene.capture_region(r, packet, &texture, region, scene::Output::Artwork(None), &mut encoder), Err(GpuRasterError::DeferredObjectWork)));
    r.snapshot_cancelled.as_ref().unwrap().store(true, Ordering::Relaxed);
    assert!(matches!(pollster::block_on(scene.drain_exact_objects_async(r)), Err(GpuRasterError::Color(message)) if message == "Snapshot capture cancelled"));
    assert_eq!(reader.control.output_rows(), 0);
}

fn photo_objects(extent: [u32; 2], objects: &[([u32; 2], [f64; 6])]) -> Document {
    let mut artwork = Artwork::new(extent).unwrap();
    let owners = objects.iter().map(|&(size, affine)| {
        let image = layer_core::color::source::rgba8_source(size, |x, y| [(x * 7 % 256) as u8, (y * 5 % 256) as u8, ((x ^ y) % 256) as u8, 255]);
        let mut object = ImageObject::new(layer_core::authored::Image::new(image));
        object.affine = Affine64(affine);
        let handle=artwork.objects.insert(PortableId::random(), object).unwrap();
        artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Image")).unwrap()
    }).collect();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries = owners;
    Document::from_artwork(artwork).unwrap()
}

#[test]
fn whole_document_object_previews_drain_each_band_in_a_few_submissions() {
    let doc = photo_objects([2048, 1536], &[([2048, 1536], [1., 0., 0., 1., 0., 0.]), ([512, 384], [1., 0., 0., 1., 700., 500.])]);
    let mut reader = capture(doc).unwrap();
    let preview = reader.preview_document([512; 2], layer_core::color::RgbSpace::Srgb).unwrap();
    assert_eq!(preview.extent, [512, 384]);
    let drains = reader.renderer.metrics.object_drain_submissions;
    assert!(drains <= 24, "each of 24 band captures samples its object pages together: {drains} submissions");
}

#[test]
fn nearest_object_captures_finish_while_their_coordinates_are_prepared() {
    let mut doc = photo_objects([1024, 768], &[([400, 300], [2.5, 0.3, -0.3, 2.5, 30., 10.])]);
    let object = doc.artwork.objects.iter().next().unwrap().0;
    doc.artwork.objects.get_mut(object).unwrap().interpolation = ImageInterpolation::Nearest;
    let doc = Document::from_artwork(doc.artwork).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut reader = capture(doc).unwrap();
        let _ = sender.send(reader.read_region([0, 0, 1024, 768]).map(|pixels| (pixels.len(), reader.renderer.metrics.object_drain_submissions)));
    });
    let (pixels, drains) = receiver.recv_timeout(std::time::Duration::from_secs(120)).expect("a Nearest object capture makes progress").unwrap();
    worker.join().unwrap();
    assert_eq!(pixels, 1024 * 768);
    assert!(drains <= 64, "coordinate preparation waits without resubmitting: {drains} submissions");
}
