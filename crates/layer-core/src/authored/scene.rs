use super::*;
use crate::{EffectView, LayerKind, Point, ImageTransform, Affine, Projective, LayerPlacement, raster::RasterRevision};
use std::{collections::BTreeSet,sync::Arc};

#[derive(Clone,Debug,Default,PartialEq)]
pub struct SceneIndex {
    order:Vec<OccurrenceHandle>,
    positions:Vec<Option<usize>>,
    parents:Vec<Option<OccurrenceHandle>>,
    containing:Vec<Option<StackHandle>>,
    paint_uses:Vec<Option<OccurrenceHandle>>,
    coverage_uses:Vec<Option<OccurrenceHandle>>,
    selection_uses:Vec<Option<OccurrenceHandle>>,
}
impl SceneIndex {
    pub fn build(artwork:&Artwork)->Result<Self,String> {
        let shape=artwork.topology()?;
        let root=artwork.compositions.id(artwork.root).ok_or("Missing composition")?;
        if !matches!(shape.validate(root,Default::default())?,Support::Editable){return Err("Artwork is outside the editable subset".into());}
        let n=artwork.occurrences.capacity();
        let mut index=Self {order:Vec::with_capacity(artwork.occurrences.len()),positions:vec![None;n],parents:vec![None;n],containing:vec![None;n],
            paint_uses:vec![None;artwork.paint.capacity()],coverage_uses:vec![None;artwork.coverage.capacity()],selection_uses:vec![None;artwork.selections.capacity()]};
        fn visit(a:&Artwork,index:&mut SceneIndex,stack:StackHandle,parent:Option<OccurrenceHandle>)->Result<(),String>{
            for &h in &a.stacks.get(stack).ok_or("Missing stack")?.entries {
                let at=h.index() as usize;
                index.containing[at]=Some(stack);index.parents[at]=parent;
                index.positions[at]=Some(index.order.len());index.order.push(h);
                if let OccurrenceContent::Stack(child)=a.occurrences.get(h).ok_or("Missing occurrence")?.content {visit(a,index,child,Some(h))?;}
            }
            Ok(())
        }
        visit(artwork,&mut index,artwork.compositions.get(artwork.root).ok_or("Missing composition")?.result,None)?;
        for &h in &index.order {
            let o=artwork.occurrences.get(h).ok_or("Missing placed occurrence")?;
            match o.content {
                OccurrenceContent::Paint(p)=>index.paint_uses[p.index() as usize]=Some(h),
                OccurrenceContent::Selection(p)=>index.selection_uses[p.index() as usize]=Some(h),_=>(),
            }
            if let Some(mask)=&o.mask {index.coverage_uses[mask.source.index() as usize]=Some(h);}
        }
        Ok(index)
    }
}

