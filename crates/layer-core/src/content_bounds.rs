use crate::{Document,Rect,Selection,authored::*};
use std::sync::Arc;

#[derive(Clone,Copy,Debug,PartialEq,Eq,serde::Serialize,serde::Deserialize)]
pub enum ContentScope {Canvas,Visible,All,Target(SourceTarget),PlacedTarget(OccurrenceHandle)}
#[derive(Clone,Debug,PartialEq)]
pub struct ContentBoundsRequest {pub snapshot:Arc<SceneSnapshot>,pub scope:ContentScope,pub selection:Option<Selection>}
impl ContentBoundsRequest {
    pub fn new(document:&Document,scope:ContentScope)->Self{Self {snapshot:document.snapshot(),scope,selection:document.working.selection.clone()}}
    pub fn known_bounds(&self)->Option<Rect>{
        let ContentScope::Target(target)=self.scope else{return None;};if self.selection.is_some(){return None;}
        let scene=self.snapshot.view();let base=scene.paint_base(target)?;
        if base.image.interpretation.channels.has_alpha() || !scene.raster(target)?.is_empty() || !scene.operations(target)?.is_empty() {return None;}
        let end=[base.offset[0].checked_add(base.image.extent[0])?,base.offset[1].checked_add(base.image.extent[1])?];
        Some(Rect {min:crate::Point {x:base.offset[0] as f32,y:base.offset[1] as f32},max:crate::Point {x:end[0] as f32,y:end[1] as f32}})
    }
}
#[derive(Default)]
pub struct ContentBoundsCache {entries:Vec<(ContentBoundsRequest,Rect)>}
impl ContentBoundsCache {
    pub fn get(&self,request:&ContentBoundsRequest)->Option<Rect>{self.entries.iter().find(|(key,_)|key==request).map(|(_,bounds)|*bounds)}
    pub fn current(&self,document:&Document,scope:ContentScope)->Option<Rect>{self.entries.iter().find(|(key,_)|key.snapshot.owner==document.owner&&key.snapshot.revision==document.revision&&key.scope==scope).map(|(_,bounds)|*bounds)}
    pub fn insert(&mut self,request:ContentBoundsRequest,bounds:Rect){self.entries.retain(|(key,_)|key.snapshot.owner==request.snapshot.owner&&key.snapshot.revision==request.snapshot.revision&&key.scope!=request.scope);self.entries.push((request,bounds));}
    pub fn discard_changed(&mut self,document:&Document){self.entries.retain(|(key,_)|key.snapshot.revision==document.revision&&key.snapshot.owner==document.owner);}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point,Affine,RasterOperation,RasterOperationKind,CoverageSnapshot,Edit,color::{ColorProfile,RgbSpace,SampleDepth,source::{SourceBuilder,SourceChannels,SourceInterpretation}}};
    use crate::operation_test_support as fixture;
    fn document(channels:SourceChannels)->(Document,SourceTarget) {
        let mut document=fixture::document([1024;2],&["Ink"]);
        let mut builder=SourceBuilder::new([257,259],SourceInterpretation {channels,depth:SampleDepth::U8,profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false},1024*1024).unwrap();
        let row=vec![0;257*channels.count()];for _ in 0..259 {builder.push_row(&row).unwrap();}
        fixture::paint_mut(&mut document,"Ink").base=Some(PaintBase {image:Arc::new(builder.finish().unwrap()).into(),offset:[13,29],policy:PaintBasePolicy::SourceProfile});
        let target=fixture::target(&document,"Ink");(document,target)
    }
    #[test]
    fn opaque_gray_and_rgb_bounds_include_non_aligned_base_offset() {
        for channels in [SourceChannels::Gray,SourceChannels::Rgb] {
            let (document,target)=document(channels);
            assert_eq!(ContentBoundsRequest::new(&document,ContentScope::Target(target)).known_bounds(),Some(Rect {min:Point {x:13.,y:29.},max:Point {x:270.,y:288.}}));
            assert_eq!(ContentBoundsRequest::new(&document,ContentScope::PlacedTarget(fixture::id(&document,"Ink"))).known_bounds(),None);
        }
    }
    #[test]
    fn pending_operations_require_measured_bounds_even_with_empty_raster() {
        let (mut document,target)=document(SourceChannels::Rgb);
        let SourceTarget::Paint(handle)=target else {unreachable!()};
        let mut paint=document.scene().paint(handle).unwrap().clone();
        paint.operations=vec![RasterOperation {placement:Affine::IDENTITY,coverage:CoverageSnapshot::reveal_all(document.allocate_coverage_handle(),[1024;2],[0;2]),kind:RasterOperationKind::Erase {alpha_locked:false}}].into();
        document.apply(Edit::Paint(RecordChange::replace(&document.artwork.paint,handle,Some(paint)).unwrap())).unwrap();
        assert!(document.scene().raster(target).unwrap().is_empty());
        assert_eq!(ContentBoundsRequest::new(&document,ContentScope::Target(target)).known_bounds(),None);
    }
}
