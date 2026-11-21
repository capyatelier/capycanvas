use super::*;
use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};

const TILES:u32=2;

fn present(layer:u32,tile:u32)->bool {(layer+tile)%4!=3}

fn samples(layer:u32,tile:u32)->Vec<u8> {
    (0..PAGE_SIZE*PAGE_SIZE).flat_map(|i| {
        let [x,y]=[i%PAGE_SIZE,i/PAGE_SIZE];
        let alpha=match (x/16+y/16+layer+tile)%5 {0=>0,1=>255,2=>1,n=>(x*3+y*5+layer*29+n*17)%256};
        [(x+layer*37) as u8,(y*3+layer*11+tile*50) as u8,(x^y^layer) as u8,alpha as u8]
    }).collect()
}

struct Folded {doc:layer_core::Document,layers:Vec<SourceTarget>,mask:Option<(usize,SourceTarget)>}

fn folded_document(count:u32)->Folded {
    let extent=[PAGE_SIZE*TILES,PAGE_SIZE];
    let mut doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    for handle in doc.scene().order().to_vec() {doc.artwork.occurrences.get_mut(handle).unwrap().visible=false;}
    let color=doc.composition().color;
    let mut layers=Vec::new();
    let mut mask=None;
    for layer in 0..count {
        let (owner,target)=crate::test_support::add_paint(&mut doc.artwork,format!("Layer {layer}"),extent);
        let SourceTarget::Paint(paint)=target else {unreachable!()};
        let tiles=(0..TILES).filter(|tile|present(layer,*tile)).map(|tile|(TileKey {plane:RasterPlane::Color,coordinate:[tile,0]},
            RasterTile::backed(TileBlob::encode(color.paint_descriptor(),&samples(layer,tile)).unwrap()))).collect();
        doc.artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(RasterData {tiles,watercolor:None});
        doc.artwork.occurrences.get_mut(owner).unwrap().opacity=[1.,0.75,0.3,0.5][layer as usize%4];
        if layer==2 {
            let mut coverage=layer_core::CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(),extent,[0;2]);
            let bytes=(0..PAGE_SIZE*PAGE_SIZE).map(|i|(i%PAGE_SIZE+i/PAGE_SIZE*3) as u8).collect::<Vec<_>>();
            coverage.source.raster=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Mask,coordinate:[0,0]},
                RasterTile::backed(TileBlob::encode(color.coverage_descriptor(),&bytes).unwrap()))].into(),..Default::default()});
            doc.artwork.coverage.insert(layer_core::PortableId::random(),coverage.source).unwrap();
            mask=Some((layers.len(),SourceTarget::Coverage(coverage.use_.source)));
            doc.artwork.occurrences.get_mut(owner).unwrap().mask=Some(coverage.use_);
        }
        layers.push(target);
    }
    Folded {doc:layer_core::Document::from_artwork(doc.artwork).unwrap(),layers,mask}
}

fn tile_pixels(r:&mut WgpuRasterizer,capture:&mut crate::artwork::Capture,target:SourceTarget,coordinate:[u32;2])->Vec<[f32;4]> {
    let tile=crate::test_support::submit_encoded(r,|r,encoder|crate::test_support::query_ready(r,encoder,|r,encoder|capture.layer_tile(r,target,coordinate,encoder)));
    crate::test_support::float_pixels(r,&tile.texture)
}

#[test]
fn normal_runs_fold_each_cell_in_a_pass_per_sixteen_inputs_to_the_per_layer_arithmetic() {
    for count in [17,1,15,16,33] {
        let folded=folded_document(count);
        let doc=&folded.doc;
        let extent=doc.composition().size;
        let mut r=crate::memory::memory_tests::renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
        crate::test_support::submit_settled(&mut r,crate::test_support::packet(doc.scene(),extent));
        crate::test_support::evict_sources(&mut r);
        let before=crate::test_support::PassCounts::snapshot();
        let composite=crate::test_support::canonical_pixels(&mut r);
        let work=crate::test_support::PassCounts::snapshot().delta(before);
        let mut capture=crate::artwork::Capture::default();
        let scene=doc.scene();
        let order:Vec<_>=scene.members(None).rev().filter_map(|handle|folded.layers.iter().position(|target|scene.source_target(handle)==Some(*target))).collect();
        assert_eq!(order.len(),count as usize);
        let mut inputs=0;
        let mut passes=0;
        for tile in 0..TILES {
            let layers:Vec<_>=order.iter().map(|&index| {
                let raw=tile_pixels(&mut r,&mut capture,folded.layers[index],[tile,0]);
                let mask=folded.mask.filter(|(masked,_)|*masked==index).map(|(_,target)|tile_pixels(&mut r,&mut capture,target,[tile,0]));
                (raw,mask,[1.,0.75,0.3,0.5][index%4])
            }).collect();
            let present=(0..count).filter(|layer|present(*layer,tile)).count() as u64;
            inputs+=present;passes+=present.div_ceil(16)+1;
            let reference=|fused:bool|(0..(PAGE_SIZE*PAGE_SIZE) as usize).map(|p|layers.iter().fold([0f32;4],|acc,(raw,mask,opacity)| {
                let m=mask.as_ref().map_or(1.,|mask|mask[p][0]);
                let src=raw[p].map(|c|c*m*opacity);
                std::array::from_fn(|c|if fused {acc[c].mul_add(1.-src[3],src[c])}else {src[c]+acc[c]*(1.-src[3])})
            })).collect::<Vec<[f32;4]>>();
            let references=[reference(false),reference(true)];
            let folded=|p:usize|composite[(p/PAGE_SIZE as usize)*extent[0] as usize+(tile*PAGE_SIZE) as usize+p%PAGE_SIZE as usize];
            let error=|p:usize|references.iter().map(|reference|(0..4).map(|c|(folded(p)[c]-reference[p][c]).abs()).fold(0f32,f32::max)).fold(f32::MAX,f32::min);
            let (worst,at)=(0..references[0].len()).map(|p|(error(p),p)).fold((0.,0),|worst,next|if next.0>worst.0 {next}else {worst});
            assert!(worst<=f32::EPSILON,"{count} layers, tile {tile}: the fold differs from the per-layer arithmetic by {worst:e} at pixel {at}: {:?} against {:?}",
                folded(at),references.each_ref().map(|reference|reference[at]));
            assert!(references[0].iter().any(|pixel|pixel[3]>0.),"{count} layers, tile {tile}: the fold covers visible pixels");
        }
        assert!(work.scene_dispatches+work.normalizations<=passes,"{count} layers: {} draws or dispatches and {} normalizations for cells allowing {passes}",
            work.scene_dispatches,work.normalizations);
        assert_eq!(work.fold_inputs,inputs,"{count} layers: every present layer tile is folded");
    }
}