#[derive(Clone,Debug,Default,PartialEq)]
pub enum SceneScope {
    #[default] All,
    Members(Arc<[OccurrenceHandle]>),
    Raw(SourceTarget),
    Prefix {before:OccurrenceHandle,clipped:bool},
}
#[derive(Clone,Debug,PartialEq)]
pub struct SceneSnapshot {
    pub artwork:Artwork,
    pub index:Arc<SceneIndex>,
    pub owner:u64,
    pub revision:u64,
    pub context:EvaluationContext,
    pub scope:SceneScope,
    pub offset:Point,
}
impl SceneSnapshot {
    pub fn new(artwork:Artwork,index:Arc<SceneIndex>,owner:u64,revision:u64,context:EvaluationContext)->Self {
        Self {artwork,index,owner,revision,context,scope:SceneScope::All,offset:Point::default()}
    }
    pub fn view(&self)->SceneView<'_>{SceneView::new(&self.artwork,&self.index).with_owner(self.owner,self.revision).with_scope(&self.scope).with_offset(self.offset).with_context(&self.context)}
    pub fn with_scope(mut self,scope:SceneScope)->Self{self.scope=scope;self}
    pub fn scoped_transfer_artwork(&self,required:&[SourceTarget])->Artwork {
        let mut artwork=self.artwork.clone();
        let paint:Vec<_>=artwork.paint.iter().map(|(h,_,_)|h).filter(|h|!required.contains(&SourceTarget::Paint(*h))).collect();
        for handle in paint {
            let source=artwork.paint.get_mut(handle).unwrap();
            source.raster=Default::default();source.original=None;source.operations=Arc::default();
        }
        let coverage:Vec<_>=artwork.coverage.iter().map(|(h,_,_)|h).filter(|h|!required.contains(&SourceTarget::Coverage(*h))).collect();
        for handle in coverage {
            let source=artwork.coverage.get_mut(handle).unwrap();
            source.raster=Default::default();source.initial=None;source.operations=Arc::default();
        }
        artwork
    }
}
#[derive(Clone,Copy,Debug)]
pub struct SceneView<'a> {
    artwork:&'a Artwork,
    index:&'a Arc<SceneIndex>,
    scope:Option<&'a SceneScope>,
    owner:u64,
    revision:u64,
    offset:Point,
    context:Option<&'a EvaluationContext>,
}
impl<'a> SceneView<'a> {
    #[inline]
    pub fn new(artwork:&'a Artwork,index:&'a Arc<SceneIndex>)->Self{Self {artwork,index,scope:None,owner:0,revision:0,offset:Point::default(),context:None}}
    #[inline]
    pub fn with_context(mut self,context:&'a EvaluationContext)->Self{self.context=Some(context);self}
    #[inline]
    pub fn evaluation_context(self)->Option<&'a EvaluationContext>{self.context}
    #[inline]
    pub fn with_offset(mut self,offset:Point)->Self{self.offset.x+=offset.x;self.offset.y+=offset.y;self}
    #[inline]
    pub fn with_owner(mut self,owner:u64,revision:u64)->Self{self.owner=owner;self.revision=revision;self}
    #[inline]
    pub fn with_scope(mut self,scope:&'a SceneScope)->Self{self.scope=Some(scope);self}
    pub fn snapshot(self,context:EvaluationContext)->SceneSnapshot{SceneSnapshot {artwork:self.artwork.clone(),index:self.index.clone(),owner:self.owner,revision:self.revision,context,scope:self.scope.cloned().unwrap_or_default(),offset:self.offset}}
    #[inline]
    pub fn owner(self)->u64{self.owner}
    #[inline]
    pub fn revision(self)->u64{self.revision}
    #[inline]
    pub fn scope(self)->Option<&'a SceneScope>{self.scope}
    #[inline]
    pub fn original(self,t:SourceTarget)->Option<&'a Arc<crate::color::source::SourceImage>>{match t{SourceTarget::Paint(h)=>self.paint(h)?.original.as_ref(),_=>None}}
    #[inline]
    pub fn artwork(self)->&'a Artwork{self.artwork}
    #[inline]
    pub fn composition(self)->&'a Composition{self.artwork.compositions.get(self.artwork.root).expect("Admitted composition")}
    #[inline]
    pub fn output(self)->&'a Output{self.artwork.outputs.get(self.artwork.default_output).expect("Admitted output")}
    #[inline]
    pub fn order(self)->&'a [OccurrenceHandle]{&self.index.order}
    pub fn constant_backdrop(self)->&'a [OccurrenceHandle]{
        let roots=self.children(None);
        let count=roots.iter().rev().take_while(|&&h|self.includes(h)&&self.occurrence(h).is_some_and(|o|
            !o.clipped&&o.blend==crate::LayerBlend::Normal&&!o.mask.as_ref().is_some_and(|m|m.enabled)
                &&self.effect(h).is_some_and(|e|e.constant_color().is_some()))).count();
        &roots[roots.len()-count..]
    }
    #[inline]
    pub fn occurrence(self,h:OccurrenceHandle)->Option<&'a Occurrence>{self.artwork.occurrences.get(h)}
    #[inline]
    pub fn position(self,h:OccurrenceHandle)->Option<usize>{self.index.positions.get(h.index() as usize).copied().flatten()}
    #[inline]
    pub fn parent(self,h:OccurrenceHandle)->Option<OccurrenceHandle>{self.index.parents.get(h.index() as usize).copied().flatten()}
    #[inline]
    pub fn stack(self,h:OccurrenceHandle)->Option<StackHandle>{self.index.containing.get(h.index() as usize).copied().flatten()}
    #[inline]
    pub fn children(self,parent:Option<OccurrenceHandle>)->&'a [OccurrenceHandle]{
        let h=match parent {Some(h)=>match self.occurrence(h).map(|o|&o.content){Some(OccurrenceContent::Stack(s))=>*s,_=>return &[]},None=>self.composition().result};
        self.artwork.stacks.get(h).map_or(&[],|s|s.entries.as_slice())
    }
    #[inline]
    pub fn evaluation_parent(self,h:OccurrenceHandle)->Option<OccurrenceHandle>{
        let mut parent=self.parent(h);while let Some(h)=parent{if self.includes(h){return Some(h);}parent=self.parent(h);}None
    }
    #[inline]
    pub fn members(self,parent:Option<OccurrenceHandle>)->SceneChildren<'a>{
        if matches!(self.scope,Some(SceneScope::Members(_))|Some(SceneScope::Raw(_))){SceneChildren::Scoped{order:self.order().iter(),scene:self,parent}}
        else{SceneChildren::Stack(self.children(parent).iter())}
    }
    #[inline]
    pub fn effective_clipped(self,h:OccurrenceHandle)->bool{
        let Some(o)=self.occurrence(h) else{return false;};if !o.clipped{return false;}
        if !matches!(self.scope,Some(SceneScope::Members(_))){return true;}
        let siblings=self.children(self.parent(h));let Some(at)=siblings.iter().position(|v|*v==h) else{return false;};
        siblings[at+1..].iter().find(|v|self.occurrence(**v).is_some_and(|o|o.is_artwork()&&!o.clipped)).is_some_and(|v|self.includes(*v))
    }
    #[inline]
    pub fn paint(self,h:PaintHandle)->Option<&'a PaintSource>{self.artwork.paint.get(h)}
    #[inline]
    pub fn coverage(self,h:CoverageHandle)->Option<&'a CoverageSource>{self.artwork.coverage.get(h)}
    #[inline]
    pub fn paint_source(self,h:OccurrenceHandle)->Option<&'a PaintSource>{match self.occurrence(h)?.content{OccurrenceContent::Paint(p)=>self.paint(p),_=>None}}
    #[inline]
    pub fn effect_handle(self,h:OccurrenceHandle)->Option<EffectHandle>{match self.occurrence(h)?.content{OccurrenceContent::Effect(e)=>Some(e),_=>None}}
    #[inline]
    pub fn effect_application(self,h:OccurrenceHandle)->Option<&'a EffectApplication>{self.artwork.effects.get(self.effect_handle(h)?)}
    #[inline]
    pub fn effect_by_handle(self,h:EffectHandle)->Option<EffectView<'a>>{
        let application=self.artwork.effects.get(h)?;let definition=self.artwork.definitions.get(application.definition)?;
        Some(EffectView::new(&definition.program,&application.values))
    }
    #[inline]
    pub fn effect(self,h:OccurrenceHandle)->Option<EffectView<'a>>{self.effect_by_handle(self.effect_handle(h)?)}
    #[inline]
    pub fn mask(self,h:OccurrenceHandle)->Option<(&'a MaskUse,&'a CoverageSource)>{let mask=self.occurrence(h)?.mask.as_ref()?;Some((mask,self.coverage(mask.source)?))}
    #[inline]
    pub fn source_target(self,h:OccurrenceHandle)->Option<SourceTarget>{match self.occurrence(h)?.content{OccurrenceContent::Paint(p)=>Some(SourceTarget::Paint(p)),OccurrenceContent::Selection(s)=>Some(SourceTarget::Selection(s)),_=>None}}
    #[inline]
    pub fn source_owner(self,t:SourceTarget)->Option<OccurrenceHandle>{match t {
        SourceTarget::Paint(h)=>self.index.paint_uses.get(h.index() as usize),SourceTarget::Coverage(h)=>self.index.coverage_uses.get(h.index() as usize),SourceTarget::Selection(h)=>self.index.selection_uses.get(h.index() as usize),
    }.copied().flatten()}
    #[inline]
    pub fn targets(self)->impl Iterator<Item=SourceTarget>+'a{
        self.artwork.paint.iter().map(|(h,_,_)|SourceTarget::Paint(h)).chain(self.artwork.coverage.iter().map(|(h,_,_)|SourceTarget::Coverage(h))).chain(self.artwork.selections.iter().map(|(h,_,_)|SourceTarget::Selection(h)))
    }
    #[inline]
    pub fn raster(self,t:SourceTarget)->Option<&'a RasterRevision>{match t{SourceTarget::Paint(h)=>Some(&self.paint(h)?.raster),SourceTarget::Coverage(h)=>Some(&self.coverage(h)?.raster),SourceTarget::Selection(_)=>None}}
    #[inline]
    pub fn operations(self,t:SourceTarget)->Option<&'a [crate::RasterOperation]>{match t{SourceTarget::Paint(h)=>Some(&self.paint(h)?.operations),SourceTarget::Coverage(h)=>Some(&self.coverage(h)?.operations),SourceTarget::Selection(_)=>None}}
    #[inline]
    pub fn target_extent(self,t:SourceTarget)->[u32;2]{match t{SourceTarget::Paint(h)=>self.paint(h).map(|p|p.domain),SourceTarget::Coverage(h)=>self.coverage(h).map(|p|p.domain),SourceTarget::Selection(_)=>None}.unwrap_or(self.composition().size)}
    #[inline]
    pub fn local_extent(self,h:OccurrenceHandle)->[u32;2]{self.source_target(h).map(|t|self.target_extent(t)).or_else(||self.effect_application(h).map(|e|e.domain)).unwrap_or(self.composition().size)}
    #[inline]
    pub fn includes(self,h:OccurrenceHandle)->bool{match self.scope{None|Some(SceneScope::All)|Some(SceneScope::Prefix{..})=>true,Some(SceneScope::Members(m))=>m.contains(&h),Some(SceneScope::Raw(t))=>self.source_owner(*t)==Some(h)}}
    #[inline]
    pub fn visible(self,h:OccurrenceHandle)->bool{
        if !self.includes(h){return false;}
        if matches!(self.scope,Some(SceneScope::Raw(_))){return true;}let mut current=Some(h);
        while let Some(h)=current {if self.occurrence(h).is_none_or(|o|!o.visible){return false;}current=self.evaluation_parent(h);}true
    }
    #[inline]
    pub fn stop_before(self)->Option<(OccurrenceHandle,bool)>{match self.scope{Some(SceneScope::Prefix{before,clipped})=>Some((*before,*clipped)),_=>None}}
    #[inline]
    pub fn occurrence_offset(self,h:OccurrenceHandle)->Point {
        let mut offset=self.offset;let mut current=Some(h);
        while let Some(h)=current {let Some(o)=self.occurrence(h) else{break;};offset.x+=o.translation.x;offset.y+=o.translation.y;current=self.parent(h);}offset
    }
    pub fn target_geometry(self,t:SourceTarget)->ImageTransform {
        let Some(h)=self.source_owner(t) else{return ImageTransform::default();};let owner=self.occurrence(h).unwrap();
        let world=self.occurrence_offset(h);
        if matches!(t,SourceTarget::Coverage(_)) {
            let Some((mask,_))=self.mask(h) else{return ImageTransform::default();};let mut geometry=mask.geometry_in_parent(owner);
            let parent=Point{x:world.x-owner.translation.x,y:world.y-owner.translation.y};
            geometry.placement=geometry.placement.post(Projective::from_affine(Affine::translation(parent))).unwrap_or_else(||LayerPlacement::from_projective(Projective([f32::NAN;9])));geometry
        } else {ImageTransform {placement:owner.placement.post(Projective::from_affine(Affine::translation(world))).unwrap_or_else(||LayerPlacement::from_projective(Projective([f32::NAN;9]))),..Default::default()}}
    }
    pub fn target_offset(self,t:SourceTarget)->Point {
        let Some(h)=self.source_owner(t) else{return Point::default();};let mut offset=self.occurrence_offset(h);
        if matches!(t,SourceTarget::Coverage(_)) && let Some((mask,_))=self.mask(h) {let owner=self.occurrence(h).unwrap();offset.x+=mask.translation.x-owner.translation.x;offset.y+=mask.translation.y-owner.translation.y;}offset
    }
    pub fn references(self)->BTreeSet<OccurrenceHandle>{self.order().iter().copied().filter(|h|self.occurrence(*h).is_some_and(|o|o.reference)).collect()}
}
impl Occurrence {
    #[inline]
    pub fn kind(&self)->LayerKind {match self.content{OccurrenceContent::Paint(_)=>LayerKind::Paint,OccurrenceContent::Stack(_)=>LayerKind::Group,OccurrenceContent::Effect(_)=>LayerKind::Effect,OccurrenceContent::Selection(_)=>LayerKind::Selection}}
    #[inline]
    pub fn is_artwork(&self)->bool{!matches!(self.content,OccurrenceContent::Selection(_))}
    #[inline]
    pub fn passes_through(&self)->bool{matches!(self.content,OccurrenceContent::Stack(_)) && self.blend==crate::LayerBlend::PassThrough && !self.clipped}
}
impl Default for SourceTarget {fn default()->Self{Self::Paint(PaintHandle::INVALID)}}
impl SourceTarget {
    pub fn wire_id(self)->u64{match self{Self::Paint(h)=>u64::from(h.index())+1,Self::Coverage(h)=>(1u64<<32)|(u64::from(h.index())+1),Self::Selection(h)=>(2u64<<32)|(u64::from(h.index())+1)}}
    pub fn from_wire_id(value:u64)->Option<Self>{let index=u32::try_from(value&u64::from(u32::MAX)).ok()?.checked_sub(1)?;Some(match value>>32{0=>Self::Paint(PaintHandle::from_index(index)),1=>Self::Coverage(CoverageHandle::from_index(index)),2=>Self::Selection(SelectionHandle::from_index(index)),_=>return None})}
    #[inline]
    pub fn is_coverage(self)->bool{matches!(self,Self::Coverage(_))}
}

