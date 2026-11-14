use super::*;
use crate::test_support::prepare_raster as prepare;
use crate::layer_tests::placement::{paint_document,target,set_mask,reveal_all};
fn restore_document(color: DocumentColor) -> layer_core::Document {
    let mut document = paint_document([9504,6336],"native restore");
    let root = document.artwork.root;
    document.artwork.compositions.get_mut(root).unwrap().color = color;
    document
}
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};

fn blob(color: DocumentColor, plane: RasterPlane, seed: u32) -> TileBlob {
    let descriptor = plane.descriptor(color);
    let pixel: Vec<_> = (0..descriptor.channels).flat_map(|channel| {
        let code = if channel == 3 { color.depth.maximum() }
            else { (seed * 251 + u32::from(channel) * 71) & color.depth.maximum() };
        (code as u16).to_le_bytes().into_iter().take(color.depth.bytes())
    }).collect();
    TileBlob::encode(descriptor, &pixel.repeat(PAGE_SIZE.pow(2) as usize)).unwrap()
}

fn data(color: DocumentColor, count: u32, planes: &[RasterPlane], seed: u32) -> RasterData {
    RasterData {
        tiles: planes.iter().flat_map(|&plane| (0..count).map(move |i| (
            TileKey { plane, coordinate: [i % 11, i / 11] },
            RasterTile::backed(blob(color, plane, seed + i)),
        ))).collect(),
        watercolor: planes.contains(&RasterPlane::WatercolorWetness)
            .then_some(layer_core::raster::RasterWatercolor { wet_edge: 0.5, burnt_edge: 0.5, edge_width: 2. }),
    }
}

fn restored_color<'a>(r:&'a WgpuRasterizer,source:SourceTarget,selected:&RasterData,coordinate:[u32;2])->crate::resident::Read<'a,PageSurface> {
    let expected=selected.tiles[&TileKey {plane:RasterPlane::Color,coordinate}].content_version();
    r.read_layer_color(source,coordinate,&expected,None).unwrap()
}
fn live_pixels(r:&WgpuRasterizer,selected:&RasterData)->Vec<(RasterPlane,[u32;2],Vec<u8>)> {
    let layer=&r.paint_layers[0];
    layer.pages.iter().map(|page|(RasterPlane::Color,page.coordinate,
        crate::layer_tests::page_bytes(r,&restored_color(r,layer.id,selected,page.coordinate).value().texture)))
        .chain(layer.watercolor_wetness_pages.iter().map(|page| {
            let key=TileKey {plane:RasterPlane::WatercolorWetness,coordinate:page.coordinate};
            let read=page.active().read(selected.tiles[&key].content_version(),None).unwrap();
            (key.plane,key.coordinate,crate::layer_tests::page_bytes(r,&read.value().texture))
        })).collect()
}

#[test]
fn cold_multilayer_restore_keeps_unedited_pages_backed() {
    use layer_core::{authored::*,MemoryBudget,MemoryFacts,DeviceMemoryFacts};
    let extent=[4216,3016];let color=DocumentColor::default();
    let mut artwork=Artwork::new(extent).unwrap();
    artwork.compositions.get_mut(artwork.root).unwrap().color=color;
    let mut ink=None;
    for layer in 0..28 {
        let (_,target)=crate::test_support::add_paint(&mut artwork,"Cold raster",extent);
        let SourceTarget::Paint(paint)=target else {unreachable!()};
        let tile=Arc::new(blob(color,RasterPlane::Color,0x139a+layer));
        let tiles=(0..extent[1].div_ceil(PAGE_SIZE)).flat_map(|y|(0..extent[0].div_ceil(PAGE_SIZE)).map(move|x|[x,y]))
            .map(|coordinate|(TileKey {plane:RasterPlane::Color,coordinate},RasterTile::backed_shared(tile.clone()))).collect();
        artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(RasterData {tiles,watercolor:None});
        ink=Some(target);
    }
    let mut document=layer_core::Document::from_artwork(artwork).unwrap();document.working.target=ink;
    let mut r=crate::memory::memory_tests::renderer_in_budget(color,
        DeviceMemoryFacts {budget_bytes:Some(8<<30),unified:false,..Default::default()},
        MemoryBudget::with_facts(MemoryFacts {host_default_bytes:Some(8<<30),override_bytes:Some(8<<30),..Default::default()})).unwrap();
    r.ensure_document_metadata(extent,document.scene()).unwrap();
    let mut frame=crate::test_support::packet(document.scene(),extent);
    frame.view=crate::test_support::view([3840,2160]);
    frame.view.document_to_surface=[0.,0.01,-0.01,0.,1920.,1080.];
    r.prepare_raster_restores(frame);
    assert!(r.take_restore_failure().is_none(),"optional restoration must not refuse a valid cold document");
    assert_eq!(r.projected_restore_bytes(),0,"opening a drawing restores no unedited pages");
    assert_eq!(r.source_cache_work(),[0,0],"selection must not decode the cold layers");
}

