use super::*;

fn resolved(r:&mut WgpuRasterizer,scene:&mut Scene,doc:&layer_core::Document,request:&crate::scene::objects::CollectionJob)->wgpu::TextureView {
    let mut cache=crate::scene::object_cache::ObjectCache::default();
    for _ in 0..4000 {
        if let Some(view)=cache.resolve_collection(r,doc.scene(),request).unwrap() {return view;}
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        cache.advance(r,scene,&mut encoder,false).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("independent canonical job must complete");
}

fn float_photo()->AuthoredImage {
    let extent=[1025,769];
    let mut builder=SourceBuilder::new(extent,SourceInterpretation {channels:SourceChannels::Rgba,
        depth:SampleDepth::F32,profile:Default::default(),profile_assumed:false},64<<20).unwrap();
    for y in 0..extent[1] {
        let row:Vec<_>=(0..extent[0]).flat_map(|x| {
            [((x*37+y*113)%257) as f32/256.,((x*67+y*11)%251) as f32/250.,
                0.125+x as f32*0.000031+y as f32*0.000017,0.25+((x^y)%17) as f32/32.]
        }).flat_map(f32::to_le_bytes).collect();
        builder.push_row(&row).unwrap();
    }
    AuthoredImage::new(Arc::new(builder.finish().unwrap()))
}

#[test]
fn canonical_blocks_and_independent_regions_preserve_pixel_precision() {
    let source=float_photo();
    let cases=[
        (false,[0.73,0.17,-0.11,0.91,21.375,14.625],[83,117],[61,73],1.),
        (false,[0.41,-0.12,0.17,0.63,-37.125,88.875],[185,127],[71,53],1.),
        (false,[-0.83,0.19,0.07,1.11,717.125,-17.375],[31,237],[673,93],1.),
        (true,[0.73,0.17,-0.11,0.91,21.375,14.625],[83,117],[61,73],1.),
        (false,[0.73,0.17,-0.11,0.91,21.375,14.625],[100,92],[61,73],4.),
        (false,[0.41,-0.12,0.17,0.63,-37.125,88.875],[-77,109],[95,57],1.),
        (true,[-0.83,0.19,0.07,1.11,717.125,-17.375],[31,237],[673,93],1.),
    ];
    let mut failures=Vec::new();
    for (case,(nearest,affine,origin,size,side)) in cases.into_iter().enumerate() {
        let mut object=ImageObject::new(source.clone());object.affine=Affine64(affine);
        if nearest {object.interpolation=ImageInterpolation::Nearest;}
        let (doc,owner,_)=document([2048;2],vec![object]);
        let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
        r.document_extent=[2048;2];crate::memory::memory_tests::legacy_scene_frame(&mut r,packet(doc.scene(),[2048;2]));
        let mut scene=Scene::new(&r).unwrap();scene.object_spatial.prepare(doc.scene());
        let window=ObjectWindow {origin:origin.map(f64::from),side,size};
        let reference_request=canonical_request(&r,&mut scene,&doc,owner,window);
        let reference=resolved(&mut r,&mut scene,&doc,&reference_request);
        let expected=float_pixels(&r,reference.texture());
        let control_window=ObjectWindow {origin:[window.origin[0]-17.*side,window.origin[1]-11.*side],side,size:[size[0]+37,size[1]+29]};
        let control_request=canonical_request(&r,&mut scene,&doc,owner,control_window);
        let control=resolved(&mut r,&mut scene,&doc,&control_request);
        let control_pixels=float_pixels(&r,control.texture());
        let parent:Vec<_>=(0..size[1]).flat_map(|y| {
            let row=&control_pixels[((y+11)*control_window.size[0]+17) as usize..];
            row[..size[0] as usize].iter().map(|pixel|pixel.map(f32::to_bits))
        }).collect();
        let mut actual=vec![[0u32;4];(size[0]*size[1]) as usize];
        let span=f64::from(crate::scene::objects::COLLECTION_PART)*side;
        let level=side.log2() as u32;
        let support=scene.object_spatial.content(doc.scene(),owner).unwrap().bounds_at(level).aligned(side as u32);
        let grid_window=|cell:[i64;2]| {
            let rect=DocRect {min:cell.map(|v|(v as f64*span) as i64),max:cell.map(|v|((v+1) as f64*span) as i64)}.intersect(support);
            (!rect.is_empty()).then(||ObjectWindow {origin:rect.min.map(|v|v as f64),side,size:rect.size().unwrap().map(|v|v/(side as u32))})
        };
        let low=origin.map(|v|(f64::from(v)/span).floor() as i64);
        let high=std::array::from_fn::<_,2,_>(|axis|((f64::from(origin[axis])+f64::from(size[axis])*side)/span).ceil() as i64);
        for by in low[1]..high[1] {for bx in low[0]..high[0] {
            let Some(block)=grid_window([bx,by]) else {continue;};
            let request=canonical_request(&r,&mut scene,&doc,owner,block);
            let view=resolved(&mut r,&mut scene,&doc,&request);let pixels=float_pixels(&r,view.texture());
            let repeated=resolved(&mut r,&mut scene,&doc,&request);let repeated=float_pixels(&r,repeated.texture());
            assert!(pixels.iter().zip(&repeated).all(|(a,b)|a.map(f32::to_bits)==b.map(f32::to_bits)),"case{case}: repeated fixed block must be deterministic");
            for y in 0..size[1] {for x in 0..size[0] {
                let point=[f64::from(origin[0])+f64::from(x)*side,f64::from(origin[1])+f64::from(y)*side];
                if (0..2).all(|axis|point[axis]>=block.origin[axis] && point[axis]<block.origin[axis]+f64::from(block.size[axis])*side) {
                    let local=std::array::from_fn::<_,2,_>(|axis|((point[axis]-block.origin[axis])/side) as usize);
                    actual[(y*size[0]+x) as usize]=pixels[local[1]*block.size[0] as usize+local[0]].map(f32::to_bits);
                }
            }}
        }}
        let sample=|request:&crate::scene::objects::CollectionJob,point:[u32;2]| {
            let part_size=request.part(0).size;
            let columns=request.window.size[0].div_ceil(crate::scene::objects::COLLECTION_PART);
            let cell=std::array::from_fn::<_,2,_>(|axis|point[axis]/part_size[axis]);
            let part=request.part(cell[1]*columns+cell[0]);
            let [a,b,c,d,tx,ty]=request.child(0,part,r.empty_view.clone()).unwrap().inverse;
            let [x,y]=std::array::from_fn::<_,2,_>(|axis|f64::from(point[axis]-cell[axis]*part_size[axis])+0.5);
            [a*x+c*y+tx,b*x+d*y+ty]
        };
        let mut parent_max=0f64;
        for (kind,actual) in [("parent",&parent),("grid",&actual)] {
            let mut maximum=[0f64;4];let mut border_max=0f64;let mut interior_max=0f64;let mut non_ties=0usize;
            for (index,(bits,expected)) in actual.iter().zip(&expected).enumerate() {
                let pixel=bits.map(f32::from_bits);let errors=std::array::from_fn::<_,4,_>(|axis|(f64::from(pixel[axis])-f64::from(expected[axis])).abs());
                for axis in 0..4 {maximum[axis]=maximum[axis].max(errors[axis]);}
                let differs=*bits!=expected.map(f32::to_bits);
                let point=[index as u32%size[0],index as u32/size[0]];
                let global=std::array::from_fn::<_,2,_>(|axis|f64::from(origin[axis])+f64::from(point[axis])*side+side*0.5);
                let border=global.iter().any(|v| {let phase=(v/side).rem_euclid(f64::from(crate::scene::objects::COLLECTION_PART));phase.min(f64::from(crate::scene::objects::COLLECTION_PART)-phase)<=1.5});
                let error=errors.into_iter().fold(0f64,f64::max);
                if border {border_max=border_max.max(error);} else {interior_max=interior_max.max(error);}
                if nearest && differs {
                    let reference_sample=sample(&reference_request,point);
                    let other_sample=if kind=="parent" {sample(&control_request,[point[0]+17,point[1]+11])} else {
                        let block=grid_window(global.map(|value|(value/span).floor() as i64)).unwrap();
                        let request=canonical_request(&r,&mut scene,&doc,owner,block);
                        sample(&request,std::array::from_fn(|axis|((global[axis]-side*0.5-block.origin[axis])/side) as u32))
                    };
                    let different=(0..2).any(|axis|reference_sample[axis].floor()!=other_sample[axis].floor());
                    let ties=different && (0..2).all(|axis|reference_sample[axis].floor()==other_sample[axis].floor()
                        || (reference_sample[axis].round()==other_sample[axis].round()
                            && (reference_sample[axis]-reference_sample[axis].round()).abs()<=1e-9
                            && (other_sample[axis]-other_sample[axis].round()).abs()<=1e-9));
                    if !ties {non_ties+=1;}
                }
            }
            let error=maximum.into_iter().fold(0f64,f64::max);
            if kind=="parent" {parent_max=error;}
            let limit=if parent_max==0. {2f64.powi(-16)} else {2.*parent_max};
            let ratio=if interior_max==0. {if border_max==0. {0.} else {f64::INFINITY}} else {border_max/interior_max};
            if nearest && non_ties>0 || !nearest && kind=="grid" && (error>limit || ratio>2.) {
                failures.push(format!("case{case} {kind}: precision class exceeded: max={maximum:?}, parent={parent_max:e}, bound={limit:e}, border/interior={ratio}, non_ties={non_ties}"));
            }
        }
    }
    assert!(failures.is_empty(),"{}",failures.join("\n"));
}

#[test]
fn full_block_mapping_can_refuse_a_supported_nearest_region() {
    let inverse=[1_000_000.,0.,0.,1.,-128_499_872.,0.];
    assert!(crate::object_sampling::mapping(inverse,[256;2]).is_ok());
    assert!(matches!(crate::object_sampling::mapping(inverse,[512;2]),Err(GpuRasterError::ExtentUnsupported)));
    let mut object=ImageObject::new(image([256;2],[255,0,0,255]));
    object.interpolation=ImageInterpolation::Nearest;object.affine=Affine64([0.000002,0.,0.,2.,256.999744,0.]);
    let (doc,owner,handles)=document([1024;2],vec![object]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    r.document_extent=[1024;2];let frame=packet(doc.scene(),[1024;2]);crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
    crate::object_sampling::preflight_object_affine(doc.scene(),handles[0],doc.scene().object(handles[0]).unwrap().affine,
        layer_render::ViewState {width_px:512,height_px:512,document_to_surface:[0.5,0.,0.,0.5,0.,0.]},r.device.limits().max_storage_buffer_binding_size).unwrap();
    let mut scene=Scene::new(&r).unwrap();scene.object_spatial.prepare(doc.scene());
    let request=canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {origin:[0.;2],side:2.,size:[256;2]});
    let view=resolved(&mut r,&mut scene,&doc,&request);let pixels=float_pixels(&r,view.texture());
    assert_eq!(pixels[128],[1.,0.,0.,1.]);
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    for _ in 0..4000 {
        if let Some(pieces)=scene.canonical_cover(&r,frame,&content,&request).unwrap() {
            assert_eq!(pieces.len(),1);
            assert_eq!(pieces[0].1,content.bounds_at(1).aligned(2));
            assert_eq!(float_pixels(&r,pieces[0].0.texture())[0],[1.,0.,0.,1.]);
            return;
        }
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        let mut cache=std::mem::take(&mut scene.object_results);
        cache.advance(&mut r,&mut scene,&mut encoder,false).unwrap();scene.object_results=cache;
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("owner-clipped blocks retain supported nearest mappings");
}

#[test]
fn signed_canonical_blocks_reuse_evaluation_offsets_and_keep_owner_and_level_identity() {
    let mut object=ImageObject::new(image([1024;2],[31,73,149,255]));
    object.affine.0[4]=-700.;object.affine.0[5]=-600.;
    let (doc,owner,handles)=document([2048;2],vec![object.clone(),object]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    r.document_extent=[2048;2];let frame=packet(doc.scene(),[2048;2]);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
    let mut scene=Scene::new(&r).unwrap();
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    let window=ObjectWindow {origin:[-512.;2],side:4.,size:[128;2]};
    let request=scene.collection_job(&r,frame,owner,content.clone(),window);
    assert!(scene.canonical_cover(&r,frame,&content,&request).unwrap().is_none());
    let offset=[4096.,-8192.];let shifted=packet(doc.scene().with_offset64(offset),[2048;2]);
    let shifted_content=scene.object_spatial.content(shifted.scene,owner).unwrap();
    let shifted_request=scene.collection_job(&r,shifted,owner,shifted_content.clone(),
        ObjectWindow {origin:std::array::from_fn(|i|window.origin[i]+offset[i]),..window});
    assert!(request.same(&shifted_request));
    assert!(scene.canonical_cover(&r,shifted,&shifted_content,&shifted_request).unwrap().is_none());
    assert_eq!(scene.object_results.work_progress().0,1,"offset views reuse the same pending block");
    let mut ready=None;
    for _ in 0..4000 {
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        let mut cache=std::mem::take(&mut scene.object_results);
        cache.advance(&mut r,&mut scene,&mut encoder,false).unwrap();scene.object_results=cache;
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        if let Some(pieces)=scene.canonical_cover(&r,frame,&content,&request).unwrap() {ready=Some(pieces);break;}
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let pieces=ready.expect("signed canonical block completes");
    let bounds=DocRect {min:[-2048;2],max:[0;2]}.intersect(content.bounds_at(2).aligned(4));
    assert_eq!(pieces.len(),1);assert_eq!(pieces[0].1,bounds);
    let shifted_pieces=scene.canonical_cover(&r,shifted,&shifted_content,&shifted_request).unwrap().unwrap();
    assert_eq!(shifted_pieces[0].1,bounds);assert_eq!(scene.object_results.work_progress().0,1);
    let other=doc.scene().object_owner(handles[1]).unwrap();
    let other_content=scene.object_spatial.content(doc.scene(),other).unwrap();
    let other_request=scene.collection_job(&r,frame,other,other_content.clone(),window);
    assert!(scene.canonical_cover(&r,frame,&other_content,&other_request).unwrap().is_none());
    let coarser=scene.collection_job(&r,frame,owner,content.clone(),ObjectWindow {side:8.,size:[64;2],..window});
    assert!(scene.canonical_cover(&r,frame,&content,&coarser).unwrap().is_none());
    assert_eq!(scene.object_results.work_progress().0,3,"owners and levels have distinct blocks");
}

#[test]
fn eight_ready_photo_mips_resettle_without_fragmented_canonical_work() {
    let (doc,_,_,mut view)=tablet_scene(true,8,false);
    view.document_to_surface=[0.1597,0.,0.,0.1597,670.09,445.26];
    let mut r=crate::memory::memory_tests::renderer_for_restore(&doc);
    let frame=FramePacket {view,..packet(doc.scene(),[4248,2832])};
    crate::memory::memory_tests::declare_source_budget(&mut r,frame,400,64<<20,380<<20);
    settle_live_frames(&mut r,frame,4000);
    assert!(!r.moving_images_pending());assert!(!r.scene.as_ref().unwrap().object_results.pending());
    let completed=r.moving_images.completed_images;let built=r.moving_images.tile_builds;
    r.scale_display=None;
    let scene=r.scene.as_mut().unwrap();scene.object_results.clear();
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    scene.object_results.flush_retired(&encoder);r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
    let revision=r.composite_revision;
    let frames=settle_live_frames(&mut r,frame,4000);
    assert_eq!(r.moving_images.completed_images,completed);assert_eq!(r.moving_images.tile_builds,built);
    assert!(r.composite_revision!=revision);assert!(!r.object_deferred);
    assert!(!r.scene.as_ref().unwrap().object_results.pending());
    assert!(r.scale_display.as_ref().is_some_and(|cache|!cache.approximating()));
    assert!(frames<400,"ready mips must not fragment exact resettlement: {frames} frames");
}
