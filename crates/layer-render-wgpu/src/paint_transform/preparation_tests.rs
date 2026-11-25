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
            Err(pending@(GpuRasterError::NotReady|GpuRasterError::Waiting(_)))=>{
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

const SUBTEXEL_ROUNDING: f32 = 2. / 256.;

fn preview_pixels(r: &mut WgpuRasterizer, state: &mut ImageTransformState, preview: &layer_render::TransformPreview,
    extent: [u32; 2], layer_extent: [u32; 2], offset: [i64; 2]) -> Vec<Vec<[f32; 4]>> {
    let placement = layer_core::Affine::translation(layer_core::Point { x: offset[0] as f32, y: offset[1] as f32 });
    [1, 3, 6].into_iter().map(|level| {
        let target = display_mips::Plan::window(extent, level, PixelRect::full(extent));
        let output = create_target(&r.device, target.size, wgpu::TextureFormat::Rgba32Float, "transform preview from a reduced input");
        let display = pixel_transform::DisplayLevel { side: 1 << level, opacity: 1., extent, backdrop: [0.; 4], encode: false };
        let region = crate::test_support::submit_encoded(r, |r, encoder|
            state.prepare_region(r, encoder, preview, layer_extent, placement, &output.1, display, target, target.doc_bounds, 0)).unwrap();
        super::prepared_region_tests::encode(r, &[region]).submit(&r.queue);
        crate::test_support::complete(r);
        crate::test_support::floats(&crate::layer_tests::page_bytes(r, &output.0))
    }).collect()
}

fn reduce_until_ready(r: &mut WgpuRasterizer, message: &str, mut step: impl FnMut(&mut WgpuRasterizer, &mut submission::CommandEncoder) -> Result<(), GpuRasterError>) {
    crate::test_support::wait_until(message, || match crate::test_support::submit_encoded(r, &mut step) {
        Err(GpuRasterError::NotReady|GpuRasterError::Waiting(_)) => { crate::test_support::complete(r); Err(GpuRasterError::NotReady) },
        result => { assert!(result.is_ok(), "{message}: {result:?}"); Ok(()) },
    });
}

fn cropped_against_full_extent(extent: [u32; 2], pages: &[[u32; 2]], offset: [i64; 2], selection: Option<layer_core::Selection>, level: u32)
    -> ([PixelRect; 2], [u64; 2], f32) {
    let mut doc = crate::layer_tests::placement::paint_document(extent, "Sparse");
    crate::layer_tests::placement::paint_mut(&mut doc).raster = crate::test_support::tiled_raster(doc.composition().color, pages);
    crate::layer_tests::placement::occurrence_mut(&mut doc).offset = offset;
    let (scene, target) = (doc.scene(), crate::layer_tests::placement::target(&doc));
    let layer_extent = scene.target_extent(target);
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r, crate::test_support::packet(scene, extent));
    let mut transforms = PaintTransforms::new(&r.device);
    let exact = transforms.0[0].color.display_pipeline();
    pollster::block_on(r.compilation.prepare_all([&r.recipes.display.reduce, &r.recipes.display.fused_reduce, &exact]
        .into_iter().chain(r.selection_clip.pipelines()))).unwrap();
    pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.resample.mesh(wgpu::TextureFormat::Rgba32Float).iter())).unwrap();
    let [x, y] = pages[0].map(|v| (v * PAGE_SIZE + PAGE_SIZE / 2) as f32);
    let preview = layer_render::TransformPreview { transaction: 1, target, moving: true, selection: selection.clone(),
        transform: layer_core::ImageTransform::affine(layer_core::Affine::around(layer_core::Point { x, y }, [0.9; 2], 0.3,
            layer_core::Point { x: 37.5, y: -21.25 })) };
    reduce_until_ready(&mut r, "the cropped transform input reduces", |r, encoder| transforms.prepare_preview(r, encoder, &preview, scene, level, false));
    let predicted = transforms.input_requirements(&preview, level, scene).1;
    let state = transforms.0[0].preparing.as_mut().unwrap();
    let cropped = preview_pixels(&mut r, state, &preview, extent, layer_extent, offset);
    let input = state.reduced.take().unwrap();
    let windows = [input.image.plan.bounds, input.kept.as_ref().map_or(PixelRect::EMPTY, |kept| kept.plan.bounds)];
    assert_eq!(predicted, windows, "preparation reserves the windows it allocates");
    let full = PixelRect::full([layer_extent[0] + input.phase[0], layer_extent[1] + input.phase[1]]);
    let mut reference = snapshot::DisplayInputs::new(&r, input.level, layer_extent, input.phase,
        [full, if input.kept.is_some() { full } else { PixelRect::EMPTY }]);
    reference.pending = Some(state.display_windows(selection.as_ref(), layer_extent, input.phase).0);
    state.reduced = Some(reference);
    reduce_until_ready(&mut r, "the full-extent transform input reduces",
        |r, encoder| state.prepare_reduced(r, encoder, selection.as_ref(), Some((scene, target)), layer_extent, input.level));
    let expected = preview_pixels(&mut r, state, &preview, extent, layer_extent, offset);
    let difference = cropped.iter().flatten().zip(expected.iter().flatten())
        .flat_map(|(a, b)| (0..4).map(move |channel| (a[channel] - b[channel]).abs())).fold(0., f32::max);
    (windows, [input.storage_bytes(), state.reduced.as_ref().unwrap().storage_bytes()], difference)
}

