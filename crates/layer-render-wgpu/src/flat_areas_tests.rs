use super::*;
use crate::test_support::{packet,submit_ready};
use crate::layer_tests::placement::{self,paint_document,target};
use layer_core::{Document,color::SampleDepth, raster::{RasterPlane, RasterRevision, TILE_SIZE}};
use layer_render::{RegionEdges, RegionOutput, RegionRefinement, RegionRequest, RegionSource, RegionTask};
use std::collections::VecDeque;

fn source(extent:[u32;2], ink:impl Fn(u32,u32)->bool)->Document {
    let mut document=paint_document(extent,"Flat areas reference");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y|if ink(x,y){[0,0,0,255]}else{[255;4]}));
    document
}
fn frame(r:&mut WgpuRasterizer,document:&Document,extent:[u32;2]) {
    let mut packet=packet(document.scene(),extent);
    packet.reset_layers=true;
    packet.view=layer_render::ViewState {width_px:128,height_px:128,document_to_surface:[0.01,0.,0.,0.01,0.,0.]};
    submit_ready(r,packet).unwrap();
}
fn receive(r:&mut WgpuRasterizer,document:&Document)->RasterRevision {receive_source(r,RegionSource::Source(target(document)),false)}
fn receive_source(r:&mut WgpuRasterizer,source:RegionSource,close_gaps:bool)->RasterRevision {
    let request=RegionRequest {request_id:83,source,position:[0,0],tolerance:0.1,
        contiguous:false,selection:None,limit:None,enclosure:None,reference_count:None,paper:None,fill:None,fill_areas:Some(layer_core::FillAreas::TransparentOrWhite),
        colour_mode:Default::default(),refinement:RegionRefinement {close_gaps,edges:RegionEdges::LineMiddle,smooth_edges:false}};
    assert!(r.request_region(RegionTask::FlatAreas(request)).unwrap());
    let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
    loop {
        if let Some(output)=r.take_region() {
            let RegionOutput::FlatAreas {request_id,raster}=output.unwrap() else {panic!("expected flat areas raster")};
            assert_eq!(request_id,83);return raster;
        }
        assert!(std::time::Instant::now()<deadline,"flat areas readback timed out");
        std::thread::yield_now();
    }
}
fn pixels(raster:&RasterRevision,extent:[u32;2])->Vec<[u8;4]> {
    let mut pixels=vec![[0;4];(extent[0]*extent[1])as usize];
    for (key,tile) in &raster.wait_data().unwrap().tiles {
        assert_eq!(key.plane,RasterPlane::Color);
        let blob=tile.wait_backing().unwrap();assert_eq!(blob.descriptor.depth(),SampleDepth::U8);assert_eq!(blob.descriptor.channels,4);
        let bytes=blob.decode_on_worker().unwrap();
        for y in 0..TILE_SIZE {for x in 0..TILE_SIZE {
            let [px,py]=[key.coordinate[0]*TILE_SIZE+x,key.coordinate[1]*TILE_SIZE+y];
            if px<extent[0] && py<extent[1] {pixels[(py*extent[0]+px)as usize]=bytes[((y*TILE_SIZE+x)*4)as usize..((y*TILE_SIZE+x)*4+4)as usize].try_into().unwrap();}
        }}
    }
    pixels
}
fn partition(extent:[u32;2],same:impl Fn(usize,usize)->bool,present:impl Fn(usize)->bool)->Vec<u32> {
    let mut labels=vec![0;(extent[0]*extent[1])as usize];let mut queue=VecDeque::new();
    for start in 0..labels.len() {
        if labels[start]!=0 || !present(start) {continue;}
        let token=start as u32+1;labels[start]=token;queue.push_back(start);
        while let Some(at)=queue.pop_front() {
            let x=at as u32%extent[0];let y=at as u32/extent[0];
            for next in [(x>0).then(||at-1),(x+1<extent[0]).then_some(at+1),(y>0).then(||at-extent[0]as usize),(y+1<extent[1]).then_some(at+extent[0]as usize)].into_iter().flatten() {
                if labels[next]==0 && present(next) && same(at,next) {labels[next]=token;queue.push_back(next);}
            }
        }
    }
    labels
}
#[test]
fn connected_word_runs_match_cpu_flood_across_window_and_canvas_edges() {
    let extent=[1900;2];let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for (plane,seeds) in [
        [[31,32],[1855,1856]],[[1,2],[2,2]],[[32,1854],[1856,1856]],[[32,32],[32,31]],
        [[0,1899],[1899,0]],[[1898,1],[1,1]],
    ].into_iter().enumerate() {
        let mut ink:Vec<_>=(0..1900*1900).map(|at| {
            let (x,y)=(at%1900,at/1900);
            match plane {
                0=>(x/2+y/2)%2==0,
                2=>y==1855 || x%64==31 && y<1880,
                3=>x/(if y%4<2 {32}else{64})%2==y%2,
                5=>x==0 || y==0 || x==1899 || y==1899,
                _=>false,
            }
        }).collect();
        if plane==1 {
            let mut p=[1i32;2];let mut length=1897;
            ink[1901]=true;
            for (turn,[dx,dy]) in [[1,0],[0,1],[-1,0],[0,-1]].into_iter().cycle().enumerate() {
                if length<=0 {break;}
                for _ in 0..length {p[0]+=dx;p[1]+=dy;ink[(p[1]*1900+p[0])as usize]=true;}
                if turn%2==1 {length-=2;}
            }
        }
        assert!(ink.iter().enumerate().all(|(at,&solid)|solid || {
            let (x,y)=(at%1900,at/1900);
            ![(x>0).then(||at-1),(x<1899).then_some(at+1),(y>0).then(||at-1900),(y<1899).then_some(at+1900)]
                .into_iter().all(|next|next.is_none_or(|next|ink[next]))
        }),"plane={plane} has an open pixel surrounded by ink");
        let labels=partition(extent,|_,_|true,|at|!ink[at]);
        let document=source(extent,|x,y|ink[(y*1900+x)as usize]);frame(&mut r,&document,extent);
        for position in seeds {
            assert!(!ink[(position[1]*1900+position[0])as usize],"plane={plane} seed={position:?} is ink");
            let result=crate::test_support::receive_request(&mut r,RegionRequest {request_id:plane as u64,source:RegionSource::Source(target(&document)),position,
                tolerance:0.,contiguous:true,selection:None,limit:None,enclosure:None,reference_count:None,paper:None,fill:None,fill_areas:None,colour_mode:Default::default(),
                refinement:RegionRefinement {close_gaps:false,edges:RegionEdges::Exact,smooth_edges:false}});
            let selected=labels[(position[1]*1900+position[0])as usize];
            let count=result.pixels.pixels_per_word();let bits=32/count;let stride=1900u32.div_ceil(count);
            let mut bounds=[1900,1900,0,0];
            for (index,&actual) in result.pixels.words().iter().enumerate() {
                let [x,y]=[(index as u32%stride)*count,index as u32/stride];let mut expected=0;
                for i in 0..count.min(1900-x) {if labels[(y*1900+x+i)as usize]==selected {
                    expected|=(if count==4 {255}else{4})<<(i*bits);
                    bounds[0]=bounds[0].min(x+i);bounds[1]=bounds[1].min(y);bounds[2]=bounds[2].max(x+i+1);bounds[3]=bounds[3].max(y+1);
                }}
                assert_eq!(actual,expected,"plane={plane} seed={position:?} word={index}");
            }
            assert_eq!(result.pixels.bounds(),bounds,"plane={plane} seed={position:?}");
        }
    }
}
fn border([x0,y0,x1,y1]:[u32;4],x:u32,y:u32)->bool {
    x>=x0 && x<x1 && y>=y0 && y<y1 && (x==x0 || x+1==x1 || y==y0 || y+1==y1)
}
#[test]
fn flat_areas_match_closed_components_exclude_exterior_and_thin_pockets() {
    for width in [1,10] {
        let extent=[129,97];let left=64-width/2;let ink=|x,y|border([12,12,117,85],x,y)||((left..left+width).contains(&x)&&(12..85).contains(&y))||border([20,20,23,23],x,y)||border([38,49,50,61],x,y);
        let document=source(extent,ink);let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
        let idle=r.raster_tiers.paint.used();
        let first=pixels(&receive(&mut r,&document),extent);
        assert_eq!(r.raster_tiers.paint.used()-idle,r.regions.as_ref().unwrap().storage_bytes(),"region buffers are charged to the paint tier");assert_eq!(first,pixels(&receive(&mut r,&document),extent),"same input has identical stored colours");
        let oracle=partition(extent,|_,_|true,|at|!ink(at as u32%extent[0],at as u32/extent[0]));
        let mut colours=std::collections::BTreeMap::new();
        for (at,&token) in oracle.iter().enumerate() {
            let x=at as u32%extent[0];let y=at as u32/extent[0];
            if token==0 || [x,y]==[21,21] {continue;}
            if token==1 {assert_eq!(first[at][3],0,"open background {x},{y}");}
            else {assert_eq!(first[at][3],255,"closed area {x},{y}");assert_eq!(*colours.entry(token).or_insert(first[at]),first[at],"one colour per CPU component {x},{y}");}
        }
        assert_eq!(colours.len(),3);
        assert_eq!(first[(21*extent[0]+21)as usize],first[(24*extent[0]+24)as usize],"thin pocket joins its surrounding line-middle area");
        for y in 30..75 {for x in left..left+width {let p=first[(y*extent[0]+x)as usize];assert_eq!(p[3],255,"no middle gap {x},{y}");assert!(colours.values().any(|c|*c==p),"hard line-middle partition {x},{y}");}}
    }
}
fn lower_limits(r:&WgpuRasterizer)->WgpuRasterizer {
    let adapter=r.renderer_factory().adapter().clone();let mut limits=rendering_limits(adapter.limits());limits.max_storage_buffer_binding_size=32*1024*1024;
    let (device,queue)=pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {label:Some("flat areas smaller window"),required_limits:limits,required_features:rendering_features(&adapter),memory_hints:memory_hints(),..Default::default()})).unwrap();
    let mut renderer=*WgpuRasterizer::from_wgpu_inner(adapter,PipelineDevice::from(device),queue,Initialization::Headless).unwrap();renderer.initialize_native(Default::default()).unwrap();renderer
}
#[test]
fn flat_areas_partitions_agree_across_device_window_limits() {
    let extent=[2304,257];let document=source(extent,|x,y|border([10,10,2294,247],x,y)||(x==831||x==1855)&&(10..247).contains(&y));
    let mut normal=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let mut small=lower_limits(&normal);
    assert_ne!(normal.device.limits().max_storage_buffer_binding_size,small.device.limits().max_storage_buffer_binding_size);
    frame(&mut normal,&document,extent);frame(&mut small,&document,extent);
    let a=pixels(&receive(&mut normal,&document),extent);let b=pixels(&receive(&mut small,&document),extent);
    for output in [&a,&b] {for point in [[40,40],[1000,40],[2200,40]] {assert_eq!(output[(point[1]*extent[0]+point[0])as usize][3],255);}assert_eq!(output[0][3],0);}
    let canonical=|p:&Vec<[u8;4]>|partition(extent,|i,j|p[i]==p[j],|i|p[i][3]!=0);
    let labels=canonical(&a);assert_eq!(labels,canonical(&b),"window layout may change colours but not partitions");
    let mut crc=crc32fast::Hasher::new();for label in labels {crc.update(&label.to_le_bytes());}
    eprintln!("FLAT_AREAS_PARTITION extent={extent:?} crc={:08x}",crc.finalize());
}
fn large(extent:[u32;2]) {
    let document=source(extent,|x,y|border([16,16,extent[0]-16,extent[1]-16],x,y)||x==extent[0]/2&&(16..extent[1]-16).contains(&y));
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    let start=std::time::Instant::now();let raster=receive(&mut r,&document);let tiles=raster.wait_data().unwrap().tiles.len();let actual=pixels(&raster,extent);let mut colours=[None;2];
    for y in 0..extent[1] {for x in 0..extent[0] {
        let p=actual[(y*extent[0]+x)as usize];
        if x<16||y<16||x>=extent[0]-16||y>=extent[1]-16 {assert_eq!(p[3],0,"exterior {x},{y}");}
        else if x>16&&y>16&&x+17<extent[0]&&y+17<extent[1]&&x!=extent[0]/2 {
            assert_eq!(p[3],255,"interior {x},{y}");let side=usize::from(x>extent[0]/2);assert_eq!(*colours[side].get_or_insert(p),p,"one colour per area {x},{y}");
        }
    }}
    eprintln!("flat areas {extent:?}: {:.3}s, {} native tiles",start.elapsed().as_secs_f64(),tiles);
}
#[test]
#[ignore="24 MP hardware qualification"]
fn flat_areas_24mp(){large([6000,4000]);}
#[test]
#[ignore="61 MP hardware qualification"]
fn flat_areas_61mp(){large([9504,6336]);}