#[test]
fn ready_restore_requires_consumed_preparation_progress_between_iterations() {
    use std::sync::mpsc;
    use std::time::Duration;

    let color = DocumentColor::default();
    let document = restore_document(color);
    let mut r = crate::memory::memory_tests::renderer_for_restore(&document);
    let source = target(&document);
    r.ensure_document_metadata([9504, 6336], document.scene()).unwrap();
    let first = data(color, 1, &[RasterPlane::Color], 1);
    let previous = r.preparation_progress();
    prepare(&mut r, |r| r.restore_raster(source, &RasterData::default(), &first)).unwrap();
    assert!(r.preparation_progress() > previous);
    let original = live_pixels(&r,&first);

    let compiler = r.compilation.clone();
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    compiler.enqueue(crate::startup::DOCUMENT, move || {
        entered.send(()).unwrap();
        let _ = release_rx.recv();
        Ok(())
    });
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    r.scene_pipelines.source = crate::scene::Pipelines::new(&r.device).source;
    assert!(!r.scene_pipelines.source.pipeline.ready());

    let second = data(color, 1, &[RasterPlane::Color], 2);
    let blob = second.tiles.values().next().unwrap().wait_backing().unwrap();
    let pixels = crate::object_image_mips::prepare_raster_tile(&blob, color.space, &r.device.source_samples);
    r.source_results.push_back((color.space, crate::object_image_mips::PreparedImageWork::Raster(
        crate::object_image_mips::PreparedRasterTile {
            class: crate::residency::Class::Authored, blob, space: color.space, destination: color.space, scalar: false, pixels,
        },
    )));
    assert!(matches!(r.restore_raster(source, &first, &second), Err(GpuRasterError::NotReady)));
    assert!(r.held_result_ready());
    assert!(r.raster_preparation_ready(previous));
    let current = r.preparation_progress();
    for _ in 0..2 {
        assert!(matches!(r.restore_raster(source, &first, &second), Err(GpuRasterError::NotReady)));
        assert_eq!(r.preparation_progress(), current);
        assert!(r.held_result_ready());
        assert!(!r.raster_preparation_ready(current));
    }

    release.send(()).unwrap();
    prepare(&mut r, |r| r.restore_raster(source, &first, &second)).unwrap();
    assert!(r.preparation_progress() > current);
    assert_ne!(live_pixels(&r,&second), original);
}

