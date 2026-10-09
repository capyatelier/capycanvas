use super::*;
use layer_core::authored::{ImageObject, Occurrence, Store};
use std::collections::HashMap;

struct Entry { owner:OccurrenceHandle, bounds:DocRect, keys:Arc<[objects::ObjectKey]> }
#[derive(Clone)]
pub(super) struct Content(Arc<Entry>);
impl PartialEq for Content {fn eq(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}}
impl Eq for Content {}
impl PartialOrd for Content {fn partial_cmp(&self,other:&Self)->Option<std::cmp::Ordering> {Some(self.cmp(other))}}
impl Ord for Content {fn cmp(&self,other:&Self)->std::cmp::Ordering {Arc::as_ptr(&self.0).cmp(&Arc::as_ptr(&other.0))}}
impl std::hash::Hash for Content {fn hash<H:std::hash::Hasher>(&self,state:&mut H) {std::hash::Hash::hash(&Arc::as_ptr(&self.0),state)}}
impl Content {
    pub fn owner(&self)->OccurrenceHandle {self.0.owner}
    pub fn keys(&self)->Arc<[objects::ObjectKey]> {self.0.keys.clone()}
    pub fn bounds(&self)->DocRect {self.0.bounds}
    pub fn bounds_at(&self,level:u32)->DocRect {if self.0.keys[0].nearest {self.bounds()} else {self.bounds().expand(2<<level)}}
    pub fn query_at(&self,window:DocRect,level:u32)->Vec<objects::ObjectKey> {
        if self.bounds_at(level).intersect(window).is_empty() {Vec::new()} else {self.0.keys.to_vec()}
    }
    pub fn len(&self)->usize {1}
}
#[derive(Default,Clone)]
pub(super) struct SpatialIndex {
    objects:Option<Store<ImageObject>>,
    occurrences:Option<Store<Occurrence>>,
    layers:HashMap<OccurrenceHandle,Content>,
}
impl SpatialIndex {
    pub fn prepare(&mut self,scene:SceneView<'_>) {
        let artwork=scene.artwork();
        if self.objects.as_ref().is_none_or(|objects|!objects.same_root(&artwork.objects))
            || self.occurrences.as_ref().is_none_or(|occurrences|!occurrences.same_root(&artwork.occurrences)) {
            self.layers.retain(|owner,content|scene.object_handle(*owner).is_some_and(|h|objects::ObjectKey::new(scene,*owner,h)==content.0.keys[0]));
            self.objects=Some(artwork.objects.clone());self.occurrences=Some(artwork.occurrences.clone());
        }
    }
    pub fn content(&mut self,scene:SceneView<'_>,owner:OccurrenceHandle)->Option<Content> {
        self.prepare(scene);
        let handle=scene.object_handle(owner)?;
        let key=objects::ObjectKey::new(scene,owner,handle);
        if self.layers.get(&owner).is_none_or(|content|content.0.keys[0]!=key) {
            let bounds=objects::object_bounds(scene,&key);
            self.layers.insert(owner,Content(Arc::new(Entry {owner,bounds,keys:vec![key].into()})));
        }
        self.layers.get(&owner).cloned()
    }
    pub fn key_allocations(&self)->impl Iterator<Item=(usize,u64)>+'_ {
        self.layers.values().map(|content|(Arc::as_ptr(&content.0.keys) as *const objects::ObjectKey as usize,std::mem::size_of::<objects::ObjectKey>() as u64))
    }
    pub fn storage_bytes(&self)->u64 {
        self.layers.len() as u64*(std::mem::size_of::<Entry>()+std::mem::size_of::<objects::ObjectKey>()) as u64
    }
}

#[cfg(test)]
pub(in crate::scene) mod tests {
    use super::*;
    use layer_core::{Document, Edit, authored::*};

