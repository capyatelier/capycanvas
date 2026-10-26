use super::*;
use crate::layer_tests::{dab, preset_style, placement};
use layer_core::{DefaultBrushPreset, Document, Point, Selection, raster::RasterRevision};

#[test]
fn crystals_third_published_shift_copies_black_across_integer_map_slope() {
    fn hash(mut value:u32)->u32 {
        value=(value^(value>>16)).wrapping_mul(2146121005);
        value=(value^(value>>15)).wrapping_mul(2221713035);
        value^(value>>16)
    }
    fn stroke(r:&mut WgpuRasterizer,doc:&mut Document,seed:u32,id:u64) {
        let mut style=preset_style(DefaultBrushPreset::LiquifyCrystals);
        let context=layer_core::liquify::Context::for_source(placement::paint(doc),false);
        if id>1 { assert!(context.field.is_some()); }
        style.liquify=Some(Arc::new(context));
        placement::paint_mut(doc).raster=RasterRevision::pending();
        let mut contact=dab([0.;4]); contact.material[3]=1.; contact.contact[3]=seed as f32;
        let mut batch=crate::test_support::dab_batch(placement::target(doc),style,layer_core::Rect::from_extent([128;2]));
        batch.stroke_id=StrokeId(id);
        crate::test_support::submit_ready(r, FramePacket{dabs:&[contact],dab_batches:&[batch],..crate::test_support::packet(doc.scene(),[128;2])}).unwrap();
        assert!(placement::paint(doc).raster.liquify().is_some());
    }
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document([128;2],"crystals slope regression");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source([128;2],|x,_| if x%2==0 { [0,0,0,255] } else { [255;4] }));
    crate::test_support::submit_ready(&mut r, FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),[128;2])}).unwrap();
    stroke(&mut r,&mut doc,0,1);
    stroke(&mut r,&mut doc,1,2);
    let state=placement::paint(&doc).raster.liquify().unwrap();
    let field=state.field.as_any().downcast_ref::<Field>().unwrap();
    let tile=&field.tiles[&[0,0]];
    let mut bytes=Vec::with_capacity(256*256*16);
    for _y in 0..256 { for x in 0..256 {
        let displacement: f32=match x {63=>-4.,64=>-2.,_=>0.};
        for word in [displacement.to_bits(),0,0x00003f80,0x3f800000] {bytes.extend(word.to_ne_bytes());}
    }}
    r.queue.write_texture(tile.texture.as_image_copy(),&bytes,wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(256*16),rows_per_image:Some(256)},tile.texture.size());
    let seed=(0..65536u32).find(|&random| {
        let cell=hash(15^hash(16)^random^hash(0));
        [2654435769,2246822507].map(|salt| (24.*0.6*((hash(cell^salt)>>8) as f32/16777216.-0.5)+0.5).floor() as i32)==[2,0]
    }).unwrap();
    stroke(&mut r,&mut doc,seed,3);
    let image=r.readback_srgb_rgba8().unwrap();
    let at=(64*128+62)*4;
    assert_eq!(&image[at..at+4],&[0,0,0,255],"a whole-pixel Crystals shift must not filter the integer slope as compression");
}

fn seeded_context(r:&WgpuRasterizer,doc:&Document,map:impl Fn(u32,u32)->([f32;2],[f32;4]))->layer_core::liquify::Context {
    let extent=placement::paint(doc).domain;
    let tile=Arc::new(Tile::new(r,PixelRect::full(extent)));
    let mut bytes=Vec::with_capacity(256*256*16);
    for y in 0..256 {for x in 0..256 {
        let (displacement,j)=map(x,y);
        let pack=|a:f32,b:f32| (a.to_bits()>>16)|(b.to_bits()&0xffff0000);
        for word in [displacement[0].to_bits(),displacement[1].to_bits(),pack(j[0],j[1]),pack(j[2],j[3])] {bytes.extend(word.to_ne_bytes());}
    }}
    r.queue.write_texture(tile.texture.as_image_copy(),&bytes,wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(4096),rows_per_image:Some(256)},tile.texture.size());
    let source=Arc::new(placement::paint(doc).clone());
    layer_core::liquify::Context {current:source.clone(),original:source,amount:None,field:Some(Arc::new(Field{
        device:(*r.device).clone(),tiles:BTreeMap::from([([0,0],tile)]),originals:Arc::new(Mutex::new(Originals::default())),
    }))}
}

fn render_seeded(r:&mut WgpuRasterizer,doc:&mut Document,context:layer_core::liquify::Context,id:u64)->Vec<u8> {
    let extent=placement::paint(doc).domain;
    let amount=context.amount;
    let mut style=preset_style(DefaultBrushPreset::LiquifyPush);style.liquify=Some(Arc::new(context));
    placement::paint_mut(doc).raster=RasterRevision::pending();
    let mut contact=dab([0.;4]);contact.material[3]=0.;
    let mut batch=crate::test_support::dab_batch(placement::target(doc),style,layer_core::Rect::from_extent([256;2]));
    batch.stroke_id=StrokeId(id); if amount.is_some(){batch.dab_count=0;}
    crate::test_support::submit_ready(r, FramePacket{dabs:&[contact],dab_batches:&[batch],..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    r.readback_srgb_rgba8().unwrap()
}

#[test]
fn liquify_indexed_originals_cross_array_layers_and_sparse_tile_borders() {
    let extent=[18*256,256];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"indexed liquify sources");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,_| {
        if x/256==7 {[0;4]} else {let c=(x/256*13) as u8;[c,c,c,255]}
    }));
    crate::test_support::submit_ready(&mut r, FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let context=seeded_context(&r,&doc,|x,_|{
        let source=if x<18 {x as f32*256.+128.5} else if x==20 {1792.} else if x==21 {4608.5} else if x==22 {-0.5} else {255.5};
        ([source-(x as f32+0.5),0.],[1.,0.,0.,1.])
    });
    let image=render_seeded(&mut r,&mut doc,context,50);
    for x in 0..18usize {
        let at=(64*extent[0] as usize+x)*4;
        let expected=if x==7 {[0;4]} else {[(x*13) as u8,(x*13) as u8,(x*13) as u8,255]};
        for c in 0..4 {assert!((image[at+c] as i16-expected[c] as i16).abs()<=1,"tile {x} component {c}: {:?}",&image[at..at+4]);}
    }
    let at=(64*extent[0] as usize+20)*4;
    assert!((image[at+3] as i16-128).abs()<=1,"bilinear tap crosses authored-to-empty tile edge: {:?}",&image[at..at+4]);
    for x in [21,22] {let at=(64*extent[0] as usize+x)*4;assert_eq!(&image[at..at+4],&[0;4],"outside true source extent {x}");}
    let state=placement::paint(&doc).raster.liquify().unwrap();
    let field=state.field.as_any().downcast_ref::<Field>().unwrap();
    assert!(field.originals.lock().unwrap().atlases.len()>=2);
}