#[test]
fn native_raster_restore_batches_keep_pixels_reuse_and_late_failure_atomicity() {
    for color in [DocumentColor::default(), DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 }] {
        let document = restore_document(color);
        let mut r = crate::memory::memory_tests::renderer_for_restore(&document);
        let source = target(&document);
        r.ensure_document_metadata([9504, 6336], document.scene()).unwrap();
        let first = data(color, 33, &[RasterPlane::Color, RasterPlane::WatercolorWetness], 1);
        let before = r.metrics.native_restore_submissions;
        prepare(&mut r,|r|r.restore_raster(source, &RasterData::default(), &first)).unwrap();
        assert!((1..=132).contains(&(r.metrics.native_restore_submissions-before)));
        let original = live_pixels(&r,&first);
        assert_eq!(original.len(), 66);
        let before = r.metrics.native_restore_submissions;
        prepare(&mut r,|r|r.restore_raster(source, &RasterData::default(), &first)).unwrap();
        assert!((1..=132).contains(&(r.metrics.native_restore_submissions-before)));
        assert_eq!(original, live_pixels(&r,&first));
        let before = r.metrics.native_restore_submissions;
        prepare(&mut r,|r|r.restore_raster(source, &first, &first)).unwrap();
        assert_eq!(r.metrics.native_restore_submissions, before, "unchanged backing needs no uploads");

        let second = data(color, 33, &[RasterPlane::Color, RasterPlane::WatercolorWetness], 100);
        prepare(&mut r,|r|r.restore_raster(source, &first, &second)).unwrap();
        assert_ne!(original, live_pixels(&r,&second));
        prepare(&mut r,|r|r.restore_raster(source, &second, &first)).unwrap();
        assert_eq!(original, live_pixels(&r,&first), "undo must preserve exact working pixels");
        prepare(&mut r,|r|r.restore_raster(source, &first, &second)).unwrap();
        let previous_pixels = live_pixels(&r,&second);

        // Several valid candidate batches reach the queue before a later digest
        // fails. None may replace a live page, and retry must remain usable.
        let mut corrupt = data(color, 33, &[RasterPlane::Color, RasterPlane::WatercolorWetness], 200);
        let bad = blob(color, RasterPlane::WatercolorWetness, 232);
        let bad=crate::test_support::corrupt_tile(bad);
        corrupt.tiles.insert(TileKey { plane: RasterPlane::WatercolorWetness, coordinate: [10, 2] }, bad);
        let before = r.metrics.native_restore_submissions;
        assert!(prepare(&mut r,|r|r.restore_raster(source,&second,&corrupt)).is_err());
        assert!(r.metrics.native_restore_submissions > before, "failure must occur after an earlier batch was submitted");
        assert_eq!(previous_pixels, live_pixels(&r,&second), "failed candidates changed live artwork");
        prepare(&mut r,|r|r.restore_raster(source, &second, &first)).unwrap();
        assert_eq!(original, live_pixels(&r,&first));

        // Removing a tile must not replace untouched GPU pages or submit work.
        let retained = restored_color(&r,source,&first,[1,0]).value().texture.clone();
        let mut removed = first.clone();
        removed.tiles.remove(&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] });
        let before = r.metrics.native_restore_submissions;
        prepare(&mut r,|r|r.restore_raster(source, &first, &removed)).unwrap();
        assert_eq!(r.metrics.native_restore_submissions, before);
        assert_eq!(r.paint_layers[0].pages.len(), 32);
        assert_eq!(restored_color(&r,source,&removed,[1,0]).value().texture, retained);
        assert!(r.metrics.source_upload_peak_bytes <= 16 * 1024 * 1024);
    }
}