    pub(in crate::scene) fn document()->(Document,OccurrenceHandle,Vec<ImageObjectHandle>) {
        let mut art=Artwork::new([256;2]).unwrap();
        let image=layer_core::authored::Image::new(layer_core::color::source::rgba8_source([8;2],|_,_|[255;4]));
        let mut handles=Vec::new();let mut owners=Vec::new();
        for rank in 0..1024 {
            let mut object=ImageObject::new(image.clone());object.affine.0[4]=rank as f64*1024.-1024.;
            let handle=art.objects.insert(PortableId::random(),object).unwrap();handles.push(handle);
            owners.push(art.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Image")).unwrap());
        }
        let paint=art.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:[256;2],raster:Default::default(),base:None,operations:Arc::default()}).unwrap();
        let ink=art.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),"Ink")).unwrap();
        let stack=art.compositions.get(art.root).unwrap().result;let owner=owners[1];owners.push(ink);art.stacks.get_mut(stack).unwrap().entries=owners;
        (Document::from_artwork(art).unwrap(),owner,handles)
    }
    fn hits(index:&mut SpatialIndex,scene:SceneView<'_>,window:DocRect)->Vec<ImageObjectHandle> {
        scene.order().iter().rev().copied().filter(|owner|scene.visible(*owner)).flat_map(|owner|
            index.content(scene,owner).into_iter().flat_map(move |content|content.query_at(window,0))).map(|key|key.handle).collect()
    }
    #[test]
    fn coarse_linear_queries_include_tent_neighbors_without_expanding_nearest_queries() {
        let (mut doc,_,handles)=document();
        for &handle in &handles[..2] {doc.artwork.objects.get_mut(handle).unwrap().affine.0[4]=200.;}
        doc.artwork.objects.get_mut(handles[1]).unwrap().interpolation=ImageInterpolation::Nearest;
        let owners=handles[..2].iter().map(|h|doc.scene().object_owner(*h).unwrap()).collect::<Vec<_>>();
        let mut index=SpatialIndex::default();let linear=index.content(doc.scene(),owners[0]).unwrap();let nearest=index.content(doc.scene(),owners[1]).unwrap();
        let window=DocRect {min:[256,0],max:[512,256]};
        assert!(linear.query_at(window,0).is_empty());assert!(nearest.query_at(window,0).is_empty());
        assert_eq!(linear.query_at(window,8).iter().map(|key|key.handle).collect::<Vec<_>>(),[handles[0]]);
        assert!(nearest.query_at(window,8).is_empty());
        assert!(!linear.bounds_at(8).intersect(window).is_empty());
        for content in [linear,nearest] {assert_eq!(content.query_at(DocRect {min:[200,0],max:[208,8]},8).len(),1);}
    }
    #[test]
    fn unrelated_object_edits_keep_cached_layers_and_retired_roots_release_image_storage() {
        let (mut doc,owner,handles)=document();let mut index=SpatialIndex::default();
        let image=doc.artwork.objects.get(handles[0]).unwrap().image.clone();let weak=Arc::downgrade(image.storage());
        let (other,edit)=doc.create_object_layer_edit("Other",ImageObject::new(image.clone()),None,0).unwrap();doc.apply(edit).unwrap();drop(image);
        let child=doc.scene().object_handle(other).unwrap();let content=index.content(doc.scene(),owner).unwrap();
        doc.apply(doc.set_image_object_affine_edit(child,Affine64([1.,0.,0.,1.,8.,0.])).unwrap()).unwrap();
        assert!(content==index.content(doc.scene(),owner).unwrap());
        let mut renamed=doc.scene().occurrence(owner).unwrap().clone();renamed.name="Renamed".into();
        doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,owner,Some(renamed)).unwrap())).unwrap();
        assert!(content==index.content(doc.scene(),owner).unwrap());
        drop(doc);let empty=Document::from_artwork(Artwork::new([16;2]).unwrap()).unwrap();index.prepare(empty.scene());
        assert_eq!(weak.strong_count(),0);assert_eq!(index.storage_bytes(),0);
    }
    #[test]
    fn signed_queries_reuse_paint_independent_roots_and_rebuild_changed_object_bounds() {
        let (mut doc,owner,handles)=document();let mut index=SpatialIndex::default();
        let window=DocRect {min:[-4,-4],max:[12,12]};
        assert_eq!(hits(&mut index,doc.scene(),window),[handles[1]]);
        let content=index.content(doc.scene(),owner).unwrap();assert_eq!(index.layers.len(),1024);assert!(index.storage_bytes()<512*1024);
        let paint=doc.artwork.paint.iter().next().unwrap().0;let mut source=doc.artwork.paint.get(paint).unwrap().clone();source.domain=[257,256];
        doc.apply(Edit::Paint(RecordChange::replace(&doc.artwork.paint,paint,Some(source)).unwrap())).unwrap();
        assert_eq!(hits(&mut index,doc.scene(),window),[handles[1]]);assert!(content==index.content(doc.scene(),owner).unwrap());
        let changed=doc.scene().object_owner(handles[2]).unwrap();let old=index.content(doc.scene(),changed).unwrap();
        doc.apply(doc.set_image_object_affine_edit(handles[2],Affine64::default()).unwrap()).unwrap();
        assert_eq!(hits(&mut index,doc.scene(),window),[handles[2],handles[1]]);assert!(old!=index.content(doc.scene(),changed).unwrap());
        assert!(content==index.content(doc.scene(),owner).unwrap());
        assert_eq!(hits(&mut index,doc.scene(),DocRect {min:[-1030,-4],max:[-1010,12]}),[handles[0]]);
        let mut occurrence=doc.scene().occurrence(owner).unwrap().clone();occurrence.offset=[256,0];
        doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,owner,Some(occurrence)).unwrap())).unwrap();
        assert_eq!(hits(&mut index,doc.scene(),window),[handles[2]]);
        let shifted=DocRect {min:[252,-4],max:[268,12]};assert_eq!(hits(&mut index,doc.scene(),shifted),[handles[1]]);
        assert!(content!=index.content(doc.scene(),owner).unwrap());
        let before_hidden=index.content(doc.scene(),owner).unwrap();let mut hidden=doc.scene().occurrence(owner).unwrap().clone();hidden.visible=false;
        doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,owner,Some(hidden)).unwrap())).unwrap();
        assert!(hits(&mut index,doc.scene(),shifted).is_empty());let raw=index.content(doc.scene(),owner).unwrap();
        assert!(before_hidden==raw);assert_eq!(raw.query_at(shifted,0).len(),1);
    }
}
