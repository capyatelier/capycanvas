use super::*;

#[test]
fn cold_reduced_preview_retains_its_cursor_across_more_than_sixty_four_sources() {
    let extent=[5120,1024];
    let source=layer_core::color::source::rgba8_source(extent,|_,_|[255,0,0,255]);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    r.source_tiles.borrow_mut().admit_limits(16 << 20);
    pollster::block_on(r.compilation.prepare_all([&r.recipes.display.reduce,&r.recipes.display.fused_reduce])).unwrap();
    let mut state=ImageTransformState::new(&r.device);
    state.sources[0]=Some(TileSnapshot {pages:Default::default(),base:Some(layer_core::authored::PaintBase::new(source.clone().into())),
        backing:None,bounds:PixelRect::full(extent)});
    let deadline=std::time::Instant::now()+Duration::from_secs(30);
    loop {
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let result=state.prepare_reduced(&mut r,&mut encoder,None,None,extent,1);
        r.uploads.finish(&encoder);encoder.submit(&r.queue);
        match result {
            Ok(())=>break,
            Err(GpuRasterError::NotReady)=>{
                assert!(state.reduced.as_ref().unwrap().pending.is_some());
                assert!(std::time::Instant::now()<deadline);
                r.wait_idle().unwrap();std::thread::sleep(Duration::from_millis(1));
            },
            Err(error)=>panic!("{error}"),
        }
    }
    assert!(state.reduced.as_ref().unwrap().pending.is_none());
    assert!(r.paint_layers.is_empty());
    let image=&state.reduced.as_ref().unwrap().image;
    let pixels=crate::layer_tests::page_bytes(&r,&image.texture);
    for coordinate in page_coordinates(PixelRect::full(extent)) {
        let x=coordinate[0]*PAGE_SIZE/2+32;let y=coordinate[1]*PAGE_SIZE/2+32;
        let offset=((y*image.texture.width()+x)*16) as usize;
        let value=std::array::from_fn::<_,4,_>(|channel|f32::from_le_bytes(pixels[offset+channel*4..offset+channel*4+4].try_into().unwrap()));
        assert_eq!(value, [1.,0.,0.,1.]);
    }
    state.release_snapshot();
}
