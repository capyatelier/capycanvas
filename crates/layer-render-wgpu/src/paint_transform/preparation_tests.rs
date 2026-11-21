use super::*;
impl PaintTransforms {
    pub(crate) fn bound_color_pipeline(&self) -> (String, Deferred<wgpu::RenderPipeline>) {
        self.0[0].color.bound_pipeline()
    }
    pub(crate) fn hold_spare(&mut self, page: LayerPage) { self.0[0].spares.paint.push(page); }
    pub(crate) fn storage_bytes(&self) -> u64 {
        self.0.iter().map(ImageTransformState::storage_bytes).sum::<u64>() - 3 * 48
    }
}

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
    crate::test_support::wait_until("the reduced preview finishes preparing",||
        match crate::test_support::submit_encoded(&mut r,|r,encoder|state.prepare_reduced(r,encoder,None,None,extent,1)) {
            Err(pending@GpuRasterError::NotReady)=>{
                assert!(state.reduced.as_ref().unwrap().pending.is_some());
                r.wait_idle().unwrap();Err(pending)
            },
            result=>Ok(result.unwrap()),
        });
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
