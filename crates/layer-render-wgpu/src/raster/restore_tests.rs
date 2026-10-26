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

fn live_pixels(r: &WgpuRasterizer) -> Vec<(RasterPlane, [u32; 2], Vec<u8>)> {
    let layer = &r.paint_layers[0];
    layer.pages.iter().map(|p| (RasterPlane::Color, p.coordinate,
        crate::layer_tests::page_bytes(r, &p.primary.texture)))
        .chain(layer.watercolor_wetness_pages.iter().map(|p| (RasterPlane::WatercolorWetness, p.coordinate,
            crate::layer_tests::page_bytes(r, &p.active().texture)))).collect()
}

#[test]
fn native_raster_restore_batches_keep_pixels_reuse_and_late_failure_atomicity() {
    for color in [DocumentColor::default(), DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 }] {
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        r.source_tiles.get_mut().admit(0);
        let document = restore_document(color);
        let source = target(&document);
        r.ensure_document_metadata([9504, 6336], document.scene()).unwrap();
        let first = data(color, 33, &[RasterPlane::Color, RasterPlane::WatercolorWetness], 1);
        let before = r.metrics.native_restore_submissions;
        prepare(&mut r,|r|r.restore_raster(source, &RasterData::default(), &first)).unwrap();
        assert!((1..=132).contains(&(r.metrics.native_restore_submissions-before)));
        let original = live_pixels(&r);
        assert_eq!(original.len(), 66);
        let before = r.metrics.native_restore_submissions;
        prepare(&mut r,|r|r.restore_raster(source, &RasterData::default(), &first)).unwrap();
        assert!((1..=132).contains(&(r.metrics.native_restore_submissions-before)));
        assert_eq!(original, live_pixels(&r));
        let before = r.metrics.native_restore_submissions;
        prepare(&mut r,|r|r.restore_raster(source, &first, &first)).unwrap();
        assert_eq!(r.metrics.native_restore_submissions, before, "unchanged backing needs no uploads");

        let second = data(color, 33, &[RasterPlane::Color, RasterPlane::WatercolorWetness], 100);
        prepare(&mut r,|r|r.restore_raster(source, &first, &second)).unwrap();
        assert_ne!(original, live_pixels(&r));
        prepare(&mut r,|r|r.restore_raster(source, &second, &first)).unwrap();
        assert_eq!(original, live_pixels(&r), "undo must preserve exact working pixels");
        prepare(&mut r,|r|r.restore_raster(source, &first, &second)).unwrap();
        let previous_pixels = live_pixels(&r);

        // Several valid candidate batches reach the queue before a later digest
        // fails. None may replace a live page, and retry must remain usable.
        let mut corrupt = data(color, 33, &[RasterPlane::Color, RasterPlane::WatercolorWetness], 200);
        let bad = blob(color, RasterPlane::WatercolorWetness, 232);
        let bad=crate::test_support::corrupt_tile(bad);
        corrupt.tiles.insert(TileKey { plane: RasterPlane::WatercolorWetness, coordinate: [10, 2] }, bad);
        let before = r.metrics.native_restore_submissions;
        assert!(prepare(&mut r,|r|r.restore_raster(source,&second,&corrupt)).is_err());
        assert!(r.metrics.native_restore_submissions > before, "failure must occur after an earlier batch was submitted");
        assert_eq!(previous_pixels, live_pixels(&r), "failed candidates changed live artwork");
        prepare(&mut r,|r|r.restore_raster(source, &second, &first)).unwrap();
        assert_eq!(original, live_pixels(&r));

        // Removing a tile must not replace untouched GPU pages or submit work.
        let retained = r.paint_layers[0].pages.iter().find(|p| p.coordinate == [1, 0]).unwrap().primary.texture.clone();
        let mut removed = first.clone();
        removed.tiles.remove(&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] });
        let before = r.metrics.native_restore_submissions;
        prepare(&mut r,|r|r.restore_raster(source, &first, &removed)).unwrap();
        assert_eq!(r.metrics.native_restore_submissions, before);
        assert_eq!(r.paint_layers[0].pages.len(), 32);
        assert_eq!(r.paint_layers[0].pages.iter().find(|p| p.coordinate == [1, 0]).unwrap().primary.texture, retained);
        assert!(r.metrics.source_upload_peak_bytes <= 16 * 1024 * 1024);
    }
}