#[test]
fn liquify_sparse_original_atlas_chunks_match_unfragmented_source() {
    let render = |fragmented: bool| {
        let extent = [512, 256];
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut doc = placement::paint_document(extent, "liquify fragmented original atlases");
        placement::set_source(&mut doc, layer_core::color::source::rgba8_source(extent,
            |x, _| if x < 256 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }));
        crate::test_support::submit_ready(&mut r, FramePacket { reset_layers: true,
            ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
        let mut style = preset_style(DefaultBrushPreset::LiquifyPush);
        style.liquify = Some(Arc::new(seeded_context(&r, &doc, |x, _| {
            ([256. - (x as f32 + 0.5), 0.], [1., 0., 0., 1.])
        })));
        placement::paint_mut(&mut doc).raster = RasterRevision::pending();
        let mut contact = dab([0.; 4]);
        contact.material[3] = 0.;
        let mut batch = crate::test_support::dab_batch(placement::target(&doc), style,
            layer_core::Rect::from_extent([256; 2]));
        batch.stroke_id = StrokeId(290);
        crate::test_support::submit_ready(&mut r, FramePacket { dabs: &[contact], dab_batches: &[batch],
            ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
        let state = placement::paint(&doc).raster.liquify().unwrap();
        let field = state.field.as_any().downcast_ref::<Field>().unwrap();
        assert!(page_coordinates(field.tiles[&[0, 0]].source_bounds(extent, false))
            .any(|coordinate| coordinate == [1, 0]),
            "the reconstructed tile must sample both source tiles");
        let mut originals = field.originals.lock().unwrap();
        assert_eq!(originals.atlases.len(), 1);
        assert!(originals.tiles.contains_key(&[0, 0]) && originals.tiles.contains_key(&[1, 0]));
        if fragmented {
            let base = &originals.atlases[0];
            let texture = base.texture.clone();
            let view = base.view.clone();
            for _ in 1..12 {
                originals.atlases.push(OriginalAtlas {
                    id: NEXT_RESOURCE.fetch_add(1, Ordering::Relaxed),
                    texture: texture.clone(), view: view.clone(),
                });
            }
            let distant = originals.tiles[&[1, 0]].clone();
            originals.tiles.insert([1, 0], Arc::new(OriginalTile {
                index: 11 * ORIGINAL_LAYERS + 1,
                view: distant.view.clone(),
            }));
        }
        drop(originals);
        let mut style = preset_style(DefaultBrushPreset::LiquifyPush);
        style.liquify = Some(Arc::new(layer_core::liquify::Context::for_source(
            placement::paint(&doc), false)));
        placement::paint_mut(&mut doc).raster = RasterRevision::pending();
        contact.center = Point { x: 64., y: 128. };
        contact.radii = [64.; 2];
        contact.material[3] = 1.;
        contact.motion = [1., 0.];
        let mut batch = crate::test_support::dab_batch(placement::target(&doc), style, contact.bounds());
        batch.stroke_id = StrokeId(291);
        r.submit(FramePacket { dabs: &[contact], dab_batches: &[batch],
            ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
        let image = r.readback_srgb_rgba8().unwrap();
        let at = (128 * extent[0] as usize + 64) * 4;
        assert!(image[at] > 100 && image[at + 2] > 100 && image[at + 3] == 255,
            "the cross-tile sample must use red and blue original tiles: {:?}", &image[at..at + 4]);
        image
    };
    assert_eq!(render(false), render(true),
        "splitting the two needed originals across atlas chunks must preserve all reconstructed pixels");
}

#[test]
fn liquify_filtered_and_manual_source_sampling_agree_inside_and_across_tiles() {
    let extent=[512,256];
    let render=|omitted| {
        let mut r=WgpuRasterizer::native_headless_without(Default::default(),omitted).unwrap();
        let mut doc=placement::paint_document(extent,"liquify optional filter parity");
        placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y| {
            let alpha=(48+(x*17+y*11)%192) as u8;
            [((x*37+y*13)%256) as u8,((x*7+y*43)%256) as u8,
                ((x*23+y*29)%256) as u8,alpha]
        }));
        r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
        let before=r.readback_srgb_rgba8().unwrap();
        let source_pages=r.paint_layers.iter().find(|layer|layer.id==placement::target(&doc)).unwrap()
            .pages.iter().map(|page|(page.coordinate,crate::layer_tests::page_bytes(&r,&page.active().texture)))
            .collect::<BTreeMap<_,_>>();
        let mut context=seeded_context(&r,&doc,|x,_| {
            if x>=224 {return ([300.5-(x as f32+0.5),0.],[1.,0.,0.,1.]);}
            let source_x=if x<128 {x as f32+0.73} else {256.1+(x-128) as f32*0.02};
            ([source_x-(x as f32+0.5),-0.09],[1.35,0.2,-0.15,1.1])
        });
        context.amount=Some(1.);
        let pixels=render_seeded(&mut r,&mut doc,context,240);
        let target=placement::target(&doc);
        let page=r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter()
            .find(|page|page.coordinate==[0,0]).unwrap();
        let linear=crate::test_support::float_pixels(&r,&page.active().texture);
        (r.device.float32_filterable(),before,source_pages,pixels,linear)
    };
    let (filterable,before,source_pages,filtered,filtered_linear)=render(wgpu::Features::empty());
    let (manual,manual_before,manual_source_pages,fallback,fallback_linear)=render(wgpu::Features::FLOAT32_FILTERABLE);
    assert!(!manual,"the second renderer must omit float filtering");
    assert_eq!(source_pages,manual_source_pages,"GPU source pixels must agree before Liquify");
    assert_eq!(before,manual_before,"undeformed displays must agree before Liquify");
    assert!(filtered.as_chunks::<4>().0.iter().zip(before.as_chunks::<4>().0.iter())
        .enumerate().filter(|(i,(a,b))| i%(extent[0] as usize)<224 && a!=b).count()>1000,
        "fractional inside-tile and cross-tile sampling must visibly deform the source");
    assert!(filtered==fallback,"Liquify RGBA8 output must be exact with or without float filtering (feature present: {filterable})");
    assert!(filtered_linear==fallback_linear,"Liquify linear pages must be exact with or without float filtering (feature present: {filterable})");
}

#[test]
fn liquify_compression_mips_average_one_pixel_stripes_at_32_and_64_to_one() {
    let extent=[768,256];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"liquify minification reference");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,_| if x%2==0 {[0,0,0,255]} else {[255;4]}));
    r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let context=seeded_context(&r,&doc,|x,_|{
        let ratio=if x<128 {32.} else {64.};
        ([255.5-(x as f32+0.5),0.],[ratio,0.,0.,1.])
    });
    let image=render_seeded(&mut r,&mut doc,context,60);
    for x in [31,95,159,223] {let at=(64*extent[0] as usize+x)*4;
        for c in 0..3 {assert!((image[at+c] as i16-188).abs()<=1,"32/64:1 pixel {x}: {:?}",&image[at..at+4]);}
        assert_eq!(image[at+3],255);
    }
}

