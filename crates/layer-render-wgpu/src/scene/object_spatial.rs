use super::*;
use layer_core::authored::{ImageObject, ObjectLayer, Store};
use std::{collections::HashMap, ops::Range};

struct Entry { bounds:DocRect, rank:usize, key:objects::ObjectKey }
enum Children { Leaf(Range<usize>), Branch([usize;2]) }
struct Node { bounds:DocRect, children:Children }
struct Tree { owner:OccurrenceHandle, entries:Vec<Entry>, nodes:Vec<Node>, keys:Arc<[objects::ObjectKey]>, linear:bool }
impl Tree {
    fn new(scene:SceneView<'_>,owner:OccurrenceHandle)->Self {
        let entries=scene.object_layer(owner).into_iter().flat_map(|layer|layer.children.iter().copied().enumerate())
            .filter(|(_,h)|scene.object(*h).is_some_and(|object|object.visible))
            .map(|(rank,handle)| {let key=objects::ObjectKey::new(scene,owner,handle);Entry {bounds:objects::object_bounds(scene,&key),rank,key}})
            .filter(|entry|!entry.bounds.is_empty()).collect::<Vec<_>>();
        let keys:Arc<[objects::ObjectKey]>=entries.iter().map(|entry|entry.key.clone()).collect::<Vec<_>>().into();
        let linear=keys.iter().any(|key|!key.nearest);
        let mut tree=Self {owner,entries,nodes:Vec::new(),keys,linear};
        if !tree.entries.is_empty() {tree.build(0..tree.entries.len());}
        tree
    }
    fn build(&mut self,range:Range<usize>)->usize {
        let bounds=self.entries[range.clone()].iter().fold(DocRect::default(),|bounds,entry|bounds.union(entry.bounds));
        let index=self.nodes.len();
        self.nodes.push(Node {bounds,children:Children::Leaf(range.clone())});
        if range.len()>4 {
            let axis=usize::from(bounds.max[1].saturating_sub(bounds.min[1])>bounds.max[0].saturating_sub(bounds.min[0]));
            let middle=range.start+range.len()/2;
            self.entries[range.clone()].select_nth_unstable_by_key(middle-range.start,|entry|
                i128::from(entry.bounds.min[axis])+i128::from(entry.bounds.max[axis]));
            let left=self.build(range.start..middle);let right=self.build(middle..range.end);
            self.nodes[index].children=Children::Branch([left,right]);
        }
        index
    }
    fn visit(&self,node:usize,window:DocRect,nearest_window:DocRect,result:&mut Vec<(usize,objects::ObjectKey)>) {
        let node=&self.nodes[node];
        if node.bounds.intersect(window).is_empty() {return;}
        match &node.children {
            Children::Leaf(range)=>result.extend(self.entries[range.clone()].iter()
                .filter(|entry|!entry.bounds.intersect(if entry.key.nearest {nearest_window} else {window}).is_empty()).map(|entry|(entry.rank,entry.key.clone()))),
            Children::Branch(children)=>{for &child in children {self.visit(child,window,nearest_window,result);}},
        }
    }
    fn query_radius(&self,window:DocRect,radius:u32)->Vec<objects::ObjectKey> {
        let mut result=Vec::new();
        if !self.nodes.is_empty() && !window.is_empty() {self.visit(0,window.expand(radius),window,&mut result);}
        result.sort_unstable_by_key(|(rank,_)|std::cmp::Reverse(*rank));
        result.into_iter().map(|(_,handle)|handle).collect()
    }
    fn storage_bytes(&self)->u64 {
        (self.entries.capacity()*std::mem::size_of::<Entry>()+self.nodes.capacity()*std::mem::size_of::<Node>()+self.keys.len()*std::mem::size_of::<objects::ObjectKey>()) as u64
    }
}
#[derive(Clone)]
pub(super) struct Content(Arc<Tree>);
impl PartialEq for Content {fn eq(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}}
impl Eq for Content {}
impl PartialOrd for Content {fn partial_cmp(&self,other:&Self)->Option<std::cmp::Ordering> {Some(self.cmp(other))}}
impl Ord for Content {fn cmp(&self,other:&Self)->std::cmp::Ordering {Arc::as_ptr(&self.0).cmp(&Arc::as_ptr(&other.0))}}
impl std::hash::Hash for Content {fn hash<H:std::hash::Hasher>(&self,state:&mut H) {std::hash::Hash::hash(&Arc::as_ptr(&self.0),state)}}
impl Content {
    pub fn owner(&self)->OccurrenceHandle {self.0.owner}
    pub fn keys(&self)->Arc<[objects::ObjectKey]> {self.0.keys.clone()}
    pub fn bounds(&self)->DocRect {self.0.nodes.first().map_or(DocRect::default(),|node|node.bounds)}
    pub fn bounds_at(&self,level:u32)->DocRect {if self.0.linear {self.bounds().expand(2<<level)} else {self.bounds()}}
    pub fn query_at(&self,window:DocRect,level:u32)->Vec<objects::ObjectKey> {self.0.query_radius(window,2<<level)}
    pub fn len(&self)->usize {self.0.entries.len()}
}
#[derive(Clone)]
struct LayerIndex {offset:[f64;2],tree:Arc<Tree>}
#[derive(Default,Clone)]
pub(super) struct SpatialIndex {
    objects:Option<Store<ImageObject>>,
    collections:Option<Store<ObjectLayer>>,
    layers:HashMap<OccurrenceHandle,LayerIndex>,
}
impl SpatialIndex {
    pub fn prepare(&mut self,scene:SceneView<'_>) {
        let artwork=scene.artwork();
        if self.objects.as_ref().is_none_or(|objects|!objects.same_root(&artwork.objects))
            || self.collections.as_ref().is_none_or(|collections|!collections.same_root(&artwork.object_layers)) {
            self.layers.retain(|owner,index|scene.object_layer(*owner).is_some_and(|layer|
                index.offset==scene.occurrence_offset64(*owner) && layer.children.iter().copied()
                    .filter(|h|scene.object(*h).is_some_and(|object|object.visible))
                    .map(|h|objects::ObjectKey::new(scene,*owner,h)).eq(index.tree.keys.iter().cloned())));
            self.objects=Some(artwork.objects.clone());self.collections=Some(artwork.object_layers.clone());
        }
    }
    pub fn content(&mut self,scene:SceneView<'_>,owner:OccurrenceHandle)->Option<Content> {
        self.prepare(scene);
        if scene.object_layer(owner).is_none() {self.layers.remove(&owner);return None;}
        let offset=scene.occurrence_offset64(owner);
        if self.layers.get(&owner).is_none_or(|layer|layer.offset!=offset) {
            self.layers.insert(owner,LayerIndex {offset,tree:Arc::new(Tree::new(scene,owner))});
        }
        Some(Content(self.layers[&owner].tree.clone()))
    }
    pub fn key_allocations(&self)->impl Iterator<Item=(usize,u64)>+'_ {
        self.layers.values().map(|layer|(Arc::as_ptr(&layer.tree.keys) as *const objects::ObjectKey as usize,layer.tree.keys.len() as u64*std::mem::size_of::<objects::ObjectKey>() as u64))
    }
    pub fn storage_bytes(&self)->u64 {
        self.layers.values().map(|layer|layer.tree.storage_bytes()).sum()
    }
}

