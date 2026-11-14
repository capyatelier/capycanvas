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
fn object_only_worker_exact_capture_contains_authored_alpha() {
    use layer_core::package::{ImmutableBacking,codec::{open,OpenOutcome}};
    let bytes=include_bytes!("../../../../../apps/layer-web/fixtures/shared-image-f64-builtin.capy");
    let backing=ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes.as_slice()))).unwrap();
    let OpenOutcome::Candidate {artwork,..}=open(backing,Default::default(),&AtomicBool::new(false)).unwrap()
        else {panic!("editable object fixture")};
    let mut document=Document::from_artwork(artwork).unwrap();
    let helpers=document.scene().order().iter().copied().filter(|owner|
        !matches!(document.scene().occurrence(*owner).unwrap().content,OccurrenceContent::Objects(_) | OccurrenceContent::Stack(_))).collect::<Vec<_>>();
    document.apply(document.delete_layers_edit(&helpers).unwrap()).unwrap();
    assert!(document.artwork.paint.is_empty());
    assert_eq!(document.artwork.objects.len(),3);
    let extent=document.composition().size;
    let control=CaptureControl::default();let cancel=control.clone();
    let (send,receive)=std::sync::mpsc::channel();
    let worker=std::thread::spawn(move || {
        let result=(|| {
            let mut reader=gpu().capture_scene(document.snapshot(),SceneScope::All,control)?;
            assert!(reader.renderer.snapshot_worker);
            reader.read_region([0,0,extent[0],extent[1]])
                .map(|pixels|(pixels.len(),pixels.iter().filter(|pixel|pixel[3]>0.).count()))
        })();
        let _=send.send(result);
    });
    let result=receive.recv_timeout(std::time::Duration::from_secs(60));
    if result.is_err() {cancel.cancel();}
    let (pixels,authored)=result.expect("object-only exact capture completes within its deadline").unwrap();
    worker.join().unwrap();
    assert_eq!(pixels,extent[0] as usize*extent[1] as usize);
    assert!(authored>0,"exact worker capture must include the object pixels");
}

#[test]
fn deferred_object_snapshot_retries_preserve_filtered_windows_and_histogram() {
    let doc = document();
    let mut live = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    live.ensure_document_metadata([32; 2], doc.scene()).unwrap();
    let (texture, _) = create_color_target(&live.device, [32; 2], "native object reference");
    let mut scene = live.scene.take().unwrap_or_else(|| scene::Scene::new(&live).unwrap());
    let mut encoder = submission::CommandEncoder::new(&live.device, &Default::default());
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
    loop {
        match scene.capture_region_prepared(&mut live, packet(doc.scene(), [32; 2]), &texture,
            PixelRect::full([32; 2]), scene::Output::Artwork(None), &mut encoder) {
            Ok(())=>break,
            Err(GpuRasterError::NotReady)=>{},
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
fn cancelled_object_captures_keep_output_unpublished() {
    let doc = document();
    let mut live = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    live.ensure_document_metadata([32; 2], doc.scene()).unwrap();
    let (texture, _) = create_color_target(&live.device, [32; 2], "cancelled object capture");
    let mut scene = live.scene.take().unwrap_or_else(|| scene::Scene::new(&live).unwrap());
    let mut encoder = submission::CommandEncoder::new(&live.device, &Default::default());
    assert!(matches!(scene.capture_region(&mut live, packet(doc.scene(), [32; 2]), &texture, PixelRect::full([32; 2]), scene::Output::Artwork(None), &mut encoder), Err(GpuRasterError::NotReady)));
    live.snapshot_cancelled = Some(Arc::new(AtomicBool::new(true)));
    assert!(matches!(pollster::block_on(scene.prepare_capture_work_async(&mut live)), Err(GpuRasterError::Color(message)) if message == "Snapshot capture cancelled"));
    let mut reader = capture(doc).unwrap();
    reader.control().cancel();
    assert!(reader.read_region([0, 0, 32, 32]).is_err());
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
fn export_sources_fit_one_aligned_capture_window_instead_of_the_whole_document() {
    let extent=[1024;2];
    let doc=photo_objects(extent,&vec![(extent,[1.,0.,0.,1.,0.,0.]);28]);
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(1<<40),override_bytes:Some(1<<40),..Default::default()});
    let hardware=crate::memory::memory_tests::renderer_in_budget(doc.composition().color,
        layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<40),..Default::default()},budget).unwrap();
    let mut reader=hardware.snapshot_gpu().capture_scene(doc.snapshot(),SceneScope::All,Default::default()).unwrap();
    for region in [[256,256,512,256],[0,0,1024,1024]] {
        let (renderer,_,_,encoder)=reader.prepare_region_gpu(region,0).unwrap();
        let slots=crate::memory::memory_tests::declared_source_demand(renderer)/crate::scene::sources::NORMALIZED_PAGE_BYTES;
        assert!(slots>=28*4*3,"the actual window's source footprint is demanded: {slots} slots");
        assert!(slots<28*4*4,"the capture does not demand the whole document: {slots} slots");
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);
        crate::test_support::complete(renderer);
    }
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

