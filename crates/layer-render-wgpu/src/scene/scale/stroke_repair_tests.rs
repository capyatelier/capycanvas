use super::*;
use layer_core::DefaultBrushPreset;

const EXTENT:[u32;2]=[4096,2048];

fn sized(preset:DefaultBrushPreset,diameter:f32)->layer_core::BrushSnapshot {
    let mut brush=layer_core::default_brush(preset);brush.diameter=diameter;brush
}
fn hidden_paper(mut document:layer_core::Document)->layer_core::Document {
    let paper=*document.scene().order().last().unwrap();
    let mut occurrence=document.scene().occurrence(paper).unwrap().clone();occurrence.visible=false;
    document.apply(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&document.artwork.occurrences,paper,Some(occurrence)).unwrap())).unwrap();document
}
fn over_a_photo(mut document:layer_core::Document)->layer_core::Document {
    let photo=paint_occurrence(&mut document,"Photo",Some(rgba8_source(EXTENT,|x,y|[(x/17) as u8,(y/9) as u8,120,255])));
    insert_occurrence(&mut document,photo,1);document
}
fn inked_over_a_photo(mut document:layer_core::Document)->layer_core::Document {
    use layer_core::raster::{RasterData,RasterRevision,RasterTile,TileBlob};
    let descriptor=document.composition().color.paint_descriptor();
    let tiles=[(TileKey {plane:RasterPlane::Color,coordinate:[15,7]},RasterTile::backed(TileBlob::encode(descriptor,&[200,40,90,255].repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap()))].into();
    paint_mut(&mut document,0).raster=RasterRevision::backed(RasterData {tiles,..Default::default()});
    over_a_photo(document)
}
fn opened(document:layer_core::Document,scale:f32)->Harness {
    let mut harness=Harness::new(document);harness.zoom(scale);harness.settle();
    let cache=harness.engine.backend().scale_display.as_ref().unwrap();
    assert!(cache.exact_cells().iter().all(|(_,exact)|*exact),"the opened view is exact in every cell");
    harness
}
fn draw(harness:&mut Harness,first:u64,points:&[[f32;2]],brush:layer_core::BrushSnapshot) {
    harness.engine.set_brush(brush).unwrap();
    let [a,b,c,d,x,y]=harness.engine.view().document_to_surface;
    let lift=points.last().copied().into_iter();
    for (offset,point) in points.iter().copied().chain(lift).enumerate() {
        let sequence=first+offset as u64;
        let phase=if offset==0 {PenPhase::Down}else if offset==points.len() {PenPhase::Up}else {PenPhase::Move};
        harness.input.push(PenEvent {timestamp_ns:sequence*100_000_000,pressure:if phase==PenPhase::Up {0.}else {1.},view_revision:harness.view_revision,
            ..crate::test_support::pen(sequence,phase,[a*point[0]+c*point[1]+x,b*point[0]+d*point[1]+y],SampleFlags::PRIMARY)}).unwrap();
        harness.clock=harness.clock.max(sequence*100_000_000);
        harness.step();
    }
    let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
    while harness.engine.has_pending_input() || harness.engine.has_active_stroke() {
        harness.step();assert!(std::time::Instant::now()<deadline,"the lifted stroke ends");
    }
}

struct Repair {counts:crate::test_support::PassCounts,roots:BTreeSet<[u32;2]>,halves:BTreeSet<[u32;2]>,level:u32}
impl Repair {
    fn pages(&self)->u64 {1<<(2*self.level)}
    fn assert_within(&self,label:&str,cells:&[[u32;2]]) {
        let bound=cells.len() as u64*self.pages();
        assert!(self.counts.composed_texels<=bound*2*u64::from(PAGE_SIZE).pow(2),"{label}: {} texels were composed after the lift; {} cells of {} pages were reached: {:?}",
            self.counts.composed_texels,cells.len(),self.pages(),self.counts);
        let touched=cells.iter().copied().collect::<BTreeSet<_>>();
        let outside=self.roots.iter().filter(|cell|!touched.contains(*cell)).collect::<BTreeSet<_>>();
        assert!(outside.is_empty(),"{label}: cells {outside:?} that the edit left exact were captured again; it reached {touched:?}: {:?}",self.counts);
    }
    fn assert_no_halves_outside(&self,label:&str,cells:&[[u32;2]]) {
        let outside=self.halves.iter().filter(|cell|!cells.contains(*cell)).collect::<BTreeSet<_>>();
        assert!(outside.is_empty(),"{label}: halves were built in cells {outside:?} that the edit did not reach");
    }
}
fn written(harness:&Harness)->Vec<(u64,bool,[u32;2])> {
    let cache=harness.engine.backend().scale_display.as_ref().unwrap();let level=cache.plan.level;
    cache.pages.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.source.is_none() && page.key.level<=level && !page.written.is_empty())
        .map(|page|(page.written_at,cache.graph.content.as_ref()==Some(&page.key.expression),page.key.coordinate.map(|value|value>>(level-page.key.level)))).collect()
}
fn repaired(harness:&mut Harness,edit:impl FnOnce(&mut Harness))->Repair {
    edit(harness);
    let before=written(harness).into_iter().map(|(stamp,..)|stamp).collect::<BTreeSet<_>>();
    let counts=crate::test_support::PassCounts::snapshot();
    harness.settle();
    let counts=crate::test_support::PassCounts::snapshot().delta(counts);
    let fresh=written(harness).into_iter().filter(|(stamp,..)|!before.contains(stamp)).collect::<Vec<_>>();
    let cells=|root:bool|fresh.iter().filter(|(_,content,_)|*content==root).map(|(_,_,cell)|*cell).collect();
    let cache=harness.engine.backend().scale_display.as_ref().unwrap();
    assert!(cache.exact_cells().iter().all(|(_,exact)|*exact),"the repaired view is exact in every cell");
    Repair {counts,roots:cells(true),halves:cells(false),level:cache.plan.level}
}
fn exported(harness:&Harness)->Vec<[f32;4]> {
    let document=harness.engine.document();
    let mut capture=harness.engine.backend().snapshot_gpu()
        .capture(layer_core::Editor::new(document.clone()).capture(0,document.output().context.clone()).unwrap(),Default::default()).unwrap();
    capture.read_region([0,0,EXTENT[0],EXTENT[1]]).unwrap()
}
fn assert_fresh(harness:&mut Harness,scale:f32) {
    let mut reference=Harness::new(harness.engine.document().clone());reference.zoom(scale);reference.settle();
    close(&harness.present(),&reference.present());
    let [a,b]=[&*harness,&reference].map(|harness|harness.engine.backend());
    close(&pixels(a,a.scale_display.as_ref().unwrap().texture()),&pixels(b,b.scale_display.as_ref().unwrap().texture()));
    let (actual,expected)=(exported(harness),exported(&reference));
    assert!(actual.iter().zip(&expected).all(|(a,b)|a.map(f32::to_bits)==b.map(f32::to_bits)),"the export differs from a fresh renderer's");
}