#[cfg(test)]
pub(in crate::scene) mod tests {
    use super::*;
    use layer_core::{Document, Edit, Point, authored::*};

    pub(in crate::scene) fn document()->(Document,OccurrenceHandle,Vec<ImageObjectHandle>) {
        let mut art=Artwork::new([256;2]).unwrap();
        let image=layer_core::authored::Image::new(layer_core::color::source::rgba8_source([8;2],|_,_|[255;4]));
        let handles=(0..1024).map(|rank| {
            let mut object=ImageObject::new(image.clone(),"");object.affine.0[4]=rank as f64*1024.-1024.;
            art.objects.insert(PortableId::random(),object).unwrap()
        }).collect::<Vec<_>>();
        let collection=art.object_layers.insert(PortableId::random(),ObjectLayer {children:handles.clone()}).unwrap();
        let owner=art.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(collection),"Images")).unwrap();
        let paint=art.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:[256;2],raster:Default::default(),base:None,operations:Arc::default()}).unwrap();
        let ink=art.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),"Ink")).unwrap();
        let stack=art.compositions.get(art.root).unwrap().result;art.stacks.get_mut(stack).unwrap().entries=vec![owner,ink];
        (Document::from_artwork(art).unwrap(),owner,handles)
    }
    fn hits(index:&mut SpatialIndex,scene:SceneView<'_>,owner:OccurrenceHandle,window:DocRect)->Vec<ImageObjectHandle> {
        index.content(scene,owner).unwrap().query_at(window,0).into_iter().map(|key|key.handle).collect()
    }
    #[test]
    fn coarse_linear_queries_include_tent_neighbors_without_expanding_nearest_queries() {
        let (mut doc,owner,handles)=document();
        for &handle in &handles[..2] {doc.artwork.objects.get_mut(handle).unwrap().affine.0[4]=200.;}
        doc.artwork.objects.get_mut(handles[1]).unwrap().interpolation=ImageInterpolation::Nearest;
        let collection=doc.artwork.object_layers.iter().next().unwrap().0;
        doc.artwork.object_layers.get_mut(collection).unwrap().children=handles[..2].to_vec();
        let mut index=SpatialIndex::default();let content=index.content(doc.scene(),owner).unwrap();
        let window=DocRect {min:[256,0],max:[512,256]};
        assert!(content.query_at(window,0).is_empty());
        assert_eq!(content.query_at(window,8).iter().map(|key|key.handle).collect::<Vec<_>>(),[handles[0]]);
        assert!(!content.bounds_at(8).intersect(window).is_empty());
        assert_eq!(content.query_at(DocRect {min:[200,0],max:[208,8]},8).len(),2);
    }
    #[test]
    fn unrelated_object_edits_keep_cached_layers_and_retired_roots_release_image_storage() {
        let (mut doc,owner,handles)=document();let mut index=SpatialIndex::default();
        let window=DocRect {min:[-4,-4],max:[12,12]};
        let image=doc.artwork.objects.get(handles[0]).unwrap().image.clone();
        let weak=Arc::downgrade(image.storage());
        let (other,edit)=doc.create_object_layer_edit("Other",None,0).unwrap();doc.apply(edit).unwrap();
        let (child,edit)=doc.add_image_object_edit(other,ImageObject::new(image.clone(),"Other image"),0).unwrap();doc.apply(edit).unwrap();
        drop(image);
        hits(&mut index,doc.scene(),owner,window);let tree=index.layers[&owner].tree.clone();
        doc.apply(doc.set_image_object_affine_edit(child,Affine64([1.,0.,0.,1.,8.,0.])).unwrap()).unwrap();
        assert_eq!(hits(&mut index,doc.scene(),owner,window),[handles[1]]);
        assert!(Arc::ptr_eq(&tree,&index.layers[&owner].tree));
        let mut renamed=doc.artwork.objects.get(handles[1]).unwrap().clone();renamed.name="Renamed".into();
        doc.apply(Edit::ImageObject(RecordChange::replace(&doc.artwork.objects,handles[1],Some(renamed)).unwrap())).unwrap();
        hits(&mut index,doc.scene(),owner,window);assert!(Arc::ptr_eq(&tree,&index.layers[&owner].tree));
        drop(doc);
        let empty=Document::from_artwork(Artwork::new([16;2]).unwrap()).unwrap();index.prepare(empty.scene());
        assert_eq!(weak.strong_count(),0);assert_eq!(index.storage_bytes(),0);
    }

    #[test]
    fn signed_queries_reuse_paint_independent_roots_and_rebuild_changed_object_bounds() {
        let (mut doc,owner,handles)=document();let mut index=SpatialIndex::default();
        let window=DocRect {min:[-4,-4],max:[12,12]};
        assert_eq!(hits(&mut index,doc.scene(),owner,window),[handles[1]]);
        let tree=index.layers[&owner].tree.clone();
        assert_eq!(tree.entries.len(),1024);assert!(tree.nodes.len()<1024);
        assert!(index.storage_bytes()<512*1024);
        let paint=doc.artwork.paint.iter().next().unwrap().0;
        let mut source=doc.artwork.paint.get(paint).unwrap().clone();source.domain=[257,256];
        doc.apply(Edit::Paint(RecordChange::replace(&doc.artwork.paint,paint,Some(source)).unwrap())).unwrap();
        assert_eq!(hits(&mut index,doc.scene(),owner,window),[handles[1]]);
        assert!(Arc::ptr_eq(&tree,&index.layers[&owner].tree));
        doc.apply(doc.set_image_object_affine_edit(handles[2],Affine64::default()).unwrap()).unwrap();
        assert_eq!(hits(&mut index,doc.scene(),owner,window),[handles[2],handles[1]]);
        assert!(!Arc::ptr_eq(&tree,&index.layers[&owner].tree));
        assert_eq!(hits(&mut index,doc.scene(),owner,DocRect {min:[-1030,-4],max:[-1010,12]}),[handles[0]]);
        let mut occurrence=doc.scene().occurrence(owner).unwrap().clone();occurrence.translation=Point {x:256.,y:0.};
        doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,owner,Some(occurrence)).unwrap())).unwrap();
        assert!(hits(&mut index,doc.scene(),owner,window).is_empty());
        assert_eq!(hits(&mut index,doc.scene(),owner,DocRect {min:[252,-4],max:[268,12]}),[handles[2],handles[1]]);
        let mut child=doc.artwork.objects.get(handles[1]).unwrap().clone();child.visible=false;
        doc.apply(Edit::ImageObject(RecordChange::replace(&doc.artwork.objects,handles[1],Some(child)).unwrap())).unwrap();
        assert_eq!(hits(&mut index,doc.scene(),owner,DocRect {min:[252,-4],max:[268,12]}),[handles[2]]);
    }
}