#[test]
fn cached_restore_prefix_is_copied_before_cold_decodes_reuse_slots() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
    let document = restore_document(color);
    let mut r = crate::memory::memory_tests::renderer_for_restore(&document);
    let source = target(&document);
    r.ensure_document_metadata([9504, 6336],document.scene()).unwrap();
    let target = create_color_target(&r.device, [PAGE_SIZE; 2], "cache warmup").0;
    let cached: Vec<_> = (1..=64).map(|seed| Arc::new(blob(color, RasterPlane::Color, seed))).collect();
    for blob in &cached {
        prepare(&mut r,|r|r.restore_native_tiles(&[crate::native_tiles::NativeTileRestore {
            blob, working: &target, space: color.space, destination: color.space,
        }])).unwrap();
    }
    let mixed = RasterData {
        tiles: (0..80).map(|i| (
            TileKey { plane: RasterPlane::Color, coordinate: [i / 25, i % 25] },
            if i < 8 { RasterTile::backed_shared(cached[i as usize].clone()) }
                else { RasterTile::backed(blob(color, RasterPlane::Color, i + 100)) },
        )).collect(),
        watercolor: None,
    };
    let before = r.metrics.native_restore_submissions;
    prepare(&mut r,|r|r.restore_raster(source, &RasterData::default(), &mixed)).unwrap();
    assert!((1..=160).contains(&(r.metrics.native_restore_submissions-before)));
    for page in &r.paint_layers[0].pages {
        let index = page.coordinate[0] * 25 + page.coordinate[1];
        let seed = if index < 8 { index + 1 } else { index + 100 };
        let expected: [f32; 4] = std::array::from_fn(|channel| if channel == 3 { 1. } else {
            color.space.decode(f64::from((seed * 251 + channel as u32 * 71) & 65535) / 65535.) as f32
        });
        for pixel in crate::layer_tests::page_bytes(&r, &restored_color(&r,source,&mixed,page.coordinate).value().texture).as_chunks::<16>().0.iter() {
            for channel in 0..4 {
                let actual = f32::from_le_bytes(pixel[channel * 4..channel * 4 + 4].try_into().unwrap());
                assert!((actual - expected[channel]).abs() < 2e-7, "tile {index}, channel {channel}");
            }
        }
    }
}

#[test]
fn native_mask_restoration_keeps_existing_scalar_path() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let mut document = restore_document(color);
    set_mask(&mut document,reveal_all([9504,6336], [0, 0]));
    let source = SourceTarget::Coverage(document.scene().mask(document.scene().order()[0]).unwrap().0.source);
    r.ensure_document_metadata([9504, 6336],document.scene()).unwrap();
    let first = data(color, 33, &[RasterPlane::Mask], 1);
    let before = r.metrics.native_restore_submissions;
    let decoded = |r: &WgpuRasterizer| (r.source_tiles.borrow().gpu_bytes(), r.source_tiles.borrow().misses);
    let slots = decoded(&r);
    prepare(&mut r,|r|r.restore_raster(source, &RasterData::default(), &first)).unwrap();
    assert!((1..=66).contains(&(r.metrics.native_restore_submissions-before)));
    assert_eq!(decoded(&r), slots, "restored mask tiles occupied decoded source slots");
    assert_eq!(r.layer_masks.pages.len(), 33);
    for (key, tile) in &first.tiles {
        let expected = tile.wait_backing().unwrap().decode_on_worker().unwrap();
        let actual = crate::layer_tests::page_bytes(&r, &r.layer_masks.pages[&(source,key.coordinate)].read(tile.content_version(),None).unwrap().value().texture);
        for (actual, expected) in actual.as_chunks::<4>().0.iter().zip(expected.as_chunks::<2>().0.iter()) {
            assert_eq!(f32::from_le_bytes(*actual),
                f32::from(u16::from_le_bytes(*expected)) / 65535.);
        }
    }
}

#[test]
fn a_native_restore_inside_a_frame_leaves_the_frame_uploads_submittable() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    r.source_tiles.get_mut().admit_limits(16 << 20);
    let document = restore_document(color);
    r.ensure_document_metadata([9504, 6336], document.scene()).unwrap();
    let working = create_color_target(&r.device, [PAGE_SIZE; 2], "restore candidate").0;
    let page = create_color_target(&r.device, [PAGE_SIZE; 2], "frame upload").0;
    let pixels: Vec<u8> = (0..PAGE_SIZE.pow(2)).flat_map(|_| [0.25f32, 0.5, 0.75, 1.].into_iter().flat_map(f32::to_le_bytes)).collect();
    let mut frame = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.uploads.write_texture(&mut frame, &page, PAGE_SIZE * 16, |mapped| mapped.copy_from_slice(&pixels)).unwrap();
    let restored = Arc::new(blob(color, RasterPlane::Color, 3));
    prepare(&mut r,|r|r.restore_native_tiles(&[crate::native_tiles::NativeTileRestore { blob: &restored, working: &working, space: color.space, destination: color.space }])).unwrap();
    r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    r.uploads.finish(&frame);
    frame.submit(&r.queue);
    assert_eq!(crate::layer_tests::page_bytes(&r, &page), pixels, "the frame's upload survives a restore submitted before it");
}