#[test]
fn cached_restore_prefix_is_copied_before_cold_decodes_reuse_slots() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    r.source_tiles.get_mut().admit(0);
    let document = restore_document(color);
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
        for pixel in crate::layer_tests::page_bytes(&r, &page.primary.texture).as_chunks::<16>().0.iter() {
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
    prepare(&mut r,|r|r.restore_raster(source, &RasterData::default(), &first)).unwrap();
    assert!((1..=66).contains(&(r.metrics.native_restore_submissions-before)));
    assert_eq!(r.layer_masks.pages.len(), 33);
    for (key, tile) in &first.tiles {
        let expected = tile.wait_backing().unwrap().decode().unwrap();
        let actual = crate::layer_tests::page_bytes(&r, &r.layer_masks.pages[&(source, key.coordinate)].texture);
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
    r.source_tiles.get_mut().admit(0);
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
        if !engine.has_pending_document_edits() && !engine.raster_backing_pending() && !engine.backend().has_pending_work() {return;}
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
    let pixels=crate::layer_tests::page_bytes(engine.backend(),&page.active().texture);
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
    assert_eq!(original,crate::layer_tests::page_bytes(engine.backend(),&page.active().texture));
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
    let mut page=r.create_page([0;2],"retained live pixels");page.primary_needs_clear=false;
    let original=[0.125f32,0.25,0.5,1.].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>().repeat(PAGE_SIZE.pow(2) as usize);
    crate::test_support::upload_page(&r,&page.primary.texture,&original);let texture=page.primary.texture.clone();
    r.paint_layers.iter_mut().find(|layer|layer.id==target).unwrap().pages.push(page);
    let batch=DabBatch {kind:DabBatchKind::RasterOperation(0),dab_count:0,..crate::test_support::dab_batch(target,
        crate::tests::test_style(BrushExecution::Dry),layer_core::Rect::from_extent(extent))};
    let batches=[batch];let packet=FramePacket {dab_batches:&batches,..crate::test_support::packet(document.scene(),extent)};
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);let mut first_ready=false;
    while !r.raster_dependencies_ready(packet,None) {
        first_ready|=r.prepared_bake(target,0,[0;2]).is_some();
        assert_eq!(r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages[0].primary.texture,texture);
        assert!(std::time::Instant::now()<deadline,"invalid source did not resolve its preparation");
        r.wait_idle().unwrap();std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(first_ready,"a complete private page must precede the invalid source");
    let submissions=r.metrics.submissions;assert!(r.submit(packet).is_err());assert_eq!(r.metrics.submissions,submissions);
    assert_eq!(r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages[0].primary.texture,texture);
    assert_eq!(crate::layer_tests::page_bytes(&r,&texture),original);
    assert_eq!(document.scene().operations(target).unwrap(),operations);
    assert!(document.scene().raster(target).unwrap().try_data().unwrap().unwrap().tiles.is_empty());
}

#[test]
fn replacement_overlap_is_rejected_before_any_private_page_upload() {
    let color=DocumentColor::default();let document=paint_document([16000;2],"overlap");let source=target(&document);
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();
    r.ensure_document_metadata(document.composition().size,document.scene()).unwrap();
    let first=data(color,1,&[RasterPlane::Color],1);
    prepare(&mut r,|r|r.restore_raster(source,&RasterData::default(),&first)).unwrap();
    let original=live_pixels(&r);let texture=r.paint_layers[0].pages[0].active().texture.clone();
    let native=r.native_edit.as_mut().unwrap();native.composition_bytes=0;native.color_cache_bytes=0;
    let limit=native.image_pixel_allowance();
    let bytes=u64::from(PAGE_SIZE).pow(2)*2*u64::from(r.device.scalar_format().block_copy_size(None).unwrap());
    assert_eq!(limit%bytes,0);
    let tile=RasterTile::backed(blob(color,RasterPlane::WatercolorWetness,2));
    let candidate=RasterData {tiles:(0..limit/bytes).map(|i|(TileKey {plane:RasterPlane::WatercolorWetness,coordinate:[(i%36) as u32,(i/36) as u32]},tile.clone())).collect(),watercolor:Some(layer_core::raster::RasterWatercolor {wet_edge:0.5,burnt_edge:0.5,edge_width:2.})};
    let submissions=r.metrics.native_restore_submissions;
    assert!(matches!(r.restore_raster(source,&first,&candidate),Err(GpuRasterError::SourceWorkingSetExceeded)));
    assert_eq!(r.metrics.native_restore_submissions,submissions);
    assert_eq!(r.paint_layers[0].pages[0].active().texture,texture);assert_eq!(live_pixels(&r),original);
    let replacement=data(color,1,&[RasterPlane::Color],3);
    prepare(&mut r,|r|r.restore_raster(source,&first,&replacement)).unwrap();
    assert_ne!(live_pixels(&r),original);
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
        assert!(r.raster.as_ref().unwrap().targets.values().all(|target|target.data.tiles.is_empty()));
    }
}