fn one_cell_stroke(document:layer_core::Document,scale:f32,cells:&[[u32;2]],label:&str) {
    let mut harness=opened(document,scale);
    let repair=repaired(&mut harness,|harness|draw(harness,1,&[[1500.,600.],[1516.,604.],[1532.,600.]],sized(DefaultBrushPreset::Pencil,9.)));
    assert!(!repair.roots.is_empty(),"{label}: the stroke's cell is repaired");
    repair.assert_within(label,cells);repair.assert_no_halves_outside(label,cells);
    assert_fresh(&mut harness,scale);
}

#[test]
fn the_first_stroke_on_an_empty_layer_over_paper_repairs_only_the_cell_it_reaches() {
    one_cell_stroke(blank_document(EXTENT),0.25,&[[1,0]],"over Paper");
}

#[test]
fn the_first_stroke_on_an_empty_layer_without_paper_repairs_only_the_cell_it_reaches() {
    one_cell_stroke(hidden_paper(blank_document(EXTENT)),0.25,&[[1,0]],"Paper hidden");
}

#[test]
fn the_first_stroke_at_full_size_repairs_only_the_pages_it_reaches() {
    one_cell_stroke(blank_document(EXTENT),1.,&[[5,2],[6,2]],"at full size");
}

#[test]
fn a_stroke_across_a_cell_edge_repairs_the_cells_on_both_sides() {
    let mut harness=opened(blank_document(EXTENT),0.25);
    let repair=repaired(&mut harness,|harness|draw(harness,1,&[[1000.,600.],[1024.,604.],[1048.,600.]],sized(DefaultBrushPreset::Pencil,9.)));
    assert_eq!(repair.roots,BTreeSet::from([[0,0],[1,0]]),"both cells the stroke crossed are repaired");
    repair.assert_within("across an edge",&[[0,0],[1,0]]);repair.assert_no_halves_outside("across an edge",&[[0,0],[1,0]]);
    assert_fresh(&mut harness,0.25);
}