#[test]
fn flat_areas_auto_closes_gaps_in_both_coloured_boundaries() {
    let extent=[160,96];let mut document=paint_document(extent,"Coloured boundaries");
    placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|x,y| {
        if border([12,12,70,84],x,y)&&!(y==12&&(35..43).contains(&x)) {[255,0,0,255]}
        else if border([90,12,148,84],x,y)&&!(y==12&&(113..121).contains(&x)) {[0,0,255,255]}
        else {[255;4]}
    }));
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,&document,extent);
    let line=RegionSource::Source(target(&document));let off=pixels(&receive_source(&mut r,line.clone(),false),extent);
    assert!(off.iter().all(|p|p[3]==0),"Off leaves both interrupted areas open");
    let auto=pixels(&receive_source(&mut r,line,true),extent);
    for [left,right] in [[13,69],[91,147]] {
        let colour=auto[(48*extent[0]+left+2)as usize];assert_eq!(colour[3],255);
        for y in 16..80 {for x in left..right {assert_eq!(auto[(y*extent[0]+x)as usize],colour,"closed coloured area {x},{y}");}}
    }
    assert_eq!(auto[0][3],0);
}
#[test]
fn flat_areas_rasterizes_vector_line_sources() {
    use layer_core::authored::vector::*;
    let extent=[128;2];let anchors=[[24.,24.],[104.,24.],[104.,104.],[24.,104.]].map(Anchor::corner);
    let paint=Paint::color(layer_core::color::RgbColor::new(Default::default(),[0.,0.,0.,1.]).unwrap());
    let path=Record::Path(PathItem::new(ItemId(1),Arc::new(PathGeometry::new(vec![Subpath::new(&anchors,true)]).unwrap()),Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(paint,4.))]))));
    let artwork=layer_core::authored::Artwork::new(extent).unwrap();let mut editor=layer_core::Editor::new(Document::from_artwork(artwork).unwrap());
    let (layer,edit)=editor.document().create_vector_layer_edit("Vector lines",VectorObject::from_records(vec![path]),None,0).unwrap();editor.perform(edit).unwrap();
    let document=editor.document();let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();frame(&mut r,document,extent);
    let output=pixels(&receive_source(&mut r,RegionSource::Scene {snapshot:document.snapshot(),scope:layer_core::SceneScope::RawObjects(layer)},false),extent);
    let colour=output[(64*extent[0]+64)as usize];assert_eq!(colour[3],255);assert_eq!(output[0][3],0);
    for y in 30..98 {for x in 30..98 {assert_eq!(output[(y*extent[0]+x)as usize],colour,"vector interior {x},{y}");}}
}

#[test]
fn flat_areas_thick_ink_core_and_seedless_sources_stay_transparent() {
    let extent=[192,128];let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document=source(extent,|x,y|border([8,8,184,120],x,y)||((64..128).contains(&x)&&(8..120).contains(&y)));
    frame(&mut r,&document,extent);let output=pixels(&receive(&mut r,&document),extent);
    for x in 89..103 {assert_eq!(output[(64*extent[0]+x)as usize][3],0,"ink farther than 24px from an area {x}");}
    for x in [66,126] {assert_eq!(output[(64*extent[0]+x)as usize][3],255,"nearby ink gets a flat {x}");}
    for ink in [false,true] {
        let document=source(extent,|_,_|ink);frame(&mut r,&document,extent);
        assert!(pixels(&receive(&mut r,&document),extent).iter().all(|p|*p==[0;4]),"uniform ink={ink} has no closed area");
    }
}
