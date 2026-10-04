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
        let scene=self.snapshot.view();let source=scene.original(target)?;
        (!source.interpretation.channels.has_alpha()&&scene.raster(target)?.is_empty()).then(||Rect::from_extent(source.extent))
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