#[cfg(not(target_arch="wasm32"))]
#[test]
fn deferred_capture_finishes_two_bands_at_the_drained_working_floor() {
    fn document(layers:usize)->Document {
        let extent=[256,512];
        let mut doc=layered_painting(extent,1,false);
        let descriptor=doc.composition().color.paint_descriptor();
        let stack=doc.composition().result;
        let first=paint_id(&doc);
        for layer in 0..layers.max(1) {
            let paint=if layer==0 {first} else {
                let paint=doc.artwork.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:extent,raster:Default::default(),base:None,operations:Default::default()}).unwrap();
                let owner=doc.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),format!("Paint {layer}"))).unwrap();
                doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(0,owner);paint
            };
            let pixel=[layer as u8,(layer>>8) as u8,128,if layer==0 {255}else{128}];
            let tiles=if layers==0 {Default::default()}else{[(TileKey {plane:RasterPlane::Color,coordinate:[0,1]},RasterTile::backed(TileBlob::encode(descriptor,&pixel.repeat(65536)).unwrap()))].into()};
            doc.artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(RasterData {tiles,..Default::default()});
        }
        let source=layer_core::color::source::rgba8_source([16;2],|_,_|[255,0,0,255]);
        let image=doc.artwork.objects.insert(PortableId::random(),ImageObject::new(layer_core::authored::Image::new(source))).unwrap();
        let owner=doc.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(image),"Image")).unwrap();
        doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(0,owner);refresh(&mut doc);doc
    }
    let seed=document(0);
    let hardware=crate::memory::memory_tests::renderer(seed.composition().color,layer_core::DeviceMemoryFacts {unified:true,..Default::default()}).unwrap();
    let budget=hardware.factory.memory_budget().clone();budget.set_ram_override(Some(384<<20));
    let private_gpu=hardware.snapshot_gpu();drop(hardware);
    let mut floor=private_gpu.capture_scene(seed.snapshot(),SceneScope::All,Default::default()).unwrap();
    floor.renderer.publish_memory();budget.flush_reports();
    let _import=budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert_eq!(budget.import_headroom_bytes(),0);
    floor.renderer.refresh_memory_limits();
    let capacity=floor.renderer.source_tiles.borrow().free_slots();
    assert!(capacity>=SOURCE_SLOTS,"the production working floor remains granted: {capacity}");
    drop(floor);
    let doc=document(capacity+1);
    let mut reference=gpu().capture_scene(doc.snapshot(),SceneScope::All,Default::default()).unwrap();
    let expected=[pollster::block_on(reference.read_region_async([0,0,256,256])).unwrap(),pollster::block_on(reference.read_region_async([0,256,256,256])).unwrap()];
    drop(reference);
    let control=CaptureControl::default();let cancel=control.clone();
    let completed=Arc::new(std::sync::atomic::AtomicUsize::new(0));let progress=completed.clone();
    let (send,receive)=std::sync::mpsc::channel();
    let worker=std::thread::spawn(move || {
        let result=(|| {
            let mut reader=private_gpu.capture_scene(doc.snapshot(),SceneScope::All,control)?;
            reader.renderer.snapshot_worker=false;
            reader.renderer.publish_memory();budget.flush_reports();
            let _rest=budget.admit_import(budget.import_headroom_bytes()).unwrap();
            assert_eq!(budget.import_headroom_bytes(),0);reader.renderer.refresh_memory_limits();
            assert!(reader.renderer.source_tiles.borrow().free_slots()>capacity,"the drained floor pins the full-stack unit beyond the seed's capacity");
            let first=pollster::block_on(reader.read_region_async([0,0,256,256]))?;
            progress.store(1,Ordering::Release);
            let second=pollster::block_on(reader.read_region_async([0,256,256,256]))?;
            progress.store(2,Ordering::Release);
            Ok::<_,GpuRasterError>([first,second])
        })();
        let _=send.send(result);
    });
    let received=receive.recv_timeout(std::time::Duration::from_secs(10));
    if received.is_err() {cancel.cancel();}
    worker.join().expect("capture worker exits after completion or cancellation");
    let actual=received.unwrap_or_else(|error|panic!("two-band capture stops after {} completed bands at {capacity} decoded slots and {} cold inputs: {error}",completed.load(Ordering::Acquire),capacity+1)).unwrap();
    for (actual,expected) in actual.iter().zip(&expected) {assert!(crate::test_support::max_error(actual,expected)<3e-5);}
}