fn finish(engine:&mut layer_engine::CanvasEngine<WgpuRasterizer>) {
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
    loop {
        engine.render_frame_at(0).unwrap();engine.backend_mut().wait_idle().unwrap();
        if !engine.has_pending_document_edits() && !engine.raster_backing_pending() && engine.backend().next_frame()!=layer_render::FrameDemand::Work {return;}
        assert!(std::time::Instant::now()<deadline,"prepared frame did not settle");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn a_single_bake_page_keeps_more_than_sixty_four_distinct_shifted_sources() {
    use layer_core::authored::{Occurrence,OccurrenceContent,PaintBase,PaintSource,PortableId};
    let color=DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::U16};
    let extent=[16;2];let mut document=paint_document(extent,"Sources");
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color=color;
    let root=document.composition().result;
    let codes=|seed:u32,x:u32,y:u32|[(seed*11+x/256*17) as u8,(seed*7+y/256*23) as u8,(seed*5+x/256*3+y/256*13) as u8,40];
    let mut entries=Vec::new();
    for seed in 0..20 {
        let image=layer_core::color::source::rgba8_source([272;2],|x,y|codes(seed,x,y));
        let paint=document.artwork.paint.insert(PortableId::random(),PaintSource {domain:[272;2],base:Some(PaintBase::new(image.into())),
            color_mode:Default::default(),raster:Default::default(),operations:Default::default()}).unwrap();
        let mut occurrence=Occurrence::new(OccurrenceContent::Paint(paint),"Source");occurrence.offset=[-255;2];
        entries.push(document.artwork.occurrences.insert(PortableId::random(),occurrence).unwrap());
    }
    document.artwork.stacks.get_mut(root).unwrap().entries=entries;
    let mut document=layer_core::Document::from_artwork(document.artwork).unwrap();
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().blend=layer_core::BlendSpace::Linear;
    let gpu=WgpuRasterizer::new_native_headless(color).unwrap();
    let mut engine=layer_engine::CanvasEngine::new(gpu,document,layer_engine::input_queue(8).1,crate::test_support::view(extent),layer_engine::ViewTransform::IDENTITY).unwrap();
    let plan=engine.document().merge_plan(layer_core::MergeKind::Visible).unwrap();let target=plan.target;
    engine.insert_with_operations(plan.edits,vec![(target,plan.operation)],None).unwrap();
    finish(&mut engine);
    let page=&engine.backend().paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter().find(|page|page.coordinate==[1;2]).unwrap();
    let selected=engine.document().scene().raster(target).unwrap().wait_data().unwrap();
    let pixels=crate::layer_tests::page_bytes(engine.backend(),&restored_color(engine.backend(),target,&selected,page.coordinate).value().texture);
    for [x,y] in [[0,0],[1,0],[0,1],[1,1],[15,15]] {
        let mut expected=[0.;4];
        for seed in (0..20).rev() {
            let sample=codes(seed,x+255,y+255);let alpha=f64::from(sample[3])/255.;
            for channel in 0..3 {expected[channel]=color.space.decode(f64::from(sample[channel])/255.)*alpha+expected[channel]*(1.-alpha);}
            expected[3]=alpha+expected[3]*(1.-alpha);
        }
        for (channel,expected) in expected.into_iter().enumerate() {
            let at=((y*PAGE_SIZE+x)*16) as usize+channel*4;
            let actual=f32::from_le_bytes(pixels[at..at+4].try_into().unwrap());
            assert!((f64::from(actual)-expected).abs()<2e-5,"{x},{y} channel {channel}: {actual} != {expected}");
        }
    }
    let original=pixels;assert!(engine.undo().unwrap());finish(&mut engine);
    assert!(engine.redo().unwrap());finish(&mut engine);
    let page=&engine.backend().paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter().find(|page|page.coordinate==[1;2]).unwrap();
    let selected=engine.document().scene().raster(target).unwrap().wait_data().unwrap();
    assert_eq!(original,crate::layer_tests::page_bytes(engine.backend(),&restored_color(engine.backend(),target,&selected,page.coordinate).value().texture));
}

#[test]
fn a_late_bake_source_failure_is_admitted_before_any_authored_frame_work() {
    use layer_core::authored::{PaintBase,SceneScope};
    let extent=[512,256];let color=DocumentColor::default();
    let mut image=Arc::try_unwrap(layer_core::color::source::rgba8_source(extent,|_,_|[33,99,177,255])).unwrap();
    let blob=Arc::try_unwrap(image.tiles.remove(&[1,0]).unwrap()).unwrap();
    let invalid=crate::test_support::corrupt_tile(blob).wait_backing().unwrap();image.tiles.insert([1,0],invalid);
    let mut frozen=paint_document(extent,"Frozen source");
    crate::layer_tests::placement::paint_mut(&mut frozen).base=Some(PaintBase::new(Arc::new(image).into()));
    let mut document=paint_document(extent,"Live destination");let target=target(&document);
    let coverage=layer_core::CoverageSnapshot::reveal_all(document.artwork.coverage.next_handle(),extent,[0;2]);
    let operation=layer_core::RasterOperation {placement:layer_core::Affine::IDENTITY,coverage,
        kind:layer_core::RasterOperationKind::Bake {scene:Arc::new(frozen.scene().snapshot()),scope:SceneScope::All,offset:Default::default()}};
    crate::layer_tests::placement::paint_mut(&mut document).operations=Arc::new(vec![operation]);
    let operations=document.scene().operations(target).unwrap().to_vec();
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.ensure_document_metadata(extent,document.scene()).unwrap();
    prepare(&mut r,|r|r.reconcile_rasters(crate::test_support::packet(document.scene(),extent),false,&[])).unwrap();
    let mut page=r.create_page([0;2],"retained live pixels");
    let original=[0.125f32,0.25,0.5,1.].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>().repeat(PAGE_SIZE.pow(2) as usize);
    let live=layer_core::raster::ContentName::fresh();
    let intent=r.raster_intent(target,RasterPlane::Color,[0;2]).unwrap();
    let mut commands=submission::CommandEncoder::new(&r.device,&Default::default());let receipt=commands.submission();
    let initialize=page.primary.initialize(&receipt).unwrap();
    {let _pass=commands.color_pass("empty selected paint",&initialize.value().view,wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));}
    initialize.publish(intent.version());
    let write=page.primary.write(&intent,&receipt).unwrap();
    r.uploads.write_texture(&mut commands,&write.value().texture,PAGE_SIZE*16,|mapped|mapped.copy_from_slice(&original)).unwrap();
    r.record_raster_write(target,write.publish(live)).unwrap();page.primary_needs_clear=false;
    r.uploads.finish(&commands);commands.submit(&r.queue);crate::test_support::complete(&r);
    r.paint_layers.iter_mut().find(|layer|layer.id==target).unwrap().pages.push(page);
    let texture=r.read_layer_color(target,[0;2],&live.into(),None).unwrap().value().texture.clone();
    let batch=DabBatch {kind:DabBatchKind::RasterOperation(0),dab_count:0,..crate::test_support::dab_batch(target,
        crate::tests::test_style(BrushExecution::Dry),layer_core::Rect::from_extent(extent))};
    let batches=[batch];let packet=FramePacket {dab_batches:&batches,..crate::test_support::packet(document.scene(),extent)};
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);let mut first_ready=false;
    while !r.raster_dependencies_ready(packet,None) {
        first_ready|=r.prepared_bake(target,0,[0;2]).is_some();
        assert_eq!(r.read_layer_color(target,[0;2],&live.into(),None).unwrap().value().texture,texture);
        assert!(std::time::Instant::now()<deadline,"invalid source did not resolve its preparation");
        r.wait_idle().unwrap();std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(first_ready,"a complete private page must precede the invalid source");
    let submissions=r.metrics.submissions;assert!(r.submit(packet).is_err());assert_eq!(r.metrics.submissions,submissions);
    assert_eq!(r.read_layer_color(target,[0;2],&live.into(),None).unwrap().value().texture,texture);
    assert_eq!(crate::layer_tests::page_bytes(&r,&r.read_layer_color(target,[0;2],&live.into(),None).unwrap().value().texture),original);
    assert_eq!(document.scene().operations(target).unwrap(),operations);
    assert!(document.scene().raster(target).unwrap().try_data().unwrap().unwrap().tiles.is_empty());
}