#[test]
fn undoing_the_first_stroke_restores_its_cell_without_recapturing_the_others() {
    let mut harness=opened(blank_document(EXTENT),0.25);
    draw(&mut harness,1,&[[1500.,600.],[1516.,604.],[1532.,600.]],sized(DefaultBrushPreset::Pencil,9.));harness.settle();
    let repair=repaired(&mut harness,|harness|assert!(harness.engine.undo().unwrap()));
    repair.assert_within("Undo",&[[1,0]]);repair.assert_no_halves_outside("Undo",&[[1,0]]);
    assert_fresh(&mut harness,0.25);
}

#[test]
fn later_strokes_with_an_eraser_and_a_blender_repair_only_their_cells() {
    let mut harness=opened(blank_document(EXTENT),0.25);
    draw(&mut harness,1,&[[1400.,600.],[1500.,604.],[1600.,600.]],sized(DefaultBrushPreset::GPen,40.));harness.settle();
    for (first,preset,label) in [(10,DefaultBrushPreset::Eraser,"an eraser"),(20,DefaultBrushPreset::Smudge,"a blender")] {
        let repair=repaired(&mut harness,|harness|draw(harness,first,&[[1480.,560.],[1500.,600.],[1520.,640.]],sized(preset,24.)));
        assert!(!repair.roots.is_empty(),"{label} changes the ink it crosses");
        repair.assert_within(label,&[[1,0]]);repair.assert_no_halves_outside(label,&[[1,0]]);
    }
    assert_fresh(&mut harness,0.25);
}

fn first_stroke_over_a_photo(document:layer_core::Document,label:&str) {
    let mut harness=opened(document,0.25);
    assert!(harness.engine.backend().scale_display.as_ref().unwrap().half_pages().is_empty(),"{label}: the open builds no halves");
    let first=repaired(&mut harness,|harness|draw(harness,1,&[[1500.,600.],[1516.,604.],[1532.,600.]],sized(DefaultBrushPreset::Pencil,9.)));
    assert!(!first.roots.is_empty(),"{label}: the stroke's cell is repaired");
    first.assert_within(label,&[[1,0]]);
    let cache=harness.engine.backend().scale_display.as_ref().unwrap();
    let reached=pages::bounds([1,0],cache.plan.level,EXTENT);
    assert_eq!(cache.half_cells(reached),[reached],"{label}: the cell the stroke reached holds its halves");
    assert_fresh(&mut harness,0.25);
}

#[test]
fn a_stroke_into_a_cell_without_its_halves_builds_them_and_recaptures_no_other_cell() {
    first_stroke_over_a_photo(inked_over_a_photo(blank_document(EXTENT)),"an inked layer over a photo");
}

#[test]
fn the_first_stroke_on_an_empty_layer_over_a_photo_repairs_only_the_cell_it_reaches() {
    first_stroke_over_a_photo(over_a_photo(blank_document(EXTENT)),"an empty layer over a photo");
}