#[test]
fn a_central_island_reduces_only_its_surrounding_pages_and_previews_as_the_full_extent() {
    let (windows, [cropped, full], difference) = cropped_against_full_extent([6000, 4000], &[[11, 7]], [0; 2], None, 1);
    assert_eq!(windows, [PixelRect::new(2560, 1536, 3328, 2304), PixelRect::EMPTY]);
    assert!(cropped * 10 <= full, "an island keeps at most a tenth of the full pyramid: {cropped} of {full} bytes");
    assert!(difference <= SUBTEXEL_ROUNDING, "{difference}");
    assert_eq!(cropped_against_full_extent([8192, 4096], &[[0, 0]], [0; 2], None, 1).2, 0.,
        "texture coordinates normalize exactly on power-of-two pyramids");
}

#[test]
fn opposite_corners_reduce_the_full_extent_and_never_more() {
    let (windows, [cropped, full], difference) = cropped_against_full_extent([6000, 4000], &[[0, 0], [23, 15]], [0; 2], None, 1);
    assert_eq!(windows, [PixelRect::full([6000, 4000]), PixelRect::EMPTY]);
    assert_eq!(cropped, full);
    assert_eq!(difference, 0.);
}

#[test]
fn kept_selections_bound_their_moved_and_kept_pixels_separately() {
    let selection = layer_core::Selection::polygon([[1300., 1050.], [1500., 1050.], [1300., 1250.]]
        .map(|[x, y]| layer_core::Point { x, y }).to_vec()).unwrap();
    let (windows, [cropped, full], difference) = cropped_against_full_extent([6000, 4000], &[[5, 4], [17, 10]], [0; 2], Some(selection), 2);
    assert_eq!(windows, [PixelRect::new(1024, 768, 1792, 1536), PixelRect::new(1024, 768, 4864, 3072)]);
    assert!(cropped < full);
    assert!(difference <= SUBTEXEL_ROUNDING, "{difference}");
}

#[test]
fn phased_islands_reduce_the_pages_of_the_shifted_grid() {
    let (windows, [cropped, full], difference) = cropped_against_full_extent([6000, 4000], &[[11, 7]], [100, 37], None, 1);
    assert_eq!(windows, [PixelRect::new(2560, 1536, 3584, 2560), PixelRect::EMPTY]);
    assert!(cropped * 10 <= full);
    assert!(difference <= SUBTEXEL_ROUNDING, "{difference}");
}