#[test]
fn replacement_overlap_is_rejected_before_any_private_page_upload() {
    let color=DocumentColor::default();let document=paint_document([16000;2],"overlap");let source=target(&document);
    let mut r=crate::memory::memory_tests::renderer(color, layer_core::DeviceMemoryFacts {unified:true,..Default::default()}).unwrap();
    let budget=r.factory.memory_budget().clone();budget.set_ram_override(Some(384<<20));
    r.prepare_capture_memory(document.scene(), document.composition().size);
    r.ensure_document_metadata(document.composition().size,document.scene()).unwrap();
    let first=data(color,1,&[RasterPlane::Color],1);
    prepare(&mut r,|r|r.restore_raster(source,&RasterData::default(),&first)).unwrap();
    let original=live_pixels(&r,&first);let texture=restored_color(&r,source,&first,[0;2]).value().texture.clone();
    let limit=budget.ram_limit_bytes();
    let bytes=u64::from(PAGE_SIZE).pow(2)*2*u64::from(r.device.scalar_format().block_copy_size(None).unwrap());
    assert_eq!(limit%bytes,0);
    let tile=RasterTile::backed(blob(color,RasterPlane::WatercolorWetness,2));
    let candidate=RasterData {tiles:(0..limit/bytes).map(|i|(TileKey {plane:RasterPlane::WatercolorWetness,coordinate:[(i%36) as u32,(i/36) as u32]},tile.clone())).collect(),watercolor:Some(layer_core::raster::RasterWatercolor {wet_edge:0.5,burnt_edge:0.5,edge_width:2.})};
    let submissions=r.metrics.native_restore_submissions;
    let result=r.restore_raster(source,&first,&candidate);
    assert!(matches!(&result,Err(GpuRasterError::SourceWorkingSetExceeded)),"{result:?}");
    assert_eq!(r.metrics.native_restore_submissions,submissions);
    assert_eq!(restored_color(&r,source,&first,[0;2]).value().texture,texture);assert_eq!(live_pixels(&r,&first),original);
    let replacement=data(color,1,&[RasterPlane::Color],3);
    prepare(&mut r,|r|r.restore_raster(source,&first,&replacement)).unwrap();
    assert_ne!(live_pixels(&r,&replacement),original);
}

