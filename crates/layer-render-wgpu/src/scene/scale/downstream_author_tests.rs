use super::*;
use layer_core::{DefaultBrushPreset,Selection,RasterOperation,RasterOperationKind,CoverageSnapshot,Point};
use layer_render::{RegionRequest,RegionSource,RegionRefinement,RegionEdges,RegionTask};

#[derive(Clone,Copy,Debug)]
enum Consumer {Smudge,LoadedOil,FloodFill}

fn consume(fixture:&mut Harness,consumer:Consumer) {
    match consumer {
        Consumer::Smudge|Consumer::LoadedOil=> {
            let preset=match consumer {Consumer::Smudge=>DefaultBrushPreset::Smudge,_=>DefaultBrushPreset::LoadedOil};
            let mut brush=layer_core::default_brush(preset);
            brush.diameter=40.;brush.color_rgba_linear=[0.,0.,1.,1.];
            fixture.queue(4,&[[112.,120.],[256.,120.],[400.,120.]],brush);
            fixture.finish();
        },
        Consumer::FloodFill=> {
            fixture.finish();
            let target=fixture.engine.document().working.target.unwrap();
            let extent=fixture.engine.document().composition().size;
            let point=[256,120];
            let result=crate::test_support::receive_request(fixture.engine.backend_mut(),RegionTask::Fill {
                request:RegionRequest {fill_areas:None,colour_mode:Default::default(),fill:None,reference_count:None,paper:None,
                    request_id:41,source:RegionSource::Source(target),contiguous:true,position:point,tolerance:0.,
                    refinement:RegionRefinement {close_gaps:false,edges:RegionEdges::Exact,smooth_edges:false},
                    limit:None,enclosure:None,selection:None},target,position:Point{x:point[0] as f32,y:point[1] as f32}});
            let mut coverage=CoverageSnapshot::reveal_all(fixture.engine.allocate_coverage_handle(),extent,[0;2]);
            coverage.selection=Some(Selection::pixels(result.pixels));
            fixture.engine.append_raster_operation(target,RasterOperation {placement:result.placement,coverage,
                kind:RasterOperationKind::Fill {edges:None,underfill:result.underfill.map(Selection::pixels),
                    texture:Default::default(),color:[0.,0.,1.,1.],alpha_locked:false}}).unwrap();
            fixture.finish();
        },
    }
}

fn author_after_undo(consumer:Consumer) {
    for scale in [1.,0.25] {
        let mut actual=Harness::new(blank_document([512,256]));
        actual.zoom(scale);actual.finish();
        actual.gpen(1,96.,[1.,0.,0.,1.]);actual.finish();
        actual.assert_resident(&[[112,96],[400,96]]);
        let old=actual.committed_bytes();
        assert!(pixel(&old,256,96)[3]>0,"A must have an exclusive nontransparent core");
        assert!(actual.engine.undo().unwrap());
        let selected=actual.engine.document().clone();
        let mut reference=Harness::new(selected);
        reference.zoom(scale);reference.finish();
        assert!(reference.committed_bytes().values().all(|bytes|bytes.iter().all(|byte|*byte==0)),"fresh selected history contains no A");
        assert_eq!(actual.engine.document().next_stroke_id(),reference.engine.document().next_stroke_id(),"fresh selected history must preserve the brush seed identity");
        consume(&mut actual,consumer);consume(&mut reference,consumer);
        actual.settle();reference.settle();
        let expected=reference.committed_bytes();let result=actual.committed_bytes();
        assert_contents(&result,&expected);
        assert_eq!(actual.present(),reference.present(),"{consumer:?} at {scale}: presentation must match fresh selected history");
        match consumer {
            Consumer::Smudge=>assert!(result.values().all(|bytes|bytes.iter().all(|byte|*byte==0)),"Smudge must not pick up undone A"),
            Consumer::LoadedOil=>assert!(result.values().any(|bytes|bytes.chunks_exact(4).any(|rgba|rgba[3]>0)),"Loaded Oil must author real B"),
            Consumer::FloodFill=> {
                assert_eq!(pixel(&expected,256,96),[0,0,255,255],"the fresh fill covers the former A core");
                assert_eq!(pixel(&result,256,96),pixel(&expected,256,96),"undone A must not bound or survive the fill");
            },
        }
    }
}

#[test]
fn undo_then_smudge_reads_only_selected_history() {author_after_undo(Consumer::Smudge);}
#[test]
fn undo_then_loaded_oil_mixes_only_selected_history() {author_after_undo(Consumer::LoadedOil);}
#[test]
fn undo_then_flood_fill_authors_only_selected_history() {author_after_undo(Consumer::FloodFill);}