#[derive(Clone,Debug,PartialEq)]
pub struct EffectBaseline {pub occurrence:OccurrenceHandle,pub effect:EffectHandle,pub application:EffectApplication}

pub enum SceneChildren<'a>{Stack(std::slice::Iter<'a,OccurrenceHandle>),Scoped{order:std::slice::Iter<'a,OccurrenceHandle>,scene:SceneView<'a>,parent:Option<OccurrenceHandle>}}
impl Iterator for SceneChildren<'_>{
    type Item=OccurrenceHandle;
    #[inline]
    fn next(&mut self)->Option<Self::Item>{match self{Self::Stack(iter)=>iter.next().copied(),Self::Scoped{order,scene,parent}=>order.find(|h|scene.includes(**h)&&scene.evaluation_parent(**h)==*parent).copied()}}
}
impl DoubleEndedIterator for SceneChildren<'_>{
    #[inline]
    fn next_back(&mut self)->Option<Self::Item>{match self{Self::Stack(iter)=>iter.next_back().copied(),Self::Scoped{order,scene,parent}=>order.rfind(|h|scene.includes(**h)&&scene.evaluation_parent(**h)==*parent).copied()}}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constant_backdrop_is_a_borrowed_contiguous_root_suffix() {
        let mut doc=crate::Document::new(PortableId::random(),16,16,crate::DocumentNames{paint:"Ink".into(),paper:"White fill".into()});
        let root=doc.composition().result;let fill=doc.scene().children(None)[1];
        assert_eq!(doc.scene().effect(fill).unwrap().constant_color(),Some(crate::color::RgbColor::WHITE));
        assert_eq!(doc.scene().constant_backdrop(),&[fill]);
        assert_eq!(doc.scene().constant_backdrop().as_ptr(),doc.scene().children(None)[1..].as_ptr());
        let (edit,copies)=doc.duplicate_layers_edit(&[fill]).unwrap();doc.apply(edit).unwrap();
        assert_eq!(doc.scene().constant_backdrop(),&[copies[0],fill]);
        doc.artwork.occurrences.get_mut(copies[0]).unwrap().blend=crate::LayerBlend::Multiply;
        assert_eq!(doc.scene().constant_backdrop(),&[fill]);
        doc.artwork.occurrences.get_mut(copies[0]).unwrap().blend=crate::LayerBlend::Normal;
        doc.artwork.occurrences.get_mut(fill).unwrap().clipped=true;
        assert!(doc.scene().constant_backdrop().is_empty());
        doc.artwork.occurrences.get_mut(fill).unwrap().clipped=false;
        let coverage=doc.allocate_coverage_handle();let mask=crate::CoverageSnapshot::reveal_all(coverage,[16,16],Point::default());
        doc.artwork.coverage.install(coverage,mask.source).unwrap();doc.artwork.occurrences.get_mut(fill).unwrap().mask=Some(mask.use_);
        assert!(doc.scene().constant_backdrop().is_empty());
        doc.artwork.occurrences.get_mut(fill).unwrap().mask.as_mut().unwrap().enabled=false;
        assert_eq!(doc.scene().constant_backdrop(),&[copies[0],fill]);
        let scope=SceneScope::Members(Arc::from([copies[0]]));
        assert!(doc.scene().with_scope(&scope).constant_backdrop().is_empty());
        doc.artwork.stacks.get_mut(root).unwrap().entries.swap(0,2);
        doc.scene_index=Arc::new(SceneIndex::build(&doc.artwork).unwrap());
        assert!(doc.scene().constant_backdrop().is_empty());
    }
    #[test]
    fn fill_routes_drawing_only_to_its_own_mask() {
        let mut doc=crate::Document::new(PortableId::random(),16,16,crate::DocumentNames{paint:"Ink".into(),paper:"White fill".into()});
        let fill=doc.scene().children(None)[1];doc.working.occurrence=Some(fill);doc.working.target=None;
        assert_eq!(doc.try_drawing_target(),Err(crate::DrawingRefusal::Fill));
        let coverage=doc.allocate_coverage_handle();let mask=crate::CoverageSnapshot::reveal_all(coverage,[16,16],Point::default());
        doc.artwork.coverage.install(coverage,mask.source).unwrap();doc.artwork.occurrences.get_mut(fill).unwrap().mask=Some(mask.use_);
        doc.scene_index=Arc::new(SceneIndex::build(&doc.artwork).unwrap());
        assert_eq!(doc.try_drawing_target(),Ok(SourceTarget::Coverage(coverage)));
    }
    #[test]
    fn unplaced_occurrences_retain_sources_without_entering_the_evaluation_index() {
        let mut artwork=Artwork::new([16,16]).unwrap();
        let paint=artwork.paint.insert(PortableId::random(),PaintSource {domain:[16,16],raster:Default::default(),original:None,operations:Arc::default()}).unwrap();
        let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),"Retained content")).unwrap();
        let index=Arc::new(SceneIndex::build(&artwork).unwrap());let scene=SceneView::new(&artwork,&index);
        assert!(scene.targets().any(|t|t==SourceTarget::Paint(paint)));
        assert!(scene.occurrence(occurrence).is_some());
        assert_eq!(scene.source_owner(SourceTarget::Paint(paint)),None);
        assert_eq!(scene.position(occurrence),None);
    }
}