#[test]
fn an_empty_restore_cohort_is_consumed_even_when_it_has_no_candidate_pages() {
    let document=paint_document([64;2],"empty reset");let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let frame=FramePacket {reset_layers:true,..crate::test_support::packet(document.scene(),[64;2])};
    for _ in 0..3 {
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while !r.raster_dependencies_ready(frame,None) {
            assert!(std::time::Instant::now()<deadline);r.wait_idle().unwrap();std::thread::sleep(std::time::Duration::from_millis(1));
        }
        r.submit(frame).unwrap();r.wait_idle().unwrap();
        assert!(r.paint_layers.iter().all(|layer|layer.pages.is_empty()));
        assert!(r.raster.as_ref().unwrap().targets.values().all(|target|target.contents.data().tiles.is_empty()));
    }
}
#[test]
fn renderer_recreation_restores_pixels_without_changing_artwork_revision() {
    use layer_render::CanvasRenderer;
    let extent=[256;2];
    let mut document=paint_document(extent,"Recreated native paint");
    let color=document.composition().color;
    crate::layer_tests::placement::paint_mut(&mut document).raster=
        RasterRevision::backed(data(color,1,&[RasterPlane::Color],17));
    let frame=FramePacket {reset_layers:true,..crate::test_support::packet(document.scene(),extent)};
    let mut previous=WgpuRasterizer::new_native_headless(color).unwrap();
    crate::test_support::submit_published(&mut previous,frame);
    let expected=crate::scene::scale::tests::display_pixels(&previous);
    assert!(expected.iter().any(|pixel|pixel[0]>0. && pixel[3]>0.));
    let artwork_revision=previous.artwork_revision;
    let composite_revision=previous.composite_revision;
    let factory=previous.factory.clone();
    let mut renderer=WgpuRasterizer::from_factory_inner(factory.clone(),factory.pipeline_device(color),Initialization::Headless).unwrap();
    renderer.initialize_native(color).unwrap();
    renderer.adopt_committed_pages(&mut previous);
    assert_eq!(renderer.artwork_revision,artwork_revision);
    crate::test_support::submit_published(&mut renderer,frame);
    assert_eq!(crate::scene::scale::tests::display_pixels(&renderer),expected);
    assert!(renderer.composite_revision>composite_revision,"restored storage must publish composition pixels");
    assert_eq!(renderer.artwork_revision,artwork_revision,"physical restoration of unchanged authored tiles is not an artwork edit");
    let target=target(&document);
    renderer.set_transform_preview(Some(&layer_render::TransformPreview {
        transaction:1,target,moving:false,selection:None,
        transform:layer_core::ImageTransform::affine(layer_core::Affine::translation(layer_core::Point {x:16.,y:0.})),
    })).unwrap();
    crate::test_support::submit_published(&mut renderer,frame);
    assert!(renderer.artwork_revision>artwork_revision,"a real transform preview still changes artwork during a reset");
    let preview_revision=renderer.artwork_revision;
    renderer.set_transform_preview(None).unwrap();
    crate::test_support::submit_published(&mut renderer,frame);
    assert!(renderer.artwork_revision>preview_revision,"cancelling the transform still changes artwork during a reset");
}

#[test]
fn display_damage_republishes_pixels_without_an_artwork_edit() {
    let extent=[96;2];let mut document=paint_document(extent,"Retained picture");
    crate::layer_tests::placement::paint_mut(&mut document).base=Some(layer_core::authored::PaintBase::new(
        layer_core::color::source::rgba8_source(extent,|_,_|[31,71,113,255]).into()));
    let mut renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let frame=crate::test_support::packet(document.scene(),extent);
    crate::test_support::submit_published(&mut renderer,frame);
    let (artwork,composite)=(renderer.artwork_revision,renderer.composite_revision);
    let expected=crate::scene::scale::tests::display_pixels(&renderer);
    renderer.document_damage.push(PixelRect::full(extent));
    crate::test_support::submit_published(&mut renderer,frame);
    assert_eq!(crate::scene::scale::tests::display_pixels(&renderer),expected);
    assert!(renderer.composite_revision>composite);
    assert_eq!(renderer.artwork_revision,artwork,"derived display damage must not look like an authored edit");
}