#[test]
fn local_liquify_keeps_previously_deformed_untouched_pixels_in_preview_and_commit() {
    let extent=[256;2];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"liquify untouched prior deformation");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y| {
        let value=((x*37+y*71+x*y%251)%256) as u8;
        [value,value,value,255]
    }));
    r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let mut context=seeded_context(&r,&doc,|_,_|([0.375,0.],[8.25,0.,0.,1.]));
    context.amount=Some(1.);
    let before=render_seeded(&mut r,&mut doc,context,90);
    let prior=placement::paint(&doc).raster.liquify().unwrap();
    let prior_field=prior.field.as_any().downcast_ref::<Field>().unwrap();
    let map_before=crate::layer_tests::page_bytes(&r,&prior_field.tiles[&[0,0]].texture);
    let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
    style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
    let mut contact=dab([0.;4]);
    contact.center=layer_core::Point{x:192.,y:128.};
    contact.radii=[24.;2];
    contact.material[3]=1.;
    contact.motion=[4.,0.];
    let mut preview=crate::test_support::dab_batch(placement::target(&doc),style.clone(),contact.bounds());
    preview.kind=DabBatchKind::Preview;
    preview.stroke_id=StrokeId(91);
    preview.stroke_end=false;
    r.submit(FramePacket{dabs:&[contact],dab_batches:&[preview],..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let predicted=r.readback_srgb_rgba8().unwrap();
    assert!((105..150).any(|y|(175..210).any(|x| {
        let at=(y*256+x)*4;
        predicted[at..at+4]!=before[at..at+4]
    })),"the local preview must move visible pixels");
    let preview_mismatches=(100..150).flat_map(|y|(32..96).map(move |x|(x,y)))
        .filter(|&(x,y)| {let at=(y*256+x)*4;predicted[at..at+4]!=before[at..at+4]})
        .count();
    r.submit(crate::test_support::packet(doc.scene(),extent)).unwrap();
    assert_eq!(r.readback_srgb_rgba8().unwrap(),before,"cancelled preview restores the prior deformation");
    placement::paint_mut(&mut doc).raster=RasterRevision::pending();
    let mut commit=crate::test_support::dab_batch(placement::target(&doc),style,contact.bounds());
    commit.stroke_id=StrokeId(92);
    r.submit(FramePacket{dabs:&[contact],dab_batches:&[commit],..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let committed=r.readback_srgb_rgba8().unwrap();
    let current=placement::paint(&doc).raster.liquify().unwrap();
    let current_field=current.field.as_any().downcast_ref::<Field>().unwrap();
    let map_after=crate::layer_tests::page_bytes(&r,&current_field.tiles[&[0,0]].texture);
    for y in 100..150 { for x in 32..96 {
        let at=(y*256+x)*4;
        assert_eq!(&committed[at..at+4],&before[at..at+4],"untouched committed pixel {x},{y}");
        let at=(y*256+x)*16;
        assert_eq!(&map_after[at..at+16],&map_before[at..at+16],"untouched map value {x},{y}");
    }}
    assert_eq!(preview_mismatches,0,"the preview changed {preview_mismatches} untouched pixels");
}

#[test]
fn fit_persistent_preview_preserves_untouched_prior_compression_in_same_tile() {
    let extent=[256;2];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"liquify Fit prior compression");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y| {
        let value=((x*37+y*71+x*y%251)%256) as u8;
        if (176..208).contains(&x) && (104..152).contains(&y) {[245,35,45,255]}
        else {[value,value,value,255]}
    }));
    r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let mut context=seeded_context(&r,&doc,|_,_|([0.375,0.],[8.25,0.,0.,1.]));
    context.amount=Some(1.);
    render_seeded(&mut r,&mut doc,context,270);
    let target=placement::target(&doc);
    let view=layer_render::ViewState{width_px:32,height_px:32,
        document_to_surface:[0.125,0.,0.,0.125,0.,0.]};
    r.submit(FramePacket{view,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let before=crate::scene::scale::tests::display_pixels(&r);
    let [width,height]=r.scale_display.as_ref().unwrap().plan.size.map(|n|n as usize);
    let raw_before={
        let page=r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter()
            .find(|page|page.coordinate==[0,0]).unwrap();
        crate::layer_tests::page_bytes(&r,&page.active().texture)
    };
    let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
    style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
    let mut real=dab([0.;4]);
    real.center=Point{x:192.,y:128.};
    real.radii=[40.;2];
    real.material[3]=1.;
    real.motion=[20.,0.];
    let mut predicted=real;
    predicted.center.x+=8.;
    let mut isolated=crate::test_support::dab_batch(target,style.clone(),predicted.bounds());
    isolated.kind=DabBatchKind::Preview;
    isolated.stroke_id=StrokeId(272);
    isolated.stroke_end=false;
    r.submit(FramePacket{view,dabs:&[predicted],dab_batches:&[isolated],
        ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let isolated_display=crate::scene::scale::tests::display_pixels(&r);
    let isolated_mismatches=(height*11/32..height*21/32).flat_map(|y|
        (width*4/32..width*12/32).map(move |x|y*width+x))
        .filter(|&at|isolated_display[at]!=before[at]).count();
    assert_eq!(isolated_mismatches,0,"a preview-only dab must retain untouched compressed pixels");
    r.submit(FramePacket{view,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    assert!(crate::scene::scale::tests::display_pixels(&r)==before,"cancel restores the prior compressed display");
    placement::paint_mut(&mut doc).raster=RasterRevision::pending();
    let mut persistent=crate::test_support::dab_batch(target,style,real.bounds());
    persistent.stroke_id=StrokeId(271);
    persistent.stroke_end=false;
    let mut preview=persistent.clone();
    preview.kind=DabBatchKind::Preview;
    preview.first_dab=1;
    preview.damage=predicted.bounds();
    r.submit(FramePacket{view,dabs:&[real,predicted],dab_batches:&[persistent,preview],
        ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    assert_eq!(r.preview_level,2,"the persistent tail must use the reduced Fit preview");
    let stroke=r.liquify.as_ref().unwrap().strokes.get(&target).unwrap();
    let colored=crate::layer_tests::page_bytes(&r,&stroke.colored_tiles[&[0,0]].texture);
    let updated=crate::layer_tests::page_bytes(&r,&stroke.field.tiles[&[0,0]].texture);
    let map_changes=(96..160).flat_map(|y|(160..224).map(move |x|(x,y)))
        .filter(|&(x,y)| {let at=(y*256+x)*16;colored[at..at+16]!=updated[at..at+16]}).count();
    assert!(map_changes>0,"the persistent chunk must update the map near the dab");
    let old_map_changes=(88..168).flat_map(|y|(32..96).map(move |x|(x,y)))
        .filter(|&(x,y)| {let at=(y*256+x)*16;colored[at..at+16]!=updated[at..at+16]}).count();
    assert_eq!(old_map_changes,0,"the prior compressed region must retain exact packed map values");
    assert_ne!(stroke.colored_tiles[&[0,0]].id,stroke.field.tiles[&[0,0]].id,
        "the unresolved field map must differ from the last color-resolved map");
    let accounted=r.liquify.as_ref().unwrap().storage_bytes();
    assert!(r.metrics().paint_state_storage_bytes>=accounted,
        "paint-state metrics must include retained resolved maps");
    let old=r.liquify.as_mut().unwrap().strokes.get_mut(&target).unwrap().colored_tiles.remove(&[0,0]).unwrap();
    let without_old=r.liquify.as_ref().unwrap().storage_bytes();
    assert_eq!(accounted-without_old,texture_bytes(&old.texture),
        "storage accounting must include the retained resolved map while color is deferred");
    r.liquify.as_mut().unwrap().strokes.get_mut(&target).unwrap().colored_tiles.insert([0,0],old);
    let raw_after={
        let page=r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter()
            .find(|page|page.coordinate==[0,0]).unwrap();
        crate::layer_tests::page_bytes(&r,&page.active().texture)
    };
    assert!(raw_before==raw_after,"persistent full-resolution color must wait for the Fit preview");
    let display=crate::scene::scale::tests::display_pixels(&r);
    let mut mismatches=0usize;
    let mut max_linear=0f32;
    let mut max_ulp=0u32;
    let mut max_encoded=0u8;
    for y in height*11/32..height*21/32 {for x in width*4/32..width*12/32 {
        let at=y*width+x;
        mismatches+=usize::from(display[at]!=before[at]);
        for channel in 0..4 {
            let a=before[at][channel];let b=display[at][channel];
            max_linear=max_linear.max((a-b).abs());
            max_ulp=max_ulp.max(a.to_bits().abs_diff(b.to_bits()));
            let encode=|value:f32|if channel==3 {value}
                else {layer_core::color::RgbSpace::Srgb.encode(f64::from(value)) as f32};
            let a=(encode(a).clamp(0.,1.)*255.).round() as u8;
            let b=(encode(b).clamp(0.,1.)*255.).round() as u8;
            max_encoded=max_encoded.max(a.abs_diff(b));
        }
    }}
    assert!(max_encoded==0 && max_linear<=1e-6,
        "untouched prior compression: {mismatches} float pixels, max linear {max_linear}, ULP {max_ulp}, encoded {max_encoded}/255");
}

#[test]
fn tiny_footprint_is_bit_exact_where_local_dab_has_no_influence() {
    let extent=[256;2];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"liquify tiny footprint");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y| {
        let value=((x*37+y*71+x*y%251)%256) as u8;
        [value,value,value,255]
    }));
    r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let mut context=seeded_context(&r,&doc,|_,_|([0.375,0.],[1e-5,0.,0.,1.]));
    context.amount=Some(1.);
    let before=render_seeded(&mut r,&mut doc,context,220);
    let prior=placement::paint(&doc).raster.liquify().unwrap();
    let prior_field=prior.field.as_any().downcast_ref::<Field>().unwrap();
    let map_before=crate::layer_tests::page_bytes(&r,&prior_field.tiles[&[0,0]].texture);
    let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
    style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
    let mut contact=dab([0.;4]);
    contact.center=Point{x:192.,y:128.};
    contact.radii=[24.;2];
    contact.material[3]=1.;
    contact.motion=[4.,0.];
    let mut batch=crate::test_support::dab_batch(placement::target(&doc),style,contact.bounds());
    batch.stroke_id=StrokeId(221);
    placement::paint_mut(&mut doc).raster=RasterRevision::pending();
    r.submit(FramePacket{dabs:&[contact],dab_batches:&[batch],
        ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let after=r.readback_srgb_rgba8().unwrap();
    let current=placement::paint(&doc).raster.liquify().unwrap();
    let field=current.field.as_any().downcast_ref::<Field>().unwrap();
    let map_after=crate::layer_tests::page_bytes(&r,&field.tiles[&[0,0]].texture);
    for [x,y] in [[40,128],[171,108],[213,108],[171,148]] {
        let color=(y*256+x)*4;
        let map=(y*256+x)*16;
        assert_eq!(&after[color..color+4],&before[color..color+4],"untouched color {x},{y}");
        assert_eq!(&map_after[map..map+16],&map_before[map..map+16],"untouched packed map {x},{y}");
    }
    let changed=(112..144).flat_map(|y|(176..208).map(move |x|(x,y)))
        .any(|(x,y)| {let at=(y*256+x)*16;map_after[at..at+16]!=map_before[at..at+16]});
    assert!(changed,"the local dab must change nearby map values");
}

#[test]
fn untouched_own_map_survives_when_it_is_in_a_later_source_chunk() {
    for preset in [DefaultBrushPreset::LiquifyPush,DefaultBrushPreset::LiquifyPinch] {
    let pinch=matches!(preset,DefaultBrushPreset::LiquifyPinch);
    let extent=[14*256,256];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"liquify own map later chunk");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y| {
        let value=((x*37+y*71+x*y%251)%256) as u8;
        [value,value,value,255]
    }));
    r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let seed=seeded_context(&r,&doc,|_,_|([0.375,0.],[1e-5,0.,0.,1.]));
    let bytes={
        let field=seed.field.as_ref().unwrap().as_any().downcast_ref::<Field>().unwrap();
        crate::layer_tests::page_bytes(&r,&field.tiles[&[0,0]].texture)
    };
    let tile_bytes=|x:u32| {
        let mut tile_bytes=bytes.clone();
        for pixel in tile_bytes.as_chunks_mut::<16>().0 {
            pixel[..4].copy_from_slice(&(0.375+x as f32*0.125).to_ne_bytes());
        }
        tile_bytes
    };
    let tiles=(0..14).map(|x| {
        let tile=Arc::new(Tile::new(&r,PixelRect::new(x*256,0,(x+1)*256,256)));
        r.queue.write_texture(tile.texture.as_image_copy(),&tile_bytes(x),
            wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(4096),rows_per_image:Some(256)},tile.texture.size());
        ([x,0],tile)
    }).collect();
    let context=layer_core::liquify::Context{field:Some(Arc::new(Field{
        device:(*r.device).clone(),tiles,originals:Arc::new(Mutex::new(Originals::default())),
    })),..seed};
    let mut style=preset_style(preset);
    style.liquify=Some(Arc::new(context));
    let dabs=if pinch {
        let mut sweep=dab([0.;4]);
        sweep.center=Point{x:13.*256.+2000.5,y:8.5};
        sweep.radii=[6000.,16.];
        sweep.material[3]=1.;
        let mut contact=sweep;
        contact.center.x=13.*256.+428.5;
        contact.center.y=128.5;
        contact.radii=[2000.;2];
        [sweep;4].into_iter().chain([contact;4]).collect::<Vec<_>>()
    } else {
        let mut sweep=dab([0.;4]);
        sweep.center=Point{x:13.*256.+240.5,y:8.5};
        sweep.radii=[16.;2];
        sweep.material[3]=1.;
        sweep.motion=[3400.,0.];
        let mut dabs=vec![sweep];
        for (source,y) in [(9,32.5),(10,128.5),(11,224.5)] {
            let mut contact=dab([0.;4]);
            contact.center=Point{x:13.*256.+128.5,y};
            contact.radii=[16.;2];
            contact.material[3]=1.;
            contact.motion=[(13-source) as f32*256.,0.];
            dabs.push(contact);
        }
        dabs
    };
    let mut batch=crate::test_support::dab_batch(placement::target(&doc),style,
        if pinch {PixelRect::new(13*256+60,120,13*256+196,137).to_rect()}
        else {PixelRect::new(13*256,0,14*256,256).to_rect()});
    batch.stroke_id=StrokeId(222);
    batch.dab_count=dabs.len() as u32;
    let bounds=crate::material_sources::sample_rect(&batch,&dabs,[13,0]);
    assert!(bounds.min.x<256. && bounds.max.x>13.*256.,"{preset:?}: the trace must bind fourteen source maps");
    placement::paint_mut(&mut doc).raster=RasterRevision::pending();
    crate::test_support::submit_ready(&mut r, FramePacket{dabs:&dabs,dab_batches:&[batch],
        ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let current=placement::paint(&doc).raster.liquify().unwrap();
    let field=current.field.as_any().downcast_ref::<Field>().unwrap();
    let after=crate::layer_tests::page_bytes(&r,&field.tiles[&[13,0]].texture);
    let untouched=(20*256+20)*16;
    let own_bytes=tile_bytes(13);
    assert_eq!(&after[untouched..untouched+16],&own_bytes[untouched..untouched+16],
        "own tile map in source chunk two must copy its exact packed words");
    for (source,x,y) in if pinch {[(9,64,128),(10,128,128),(11,192,128)]}
        else {[(9,128,32),(10,128,128),(11,128,224)]} {
        let at=(y*256+x)*16;
        let actual=f32::from_ne_bytes(after[at..at+4].try_into().unwrap());
        let initial=13.*256.+x as f32+0.5;
        let expected=if pinch {
            let mut mapped=initial;
            for _ in 0..4 { mapped+=(mapped-(13.*256.+428.5))*0.35; }
            assert!((source*256) as f32<=mapped && mapped<((source+1)*256) as f32,
                "Pinch sample must read source map {source}, got {mapped}");
            mapped-initial+0.375+source as f32*0.125
        } else {-((13-source)*256) as f32+0.375+source as f32*0.125};
        assert!((actual-expected).abs()<0.005,
            "{preset:?} source map {source} must survive the second chunk: {actual} vs {expected}");
    }
    }
}

#[test]
#[ignore = "writes GPU filter comparison PNGs to artifacts/liquify-filter-c8"]
fn capture_liquify_filter_tap_reference_images() {
    let extent=[256;2];
    let dir=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/liquify-filter-c8");
    std::fs::create_dir_all(&dir).unwrap();
    let source=layer_core::color::source::rgba8_source(extent,|x,y| {
        let checker=if (x/2+y/2)%2==0 { 35 } else { 220 };
        let vertical=if x%29<2 { 0 } else { 255 };
        let horizontal=if y%31<2 { 0 } else { 255 };
        let natural=((x as f32*0.7+y as f32*0.3+25.*((x as f32)/17.).sin()).clamp(0.,255.)) as u8;
        if x<128 {[checker,vertical,horizontal,255]} else {[natural,(y as u8).saturating_add(30),vertical,255]}
    });
    let save=|name:&str,pixels:&[u8]| {
        let file=std::fs::File::create(dir.join(name)).unwrap();
        let mut png=png::Encoder::new(file,extent[0],extent[1]);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header().unwrap().write_image_data(pixels).unwrap();
    };
    for (ratio,angle) in [(2.,0.),(8.,0.),(8.,45.),(32.,45.)] {
        let name=format!("r{}_a{}",ratio as u32,angle as u32);
        let radians=(angle as f32).to_radians();
        let (s,c)=radians.sin_cos();
        let footprint=[ratio*c,ratio*s,-s,c];
        let render=|preview:bool| {
            let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let mut doc=placement::paint_document(extent,"liquify tap reference");
            placement::set_source(&mut doc,source.clone());
            r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
            let mut context=seeded_context(&r,&doc,|_,_|([0.375,0.21],footprint));
            context.amount=Some(1.);
            let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
            style.liquify=Some(Arc::new(context));
            let mut batch=crate::test_support::dab_batch(placement::target(&doc),style,
                layer_core::Rect::from_extent(extent));
            batch.stroke_id=StrokeId(230);
            batch.dab_count=0;
            if preview {
                batch.kind=DabBatchKind::Preview;
                batch.stroke_end=false;
            } else {placement::paint_mut(&mut doc).raster=RasterRevision::pending();}
            r.submit(FramePacket{dab_batches:&[batch],
                ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
            if preview {assert_eq!(r.preview_level,0,"the comparison must vary tap count at one-pixel blocks");}
            r.readback_srgb_rgba8().unwrap()
        };
        let exact=render(false);
        let four=render(true);
        save(&format!("{name}_16.png"),&exact);
        save(&format!("{name}_4.png"),&four);
        let mean=exact.iter().zip(&four).map(|(a,b)|a.abs_diff(*b) as u64).sum::<u64>() as f32/exact.len() as f32;
        let max=exact.iter().zip(&four).map(|(a,b)|a.abs_diff(*b)).max().unwrap();
        println!("{name}: 4 versus 16 taps mean {mean:.3}/255, max {max}/255");
    }
}

#[test]
fn fit_view_liquify_preview_matches_committed_display_and_cancel_restores_it() {
    let extent=[1024;2];
    let mut preview=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut committed=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"fit liquify preview");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y| {
        let marker=if (x as i32-570).abs()<24 && (y as i32-512).abs()<24 { 220 } else { 0 };
        [(x/4).min(255) as u8,(y/4).min(255) as u8,marker,255]
    }));
    let mut commit_doc=doc.clone();
    placement::paint_mut(&mut commit_doc).raster=RasterRevision::pending();
    let mut fit=crate::test_support::packet(doc.scene(),extent);
    fit.view.width_px=128;
    fit.view.height_px=128;
    fit.view.document_to_surface=[0.125,0.,0.,0.125,0.,0.];
    for r in [&mut preview,&mut committed] {
        crate::layer_tests::publish(r,FramePacket{reset_layers:true,..fit});
    }
    let before=crate::scene::scale::tests::display_pixels(&preview);
    let mut contact=dab([0.;4]);
    contact.center=Point{x:512.,y:512.};
    contact.radii=[112.;2];
    contact.motion=[20.,0.];
    contact.material[3]=1.;
    let mut batch=crate::test_support::dab_batch(placement::target(&doc),
        preset_style(DefaultBrushPreset::LiquifyPush),contact.bounds());
    batch.style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
    batch.stroke_id=StrokeId(200);
    batch.stroke_end=false;
    batch.kind=DabBatchKind::Preview;
    preview.submit(FramePacket{dabs:&[contact],dab_batches:std::slice::from_ref(&batch),..fit}).unwrap();
    assert_eq!(preview.preview_level,2,"Fit view must select 4-by-4 map and color preview blocks");
    assert!(preview.preview_pages.iter().all(|page|page.active().texture.width()==64));
    let predicted=crate::scene::scale::tests::display_pixels(&preview);
    preview.submit(fit).unwrap();
    assert!(crate::scene::scale::tests::display_pixels(&preview)==before,"cancel restores the exact display");
    batch.kind=DabBatchKind::Persistent;
    batch.stroke_end=true;
    let commit=FramePacket{dabs:&[contact],dab_batches:std::slice::from_ref(&batch),
        scene:commit_doc.scene(),..fit};
    committed.submit(commit).unwrap();
    let accepted=crate::scene::scale::tests::display_pixels(&committed);
    assert_eq!(predicted.len(),accepted.len());
    let changed=accepted.iter().zip(&before).filter(|(a,b)|a!=b).count();
    let predicted_changed=predicted.iter().zip(&before).filter(|(a,b)|a!=b).count();
    let [width,height]=preview.scale_display.as_ref().unwrap().plan.size.map(|n|n as usize);
    let mut difference=0f64;
    let mut components=0usize;
    for y in height*40/128..height*88/128 {for x in width*40/128..width*88/128 {
        let at=y*width+x;
        for channel in 0..4 {difference+=(predicted[at][channel]-accepted[at][channel]).abs() as f64;components+=1;}
    }}
    let average=difference/components as f64;
    assert!(changed>100,"the committed dab must visibly deform the Fit display");
    assert!(predicted_changed>100,"the Fit preview must visibly deform the display");
    assert!(average<0.05,"Fit preview diverges from full-resolution commit around the dab: average linear component error {average}");
}

#[test]
fn grouped_liquify_chunks_match_split_frames_in_pixels_and_map() {
    let render=|split:bool| {
        let extent=[512,256];
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut doc=placement::paint_document(extent,"liquify grouped chunks");
        placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y| {
            let value=((x*37+y*71+x*y%251)%256) as u8;
            [value,255-value,value/2,255]
        }));
        r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
        let original_pixels=r.readback_srgb_rgba8().unwrap();
        let before=r.metrics().command_passes;
        let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
        style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
        let dabs:Vec<_>=(0..3).map(|i| {
            let mut contact=dab([0.;4]);
            contact.center=Point{x:[96.,380.,420.][i],y:128.};
            contact.radii=[64.;2];
            contact.material[3]=1.;
            contact.motion=[0.4,0.];
            contact
        }).collect();
        let batches:Vec<_>=dabs.iter().enumerate().map(|(i,contact)| {
            let mut batch=crate::test_support::dab_batch(placement::target(&doc),style.clone(),contact.bounds());
            batch.stroke_id=StrokeId(210);
            batch.stroke_start=i==0;
            batch.stroke_end=i==2;
            batch.first_dab=i as u32;
            batch
        }).collect();
        if split {
            for i in 0..3 {
                if i==2 {placement::paint_mut(&mut doc).raster=RasterRevision::pending();}
                let mut batch=batches[i].clone();
                batch.first_dab=0;
                r.submit(FramePacket{dabs:&dabs[i..i+1],dab_batches:std::slice::from_ref(&batch),
                    ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
            }
        } else {
            placement::paint_mut(&mut doc).raster=RasterRevision::pending();
            r.submit(FramePacket{dabs:&dabs,dab_batches:&batches,
                ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
        }
        let passes=r.metrics().command_passes-before;
        let pixels=r.readback_srgb_rgba8().unwrap();
        assert_ne!(pixels,original_pixels,"the grouped chunks must visibly deform the source");
        let state=placement::paint(&doc).raster.liquify().unwrap();
        let field=state.field.as_any().downcast_ref::<Field>().unwrap();
        let map=field.tiles.iter().map(|(coordinate,tile)|(*coordinate,
            crate::layer_tests::page_bytes(&r,&tile.texture))).collect::<BTreeMap<_,_>>();
        (pixels,map,passes)
    };
    let (grouped_pixels,grouped_map,grouped_passes)=render(false);
    let (split_pixels,split_map,split_passes)=render(true);
    assert_eq!(grouped_map.len(),2,"the chunks must dirty both separated tiles");
    assert_eq!(grouped_map,split_map,"grouping must retain every map update across tile boundaries");
    assert_eq!(grouped_pixels,split_pixels,"grouping must reconstruct from the same original pixels");
    assert!(grouped_passes<split_passes,"grouped chunks must save reconstruction passes: {grouped_passes} vs {split_passes}");
}

#[test]
fn strong_pinch_keeps_opaque_source_when_footprint_exceeds_trace_tile() {
    let render = |published: bool| {
        let extent = [1200; 2];
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut doc = placement::paint_document(extent, "strong Pinch source footprint");
        placement::set_source(&mut doc, layer_core::color::source::rgba8_source(extent,
            |_, _| [83, 149, 211, 255]));
        r.submit(FramePacket { reset_layers: true,
            ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
        let target = placement::target(&doc);
        let raw_center = |r: &WgpuRasterizer| {
            let page = r.paint_layers.iter().find(|layer| layer.id == target).unwrap()
                .pages.iter().find(|page| page.coordinate == [2, 2]).unwrap();
            crate::test_support::float_pixels(r, &page.active().texture)[88 * 256 + 88]
        };
        let original = [83, 149, 211, 255].map(|code| {
            layer_core::color::RgbSpace::Srgb.decode(f64::from(code) / 255.) as f32
        });
        let mut contact = dab([0.; 4]);
        contact.center = Point { x: 600.5, y: 600.5 };
        contact.radii = [64.; 2];
        contact.material[3] = 1.;
        contact.previous_contact[0] = 1.;
        for chunk in 0..6 {
            let mut style = preset_style(DefaultBrushPreset::LiquifyPinch);
            style.liquify = Some(Arc::new(layer_core::liquify::Context::for_source(
                placement::paint(&doc), false)));
            let mut batch = crate::test_support::dab_batch(target, style, contact.bounds());
            batch.stroke_id = StrokeId(if published { 260 + chunk } else { 260 });
            batch.stroke_start = published || chunk == 0;
            batch.stroke_end = published || chunk == 5;
            batch.dab_count = 4;
            if batch.stroke_end {
                placement::paint_mut(&mut doc).raster = RasterRevision::pending();
            }
            let mut dabs = [contact; 4];
            if chunk == 5 { dabs[3].material[3] = 0.2; }
            crate::test_support::submit_ready(&mut r, FramePacket { dabs: &dabs, dab_batches: &[batch],
                ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
        }
        let state = placement::paint(&doc).raster.liquify().unwrap();
        let field = state.field.as_any().downcast_ref::<Field>().unwrap();
        let originals = field.originals.lock().unwrap();
        for coordinate in [[1, 2], [3, 2], [2, 1], [2, 3]] {
            assert!(originals.tiles.contains_key(&coordinate),
                "{}: source cache missed footprint tile {coordinate:?}",
                if published { "published" } else { "one stroke" });
        }
        drop(originals);
        let map = crate::layer_tests::page_bytes(&r, &field.tiles[&[2, 2]].texture);
        let at = (88 * 256 + 88) * 16;
        let packed = u32::from_ne_bytes(map[at + 8..at + 12].try_into().unwrap());
        let center_j = f32::from_bits((packed & 0xffff) << 16);
        assert!((1024. ..=1200.).contains(&center_j),
            "{}: effective center footprint must reach 1024–1200, got {center_j}",
            if published { "published" } else { "one stroke" });
        let actual = raw_center(&r);
        for component in 0..4 {
            assert!((actual[component] - original[component]).abs() <= 1e-5,
                "{}: constant opaque source changed at center component {component}: {actual:?} vs {original:?}, J={center_j}",
                if published { "published" } else { "one stroke" });
        }
    };
    render(false);
    render(true);
}

#[test]
fn pinch_minification_keeps_transparency_at_physical_canvas_edge() {
    let extent = [512, 256];
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc = placement::paint_document(extent, "Pinch transparent canvas edge");
    placement::set_source(&mut doc, layer_core::color::source::rgba8_source(extent,
        |_, _| [83, 149, 211, 255]));
    crate::test_support::submit_ready(&mut r, FramePacket { reset_layers: true,
        ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
    let target = placement::target(&doc);
    let mut contact = dab([0.; 4]);
    contact.center = Point { x: 0.5, y: 128.5 };
    contact.radii = [64.; 2];
    contact.material[3] = 1.;
    contact.previous_contact[0] = 1.;
    let mut style = preset_style(DefaultBrushPreset::LiquifyPinch);
    style.liquify = Some(Arc::new(layer_core::liquify::Context::for_source(
        placement::paint(&doc), false)));
    for chunk in 0..3 {
        let mut batch = crate::test_support::dab_batch(target, style.clone(), contact.bounds());
        batch.stroke_id = StrokeId(280);
        batch.stroke_start = chunk == 0;
        batch.stroke_end = chunk == 2;
        batch.dab_count = 4;
        if batch.stroke_end { placement::paint_mut(&mut doc).raster = RasterRevision::pending(); }
        let dabs = [contact; 4];
        crate::test_support::submit_ready(&mut r, FramePacket { dabs: &dabs, dab_batches: &[batch],
            ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
    }
    let field = placement::paint(&doc).raster.liquify().unwrap().field.clone();
    let field = field.as_any().downcast_ref::<Field>().unwrap();
    let map = crate::layer_tests::page_bytes(&r, &field.tiles[&[0, 0]].texture);
    let at = (128 * 256) * 16;
    let packed = u32::from_ne_bytes(map[at + 8..at + 12].try_into().unwrap());
    let edge_j = f32::from_bits((packed & 0xffff) << 16);
    assert!((32. ..64.).contains(&edge_j), "edge Pinch must create a minifying footprint: {edge_j}");
    let page = r.paint_layers.iter().find(|layer| layer.id == target).unwrap().pages.iter()
        .find(|page| page.coordinate == [0, 0]).unwrap();
    let pixels = crate::test_support::float_pixels(&r, &page.active().texture);
    let edge = pixels[128 * 256];
    let interior = pixels[128 * 256 + 128];
    assert!((0.49..=0.68).contains(&edge[3]),
        "the edge must retain partial source coverage through the minifying footprint: {edge:?}, J={edge_j}");
    for channel in 0..3 {
        let original = layer_core::color::RgbSpace::Srgb.decode(f64::from([83, 149, 211][channel]) / 255.) as f32;
        assert!((edge[channel] - original * edge[3]).abs() <= 1e-5,
            "edge color must retain its premultiplied source hue: {edge:?}");
    }
    assert!((interior[3] - 1.).abs() <= 1e-5,
        "pixels away from the real canvas edge must remain opaque: {interior:?}");
}

#[test]
fn fit_preview_defers_persistent_color_until_zero_dab_end_across_tiles() {
    let extent=[512,256];
    let render=|fit:bool,cancel:bool| {
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut doc=placement::paint_document(extent,"liquify multiframe fit preview");
        placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y| {
            if (106..146).contains(&x) && (104..152).contains(&y) {[250,35,45,255]}
            else if (358..398).contains(&x) && (104..152).contains(&y) {[30,45,245,255]}
            else {[((x*3+y)%160+45) as u8,((x+y*2)%150+45) as u8,80,255]}
        }));
        let view=if fit {layer_render::ViewState{width_px:64,height_px:32,
            document_to_surface:[0.125,0.,0.,0.125,0.,0.]}}
            else {crate::test_support::view(extent)};
        crate::test_support::submit_settled(&mut r, FramePacket{view,reset_layers:true,..crate::test_support::packet(doc.scene(),extent)});
        let target=placement::target(&doc);
        let colors=|r:&WgpuRasterizer|r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter()
            .map(|page|(page.coordinate,crate::layer_tests::page_bytes(r,&page.active().texture)))
            .collect::<BTreeMap<_,_>>();
        let original_colors=colors(&r);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let source_tiles=[0,1].map(|x| {
            let tile=crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|r.raw_layer_tile(target,[x,0],encoder)).unwrap();
            ([x,0],tile.texture)
        });
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        let source_colors=source_tiles.into_iter().map(|(coordinate,texture)|
            (coordinate,crate::layer_tests::page_bytes(&r,&texture))).collect::<BTreeMap<_,_>>();
        let original_display=crate::scene::scale::tests::display_pixels(&r);
        let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
        style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
        let mut two_page_work=(0,0);
        for (step,x) in [126.,378.,390.].into_iter().enumerate() {
            if fit && step==2 {
                let shown=crate::scene::scale::tests::display_pixels(&r);
                let mut idle=crate::test_support::dab_batch(target,style.clone(),layer_core::Rect::EMPTY);
                idle.kind=DabBatchKind::Preview;
                idle.stroke_id=StrokeId(250);
                idle.stroke_start=false;
                idle.stroke_end=false;
                idle.dab_count=0;
                r.submit(FramePacket{view,composite_all:false,dab_batches:&[idle.clone()],
                    ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
                assert_eq!(r.preview_level,2,"zero-dab tail must retain the reduced Fit preview");
                assert!(colors(&r).iter().all(|(coordinate,pixels)|source_colors.get(coordinate)==Some(pixels)),
                    "a zero-dab tail must keep persistent color deferred");
                let idle_display=crate::scene::scale::tests::display_pixels(&r);
                let [width,height]=r.scale_display.as_ref().unwrap().plan.size.map(|n|n as usize);
                let left_drift=(height*8/32..height*24/32).flat_map(|y|
                    (width*5/64..width*29/64).map(move |x|y*width+x))
                    .filter(|&at|idle_display[at]!=shown[at]).count();
                assert_eq!(left_drift,0,"zero-dab preview must retain the earlier tile's displayed pixels");
                let before=r.metrics();
                r.submit(FramePacket{view,composite_all:false,dab_batches:&[idle],
                    ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
                let after=r.metrics();
                assert_eq!(after.composited_pixels,before.composited_pixels,
                    "settled Fit preview pages must not recompose on a repeated zero-dab tail");
                assert_eq!(after.display_reduction_reads,before.display_reduction_reads,
                    "settled Fit preview pages must not rebuild display mips on a repeated zero-dab tail");
                assert_eq!(crate::scene::scale::tests::display_pixels(&r),idle_display,
                    "repeated zero-dab preview must preserve the displayed image");
            }
            let mut real=dab([0.;4]);
            real.center=Point{x,y:128.};
            real.radii=[72.;2];
            real.material[3]=1.;
            real.motion=[20.,0.];
            let mut persistent=crate::test_support::dab_batch(target,style.clone(),real.bounds());
            persistent.stroke_id=StrokeId(250);
            persistent.stroke_start=step==0;
            persistent.stroke_end=false;
            if fit {
                let mut predicted=real;
                predicted.center.x+=8.;
                let mut preview=persistent.clone();
                preview.kind=DabBatchKind::Preview;
                preview.first_dab=1;
                preview.damage=predicted.bounds();
                let cached=if step==2 {
                    let stroke=r.liquify.as_ref().unwrap().strokes.get(&target).unwrap();
                    assert!(stroke.preview_tiles.contains_key(&[0,0]),
                        "the prior tile must be cached before a later disjoint preview");
                    let page=r.preview_pages.iter().find(|page|page.coordinate==[0,0]).unwrap();
                    Some(crate::layer_tests::page_bytes(&r,&page.active().texture))
                } else {None};
                let dabs=[real,predicted];
                let batches=[persistent,preview];
                if step==1 {
                    let mut plans=batches.iter().map(|batch|crate::brush_tiles::plan(batch,
                        &dabs[batch.first_dab as usize..(batch.first_dab+batch.dab_count) as usize],extent)).collect::<Vec<_>>();
                    crate::liquify::Liquify::extend_plans(&mut r,FramePacket{view,dabs:&dabs,dab_batches:&batches,
                        ..crate::test_support::packet(doc.scene(),extent)},&mut plans);
                    assert!(plans[1].iter().any(|tile|tile.coordinate==[0,0]),
                        "the next preview plan must include the earlier dirty tile");
                }
                let before=r.metrics();
                r.submit(FramePacket{view,composite_all:false,dabs:&dabs,dab_batches:&batches,
                    ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
                let after=r.metrics();
                let work=(after.composited_pixels-before.composited_pixels,
                    after.display_reduction_reads-before.display_reduction_reads);
                if step==1 { two_page_work=work; }
                if step==2 {
                    assert!(r.liquify.as_ref().unwrap().preview_unchanged(target,[0,0]),
                        "the earlier off-tail page must remain cached during later prediction");
                    assert!(work.0<two_page_work.0,
                        "later prediction must recompose less than the earlier two-page update: {work:?} vs {two_page_work:?}");
                    assert!(work.1<two_page_work.1,
                        "later prediction must rebuild fewer display mips than the earlier two-page update: {work:?} vs {two_page_work:?}");
                }
                assert_eq!(r.preview_level,2,"Fit must use a reduced Liquify preview");
                let current=colors(&r);
                assert!(current.iter().all(|(coordinate,pixels)|source_colors.get(coordinate)==Some(pixels)),
                    "persistent color waits while Fit preview covers dirty tiles at step {step}");
                if step==1 {
                    let stroke=r.liquify.as_ref().unwrap().strokes.get(&target).unwrap();
                    assert!(stroke.preview_tiles.contains_key(&[0,0]),
                        "the earlier dirty tile must enter the preview cache");
                    let display=crate::scene::scale::tests::display_pixels(&r);
                    let [width,height]=r.scale_display.as_ref().unwrap().plan.size.map(|n|n as usize);
                    for region in [width*5/64..width*29/64,width*36/64..width*61/64] {
                        let changed=(height*8/32..height*24/32).flat_map(|y|region.clone().map(move |x|(x,y)))
                            .filter(|&(x,y)| {let at=y*width+x;display[at]!=original_display[at]})
                            .count();
                        assert!(changed>8,"Fit preview must show both earlier and newer tile deformation: region {region:?}, changed {changed}");
                    }
                }
                if let Some(pixels)=cached {
                    let page=r.preview_pages.iter().find(|page|page.coordinate==[0,0]).unwrap();
                    assert!(crate::layer_tests::page_bytes(&r,&page.active().texture)==pixels,
                        "a later frame must preserve unchanged preview tile pixels");
                }
            } else {
                r.submit(FramePacket{view,composite_all:false,dabs:&[real],dab_batches:&[persistent],
                    ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
                let current=colors(&r);
                assert!(current.iter().any(|(coordinate,pixels)|source_colors.get(coordinate)!=Some(pixels)),
                    "native view must reconstruct persistent color without a preview tail");
            }
        }
        if cancel {
            crate::test_support::submit_settled(&mut r, FramePacket{view,reset_layers:true,..crate::test_support::packet(doc.scene(),extent)});
            let cancelled=crate::scene::scale::tests::display_pixels(&r);
            let mismatches=cancelled.iter().zip(&original_display).filter(|(a,b)|a!=b).count();
            assert_eq!(mismatches,0,"cancel must restore the old raster display after deferred persistent frames");
            return None;
        }
        placement::paint_mut(&mut doc).raster=RasterRevision::pending();
        let mut end=crate::test_support::dab_batch(target,style,layer_core::Rect::EMPTY);
        end.stroke_id=StrokeId(250);
        end.stroke_start=false;
        end.stroke_end=true;
        end.dab_count=0;
        r.submit(FramePacket{view,composite_all:false,dab_batches:&[end],
            ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
        let final_colors=colors(&r);
        assert_ne!(final_colors,original_colors,"zero-dab end must flush the persistent color");
        let state=placement::paint(&doc).raster.liquify().unwrap();
        let field=state.field.as_any().downcast_ref::<Field>().unwrap();
        let maps=field.tiles.iter().map(|(coordinate,tile)|(*coordinate,
            crate::layer_tests::page_bytes(&r,&tile.texture))).collect::<BTreeMap<_,_>>();
        Some((final_colors,maps))
    };
    let (fit_colors,fit_maps)=render(true,false).unwrap();
    let (exact_colors,exact_maps)=render(false,false).unwrap();
    render(true,true);
    assert_eq!(fit_maps,exact_maps,"Fit prediction must not alter persistent map precision");
    assert_eq!(fit_colors,exact_colors,"zero-dab end must reconstruct full-resolution color exactly");
}

#[test]
fn liquify_selection_stops_deformation_at_a_sparse_tile_boundary() {
    let extent=[512,256];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"liquify selected tile boundary");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y| {
        let value=((x*37+y*71+x*y%251)%256) as u8;
        [value,value,value,255]
    }));
    r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let before=r.readback_srgb_rgba8().unwrap();
    let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
    style.selection=Some(Arc::new(Selection::polygon(vec![
        Point{x:0.,y:0.},Point{x:256.,y:0.},Point{x:256.,y:256.},Point{x:0.,y:256.},
    ]).unwrap()));
    style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
    placement::paint_mut(&mut doc).raster=RasterRevision::pending();
    let mut contact=dab([0.;4]);
    contact.center=Point{x:256.,y:128.};
    contact.radii=[32.;2];
    contact.material[3]=1.;
    contact.motion=[3.,0.];
    let mut batch=crate::test_support::dab_batch(placement::target(&doc),style,contact.bounds());
    batch.stroke_id=StrokeId(93);
    r.submit(FramePacket{dabs:&[contact],dab_batches:&[batch],..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let after=r.readback_srgb_rgba8().unwrap();
    let mut selected_changes=0;
    for y in 108..148 {for x in 232..280 {
        let at=(y*512+x)*4;
        if x<256 {selected_changes+=usize::from(after[at..at+4]!=before[at..at+4]);}
        else {assert_eq!(&after[at..at+4],&before[at..at+4],"unselected pixel {x},{y}");}
    }}
    assert!(selected_changes>100,"the selected half of the dab must deform the image");
}

#[test]
fn liquify_adjust_zero_half_and_full_use_the_anchored_field() {
    let extent=[256;2];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"liquify adjust reference");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,_| if x%4<2 {[0,0,0,255]} else {[255;4]}));
    r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let anchor=seeded_context(&r,&doc,|_,_|([2.,0.],[1.,0.,0.,1.]));
    for (i,amount) in [0.,0.5,1.].into_iter().enumerate() {
        let mut context=anchor.clone();context.amount=Some(amount);
        let image=render_seeded(&mut r,&mut doc,context,70+i as u64);
        assert_eq!(placement::paint(&doc).raster.liquify().is_some(), amount != 0.);
        for x in 60..68 {let at=(64*256+x)*4;let source=x+(2.*amount) as usize;
            let expected=if source%4<2 {0}else{255};assert_eq!(&image[at..at+4],&[expected,expected,expected,255],"amount {amount}, pixel {x}");
        }
    }
}

#[test]
fn fit_amount_preview_keeps_authored_color_and_matches_full_resolution_commit() {
    let extent=[512,256];
    let source=layer_core::color::source::rgba8_source(extent,|x,_| {
        if x%32<16 {[235,35,45,255]} else {[35,85,235,255]}
    });
    let mut fit=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"liquify Fit Amount preview");
    placement::set_source(&mut doc,source.clone());
    let view=layer_render::ViewState{width_px:64,height_px:32,
        document_to_surface:[0.125,0.,0.,0.125,0.,0.]};
    crate::layer_tests::publish(&mut fit,FramePacket{view,reset_layers:true,..crate::test_support::packet(doc.scene(),extent)});
    let before=crate::scene::scale::tests::display_pixels(&fit);
    let target=placement::target(&doc);
    let colors=|r:&WgpuRasterizer|r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter()
        .map(|page|(page.coordinate,crate::layer_tests::page_bytes(r,&page.active().texture)))
        .collect::<BTreeMap<_,_>>();
    let authored=colors(&fit);
    let anchor=seeded_context(&fit,&doc,|_,_|([16.,0.],[1.,0.,0.,1.]));
    for (index,amount) in [0.,0.5,1.].into_iter().enumerate() {
        let mut context=anchor.clone();context.amount=Some(amount);
        let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
        style.liquify=Some(Arc::new(context));
        let mut preview=crate::test_support::dab_batch(target,style,layer_core::Rect::from_extent(extent));
        preview.kind=DabBatchKind::Preview;
        preview.stroke_id=StrokeId(260+index as u64);
        preview.dab_count=0;
        preview.stroke_end=false;
        fit.submit(FramePacket{view,dab_batches:&[preview],
            ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
        assert_eq!(fit.preview_level,2,"Amount {amount} must use a reduced Fit preview");
        assert!(fit.liquify.as_ref().unwrap().strokes.get(&target).is_some_and(|stroke|stroke.preview_only),
            "Amount {amount} preview must have a preview-only stroke");
        assert!(colors(&fit)==authored,"Amount {amount} preview must leave authored color untouched");
        let display=crate::scene::scale::tests::display_pixels(&fit);
        let changes=display.iter().zip(&before).filter(|(a,b)|a!=b).count();
        if amount==0. {assert_eq!(changes,0,"zero Amount preview must restore the original display");}
        else {assert!(changes>100,"Amount {amount} preview must be visible at Fit scale: {changes} pixels");}
        fit.submit(FramePacket{view,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
        assert!(!fit.liquify.as_ref().unwrap().strokes.contains_key(&target),
            "a no-batch frame must discard the canceled Amount {amount} preview stroke");
        assert!(crate::scene::scale::tests::display_pixels(&fit)==before,
            "cancel must restore Amount {amount} display");
        assert!(colors(&fit)==authored,"cancel must preserve authored color");
    }
    let mut context=anchor;context.amount=Some(0.5);
    let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
    style.liquify=Some(Arc::new(context));
    let mut commit=crate::test_support::dab_batch(target,style,layer_core::Rect::from_extent(extent));
    commit.stroke_id=StrokeId(264);
    commit.dab_count=0;
    placement::paint_mut(&mut doc).raster=RasterRevision::pending();
    crate::test_support::submit_ready(&mut fit,FramePacket{view,dab_batches:&[commit],
        ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let fit_colors=colors(&fit);
    assert!(fit_colors!=authored,"committing Amount must publish full-resolution authored color");

    let mut exact=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut reference=placement::paint_document(extent,"liquify Amount full resolution reference");
    placement::set_source(&mut reference,source);
    crate::test_support::submit_settled(&mut exact,FramePacket{reset_layers:true,..crate::test_support::packet(reference.scene(),extent)});
    let mut context=seeded_context(&exact,&reference,|_,_|([16.,0.],[1.,0.,0.,1.]));
    context.amount=Some(0.5);
    let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
    style.liquify=Some(Arc::new(context));
    let mut commit=crate::test_support::dab_batch(placement::target(&reference),style,
        layer_core::Rect::from_extent(extent));
    commit.stroke_id=StrokeId(264);
    commit.dab_count=0;
    placement::paint_mut(&mut reference).raster=RasterRevision::pending();
    crate::test_support::submit_ready(&mut exact,FramePacket{dab_batches:&[commit],
        ..crate::test_support::packet(reference.scene(),extent)}).unwrap();
    assert!(fit_colors==colors(&exact),"Fit Amount commit must match native full-resolution color exactly");
}

#[test]
fn liquify_edge_and_reconstruct_transport_the_seeded_map_and_footprint() {
    let extent=[256;2];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"liquify map edit oracles");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|_,_|[255;4]));
    r.submit(FramePacket{reset_layers:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    for (i,mode) in [LiquifyMode::Edge,LiquifyMode::Reconstruct].into_iter().enumerate() {
        let context=seeded_context(&r,&doc,|_,_|([2.,0.],[1.,0.,0.,1.]));
        let mut style=preset_style(DefaultBrushPreset::LiquifyPush);style.deform.mode=mode;style.liquify=Some(Arc::new(context));
        placement::paint_mut(&mut doc).raster=RasterRevision::pending();
        let mut contact=dab([0.;4]);contact.material[3]=1.;contact.motion=[1.,0.];contact.previous_contact=[1.,0.,0.,1.];
        let mut batch=crate::test_support::dab_batch(placement::target(&doc),style,layer_core::Rect::from_extent([128;2]));batch.stroke_id=StrokeId(80+i as u64);
        r.submit(FramePacket{dabs:&[contact],dab_batches:&[batch],..crate::test_support::packet(doc.scene(),extent)}).unwrap();
        let state=placement::paint(&doc).raster.liquify().unwrap();let field=state.field.as_any().downcast_ref::<Field>().unwrap();
        let bytes=crate::layer_tests::page_bytes(&r,&field.tiles[&[0,0]].texture);
        let at=(72*256+64)*16;
        let words:Vec<_>=bytes[at..at+16].as_chunks::<4>().0.iter().map(|b|u32::from_ne_bytes(*b)).collect();
        let displacement=[f32::from_bits(words[0]),f32::from_bits(words[1])];
        if mode==LiquifyMode::Edge {
            assert!((displacement[0]-2.).abs()<1e-5 && (displacement[1]-2.975).abs()<1e-5,"Edge {displacement:?}");
            assert_eq!(f32::from_bits(words[2]<<16),1.);
            assert!((f32::from_bits(words[3]&0xffff0000)-1.35).abs()<0.004);
        } else {
            assert_eq!(displacement,[0.;2]);assert_eq!(words[2],0x00003f80);assert_eq!(words[3],0x3f800000);
        }
    }
}

#[test]
fn smooth_liquify_gpu_map_and_footprint_match_numerical_reference() {
    fn influence(p: [f64; 2]) -> f64 {
        let distance = (p[0] - 64.).hypot(p[1] - 64.) / 32.;
        0.6 * ((1. - distance) / 0.5).clamp(0., 1.)
    }
    fn old_map(p: [f64; 2]) -> [f64; 2] {
        [2. + 0.03125 * (p[0] - 64.), -1. + 0.015625 * (p[1] - 64.)]
    }
    fn expected(mode: LiquifyMode, p: [f64; 2]) -> [f64; 2] {
        let s = influence(p);
        let d = [p[0] - 64., p[1] - 64.];
        let traced = match mode {
            LiquifyMode::Push => [p[0] - 4. * s, p[1] + 2. * s],
            LiquifyMode::TwirlCounterclockwise | LiquifyMode::TwirlClockwise => {
                let sign = if mode == LiquifyMode::TwirlCounterclockwise { 1. } else { -1. };
                let angle = sign * 0.75 * s;
                [64. + angle.cos() * d[0] - angle.sin() * d[1],
                    64. + angle.sin() * d[0] + angle.cos() * d[1]]
            }
            LiquifyMode::Pinch | LiquifyMode::Expand => {
                let sign = if mode == LiquifyMode::Pinch { 1. } else { -1. };
                [64. + (1. + sign * 0.35 * s) * d[0], 64. + (1. + sign * 0.35 * s) * d[1]]
            }
            LiquifyMode::Edge => {
                let distance = 0.6 * d[0] + 0.8 * d[1];
                [p[0] + 0.35 * s * distance * 0.6, p[1] + 0.35 * s * distance * 0.8]
            }
            LiquifyMode::Reconstruct => p,
            _ => unreachable!(),
        };
        let old = old_map(traced);
        if mode == LiquifyMode::Reconstruct {
            [p[0] + (1. - s) * old[0], p[1] + (1. - s) * old[1]]
        } else { [traced[0] + old[0], traced[1] + old[1]] }
    }
    fn unpack(bytes: &[u8], p: [f64; 2]) -> ([f64; 2], [f64; 4]) {
        let at = ((p[1] as usize * 256) + p[0] as usize) * 16;
        let word = |i: usize| u32::from_ne_bytes(bytes[at + i * 4..at + i * 4 + 4].try_into().unwrap());
        let bf = |bits: u32| f32::from_bits(bits << 16) as f64;
        ([f32::from_bits(word(0)) as f64, f32::from_bits(word(1)) as f64],
            [bf(word(2) & 65535), bf(word(3) & 65535), bf(word(2) >> 16), bf(word(3) >> 16)])
    }
    fn bilinear(bytes: &[u8], p: [f64; 2]) -> ([f64; 2], [f64; 4]) {
        let x = (p[0] - 0.5).floor();
        let y = (p[1] - 0.5).floor();
        let tx = p[0] - 0.5 - x;
        let ty = p[1] - 0.5 - y;
        let mut u = [0.; 2];
        let mut j = [0.; 4];
        for (dx, dy, weight) in [(0., 0., (1. - tx) * (1. - ty)),
            (1., 0., tx * (1. - ty)), (0., 1., (1. - tx) * ty), (1., 1., tx * ty)] {
            let (sample_u, sample_j) = unpack(bytes, [x + dx + 0.5, y + dy + 0.5]);
            for axis in 0..2 { u[axis] += sample_u[axis] * weight; }
            for axis in 0..4 { j[axis] += sample_j[axis] * weight; }
        }
        (u, j)
    }

    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc = placement::paint_document([128; 2], "liquify finite difference reference");
    placement::set_source(&mut doc, layer_core::color::source::rgba8_source([128; 2], |_, _| [255; 4]));
    renderer.submit(FramePacket { reset_layers: true,
        ..crate::test_support::packet(doc.scene(), [128; 2]) }).unwrap();
    let points = [[85.5, 71.5], [79.5, 79.5], [69.5, 86.5], [56.5, 43.5]];
    let mut max_displacement_error: f64 = 0.;
    let mut max_footprint_error: f64 = 0.;
    let mut previous_map = Vec::new();
    for (index, mode) in [LiquifyMode::Push, LiquifyMode::TwirlCounterclockwise,
        LiquifyMode::TwirlClockwise, LiquifyMode::Pinch, LiquifyMode::Expand,
        LiquifyMode::Edge, LiquifyMode::Reconstruct].into_iter().enumerate() {
        let context = seeded_context(&renderer, &doc, |x, y| {
            ([2. + 0.03125 * (x as f32 + 0.5 - 64.),
                -1. + 0.015625 * (y as f32 + 0.5 - 64.)], [1.03125, 0., 0., 1.015625])
        });
        let mut style = preset_style(DefaultBrushPreset::LiquifyPush);
        style.deform.mode = mode;
        style.liquify = Some(Arc::new(context));
        placement::paint_mut(&mut doc).raster = RasterRevision::pending();
        let mut contact = dab([0.; 4]);
        contact.center = Point { x: 64., y: 64. };
        contact.radii = [32.; 2];
        contact.hardness = 0.5;
        contact.material[3] = 0.6;
        contact.motion = [4., -2.];
        contact.previous_contact = [1., 0., 0.6, 0.8];
        let mut batch = crate::test_support::dab_batch(placement::target(&doc), style, contact.bounds());
        batch.stroke_id = StrokeId(300 + index as u64);
        renderer.submit(FramePacket { dabs: &[contact], dab_batches: &[batch],
            ..crate::test_support::packet(doc.scene(), [128; 2]) }).unwrap();
        let state = placement::paint(&doc).raster.liquify().unwrap();
        let field = state.field.as_any().downcast_ref::<Field>().unwrap();
        let bytes = crate::layer_tests::page_bytes(&renderer, &field.tiles[&[0, 0]].texture);
        for p in points {
            let (gpu_u, gpu_j) = unpack(&bytes, p);
            let target = expected(mode, p);
            let reference_u = [target[0] - p[0], target[1] - p[1]];
            let mut reference_j = [0.; 4];
            for axis in 0..2 {
                let h = 0.01;
                let mut plus = p; plus[axis] += h;
                let mut minus = p; minus[axis] -= h;
                let a = expected(mode, plus);
                let b = expected(mode, minus);
                reference_j[axis] = (a[0] - b[0]) / (2. * h);
                reference_j[axis + 2] = (a[1] - b[1]) / (2. * h);
            }
            for axis in 0..2 {
                let error = (gpu_u[axis] - reference_u[axis]).abs();
                max_displacement_error = max_displacement_error.max(error);
                assert!(error < 0.015, "{mode:?} {p:?} displacement axis {axis}: GPU {}, reference {}", gpu_u[axis], reference_u[axis]);
            }
            for axis in 0..4 {
                let error = (gpu_j[axis] - reference_j[axis]).abs();
                max_footprint_error = max_footprint_error.max(error);
                assert!(error < 0.035, "{mode:?} {p:?} footprint axis {axis}: GPU {}, reference {}", gpu_j[axis], reference_j[axis]);
            }
        }
        previous_map = bytes;
    }
    let mut style = preset_style(DefaultBrushPreset::LiquifyPush);
    style.liquify = Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc), false)));
    placement::paint_mut(&mut doc).raster = RasterRevision::pending();
    let mut contact = dab([0.; 4]);
    contact.center = Point { x: 64., y: 64. };
    contact.radii = [32.; 2];
    contact.hardness = 0.5;
    contact.material[3] = 0.6;
    contact.motion = [4., -2.];
    contact.previous_contact[0] = 1.;
    let mut batch = crate::test_support::dab_batch(placement::target(&doc), style, contact.bounds());
    batch.stroke_id = StrokeId(308);
    renderer.submit(FramePacket { dabs: &[contact], dab_batches: &[batch],
        ..crate::test_support::packet(doc.scene(), [128; 2]) }).unwrap();
    let state = placement::paint(&doc).raster.liquify().unwrap();
    let field = state.field.as_any().downcast_ref::<Field>().unwrap();
    let composed = crate::layer_tests::page_bytes(&renderer, &field.tiles[&[0, 0]].texture);
    for p in points {
        let s = influence(p);
        let g = [p[0] - 4. * s, p[1] + 2. * s];
        let (prior_u, prior_j) = bilinear(&previous_map, g);
        let (gpu_u, gpu_j) = unpack(&composed, p);
        let reference_u = [g[0] - p[0] + prior_u[0], g[1] - p[1] + prior_u[1]];
        let h = 0.01;
        let mut g_j = [0.; 4];
        for axis in 0..2 {
            let mut plus = p; plus[axis] += h;
            let mut minus = p; minus[axis] -= h;
            let a = influence(plus); let b = influence(minus);
            g_j[axis] = f64::from(axis == 0) - 4. * (a - b) / (2. * h);
            g_j[axis + 2] = f64::from(axis == 1) + 2. * (a - b) / (2. * h);
        }
        let reference_j = [prior_j[0] * g_j[0] + prior_j[1] * g_j[2],
            prior_j[0] * g_j[1] + prior_j[1] * g_j[3],
            prior_j[2] * g_j[0] + prior_j[3] * g_j[2],
            prior_j[2] * g_j[1] + prior_j[3] * g_j[3]];
        for axis in 0..2 {
            let error = (gpu_u[axis] - reference_u[axis]).abs();
            max_displacement_error = max_displacement_error.max(error);
            assert!(error < 0.015, "mixed Reconstruct→Push {p:?} displacement axis {axis}: {gpu_u:?} vs {reference_u:?}");
        }
        for axis in 0..4 {
            let error = (gpu_j[axis] - reference_j[axis]).abs();
            max_footprint_error = max_footprint_error.max(error);
            assert!(error < 0.035, "mixed Reconstruct→Push {p:?} footprint axis {axis}: {gpu_j:?} vs {reference_j:?}");
        }
    }
    println!("smooth liquify GPU oracle: max displacement error {max_displacement_error:.6} px, max footprint component error {max_footprint_error:.6}");
}

#[test]
fn sustained_pinch_preview_keeps_opaque_center_at_finite_footprint() {
    let extent=[256;2];
    let view=layer_render::ViewState{width_px:32,height_px:32,
        document_to_surface:[0.125,0.,0.,0.125,0.,0.]};
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc=placement::paint_document(extent,"sustained Pinch finite preview");
    placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|_,_|[83,149,211,255]));
    crate::test_support::submit_settled(&mut r,FramePacket{view,reset_layers:true,..crate::test_support::packet(doc.scene(),extent)});
    let target=placement::target(&doc);
    let mut contact=dab([0.;4]);
    contact.center=Point{x:126.5,y:126.5};
    contact.radii=[64.;2];
    contact.material[3]=1.;
    contact.previous_contact[0]=3.;
    let mut style=preset_style(DefaultBrushPreset::LiquifyPinch);
    style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
    let dabs=[contact;272];
    let batches=(0..34).map(|chunk|{
        let mut batch=crate::test_support::dab_batch(target,style.clone(),contact.bounds());
        batch.stroke_id=StrokeId(310);
        batch.stroke_start=chunk==0;
        batch.stroke_end=chunk==33;
        batch.first_dab=chunk*8;
        batch.dab_count=8;
        batch
    }).collect::<Vec<_>>();
    placement::paint_mut(&mut doc).raster=RasterRevision::pending();
    crate::test_support::submit_ready(&mut r,FramePacket{view,dabs:&dabs,dab_batches:&batches,
        ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let field=placement::paint(&doc).raster.liquify().unwrap().field.clone();
    let field=field.as_any().downcast_ref::<Field>().unwrap();
    let map=crate::layer_tests::page_bytes(&r,&field.tiles[&[0,0]].texture);
    let at=(126*256+126)*16+8;
    let packed=u32::from_ne_bytes(map[at..at+4].try_into().unwrap());
    let center_j=f32::from_bits((packed&0xffff)<<16);
    assert!(center_j>1.0e38,"sustained Pinch must reach a large finite native footprint: {center_j}");
    style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
    let mut predicted=contact;
    predicted.center=Point{x:126.,y:126.};
    let mut preview=crate::test_support::dab_batch(target,style,predicted.bounds());
    preview.kind=DabBatchKind::Preview;
    preview.stroke_id=StrokeId(311);
    preview.dab_count=8;
    r.submit(FramePacket{view,composite_all:false,dabs:&[predicted;8],dab_batches:&[preview],
        ..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    assert_eq!(r.preview_level,2,"Fit preview must use a four-pixel map block");
    let page=r.preview_pages.iter().find(|page|page.coordinate==[0,0]).unwrap();
    let center=crate::test_support::float_pixels(&r,&page.active().texture)[31*64+31];
    let original=[83,149,211,255].map(|code|
        layer_core::color::RgbSpace::Srgb.decode(f64::from(code)/255.) as f32);
    for component in 0..4 {
        assert!(center[component].is_finite() && (center[component]-original[component]).abs()<=1e-5,
            "finite Pinch preview must preserve source component {component}: {center:?} vs {original:?}, native J={center_j}");
    }
}

#[test]
fn a_new_stroke_prepares_the_displaced_footprint_of_its_saved_field() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let extent=[2048;2];let document=placement::paint_document(extent,"saved field bounds");
    let source=Arc::new(placement::paint(&document).clone());
    let field=Field {device:(*r.device).clone(),tiles:[([0;2],Arc::new(Tile::new(&r,PixelRect::new(0,0,1536,1536))))].into(),originals:Default::default()};
    let mut style=preset_style(DefaultBrushPreset::LiquifyPush);
    style.liquify=Some(Arc::new(layer_core::liquify::Context {current:source.clone(),original:source,amount:Some(0.5),field:Some(Arc::new(field))}));
    let mut batch=crate::test_support::dab_batch(placement::target(&document),style,layer_core::Rect::from_extent([256;2]));
    batch.stroke_start=true;batch.dab_count=0;
    let packet=FramePacket {dab_batches:std::slice::from_ref(&batch),..crate::test_support::packet(document.scene(),extent)};
    let tiles=vec![vec![BrushTile {coordinate:[0;2],local:PixelRect::full([PAGE_SIZE;2]),dabs:0..0,indices:Vec::new()}]];
    let mut required=Default::default();Liquify::required_sources(&r,packet,&tiles,&mut required);
    assert!(required[&batch.target].contains(&[5,5]));
    assert!(required[&batch.target].contains(&[0,0]));
}

#[test]
fn liquify_shaders_store_footprints_without_hlsl_matrix_member_setters() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let shaders=(0..8).flat_map(|mode|[false,true].map(|analytic|super::map_shader(mode,analytic))).chain([super::color_shader(&r.device)]);
    for source in shaders {
        let module=naga::front::wgsl::parse_str(&source).unwrap();
        let info=naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
        let (module,info)=naga::back::pipeline_constants::process_overrides(&module,&info,None,&Default::default()).unwrap();
        let options=naga::back::hlsl::Options {shader_model:naga::back::hlsl::ShaderModel::V6_0,..Default::default()};
        let pipeline=Default::default();
        let mut hlsl=String::new();
        naga::back::hlsl::Writer::new(&mut hlsl,&options,&pipeline).write(&module,&info,None).unwrap();
        let calls:Vec<_>=hlsl.lines().filter(|line|line.contains("SetMat")&&!line.trim_start().starts_with("void ")).collect();
        assert!(calls.is_empty(),"naga's HLSL matrix member setters copy their struct, so Direct3D drops the stored footprint: {calls:?}");
    }
}
