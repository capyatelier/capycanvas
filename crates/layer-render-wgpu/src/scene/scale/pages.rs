use super::*;
use layer_core::raster::ContentVersion;

#[derive(Default)]
pub(super) struct Pixels {
    root: Option<Box<Window>>,
    next: Option<Box<Window>>,
    unreduced: Backlog,
}
#[derive(Default)]
pub(super) struct Backlog {region:PixelRect,taken:Vec<(submission::Submission,PixelRect)>}
impl Backlog {
    pub(super) fn add(&mut self,region:PixelRect) {self.region=self.region.union(region);}
    pub(super) fn region(&self)->PixelRect {
        self.taken.iter().filter(|(submission,_)|submission.discarded()).fold(self.region,|region,(_,taken)|region.union(*taken))
    }
    pub(super) fn take(&mut self,submission:submission::Submission) {
        let region=self.region();
        self.taken.retain(|(submission,_)|submission.pending());
        if !region.is_empty() {self.taken.push((submission,region));}
        self.region=PixelRect::EMPTY;
    }
}
struct Window {image:Image,_charge:crate::residency::Charge,newest:Option<CommittedImage>,good:Option<CommittedImage>,changed:PixelRect,taken:u64}
fn new_take()->u64 {static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)}
pub(super) struct CommittedImage {
    image: Image,
    submission: submission::Submission,
    pages: Vec<Held>,
    versions:Vec<Recorded>,
    current:Option<Current>,
    authored:Option<layer_core::raster::ContentStamp>,
    complete:bool,
    whole:Option<layer_core::BlendSpace>,
    keyed:bool,
}
#[derive(Clone)]
struct Current {frame:Arc<SignatureFrame>,derived:bool}
pub(super) struct WindowSource {image:Image,versions:Vec<Recorded>,current:Option<Current>,authored:Option<layer_core::raster::ContentStamp>}
impl WindowSource {
    pub(super) fn level(&self)->u32 {self.image.plan.level}
    pub(super) fn current(&self)->bool {self.current.is_some()}
}
impl CommittedImage {
    pub(super) fn authored(&self)->Option<layer_core::raster::ContentStamp> {self.authored}
    pub fn image(&self)->&Image {&self.image}
    pub fn view(&self)->&wgpu::TextureView {&self.image.view}
    pub(super) fn whole(&self)->Option<layer_core::BlendSpace> {self.whole}
    pub(super) fn complete(&self)->bool {self.complete}
    pub(super) fn submission(&self)->&submission::Submission {&self.submission}
    fn current_at(&self,frame:&Arc<SignatureFrame>,key:&Key)->Option<bool> {
        let current=self.current.as_ref()?;
        if Arc::ptr_eq(&current.frame,frame) {(self.authored==frame.authored_revision(&key.expression).ok()).then_some(true)}
        else {current.derived.then_some(false)}
    }
}
impl Window {
    fn new(image:Image,charge:crate::residency::Charge)->Box<Self> {Box::new(Self {image,_charge:charge,newest:None,good:None,changed:PixelRect::EMPTY,taken:new_take()})}
    fn committed(&self)->Option<&CommittedImage> {self.newest.iter().chain(&self.good).find(|image|!image.submission.discarded())}
    fn committed_mut(&mut self)->Option<&mut CommittedImage> {self.newest.iter_mut().chain(&mut self.good).find(|image|!image.submission.discarded())}
    fn commit(&mut self,submission:submission::Submission,pages:Vec<Held>,versions:Vec<Recorded>,current:Option<Current>,authored:Option<layer_core::raster::ContentStamp>) {
        if let Some(replaced)=self.newest.take().filter(|newest|!newest.submission.discarded() && !newest.submission.same(&submission)) {self.good=Some(replaced);}
        self.newest=Some(CommittedImage {image:self.image.clone(),submission,pages,versions,current,authored,complete:false,whole:None,keyed:false});
    }
    fn take_changes(&mut self,since:Option<u64>)->(u64,PixelRect) {
        let (plan,changed)=(self.image.plan,std::mem::replace(&mut self.changed,PixelRect::EMPTY));
        let known=since==Some(std::mem::replace(&mut self.taken,new_take()));
        let region=if known {PixelRect::new(changed.min_x()<<plan.level,changed.min_y()<<plan.level,changed.max_x()<<plan.level,changed.max_y()<<plan.level)
            .translated([plan.bounds.min_x(),plan.bounds.min_y()]).intersect(plan.bounds)}else {plan.bounds};
        (self.taken,region)
    }
}
pub(crate) struct PresentedPage<'a> {
    pub(super) holes: bool,
    pub(super) detail: Option<[&'a CommittedImage;2]>,
    pub(super) stand_in: Option<&'a CommittedImage>,
    pub(super) geometry: &'a wgpu::Buffer,
}
impl<'a> PresentedPage<'a> {
    pub(super) fn floor(floor:&'a CommittedImage,geometry:&'a wgpu::Buffer)->Self {Self {holes:true,detail:None,stand_in:Some(floor),geometry}}
    pub(super) fn with_floor(mut self,floor:&'a CommittedImage)->Self {
        if self.holes && self.stand_in.is_none_or(|image|floor.image.plan.level<image.image.plan.level && image.authored.is_some() && image.authored==floor.authored) {self.stand_in=Some(floor);}
        self
    }
    pub(super) fn images(&self)->impl Iterator<Item=&'a CommittedImage> {self.detail.into_iter().flatten().chain(self.stand_in)}
    pub fn views(&self,empty:&'a wgpu::TextureView)->[&'a wgpu::TextureView;3] {
        let [root,next]=self.detail.map_or([empty;2],|images|images.map(CommittedImage::view));
        [root,self.stand_in.map_or(root,CommittedImage::view),next]
    }
    pub fn stand_in_scale(&self)->f32 {self.stand_in.map_or(0.,|image|(1u32<<image.image.plan.level) as f32)}
    pub fn stand_in_perceptual(&self)->bool {self.stand_in.and_then(CommittedImage::whole)==Some(layer_core::BlendSpace::Perceptual)}
    pub fn geometry(&self)->&'a wgpu::Buffer {self.geometry}
    #[cfg(test)]
    pub(crate) fn storm_coverage(&self,view:layer_render::ViewState,stand_in_drawn:bool)->crate::test_support::StormCoverage {
        let extent=self.images().next().map_or([0;2],|image|image.image.plan.extent);
        let total=crate::test_support::storm_surface_area(PixelRect::full(extent),view);
        let unknown=crate::test_support::StormCoverage {root:0.,presented:0.,total,known:false};
        let root=match self.detail {
            None=>0.,
            Some([root,_])=>{
                let Some(published)=crate::test_support::published_coverage(crate::test_support::coverage_hash(self.geometry)) else {return unknown;};
                if published.receipt!=root.submission.observation_id() || published.pages!=root.pages.iter().map(|page|Arc::as_ptr(page.image()) as usize).collect::<Vec<_>>() {return unknown;}
                coordinates(published.plan.bounds,published.plan.level).enumerate()
                    .filter(|(index,_)|published.written.get(index/32).is_some_and(|word|word&(1<<(index%32))!=0))
                    .map(|(_,coordinate)|crate::test_support::storm_surface_area(bounds(coordinate,published.plan.level,published.plan.extent).intersect(published.plan.bounds),view)).sum()
            }
        };
        let presented=match self.stand_in.filter(|_|self.holes && stand_in_drawn) {
            None=>root,
            Some(stand_in) if stand_in.whole.is_some() && !stand_in.submission.discarded()=>total,
            Some(_)=>return unknown,
        };
        crate::test_support::StormCoverage {root,presented,total,known:true}
    }
}
impl Pixels {
    pub fn root(&self) -> Option<&Image> { self.root.as_ref().map(|window|&window.image) }
    pub fn next(&self) -> Option<&Image> { self.next.as_ref().map(|window|&window.image) }
    pub fn committed_root(&self)->Option<&CommittedImage> {self.root.as_ref()?.committed()}
    pub fn committed_next(&self)->Option<&CommittedImage> {self.next.as_ref()?.committed()}
    fn commit_root(&mut self,submission:submission::Submission,pages:Vec<Held>,versions:Vec<Recorded>,current:Option<Current>,authored:Option<layer_core::raster::ContentStamp>) {self.root.as_mut().unwrap().commit(submission,pages,versions,current,authored);}
    pub fn commit_next(&mut self,submission:submission::Submission,reduced:PixelRect) {
        let covered=self.committed_next().is_some_and(|next|next.whole.is_some());
        let root=self.committed_root().unwrap();let pages=root.pages.clone();let versions=root.versions.clone();let current=root.current.clone();let authored=root.authored;let whole=root.whole;
        let next=self.next.as_mut().unwrap();
        let whole=whole.filter(|_|covered || reduced==PixelRect::new(0,0,next.image.plan.size[0],next.image.plan.size[1]));
        self.unreduced.take(submission.clone());
        next.commit(submission,pages,versions,current,authored);next.changed=next.changed.union(reduced);
        if let Some(image)=&mut next.newest {image.complete=whole.is_some();image.whole=whole;}
    }
    fn carry_to_next(&mut self,submission:submission::Submission) {
        if self.unreduced.region().is_empty() && self.committed_next().is_some() {self.commit_next(submission,PixelRect::EMPTY);}
    }
    pub(super) fn restamp(&mut self,frame:&SignatureFrame,authored:layer_core::raster::ContentStamp)->bool {
        let reduced=self.unreduced.region().is_empty();
        let Some(root)=self.root.as_deref_mut().and_then(Window::committed_mut)
            .filter(|root|root.keyed && root.versions.iter().all(|recorded|authored_matches(std::slice::from_ref(recorded),frame,authored,recorded.written))) else {return false;};
        root.authored=Some(authored);
        if reduced && let Some(next)=self.next.as_deref_mut().and_then(Window::committed_mut) {next.authored=Some(authored);}
        true
    }
    pub(super) fn commit_whole(&mut self,submission:submission::Submission,authored:Option<layer_core::raster::ContentStamp>,blend:layer_core::BlendSpace) {
        self.commit_root(submission,Vec::new(),Vec::new(),None,authored);self.certify_root(true,Some(blend),false);
    }
    fn certify_root(&mut self,complete:bool,whole:Option<layer_core::BlendSpace>,keyed:bool) {
        if let Some(image)=self.root.as_mut().and_then(|window|window.newest.as_mut()) {image.complete=complete;image.whole=whole;image.keyed=keyed;}
    }
    pub(super) fn wrote(&mut self,region:PixelRect) {
        self.unreduced.add(region);
        if let Some(root)=&mut self.root {root.changed=root.changed.union(region);}
    }
    pub(super) fn take_changes(&mut self,source:&WindowSource,since:Option<u64>)->(u64,PixelRect) {
        self.root.iter_mut().chain(&mut self.next).find(|window|window.image.texture==source.image.texture)
            .map_or_else(||(new_take(),source.image.plan.bounds),|window|window.take_changes(since))
    }
    pub(super) fn hold_current(&mut self,frame:&Arc<SignatureFrame>) {
        if let Some(image)=self.root.as_deref_mut().and_then(Window::committed_mut) {image.current=Some(Current {frame:frame.clone(),derived:false});}
    }
    pub(super) fn derived(&mut self,region:PixelRect,submission:submission::Submission) {
        self.wrote(region);
        if self.committed_root().is_none() {self.commit_root(submission,Vec::new(),Vec::new(),None,None);}
    }
    pub(super) fn unreduced(&self)->PixelRect {
        let unreduced=self.unreduced.region();
        if unreduced.is_empty() || self.committed_next().is_some() {return unreduced;}
        self.root().map_or(unreduced,|root|PixelRect::new(0,0,root.plan.size[0],root.plan.size[1]))
    }
    pub fn ensure_root(&mut self, r: &WgpuRasterizer, plan: display_mips::Plan, format:wgpu::TextureFormat, _label: &'static str, charge: crate::residency::Charge) {
        self.root.get_or_insert_with(|| Window::new(Image::with_format(r,plan,format),charge));
    }
    pub fn ensure(&mut self, r: &WgpuRasterizer, plan: display_mips::Plan, format:wgpu::TextureFormat, charge: crate::residency::Charge) {
        self.next.get_or_insert_with(|| Window::new(Image::with_format(r,
            display_mips::Plan::window(plan.extent, plan.level + 1, plan.bounds),format),charge));
    }
    pub fn storage_bytes(&self) -> u64 { self.root.iter().chain(&self.next).map(|window|window.image.bytes()).sum() }
    pub(super) fn release_next(&mut self)->bool {
        let (Some(root),Some(_))=(self.root().map(|root|root.plan.size),self.next.take()) else {return false;};
        self.unreduced.add(PixelRect::new(0,0,root[0],root[1]));
        true
    }
    pub(super) fn take(&mut self)->Vec<Image> {
        self.root.take().into_iter().chain(self.next.take()).map(|window|window.image).collect()
    }
}

#[cfg(test)]
pub(crate) fn layer_pages(r:&WgpuRasterizer)->BTreeSet<SourceTarget> {
    r.scene.as_ref().map_or_else(BTreeSet::new,|scene|scene.scale_pages.lock().unwrap().pages.keys().filter_map(|key|key.source).collect())
}
#[cfg(test)]
pub(crate) fn composite_pages_at(r:&WgpuRasterizer,level:u32)->usize {
    r.scene.as_ref().map_or(0,|scene|scene.scale_pages.lock().unwrap().pages.keys().filter(|key|key.source.is_none() && key.level==level).count())
}
impl Cache {
    #[cfg(test)]
    pub(crate) fn assert_pending_capacity(&self,r:&WgpuRasterizer) {
        let optional=self.pages.lock().unwrap().pages.values().filter(|page|page.optional).map(|page|page.image.bytes()).sum::<u64>();
        assert!(optional==0 || r.scene_tiers.display.headroom(Class::Visible)>0,
            "optional branch pages cannot consume the nearest pending unit's display allowance: optional={optional}");
    }
    #[cfg(test)]
    pub(crate) fn assert_native_effect_scope(&self,scene:SceneView<'_>) {
        let root=self.graph.root.as_ref().unwrap();
        if self.plan.level>0 {
            assert!(self.evaluation!=Evaluation::Native,"native capture belongs to the effect node, not the reduced root");
            assert!(cell_plan::Footprint::captures(root,scene,self.plan.level,true));
            let scope=cell_plan::Footprint::of(root,scene,self.plan.bounds,self.plan.extent,self.plan.level,true);
            assert!(!scope.native.is_empty(),"the effect requests its input on the native grid");
        } else {
            assert_eq!(self.required_input_level(),0,"the effect's output grid is already native");
        }
    }
    #[cfg(test)]
    pub(crate) fn cached_branches(&self) -> usize {
        self.pages.lock().unwrap().retained_pages().into_iter()
            .filter(|page|page.key.source.is_none() && !page.written.is_empty()
                && Some(&page.key.expression)!=self.graph.root.as_ref() && Some(&page.key.expression)!=self.graph.content.as_ref()
                && self.graph.caches(&page.key.expression))
            .map(|page|page.key.expression).collect::<std::collections::HashSet<_>>().len()
    }
    #[cfg(test)]
    pub(crate) fn has_cell_halves(&self,region:PixelRect)->bool {
        let cells=self.half_cells(region);
        !cells.is_empty() && cells.len()==coordinates(region.intersect(self.plan.bounds),self.plan.level).count()
    }
    #[cfg(test)]
    pub(crate) fn half_pages(&self)->Vec<(u32,PixelRect)> {
        let halves=self.pivot_halves();
        self.pages.lock().unwrap().pages.keys().filter(|key|key.source.is_none() && halves.contains(&key.expression)).map(|key|(key.level,key.plan().bounds)).collect()
    }
    #[cfg(test)]
    pub(in crate::scene) fn commit_state(&self)->Vec<(u64,[PageSet;3])> {
        let mut state=self.pages.lock().unwrap().retained_pages().into_iter()
            .filter(|page|page.key.source.is_none() && !page.written.is_empty())
            .map(|page|(crate::test_support::coverage_hash(&page.key),[page.written,page.current,page.exact])).collect::<Vec<_>>();
        state.sort_by_key(|(key,_)|*key);state
    }

    #[cfg(test)]
    pub(crate) fn fed_cell_current_or_owed(&self,region:PixelRect)->bool {
        let Some((store,cells,_))=self.cells() else {return false;};
        let Some(frame)=store.expected() else {return false;};
        let area=region.intersect(self.plan.bounds);
        let reached=Cells::reached(self.plan,area);
        let [index]=reached[..] else {return false;};
        let cell=&cells.cells[index];
        if cell.derived {return false;}
        match &cell.shown {
            Some(shown) if shown.submission.discarded()=>false,
            Some(shown) if shown.exact && !cell.queued=>cell.current && shown.recorded.as_ref().is_some_and(|recorded| {
                recorded.key.level==self.plan.level && recorded.written.intersect(area)==area
                    && recorded.matches(frame,self.required_input_level())==Some(true)
            }),
            Some(shown)=>cell.pending() && (!shown.exact || !cell.current),
            None=>cell.queued && cell.pending(),
        }
    }
    #[cfg(test)]
    pub(crate) fn exact_cells(&self)->Vec<(PixelRect,bool)> {
        let Some((_store,cells,_))=self.cells() else {return Vec::new();};
        cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level))
            .map(|(cell,coordinate)|(bounds(coordinate,self.plan.level,self.plan.extent),!cell.queued && cell.shown.as_ref().is_some_and(|shown|shown.exact))).collect()
    }
    #[cfg(test)]
    pub(crate) fn shown_cells(&self)->Vec<([u32;2],Option<bool>)> {
        let Some((_store,cells,_))=self.cells() else {return Vec::new();};
        cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level)).map(|(cell,coordinate)| {
            let owed=!cell.derived && (cell.queued || !cell.current) || !cell.written();
            (coordinate,(!owed).then(||cell.derived || cell.shown.as_ref().is_some_and(|shown|shown.exact)))
        }).collect()
    }
    #[cfg(test)]
    pub(crate) fn mislabelled_cells(&self)->Vec<[u32;2]> {
        let Some((store,cells,_))=self.cells() else {return Vec::new();};
        let Some(frame)=store.expected().cloned() else {return Vec::new();};
        let level=self.required_input_level();
        cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level))
            .filter(|(cell,_)|!cell.derived && cell.current && !cell.queued
                && cell.shown.as_ref().and_then(|shown|shown.recorded.as_ref()).is_some_and(|recorded|recorded.matches(&frame,level)==Some(false)))
            .map(|(_,coordinate)|coordinate).collect()
    }
    #[cfg(test)]
    pub(crate) fn shows_reduced(&self,region:PixelRect)->bool {
        let Some((_store,cells,_))=self.cells() else {return false;};
        let reached=Cells::reached(self.plan,region);
        !reached.is_empty() && reached.into_iter().all(|index|cells.cells[index].shown.as_ref().is_some_and(|shown|!shown.exact))
    }
    #[cfg(test)]
    pub(crate) fn shows_current_reduced_grid(&self,level:u32,region:PixelRect)->bool {
        if level==0 || self.plan.level!=level || self.pixels.committed_root().is_none() {return false;}
        let Some((store,cells,_))=self.cells() else {return false;};
        let Some(frame)=store.expected() else {return false;};
        let reached=Cells::reached(self.plan,region);
        !reached.is_empty() && reached.into_iter().all(|index| {
            let cell=&cells.cells[index];
            cell.current && cell.written() && cell.shown.as_ref().is_some_and(|shown| {
                !shown.submission.discarded() && match &shown.recorded {
                    Some(recorded)=>recorded.key.level==level && recorded.matches(frame,self.required_input_level())!=Some(false),
                    None=>self.gesture.is_some() && (!shown.exact || self.contact_proves(shown.witness.as_ref())),
                }
            })
        })
    }
    #[cfg(test)]
    pub(crate) fn storm_view_settled(&self,view:layer_render::ViewState)->Result<(),String> {
        let visible=display_mips::view_bounds(view,self.plan.extent,0).unwrap_or(PixelRect::EMPTY).intersect(self.plan.bounds);
        let Some((_store,cells,_))=self.cells() else {return Err(String::from("the view has no cells"));};
        for coordinate in coordinates(visible,self.plan.level) {
            let cell=&cells.cells[Cells::index(self.plan,coordinate)];
            let exact=self.presented!=Accuracy::Exact || cell.shown.as_ref().is_some_and(|shown|shown.exact);
            if !cell.derived && !(cell.current && cell.shown.is_some() && exact) {
                return Err(format!("cell {coordinate:?} current={} shown={} exact={exact} queued={} refusal={:?}",cell.current,cell.shown.is_some(),cell.queued,cell.refusal));
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn view_cells(&self)->u64 {coordinates(self.plan.bounds,self.plan.level).count() as u64}
    #[cfg(test)]
    pub(crate) fn presentable_pages(&self) -> usize {
        usize::from(self.pixels.committed_root().is_some())*self.cells().map_or(0,|(_,cells,_)|cells.cells.iter().filter(|cell|cell.written()).count())
            +self.overview.as_ref().map_or(0,|cache|cache.presentable_pages())
    }
    #[cfg(test)]
    pub(crate) fn prepared_pages(&self) -> usize {
        self.graph.prepared.iter().flatten().map(|half| coordinates(self.plan.bounds,self.plan.level).filter(|coordinate|self.branch_ready(half,bounds(*coordinate,self.plan.level,self.plan.extent),Class::Prefetch)).count()).sum()
    }
    pub(crate) fn resident_bytes(&self) -> u64 { self.pixels.storage_bytes() }
    pub(crate) fn working_bytes(&self) -> u64 { self.storage_bytes().saturating_sub(self.resident_bytes()) }
    pub(super) fn output_plan(&self) -> display_mips::Plan { self.pixels.root().map_or(self.plan, |i| i.plan) }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Key {
    pub source: Option<SourceTarget>,
    pub expression: graph::Node,
    pub extent: [u32; 2],
    pub color: layer_core::color::DocumentColor,
    pub blend_space: layer_core::BlendSpace,
    pub level: u32,
    pub coordinate: [u32; 2],
    pub format: wgpu::TextureFormat,
}
impl Key {
    pub fn plan(&self) -> display_mips::Plan {display_mips::Plan::window(self.extent,self.level,bounds(self.coordinate,self.level,self.extent))}
    pub(super) fn bytes(&self)->u64 {self.plan().size.map(u64::from).into_iter().product::<u64>()*u64::from(self.format.block_copy_size(None).unwrap())}
    fn finer(&self)->impl Iterator<Item=Self>+'_ {children(self.coordinate,self.level,self.extent).map(|coordinate|Self {coordinate,level:self.level-1,..self.clone()})}

}

#[derive(Clone)]
pub(super) struct Interest {
    pub key:Key,
    evaluation:Evaluation,
    region:PixelRect,
    input_level:u32,
}
impl Interest {
    pub(super) fn new(key:Key,evaluation:Evaluation,region:PixelRect,input_level:u32)->Self {Self {key,evaluation,region,input_level}}
    #[cfg(test)]
    pub(super) fn input_level(&self)->u32 {self.input_level}
    fn contains(&self,accepted:&Self)->bool {
        let wanted=&self.key;let key=&accepted.key;
        key.source==wanted.source && key.expression==wanted.expression && key.extent==wanted.extent && key.color==wanted.color
            && key.blend_space==wanted.blend_space && key.level==wanted.level && key.format==wanted.format
            && accepted.input_level==self.input_level && (key.source.is_some() || accepted.evaluation==self.evaluation)
            && if key.source.is_some() {accepted.region.intersect(self.region)==accepted.region}else {!accepted.region.intersect(self.region).is_empty()}
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::scene) enum Accuracy { Display, Exact }

pub(super) fn coordinates(bounds:PixelRect,level:u32)->impl DoubleEndedIterator<Item=[u32;2]>+Clone {
    let side=1<<level;
    page_coordinates(PixelRect::new(bounds.min_x()/side,bounds.min_y()/side,bounds.max_x().div_ceil(side),bounds.max_y().div_ceil(side)))
}
pub(super) fn bounds(coordinate:[u32;2],level:u32,extent:[u32;2])->PixelRect {
    let span=PAGE_SIZE<<level;let [x,y]=coordinate.map(|value|value*span);
    PixelRect::new(x,y,x.saturating_add(span).min(extent[0]),y.saturating_add(span).min(extent[1]))
}
fn children(coordinate:[u32;2],level:u32,extent:[u32;2])->impl Iterator<Item=[u32;2]> {
    (0..2).flat_map(move|y|(0..2).map(move|x|[coordinate[0]*2+x,coordinate[1]*2+y]))
        .filter(move|coordinate|coordinate.into_iter().zip(extent).all(|(coordinate,extent)|u64::from(*coordinate)*u64::from(PAGE_SIZE<<(level-1))<u64::from(extent)))
}
#[derive(Clone,Default,Debug,PartialEq,Eq)]
pub(in crate::scene) struct PageSet {
    level:u32,
    entries:std::collections::HashSet<[u32;2]>,
}
impl std::ops::Deref for PageSet {
    type Target=std::collections::HashSet<[u32;2]>;
    fn deref(&self)->&Self::Target {&self.entries}
}
impl PageSet {
    fn new(level:u32)->Self {Self {level,entries:Default::default()}}
    #[cfg(test)]
    pub(super) fn regions(&self,area:PixelRect)->Vec<PixelRect> {
        let side=1<<self.level;
        let texels=PixelRect::new(area.min_x()/side,area.min_y()/side,area.max_x().div_ceil(side),area.max_y().div_ceil(side));
        page_regions(self.entries.iter().copied(),texels).into_iter().map(|region|PixelRect::new(
            region.min_x()*side,region.min_y()*side,region.max_x()*side,region.max_y()*side).intersect(area)).collect()
    }
}
#[derive(Clone,Default)]
pub(in crate::scene) struct Coverage {
    pub written:PageSet,
    pub matching:PageSet,
    pub exact:PageSet,
    pending:PageSet,
}
impl Coverage {
    pub(super) fn new(level:u32)->Self {Self {written:PageSet::new(level),matching:PageSet::new(level),exact:PageSet::new(level),pending:PageSet::new(level)}}
    pub fn covers(pages:&PageSet,area:PixelRect)->bool {coordinates(area,pages.level).all(|coordinate|pages.contains(&coordinate))}
    #[cfg(test)]
    pub fn ready(&self,area:PixelRect,accuracy:Accuracy)->bool {
        coordinates(area,self.matching.level).all(|coordinate|self.matching.contains(&coordinate) && (accuracy==Accuracy::Display || self.exact.contains(&coordinate)))
    }
    fn include(&mut self,coordinate:[u32;2],matches:Option<bool>,exact:bool) {
        match matches {
            Some(true)=>{self.matching.entries.insert(coordinate);if exact {self.exact.entries.insert(coordinate);}},
            None=>{self.pending.entries.insert(coordinate);},Some(false)=>{}
        }
    }
    pub fn needs(&self,area:PixelRect,accuracy:Accuracy)->bool {
        coordinates(area,self.matching.level).any(|coordinate|!self.pending.contains(&coordinate)
            && (!self.matching.contains(&coordinate) || accuracy==Accuracy::Exact && !self.exact.contains(&coordinate)))
    }
}
fn materialize(frame:&SignatureFrame,key:&Key,region:PixelRect,input_level:u32)->Result<Recorded,GpuRasterError> {
    let version=frame.expected_key(key,region,input_level)?.into_version();let authored=frame.authored_revision(&key.expression)?;
    Ok(Recorded {key:key.clone(),region,written:region,input_level,version,authored})
}
#[derive(Clone)]
struct Recorded {
    key:Key,
    region:PixelRect,
    written:PixelRect,
    input_level:u32,
    version:ContentVersion,
    authored:layer_core::raster::ContentStamp,
}
impl Recorded {
    fn clipped(&self,area:PixelRect)->Option<Self> {
        let written=self.written.intersect(area);
        (!written.is_empty()).then(||Self {written,..self.clone()})
    }
    fn matches(&self,frame:&SignatureFrame,required_input_level:u32)->Option<bool> {self.matches_as(frame,&self.key.expression,required_input_level)}
    fn matches_as(&self,frame:&SignatureFrame,expression:&graph::Node,required_input_level:u32)->Option<bool> {
        if self.input_level>required_input_level {return Some(false);}
        let key=Key {expression:expression.clone(),..self.key.clone()};
        let expected=match frame.expected_key(&key,self.region,self.input_level) {Ok(expected)=>expected,Err(_)=>return Some(false)};
        if self.version.authored()!=expected.version().authored() {return Some(false);}
        frame.matches(&self.version,&expected)
    }
}
fn recorded_matches(records:&[Recorded],frame:&SignatureFrame,area:PixelRect,required_input_level:u32)->Option<bool> {
    let mut missing=vec![area];let mut pending=false;
    for recorded in records {
        if recorded.written.intersect(area).is_empty() {continue;}
        match recorded.matches(frame,required_input_level) {Some(false)=>return Some(false),None=>pending=true,Some(true)=>{}}
        missing=missing.into_iter().flat_map(|part|part.subtract(recorded.written)).filter(|part|!part.is_empty()).collect();
    }
    if !missing.is_empty() {Some(false)}else {(!pending).then_some(true)}
}
fn authored_matches(records:&[Recorded],frame:&SignatureFrame,authored:layer_core::raster::ContentStamp,area:PixelRect)->bool {
    let mut missing=vec![area];
    for recorded in records {
        if recorded.written.intersect(area).is_empty() {continue;}
        if recorded.authored!=authored {
            if frame.authored_revision(&recorded.key.expression).ok()!=Some(authored) {return false;}
            let Ok(expected)=frame.expected_key(&recorded.key,recorded.region,recorded.input_level) else {return false;};
            if recorded.version.authored()!=expected.version().authored() {return false;}
        }
        missing=missing.into_iter().flat_map(|part|part.subtract(recorded.written)).filter(|part|!part.is_empty()).collect();
    }
    missing.is_empty()
}
fn publication_matches(frame:&SignatureFrame,key:&Key,area:PixelRect,input_level:u32,held:Option<(layer_core::raster::ContentStamp,&[Recorded])>)->Option<bool> {
    let Ok(authored)=frame.authored_revision(&key.expression) else {return Some(false);};
    match held {
        Some((previous,records)) if previous==authored=> {
            if records.iter().any(|record|record.key.level>key.level && !record.written.intersect(area).is_empty()) {return Some(false);}
            recorded_matches(records,frame,area,input_level)
        },
        _=>Some(false),
    }
}
#[derive(Clone)]
pub(super) struct Accepted {
    frame:Arc<SignatureFrame>,
    recorded:Recorded,
}
impl Accepted {
    pub(super) fn key(&self)->&Key {&self.recorded.key}
    pub(super) fn region(&self)->PixelRect {self.recorded.region}
    pub(super) fn input_level(&self)->u32 {self.recorded.input_level}
    pub(super) fn input_unit(&self)->crate::scene::sources::InputUnit {
        crate::scene::sources::InputUnit::new(self.region(),self.input_level())
    }
}
#[derive(Clone)]
struct WrittenCoverage {
    written:PageSet,
    exact:PageSet,
    submissions:std::collections::HashMap<[u32;2],submission::Submission>,
}
impl WrittenCoverage {
    fn new(level:u32)->Self {Self {written:PageSet::new(level),exact:PageSet::new(level),submissions:Default::default()}}
    fn commit(&mut self,area:PixelRect,accuracy:Accuracy,submission:&submission::Submission) {
        for coordinate in coordinates(area,self.written.level) {
            self.written.entries.insert(coordinate);self.exact.entries.remove(&coordinate);
            if accuracy==Accuracy::Exact {self.exact.entries.insert(coordinate);}
            self.submissions.insert(coordinate,submission.clone());
        }
    }
    fn valid(&self,coordinate:&[u32;2])->bool {self.written.contains(coordinate) && self.submissions.get(coordinate).is_none_or(|submission|!submission.discarded())}
    fn valid_written(&self)->impl Iterator<Item=&[u32;2]> {self.written.iter().filter(|coordinate|self.valid(coordinate))}
    fn covers(&self,area:PixelRect)->bool {coordinates(area,self.written.level).all(|coordinate|self.valid(&coordinate))}
}
#[derive(Clone)]
pub(super) struct Held {image:Arc<Image>,pin:Arc<crate::residency::RetentionPin>,written_at:u64}
impl std::ops::Deref for Held {type Target=Image;fn deref(&self)->&Image {&self.image}}
impl Held {
    #[cfg(test)]
    pub(super) fn image(&self)->&Arc<Image> {&self.image}
    pub(super) fn track(self,encoder:&submission::CommandEncoder) {encoder.hold(self.pin);}
    pub(super) fn stamp(&self)->(usize,u64) {(Arc::as_ptr(&self.image) as usize,self.written_at)}
}
struct Page {
    image: Arc<Image>,
    pin: std::sync::Weak<crate::residency::RetentionPin>,
    optional: bool,
    coverage: WrittenCoverage,
    versions:std::collections::HashMap<[u32;2],Recorded>,
    used: u64,
    value: u64,
    priority: u64,
    retention: crate::residency::Retained,
    _charge: crate::residency::Charge,
    window: WindowId,
    demand: crate::residency::Demand,
    id: u64,
    written_at: u64,
}
impl Page {
    fn pin(&mut self,tier:&Arc<crate::residency::Tier>)->Arc<crate::residency::RetentionPin> {
        self.pin.upgrade().unwrap_or_else(|| {let pin=Arc::new(self.retention.pin(tier.charge(self.image.bytes())));self.pin=Arc::downgrade(&pin);pin})
    }
    fn set_optional(&mut self,optional:bool) {self.optional=optional;self.retention.set_optional(optional);}
    fn hold(&mut self,tier:&Arc<crate::residency::Tier>)->Held {Held {image:self.image.clone(),pin:self.pin(tier),written_at:self.written_at}}
    fn pinned(&self)->bool {self.pin.strong_count()!=0}
    fn unwanted(&self)->bool {self.coverage.written.is_empty() && !self.pinned()}
    fn matches(&self,frame:Option<&SignatureFrame>,area:PixelRect,required_input_level:u32)->Option<bool> {
        if area.intersect(self.image.plan.bounds)!=area || !self.coverage.covers(area) {return Some(false);}
        let Some(frame)=frame else {return Some(false);};let mut pending=false;
        for coordinate in coordinates(area,self.coverage.written.level) {
            let cell=bounds(coordinate,self.coverage.written.level,self.image.plan.extent).intersect(area);
            let Some(recorded)=self.versions.get(&coordinate).filter(|recorded|recorded.written.intersect(cell)==cell) else {return Some(false);};
            match recorded.matches(frame,required_input_level) {Some(false)=>return Some(false),None=>pending=true,Some(true)=>{}}
        }
        (!pending).then_some(true)
    }
}
struct Inputs {
    region:PixelRect,
    evaluation:Evaluation,
    sources:crate::scene::sources::AcceptedInputs,
    pages:Vec<(Key,PixelRect,Held)>,
    accepted:Accepted,
    pin:Option<Arc<crate::residency::RetentionPin>>,
    stream:super::sources::Stream,
}

impl Inputs {
    fn at(&self,region:PixelRect,input_level:u32)->bool {self.region==region && self.accepted.recorded.input_level==input_level}
    fn interest(&self)->Interest {Interest::new(self.accepted.recorded.key.clone(),self.evaluation,self.region,self.accepted.recorded.input_level)}
}

#[derive(Default)]
struct Entries {keys:Vec<Key>}
type Columns = BTreeMap<u32,BTreeMap<u32,Entries>>;
#[derive(Default)]
struct Index {
    owners:std::collections::HashMap<(Option<SourceTarget>,[u32;2]),BTreeMap<u32,Columns>>,
    allocated:u64,
}
fn tree_entry_bytes<T>()->u64 {(std::mem::size_of::<T>()+3*std::mem::size_of::<usize>()) as u64}
impl Index {
    fn insert(&mut self,key:&Key) {
        let allocated=&mut self.allocated;
        let levels=self.owners.entry((key.source,key.extent)).or_default();
        let columns=levels.entry(key.level).or_insert_with(|| {*allocated+=tree_entry_bytes::<(u32,Columns)>();Default::default()});
        let rows=columns.entry(key.coordinate[0]).or_insert_with(|| {*allocated+=tree_entry_bytes::<(u32,BTreeMap<u32,Entries>)>();Default::default()});
        let entries=rows.entry(key.coordinate[1]).or_insert_with(|| {*allocated+=tree_entry_bytes::<(u32,Entries)>();Entries::default()});
        debug_assert!(!entries.keys.contains(key));
        let before=entries.keys.capacity();
        entries.keys.push(key.clone());
        *allocated+=(entries.keys.capacity()-before) as u64*std::mem::size_of::<Key>() as u64;
    }
    fn remove(&mut self,key:&Key) {
        let owner=(key.source,key.extent);
        let levels=self.owners.get_mut(&owner).unwrap();
        let columns=levels.get_mut(&key.level).unwrap();
        let rows=columns.get_mut(&key.coordinate[0]).unwrap();
        let entries=rows.get_mut(&key.coordinate[1]).unwrap();
        entries.keys.retain(|candidate|candidate!=key);
        if entries.keys.is_empty() {
            self.allocated-=tree_entry_bytes::<(u32,Entries)>()+entries.keys.capacity() as u64*std::mem::size_of::<Key>() as u64;
            rows.remove(&key.coordinate[1]);
        }
        if rows.is_empty() {self.allocated-=tree_entry_bytes::<(u32,BTreeMap<u32,Entries>)>();columns.remove(&key.coordinate[0]);}
        if columns.is_empty() {self.allocated-=tree_entry_bytes::<(u32,Columns)>();levels.remove(&key.level);}
        if levels.is_empty() {self.owners.remove(&owner);}
    }
    fn overlapping<'a>(&'a self,key:&Key,area:PixelRect,levels:std::ops::RangeInclusive<u32>)->impl Iterator<Item=&'a Key>+'a {
        self.owners.get(&(key.source,key.extent)).into_iter().flat_map(move|owned|owned.range(levels.clone()))
            .flat_map(move|(level,columns)| {
                let span=PAGE_SIZE<<level;
                columns.range(area.min_x()/span..area.max_x().div_ceil(span)).flat_map(move|(_,rows)|
                    rows.range(area.min_y()/span..area.max_y().div_ceil(span)).flat_map(|(_,entries)|&entries.keys))
            }).inspect(|_| {
                #[cfg(test)]
                crate::test_support::PassCounts {store_page_visits:1,..Default::default()}.record();
            })
    }
    fn storage_bytes(&self)->u64 {
        self.allocated+self.owners.capacity() as u64*(std::mem::size_of::<((Option<SourceTarget>,[u32;2]),BTreeMap<u32,Columns>)>()+1) as u64
    }
}
#[cfg(not(target_arch="wasm32"))]
struct Retirement {
    sender:crate::device_poll::RetirementHandle,
    total:Arc<crate::residency::Tier>,
    memory:Option<layer_core::MemoryOwner>,
    retiring:Arc<crate::residency::Tier>,
}
struct Victim {rank:(bool,u64,u64),id:u64}
impl PartialEq for Victim {fn eq(&self,other:&Self)->bool {self.rank==other.rank}}
impl Eq for Victim {}
impl PartialOrd for Victim {fn partial_cmp(&self,other:&Self)->Option<std::cmp::Ordering> {Some(self.cmp(other))}}
impl Ord for Victim {fn cmp(&self,other:&Self)->std::cmp::Ordering {other.rank.cmp(&self.rank)}}
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Hash)]
pub(super) struct WindowId(u64);
impl WindowId {
    pub(super) fn new()->Self {static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);Self(NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed))}
}
#[derive(Default)]
struct Share {bytes:u64,touched:u64,heap:std::collections::BinaryHeap<Victim>}
#[derive(Default)]
struct Victims {windows:std::collections::HashMap<WindowId,Share>,inflation:u64,epoch:u64}
fn rank(page:&Page)->(bool,u64,u64) {(!page.optional,page.priority,page.used)}
fn rebuild_value(work:u64,bytes:u64)->u64 {work.saturating_mul(1<<32)/bytes.max(1)}
fn estimated_work(key:&Key)->u64 {
    let bounds=key.plan().bounds;
    (u64::from(bounds.width().div_ceil(PAGE_SIZE))*u64::from(bounds.height().div_ceil(PAGE_SIZE))).saturating_mul(object_cache::PASS_TAPS)
}
impl Victims {
    fn touch(&mut self,page:&Page) {
        self.windows.entry(page.window).or_default().heap.push(Victim {rank:rank(page),id:page.id});
    }
    fn used(&mut self,clock:&mut u64,page:&mut Page,class:Class) {
        *clock+=1;page.used=*clock;page.priority=self.inflation.saturating_add(page.value);
        page.demand.stamp(self.epoch,class);
        self.windows.entry(page.window).or_default().touched=self.epoch;self.touch(page);
    }
    fn charge(&mut self,window:WindowId,bytes:u64) {let share=self.windows.entry(window).or_default();share.bytes+=bytes;share.touched=self.epoch;}
    fn release(&mut self,window:WindowId,bytes:u64) {
        let share=self.windows.get_mut(&window).unwrap();share.bytes-=bytes;
        if share.bytes==0 {self.windows.remove(&window);}
    }
    fn next(&self,limit:u64)->Option<WindowId> {
        let active=|share:&Share|share.touched.saturating_add(1)>=self.epoch;
        let fair=limit/self.windows.values().filter(|share|active(share)).count().max(1) as u64;
        let class=|share:&Share|share.heap.peek().map(|victim|victim.rank.0);
        let lowest=self.windows.values().filter_map(class).min()?;
        let candidates=||self.windows.iter().filter(|(_,share)|class(share)==Some(lowest));
        let first=|eligible:&dyn Fn(&Share)->bool|candidates().filter(|(_,share)|eligible(share))
            .max_by(|(_,a),(_,b)|a.heap.peek().cmp(&b.heap.peek())).map(|(window,_)|*window);
        first(&|share|!active(share) && share.bytes>0).or_else(||first(&|share|active(share) && share.bytes>fair)).or_else(||first(&|_|true))
    }
    fn len(&self)->usize {self.windows.values().map(|share|share.heap.len()).sum::<usize>()}
    fn compact(&mut self,pages:&crate::IdMap<Key,Page>) {
        if self.len()<=4*pages.len()+64 {return;}
        for share in self.windows.values_mut() {share.heap=Default::default();}
        for page in pages.values() {self.touch(page);}
    }
    fn bytes(&self)->u64 {
        (self.windows.values().map(|share|share.heap.capacity()).sum::<usize>()*std::mem::size_of::<Victim>()
            +self.windows.capacity()*(std::mem::size_of::<(WindowId,Share)>()+1)) as u64
    }
}
enum Change {Page(Key),Keys(u32,DocRect,Option<PixelRect>),Reset}
struct Changes {first:u64,log:std::collections::VecDeque<Change>}
impl Default for Changes {
    fn default()->Self {
        static STORES:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
        Self {first:STORES.fetch_add(1,std::sync::atomic::Ordering::Relaxed)<<32,log:Default::default()}
    }
}
impl Changes {
    const KEPT:usize=1<<12;
    fn push(&mut self,change:Change) {
        if self.log.len()==Self::KEPT {self.log.pop_front();self.first+=1;}
        self.log.push_back(change);
    }
    fn end(&self)->u64 {self.first+self.log.len() as u64}
    fn since(&self,position:u64)->Option<impl Iterator<Item=&Change>> {
        (self.first<=position && position<=self.end()).then(||self.log.range((position-self.first) as usize..))
    }
}
#[derive(Default)]
pub(in crate::scene) struct Store {
    pages: crate::IdMap<Key, Page>,
    inputs: crate::IdMap<Key, Vec<Inputs>>,
    changes: Changes,
    unconfirmed: Vec<(submission::Submission,Key)>,
    frames: u64,
    victims: Victims,
    ids: crate::IdMap<u64,Key>,
    index: Index,
    clock: u64,
    total: Arc<crate::residency::Tier>,
    version_slots: u64,
    submission_slots: u64,
    pinned: Arc<crate::residency::Tier>,
    expected:Option<Arc<SignatureFrame>>,
    previous:Option<Arc<SignatureFrame>>,
    moving:Vec<crate::scene::vectors::Moving>,
    #[cfg(not(target_arch="wasm32"))]
    retirement:Option<Retirement>,
    #[cfg(not(target_arch="wasm32"))]
    search:Option<crate::residency::Hold>,
    retired:Retired,
}
#[cfg(not(target_arch="wasm32"))]
#[derive(Default)]
struct Retired(Option<crate::device_poll::RetirementHandle>,Vec<(Arc<Image>,crate::present_transition::Charge,crate::residency::Charge,Option<crate::residency::Hold>)>);
#[cfg(not(target_arch="wasm32"))]
impl Drop for Retired {
    fn drop(&mut self) {
        let pages=std::mem::take(&mut self.1);
        if let Some(sender)=self.0.take().filter(|_|!pages.is_empty()) {
            pages[0].1.report();
            sender.run(move||for (image,charge,retiring,search) in pages {image.texture.destroy();drop((image,charge,retiring,search));});
        }
    }
}
#[cfg(target_arch="wasm32")]
#[derive(Default)]
struct Retired(Vec<Arc<Image>>);
#[cfg(target_arch="wasm32")]
impl Drop for Retired {
    fn drop(&mut self) {for image in std::mem::take(&mut self.0) {image.texture.destroy();}}
}
#[derive(Clone,Default)]
pub(in crate::scene) struct Pages(Arc<std::sync::Mutex<Store>>);
pub(in crate::scene) struct Locked<'a>(Option<std::sync::MutexGuard<'a,Store>>);
impl Pages {
    pub(in crate::scene) fn new(store:Store)->Self {Self(Arc::new(std::sync::Mutex::new(store)))}
    pub(in crate::scene) fn lock(&self)->std::sync::LockResult<Locked<'_>> {
        self.0.lock().map(|store|Locked(Some(store))).map_err(|poisoned|std::sync::PoisonError::new(Locked(Some(poisoned.into_inner()))))
    }
    pub(in crate::scene) fn same(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}
}
impl std::ops::Deref for Locked<'_> {type Target=Store;fn deref(&self)->&Store {self.0.as_ref().unwrap()}}
impl std::ops::DerefMut for Locked<'_> {fn deref_mut(&mut self)->&mut Store {self.0.as_mut().unwrap()}}
impl Drop for Locked<'_> {
    fn drop(&mut self) {
        let retired=self.0.as_mut().map(|store|std::mem::take(&mut store.retired));
        drop(self.0.take());
        drop(retired);
    }
}
impl Store {
    pub(super) fn search(&mut self,trim:impl FnOnce(&mut Self))->Option<crate::residency::Batch> {
        #[cfg(not(target_arch="wasm32"))]
        {
            let search=crate::residency::Search::open([]);
            self.search=Some(search.hold());trim(self);self.search=None;
            let batch=search.seal();
            (!batch.settled()).then_some(batch)
        }
        #[cfg(target_arch="wasm32")]
        {trim(self);None}
    }
    pub(super) fn within(display:&Arc<crate::residency::Tier>)->Self {Self {total:crate::residency::Tier::within(display),..Default::default()}}
    pub(super) fn tier(&self)->Arc<crate::residency::Tier> {self.total.clone()}
    pub(in crate::scene) fn set_expected(&mut self,frame:Arc<SignatureFrame>) {self.set_expected_within(frame,None);}
    pub(in crate::scene) fn set_expected_within(&mut self,frame:Arc<SignatureFrame>,bound:Option<PixelRect>) {
        if self.expected.as_ref().is_none_or(|previous|!Arc::ptr_eq(previous,&frame)) {
            self.confirm();
            match self.expected.as_ref().and_then(|previous|frame.carry(previous)) {
                Some(regions)=>for (level,region) in regions {self.changes.push(Change::Keys(level,region,bound.filter(|_|level==0)));},
                None=>{self.changes.push(Change::Reset);self.previous=self.expected.take();},
            }
            self.frames+=1;
        }
        self.expected=Some(frame);self.victims.epoch+=1;
    }
    fn touch(&mut self,key:&Key) {self.changes.push(Change::Page(key.clone()));}
    fn confirm(&mut self) {
        let changes=&mut self.changes;
        self.unconfirmed.retain(|(submission,key)| {
            if submission.discarded() {changes.push(Change::Page(key.clone()));}
            submission.pending()
        });
    }
    pub(in crate::scene) fn expected(&self)->Option<&Arc<SignatureFrame>> {self.expected.as_ref()}
    pub(in crate::scene) fn release_moved<'a>(&mut self,live:impl Iterator<Item=&'a crate::scene::vectors::Moving>+Clone) {
        if self.moving.iter().eq(live.clone()) {return;}
        self.moving=live.cloned().collect();
        let dead:Vec<Key>=self.pages.iter().filter(|(key,page)|!page.pinned() && graph::Expression::moves_other_than(&key.expression,&self.moving)).map(|(key,_)|key.clone()).collect();
        for key in dead {let image=self.remove(&key).image;self.retire(image);}
    }
    pub(super) fn superseded(&self,key:&Key,view:&wgpu::TextureView,region:PixelRect,input_level:u32)->Option<(&SignatureFrame,bool)> {
        let (page,previous)=(self.pages.get(key)?,self.previous.as_deref()?);
        (page.image.view==*view && page.matches(Some(previous),region,input_level)==Some(true)).then(||(previous,Coverage::covers(&page.coverage.exact,region)))
    }
    pub(in crate::scene) fn advance_epoch(&mut self) {self.victims.epoch+=1;}
    fn capture(&self,key:&Key,region:PixelRect,input_level:u32)->Result<Accepted,GpuRasterError> {
        let frame=self.expected.as_ref().ok_or(GpuRasterError::Invariant("signature frame is missing or inconsistent"))?.clone();
        Ok(Accepted {recorded:materialize(&frame,key,region,input_level)?,frame})
    }
    pub(super) fn accept_version(&mut self,interest:&Interest)->Result<Option<Accepted>,GpuRasterError> {
        let Interest {key,region,input_level,..}=interest;let (region,input_level)=(*region,*input_level);
        let frame=self.expected.as_ref().ok_or(GpuRasterError::Invariant("signature frame is missing or inconsistent"))?;
        if let Some(inputs)=self.inputs.get_mut(key) {
            inputs.retain(|input|!input.at(region,input_level) || input.accepted.recorded.matches(frame,input_level)!=Some(false));
            if let Some(input)=inputs.iter().find(|input|input.at(region,input_level)) {
                return Ok(input.accepted.recorded.matches(frame,input_level).map(|_|input.accepted.clone()));
            }
        }
        self.capture(key,region,input_level).map(Some)
    }
    pub(super) fn prepare_write(&self,key:Key,region:PixelRect,accuracy:Accuracy,input_level:u32)->Result<Write,GpuRasterError> {
        #[cfg(test)]
        crate::scene::sources::assert_reading("store write");

        let frame=self.expected.as_ref().ok_or(GpuRasterError::Invariant("signature frame is missing or inconsistent"))?;
        let accepted=if let Some(input)=self.inputs.get(&key).and_then(|inputs|inputs.iter().find(|input|input.at(region,input_level) && Arc::ptr_eq(&input.accepted.frame,frame))) {
            input.accepted.clone()
        }else {self.capture(&key,region,input_level)?};
        Ok(Write {key,region,accuracy,accepted:Some(accepted),witness:None})
    }
    pub(super) fn restamped(&self,key:&Key,held:&Held)->Held {
        Held {written_at:self.pages.get(key).map_or(held.written_at,|page|page.written_at),..held.clone()}
    }
    pub(super) fn forget(&mut self,key:&Key,region:PixelRect) {
        if let Some(page)=self.pages.get_mut(key) {for coordinate in coordinates(region,page.coverage.written.level) {page.versions.remove(&coordinate);}}
    }
    pub(super) fn materialize_source(&mut self,key:&Key,region:PixelRect,input_level:u32,frame:&SignatureFrame) {
        let Some(page)=self.pages.get_mut(key).filter(|page|Coverage::covers(&page.coverage.written,region)) else {return;};
        let Some(coordinate)=coordinates(region,page.coverage.written.level).next() else {return;};
        if let Ok(recorded)=materialize(frame,key,region,input_level) {page.versions.insert(coordinate,recorded);}
    }
    pub(super) fn coverage(&self,key:&Key,area:PixelRect,required_input_level:u32)->Coverage {
        let mut coverage=Coverage::new(key.level);
        coverage.written=self.written(key,area);
        self.include_matching(key,area,required_input_level,&mut coverage);
        coverage
    }
    fn present<'a>(&'a self,key:&'a Key,area:PixelRect,levels:std::ops::RangeInclusive<u32>)->impl Iterator<Item=(&'a Key,&'a Page)>+'a {
        self.index.overlapping(key,area,levels).map(|candidate|(candidate,&self.pages[candidate])).filter(move|(candidate,page)|candidate.source==key.source && candidate.extent==key.extent
            && !candidate.plan().bounds.intersect(area).is_empty()
            && (!page.optional || candidate.expression==key.expression))
    }
    fn include_matching(&self,key:&Key,area:PixelRect,required_input_level:u32,coverage:&mut Coverage) {
        for (candidate,page) in self.present(key,area,key.level..=key.level) {
            if candidate.expression!=key.expression || candidate.color!=key.color || candidate.blend_space!=key.blend_space
                || candidate.format!=key.format && candidate.format!=wgpu::TextureFormat::Rgba32Float {continue;}
            for coordinate in coordinates(candidate.plan().bounds.intersect(area),coverage.matching.level) {
                let region=bounds(coordinate,coverage.matching.level,key.extent).intersect(area);
                coverage.include(coordinate,page.matches(self.expected.as_deref(),region,required_input_level),
                    Coverage::covers(&page.coverage.exact,region));
            }
        }
    }
    pub(super) fn written(&self,key:&Key,area:PixelRect)->PageSet {
        let mut written:Vec<_>=(0..=display_mips::MAX_LEVEL).map(PageSet::new).collect();
        for (candidate,page) in self.present(key,area,0..=display_mips::MAX_LEVEL) {
            written[candidate.level as usize].entries.extend(page.coverage.valid_written().copied());
        }
        for level in 1..=key.level {
            let parents=written[level as usize-1].iter().map(|coordinate|coordinate.map(|value|value/2)).collect::<std::collections::HashSet<_>>();
            for coordinate in parents {
                if children(coordinate,level,key.extent).all(|child|written[level as usize-1].contains(&child)) {written[level as usize].entries.insert(coordinate);}
            }
        }
        let mut covered=PageSet::new(key.level);
        for coordinate in coordinates(area,key.level) {
            if (key.level..=display_mips::MAX_LEVEL).any(|level|written[level as usize].contains(&coordinate.map(|value|value>>(level-key.level)))) {
                covered.entries.insert(coordinate);
            }
        }
        covered
    }
    pub(super) fn evaluated_from(&self,view:&wgpu::TextureView,bounds:PixelRect,frame:&SignatureFrame,required_input_level:u32)->Option<BTreeSet<[u32;2]>> {
        let page=self.pages.values().find(|page|page.image.view==*view)?;
        Some(page_coordinates(bounds).filter(|coordinate|page.matches(Some(frame),page_rect(*coordinate).intersect(bounds),required_input_level)==Some(true)).collect())
    }
    pub(super) fn set_optional(&mut self,key:&Key) {let page=self.pages.get_mut(key).unwrap();page.set_optional(true);self.victims.touch(page);self.victims.compact(&self.pages);self.touch(key);}
    pub(super) fn set_optional_content(&mut self,expression:&graph::Node) {
        let keys=self.pages.iter().filter(|(key,page)|key.source.is_none() && key.expression==*expression && !page.optional).map(|(key,_)|key.clone()).collect::<Vec<_>>();
        for key in keys {self.set_optional(&key);}
    }
    pub(super) fn set_visible(&mut self,key:&Key) {if let Some(page)=self.pages.get_mut(key) && page.optional {page.set_optional(false);self.victims.touch(page);self.victims.compact(&self.pages);self.touch(key);}}
    pub fn bytes(&self) -> u64 { self.total.used() }
    pub(in crate::scene) fn metadata_bytes(&self)->u64 {
        self.index.storage_bytes()+self.pages.capacity() as u64*(std::mem::size_of::<(Key,Page)>()+1) as u64
            +self.ids.capacity() as u64*(std::mem::size_of::<(u64,Key)>()+1) as u64+self.victims.bytes()
            +self.pages.len() as u64*(std::mem::size_of::<Image>()+2*std::mem::size_of::<usize>()) as u64
            +self.version_slots*(std::mem::size_of::<([u32;2],Recorded)>()+1) as u64
            +self.submission_slots*(std::mem::size_of::<([u32;2],submission::Submission)>()+1) as u64
            +self.changes.log.capacity() as u64*std::mem::size_of::<Change>() as u64
            +self.unconfirmed.capacity() as u64*std::mem::size_of::<(submission::Submission,Key)>() as u64
    }
    pub fn pinned_bytes(&self)->u64 {self.pinned.used()}
    pub(super) fn allocated(&mut self,key:&Key,class:Class)->Option<Held> {
        let page=self.pages.get_mut(key)?;
        self.victims.used(&mut self.clock,page,class);let held=page.hold(&self.pinned);self.victims.compact(&self.pages);Some(held)
    }
    #[cfg(test)]
    pub(super) fn claim(&mut self, r: &WgpuRasterizer, key: Key, limit: u64, window: WindowId, class: Class) -> Result<Held, GpuRasterError> {
        if let Some(image)=self.allocated(&key,class) {return Ok(image);}
        let bytes=key.bytes();
        self.trim(limit.saturating_sub(bytes),class);
        if bytes>limit || self.bytes().saturating_add(bytes)>limit {return Err(GpuRasterError::NotReady);}
        let charge=self.total.charge(bytes);
        Ok(self.insert(r,key,window,charge))
    }
    pub(super) fn claim_reserved(&mut self,r:&WgpuRasterizer,key:Key,window:WindowId,class:Class,reservation:&mut crate::residency::Reservation)->Result<Held,GpuRasterError> {
        if let Some(image)=self.allocated(&key,class) {return Ok(image);}
        let bytes=key.bytes();
        let charge=reservation.split(bytes).ok_or(GpuRasterError::Invariant("Store allocation exceeds its accepted reservation"))?.charge();
        Ok(self.insert(r,key,window,charge))
    }
    fn insert(&mut self,r:&WgpuRasterizer,key:Key,window:WindowId,charge:crate::residency::Charge)->Held {
        #[cfg(not(target_arch="wasm32"))]
        self.retirement.get_or_insert_with(||Retirement {sender:r.device.retirement_handle(),
            total:r.device.transient_display.clone(),memory:r.device.transient_memory.clone(),retiring:r.scene_tiers.retiring.clone()});
        let image=Image::with_format(r,key.plan(),key.format);
        self.place(key,window,charge,image)
    }
    pub(super) fn recycle(&mut self,missing:&mut Vec<Key>,wanted:u64,window:WindowId)->Vec<(Key,Held)> {
        let mut spare=self.pages.iter().filter(|(_,page)|page.optional && !page.pinned()).map(|(key,page)|(rank(page),key.clone())).collect::<Vec<_>>();
        spare.sort_by(|a,b|a.0.cmp(&b.0));
        let (mut recycled,mut covered)=(Vec::new(),0);
        missing.retain(|key| {
            let size=key.plan().size;
            let Some(index)=spare.iter().position(|(_,old)|self.pages[old].image.plan.size==size && self.pages[old].image.texture.format()==key.format).filter(|_|covered<wanted) else {return true;};
            let Page {image,_charge:charge,..}=self.remove(&spare.remove(index).1);
            covered+=charge.bytes();
            let image=Image {plan:key.plan(),..Image::clone(&image)};
            recycled.push((key.clone(),self.place(key.clone(),window,charge,image)));
            false
        });
        recycled
    }
    fn place(&mut self,key:Key,window:WindowId,charge:crate::residency::Charge,image:Image)->Held {
        self.clock+=1;
        let bytes=charge.bytes();
        let image=Arc::new(image);
        self.victims.charge(window,bytes);
        let coverage=WrittenCoverage::new(if key.source.is_some() {0}else {key.level});
        self.index.insert(&key);self.touch(&key);self.ids.insert(self.clock,key.clone());
        let value=rebuild_value(estimated_work(&key),bytes);
        let mut page=Page {image,pin:Default::default(),optional:false,coverage,versions:Default::default(),used:self.clock,value,priority:self.victims.inflation.saturating_add(value),retention:crate::residency::Retained::new(&self.total,bytes),_charge:charge,window,demand:Default::default(),id:self.clock,written_at:0};
        let held=page.hold(&self.pinned);self.victims.touch(&page);self.pages.insert(key,page);self.victims.compact(&self.pages);
        held
    }
    pub(super) fn has(&self,key:&Key,area:PixelRect,accuracy:Accuracy,required_input_level:u32)->bool {
        let exact=Key {format:wgpu::TextureFormat::Rgba32Float,..key.clone()};
        let key=if accuracy==Accuracy::Exact && !self.pages.get(key).is_some_and(|page|Coverage::covers(&page.coverage.exact,area)) {&exact}else{key};
        self.pages.get(key).is_some_and(|page|page.matches(self.expected.as_deref(),area,required_input_level)==Some(true)
            && (accuracy==Accuracy::Display || Coverage::covers(&page.coverage.exact,area)))
    }
    pub(super) fn get(&mut self,key:&Key,area:PixelRect,accuracy:Accuracy,required_input_level:u32,class:Class)->Option<Held> {
        #[cfg(test)]
        crate::test_support::PassCounts {store_gets:1,..Default::default()}.record();
        let exact=Key {format:wgpu::TextureFormat::Rgba32Float,..key.clone()};
        let key=if accuracy==Accuracy::Exact && !self.pages.get(key).is_some_and(|page|Coverage::covers(&page.coverage.exact,area)) {&exact}else{key};
        let page=self.pages.get_mut(key)?;
        if page.matches(self.expected.as_deref(),area,required_input_level)!=Some(true)
            || accuracy==Accuracy::Exact && !Coverage::covers(&page.coverage.exact,area) {return None;}
        self.victims.used(&mut self.clock,page,class);let held=page.hold(&self.pinned);self.victims.compact(&self.pages);Some(held)
    }
    fn presented(&mut self,key:&Key,area:PixelRect,class:Class)->Option<Held> {
        let page=self.pages.get_mut(key)?;
        if !page.coverage.covers(area) {return None;}
        self.victims.used(&mut self.clock,page,class);let held=page.hold(&self.pinned);self.victims.compact(&self.pages);Some(held)
    }
    fn selected(&self,key:&Key,region:PixelRect,image:Held)->Projection {
        let page=&self.pages[key];
        Projection {region,image,key:key.clone(),versions:page.versions.values().filter_map(|recorded|recorded.clipped(region)).collect()}
    }
    fn recorded(&self,key:&Key,region:PixelRect)->Option<Recorded> {
        self.pages.get(key)?.versions.get(&key.coordinate).and_then(|recorded|recorded.clipped(region))
    }
    fn held_mut(&mut self,accepted:&Accepted)->Result<&mut Inputs,GpuRasterError> {
        let (key,region,input_level)=(accepted.key(),accepted.region(),accepted.input_level());
        self.inputs.get_mut(key).and_then(|inputs|inputs.iter_mut().find(|input|input.at(region,input_level)
            && input.accepted.recorded.version.identity()==accepted.recorded.version.identity()))
            .ok_or(GpuRasterError::Invariant("accepted source hold is missing"))
    }
    pub(super) fn hold_inputs(&mut self,accepted:&Accepted,sources:crate::scene::sources::AcceptedInputs)->Result<(),GpuRasterError> {
        sources.check_unit(accepted.input_unit())?;
        self.held_mut(accepted)?.sources=sources;Ok(())
    }
    pub(super) fn streaming(&mut self,interest:&Interest)->Result<Option<Accepted>,GpuRasterError> {
        if !self.accepted(&interest.key,interest.region,interest.input_level) {return Ok(None);}
        let Some(held)=self.accept_version(interest)? else {return Ok(None);};
        Ok(self.held_mut(&held).ok().filter(|input|input.stream.pending()).map(|_|held))
    }
    pub(super) fn selects(&mut self,accepted:&Accepted,selection:&super::sources::Selection)->bool {self.held_mut(accepted).is_ok_and(|held|held.stream.selects(selection))}
    pub(super) fn restamp_inputs(&mut self,accepted:&Accepted,class:Class)->Result<(),GpuRasterError> {
        let keys=self.held_mut(accepted)?.pages.iter().map(|(key,..)|key.clone()).collect::<Vec<_>>();
        for key in keys {if let Some(page)=self.pages.get_mut(&key) {self.victims.used(&mut self.clock,page,class);}}
        self.victims.compact(&self.pages);Ok(())
    }
    pub(super) fn take_stream(&mut self,accepted:&Accepted)->Result<super::sources::Stream,GpuRasterError> {Ok(std::mem::take(&mut self.held_mut(accepted)?.stream))}
    pub(super) fn hold_stream(&mut self,accepted:&Accepted,stream:super::sources::Stream)->Result<(),GpuRasterError> {self.held_mut(accepted)?.stream=stream;Ok(())}
    pub(super) fn holds(&self,accepted:&Accepted,native:&crate::scene::sources::Inputs,areas:&[(Key,PixelRect)])->bool {
        self.expected.as_ref().is_some_and(|frame|accepted.recorded.matches(frame,accepted.input_level())==Some(true)) && self.inputs.get(accepted.key()).is_some_and(|inputs|inputs.iter().any(|held|
            held.at(accepted.region(),accepted.input_level()) && held.accepted.recorded.version.identity()==accepted.recorded.version.identity()
            && held.sources.unit()==Some(accepted.input_unit()) && held.sources.contains(native) && held.pages.len()==areas.len()
            && areas.iter().all(|(key,area)|held.pages.iter().any(|(held,region,_)|held==key && region==area))))
    }
    pub(super) fn held_refusal(&self,accepted:&Accepted)->Option<&crate::residency::Blocked> {
        self.inputs.get(accepted.key())?.iter().find(|input|input.at(accepted.region(),accepted.input_level())
            && input.accepted.recorded.version.identity()==accepted.recorded.version.identity())?.sources.objects_refusal()
    }
    pub(super) fn take_inputs(&mut self,accepted:&Accepted)->Result<crate::scene::sources::AcceptedInputs,GpuRasterError> {
        let held=self.held_mut(accepted)?;
        held.sources.check_unit(accepted.input_unit())?;
        Ok(std::mem::take(&mut held.sources))
    }
    pub(super) fn accepted(&self,key:&Key,region:PixelRect,input_level:u32)->bool {
        self.inputs.get(key).is_some_and(|inputs|inputs.iter().any(|input|input.at(region,input_level)))
    }
    #[cfg(test)]
    pub(super) fn accepted_coordinates<'a>(&'a self,key:&'a Key,input_level:u32)->impl Iterator<Item=[u32;2]>+'a {
        self.inputs.iter().filter_map(move |(candidate,inputs)|
            (*candidate==Key {coordinate:candidate.coordinate,..key.clone()} && inputs.iter().any(|input|input.accepted.recorded.input_level==input_level)).then_some(candidate.coordinate))
    }
    pub(super) fn reserve_inputs(&mut self,keys:&[Key])->(Vec<(Key,Held)>,Vec<Key>,u64) {
        let mut held=Vec::new();let mut missing=Vec::new();let mut bytes=0;
        for key in keys {
            if let Some(page)=self.pages.get_mut(key) {held.push((key.clone(),page.hold(&self.pinned)));}
            else {bytes+=key.bytes();missing.push(key.clone());}
        }
        (held,missing,bytes)
    }
    pub(super) fn accept(&mut self,interest:&Interest,images:Vec<(Key,PixelRect,Held)>,accepted:Accepted)->Result<(),GpuRasterError> {
        let Interest {key,region,evaluation,input_level}=interest;let (region,evaluation)=(*region,*evaluation);
        debug_assert!(accepted.recorded.key==*key && accepted.recorded.region==region && accepted.recorded.input_level==*input_level);
        if self.expected.as_ref().is_none_or(|frame|accepted.recorded.matches(frame,accepted.recorded.input_level)==Some(false)) {return Err(GpuRasterError::Invariant("signature frame is missing or inconsistent"));}
        let inputs=self.inputs.entry(key.clone()).or_default();
        if let Some(held)=inputs.iter_mut().find(|held|held.at(region,*input_level)) {held.pages=images;debug_assert_eq!(held.accepted.recorded.version.identity(),accepted.recorded.version.identity());}
        else {let pin=self.pages.get_mut(key).map(|page|page.pin(&self.pinned));inputs.push(Inputs {region,evaluation,pages:images,sources:Default::default(),accepted,pin,stream:Default::default()});}
        Ok(())
    }
    pub(super) fn publish_inputs(&mut self,accepted:&Accepted) {
        let published=self.held_mut(accepted).map(|held|held.pages.iter().filter(|(key,..)|key.source.is_some()).map(|(key,..)|key.clone()).collect::<Vec<_>>()).unwrap_or_default();
        self.finish_inputs(accepted);
        for key in published {if self.pages.contains_key(&key) {self.set_optional(&key);}}
    }
    pub(super) fn finish_inputs(&mut self,accepted:&Accepted) {
        if let Some(inputs)=self.inputs.get_mut(accepted.key()) {
            inputs.retain(|input|!input.at(accepted.region(),accepted.input_level()) || input.accepted.recorded.version.identity()!=accepted.recorded.version.identity());
            if inputs.is_empty() {self.inputs.remove(accepted.key());}
        }
    }
    pub(super) fn keep_outputs(&mut self,interest:&Interest,kept:&[Key]) {
        let Some(input)=self.inputs.get_mut(&interest.key).and_then(|inputs|inputs.iter_mut().find(|input|input.at(interest.region,interest.input_level))) else {return;};
        input.sources=Default::default();input.pin=None;input.stream=Default::default();
        let released=input.pages.extract_if(..,|(key,_,_)|!kept.contains(key)).map(|(key,_,_)|key).collect::<Vec<_>>();
        self.release_unwanted(&released);
    }
    #[cfg(test)]
    pub(super) fn fixture_handle(&self,key:&Key,region:PixelRect,input_level:u32)->Accepted {
        self.inputs[key].iter().find(|input|input.at(region,input_level)).unwrap().accepted.clone()
    }
    #[cfg(test)]
    pub(super) fn take_fixture_inputs(&mut self,key:&Key,region:PixelRect,input_level:u32)->crate::scene::sources::AcceptedInputs {
        let handle=self.fixture_handle(key,region,input_level);self.take_inputs(&handle).unwrap()
    }
    #[cfg(test)]
    fn finish_fixture_inputs(&mut self,key:&Key,region:PixelRect,input_level:u32) {
        let handle=self.inputs.get(key).and_then(|inputs|inputs.iter().find(|input|input.at(region,input_level))).map(|input|input.accepted.clone());
        if let Some(handle)=handle {self.finish_inputs(&handle);}
    }
    pub(super) fn retain_work(&mut self,windows:&[Interest]) {
        let expected=self.expected.clone();
        for inputs in self.inputs.values_mut() {
            inputs.retain(|input| {
                let matches=expected.as_ref().is_some_and(|frame|input.accepted.recorded.matches(frame,input.accepted.recorded.input_level)!=Some(false));
                let interest=input.interest();
                let retained=matches && windows.iter().any(|wanted|wanted.contains(&interest));
                debug_assert!(retained || !matches || expected.as_ref().is_none_or(|frame|!Arc::ptr_eq(frame,&input.accepted.frame)),
                    "same-frame preparation pruned a still-matching accepted producer");
                retained
            });
        }
        self.retain_referenced_inputs();
        self.inputs.retain(|_,inputs|!inputs.is_empty());
        let unwanted=self.pages.iter().filter(|(_,page)|page.unwanted()).map(|(key,_)|key.clone()).collect::<Vec<_>>();
        self.release_unwanted(&unwanted);
    }
    pub(super) fn park(&mut self,kept:impl Fn(&Key)->bool) {
        self.inputs.clear();
        self.release_except(kept);
    }
    pub(super) fn release_except(&mut self,kept:impl Fn(&Key)->bool) {
        let released=self.pages.iter().filter(|(key,page)|!page.pinned() && !kept(key)).map(|(key,_)|key.clone()).collect::<Vec<_>>();
        for key in released {let image=self.remove(&key).image;self.retire(image);}
    }
    pub(super) fn release_unwanted(&mut self,keys:&[Key]) {
        for key in keys {
            if self.pages.get(key).is_some_and(Page::unwanted) {let image=self.remove(key).image;self.retire(image);}
        }
    }
    fn retain_referenced_inputs(&mut self) {
        let mut accepted=std::collections::HashMap::<Key,Vec<PixelRect>>::new();
        let mut pending=self.inputs.iter().filter(|(key,_)|key.source.is_none()).flat_map(|(_,inputs)|inputs)
            .flat_map(|input|&input.pages).filter(|(key,_,_)|key.source.is_some()).map(|(key,area,_)|(key.clone(),*area)).collect::<Vec<_>>();
        while let Some((key,area))=pending.pop() {
            let regions=accepted.entry(key.clone()).or_default();
            if regions.iter().any(|region|region.intersect(area)==area) {continue;}
            regions.push(area);
            if let Some(inputs)=self.inputs.get(&key) {
                pending.extend(inputs.iter().filter(|input|!input.region.intersect(area).is_empty()).flat_map(|input|&input.pages)
                    .filter(|(child,_,_)|child.source.is_some()).map(|(child,area,_)|(child.clone(),*area)));
            }
        }
        for (key,inputs) in &mut self.inputs {if key.source.is_some() {
            inputs.retain(|input|accepted.get(key).is_some_and(|areas|areas.iter().any(|area|!area.intersect(input.region).is_empty())));
        }}
    }
    pub(super) fn commit(&mut self,write:Write,work:u64,encoder:&mut submission::CommandEncoder) {
        #[cfg(test)]
        crate::scene::sources::assert_reading("store commit");

        let Write {key,region:area,accuracy,accepted,..}=write;
        #[cfg(test)]
        if key.source.is_none() {crate::test_support::PassCounts {composite_commits:1,..Default::default()}.record();}
        let input_level=accepted.as_ref().map(|accepted|accepted.recorded.input_level);
        assert!(if key.source.is_some() {page_coordinates(area).count()==1 && area==page_rect([area.min_x()/PAGE_SIZE,area.min_y()/PAGE_SIZE]).intersect(PixelRect::full(key.extent))}
            else {area==key.plan().bounds},"a committed unit must be completely written");
        let page=self.pages.get_mut(&key).unwrap();
        self.clock+=1;page.written_at=self.clock;
        let submissions=page.coverage.submissions.capacity();
        page.coverage.commit(area,accuracy,&encoder.submission());
        self.submission_slots+=(page.coverage.submissions.capacity()-submissions) as u64;
        page.value=rebuild_value(work,page.image.bytes());page.priority=self.victims.inflation.saturating_add(page.value);self.victims.touch(page);
        let pin=page.pin(&self.pinned);encoder.hold(pin);
        let coordinate=coordinates(area,page.coverage.written.level).next().expect("nonempty committed unit");
        self.changes.push(Change::Page(key.clone()));
        if key.source.is_none() {self.unconfirmed.push((encoder.submission(),key.clone()));}
        page.versions.remove(&coordinate);
        if let Some(Accepted {recorded,..})=accepted {
            assert!(recorded.key==key && recorded.region==area);
            debug_assert!(self.expected.as_ref().is_some_and(|frame|recorded.matches(frame,recorded.input_level)==Some(true)),"a committed unit matches its frame");
            let before=page.versions.capacity();page.versions.insert(coordinate,recorded);
            self.version_slots+=(page.versions.capacity()-before) as u64;
        }
        self.victims.compact(&self.pages);
        if let Some(inputs)=self.inputs.get_mut(&key) {
            inputs.retain(|input|input.region.intersect(area)!=input.region
                || input_level.is_none_or(|level|input.accepted.recorded.input_level<level));
            if inputs.is_empty() {self.inputs.remove(&key);}
        }
    }
    fn retire(&mut self,image:Arc<Image>) {
        #[cfg(not(target_arch="wasm32"))]
        {
            let retirement=self.retirement.as_ref().expect("allocated Store retirement owner");
            let retiring=retirement.retiring.charge(image.bytes());
            let charge=crate::present_transition::Charge::detached(retirement.total.charge(image.bytes()),retirement.memory.clone());
            self.retired.0.get_or_insert_with(||retirement.sender.clone());
            self.retired.1.push((image,charge,retiring,self.search.clone()));
        }
        #[cfg(target_arch="wasm32")]
        self.retired.0.push(image);
    }
    fn remove(&mut self,key:&Key)->Page {
        let page=self.pages.remove(key).unwrap();
        if let Some(inputs)=self.inputs.get_mut(key) {
            inputs.retain(|input|input.pin.is_none());
            if inputs.is_empty() {self.inputs.remove(key);}
        }
        self.index.remove(key);self.touch(key);self.ids.remove(&page.id);self.victims.release(page.window,page.image.bytes());
        self.version_slots-=page.versions.capacity() as u64;self.submission_slots-=page.coverage.submissions.capacity() as u64;page
    }
    pub(super) fn trim(&mut self,limit:u64,class:Class) {
        let epoch=self.victims.epoch;
        self.trim_at(limit,|page|!page.optional && (page.demand.protects(epoch,class) || page.demand.outranks(epoch,class)));
    }
    pub(super) fn release_undemanded(&mut self,limit:u64) {let epoch=self.victims.epoch+1;self.trim_at(limit,|page|!page.optional && page.demand.protects(epoch,Class::Fallback));}
    fn trim_at(&mut self,limit:u64,kept:impl Fn(&Page)->bool) {
        let mut bytes=self.bytes();
        let mut pinned=Vec::new();
        while bytes>limit {
            let Some(window)=self.victims.next(limit) else {break;};
            let heap=&mut self.victims.windows.get_mut(&window).unwrap().heap;
            let Some(victim)=heap.pop() else {break;};
            let Some((key,page))=self.ids.get(&victim.id).and_then(|key|self.pages.get_key_value(key)).filter(|(_,page)|rank(page)==victim.rank) else {continue;};
            if page.pinned() || kept(page) {pinned.push((page.window,victim));continue;}
            let key=key.clone();
            let removed=self.remove(&key).image;
            if victim.rank.0 {self.victims.inflation=self.victims.inflation.max(victim.rank.1);}
            bytes-=removed.bytes();self.retire(removed);
        }
        for (window,victim) in pinned {self.victims.windows.entry(window).or_default().heap.push(victim);}
    }
    pub(super) fn refinement_frontier(&mut self,key:&Key,pages:&mut Vec<Key>) {
        if self.get(key,key.plan().bounds,Accuracy::Exact,0,Class::Refinement).is_some()
            || self.pages.get(key).is_some_and(|page|page.matches(self.expected.as_deref(),key.plan().bounds,0).is_none()) {return;}
        if !pages.contains(key) {pages.push(key.clone());}
    }
    pub(super) fn reduction(&mut self,key:&Key,required_input_level:u32,class:Class)->Option<(Vec<(Key,Held)>,Accuracy)> {
        if key.level==0 {return None;}
        let mut accuracy=Accuracy::Exact;
        let mut images=Vec::new();
        for child in key.finer() {
            let area=child.plan().bounds;
            let full=Key {format:wgpu::TextureFormat::Rgba32Float,..child.clone()};
            let (image,selected)=if let Some(image)=self.get(&full,area,Accuracy::Exact,0,class) {(image,full)}
                else if let Some(image)=self.presented(&child,area,class) {(image,child)}
                else {(self.presented(&full,area,class)?,full)};
            let coverage=&self.pages[&selected].coverage;
            if !Coverage::covers(&coverage.exact,area) {accuracy=Accuracy::Display;}
            if self.pages[&selected].matches(self.expected.as_deref(),area,required_input_level)!=Some(true) {return None;}
            images.push((selected,image));
        }
        (!images.is_empty()).then_some((images,accuracy))
    }
}

#[cfg(test)]
fn fixture_store(packet:FramePacket<'_>)->Store {
    let mut store=Store::default();
    store.set_expected(Arc::new(SignatureFrame::from_scene(packet,Declared::default()).unwrap()));store
}
#[cfg(test)]
pub(super) fn accept_fixture_inputs(store:&mut Store,key:&Key,region:PixelRect,evaluation:Evaluation,images:Vec<(Key,PixelRect,Held)>,input_level:u32) {
    let interest=Interest::new(key.clone(),evaluation,region,input_level);
    let accepted=store.accept_version(&interest).unwrap().unwrap();
    store.accept(&interest,images,accepted).unwrap();
    let handle=store.fixture_handle(key,region,input_level);
    let held=store.inputs.get_mut(key).unwrap().iter_mut().find(|held|held.at(region,input_level)).unwrap();
    if held.sources.unit().is_none() {held.sources=std::mem::take(&mut held.sources).for_unit(handle.input_unit()).unwrap();}
}
#[cfg(test)]
pub(super) fn hold_fixture_inputs(store:&mut Store,key:&Key,region:PixelRect,evaluation:Evaluation,sources:crate::scene::sources::AcceptedInputs,input_level:u32) {
    if !store.inputs.get(key).is_some_and(|inputs|inputs.iter().any(|held|held.at(region,input_level))) {
        let interest=Interest::new(key.clone(),evaluation,region,input_level);
        let accepted=store.accept_version(&interest).unwrap().unwrap();
        store.accept(&interest,Vec::new(),accepted).unwrap();
    }
    let handle=store.fixture_handle(key,region,input_level);
    store.hold_inputs(&handle,sources.for_unit(handle.input_unit()).unwrap()).unwrap();
}
#[cfg(test)]
pub(super) fn evict_composite_pages(cache:&Cache,areas:&[PixelRect]) {evict_composite_pages_where(cache,areas,|_|true);}
#[cfg(test)]
pub(super) fn evict_composite_pages_where(cache:&Cache,areas:&[PixelRect],evicted:impl Fn(&Key)->bool) {
    let mut store=cache.pages.lock().unwrap();
    let keys=store.retained_pages().into_iter().filter(|page|page.key.source.is_none() && evicted(&page.key)
        && areas.iter().any(|area|!page.key.plan().bounds.intersect(*area).is_empty())).map(|page|page.key).collect::<Vec<_>>();
    for key in keys {store.remove(&key);}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn indexed_key(coordinate:[u32;2],level:u32)->Key {
        Key {source:None,expression:Arc::new(graph::Expression::Color([1f32.to_bits();4])),extent:[65536;2],
            color:Default::default(),blend_space:layer_core::BlendSpace::Linear,level,coordinate,format:wgpu::TextureFormat::Rgba16Float}
    }
    fn full_view(document:&layer_core::Document)->FramePacket<'_> {
        let extent=document.composition().size;
        let mut frame=crate::test_support::packet(document.scene(),extent);frame.composite_all=false;
        frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
        frame
    }
    fn demanded_pages(rows:&[u32])->(layer_core::Document,WgpuRasterizer,Vec<Vec<Key>>,layer_core::MemoryOwner) {
        let document=super::super::tests::document_at([1024,512]);let frame=full_view(&document);
        let mut r=crate::memory::memory_tests::renderer(document.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(2<<30),..Default::default()}).unwrap();
        crate::test_support::submit_settled(&mut r,frame);crate::test_support::complete(&r);
        let rows=rows.iter().map(|&y|(0..256).map(|x|indexed_key([x,y],0)).collect::<Vec<_>>()).collect::<Vec<_>>();
        let mut store=r.scene.as_ref().unwrap().scale_pages.lock().unwrap();
        for (epoch,keys) in rows.iter().enumerate() {
            if epoch!=0 {store.advance_epoch();}
            for key in keys {drop(store.claim(&r,key.clone(),u64::MAX,WindowId::default(),Class::Visible).unwrap());drop(store.allocated(key,Class::Visible).unwrap());}
        }
        drop(store);
        let budget=r.factory.memory_budget().clone();
        let other=budget.register_owner(layer_core::MemoryOwnerKind::Other,layer_core::MemoryClass::Pinned,Some(budget.snapshot().devices[0].device));
        let held=budget.snapshot().devices[0].limit_bytes;
        other.report(layer_core::MemoryReport {gpu_bytes:held,pinned_gpu_bytes:held,..Default::default()});
        budget.flush_reports();
        (document,r,rows,other)
    }
    fn retained(r:&WgpuRasterizer,keys:&[Key])->usize {
        let store=r.scene.as_ref().unwrap().scale_pages.lock().unwrap();keys.iter().filter(|key|store.pages.contains_key(*key)).count()
    }
    #[test]
    fn a_lowered_display_grant_releases_pages_only_the_previous_frame_demanded() {
        let (document,mut r,rows,_pinned)=demanded_pages(&[200,201]);
        let frame=full_view(&document);
        let before=format!("used {} grant {} retiring {} store {}",r.scene_tiers.display.used(),r.memory.retention_grant(),r.scene_tiers.retiring.used(),r.scene.as_ref().unwrap().retained_scale_bytes()[0]);
        r.begin_frame_memory(frame);
        let after=format!("used {} grant {} pending {} store {:?}",r.scene_tiers.display.used(),r.memory.retention_grant(),r.scene_tiers.display.heap().pending(),r.scene.as_ref().unwrap().retained_scale_bytes());
        assert_eq!(retained(&r,&rows[1]),rows[1].len(),"the trim keeps every page the current frame demanded: before {before}, after {after}");
        assert_eq!(retained(&r,&rows[0]),0,"a lowered grant keeps no page only the previous frame demanded: before {before}, after {after}");
    }
    #[test]
    fn a_frame_waiting_on_a_refused_restore_releases_the_pages_its_last_frame_demanded() {
        let (document,mut r,rows,_pinned)=demanded_pages(&[200]);
        let target=document.active_target().unwrap();
        let dabs=(0..8).map(|n|crate::tests::test_dab([(n%4*PAGE_SIZE+128) as f32,(n/4*PAGE_SIZE+128) as f32],[1.,0.,1.,1.],1.)).collect::<Vec<_>>();
        let damage=dabs.iter().copied().map(layer_render::Dab::bounds).fold(layer_core::Rect::EMPTY,layer_core::Rect::union);
        let batch=layer_render::DabBatch {dab_count:dabs.len() as u32,kind:layer_render::DabBatchKind::Preview,
            ..crate::test_support::dab_batch(target,crate::tests::test_style(crate::BrushExecution::Dry),damage)};
        let frame=FramePacket {dabs:&dabs,dab_batches:std::slice::from_ref(&batch),..full_view(&document)};
        let refused=Some(crate::frame_demand::Outstanding::Blocked(crate::frame_demand::Input::Residency));
        let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
        while r.restore_preparation()!=refused {
            assert!(!r.raster_dependencies_ready(frame,None),"the pinned device refuses the preview outputs");
            assert!(std::time::Instant::now()<deadline,"the preview outputs wait on residency: {:?}",r.outstanding_terms());
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(retained(&r,&rows[0]),rows[0].len(),"the refusing frame keeps the pages the last frame demanded");
        assert!(!r.raster_dependencies_ready(frame,None));
        assert!(retained(&r,&rows[0])<rows[0].len(),"a frame waiting on a refused restore kept every page its last frame demanded");
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn removed_store_textures_stay_charged_until_the_poll_actor_destroys_them() {
        use std::sync::mpsc;
        use std::time::Duration;
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        for unwritten in [false,true] {
            let pages=Pages::default();let key=indexed_key([0;2],0);
            let mut store=pages.lock().unwrap();
            let before=r.device.transient_display.used();
            let image=store.claim(&r,key,u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();let bytes=image.bytes();
            let alive=Arc::downgrade(image.image());drop(image);
            let (entered,ready)=mpsc::channel();let (release,held)=mpsc::channel();
            let actor=r.device.retirement_handle();
            actor.run(move || {let _=entered.send(());held.recv_timeout(crate::READBACK_TIMEOUT).unwrap();});
            ready.recv_timeout(Duration::from_secs(2)).unwrap();
            if unwritten {store.retain_work(&[]);}else {store.trim(0,crate::residency::Class::Visible);}
            let logical=store.bytes();drop(store);
            let retained=alive.upgrade().is_some();
            let charged=r.device.transient_display.used();
            release.send(()).unwrap();r.device.retirement_handle().flush().unwrap();
            assert_eq!(logical,0);assert!(retained,"removed physical texture escaped the actor (unwritten={unwritten})");
            assert_eq!(charged,before+bytes,"detached physical texture lost its charge");
            assert!(alive.upgrade().is_none());assert_eq!(r.device.transient_display.used(),before);
        }
    }
    #[test]
    fn a_large_brush_contact_keeps_presenting_under_a_tight_device_budget() {
        let extent=[1333,1000];
        let document=super::super::tests::document_at(extent);
        let target=document.active_target().unwrap();
        let mut r=crate::memory::memory_tests::renderer(document.composition().color,
            layer_core::DeviceMemoryFacts {budget_bytes:Some(256<<20),unified:true,..Default::default()}).unwrap();
        let mut frame=crate::test_support::packet(document.scene(),extent);
        frame.view=crate::test_support::view([400,300]);
        frame.view.document_to_surface=[0.29,0.,0.,0.29,0.,0.];
        crate::test_support::submit_settled(&mut r,frame);
        let keys=|r:&WgpuRasterizer|r.scene.as_ref().unwrap().scale_pages.lock().unwrap().retained_pages()
            .into_iter().map(|page|page.key).collect::<std::collections::HashSet<_>>();
        let style=crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::Paintbrush);
        for index in 0..8 {
            let before=keys(&r);
            let mut dab=crate::tests::test_dab([400.+index as f32*50.,500.],[0.8,0.1,0.2,1.],1.);
            dab.radii=[400.;2];
            if let Some(dry)=style.contact.and_then(|contact|contact.dry) {
                let relief=dry.pose(0.5).relief;
                dab.previous=[400.,400.,1.,0.];dab.motion=[50.,0.];
                dab.contact=[0.5,relief[0],relief[1],relief[2]];dab.previous_contact=dab.contact;
            }
            let batch=layer_render::DabBatch {stroke_start:index==0,stroke_end:false,
                ..crate::test_support::dab_batch(target,style.clone(),dab.bounds())};
            crate::test_support::submit_ready(&mut r,FramePacket {reset_layers:false,composite_all:false,
                dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
            let after=keys(&r);
            assert!(before.is_subset(&after),"stroke frame {index} evicted {} previously retained Store pages",before.difference(&after).count());
            assert!(r.has_presentable_frame());
        }
        assert!(r.readback_srgb_rgba8().unwrap().as_chunks::<4>().0.iter().any(|pixel|pixel[0]>pixel[1].saturating_add(10)));
    }
    #[test]
    fn an_overview_claim_keeps_the_previous_frames_projected_view_pages() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut store=Store::default();
        store.victims.epoch=1;
        let view=indexed_key([0,0],0);
        let other=indexed_key([1,0],0);
        let bytes=u64::from(PAGE_SIZE).pow(2)*8;
        drop(store.claim(&r,view.clone(),2*bytes,WindowId::default(),Class::Visible).unwrap());
        drop(store.allocated(&view,Class::Visible).unwrap());
        drop(store.claim(&r,other.clone(),2*bytes,WindowId::default(),Class::Prefetch).unwrap());
        store.victims.epoch+=1;
        drop(store.claim(&r,indexed_key([2,0],1),2*bytes,WindowId::default(),Class::Fallback).unwrap());
        assert!(store.pages.contains_key(&view),"overview-first work evicted the page projected last frame");
        assert!(!store.pages.contains_key(&other));
    }
    #[test]
    fn seeded_optional_admissions_never_evict_a_wanted_page_a_higher_class_used_this_frame_or_last() {
        const CLASSES:[Class;6]=[Class::Authored,Class::Visible,Class::Fallback,Class::Navigator,Class::Prefetch,Class::Refinement];
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let display=Arc::new(crate::residency::Tier::default());
        let page=indexed_key([0;2],0).bytes();
        display.set_limit(8*page,2*page);
        let mut store=Store::within(&display);store.victims.epoch=1;
        let mut uses=std::collections::HashMap::<Key,Vec<(u64,Class)>>::new();let mut optional=std::collections::HashSet::new();
        let (mut seed,mut evicted)=(0x5eed_0f_9a6e_5u64,0);
        for _ in 0..600 {
            store.advance_epoch();let epoch=store.victims.epoch;
            for _ in 0..8 {
                seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;
                let class=CLASSES[(seed%6) as usize];let key=indexed_key([(seed>>8) as u32%12,0],0);
                if store.pages.contains_key(&key) {
                    drop(store.allocated(&key,class));uses.get_mut(&key).unwrap().push((epoch,class));
                    if (seed>>20)%3==0 {store.set_optional(&key);optional.insert(key);}
                    continue;
                }
                let tier=store.tier();
                let admitted=tier.admit(class,page).or_else(|blocked| {store.trim(store.bytes().saturating_sub(blocked.deficit()),class);tier.admit(class,page)});
                if let Ok(mut reservation)=admitted {
                    drop(store.claim_reserved(&r,key.clone(),WindowId::default(),class,&mut reservation).unwrap());drop(store.allocated(&key,class));uses.insert(key,vec![(epoch,class)]);
                }
                uses.retain(|key,used| {
                    if store.pages.contains_key(key) {return true;}
                    let window=if class.floor_reserved() {epoch}else {epoch-1};
                    assert!(optional.remove(key) || !used.iter().any(|&(at,user)|at>=window && user<class),"a {class:?} admission at epoch {epoch} evicted a page used {used:?}");
                    evicted+=1;false
                });
            }
        }
        assert!(evicted>100,"the run evicted only {evicted} pages");
    }
    #[test]
    fn page_index_queries_visit_only_the_requested_cells_at_each_kept_level() {
        let mut index=Index::default();
        for number in 0..50_000 {index.insert(&indexed_key([number%256,number/256],0));}
        let key=indexed_key([17,42],0);let area=key.plan().bounds;
        let coarse=indexed_key(key.coordinate.map(|value|value/8),3);index.insert(&coarse);
        let before=crate::test_support::PassCounts::snapshot();
        let found=index.overlapping(&key,area,0..=display_mips::MAX_LEVEL).cloned().collect::<std::collections::HashSet<_>>();
        assert!(found==[key.clone(),coarse.clone()].into());
        assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).store_page_visits,2);
        for kept in [&key,&coarse] {index.remove(kept);}
        assert!(index.overlapping(&key,area,0..=display_mips::MAX_LEVEL).next().is_none());
    }
    #[test]
    fn seeded_page_index_changes_match_a_full_scan_after_each_operation() {
        let mut index=Index::default();let mut retained=std::collections::HashSet::new();let mut seed=0x139au64;
        for _ in 0..1024 {
            seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let level=(seed>>48) as u32%4;
            let coordinate=[(seed>>8) as u32%32,(seed>>24) as u32%32];let key=indexed_key(coordinate,level);
            if retained.remove(&key) {index.remove(&key);}else {retained.insert(key.clone());index.insert(&key);}
            let area=PixelRect::new(coordinate[0]*PAGE_SIZE,coordinate[1]*PAGE_SIZE,(coordinate[0]+3)*PAGE_SIZE,(coordinate[1]+2)*PAGE_SIZE);
            let expected=retained.iter().filter(|key|!key.plan().bounds.intersect(area).is_empty()).cloned().collect::<std::collections::HashSet<_>>();
            let actual=index.overlapping(&key,area,0..=display_mips::MAX_LEVEL).cloned().collect::<std::collections::HashSet<_>>();
            assert!(actual==expected);
        }
        for key in retained {index.remove(&key);}
        assert!(index.owners.is_empty());assert_eq!(index.allocated,0);
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_refusal_behind_pinned_optional_pages_wakes_when_they_unpin_and_its_retry_trims_them() {
        use std::sync::mpsc;use std::time::Duration;
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let (edge,owner)=crate::device_poll::windowless();
        let display=Arc::new(crate::residency::Tier::default());
        let page=indexed_key([0;2],0).bytes();
        display.set_limit(2*page,0);
        let mut store=Store::within(&display);
        let [kept,released]=[[0,0],[1,0]].map(|coordinate|indexed_key(coordinate,0));
        let mut held=[&kept,&released].map(|key|Some(store.claim(&r,key.clone(),u64::MAX,WindowId::default(),Class::Visible).unwrap()));
        for key in [&kept,&released] {store.set_optional(key);}
        let refused=|store:&mut Store| {
            let blocked=display.admit(Class::Navigator,page).err().expect("the store fills the display");
            store.trim(store.bytes().saturating_sub(blocked.deficit()),Class::Navigator);
            display.admit(Class::Navigator,page).err().expect("pinned and visible pages do not yield")
        };
        let blocked=refused(&mut store);
        assert_eq!(display.yielding(),0);
        assert!(!blocked.fits(),"pinned optional pages hold the room");
        let (send,woken)=mpsc::channel();
        edge.on_release(&blocked,&(Arc::new(move||{let _=send.send(());}) as crate::residency::Wake));
        held[1]=store.allocated(&released,Class::Visible);held[1]=None;
        woken.recv_timeout(Duration::from_secs(1)).expect("unpinning an optional page wakes the refused claim");
        assert!(blocked.fits() && display.yielding()==page);
        store.trim(store.bytes().saturating_sub(blocked.deficit()),Class::Navigator);
        let admitted=display.admit(Class::Navigator,page).expect("the retry trims the unpinned optional page and admits");
        assert!(store.pages.contains_key(&kept) && !store.pages.contains_key(&released));
        assert_eq!(display.yielding(),0);
        held[0]=None;
        assert_eq!(display.yielding(),page);
        let repinned=store.allocated(&kept,Class::Visible).unwrap();
        assert_eq!(display.yielding(),0,"pinning withdraws the page's yielding bytes");
        assert!(!refused(&mut store).fits());
        drop(repinned);
        store.set_visible(&kept);
        assert_eq!(display.yielding(),0,"a visible page does not yield");
        assert!(!refused(&mut store).fits());
        drop((admitted,owner));
    }
    #[test]
    fn optional_subexpressions_cannot_fill_other_roots_and_yield_before_visible_pages() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let extent=[2*PAGE_SIZE,PAGE_SIZE];
        let root=Key {source:None,expression:Arc::new(graph::Expression::Color([1f32.to_bits();4])),extent,
            color:Default::default(),blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
        let branch=Key {expression:Arc::new(graph::Expression::Color([0,1f32.to_bits(),0,1f32.to_bits()])),..root.clone()};
        let visible=Key {coordinate:[1,0],..root.clone()};
        let document=super::super::tests::document_at(extent);
        let bytes=u64::from(PAGE_SIZE).pow(2)*16;let mut store=fixture_store(crate::test_support::packet(document.scene(),extent));
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        for key in [&visible,&branch] {
            let image=store.claim(&r,key.clone(),2*bytes,WindowId::default(),crate::residency::Class::Visible).unwrap();
            let write=store.prepare_write(key.clone(),image.plan.bounds,Accuracy::Display,key.level).unwrap();
            drop(encoder.color_pass("committed Store class",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
            store.commit(write,0,&mut encoder);
        }
        store.set_optional(&branch);encoder.submit(&r.queue);crate::test_support::complete(&r);
        assert!(store.coverage(&root,root.plan().bounds,0).written.is_empty());
        assert!(store.get(&root,root.plan().bounds,Accuracy::Display,0,crate::residency::Class::Visible).is_none());
        let needed=store.claim(&r,root.clone(),2*bytes,WindowId::default(),crate::residency::Class::Visible).unwrap();
        assert!(store.pages.contains_key(&visible));assert!(!store.pages.contains_key(&branch));
        assert_eq!(store.bytes(),2*bytes);assert_eq!(needed.plan,root.plan());
    }
    #[test]
    fn display_pages_and_their_reductions_require_exact_replacement() {
        let bounds=PixelRect::full([2*PAGE_SIZE,PAGE_SIZE]);
        fn written(coverage:&mut Coverage,area:PixelRect,accuracy:Accuracy) {
            for coordinate in coordinates(area,coverage.written.level) {
                coverage.written.entries.insert(coordinate);coverage.include(coordinate,Some(true),accuracy==Accuracy::Exact);
            }
        }
        let mut fine=Coverage::new(0);
        written(&mut fine,bounds,Accuracy::Display);
        assert!(fine.ready(bounds,Accuracy::Display));
        assert!(!fine.ready(bounds,Accuracy::Exact));
        let mut coarse=Coverage::new(0);
        written(&mut coarse,bounds,if fine.ready(bounds,Accuracy::Exact) {Accuracy::Exact}else{Accuracy::Display});
        assert!(!coarse.ready(bounds,Accuracy::Exact));
        let half=PixelRect::full([PAGE_SIZE;2]);
        written(&mut fine,half,Accuracy::Exact);
        assert!(fine.ready(half,Accuracy::Exact));
        assert!(!fine.ready(bounds,Accuracy::Exact));
        written(&mut fine,PixelRect::new(PAGE_SIZE,0,2*PAGE_SIZE,PAGE_SIZE),Accuracy::Exact);
        assert!(fine.ready(bounds,Accuracy::Exact));
        assert!(fine.matching.entries.remove(&[0,0]));
        assert!(Coverage::covers(&fine.written,bounds));
        assert!(!fine.ready(bounds,Accuracy::Display));
        assert!(!fine.ready(bounds,Accuracy::Exact));
    }
}

pub(super) struct Projection {
    pub(super) region: PixelRect,
    image: Held,
    key: Key,
    versions:Vec<Recorded>,
}
struct Shown {recorded:Option<Recorded>,submission:submission::Submission,exact:bool,direct:bool,refused:bool,witness:Option<super::contact::Witness>}
impl Shown {
    fn unnamed(&self)->bool {self.recorded.is_none() && !self.submission.discarded()}
    fn composition(&self,template:&Key)->bool {
        !self.exact && (self.direct || self.recorded.as_ref().is_some_and(|recorded|recorded.key.level==template.level && recorded.key.format==template.format
            && recorded.key.color==template.color && recorded.key.blend_space==template.blend_space))
    }
}
pub(super) struct Selection {pub(super) copies:Vec<(usize,Projection,bool)>,pub(super) composes:Vec<PixelRect>,pub(super) transition:bool,pub(super) held:Option<usize>}
impl Selection {
    pub(super) fn complete(&self,served:PixelRect)->bool {
        self.held.is_none() && self.composes.iter().all(|region|region.intersect(served).is_empty())
    }
    pub(super) fn pins(&self)->Vec<Held> {self.copies.iter().map(|(_,copy,_)|copy.image.clone()).collect()}
}
#[derive(Clone,Copy)]
pub(super) struct Asked {pub(super) view:layer_render::ViewState,pub(super) moving:bool}
impl Asked {
    fn no_higher_than(self,earlier:Self)->bool {self.view==earlier.view && (self.moving || !earlier.moving)}
}
#[derive(Default)]
struct Cell {shown:Option<Shown>,current:bool,wait:Option<crate::frame_demand::UnitWait>,refusal:Option<crate::residency::Blocked>,queued:bool,derived:bool,reduced:bool,reduced_from:Option<layer_core::raster::ContentStamp>,waits:Vec<super::super::object_cache::ObjectWait>,lag:Option<PixelRect>,
    selected:Option<ContentVersion>,refused_request:Option<ContentVersion>}
impl Cell {
    fn showing(shown:Shown)->Self {Self {shown:Some(shown),current:true,lag:Some(PixelRect::EMPTY),..Self::default()}}
    fn queue(&mut self) {if !self.derived {self.queued=true;}}
    fn stale(&mut self) {if !self.derived {self.current=false;self.queued=true;self.lag=None;}}
    fn invalidate(&mut self) {self.stale();self.waits.clear();self.selected=None;}
    fn select(&mut self,request:&ContentVersion) {
        if self.refusal.is_some() && self.refused_request.get_or_insert_with(||request.clone()).matches(request)!=Some(true) {self.release();}
        self.selected=Some(request.clone());
    }
    fn reach(&mut self,area:Option<PixelRect>,template:&Key) {
        let lag=self.lag.zip(area).filter(|_|self.shown.as_ref().is_some_and(|shown|shown.composition(template))).map(|(lag,area)|lag.union(area));
        self.invalidate();self.lag=lag;
    }
    fn sleeping(&self)->bool {self.waits.iter().any(|(_,waiter)|waiter.as_ref().is_some_and(|waiter|!waiter.settled()))}
    fn written(&self)->bool {self.derived && self.reduced || self.shown.is_some()}
    fn live(&self)->bool {!self.written() && self.unsettled()}
    fn unsettled(&self)->bool {!matches!(self.wait,Some(crate::frame_demand::UnitWait::Parked(..)|crate::frame_demand::UnitWait::End(_)))}
    fn awaits_native(&self,refining:bool)->bool {self.live() || refining && !self.derived && self.unsettled() && self.shown.as_ref().is_some_and(|shown|!shown.exact)}
    fn owed(&self,area:PixelRect,changed:PixelRect)->PixelRect {if !self.derived {PixelRect::EMPTY}else if self.reduced {area.intersect(changed)}else {area}}
    fn keyed(&self,area:PixelRect)->Option<&Recorded> {self.shown.as_ref()?.recorded.as_ref().filter(|recorded|recorded.written.intersect(area)==area)}
    fn pending(&self)->bool {!self.derived && (self.queued || self.shown.as_ref().is_none_or(|shown|!shown.exact))}
    fn parked_by(&self,class:Class)->bool {class!=Class::Refinement || self.current && self.shown.as_ref().is_some_and(|shown|!shown.exact)}
    fn refusal_applies(&self,refusal:&crate::residency::Blocked)->bool {self.parked_by(refusal.class())}
    fn applied_refusal(&self)->Option<&crate::residency::Blocked> {self.refusal.as_ref().filter(|refusal|self.refusal_applies(refusal))}
    fn refused(&self,r:&WgpuRasterizer)->bool {
        self.applied_refusal().is_some() && self.wait.as_ref().is_some_and(|wait|wait.refusing() && !matches!(wait.observe(r),crate::frame_demand::Turn::Enter(_)))
    }
    fn parked(&self,r:&WgpuRasterizer)->bool {self.refused(r) && self.refusal.as_ref().is_some_and(|refusal|!refusal.class().floor_reserved())}
    fn waiting(&self,r:&WgpuRasterizer,held:bool)->Option<crate::frame_demand::Input> {
        use crate::frame_demand::Turn;
        let wait=self.wait.as_ref()?;
        if wait.refusing() && self.refusal.is_some() && !(held && self.applied_refusal().is_some()) {return None;}
        match wait.observe(r) {Turn::Sleep(input)=>Some(input),Turn::Dormant=>Some(crate::frame_demand::Input::Residency),Turn::Enter(_)|Turn::Deliver=>None}
    }
    fn naming(&self,r:&WgpuRasterizer)->bool {
        self.wait.as_ref().is_some_and(|wait|wait.naming() && matches!(wait.observe(r),crate::frame_demand::Turn::Sleep(_)))
    }
    fn allowance(&self,r:&WgpuRasterizer)->crate::frame_demand::Allowance {self.wait.as_ref().and_then(|wait|wait.eligible(r)).unwrap_or(crate::frame_demand::Allowance::Fresh)}
    fn release(&mut self) {
        self.refused_request=None;
        if self.refusal.take().is_some() && self.wait.as_ref().is_some_and(crate::frame_demand::UnitWait::refusing) {self.wait=None;}
    }
    fn derive(&mut self,derived:bool) {
        if derived==self.derived {return;}
        *self=if derived {Self {current:true,derived:true,..Self::default()}}else {Self {queued:true,..Self::default()}};
    }
}
#[derive(Default)]
pub(super) struct Pairing {held:Vec<PixelRect>,live:Vec<(PixelRect,Option<crate::frame_demand::UnitWait>)>}
impl Pairing {
    pub(super) fn awaits(&self,region:PixelRect)->Option<Option<crate::frame_demand::UnitWait>> {
        if self.held.contains(&region) {return None;}
        self.live.iter().find(|(cell,_)|cell.intersect(region)==region).map(|(_,wait)|wait.clone())
    }
}
#[derive(Default)]
pub(super) struct Cells {cells:Vec<Cell>,template:Option<Key>,input_level:u32,position:u64,frames:u64,unconfirmed:Vec<(submission::Submission,usize,Option<Shown>)>,unconfirmed_reductions:Vec<(submission::Submission,usize)>}
impl Cells {
    fn index(plan:display_mips::Plan,coordinate:[u32;2])->usize {
        let side=1<<plan.level;
        let [x,y]=[plan.bounds.min_x()/side/PAGE_SIZE,plan.bounds.min_y()/side/PAGE_SIZE];
        let width=plan.bounds.max_x().div_ceil(side).div_ceil(PAGE_SIZE)-x;
        ((coordinate[1]-y)*width+coordinate[0]-x) as usize
    }
    fn reached(plan:display_mips::Plan,area:PixelRect)->Vec<usize> {
        let area=area.intersect(plan.bounds);
        if area.is_empty() {Vec::new()}else {coordinates(area,plan.level).map(|coordinate|Self::index(plan,coordinate)).collect()}
    }
    fn sync(&mut self,store:&mut Store,template:&Key,plan:display_mips::Plan,input_level:u32) {
        store.confirm();
        let count=coordinates(plan.bounds,plan.level).count();
        let regrouped=self.template.as_ref().filter(|held|Key {expression:template.expression.clone(),..(*held).clone()}==*template).map(|held|held.expression!=template.expression);
        let same=self.cells.len()==count && regrouped.is_some() && self.input_level==input_level;
        match store.changes.since(self.position).filter(|_|same) {
            None=> {
                if self.cells.len()!=count {self.cells.clear();self.cells.resize_with(count,Cell::default);self.unconfirmed.clear();self.unconfirmed_reductions.clear();}
                for cell in &mut self.cells {if same {cell.stale();} else {cell.invalidate();}}
            },
            Some(events)=>for change in events {
                match change {
                    Change::Reset=>for cell in &mut self.cells {cell.invalidate();},
                    Change::Page(key) if key.source.is_none() && key.extent==plan.extent && key.level==plan.level && key.expression==template.expression=>
                        for index in Self::reached(plan,key.plan().bounds) {
                            let cell=&mut self.cells[index];
                            if !cell.current || key.format==wgpu::TextureFormat::Rgba32Float && cell.shown.as_ref().is_some_and(|shown|!shown.exact) {cell.queue();}
                        },
                    Change::Keys(level,region,bound) if *level==plan.level=>for index in Self::reached(plan,region.in_frame(plan.extent)) {self.cells[index].reach(*bound,template);},
                    _=>{},
                }
            },
        }
        if regrouped==Some(true) {for cell in &mut self.cells {cell.queue();cell.lag=None;}}
        for (submission,index,previous) in self.unconfirmed.iter_mut().rev() {
            let cell=&mut self.cells[*index];
            if submission.discarded() && cell.shown.as_ref().is_some_and(|shown|shown.submission.same(submission)) {
                cell.shown=previous.take().filter(|shown|!shown.submission.discarded());cell.invalidate();
            }
        }
        self.unconfirmed.retain(|(submission,..)|submission.pending());
        for (submission,index) in &self.unconfirmed_reductions {
            if submission.discarded() && let Some(cell)=self.cells.get_mut(*index) {cell.reduced=false;}
        }
        self.unconfirmed_reductions.retain(|(submission,..)|submission.pending());
        self.template=Some(template.clone());self.input_level=input_level;self.position=store.changes.end();self.frames=store.frames;
    }
    pub(super) fn derivations(&self,plan:display_mips::Plan,changed:PixelRect)->PixelRect {
        self.cells.iter().zip(coordinates(plan.bounds,plan.level))
            .map(|(cell,coordinate)|cell.owed(bounds(coordinate,plan.level,plan.extent).intersect(plan.bounds),changed))
            .fold(PixelRect::EMPTY,PixelRect::union)
    }
    pub(super) fn reduced(&mut self,plan:display_mips::Plan,region:PixelRect,changed:PixelRect,submission:submission::Submission,source:Option<layer_core::raster::ContentStamp>) {
        for (index,(cell,coordinate)) in self.cells.iter_mut().zip(coordinates(plan.bounds,plan.level)).enumerate() {
            let owed=cell.owed(bounds(coordinate,plan.level,plan.extent).intersect(plan.bounds),changed);
            if owed.is_empty() {continue;}
            let whole=region.intersect(owed)==owed;
            if whole {self.unconfirmed_reductions.push((submission.clone(),index));}
            cell.reduced=whole;cell.reduced_from=source.filter(|_|whole);
        }
    }
}
pub(super) struct Write {
    pub key: Key,
    pub region: PixelRect,
    pub accuracy: Accuracy,
    accepted:Option<Accepted>,
    pub(super) witness:Option<super::contact::Witness>,
}
impl Write {
    pub(super) fn keyless(key:Key,region:PixelRect)->Self {Self {key,region,accuracy:Accuracy::Display,accepted:None,witness:None}}
}
impl RegionRecords {
    fn projection_ready(&self,r:&WgpuRasterizer,format:wgpu::TextureFormat)->bool {
        let pipeline=&r.scene_pipelines.scale.output(format).project;
        r.compilation.require([pipeline],startup::DOCUMENT)
    }
    fn project(&mut self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,image:&Image,output:&wgpu::TextureView,
        plan:display_mips::Plan,region:PixelRect,key:&Key,color:layer_core::color::DocumentColor,blend:layer_core::BlendSpace,
    )->Result<(wgpu::BindGroup,wgpu::BindGroup,u32,[u32;2],PixelRect),GpuRasterError> {
        let target_side=1<<plan.level;let source_side=(1<<image.plan.level) as f32;
        let [x,y,width,height]=paint_transform::texel_rect(region.window_local(plan.bounds),target_side);
        let matrix=key.color.space.linear_transform(color.space);
        let origin=[(region.min_x()-image.plan.bounds.min_x()) as f32/source_side,(region.min_y()-image.plan.bounds.min_y()) as f32/source_side];
        let mut bytes=[0;96];
        for (destination,value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip([x,y,width,height,origin[0].to_bits(),origin[1].to_bits(),(target_side as f32/source_side).to_bits(),working_color::space_id(key.color.space) as u32]
            .into_iter().chain(matrix.into_iter().flat_map(|row|row.map(|value|(value as f32).to_bits()).into_iter().chain([0])))
            .chain([u32::from(key.blend_space==layer_core::BlendSpace::Perceptual),u32::from(blend==layer_core::BlendSpace::Perceptual),0,0])) {
            *destination=value.to_le_bytes();
        }
        let offset=self.write(r,encoder,&bytes)?;
        let binding=Self::binding(r,&image.view,&r.empty_view,output);
        Ok((self.record_binding.clone(),binding,offset,[width,height],PixelRect::new(x,y,x+width,y+height)))
    }
    fn project_batch(&mut self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,copies:&[(&Projection,&Image)],
        color:layer_core::color::DocumentColor,blend:layer_core::BlendSpace,
    )->Result<(),GpuRasterError> {
        let mut records=Vec::with_capacity(copies.len());
        for (copy,output) in copies {
            records.push(if copy.image.plan.level==output.plan.level && copy.image.texture.format()==output.texture.format()
                && copy.key.color==color && copy.key.blend_space==blend {None}
                else {Some(self.project(r,encoder,&copy.image,&output.view,output.plan,copy.region,&copy.key,color,blend)?)});
        }
        let mut pass:Option<wgpu::ComputePass<'static>>=None;
        let mut written=Vec::<(wgpu::Texture,PixelRect)>::new();
        for ((copy,output),record) in copies.iter().zip(records) {
            if let Some((records,binding,offset,[width,height],area))=record {
                if written.iter().any(|(texture,previous)|*texture==output.texture && !previous.intersect(area).is_empty()) {drop(pass.take());written.clear();}
                let pass=pass.get_or_insert_with(||encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label:Some("project retained composite pages"),timestamp_writes:None}).forget_lifetime());
                pass.set_pipeline(&r.scene_pipelines.scale.output(output.texture.format()).project);
                pass.set_bind_group(0,&records,&[offset]);pass.set_bind_group(1,&binding,&[]);
                pass.dispatch_workgroups(width.div_ceil(8),height.div_ceil(8),1);written.push((output.texture.clone(),area));
            } else {
                drop(pass.take());written.clear();
                let side=1<<output.plan.level;
                let [sx,sy,width,height]=paint_transform::texel_rect(copy.region.window_local(copy.image.plan.bounds),side);
                let [x,y,_,_]=paint_transform::texel_rect(copy.region.window_local(output.plan.bounds),side);
                encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:sx,y:sy,z:0},..copy.image.texture.as_image_copy()},
                    wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x,y,z:0},..output.texture.as_image_copy()},wgpu::Extent3d {width,height,depth_or_array_layers:1});
            }
        }
        Ok(())
    }

}
impl Cache {
    pub(super) fn required_input_level(&self)->u32 {if self.evaluation==Evaluation::Native {0}else {self.plan.level}}
    pub(crate) fn waiting_for_name(&self,r:&WgpuRasterizer)->bool {
        self.waiting_cells(r) || self.presented_matches_expected().is_none()
            || self.overview.as_ref().is_some_and(|overview|overview.waiting_for_name(r))
    }
    pub(super) fn presented_matches_expected(&self)->Option<bool> {
        if self.gesture.is_some() {return Some(self.cells.lock().unwrap().cells.iter().all(|cell|cell.derived || !cell.queued));}
        let Some(frame)=self.pages.lock().unwrap().expected().cloned() else {return Some(false);};
        self.presents(&frame)
    }
    fn presents(&self,frame:&Arc<SignatureFrame>)->Option<bool> {
        let Some(expression)=self.graph.content.as_ref().or(self.graph.root.as_ref()) else {return Some(false);};
        let key=self.page_key(expression.clone(),self.plan.bounds);
        if let Some(matches)=self.pixels.committed_root().and_then(|root|root.current_at(frame,&key)) {return Some(matches);}
        let held=self.pixels.committed_root().and_then(|root|root.authored.map(|authored|(authored,root.versions.as_slice())));
        publication_matches(frame,&key,self.plan.bounds,self.required_input_level(),held)
    }
    pub(super) fn hold_current(&mut self,frame:&Arc<SignatureFrame>) {
        if self.presents(frame)==Some(true) {self.pixels.hold_current(frame);}
    }
    pub(super) fn page_key(&self, expression: graph::Node, region: PixelRect) -> Key {
        let span=PAGE_SIZE<<self.plan.level;
        let expression=if self.graph.root.as_ref()==Some(&expression) {self.graph.content.clone().unwrap_or(expression)}else {expression};
        Key {source:None,expression,extent:self.plan.extent,color:self.color,blend_space:self.graph.blend_space(),level:self.plan.level,
            coordinate:[region.min_x()/span,region.min_y()/span],format:self.precision.format()}
    }
    pub(super) fn project_above(&mut self,scene:&Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder,
        commands:&mut RegionRecords,
    )->Result<(),GpuRasterError> {
        let required_input_level=self.required_input_level();
        let Some(above)=self.graph.motion.as_ref().map(|halves|halves[0].clone()) else {return Ok(());};
        if graph::Expression::is_clear(&above) {return Ok(());}
        let Some(region)=r.vector_motion_region(packet.document_extent).map(|region|paint_transform::aligned(region,1<<self.plan.level,self.plan.extent)
            .intersect(self.plan.bounds)).filter(|region|!region.is_empty()) else {return Ok(());};
        let plan=display_mips::Plan::window(self.plan.extent,self.plan.level,region);
        let mut copies=Vec::new();let mut revision=0;let mut missing=Vec::new();
        {
            let mut store=self.pages.lock().unwrap();
            for coordinate in coordinates(region,self.plan.level) {
                let mut key=self.page_key(above.clone(),bounds(coordinate,self.plan.level,self.plan.extent));
                let area=key.plan().bounds.intersect(region);
                let image=if let Some(image)=store.get(&key,area,Accuracy::Display,required_input_level,Class::Visible) {image}else {
                    key.format=wgpu::TextureFormat::Rgba32Float;
                    let Some(image)=store.get(&key,area,Accuracy::Exact,0,Class::Visible) else {missing.push(coordinate);continue;};image
                };
                revision=revision.max(store.pages[&key].written_at);
                copies.push(store.selected(&key,area,image));
            }
        }
        if !missing.is_empty() {
            let mut cells=self.cells.lock().unwrap();
            for coordinate in missing {if let Some(cell)=cells.cells.get_mut(Cells::index(self.plan,coordinate)) {cell.shown=None;cell.invalidate();}}
            return Ok(());
        }
        if self.above_revision.as_ref()==Some(&(above.clone(),revision))
            && self.above.committed_root().is_some_and(|root|root.image.plan==plan) {return Ok(());}
        if copies.iter().any(|copy|copy.image.texture.format()!=self.precision.format()) && !commands.projection_ready(r,self.precision.format()) {return Ok(());}
        if self.above.root().is_none_or(|image|image.plan!=plan || image.texture.format()!=self.precision.format()) {
            let bytes=plan.size.map(u64::from).into_iter().product::<u64>()*u64::from(self.precision.format().block_copy_size(None).unwrap());
            let Ok(reservation)=self.reserve_window(r,scene,bytes,crate::frame_demand::Allowance::Fresh) else {return Ok(());};
            let window=reservation.charge();
            let old=self.above.take();let bytes=old.iter().map(Image::bytes).sum();
            let charge=crate::DisplayRetirement {_charge:r.retired_display_bytes.charge(bytes),
                #[cfg(not(target_arch="wasm32"))] _scratch:(Vec::new(),old)};
            #[cfg(target_arch="wasm32")] drop(old);
            encoder.retire(charge);
            self.above.ensure_root(r,plan,self.precision.format(),"moving paths foreground projection",window);
        }
        let output=self.above.root().unwrap();
        commands.project_batch(r,encoder,&copies.iter().map(|copy|(copy,output)).collect::<Vec<_>>(),self.color,self.graph.blend_space())?;
        let store=self.pages.lock().unwrap();
        for copy in &copies {copy.image.clone().track(encoder);}
        let versions=copies.iter().flat_map(|copy|copy.versions.iter().cloned()).collect();
        let authored=store.expected().map(|frame|frame.authored_revision(&above)).transpose()?;
        self.above.commit_root(encoder.submission(),copies.into_iter().map(|copy|copy.image).collect(),versions,None,authored);
        self.above_revision=Some((above,revision));
        Ok(())
    }
    pub(super) fn select_cells(&self,r:&WgpuRasterizer,visible:PixelRect,presented:Accuracy,class:&dyn Fn(PixelRect)->Class)->Result<Selection,GpuRasterError> {
        let mut selection=Selection {copies:Vec::new(),composes:Vec::new(),transition:false,held:None};
        let Some((mut store,mut cells,template))=self.cells() else {return Ok(selection);};
        let frame=store.expected().cloned().ok_or(GpuRasterError::Invariant("signature frame is missing or inconsistent"))?;
        selection.transition=self.pixels.committed_root().is_some_and(|root|root.authored()!=frame.authored_revision(&template.expression).ok());
        let (plan,input_level)=(self.plan,self.required_input_level());
        for (index,coordinate) in coordinates(plan.bounds,plan.level).enumerate() {
            let owed=presented==Accuracy::Exact && !cells.cells[index].derived && cells.cells[index].shown.as_ref().is_some_and(|shown|!shown.exact);
            if !(cells.cells[index].queued || owed) {continue;}
            let region=bounds(coordinate,plan.level,plan.extent);
            let hold=(!region.intersect(visible).is_empty()).then_some(index);
            let key=Key {coordinate,..template.clone()};
            let expected=frame.expected_key(&key,region,input_level);
            if let Ok(expected)=&expected {cells.cells[index].select(expected.version());}
            if cells.cells[index].refused(r) {selection.held=selection.held.or(hold);continue;}
            if cells.cells[index].sleeping() {for (lease,_) in &cells.cells[index].waits {lease.stamp(class(region));}selection.held=selection.held.or(hold);continue;}
            let cell=&mut cells.cells[index];
            match expected {
                Ok(expected) if expected.version().resolved().is_some()=>if cell.wait.as_ref().is_some_and(|wait|!wait.refusing()) {cell.wait=None;},
                Ok(expected)=>{cell.wait=Some(crate::frame_demand::UnitWait::input(crate::frame_demand::Producer::Name(expected.into_version())));selection.held=selection.held.or(hold);continue;},
                Err(error@(GpuRasterError::NotReady|GpuRasterError::Waiting(_)))=>{
                    cell.wait=Some(crate::frame_demand::UnitWait::pending(&error,crate::frame_demand::Producer::Declaration(frame.declarations())));
                    selection.held=selection.held.or(hold);continue;
                },
                Err(error)=>return Err(error),
            }
            let current=cell.shown.as_ref().filter(|shown|shown.recorded.as_ref().is_some_and(|recorded|Key {expression:key.expression.clone(),format:key.format,..recorded.key.clone()}==key
                && recorded.matches_as(&frame,&key.expression,input_level)==Some(true))).map(|shown|shown.exact);
            let exact_key=if store.pages.get(&key).is_some_and(|page|Coverage::covers(&page.coverage.exact,region)) {key.clone()}
                else {Key {format:wgpu::TextureFormat::Rgba32Float,..key.clone()}};
            let exact=(current!=Some(true)).then(||store.get(&exact_key,region,Accuracy::Exact,input_level,class(region)).map(|image|(exact_key.clone(),image,true))).flatten();
            let page=exact.or_else(||(current.is_none() && presented==Accuracy::Display).then(||store.get(&key,region,Accuracy::Display,input_level,class(region))
                .map(|image|(key.clone(),image,false))).flatten());
            match page.and_then(|(key,image,exact)|store.recorded(&key,region).map(|recorded|(key,image,exact,recorded))) {
                Some((key,image,exact,recorded))=>selection.copies.push((index,Projection {region,image,key,versions:vec![recorded]},exact)),
                None if current.is_some_and(|exact|exact || presented==Accuracy::Display)=>{cell.current=true;cell.queued=false;cell.lag=Some(PixelRect::EMPTY);},
                None=>{cell.current=current.is_some();selection.composes.push(region);},
            }
        }
        Ok(selection)
    }
    pub(super) fn ensure_window(&mut self,r:&mut WgpuRasterizer,scene:&Scene,encoder:&mut submission::CommandEncoder)->bool {
        let format=self.precision.format();
        if self.pixels.root().is_some_and(|image|image.texture.format()==format) {return true;}
        let bytes=self.plan.size.map(u64::from).into_iter().product::<u64>()*u64::from(format.block_copy_size(None).unwrap());
        let allowance=self.cell_allowance(r,self.plan.bounds);
        let reservation=match self.reserve_window(r,scene,bytes,allowance) {Ok(reservation)=>reservation,Err(refusal)=>{self.refuse(self.plan.bounds,&refusal,allowance);return false;}};
        let charge=reservation.charge();
        self.end_contact(encoder);
        let retired=self.pixels.take();
        if !retired.is_empty() {
            let bytes=retired.iter().map(Image::bytes).sum();
            encoder.retire(crate::DisplayRetirement {_charge:r.retired_display_bytes.charge(bytes),
                #[cfg(not(target_arch="wasm32"))] _scratch:(Vec::new(),retired)});
            self.forget_cells();
        }
        self.pixels.ensure_root(r,self.plan,format,"composition window",charge);
        true
    }
    pub(super) fn forget_cells(&self) {
        let mut cells=self.cells.lock().unwrap();
        for cell in &mut cells.cells {cell.shown=None;cell.reduced=false;cell.invalidate();}
        cells.unconfirmed.clear();cells.unconfirmed_reductions.clear();
    }
    pub(super) fn copy_cells(&mut self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,commands:&mut RegionRecords,
        copies:Vec<(usize,Projection,bool)>,
    )->Result<PixelRect,GpuRasterError> {
        if copies.is_empty() {return Ok(PixelRect::EMPTY);}
        let conversion=copies.iter().any(|(_,copy,_)|copy.image.texture.format()!=self.precision.format()
            || copy.key.level!=self.plan.level || copy.key.color!=self.color || copy.key.blend_space!=self.graph.blend_space());
        if conversion && !commands.projection_ready(r,self.precision.format()) {return Err(GpuRasterError::NotReady);}
        let output=self.pixels.root().ok_or(GpuRasterError::NotReady)?.clone();
        commands.project_batch(r,encoder,&copies.iter().map(|(_,copy,_)|(copy,&output)).collect::<Vec<_>>(),self.color,self.graph.blend_space())?;
        #[cfg(test)]
        crate::test_support::PassCounts {projected_copies:copies.len() as u64,destination_units:copies.len() as u64,..Default::default()}.record();
        let submission=encoder.submission();let mut changed=PixelRect::EMPTY;
        let mut cells=self.cells.lock().unwrap();
        for (index,copy,exact) in copies {
            copy.image.clone().track(encoder);changed=changed.union(copy.region);
            let recorded=copy.versions.into_iter().next().expect("a copied page records its key");
            let previous=std::mem::replace(&mut cells.cells[index],Cell::showing(Shown {recorded:Some(recorded),submission:submission.clone(),exact,direct:false,refused:false,witness:None})).shown;
            cells.unconfirmed.push((submission.clone(),index,previous));
        }
        drop(cells);
        let [x,y,width,height]=paint_transform::texel_rect(changed.window_local(self.plan.bounds),1<<self.plan.level);
        self.pixels.wrote(PixelRect::new(x,y,x+width,y+height));
        Ok(changed)
    }
    pub(super) fn show(&self,write:Write,submission:submission::Submission) {
        if write.key.level!=self.plan.level || write.accepted.is_none() && self.gesture.is_none() {return;}
        let mut cells=self.cells.lock().unwrap();
        let index=Cells::index(self.plan,write.key.coordinate);
        let previous=std::mem::replace(&mut cells.cells[index],Cell::showing(Shown {recorded:write.accepted.map(|accepted|accepted.recorded),submission:submission.clone(),exact:write.accuracy==Accuracy::Exact,direct:true,refused:false,witness:write.witness})).shown;
        cells.unconfirmed.push((submission,index,previous));
    }
    pub(super) fn commit_window(&mut self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        let Some((store,cells,template))=self.cells() else {return Ok(());};
        let frame=store.expected().cloned().ok_or(GpuRasterError::Invariant("signature frame is missing or inconsistent"))?;
        let authored=frame.authored_revision(&template.expression)?;
        let mut written=vec![0u32;(self.geometry.size() as usize-32)/4];
        for (index,cell) in cells.cells.iter().enumerate() {if cell.written() {written[index/32]|=1<<(index%32);}}
        if written.iter().all(|word|*word==0) {return Ok(());}
        let versions=cells.cells.iter().filter_map(|cell|cell.shown.as_ref().and_then(|shown|shown.recorded.clone())).collect::<Vec<_>>();
        let cell_count=cells.cells.len();
        let current=self.gesture.as_ref().filter(|_|cells.cells.iter().all(|cell|cell.derived || !cell.queued && cell.shown.is_some()))
            .map(|_|Current {frame:frame.clone(),derived:false});
        let plan=self.plan;
        let mut shown=cells.cells.iter().zip(coordinates(plan.bounds,plan.level)).filter(|(cell,_)|cell.written()).map(|(cell,coordinate)|(cell,bounds(coordinate,plan.level,plan.extent).intersect(plan.bounds)));
        let keyed=shown.clone().all(|(cell,area)|cell.keyed(area).is_some());
        let same=current.is_some() || shown.all(|(cell,area)|if cell.derived {cell.reduced_from==Some(authored)}
            else {cell.keyed(area).is_some_and(|recorded|authored_matches(std::slice::from_ref(recorded),&frame,authored,recorded.written))});
        drop((cells,store));
        let count=written.iter().map(|word|word.count_ones()).sum::<u32>();
        self.complete=count as usize==cell_count;
        let level=self.plan.level;let window=self.plan.bounds;
        let data=[count,window.min_x()>>level,window.min_y()>>level,window.max_x().div_ceil(1<<level),window.max_y().div_ceil(1<<level)];
        #[cfg(test)]
        let observed_written=written.clone();
        r.uploads.write_at(encoder,&self.geometry,12,&data.into_iter().chain(written).flat_map(u32::to_le_bytes).collect::<Vec<_>>())?;
        self.pixels.commit_root(encoder.submission(),Vec::new(),versions,current,same.then_some(authored));
        self.pixels.certify_root(self.complete,(self.complete && self.graph.motion.is_none()).then(||self.graph.blend_space()),keyed);
        self.pixels.carry_to_next(encoder.submission());
        #[cfg(test)]
        {
            let root=self.pixels.committed_root().unwrap();
            crate::test_support::record_published_coverage(crate::test_support::coverage_hash(&self.geometry),&root.submission,crate::test_support::PublishedCoverage {
                plan:root.image.plan,receipt:root.submission.observation_id(),pages:Vec::new(),written:observed_written,
            });
        }
        Ok(())
    }
    pub(super) fn write_back(&mut self,r:&mut WgpuRasterizer,scene:&Scene,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        self.end_contact(encoder);
        let Some(window)=self.pixels.committed_root().map(|root|root.image().clone()) else {return Ok(());};
        let Some(frame)=self.pages.lock().unwrap().expected().cloned() else {return Ok(());};
        let side=1<<self.plan.level;
        let cells=self.cells.get_mut().unwrap();
        for cell in &mut cells.cells {
            let Some(shown)=cell.shown.as_mut().filter(|shown|shown.direct && !shown.submission.discarded()) else {continue;};
            let Some(recorded)=&shown.recorded else {continue;};
            if recorded.region!=recorded.key.plan().bounds || recorded.matches(&frame,recorded.input_level)!=Some(true) {continue;}
            let tier=self.pages.lock().unwrap().tier();
            let Ok(mut reservation)=r.admit_tier(Some(scene),&tier,Class::Visible,recorded.key.bytes(),crate::frame_demand::Allowance::Fresh) else {continue;};
            let mut store=self.pages.lock().unwrap();
            let Ok(image)=store.claim_reserved(r,recorded.key.clone(),self.window,Class::Visible,&mut reservation) else {continue;};
            let [x,y,width,height]=paint_transform::texel_rect(recorded.region.window_local(window.plan.bounds),side);
            let [px,py,_,_]=paint_transform::texel_rect(recorded.region.window_local(image.plan.bounds),side);
            encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x,y,z:0},..window.texture.as_image_copy()},
                wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:px,y:py,z:0},..image.texture.as_image_copy()},wgpu::Extent3d {width,height,depth_or_array_layers:1});
            image.track(encoder);
            #[cfg(test)]
            crate::test_support::PassCounts {written_back:1,..Default::default()}.record();
            let write=Write {key:recorded.key.clone(),region:recorded.region,accuracy:if shown.exact {Accuracy::Exact}else {Accuracy::Display},accepted:Some(Accepted {frame:frame.clone(),recorded:recorded.clone()}),witness:None};
            store.commit(write,0,encoder);
            shown.direct=false;
        }
        Ok(())
    }
}
fn lock_cells<'a>(cells:&'a std::sync::Mutex<Cells>,pages:&'a Pages)->(Locked<'a>,std::sync::MutexGuard<'a,Cells>) {
    let cells=cells.lock().unwrap();
    (pages.lock().unwrap(),cells)
}
impl Cache {
    fn cells(&self)->Option<(Locked<'_>,std::sync::MutexGuard<'_,Cells>,Key)> {
        let template=self.page_key(self.graph.root.clone()?,self.plan.bounds);
        let (mut store,mut cells)=lock_cells(&self.cells,&self.pages);
        cells.sync(&mut store,&template,self.plan,self.required_input_level());
        Some((store,cells,template))
    }
    #[cfg(test)]
    pub(crate) fn has_exact_window(&self)->bool {
        self.window_coverage_snapshot().ready(self.plan.bounds,Accuracy::Exact)
    }
    #[cfg(test)]
    pub(super) fn window_coverage_snapshot(&self)->Coverage {
        let mut coverage=Coverage::new(self.plan.level);
        let Some(root)=self.pixels.committed_root() else {return coverage;};
        let Some(published)=crate::test_support::published_coverage(crate::test_support::coverage_hash(&self.geometry)) else {return coverage;};
        if published.plan!=self.plan || published.receipt!=root.submission.observation_id() {return coverage;}
        let Some((store,cells,_))=self.cells() else {return coverage;};
        let Some(frame)=store.expected() else {return coverage;};
        for (index,(cell,coordinate)) in cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level)).enumerate() {
            if !published.written.get(index/32).is_some_and(|word|word&(1<<(index%32))!=0) || !cell.written() {continue;}
            coverage.written.entries.insert(coordinate);
            let area=bounds(coordinate,self.plan.level,self.plan.extent).intersect(self.plan.bounds);
            let Some(shown)=cell.shown.as_ref().filter(|shown|!shown.submission.discarded()) else {continue;};
            if !cell.queued && cell.current {
                let matches=shown.recorded.as_ref().filter(|recorded|recorded.written.intersect(area)==area)
                    .and_then(|recorded|recorded.matches(frame,self.required_input_level()));
                coverage.include(coordinate,matches,shown.exact);
            }
        }
        coverage
    }
    #[cfg(test)]
    pub(super) fn coverage_snapshot(&self)->Coverage {
        let mut coverage=Coverage::new(self.plan.level);
        let Some((store,_cells,template))=self.cells() else {return coverage;};
        for coordinate in coordinates(self.plan.bounds,self.plan.level) {
            let region=bounds(coordinate,self.plan.level,self.plan.extent).intersect(self.plan.bounds);let key=Key {coordinate,..template.clone()};
            if Coverage::covers(&store.written(&key,region),region) {coverage.written.entries.insert(coordinate);}
            store.include_matching(&key,region,self.required_input_level(),&mut coverage);
        }
        coverage
    }
    pub(super) fn coverage_complete(&self)->bool {
        self.cells().is_some_and(|(_,cells,_)|cells.cells.iter().all(Cell::written))
    }
    pub(crate) fn published(&self)->bool {self.presented_matches_expected()==Some(true) && self.coverage_complete()}
    pub(crate) fn refused_bytes(&self)->u64 {
        let mut refusals:Vec<crate::residency::Blocked>=Vec::new();
        self.collect_refusals(&mut refusals);
        refusals.iter().filter(|refusal|refusal.class().required()).map(crate::residency::Blocked::need).fold(0,u64::saturating_add)
    }
    fn collect_refusals(&self,refusals:&mut Vec<crate::residency::Blocked>) {
        let cells=self.cells.lock().unwrap();
        for refusal in cells.cells.iter().filter(|cell|cell.pending()).filter_map(Cell::applied_refusal) {
            if !refusals.iter().any(|seen|seen.same(refusal)) {refusals.push(refusal.clone());}
        }
        drop(cells);
        if let Some(overview)=&self.overview {overview.collect_refusals(refusals);}
    }
    pub(super) fn output_complete(&self)->bool {
        self.cells().is_some_and(|(_,cells,_)|cells.cells.iter().all(|cell|cell.derived || !cell.queued && cell.current && cell.shown.as_ref().is_some_and(|shown|shown.exact)))
    }
    pub(super) fn cells_demand(&self,r:&WgpuRasterizer,refining:bool)->Option<crate::frame_demand::Outstanding> {
        use crate::frame_demand::Outstanding::{Blocked,Runnable};
        let (_store,cells,_)=self.cells()?;
        let held=self.refusals_hold(r);
        let mut blocked=None;
        for cell in cells.cells.iter().filter(|cell|cell.pending() && !(held && cell.parked(r)) && !cell.sleeping() && (refining || cell.queued)) {
            match cell.waiting(r,held) {
                None=>return Some(Runnable),
                Some(input)=>blocked=blocked.or(Some(Blocked(input))),
            }
        }
        blocked
    }
    pub(super) fn block(&self,region:PixelRect,wait:Option<crate::frame_demand::UnitWait>) {
        let Some((_store,mut cells,_))=self.cells() else {return;};
        for index in Cells::reached(self.plan,region) {
            let cell=&mut cells.cells[index];
            if cell.pending() {
                #[cfg(test)] if crate::test_support::destination_wait_changed(cell.wait.as_ref(),wait.as_ref()) {crate::test_support::PassCounts {destination_wait_transitions:1,..Default::default()}.record();}
                cell.wait.clone_from(&wait);cell.queued=true;
            }
        }
    }
    pub(super) fn awaits_compose(&self,region:PixelRect)->bool {
        let cells=self.cells.lock().unwrap();
        Cells::reached(self.plan,region).into_iter().filter_map(|index|cells.cells.get(index)).any(|cell|cell.pending() && !cell.parked_by(Class::Refinement))
    }
    pub(super) fn follow(&self,held:usize,regions:&[PixelRect]) {
        let Some((_store,mut cells,_))=self.cells() else {return;};
        let Some(cell)=cells.cells.get(held) else {return;};
        let (wait,waits)=(cell.wait.clone(),cell.waits.clone());
        for index in regions.iter().flat_map(|region|Cells::reached(self.plan,*region)).filter(|index|*index!=held) {
            let cell=&mut cells.cells[index];
            if cell.pending() {cell.wait.clone_from(&wait);cell.waits.clone_from(&waits);cell.queued=true;}
        }
    }
    pub(super) fn cell_allowance(&self,r:&WgpuRasterizer,region:PixelRect)->crate::frame_demand::Allowance {
        let cells=self.cells.lock().unwrap();
        let spent=Cells::reached(self.plan,region).into_iter().filter_map(|index|cells.cells.get(index)).any(|cell|cell.allowance(r)==crate::frame_demand::Allowance::Spent);
        if spent {crate::frame_demand::Allowance::Spent}else {crate::frame_demand::Allowance::Fresh}
    }
    pub(super) fn sleep(&self,region:PixelRect,waits:Vec<super::super::object_cache::ObjectWait>) {
        if !waits.iter().any(|(_,waiter)|waiter.is_some()) {return;}
        let Some((_store,mut cells,_))=self.cells() else {return;};
        for index in Cells::reached(self.plan,region) {
            let cell=&mut cells.cells[index];
            if cell.pending() {cell.waits.clone_from(&waits);}
        }
    }
    pub(super) fn residual_demand(&self,r:&WgpuRasterizer)->Option<crate::frame_demand::Outstanding> {
        use crate::frame_demand::{Input,Outstanding};
        let Some((_store,cells,_))=self.cells() else {return Some(Outstanding::Runnable);};
        let mut pending=cells.cells.iter().filter(|cell|cell.pending()).peekable();
        if self.refusals_hold(r) && pending.clone().any(|cell|cell.parked(r)) {Some(Outstanding::Blocked(Input::Residency))}
        else if pending.peek().is_some() && pending.all(Cell::sleeping) {None}
        else {Some(Outstanding::Runnable)}
    }
    pub(super) fn ask(&mut self,asked:Asked) {
        if self.asked.replace(asked).is_some_and(|earlier|!asked.no_higher_than(earlier)) {
            for cell in &mut self.cells.lock().unwrap().cells {cell.release();cell.waits.clear();}
        }
    }
    pub(super) fn refusals_hold(&self,r:&WgpuRasterizer)->bool {
        let next=r.artwork_frame.as_ref().map(|frame|Asked {view:frame.view,moving:!super::refinement::refining(r)});
        self.asked.is_none_or(|earlier|next.is_some_and(|next|next.no_higher_than(earlier)))
    }
    pub(super) fn refused(&self,r:&WgpuRasterizer,region:PixelRect)->bool {
        let cells=self.cells.lock().unwrap();
        let reached=Cells::reached(self.plan,region);
        !reached.is_empty() && reached.into_iter().all(|index|cells.cells.get(index).is_some_and(|cell|cell.refused(r)))
    }
    pub(super) fn shown_from(&self,bounds:PixelRect,frame:&SignatureFrame)->BTreeSet<[u32;2]> {
        let Some((_store,cells,template))=self.cells() else {return BTreeSet::new();};
        let input_level=self.required_input_level();
        page_coordinates(bounds).filter(|coordinate| {
            let page=page_rect(*coordinate).intersect(bounds);
            Cells::reached(self.plan,page).into_iter().all(|index|cells.cells[index].keyed(page)
                .is_some_and(|recorded|recorded.matches_as(frame,&template.expression,input_level)==Some(true)))
        }).collect()
    }
    #[cfg(test)]
    pub(super) fn refused_need(&self,region:PixelRect)->Option<u64> {
        let cells=self.cells.lock().unwrap();
        Cells::reached(self.plan,region).into_iter().find_map(|index|cells.cells.get(index)?.refusal.clone().map(|refusal|refusal.alone().need()))
    }
    #[cfg(test)]
    pub(super) fn evict_window(&self,area:PixelRect) {
        let Some((_store,mut cells,_))=self.cells() else {return;};
        for index in Cells::reached(self.plan,area) {cells.cells[index].shown=None;cells.cells[index].invalidate();}
    }
    pub(super) fn refuse(&self,region:PixelRect,refusal:&crate::residency::Blocked,allowance:crate::frame_demand::Allowance) {
        if let Some((_store,mut cells,_))=self.cells() {
            for index in Cells::reached(self.plan,region) {
                let cell=&mut cells.cells[index];
                if cell.pending() && cell.refusal_applies(refusal) {
                    #[cfg(test)] let previous=cell.wait.clone();
                    (cell.refusal,cell.wait)=(Some(refusal.clone()),Some(crate::frame_demand::UnitWait::parked(refusal.clone(),allowance)));cell.queued=true;
                    cell.refused_request=cell.selected.clone();
                    #[cfg(test)] if crate::test_support::destination_wait_changed(previous.as_ref(),cell.wait.as_ref()) {crate::test_support::PassCounts {destination_wait_transitions:1,..Default::default()}.record();}
                }
            }
        }
        if refusal.class()==Class::Refinement && let Some(overview)=&self.overview {overview.refuse(region,refusal,allowance);}
    }
    #[cfg(test)]
    pub(super) fn release(&self,region:PixelRect) {
        let Some((_store,mut cells,_))=self.cells() else {return;};
        for index in Cells::reached(self.plan,region) {cells.cells[index].release();}
    }
    pub(super) fn owe_exact(&self) {
        let mut cells=self.cells.lock().unwrap();
        for cell in cells.cells.iter_mut().filter(|cell|!cell.derived && cell.shown.as_ref().is_some_and(|shown|!shown.exact)) {cell.queued=true;cell.lag=None;}
    }
    pub(super) fn displays(&self,region:PixelRect)->bool {
        self.cells().is_some_and(|(_,cells,_)|coordinates(region,self.plan.level).all(|coordinate|
            cells.cells.get(Cells::index(self.plan,coordinate)).is_some_and(|cell|cell.current && cell.shown.is_some())))
    }
    pub(super) fn waiting_cells(&self,r:&WgpuRasterizer)->bool {
        self.cells().is_some_and(|(_,cells,_)|cells.cells.iter().any(|cell|cell.naming(r)))
    }
    #[cfg(test)]
    pub(super) fn keyless(&self,region:PixelRect)->bool {self.keyless_where(region,|_|true)}
    pub(super) fn keyless_unrefused(&self,region:PixelRect)->bool {self.keyless_where(region,|shown|!shown.refused)}
    fn keyless_where(&self,region:PixelRect,accept:impl Fn(&Shown)->bool)->bool {
        self.cells().is_some_and(|(_,cells,_)|coordinates(region,self.plan.level).all(|coordinate|
            cells.cells.get(Cells::index(self.plan,coordinate)).is_some_and(|cell|cell.shown.as_ref().is_some_and(|shown|shown.recorded.is_none() && accept(shown)))))
    }
    pub(super) fn shown_writes(&self,regions:&[PixelRect])->Vec<(Option<submission::Submission>,Option<PixelRect>)> {
        let Some((_store,cells,_))=self.cells() else {return vec![(None,None);regions.len()];};
        regions.iter().map(|region|match Cells::reached(self.plan,*region)[..] {
            [index]=>(cells.cells[index].shown.as_ref().filter(|shown|shown.unnamed()).map(|shown|shown.submission.clone()),cells.cells[index].lag),
            _=>(None,None),
        }).collect()
    }
    pub(super) fn contact_cells(&self)->Vec<PixelRect> {
        let Some((_store,cells,_))=self.cells() else {return Vec::new();};
        cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level)).filter(|(cell,_)|cell.queued && !cell.derived)
            .map(|(_,coordinate)|bounds(coordinate,self.plan.level,self.plan.extent)).collect()
    }
    pub(super) fn stale_cells(&self)->Vec<PixelRect> {
        let Some((_store,cells,_))=self.cells() else {return Vec::new();};
        cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level)).filter(|(cell,_)|cell.queued && !cell.derived && (!cell.current || cell.shown.as_ref().is_none_or(|shown|shown.recorded.is_none())))
            .map(|(_,coordinate)|bounds(coordinate,self.plan.level,self.plan.extent)).collect()
    }
    pub(super) fn cell_reads(&self,target:SourceTarget,regions:&[PixelRect])->crate::IdMap<[u32;2],Arc<[PixelRect]>> {
        let Some((store,_cells,template))=self.cells() else {return Default::default();};
        let Some(frame)=store.expected() else {return Default::default();};
        let (plan,input_level)=(self.plan,self.required_input_level());
        let cells=regions.iter().flat_map(|region|coordinates(region.intersect(plan.bounds),plan.level)).map(|coordinate|(coordinate,bounds(coordinate,plan.level,plan.extent))).collect::<Vec<_>>();
        for (coordinate,region) in &cells {drop(frame.expected_key(&Key {coordinate:*coordinate,..template.clone()},*region,input_level));}
        frame.cell_reads(target,&template.expression,plan.level,input_level,&cells)
    }
    pub(super) fn key_contact_cells(&self,frame:&SignatureFrame,proven:&dyn Fn(Option<&super::contact::Witness>)->Result<(),Option<PixelRect>>) {
        let Some((_store,mut cells,template))=self.cells() else {return;};
        let (plan,input_level)=(self.plan,self.required_input_level());
        for (cell,coordinate) in cells.cells.iter_mut().zip(coordinates(plan.bounds,plan.level)) {
            let Some(shown)=cell.shown.as_mut().filter(|shown|shown.unnamed()) else {continue;};
            let region=bounds(coordinate,plan.level,plan.extent);
            match proven(shown.witness.as_ref()).and_then(|()|materialize(frame,&Key {coordinate,..template.clone()},region,input_level).map_err(|_|None)) {
                Ok(recorded)=>{shown.recorded=Some(recorded);shown.witness=None;},
                Err(lag)=>{shown.refused=true;cell.invalidate();cell.lag=lag;},
            }
        }
    }
    pub(super) fn reached_cells(&self,region:DocRect)->Vec<usize> {Cells::reached(self.plan,region.in_frame(self.plan.extent))}
    pub(super) fn invalidate_cells(&self,indices:Vec<usize>,bound:Option<PixelRect>) {
        let Some((_store,mut cells,template))=self.cells() else {return;};
        for index in indices {if let Some(cell)=cells.cells.get_mut(index) {cell.reach(bound,&template);}}
    }
    #[cfg(test)]
    pub(super) fn projected_cells(&self)->Vec<PixelRect> {
        let template=self.page_key(self.graph.root.clone().unwrap(),self.plan.bounds);
        let cells=self.cells.lock().unwrap();
        cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level)).filter(|(cell,_)|cell.shown.as_ref().is_some_and(|shown|!shown.composition(&template)))
            .map(|(_,coordinate)|bounds(coordinate,self.plan.level,self.plan.extent)).collect()
    }
    pub(super) fn requeue_keyless_cells(&self) {
        let mut cells=self.cells.lock().unwrap();
        for cell in cells.cells.iter_mut().filter(|cell|cell.shown.as_ref().is_some_and(|shown|shown.recorded.is_none())) {cell.invalidate();}
    }
    pub(super) fn has_direct_cells(&self)->bool {
        self.cells.lock().unwrap().cells.iter().any(|cell|cell.shown.as_ref().is_some_and(|shown|shown.direct))
    }
    pub(crate) fn hole_area(&self,visible:PixelRect)->PixelRect {
        let outside=if self.plan.bounds.intersect(visible)==visible {PixelRect::EMPTY}else {visible};
        if self.pixels.committed_root().is_none() {return visible;}
        let Some((_store,cells,_))=self.cells() else {return visible;};
        cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level)).filter(|(cell,_)|!cell.written())
            .map(|(_,coordinate)|bounds(coordinate,self.plan.level,self.plan.extent).intersect(visible)).fold(outside,PixelRect::union)
    }
    pub(super) fn holes(&self,visible:PixelRect)->bool {
        self.cells().is_some_and(|(_,cells,_)|cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level))
            .any(|(cell,coordinate)|!cell.written() && !bounds(coordinate,self.plan.level,self.plan.extent).intersect(visible).is_empty()))
    }
    pub(super) fn derive_cells(&self,finer:Option<(&Cache,PixelRect)>) {
        let Some((finer,changed))=finer.filter(|(finer,_)|finer.plan.level<self.plan.level) else {
            let mut cells=self.cells.lock().unwrap();
            for cell in &mut cells.cells {cell.derive(false);}
            return;
        };
        let source=finer.pixels.committed_root().and_then(CommittedImage::authored);
        let presented={
            let cells=finer.cells.lock().unwrap();
            let sized=cells.cells.len()==coordinates(finer.plan.bounds,finer.plan.level).count();
            let committed=finer.pixels.committed_root().is_some();
            move|area:PixelRect,derived:bool|sized && finer.plan.bounds.intersect(area)==area && (committed || derived) && coordinates(area,finer.plan.level).all(|coordinate| {
                let cell=&cells.cells[Cells::index(finer.plan,coordinate)];
                if derived {cell.written()}else {cell.current && cell.shown.is_some()}
            })
        };
        let Some((_store,mut cells,_))=self.cells() else {return;};
        for (cell,coordinate) in cells.cells.iter_mut().zip(coordinates(self.plan.bounds,self.plan.level)) {
            let (area,derived)=(bounds(coordinate,self.plan.level,self.plan.extent).intersect(self.plan.bounds),cell.derived);
            cell.derive(presented(area,derived));
            if cell.reduced && area.intersect(changed).is_empty() {cell.reduced_from=source;}
        }
    }
    pub(super) fn absorptions(&self,finer:&Cache,changed:PixelRect)->Vec<PixelRect> {
        if finer.plan.level>=self.plan.level || changed.is_empty() {return Vec::new();}
        let side=1u32<<self.plan.level;
        let align=|area:PixelRect|PixelRect::new(area.min_x()/side*side,area.min_y()/side*side,
            area.max_x().div_ceil(side)*side,area.max_y().div_ceil(side)*side).intersect(self.plan.bounds);
        let finer_cells=finer.cells.lock().unwrap();
        if finer_cells.cells.len()!=coordinates(finer.plan.bounds,finer.plan.level).count() {return Vec::new();}
        let written=|area:PixelRect|!area.is_empty() && finer.plan.bounds.intersect(area)==area
            && coordinates(area,finer.plan.level).all(|coordinate|finer_cells.cells[Cells::index(finer.plan,coordinate)].written());
        let Some((_store,mut cells,_))=self.cells() else {return Vec::new();};
        let mut parts=Vec::new();
        for (cell,coordinate) in cells.cells.iter_mut().zip(coordinates(self.plan.bounds,self.plan.level)) {
            if cell.derived || cell.current {continue;}
            let Some(shown)=cell.shown.as_mut() else {continue;};
            let area=align(bounds(coordinate,self.plan.level,self.plan.extent).intersect(changed));
            let found:Vec<_>=if written(area) {vec![area]} else {
                coordinates(area,finer.plan.level).map(|coordinate|align(bounds(coordinate,finer.plan.level,finer.plan.extent).intersect(area))).filter(|part|written(*part)).collect()
            };
            if found.is_empty() {continue;}
            shown.recorded=None;
            parts.extend(found);
        }
        parts
    }
    pub(super) fn produces_for(&self,reader:&Cache)->bool {
        self.transform.is_none() && self.plan.bounds==PixelRect::full(self.plan.extent) && reader.plan.bounds==self.plan.bounds && self.plan.level<=reader.plan.level
            && self.cells().is_some_and(|(_,cells,_)|cells.cells.iter().any(Cell::live))
    }
    pub(super) fn owes_native(&self,r:&WgpuRasterizer,view:layer_render::ViewState,region:PixelRect)->bool {
        let refining=super::refinement::refines_at(r,view);
        self.plan.level==0 && self.plan.bounds.intersect(region)==region
            && self.cells().is_some_and(|(_,cells,_)|Cells::reached(self.plan,region).into_iter().all(|index|cells.cells[index].awaits_native(refining)))
    }
    pub(super) fn pairing(&self)->Pairing {
        let held=self.paired.iter().map(|(region,..)|*region).collect();
        let live=self.cells().map_or_else(Vec::new,|(_,cells,_)|cells.cells.iter().zip(coordinates(self.plan.bounds,self.plan.level)).filter(|(cell,_)|cell.live())
            .map(|(cell,coordinate)|(bounds(coordinate,self.plan.level,self.plan.extent),cell.wait.clone())).collect());
        Pairing {held,live}
    }
    pub(super) fn current_window(&self,reader:&Cache)->Option<WindowSource> {
        #[cfg(test)] crate::test_support::PassCounts {navigator_checks:1,..Default::default()}.record();
        if self.transform.is_some() || self.plan.bounds!=PixelRect::full(self.plan.extent) || reader.plan.bounds!=self.plan.bounds {return None;}
        let store=self.pages.lock().unwrap();let frame=store.expected()?;
        let key=reader.page_key(reader.graph.content.as_ref().or(reader.graph.root.as_ref())?.clone(),reader.plan.bounds);
        [self.pixels.committed_root(),self.pixels.committed_next()].into_iter().flatten().filter(|window|window.image.plan.level<=reader.plan.level)
            .filter_map(|window|match window.current_at(frame,&key) {
                Some(true)=>Some((window,window.current.clone().map(|current|Current {derived:true,..current}))),
                _=>(publication_matches(frame,&key,reader.plan.bounds,reader.required_input_level(),
                    window.authored.map(|authored|(authored,window.versions.as_slice())))==Some(true)).then_some((window,None)),
            })
            .max_by_key(|(window,_)|window.image.plan.level)
            .map(|(window,current)|WindowSource {image:window.image.clone(),versions:window.versions.clone(),current,authored:window.authored})
    }
    pub(super) fn contact_window(&self,contact:u64,reader:&Cache)->Option<WindowSource> {
        if self.contact_id()!=Some(contact) || self.transform.is_some() || self.plan.bounds!=PixelRect::full(self.plan.extent) || reader.plan.bounds!=self.plan.bounds {return None;}
        let store=self.pages.lock().unwrap();let frame=store.expected()?;
        [self.pixels.committed_root(),self.pixels.committed_next()].into_iter().flatten()
            .filter(|window|window.image.plan.level<=reader.plan.level && window.authored.is_some()
                && window.current.as_ref().is_some_and(|current|Arc::ptr_eq(&current.frame,frame)))
            .max_by_key(|window|window.image.plan.level)
            .map(|window|WindowSource {image:window.image.clone(),versions:window.versions.clone(),
                current:window.current.clone().map(|current|Current {derived:true,..current}),authored:window.authored})
    }
    pub(super) fn inherit(&mut self,scene:&Scene,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,commands:&mut RegionRecords,source:WindowSource,region:PixelRect)->Result<PixelRect,GpuRasterError> {
        let reduced=self.reduce_from(scene,r,encoder,commands,&source.image,region)?.ok_or(GpuRasterError::NotReady)?;
        for cell in &mut self.cells.lock().unwrap().cells {cell.shown=None;cell.invalidate();}
        self.pixels.commit_root(encoder.submission(),Vec::new(),source.versions,source.current,source.authored);
        Ok(reduced)
    }
    #[cfg(test)]
    pub(in crate::scene) fn retained_ready(&self)->bool {self.ready_at(Accuracy::Display)}
    #[cfg(test)]
    pub(in crate::scene) fn ready_at(&self,accuracy:Accuracy)->bool {
        self.cells().is_some_and(|(_,cells,_)|cells.cells.iter().all(|cell|cell.derived
            || !cell.queued && cell.current && (accuracy==Accuracy::Display || cell.shown.as_ref().is_some_and(|shown|shown.exact))))
    }
}

#[cfg(test)]
pub(super) mod query_tests {
    use super::*;
    use super::super::tests::paint_mut;
    use crate::test_support::packet;

    #[test]
    fn deselecting_paths_releases_the_store_pages_that_name_them() {
        use layer_core::authored::vector::*;
        let extent=[1024u32,512];
        let records=crate::vector::bench::scene_b([1024.,512.],200).into_iter().map(Record::Path).collect();
        let mut editor=layer_core::Editor::new(layer_core::Document::from_artwork(layer_core::authored::Artwork::new(extent).unwrap()).unwrap());
        let (layer,edit)=editor.document().create_vector_layer_edit("Ink",VectorObject::from_records(records),None,0).unwrap();editor.perform(edit).unwrap();
        let mut r=WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        let frame=packet(editor.document().scene(),extent);
        crate::test_support::submit_settled(&mut r,frame);
        let naming=|r:&WgpuRasterizer|r.scale_display.as_ref().unwrap().pages.lock().unwrap().retained_pages().iter().filter(|page|graph::Expression::moves_other_than(&page.key.expression,&[])).count();
        r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection {layer,items:Arc::new((1..=20).map(ItemId).collect())}));
        crate::test_support::submit_settled(&mut r,FramePacket {composite_all:false,..frame});
        assert!(naming(&r)>0,"selecting paths prepares pages that name them");
        r.set_vector_selection(None);
        crate::test_support::submit_settled(&mut r,FramePacket {composite_all:false,..frame});
        assert_eq!(naming(&r),0,"deselecting releases every page that names the paths");
    }
    #[test]
    fn a_stroke_frame_rederives_only_the_cells_its_dabs_touch_whatever_the_view_size_and_level_count() {
        let mut rederived=Vec::new();
        for extent in [[1024,512],[4096,1024]] {
            let doc=super::super::tests::document_at(extent);let target=doc.working.target.unwrap();
            let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
            frame.view.width_px=extent[0]/4;frame.view.height_px=extent[1]/4;frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
            crate::test_support::submit_published(&mut r,frame);
            frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
            crate::test_support::submit_published(&mut r,frame);
            crate::test_support::submit_ready(&mut r,frame).unwrap();
            let settled=super::super::tests::cell_keys::cached_cells(&r,r.scale_display.as_ref().unwrap());
            let selections=|r:&WgpuRasterizer| {
                let cache=r.scale_display.as_ref().unwrap();
                let cells=table(cache);
                coordinates(cache.plan.bounds,cache.plan.level).zip(&cells.cells).map(|(coordinate,cell)|(coordinate,!cell.queued,
                    cell.shown.as_ref().and_then(|shown|shown.recorded.as_ref().map(|recorded|recorded.version.identity())))).collect::<Vec<_>>()
            };
            let before=selections(&r);
            let dab=crate::tests::test_dab([300.,300.],[0.9,0.1,0.3,1.],0.8);
            let batch=crate::test_support::dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
            crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
            let cache=r.scale_display.as_ref().unwrap();
            let levels=cache.pages.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.source.is_none()).map(|page|page.key.level).collect::<BTreeSet<_>>();
            assert!(levels.len()>1,"the Store holds composite pages at several levels: {levels:?}");
            let after=selections(&r);
            assert!(after.iter().all(|(_,settled,_)|*settled),"a stroke frame leaves no cell queued");
            let changed=before.iter().zip(&after).filter(|(before,after)|before.2!=after.2).map(|(_,(coordinate,_,_))| {
                let region=bounds(*coordinate,cache.plan.level,cache.plan.extent);
                [i64::from(region.min_x()),i64::from(region.min_y()),i64::from(region.max_x()),i64::from(region.max_y())]
            }).collect::<BTreeSet<_>>();
            assert_eq!(changed,super::super::tests::cell_keys::touched(&batch,&settled),"a stroke frame re-selects exactly the cells its dabs touch out of {}",after.len());
            rederived.push((after.len(),changed.len()));
        }
        assert_eq!(rederived[0].1,rederived[1].1,"the cells a stroke re-derives do not grow with the view: {rederived:?}");
    }

    #[test]
    fn a_stroke_inside_a_zoomed_view_recomposes_only_its_view_cells_and_derives_the_overview_whatever_the_view_size() {
        let mut recomposed=Vec::new();
        for extent in [[4096,2048],[8192,2048]] {
            let doc=super::super::tests::document_at(extent);let target=doc.working.target.unwrap();
            let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            layer_render::CanvasRenderer::set_working_layer(&mut r,doc.working.occurrence);
            let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
            frame.view.width_px=extent[0]/8;frame.view.height_px=extent[1]/8;frame.view.document_to_surface=[0.125,0.,0.,0.125,0.,0.];
            crate::test_support::submit_published(&mut r,frame);
            frame.view.width_px=extent[0]/4;frame.view.height_px=extent[1]/2;frame.view.document_to_surface=[0.5,0.,0.,0.5,0.,0.];
            crate::test_support::submit_settled(&mut r,frame);
            super::super::tests::cell_keys::begin_frame(&mut r,frame);
            let settled=super::super::tests::cell_keys::cached_cells(&r,r.scale_display.as_ref().unwrap());
            let before=super::super::tests::cell_keys::expected(&r);
            let dab=crate::tests::test_dab([300.,300.],[0.9,0.1,0.3,1.],0.8);
            let batch=crate::test_support::dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
            crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
            let mut composed=r.metrics.frame_composited_pages.clone();
            while super::super::tests::publication_pending(&r) {
                r.wait_idle().unwrap();crate::test_support::submit_ready(&mut r,frame).unwrap();composed.extend(r.metrics.frame_composited_pages.iter().copied());
            }
            let cache=r.scale_display.as_ref().unwrap();let level=cache.plan.level;
            assert!(level>0 && cache.overview.is_some(),"the view of {extent:?} reduces its sources at level {level} and keeps an overview");
            let touched=super::super::tests::cell_keys::touched(&batch,&settled);
            assert_eq!(super::super::tests::cell_keys::replanned(cache,&settled,&before,&super::super::tests::cell_keys::expected(&r)),touched,"the stroke re-plans exactly the view cells its dabs touch");
            let expected=touched.iter().flat_map(|region|page_coordinates(PixelRect::new(region[0] as u32,region[1] as u32,region[2] as u32,region[3] as u32)).map(move|page|(level,page))).collect::<Vec<_>>();
            let composed=composed.into_iter().filter(|(composed,_)|*composed==level).collect::<Vec<_>>();
            assert!(composed.len()==expected.len() && composed.iter().all(|page|expected.contains(page)),"the stroke on {extent:?} composes exactly the touched view pages at level {level}: {composed:?} {expected:?}");
            let overview=cache.overview.as_deref().unwrap();
            let template=overview.page_key(overview.graph.root.clone().unwrap(),overview.plan.bounds);
            let mut cells=overview.cells.lock().unwrap();
            cells.sync(&mut overview.pages.lock().unwrap(),&template,overview.plan,overview.required_input_level());
            let damage=PixelRect::new(batch.damage.min.x.floor() as u32,batch.damage.min.y.floor() as u32,batch.damage.max.x.ceil() as u32,batch.damage.max.y.ceil() as u32);
            for (coordinate,cell) in coordinates(overview.plan.bounds,overview.plan.level).zip(&cells.cells) {
                assert!(!cell.queued,"the stroke re-derives no overview cell: {coordinate:?}");
                assert!(cell.derived || bounds(coordinate,overview.plan.level,extent).intersect(damage).is_empty(),"the overview derives the cells under the stroke: {coordinate:?}");
            }
            recomposed.push(touched.len());
        }
        assert_eq!(recomposed[0],recomposed[1],"the cells a stroke recomposes do not grow with the view: {recomposed:?}");
    }

    #[test]
    fn a_stroke_at_fit_composes_and_projects_only_view_pages() {
        let extent=[1333,1000];let doc=super::super::tests::document_at(extent);let target=doc.working.target.unwrap();
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
        frame.view.width_px=400;frame.view.height_px=300;frame.view.document_to_surface=[0.3,0.,0.,0.3,0.,0.];
        crate::test_support::submit_settled(&mut r,frame);
        let level=r.scale_display.as_ref().unwrap().plan.level;
        assert!(level>0,"Fit composes above level 0");
        let overview=|r:&WgpuRasterizer|r.scale_display.as_ref().unwrap().overview.as_ref().and_then(|overview|overview.pixels.committed_root().map(|root|root.submission.observation_id()));
        for step in 0..8u8 {
            let mut dab=crate::tests::test_dab([200.+120.*f32::from(step),500.],[0.9,0.1,0.3,1.],0.8);dab.radii=[120.;2];
            let batch=crate::test_support::dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
            let before=overview(&r);
            crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
            assert!(r.metrics.frame_composited_pages.iter().all(|(composed,_)|*composed==level),"stroke frame {step} composes only view pages at level {level}: {:?}",r.metrics.frame_composited_pages);
            assert_eq!(overview(&r),before,"stroke frame {step} projects no overview pages");
        }
    }

    fn settled_view(document:&layer_core::Document)->(WgpuRasterizer,FramePacket<'_>) {
        let extent=document.composition().size;
        let mut frame=packet(document.scene(),extent);frame.composite_all=false;
        frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        crate::test_support::submit_settled(&mut r,frame);
        r.wait_idle().unwrap();crate::test_support::complete(&r);
        (r,frame)
    }
    fn project(r:&mut WgpuRasterizer)->PixelRect {
        let mut cache=r.scale_display.take().unwrap();let scene=r.scene.take().unwrap();
        let mut commands=RegionRecords::new(r);
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let changed=cache.copy_level(&scene,r,&mut encoder,&mut commands).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(r);
        r.scene=Some(scene);r.scale_display=Some(cache);
        changed
    }
    fn show_cell(r:&WgpuRasterizer,coordinate:[u32;2],encoder:&mut submission::CommandEncoder)->Key {
        let cache=r.scale_display.as_ref().unwrap();
        let template=cache.page_key(cache.graph.root.clone().unwrap(),cache.plan.bounds);
        let key=Key {coordinate,..template};let area=key.plan().bounds;
        let write=cache.pages.lock().unwrap().prepare_write(key.clone(),area,Accuracy::Display,cache.required_input_level()).unwrap();
        cache.show(write,encoder.submission());
        key
    }
    fn quiet(cell:&Cell)->bool {!cell.queued && cell.current && cell.shown.is_some()}
    fn table(cache:&Cache)->std::sync::MutexGuard<'_,Cells> {
        let template=cache.page_key(cache.graph.root.clone().unwrap(),cache.plan.bounds);
        let mut cells=cache.cells.lock().unwrap();
        cells.sync(&mut cache.pages.lock().unwrap(),&template,cache.plan,cache.required_input_level());
        cells
    }

    #[cfg(not(target_arch="wasm32"))]
    fn refinement_pressure_view(document:&layer_core::Document)->(WgpuRasterizer,FramePacket<'_>,crate::residency::Charge,crate::residency::Blocked) {
        let extent=document.composition().size;
        let mut frame=packet(document.scene(),extent);frame.composite_all=false;frame.view.width_px=64;frame.view.height_px=64;
        let mut r=crate::memory::memory_tests::renderer(document.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(2<<30),..Default::default()}).unwrap();
        crate::test_support::submit_settled(&mut r,frame);crate::test_support::complete(&r);
        r.set_preparation_wake(Arc::new(||{}));
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let cache=r.scale_display.as_ref().unwrap();
        assert!(cache.overview.is_some(),"the fixture has a main window and an overview");
        for cache in std::iter::once(cache).chain(cache.overview.as_deref()) {
            assert!(cache.pixels.committed_root().is_some(),"each window has committed pixels");
            drop(table(cache));
            for coordinate in coordinates(cache.plan.bounds,cache.plan.level) {
                let area=bounds(coordinate,cache.plan.level,extent);
                let key=cache.page_key(cache.graph.root.clone().unwrap(),area);
                let write=cache.pages.lock().unwrap().prepare_write(key,area,Accuracy::Display,cache.required_input_level()).unwrap();
                cache.show(write,encoder.submission());
            }
        }
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        let display=r.scene_tiers.display.clone();
        let held=display.charge(display.grant().saturating_sub(display.used()));
        let refused=display.blocked(Class::Refinement,1).expect("the held grant refuses optional refinement");
        (r,frame,held,refused)
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_refinement_refusal_keeps_stale_or_absent_visible_cells_runnable_and_committable() {
        use crate::frame_demand::Outstanding::Runnable;
        let document=super::super::tests::document_at([2048,512]);
        for absent in [false,true] {
            let (mut r,frame,_held,refused)=refinement_pressure_view(&document);
            let cache=r.scale_display.as_ref().unwrap();let area=bounds(coordinates(cache.plan.bounds,cache.plan.level).next().unwrap(),cache.plan.level,cache.plan.extent);
            let expected=crate::test_support::float_pixels(&r,&cache.pixels.committed_root().unwrap().image.texture);
            cache.refuse(cache.plan.bounds,&refused,crate::frame_demand::Allowance::Fresh);
            {
                let mut cells=table(cache);let cell=&mut cells.cells[0];cell.invalidate();
                if absent {cell.shown=None;}
            }
            cache.refuse(area,&refused,crate::frame_demand::Allowance::Fresh);
            assert_eq!(cache.cells_demand(&r,true),Some(Runnable),"a refinement refusal cannot stop required visible work: absent={absent}");
            assert_eq!(r.next_frame(),layer_render::FrameDemand::Work,"required visible work requests a frame: absent={absent}");
            for _ in 0..2 {
                crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
                if r.scale_display.as_ref().unwrap().displays(area) {break;}
            }
            let cache=r.scale_display.as_ref().unwrap();
            assert!(cache.displays(area),"the required visible cell commits within two frames: absent={absent}");
            let actual=crate::test_support::float_pixels(&r,&cache.pixels.committed_root().unwrap().image.texture);
            assert!(crate::test_support::max_error(&actual,&expected)<1e-5,"the committed visible pixels survive refinement pressure: absent={absent}");
        }
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_parked_refinement_leaves_an_idle_renderer_settled_and_resumes_when_room_is_released() {
        let document=super::super::tests::document_at([2048,512]);
        let (mut r,frame,held,_)=refinement_pressure_view(&document);
        r.begin_frame_memory(frame);
        let refused=r.scene_tiers.display.blocked(Class::Refinement,1).expect("the held grant refuses optional refinement");
        let (wake,woken)=std::sync::mpsc::channel();
        r.set_preparation_wake(Arc::new(move||{let _=wake.send(());}));
        let main=r.scale_display.as_ref().unwrap();
        for cache in std::iter::once(main).chain(main.overview.as_deref()) {cache.refuse(cache.plan.bounds,&refused,crate::frame_demand::Allowance::Fresh);}
        assert!(std::iter::once(main).chain(main.overview.as_deref()).any(|cache|table(cache).cells.iter().any(|cell|cell.parked(&r))),"the fixture parks refused exactness");
        assert!(r.outstanding_terms().is_empty(),"parked optional work is not outstanding: {:?}",r.outstanding_terms());
        assert_eq!(r.next_frame(),layer_render::FrameDemand::Wait,"an idle renderer with parked refinement requests no frame");
        assert!(r.settled(),"parked optional work does not hold settledness");
        while woken.try_recv().is_ok() {}
        drop(held);
        woken.recv_timeout(std::time::Duration::from_secs(1)).expect("the release wakes the parked refinement");
        assert_eq!(r.next_frame(),layer_render::FrameDemand::Work,"released room resumes the parked refinement");
        crate::test_support::submit_settled(&mut r,frame);
        let main=r.scale_display.as_ref().unwrap();
        assert!(std::iter::once(main).chain(main.overview.as_deref()).all(|cache|table(cache).cells.iter().all(|cell|!cell.pending())),"the resumed refinement completes every cell");
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_cell_moved_onscreen_after_a_prefetch_refusal_asks_for_frames_until_it_is_exact() {
        use layer_render::FrameDemand::{Wait,Work};
        let document=super::super::tests::document_at([2048,512]);
        let extent=document.composition().size;
        let mut start=packet(document.scene(),extent);start.composite_all=false;start.view.width_px=64;start.view.height_px=300;
        let moved=FramePacket {view:layer_render::ViewState {document_to_surface:[1.,0.,0.,1.,-300.,0.],..start.view},..start};
        let onscreen=|frame:FramePacket<'_>|display_mips::view_bounds(frame.view,extent,0).unwrap();
        for refused_in_motion in [false,true] {
            let mut r=crate::memory::memory_tests::renderer(document.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(2<<30),..Default::default()}).unwrap();
            crate::test_support::submit_settled(&mut r,start);
            r.set_preparation_wake(Arc::new(||{}));
            let cache=r.scale_display.as_ref().unwrap();
            assert!(table(cache).cells.iter().all(|cell|!cell.pending()),"the fixture settles every cell exact");
            let [parked,composed]=[[1,0],[1,1]].map(|coordinate|bounds(coordinate,cache.plan.level,extent));
            assert!(parked.intersect(onscreen(start)).is_empty() && !parked.intersect(onscreen(moved)).is_empty(),"the parked cell enters the view");
            let refused=r.scene_tiers.display.blocked(Class::Prefetch,2<<30).expect("the prefetch does not fit");
            if refused_in_motion {
                cache.evict_window(composed);
                crate::test_support::submit_ready(&mut r,moved).unwrap();crate::test_support::complete(&r);
            }
            let cache=r.scale_display.as_ref().unwrap();
            cache.evict_window(parked);
            cache.refuse(parked,&refused,crate::frame_demand::Allowance::Fresh);
            if !refused_in_motion {
                assert_eq!(r.next_frame(),Wait,"an offscreen cell refused at Prefetch asks for no frame");
                crate::test_support::submit_ready(&mut r,moved).unwrap();crate::test_support::complete(&r);
            }
            let cache=r.scale_display.as_ref().unwrap();
            assert!(Cells::reached(cache.plan,parked).into_iter().all(|index|table(cache).cells[index].pending()),"the moved cell is still pending: refused_in_motion={refused_in_motion}");
            assert_eq!(r.next_frame(),Work,"an onscreen cell is asked at Visible after the motion ends, so its Prefetch refusal cannot park it: refused_in_motion={refused_in_motion}");
            crate::test_support::submit_settled(&mut r,moved);
            let cache=r.scale_display.as_ref().unwrap();
            assert!(Cells::reached(cache.plan,onscreen(moved)).into_iter().all(|index|!table(cache).cells[index].pending()),"every onscreen level-0 cell is exact at Wait: refused_in_motion={refused_in_motion}");
        }
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_required_cell_refused_again_after_its_release_wake_fired_is_woken_by_the_next_release() {
        use crate::frame_demand::{Input,Outstanding};
        let document=super::super::tests::document_at([2048,512]);
        let (mut r,_frame,held,refinement)=refinement_pressure_view(&document);
        let (wake,woken)=std::sync::mpsc::channel();
        r.set_preparation_wake(Arc::new(move||{let _=wake.send(());}));
        let display=r.scene_tiers.display.clone();let room=held.bytes();
        assert!(room>0,"the fixture holds room the refused cell needs");
        let bytes=display.limit()-display.used().saturating_sub(display.yielding())+1;
        let refused=display.blocked(Class::Visible,bytes).expect("the held room refuses the visible unit").alone();
        let main=r.scale_display.as_ref().unwrap();let overview=main.overview.as_deref().unwrap();
        overview.refuse(overview.plan.bounds,&refinement,crate::frame_demand::Allowance::Fresh);
        table(main).cells[0].invalidate();
        main.refuse(main.plan.bounds,&refused,crate::frame_demand::Allowance::Fresh);
        assert_eq!(r.next_frame(),layer_render::FrameDemand::Wait,"reading the refused cell's demand registers its release wake");
        let edge=r.device.retirement_handle();
        drop(held);edge.flush().unwrap();
        woken.try_recv().expect("the release wakes the refused cell");
        let retaken=display.charge(room);
        assert_eq!(r.outstanding_terms(),vec![("display",Outstanding::Blocked(Input::Residency))],"the cell still reports its refusal before it retried");
        assert_eq!(r.next_frame(),layer_render::FrameDemand::Wait,"a refused required cell waits for a release");
        while woken.try_recv().is_ok() {}
        drop(retaken);edge.flush().unwrap();
        woken.try_recv().expect("the next release wakes the cell that still reports its refusal");
        assert_eq!(r.next_frame(),layer_render::FrameDemand::Work,"the released room resumes the required cell");
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_refinement_refusal_in_either_window_cannot_hide_the_other_windows_required_cell() {
        use crate::frame_demand::Outstanding::Runnable;
        let document=super::super::tests::document_at([2048,512]);
        for main_required in [false,true] {
            let (r,_frame,_held,refused)=refinement_pressure_view(&document);
            let main=r.scale_display.as_ref().unwrap();let overview=main.overview.as_deref().unwrap();
            main.refuse(overview.plan.bounds,&refused,crate::frame_demand::Allowance::Fresh);
            assert_eq!(main.outstanding(&r),None,"refused optional exactness is parked, not outstanding");
            let (required,waiting)=if main_required {(main,overview)}else {(overview,main)};
            table(required).cells[0].invalidate();
            assert_eq!(waiting.cells_demand(&r,true),None,"the other window's refused exactness stays parked: main_required={main_required}");
            assert_eq!(required.cells_demand(&r,true),Some(Runnable),"required cells ignore an optional refinement refusal: main_required={main_required}");
            assert_eq!(main.outstanding(&r),Some(Runnable),"main and overview preserve runnable work in either direction: main_required={main_required}");
        }
    }

    #[test]
    fn a_required_store_page_above_the_display_grant_takes_a_ledger_reservation_beyond_the_store_share() {
        let document=super::super::tests::document_at([1024,512]);
        let extent=document.composition().size;
        let mut frame=packet(document.scene(),extent);frame.composite_all=false;
        frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
        let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        crate::test_support::submit_settled(&mut r,frame);crate::test_support::complete(&r);
        let cache=r.scale_display.as_ref().unwrap();let scene=r.scene.as_ref().unwrap();
        let key=cache.page_key(Arc::new(graph::Expression::Color([0,1f32.to_bits(),0,1f32.to_bits()])),page_rect([0;2]));
        assert!(cache.pages.lock().unwrap().allocated(&key,Class::Visible).is_none(),"the branch page is not stored yet");
        let _share=cache.pages.lock().unwrap().tier().charge(crate::memory::CACHE_BYTES);
        let display=r.scene_tiers.display.clone();
        let _grant=display.charge(r.memory.display_owner().grant().gpu_bytes.saturating_sub(display.used()));
        assert!(matches!(cache.claim_page(scene,&r,key.clone(),Class::Prefetch),Ok(Err(_))),"an optional page stays within the display grant");
        let claimed=cache.claim_page(scene,&r,key,Class::Visible).unwrap();
        assert!(claimed.is_ok(),"a required page the device holds was refused by the Store's own share: {:?}",claimed.err());
        assert!(r.scene_tiers.display.used()<=r.memory.display_owner().grant().gpu_bytes,"the page above the display grant is covered by evictions or a ledger reservation");
    }

    #[test]
    fn a_direct_cell_records_its_key_and_the_next_sync_rederives_nothing_for_it() {
        let document=super::super::tests::document_at([1024,512]);
        let (mut r,frame)=settled_view(&document);
        super::super::tests::cell_keys::begin_frame(&mut r,frame);
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let key=show_cell(&r,[1,0],&mut encoder);
        let cache=r.scale_display.as_ref().unwrap();
        let cells=table(cache);let cell=&cells.cells[Cells::index(cache.plan,key.coordinate)];
        assert!(quiet(cell) && cell.shown.as_ref().is_some_and(|shown|shown.direct && shown.recorded.as_ref().is_some_and(|recorded|recorded.key==key)),"the direct cell records its key");
    }

    #[test]
    fn a_discarded_direct_cell_keeps_its_previous_picture_and_is_queued_before_the_next_frame_begins() {
        let document=super::super::tests::document_at([1024,512]);
        let (r,_)=settled_view(&document);
        let cache=r.scale_display.as_ref().unwrap();
        let index=Cells::index(cache.plan,[1,0]);
        let settled=table(cache).cells[index].shown.as_ref().map(|shown|shown.submission.clone()).expect("the settled view shows the cell");
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        show_cell(&r,[1,0],&mut encoder);
        assert!(quiet(&table(cache).cells[index]),"the direct cell is shown while its encoder is open");
        drop(encoder);
        let cells=table(cache);let cell=&cells.cells[index];
        assert!(cell.shown.as_ref().is_some_and(|shown|shown.submission.same(&settled)) && cell.queued,"a discarded direct cell keeps its previous picture and is queued before the next frame begins");
    }

    #[test]
    fn a_discarded_direct_cell_rederives_exactly_its_cell_and_the_next_picture_matches_a_fresh_renderer() {
        let document=super::super::tests::document_at([1024,512]);
        let (mut r,frame)=settled_view(&document);
        let settled=|r:&mut WgpuRasterizer| {
            super::super::tests::cell_keys::begin_frame(r,frame);
            table(r.scale_display.as_ref().unwrap()).cells.iter().map(quiet).collect::<Vec<_>>()
        };
        settled(&mut r);let before=settled(&mut r);
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let key=show_cell(&r,[1,0],&mut encoder);
        drop(encoder);
        let after=settled(&mut r);
        let index=Cells::index(r.scale_display.as_ref().unwrap().plan,key.coordinate);
        assert!(!after[index],"the discarded direct cell is re-derived");
        assert!(before.iter().zip(&after).enumerate().all(|(cell,(before,after))|cell==index || !before || *after),"only the discarded cell is re-derived: {before:?} {after:?}");
        crate::test_support::submit_published(&mut r,frame);
        let (fresh,_)=settled_view(&document);
        let cache=r.scale_display.as_ref().unwrap();
        let error=super::super::tests::quality(&super::super::tests::display_pixels(&r),&super::super::tests::display_pixels(&fresh),cache.plan,cache.texture().format());
        assert!(error[2]<2e-5,"the picture after a discarded direct cell matches a fresh renderer: {error:?}");
    }

    #[test]
    fn a_reset_rechecks_keys_without_reprojecting_or_recopying_settled_cells() {
        let document=super::super::tests::document_at([1024,512]);
        let (mut r,_)=settled_view(&document);
        project(&mut r);
        r.scale_display.as_ref().unwrap().pages.lock().unwrap().changes.push(Change::Reset);
        assert_eq!(project(&mut r),PixelRect::EMPTY,"a Reset copies no settled cell into the window");
    }

    #[test]
    fn an_overflowed_change_log_reprojects_without_recopying_unchanged_cells() {
        let document=super::super::tests::document_at([1024,512]);
        let (mut r,_)=settled_view(&document);
        project(&mut r);
        {
            let cache=r.scale_display.as_ref().unwrap();let mut store=cache.pages.lock().unwrap();
            for _ in 0..=Changes::KEPT {store.changes.push(Change::Keys(cache.plan.level+1,DocRect::default(),None));}
        }
        assert_eq!(project(&mut r),PixelRect::EMPTY,"an overflowed log copies no unchanged cell into the window");
    }

    #[test]
    fn a_frame_without_changes_queues_no_cell_and_does_no_work() {
        let document=super::super::tests::document_at([1024,512]);
        let (mut r,frame)=settled_view(&document);
        super::super::tests::cell_keys::begin_frame(&mut r,frame);
        assert!(table(r.scale_display.as_ref().unwrap()).cells.iter().all(quiet),"a frame without changes leaves every settled cell out of Δ");
        let before=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(&mut r,frame).unwrap();
        let counts=crate::test_support::PassCounts::snapshot().delta(before);
        assert_eq!((counts.unit_acceptances,counts.projected_copies,counts.composite_commits),(0,0,0),"a frame with empty Δ composes, copies and commits nothing");
        assert!(r.metrics.frame_composited_pages.is_empty());
    }

    #[test]
    fn a_window_copy_that_its_frame_never_reduces_is_reduced_by_the_next_frame() {
        let document=super::super::tests::document_at([1024,512]);
        let (mut r,_)=settled_view(&document);
        super::super::tests::assert_presentation_mip(&r);
        let mut cache=r.scale_display.take().unwrap();let scene=r.scene.take().unwrap();
        let index=Cells::index(cache.plan,[1,0]);
        {let mut cells=table(&cache);cells.cells[index].shown=None;cells.cells[index].invalidate();}
        let mut commands=RegionRecords::new(&r);
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let selection=cache.select_cells(&r,cache.plan.bounds,Accuracy::Display,&|_|Class::Visible).unwrap();
        assert_eq!(selection.copies.len(),1,"the unshown cell copies its current Store page");
        assert!(!cache.copy_cells(&mut r,&mut encoder,&mut commands,selection.copies).unwrap().is_empty(),"the interrupted frame copies the cell");
        cache.commit_window(&mut r,&mut encoder).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        assert!(cache.copy_level(&scene,&mut r,&mut encoder,&mut commands).unwrap().is_empty(),"the next frame has nothing new to copy");
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        r.scene=Some(scene);r.scale_display=Some(cache);
        super::super::tests::assert_presentation_mip(&r);
    }

    #[test]
    fn a_discarded_encoder_of_another_renderer_leaves_this_renderers_cells_intact() {
        let extent=[1024,512];let document=super::super::tests::document_at(extent);
        let mut frame=packet(document.scene(),extent);frame.composite_all=false;
        frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        crate::test_support::submit_published(&mut r,frame);
        let frame_cells=|r:&mut WgpuRasterizer| {
            r.wait_idle().unwrap();crate::test_support::complete(r);
            super::super::tests::cell_keys::begin_frame(r,frame);
            let cache=r.scale_display.as_ref().unwrap();
            table(cache).cells.iter().map(quiet).collect::<Vec<_>>()
        };
        frame_cells(&mut r);
        let quiet=frame_cells(&mut r);
        assert!(quiet.iter().any(|quiet|*quiet),"a frame without changes leaves settled cells untouched: {quiet:?}");
        let other=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        let mut foreign=submission::CommandEncoder::new(&other.device,&Default::default());
        let discarded=foreign.submission();drop(foreign);assert!(discarded.discarded());
        let after=frame_cells(&mut r);
        assert!(quiet.iter().zip(&after).all(|(quiet,after)|!quiet || *after),"a foreign discarded submission re-derives none of the cells a plain frame leaves untouched: {quiet:?} {after:?}");
    }

    #[test]
    fn a_frame_streams_one_parent_while_its_sources_wait_and_publishes_once_they_arrive() {
        use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
        let extent=[2048,1024];
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let descriptor=document.composition().color.paint_descriptor();
        let tiles=(0..extent[0]/PAGE_SIZE).flat_map(|x|(0..extent[1]/PAGE_SIZE).map(move|y|[x,y])).map(|coordinate|(TileKey {plane:RasterPlane::Color,coordinate},
            RasterTile::backed(TileBlob::encode(descriptor,&[16,48,90,255].repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap()))).collect();
        paint_mut(&mut document,0).raster=RasterRevision::backed(RasterData {tiles,..Default::default()});
        let store=crate::test_support::GatedStore::new(&layer_core::Editor::new(document.clone()).retained_tiles(),false);
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        let frame=display_frame(&document,2);
        let check=|r:&WgpuRasterizer,attempt:usize| {
            let cache=r.scale_display.as_ref().unwrap();
            let parent=cache.page_key(cache.graph.root.clone().unwrap(),cache.plan.bounds);
            let streaming=cache.pages.lock().unwrap().inputs.iter().filter(|(key,_)|**key==Key {coordinate:key.coordinate,..parent.clone()})
                .flat_map(|(_,inputs)|inputs.iter().filter(|input|input.stream.pending()).map(|input|input.region)).collect::<Vec<_>>();
            assert!(streaming.len()<=1,"attempt {attempt} streams the sources of {streaming:?}; a cell waits for the parent ahead of it");
        };
        for attempt in 0..4 {crate::test_support::submit_ready(&mut r,frame).unwrap();check(&r,attempt);r.wait_idle().unwrap();}
        store.gate.release();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        for attempt in 4.. {
            crate::test_support::submit_ready(&mut r,frame).unwrap();check(&r,attempt);
            if !super::super::tests::publication_pending(&r) {break;}
            assert!(std::time::Instant::now()<deadline,"deferred composition must publish");
            r.wait_idle().unwrap();
        }
    }

    #[test]
    fn a_navigator_refreshes_on_its_own_once_every_cell_of_the_full_view_it_waits_for_is_parked() {
        use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
        let extent=[2048,1024];
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let descriptor=document.composition().color.paint_descriptor();
        let tiles=(0..extent[0]/PAGE_SIZE).flat_map(|x|(0..extent[1]/PAGE_SIZE).map(move|y|[x,y])).map(|coordinate|(TileKey {plane:RasterPlane::Color,coordinate},
            RasterTile::backed(TileBlob::encode(descriptor,&[16,48,90,255].repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap()))).collect();
        paint_mut(&mut document,0).raster=RasterRevision::backed(RasterData {tiles,..Default::default()});
        let store=crate::test_support::GatedStore::new(&layer_core::Editor::new(document.clone()).retained_tiles(),false);
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        crate::test_support::submit_ready(&mut r,display_frame(&document,2)).unwrap();
        assert!(r.navigator.outstanding(&r).is_none(),"the Navigator waits for the view producing the whole drawing");
        let cache=r.scale_display.as_ref().unwrap();
        cache.refuse(cache.plan.bounds,&r.scene_tiers.display.wanting(Class::Visible,1<<20),crate::frame_demand::Allowance::Spent);
        assert!(r.navigator.outstanding(&r).is_some(),"with every cell of that view parked no donor will come, so the Navigator asks to refresh on its own");
        drop(store);
    }

    #[test]
    fn a_stroke_across_two_cells_owes_both_while_their_sources_wait_and_draws_them_like_a_twin_once_they_arrive() {
        use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
        let extent=[2048,1024];
        let mut artwork=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Base".into(),paper:"Paper".into()}).artwork;
        let descriptor=layer_core::color::DocumentColor::default().paint_descriptor();
        let layers=(0..3u8).map(|layer| {
            let (occurrence,target)=crate::test_support::add_paint(&mut artwork,format!("Layer {layer}"),extent);
            let SourceTarget::Paint(paint)=target else {unreachable!()};
            let tiles=(0..extent[0]/PAGE_SIZE).flat_map(|x|(0..extent[1]/PAGE_SIZE).map(move|y|[x,y])).filter(|_|layer!=1).map(|coordinate|(TileKey {plane:RasterPlane::Color,coordinate},
                RasterTile::backed(TileBlob::encode(descriptor,&[40*layer,90,160,200].repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap()))).collect();
            artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(RasterData {tiles,..Default::default()});
            (occurrence,target)
        }).collect::<Vec<_>>();
        let mut document=layer_core::Document::from_artwork(artwork).unwrap();
        let (working,target)=layers[1];
        document.working.occurrence=Some(working);document.working.target=Some(target);
        let store=crate::test_support::GatedStore::new(&layer_core::Editor::new(document.clone()).retained_tiles(),false);
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        layer_render::CanvasRenderer::set_working_layer(&mut r,Some(working));
        let frame=display_frame(&document,2);
        crate::test_support::submit_ready(&mut r,frame).unwrap();r.wait_idle().unwrap();
        let style=crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
        let dabs=[[300.,300.],[1500.,700.]].map(|centre|crate::tests::test_dab(centre,[0.9,0.1,0.3,1.],0.8));
        let batch=|index:usize,kind,start,end|layer_render::DabBatch {kind,first_dab:index as u32,stroke_start:start,stroke_end:end,
            ..crate::test_support::dab_batch(target,style.clone(),dabs[index].bounds())};
        use layer_render::DabBatchKind::{Persistent,Preview};
        let (down,held,up)=([batch(0,Persistent,true,false),batch(1,Preview,false,false)],[batch(1,Preview,false,false)],[batch(1,Persistent,false,true)]);
        let frames=[(&down[..],0.01),(&held[..],0.02),(&up[..],0.03)].map(|(dab_batches,time_seconds)|FramePacket {dabs:&dabs,dab_batches,time_seconds,..frame});
        crate::test_support::submit_ready(&mut r,frames[0]).unwrap();
        let (plan,contact)=(r.scale_display.as_ref().unwrap().plan,r.scale_display.as_ref().unwrap().contact_id());
        let cells=dabs.map(|dab|bounds([dab.center.x as u32,dab.center.y as u32].map(|side|side/(PAGE_SIZE<<plan.level)),plan.level,plan.extent));
        let progress=|r:&WgpuRasterizer|r.scale_display.as_ref().unwrap().contact_declared_and_owed().unwrap_or_default();
        for step in 0..3 {
            let (declared,owed)=progress(&r);
            assert!(contact.is_some() && r.scale_display.as_ref().unwrap().contact_id()==contact && declared.is_empty() && cells.iter().all(|cell|owed.contains(cell)),
                "frame {step}: the contact begun at pen down owes both cells the stroke reaches while their sources wait: {cells:?}, declared {declared:?}, owed {owed:?}");
            crate::test_support::submit_ready(&mut r,frames[1]).unwrap();r.wait_idle().unwrap();
        }
        store.gate.release();
        let (signal,woken)=std::sync::mpsc::channel();
        r.set_preparation_wake(Arc::new(move||{let _=signal.send(());}));
        let deadline=std::time::Instant::now()+crate::test_support::WAIT_BOUND;
        loop {
            crate::test_support::submit_ready(&mut r,frames[1]).unwrap();crate::test_support::complete(&r);
            assert_eq!(r.scale_display.as_ref().unwrap().contact_id(),contact,"the held pen keeps its contact while the sources arrive");
            let (declared,owed)=progress(&r);
            if owed.is_empty() && cells.iter().all(|cell|declared.contains(cell)) && !super::super::tests::publication_pending(&r) {break;}
            assert!(std::time::Instant::now()<deadline,"the contact declares and shows both cells once their sources arrive: declared {declared:?}, owed {owed:?}");
            let _=woken.recv_timeout(std::time::Duration::from_millis(5));
        }
        let mut twin=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        layer_render::CanvasRenderer::set_working_layer(&mut twin,Some(working));
        crate::test_support::submit_settled(&mut twin,frame);
        let blank=super::super::tests::display_pixels(&twin);
        for packet in &frames[..2] {crate::test_support::submit_ready(&mut twin,FramePacket {composite_all:true,..*packet}).unwrap();}
        let at=|dab:&layer_render::Dab|((dab.center.y as u32>>plan.level)*plan.size[0]+(dab.center.x as u32>>plan.level)) as usize;
        let [shown,expected]=[&r,&twin].map(super::super::tests::display_pixels);
        for (dab,cell) in dabs.iter().zip(cells) {
            let ink=crate::test_support::max_error(&shown[at(dab)..=at(dab)],&blank[at(dab)..=at(dab)]);
            assert!(ink>0.05,"the held stroke shows its ink in cell {cell:?}: {ink}");
        }
        let error=crate::test_support::max_error(&shown,&expected);
        assert!(error<1e-4,"the held stroke differs from a twin that composes every frame by {error}");
        for r in [&mut r,&mut twin] {crate::test_support::submit_settled(r,frames[2]);}
        let error=crate::test_support::max_error(&super::super::tests::display_pixels(&r),&super::super::tests::display_pixels(&twin));
        assert!(error<1e-4,"after pen up the stroke differs from a twin that composed every frame by {error}");
        drop(store);
    }

    #[test]
    fn an_absent_hold_never_becomes_an_accepted_input_set() {
        let document=super::super::tests::document_at([256;2]);
        let mut store=fixture_store(packet(document.scene(),[256;2]));
        let key=Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent:[256;2],color:document.composition().color,
            blend_space:document.composition().blend,level:0,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
        let accepted=store.capture(&key,key.plan().bounds,0).unwrap();
        let result=store.take_inputs(&accepted);
        assert!(matches!(result,Err(GpuRasterError::Invariant(_))),"an absent hold must fail before an operation can consume it");
    }

    #[test]
    fn producer_interests_keep_owner_level_quality_and_region_together() {
        let key=Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent:[2048;2],color:Default::default(),
            blend_space:layer_core::BlendSpace::Linear,level:1,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
        let region=key.plan().bounds;
        let accepted=Interest::new(key.clone(),Evaluation::Display,region,0);
        let wanted=Interest::new(key.clone(),Evaluation::Display,PixelRect::full(key.extent),0);
        assert!(wanted.contains(&accepted));
        assert!(!Interest::new(key.clone(),Evaluation::Display,PixelRect::full(key.extent),1).contains(&accepted));
        assert!(!Interest::new(key.clone(),Evaluation::Native,PixelRect::full(key.extent),0).contains(&accepted));
        assert!(!Interest::new(Key {level:2,..key.clone()},Evaluation::Display,PixelRect::full(key.extent),0).contains(&accepted));
        assert!(!Interest::new(Key {format:wgpu::TextureFormat::Rgba16Float,..key.clone()},Evaluation::Display,PixelRect::full(key.extent),0).contains(&accepted));
        assert!(!Interest::new(key.clone(),Evaluation::Display,PixelRect::new(512,0,1024,512),0).contains(&accepted));
        let source=Key {source:Some(SourceTarget::Paint(layer_core::PaintHandle::from_index(0))),..key};
        let accepted=Interest::new(source.clone(),Evaluation::Display,region,1);
        assert!(Interest::new(source.clone(),Evaluation::Native,PixelRect::full(source.extent),1).contains(&accepted));
        assert!(!Interest::new(source,Evaluation::Display,PixelRect::new(0,0,256,256),1).contains(&accepted));
    }

    fn pending_display_and_exact_inputs(pressure:bool) {
            let document=super::super::tests::document_at([512;2]);
            let mut renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
            let frame=packet(document.scene(),document.composition().size);
            renderer.ensure_document_metadata(frame.document_extent,frame.scene).unwrap();
            renderer.begin_frame_memory(frame);renderer.publish_memory();
            let mut store=fixture_store(frame);let mut scene=Scene::new(&renderer).unwrap();
            let color=[31.,67.,113.].map(|channel|layer_core::color::RgbSpace::Srgb.decode(channel/255.) as f32);
            let key=Key {source:None,expression:Arc::new(graph::Expression::Color([color[0].to_bits(),color[1].to_bits(),color[2].to_bits(),1f32.to_bits()])),extent:[512;2],color:renderer.document_color(),
                blend_space:frame.blend_space,level:4,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
            let region=key.plan().bounds;
            let image=store.claim(&renderer,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
            let coarse=layer_core::color::source::rgba8_source([32;2],|_,_|[31,67,113,255]);
            let fine=layer_core::color::source::rgba8_source([512;2],|_,_|[31,67,113,255]);
            let coarse_gate=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([coarse.as_ref()]),false);
            let fine_gate=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([fine.as_ref()]),false);
            let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
            let display=Interest::new(key.clone(),Evaluation::Display,region,4);
            let mut requested=crate::scene::sources::Inputs::default();requested.image(&coarse,[0;2],renderer.device.working_space());
            let requested=requested.for_unit(crate::scene::sources::InputUnit::new(region,4)).unwrap();
            let sources=requested.acquire(&renderer,Class::Visible).unwrap();
            let read=crate::scene::sources::accepted_image(&sources,&coarse,[0;2]).unwrap();
            for pending in &read.pending {scene.enqueue_source_decode(pending.clone());}
            let mut unit=Unit {sources,..Default::default()};
            assert!(!unit.ready(&mut scene,&mut renderer,&mut encoder,0,Class::Visible).unwrap(),"the Display attempt must wait on a real decode");
            let accepted=store.accept_version(&display).unwrap().unwrap();
            store.accept(&display,vec![(key.clone(),region,image.clone())],accepted).unwrap();
            store.hold_inputs(&store.fixture_handle(&key,region,4),unit.sources).unwrap();
            scene.discard_unencoded_jobs();
            if pressure {crate::test_support::evict_sources(&mut renderer);}
            let exact=Interest::new(key.clone(),Evaluation::Display,region,0);
            let write=store.prepare_write(key.clone(),region,Accuracy::Exact,0).unwrap();drop(write);
            let accepted=store.accept_version(&exact).unwrap().unwrap();
            store.accept(&exact,vec![(key.clone(),region,image.clone())],accepted).unwrap();
            let mut requested=crate::scene::sources::Inputs::default();requested.image(&fine,[0;2],renderer.device.working_space());
            let requested=requested.for_unit(crate::scene::sources::InputUnit::new(region,0)).unwrap();
            let sources=requested.acquire(&renderer,Class::Visible).unwrap();
            let exact_read=crate::scene::sources::accepted_image(&sources,&fine,[0;2]).unwrap();
            for pending in &exact_read.pending {scene.enqueue_source_decode(pending.clone());}
            let mut unit=Unit {sources,..Default::default()};
            assert!(!unit.ready(&mut scene,&mut renderer,&mut encoder,0,Class::Visible).unwrap());
            store.hold_inputs(&store.fixture_handle(&key,region,0),unit.sources).unwrap();scene.discard_unencoded_jobs();
            store.retain_work(&[display.clone(),exact]);
            assert_eq!(store.inputs[&key].len(),2,"refinement must preserve the pending Display hold");
            assert!(store.inputs[&key].iter().any(|input|input.accepted.recorded.input_level==4 && !input.sources.is_empty()));
            coarse_gate.gate.release();
            crate::test_support::query_ready(&mut renderer,&mut encoder,|r,encoder|
                if read.encode(r,encoder,crate::residency::Class::Visible)? {Ok(())}else {Err(GpuRasterError::NotReady)});
            assert!(!exact_read.ready(),"Display must publish before the level-0 decode arrives");
            let write=store.prepare_write(key.clone(),region,Accuracy::Display,4).unwrap();
            encoder.copy_texture_to_texture(read.sampled().uncertified().texture.as_image_copy(),image.texture.as_image_copy(),
                wgpu::Extent3d {width:32,height:32,depth_or_array_layers:1});
            store.commit(write,0,&mut encoder);
            renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
            assert!(store.coverage(&key,region,4).ready(region,Accuracy::Display));
            assert!(store.get(&key,region,Accuracy::Exact,0,crate::residency::Class::Visible).is_none());
            assert_eq!(store.inputs[&key].len(),1);
            assert_eq!(store.inputs[&key][0].accepted.recorded.input_level,0,"a Display commit must keep the finer hold");
            assert!(!exact_read.ready());
            let actual=crate::test_support::float_pixels(&renderer,&image.texture);
            assert!(actual.iter().all(|pixel|(0..3).all(|c|(pixel[c]-color[c]).abs()<1e-5) && pixel[3]==1.));
            drop(fine_gate);
    }

    #[test]
    fn pending_display_and_exact_inputs_coexist_until_their_own_commits() {pending_display_and_exact_inputs(false);}

    #[test]
    fn pending_display_publishes_with_exact_inputs_pending_under_a_small_limit() {pending_display_and_exact_inputs(true);}

    pub(crate) fn assert_display_grant_decrease_reconciles_retention(r:&mut WgpuRasterizer,document:&layer_core::Document) {
        use crate::submission::ColorPass;
        let extent=document.composition().size;
        let mut frame=display_frame(document,0);frame.view.width_px=256;frame.view.height_px=256;
        r.memory_demand(frame);r.prepare_moving_projection(frame).unwrap();
        let mut scene=Scene::new(&r).unwrap();scene.scale_sources.prepare(&r,frame,&[]);
        let mut cache=Cache::new(&r,Request {plan:display_mips::Plan::at(extent,0),evaluation:Evaluation::Display},document.scene().order().len());
        crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
        cache.prepare_graph(&r,frame,&scene).unwrap();
        let root=cache.graph.root.clone().unwrap();
        let near=cache.page_key(root.clone(),page_rect([0,0]));
        let far=cache.page_key(root,page_rect([31,31]));
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        for key in [&near,&far] {
            let image=cache.pages.lock().unwrap().claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
            let write=cache.pages.lock().unwrap().prepare_write(key.clone(),key.plan().bounds,Accuracy::Display,key.level).unwrap();
            {let _pass=encoder.color_pass("accepted Display grant fixture",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE));}
            cache.pages.lock().unwrap().commit(write,0,&mut encoder);
            accept_fixture_inputs(&mut cache.pages.lock().unwrap(),key,key.plan().bounds,Evaluation::Display,vec![(key.clone(),key.plan().bounds,image)],key.level);
        }
        encoder.submit(&r.queue);crate::test_support::complete(&r);
        let far_pin=cache.pages.lock().unwrap().pages[&far].image.clone();
        let store_bytes=cache.pages.lock().unwrap().bytes();let mip_bytes=r.moving_images.storage_bytes();
        assert!(mip_bytes>=128<<20,"real optional mip retention exceeds the working floor");
        let report=r.display_memory_report(Some(&scene),cache.frame_memory(&r,0));
        r.memory.sync_report(layer_core::MemoryOwnerKind::Scene,report);
        let budget=r.factory.memory_budget().clone();budget.flush_reports();budget.set_ram_override(Some(384<<20));budget.flush_reports();
        let pressure=budget.admit_import(budget.import_headroom_bytes()).unwrap();
        assert!(r.memory.display_owner().grant().gpu_bytes>=report.pinned_gpu_bytes);
        assert!(r.memory.display_owner().grant().gpu_bytes<report.gpu_bytes,"the actual Core grant must fall below allocated retention");
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        r.memory.invalidate_limits();r.apply_memory_grants(Some(&mut scene),Some(&mut cache),&mut encoder);
        encoder.submit(&r.queue);crate::test_support::complete(&r);
        let report=r.display_memory_report(Some(&scene),cache.frame_memory(&r,0));
        assert!(r.moving_images.storage_bytes()<mip_bytes,"the existing optional mip victim path must reconcile at the owner boundary");
        assert!(report.gpu_bytes<=r.memory.display_owner().grant().gpu_bytes.max(report.pinned_gpu_bytes),"Display physical must reconcile within the frame completion");
        assert!(r.memory.display_owner().grant().gpu_bytes>=report.pinned_gpu_bytes);
        let pages=cache.pages.lock().unwrap();assert_eq!(pages.bytes(),store_bytes,"accepted Store pages are guards, not optional victims");
        assert!(pages.accepted(&near,near.plan().bounds,near.level));
        let held=pages.accepted_coordinates(&near,near.level).collect::<Vec<_>>();
        let visible=display_mips::view_bounds(frame.view,extent,0).unwrap();
        assert_eq!(nearest_pages(visible,frame.view,1,held,|_|true),vec![[0,0]]);
        assert!(Arc::strong_count(&far_pin)>1);
        drop(pages);drop(far_pin);drop(pressure);
    }

    fn display_frame(document:&layer_core::Document,level:u32)->FramePacket<'_> {
        let extent=document.composition().size;let side=1u32<<level;
        let mut frame=packet(document.scene(),extent);
        frame.blend_space=document.composition().blend;frame.composite_all=false;
        frame.view.width_px=extent[0].div_ceil(side);frame.view.height_px=extent[1].div_ceil(side);
        frame.view.document_to_surface=[1./side as f32,0.,0.,1./side as f32,0.,0.];frame
    }

    fn display(document:&layer_core::Document,source:&Arc<layer_core::color::source::SourceImage>,level:u32)->WgpuRasterizer {
        display_window(document,source,level,PixelRect::full(document.composition().size))
    }

    fn display_window(document:&layer_core::Document,source:&Arc<layer_core::color::source::SourceImage>,level:u32,window:PixelRect)->WgpuRasterizer {
        let extent=document.composition().size;
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        renderer.ensure_document_metadata(extent,document.scene()).unwrap();
        let frame=display_frame(document,level);
        crate::memory::memory_tests::legacy_scene_frame(&mut renderer,frame);
        pollster::block_on(renderer.compilation.prepare_all(renderer.scene_pipelines.for_scene(&renderer,document.scene(),&[])
            .chain(crate::startup::SourceKinds::for_scene(document.scene()).pipelines(&renderer.scene_pipelines.source)))).unwrap();
        let format=renderer.display_precision(document.scene()).format();
        pollster::block_on(renderer.compilation.prepare_all(renderer.scene_pipelines.scale_pipelines(format).into_iter()
            .chain([&renderer.scene_pipelines.scale.output(format).project,&renderer.scene_pipelines.scale.full.reduce,&renderer.scene_pipelines.constant.2,&renderer.scene_pipelines.fold.pipeline]))).unwrap();
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        for coordinate in page_coordinates(PixelRect::full(extent)) {
            if renderer.source_tiles.borrow().uploads_full(crate::residency::Class::Visible) {
                let uploaded=std::mem::replace(&mut encoder,submission::CommandEncoder::new(&renderer.device,&Default::default()));
                renderer.uploads.finish(&uploaded);uploaded.submit(&renderer.queue);crate::test_support::complete(&renderer);
            }
            let pixels=crate::prepare_image_tile(source,coordinate,renderer.device.working_space(),&renderer.device.source_samples,None).unwrap();
            Scene::decode_prepared_for(&mut renderer,&mut encoder,source,coordinate,pixels,
                crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
        }
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
        let mut scene=renderer.scene.take().unwrap_or_else(||Scene::new(&renderer).unwrap());
        let mut cache=Cache::new(&renderer,Request {plan:display_mips::Plan::window(extent,level,window),evaluation:Evaluation::Display},document.scene().order().len());
        let mut commands=RegionRecords::new(&renderer);
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        scene.begin_frame();commands.begin();scene.scale_sources.prepare(&renderer,frame,&[]);
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        {let mut work=crate::test_support::work(false);cache.render_graph(&mut scene,&mut renderer,frame,
            &mut Encoding {encoder:&mut encoder,commands:&mut commands, retained:0, budget:&mut work},Destination::View).unwrap();}
        scene.encode_jobs(&mut renderer,&mut encoder).unwrap();
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);renderer.wait_idle().unwrap();
        renderer.scene=Some(scene);renderer.scale_display=Some(cache);renderer
    }

    fn refine(renderer:&mut WgpuRasterizer,frame:FramePacket<'_>) {
        let mut scene=renderer.scene.take().unwrap();
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        {let mut work=crate::test_support::work(false);scene.refine_display(renderer,frame,&mut encoder,&mut work).unwrap();}
        renderer.scene=Some(scene);renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);renderer.wait_idle().unwrap();
    }

    #[test]
    fn an_authored_edit_to_an_image_object_layer_on_a_coarse_level_replaces_the_held_window() {
        use layer_core::authored::*;
        let extent=[1024,512];
        let mut artwork=Artwork::new(extent).unwrap();
        let source=layer_core::color::source::rgba8_source([512;2],|x,y|[(x/2) as u8,(y/2) as u8,113,255]);
        let object=artwork.objects.insert(layer_core::PortableId::random(),ImageObject::new(Image::new(source))).unwrap();
        let owner=artwork.occurrences.insert(layer_core::PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Image")).unwrap();
        let stack=artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries=vec![owner];
        let mut editor=layer_core::Editor::new(layer_core::Document::from_artwork(artwork).unwrap());
        let mut r=WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        crate::test_support::submit_settled(&mut r,display_frame(editor.document(),1));
        let mut occurrence=editor.document().scene().occurrence(owner).unwrap().clone();occurrence.opacity=0.5;
        editor.perform(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&editor.document().artwork.occurrences,owner,Some(occurrence)).unwrap())).unwrap();
        crate::test_support::submit_settled(&mut r,display_frame(editor.document(),1));
        let mut fresh=WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        crate::test_support::submit_settled(&mut fresh,display_frame(editor.document(),1));
        let shown=|r:&WgpuRasterizer|crate::test_support::float_pixels(r,r.scale_display.as_ref().unwrap().texture());
        assert!(crate::test_support::max_error(&shown(&r),&shown(&fresh))<2e-3,"the edited layer replaces the held window at level 1");
    }

    #[test]
    fn preparing_one_root_refinement_preserves_other_level_ownership() {
        use layer_core::authored::*;
        let extent=[1024,512];
        let mut artwork=Artwork::new(extent).unwrap();
        let source=layer_core::color::source::rgba8_source([512;2],|_,_|[31,71,113,255]);
        let object=artwork.objects.insert(layer_core::PortableId::random(),ImageObject::new(Image::new(source))).unwrap();
        let owner=artwork.occurrences.insert(layer_core::PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Image")).unwrap();
        let stack=artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries=vec![owner];
        let document=layer_core::Document::from_artwork(artwork).unwrap();
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        renderer.ensure_document_metadata(extent,document.scene()).unwrap();
        let frame=display_frame(&document,0);
        crate::memory::memory_tests::legacy_scene_frame(&mut renderer,frame);
        let mut scene=Scene::new(&renderer).unwrap();scene.scale_sources.prepare(&renderer,frame,&[]);
        let mut cache=Cache::new(&renderer,Request {plan:display_mips::Plan::at(extent,1),evaluation:Evaluation::Display},1);
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.pages=scene.scale_pages.clone();
        cache.prepare_root(&renderer,frame,&scene.scale_sources,false).unwrap();
        let key=cache.page_key(cache.graph.root.clone().unwrap(),bounds([0;2],1,extent));
        let image=cache.pages.lock().unwrap().claim(&renderer,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        let write=cache.pages.lock().unwrap().prepare_write(key.clone(),image.plan.bounds,Accuracy::Display,key.level).unwrap();
        drop(encoder.color_pass("committed other-level page",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
        cache.pages.lock().unwrap().commit(write,0,&mut encoder);
        let mut pending=key.clone();pending.level=2;
        drop(cache.pages.lock().unwrap().claim(&renderer,pending.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());
        let input=Arc::new(());
        hold_fixture_inputs(&mut cache.pages.lock().unwrap(),&pending,pending.plan().bounds,Evaluation::Display,vec![input.clone().into()].into(),pending.level);
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
        assert!(cache.pages.lock().unwrap().get(&key,key.plan().bounds,Accuracy::Display,key.level,crate::residency::Class::Visible).is_some());
        let mut fine=Cache::new(&renderer,Request {plan:display_mips::Plan::at(extent,0),evaluation:Evaluation::Display},1);
        fine.pages=cache.pages.clone();
        fine.prepare_root(&renderer,frame,&scene.scale_sources,false).unwrap();
        assert!(cache.pages.lock().unwrap().get(&key,key.plan().bounds,Accuracy::Display,key.level,crate::residency::Class::Visible).is_some(),"a refinement at level zero cannot stale another level's committed pixels");
        assert_eq!(Arc::strong_count(&input),2,"a refinement cannot cancel another level's accepted inputs");
        assert!(crate::test_support::float_pixels(&renderer,&image.texture).iter().all(|pixel|*pixel==[1.;4]));
    }

    #[test]
    fn an_overview_without_a_whole_record_is_no_stand_in() {
        let extent=[4096,2048];
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let source=layer_core::color::source::rgba8_source(extent,|_,_|[40,90,170,255]);
        paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
        let mut renderer=display_window(&document,&source,0,PixelRect::new(0,0,extent[0]/2,extent[1]));
        let mut cache=renderer.scale_display.take().unwrap();
        let overview=cache.overview.as_mut().unwrap();
        overview.pixels.ensure_root(&renderer,overview.plan,overview.precision.format(),"unwritten overview",renderer.scene_tiers.windows.charge(0));
        overview.pixels.ensure(&renderer,overview.plan,overview.precision.format(),renderer.scene_tiers.windows.charge(0));
        let key=overview.page_key(overview.graph.root.clone().unwrap(),overview.plan.bounds);
        let claimed=overview.pages.lock().unwrap().claim(&renderer,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
        let frame=overview.expected_frame().unwrap();
        assert!(overview.pages.lock().unwrap().evaluated_from(&claimed.view,key.plan().bounds,&frame,overview.required_input_level()).unwrap().is_empty());
        assert!(claimed.texture.width()>0);
        assert!(overview.pixels.root().is_some() && overview.pixels.next().is_some());
        assert!(overview.presentation().is_none(),"allocation and validity bookkeeping cannot mint a committed page handle");
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        overview.pixels.commit_root(encoder.submission(),Vec::new(),Vec::new(),None,None);
        encoder.submit(&renderer.queue);
        assert!(overview.presentation().is_some() && overview.picture().is_none(),"a committed record that is not whole is no picture");
        renderer.scale_display=Some(cache);
        let cache=renderer.scale_display.as_ref().unwrap();
        assert!(cache.stand_in().is_none(),"an overview whose record is not whole is no stand-in");
        assert!(cache.presentable().is_some());
        let page=renderer.presentation_page().unwrap();
        let views=page.views(&renderer.empty_view);
        assert_eq!(views[0],cache.view());
        assert_eq!((views[1],page.stand_in_scale()),(cache.view(),0.),"the page carries no stand-in");
        assert!(views.iter().all(|view|view.texture().width()>0 && view.texture().height()>0));
    }

    fn overview_cell(r:&WgpuRasterizer)->(&Cache,usize) {
        let overview=r.scale_display.as_ref().unwrap().overview.as_deref().expect("a zoomed view keeps an overview");
        (overview,Cells::index(overview.plan,[0,0]))
    }
    fn zoomed_over_the_first_overview_cell(document:&layer_core::Document)->FramePacket<'_> {
        let extent=document.composition().size;
        let mut frame=packet(document.scene(),extent);frame.composite_all=false;
        frame.view.width_px=extent[1];frame.view.height_px=extent[1];
        frame
    }

    #[test]
    fn a_discarded_commit_never_certifies_the_older_pixels_it_falls_back_to() {
        let document=crate::scene::scale::tests::document_at([256;2]);
        let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        let mut pixels=Pixels::default();
        pixels.ensure_root(&renderer,display_mips::Plan::at([256;2],0),wgpu::TextureFormat::Rgba16Float,"coverage records",renderer.scene_tiers.windows.charge(0));
        let mut partial=submission::CommandEncoder::new(&renderer.device,&Default::default());
        let receipt=partial.submission();
        pixels.commit_root(receipt.clone(),Vec::new(),Vec::new(),None,None);pixels.certify_root(false,None,false);
        partial.submit(&renderer.queue);
        let mut completing=submission::CommandEncoder::new(&renderer.device,&Default::default());
        pixels.commit_root(completing.submission(),Vec::new(),Vec::new(),None,None);pixels.certify_root(true,Some(layer_core::BlendSpace::Linear),false);
        assert!(pixels.committed_root().is_some_and(|image|image.complete() && image.whole().is_some()));
        drop(completing);
        let image=pixels.committed_root().unwrap();
        assert!(image.submission().same(&receipt) && !image.complete() && image.whole().is_none(),"the discarded commit's coverage leaves with it");
    }

    #[test]
    fn a_discarded_overview_reduction_leaves_its_cell_unreduced() {
        let document=super::super::tests::document_at([4096,2048]);
        let frame=zoomed_over_the_first_overview_cell(&document);
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
        loop {
            r.test.discard_frame=true;
            crate::test_support::submit_ready(&mut r,frame).unwrap();
            assert!(!r.test.discard_frame,"the frame's encoder is dropped");
            let (overview,index)=overview_cell(&r);
            let discarded={let cells=overview.cells.lock().unwrap();
                cells.cells[index].reduced && cells.unconfirmed_reductions.iter().any(|(submission,cell)|*cell==index && submission.discarded())};
            if discarded {
                assert!(!table(overview).cells[index].reduced,"a discarded reduction leaves its cell unreduced");
                break;
            }
            crate::test_support::submit_ready(&mut r,frame).unwrap();r.wait_idle().unwrap();
            assert!(std::time::Instant::now()<deadline,"a discarded frame reduces the derived overview cell");
        }
        crate::test_support::submit_settled(&mut r,frame);
        let (overview,index)=overview_cell(&r);
        let cells=table(overview);
        assert!(cells.cells[index].derived && cells.cells[index].reduced,"a later frame reduces the cell again");
    }

    fn half_view(document:&layer_core::Document)->FramePacket<'_> {
        let mut frame=packet(document.scene(),document.composition().size);frame.composite_all=false;
        frame.view=layer_render::ViewState {width_px:1280,height_px:1024,document_to_surface:[0.5,0.,0.,0.5,0.,0.]};
        frame
    }
    fn dabs_at(pages:&[[u32;2]],color:[f32;4])->Vec<layer_render::Dab> {
        pages.iter().map(|page|{let mut dab=crate::tests::test_dab(page.map(|value|(value*PAGE_SIZE+PAGE_SIZE/2) as f32),color,1.);dab.radii=[8.;2];dab}).collect()
    }
    fn batch_over(document:&layer_core::Document,dabs:&[layer_render::Dab],kind:layer_render::DabBatchKind)->layer_render::DabBatch {
        let style=crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
        let damage=dabs.iter().map(|dab|dab.bounds()).reduce(layer_core::Rect::union).unwrap();
        layer_render::DabBatch {kind,dab_count:dabs.len() as u32,..crate::test_support::dab_batch(document.working.target.unwrap(),style,damage)}
    }
    fn paged(document:&layer_core::Document)->WgpuRasterizer {
        let frame=half_view(document);
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        crate::test_support::submit_settled(&mut r,frame);
        let dabs=dabs_at(&[[2,2],[14,6]],[1.,0.,0.,1.]);
        let batch=batch_over(document,&dabs,layer_render::DabBatchKind::Persistent);
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:&dabs,dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        crate::test_support::submit_settled(&mut r,frame);
        r
    }
    fn blue_page(r:&mut WgpuRasterizer,document:&layer_core::Document,page:[u32;2]) {
        let blue=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|[0.1f32,0.2,0.9,1.]).flat_map(f32::to_le_bytes).collect::<Vec<_>>();
        crate::test_support::upload_layer_page(r,document.scene(),document.working.target.unwrap(),page,&blue);
    }
    fn shown_at(r:&WgpuRasterizer,image:&Image,page:[u32;2])->[f32;4] {
        let [x,y]=page.map(|value|(value*PAGE_SIZE+PAGE_SIZE/2)>>image.plan.level);
        super::super::tests::window_pixels(r,image,image.plan)[(y*image.plan.size[0]+x) as usize]
    }
    fn refuse_cell(r:&WgpuRasterizer,area:PixelRect) {
        let main=r.scale_display.as_ref().unwrap();let display=r.scene_tiers.display.clone();
        main.invalidate_cells(main.reached_cells(DocRect::from(area)),None);
        main.refuse(area,&display.blocked(Class::Visible,display.limit()+1).expect("a unit larger than the display grant is refused"),crate::frame_demand::Allowance::Fresh);
    }
    fn release_cell(r:&WgpuRasterizer,area:PixelRect) {r.scale_display.as_ref().unwrap().release(area);}
    fn changed_overview(document:&layer_core::Document,page:[u32;2])->[f32;4] {
        let mut fresh=paged(document);blue_page(&mut fresh,document,page);
        crate::test_support::submit_settled(&mut fresh,half_view(document));
        shown_at(&fresh,overview_cell(&fresh).0.pixels.root().unwrap(),page)
    }

    #[test]
    fn an_overview_whose_derived_cell_still_reduces_the_views_old_pixels_is_no_current_picture() {
        let document=super::super::tests::document_at([4096,2048]);let frame=half_view(&document);
        let (derived,direct)=([2,2],[14,6]);
        let shown=|r:&WgpuRasterizer,image:&Image|shown_at(r,image,derived);
        let expected=changed_overview(&document,derived);
        for direct_changed in [false,true] {
            let mut r=paged(&document);
            {
                let (overview,index)=overview_cell(&r);
                let cells=overview.cells.lock().unwrap();
                assert!(cells.cells[index].derived && cells.cells[index].reduced && cells.cells.iter().any(|cell|!cell.derived && cell.shown.is_some()),
                    "the overview derives its first cell from the view and composes another");
            }
            refuse_cell(&r,page_rect(derived));
            blue_page(&mut r,&document,derived);
            if direct_changed {blue_page(&mut r,&document,direct);}
            for _ in 0..4 {crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);}
            let main=r.scale_display.as_ref().unwrap();let (overview,index)=overview_cell(&r);
            assert!(!main.displays(page_rect(derived)) && overview.cells.lock().unwrap().cells[index].derived,"the view cell under the change keeps its old pixels, and the overview derives from it");
            let old=shown(&r,overview.pixels.root().unwrap());
            assert!(crate::test_support::max_error(&[old],&[expected])>0.05,"the overview still shows the old pixels: {old:?} against {expected:?}");
            let labelled=main.current_picture().map(|picture|shown(&r,picture.image()));
            assert!(labelled.is_none_or(|labelled|crate::test_support::max_error(&[labelled],&[expected])<1e-3),
                "with its composed cell {}, the overview labels a picture current that shows {labelled:?} where the change shows {expected:?}",if direct_changed {"changed too"}else {"unchanged"});
        }
    }

    #[test]
    fn an_overview_cell_reduced_in_parts_labels_no_picture_current_while_one_part_shows_a_withdrawn_preview() {
        let document=super::super::tests::document_at([4096,2048]);let frame=half_view(&document);
        let (restored,refused)=([2,2],[5,5]);
        let mut fresh=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        crate::test_support::submit_settled(&mut fresh,frame);
        let expected=shown_at(&fresh,overview_cell(&fresh).0.pixels.root().unwrap(),refused);
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        crate::test_support::submit_settled(&mut r,frame);
        let dabs=dabs_at(&[restored,refused],[0.,1.,0.,1.]);
        let batch=batch_over(&document,&dabs,layer_render::DabBatchKind::Preview);
        for _ in 0..4 {
            crate::test_support::submit_ready(&mut r,FramePacket {dabs:&dabs,dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
            crate::test_support::complete(&r);
            let main=r.scale_display.as_ref().unwrap();
            if main.displays(page_rect(restored)) && main.displays(page_rect(refused)) && main.pixels.committed_root().and_then(CommittedImage::authored)==main.current_authored().map(|(_,authored)|authored) {break;}
        }
        refuse_cell(&r,page_rect(refused));
        let withdrawn=layer_render::DabBatch {kind:layer_render::DabBatchKind::Persistent,dab_count:0,stroke_start:false,..batch};
        for _ in 0..2 {crate::test_support::submit_ready(&mut r,FramePacket {dabs:&[],dab_batches:std::slice::from_ref(&withdrawn),..frame}).unwrap();crate::test_support::complete(&r);}
        let main=r.scale_display.as_ref().unwrap();let (overview,index)=overview_cell(&r);
        assert!(main.displays(page_rect(restored)) && !main.displays(page_rect(refused)) && overview.cells.lock().unwrap().cells[index].derived,
            "the view restores one previewed cell and keeps the other, and the overview derives both");
        let previewed=shown_at(&r,overview.pixels.root().unwrap(),refused);
        assert!(crate::test_support::max_error(&[previewed],&[expected])>0.05,"the overview still shows the withdrawn preview: {previewed:?} against {expected:?}");
        let labelled=main.current_picture().map(|picture|shown_at(&r,picture.image(),refused));
        assert!(labelled.is_none_or(|labelled|crate::test_support::max_error(&[labelled],&[expected])<1e-3),
            "the overview labels a picture current that shows the withdrawn preview {labelled:?} where the scene shows {expected:?}");
    }

    #[test]
    fn an_overview_reduces_a_view_cell_that_refinement_copied_before_taking_its_stamp() {
        let document=super::super::tests::document_at([4096,2048]);let frame=half_view(&document);
        let changed=[2,2];
        let expected=changed_overview(&document,changed);
        let mut r=paged(&document);
        refuse_cell(&r,page_rect(changed));
        blue_page(&mut r,&document,changed);
        for _ in 0..4 {crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);}
        {
            let main=r.scale_display.as_ref().unwrap();
            assert!(!main.displays(page_rect(changed)),"the refused view cell keeps its old pixels");
            evict_composite_pages_where(main,&[page_rect(changed)],|key|key.level<=main.plan.level);
        }
        release_cell(&r,page_rect(changed));
        for _ in 0..8 {
            if r.scale_display.as_ref().unwrap().displays(page_rect(changed)) {break;}
            refine(&mut r,frame);
        }
        assert!(r.scale_display.as_ref().unwrap().displays(page_rect(changed)),"refinement copies the changed cell into the view");
        let dabs=dabs_at(&[[14,6]],[0.,1.,0.,1.]);
        let batch=batch_over(&document,&dabs,layer_render::DabBatchKind::Persistent);
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:&dabs,dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        crate::test_support::submit_settled(&mut r,frame);
        let labelled=r.scale_display.as_ref().unwrap().current_picture().map(|picture|shown_at(&r,picture.image(),changed));
        assert!(labelled.is_none_or(|labelled|crate::test_support::max_error(&[labelled],&[expected])<1e-3),
            "the overview labels a picture current that shows {labelled:?} where the refined change shows {expected:?}");
    }

    #[test]
    fn a_discarded_half_level_reduction_leaves_the_root_changes_it_took_unreduced() {
        let document=super::super::tests::document_at([4096,2048]);let frame=half_view(&document);
        let (changed,hole)=([2,2],PixelRect::new(2048,0,2560,512));
        let mut fresh=paged(&document);blue_page(&mut fresh,&document,changed);
        crate::test_support::submit_settled(&mut fresh,frame);
        let expected=shown_at(&fresh,fresh.scale_display.as_ref().unwrap().pixels.next().unwrap(),changed);
        let mut r=paged(&document);
        let (previous,authored)={
            let main=r.scale_display.as_ref().unwrap();
            assert!(main.pixels.committed_next().is_some(),"the view reduces its half level");
            let root=main.pixels.committed_root().unwrap();
            let previous=super::super::tests::window_pixels(&r,root.image(),root.image().plan);
            let authored=root.authored();
            main.evict_window(hole);
            (previous,authored)
        };
        refuse_cell(&r,hole);
        blue_page(&mut r,&document,changed);
        for _ in 0..6 {
            crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
            if r.scale_display.as_ref().unwrap().coverage_snapshot().ready(page_rect(changed),Accuracy::Display) {break;}
        }
        let main=r.scale_display.as_ref().unwrap();
        assert!(main.coverage_snapshot().ready(page_rect(changed),Accuracy::Display),"the changed cell composes privately");
        assert!(!main.displays(page_rect(changed)) && !main.coverage_complete(),"the hole defers the authored swap");
        let root=main.pixels.committed_root().unwrap();
        assert_eq!(root.authored(),authored);
        let retained=super::super::tests::window_pixels(&r,root.image(),root.image().plan);
        assert_eq!(crate::test_support::max_error(&retained,&previous),0.,"the whole previous root stays visible before the swap");
        let [x,y,w,h]=paint_transform::texel_rect(page_rect(changed).window_local(main.plan.bounds),1<<main.plan.level);
        let changed_texels=PixelRect::new(x,y,x+w,y+h);
        release_cell(&r,hole);
        r.test.discard_frame=true;
        crate::test_support::submit_ready(&mut r,frame).unwrap();
        assert!(!r.test.discard_frame,"the frame that fills the hole and reduces the half level is dropped");
        let main=r.scale_display.as_ref().unwrap();
        assert!(main.pixels.unreduced.taken.iter().any(|(submission,region)|submission.discarded() && !region.intersect(changed_texels).is_empty()),"the discarded reduction took the changed root region");
        assert!(!main.pixels.unreduced().intersect(changed_texels).is_empty(),"the discarded reduction leaves the change unreduced");
        let mut labelled=false;
        for _ in 0..4 {
            crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
            let main=r.scale_display.as_ref().unwrap();
            let Some(next)=main.pixels.committed_next().filter(|next|next.authored().is_some() && next.authored()==main.current_authored().map(|(_,authored)|authored)) else {continue;};
            let shown=shown_at(&r,next.image(),changed);
            assert!(crate::test_support::max_error(&[shown],&[expected])<1e-3,"the half level is labelled current while it shows {shown:?} where the change shows {expected:?}");
            labelled=true;
        }
        assert!(labelled,"the half level is labelled current once the hole fills");
    }

    #[test]
    fn a_precision_change_counts_no_earlier_reduction_as_written() {
        let document=super::super::tests::document_at([4096,2048]);
        let frame=zoomed_over_the_first_overview_cell(&document);
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        crate::test_support::submit_settled(&mut r,frame);
        {
            let (overview,index)=overview_cell(&r);
            let cells=table(overview);
            assert!(cells.cells[index].derived && cells.cells[index].reduced,"the settled overview reduces its derived cell");
        }
        layer_render::CanvasRenderer::set_clipping_preview(&mut r,true,false);
        r.test.discard_frame=true;
        crate::test_support::submit_ready(&mut r,frame).unwrap();
        assert!(!r.test.discard_frame,"the frame's encoder is dropped");
        let (overview,_)=overview_cell(&r);
        assert_eq!(overview.pixels.root().map(|image|image.texture.format()),Some(wgpu::TextureFormat::Rgba32Float),"the clipping preview replaces the overview's window");
        assert!(table(overview).cells.iter().all(|cell|!cell.reduced),"a replaced window counts no earlier reduction as written");
    }

    #[test]
    fn overview_inputs_survive_camera_level_and_evaluation_changes() {
        let extent=[8192,4096];
        let document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        let mut scene=Scene::new(&renderer).unwrap();
        let frame=display_frame(&document,0);
        scene.scale_sources.prepare(&renderer,frame,&[]);
        let half=PixelRect::new(0,0,extent[0]/2,extent[1]);
        let mut cache=Cache::new(&renderer,Request {plan:display_mips::Plan::window(extent,0,half),evaluation:Evaluation::Display},document.scene().order().len());
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        let overview=cache.overview.as_ref().unwrap();
        let key=overview.page_key(overview.graph.root.clone().unwrap(),overview.plan.bounds);
        drop(overview.pages.lock().unwrap().claim(&renderer,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());
        let owner=Arc::new(());
        hold_fixture_inputs(&mut overview.pages.lock().unwrap(),&key,key.plan().bounds,overview.evaluation,vec![owner.clone().into()].into(),key.level);
        for (level,evaluation) in [(1,Evaluation::Native),(3,Evaluation::Tiled(2)),(0,Evaluation::Display),(2,Evaluation::Native)] {
            let frame=FramePacket {composite_all:false,..display_frame(&document,level)};
            scene.scale_sources.prepare(&renderer,frame,&[]);
            (cache,_)=Cache::select(Some(cache),&renderer,frame,Request {plan:display_mips::Plan::window(extent,level,half),evaluation});
            crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
            cache.prepare_graph(&renderer,frame,&scene).unwrap();
            let global=if cache.plan==overview_plan(cache.plan) {&cache}else {cache.overview.as_deref().unwrap()};
            assert!(global.page_key(global.graph.root.clone().unwrap(),global.plan.bounds)==key);
            assert_eq!(Arc::strong_count(&owner),2,"camera/evaluation changes cannot cancel accepted overview inputs");
        }
    }

    fn regional_object_overview<'a>(renderer:&WgpuRasterizer,document:&'a layer_core::Document,owner:OccurrenceHandle)
        ->(display_mips::Plan,FramePacket<'a>,Box<Scene>,crate::scene::objects::CollectionJob) {
        let plan=overview_plan(display_mips::Plan::at(document.composition().size,0));
        let mut frame=display_frame(document,plan.level);frame.view.width_px=PAGE_SIZE;frame.view.height_px=PAGE_SIZE;
        let mut scene=Scene::new(renderer).unwrap();scene.scale_sources.prepare(renderer,frame,&[]);
        let content=scene.object_spatial.content(frame.scene,owner).unwrap();
        let leaf=scene.collection_job(renderer,frame,owner,content,
            crate::scene::objects::ObjectWindow {origin:[0.;2],side:f64::from(1u32<<plan.level),size:[PAGE_SIZE;2]});
        (plan,frame,scene,leaf)
    }
    pub(crate) fn regional_object_overview_footprint(renderer:&WgpuRasterizer,document:&layer_core::Document,owner:OccurrenceHandle)->PixelRect {
        let (_,_,_,leaf)=regional_object_overview(renderer,document,owner);
        let job=leaf.child(0,leaf.window,renderer.empty_view.clone()).unwrap();
        crate::object_sampling::ObjectSampler::source_bounds(job.inverse,job.size,job.nearest,job.source.extent).unwrap()
    }
    pub(crate) fn assert_regional_object_overview(renderer:&mut WgpuRasterizer,document:&layer_core::Document,
        owner:OccurrenceHandle,id:layer_core::PortableId) {
        let extent=document.composition().size;
        let (plan,frame,mut scene,leaf)=regional_object_overview(renderer,document,owner);
        let mut cache=Cache::new(&renderer,Request {plan,evaluation:Evaluation::Display},document.scene().order().len());
        let mut commands=RegionRecords::new(&renderer);scene.begin_frame();
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        let pipelines=&renderer.scene_pipelines;let kernels=pipelines.scale.output(cache.precision.format());
        pollster::block_on(renderer.compilation.prepare_all([&pipelines.constant.2,&pipelines.fold.pipeline,&pipelines.objects.preview,&kernels.compose,&kernels.reduce_pair,&kernels.reduce])).unwrap();
        assert_eq!(leaf.child(0,leaf.window,renderer.empty_view.clone()).unwrap().key.image,id);
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        {let mut work=crate::test_support::work(false);cache.render_graph(&mut scene,renderer,frame,&mut Encoding {encoder:&mut encoder,commands:&mut commands, retained:0, budget:&mut work},Destination::Preview).unwrap();}
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);renderer.wait_idle().unwrap();
        assert!(cache.displays(bounds([0;2],plan.level,extent)),"the overview unit must commit on the frame its regional mip inputs and halo are ready");
        assert!(!cache.displays(plan.bounds));
        let content=scene.object_spatial.content(frame.scene,owner).unwrap();
        let canonical=scene.collection_job(&renderer,frame,owner,content,leaf.window);
        assert!(scene.object_results.cover(&canonical,None).0.is_empty(),"no canonical result supplies the committed preview");
    }

    #[test]
    fn an_overview_page_commits_while_other_source_chunks_wait() {
        let extent=[4096;2];
        let mut document=super::super::tests::document_at(extent);
        let source=layer_core::color::source::rgba8_source(extent,|x,y|[(x/PAGE_SIZE) as u8,(y/PAGE_SIZE) as u8,119,255]);
        paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        renderer.ensure_document_metadata(extent,document.scene()).unwrap();
        let mut frame=display_frame(&document,1);frame.view.width_px=256;frame.view.height_px=256;
        crate::memory::memory_tests::legacy_scene_frame(&mut renderer,frame);
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        for coordinate in [[0,0],[1,0],[0,1],[1,1]] {
            let pixels=crate::prepare_image_tile(&source,coordinate,renderer.device.working_space(),&renderer.device.source_samples,None).unwrap();
            Scene::decode_prepared_for(&mut renderer,&mut encoder,&source,coordinate,pixels,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
        }
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);renderer.wait_idle().unwrap();
        let _store=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([source.as_ref()]),false);
        let mut scene=Scene::new(&renderer).unwrap();scene.scale_sources.prepare(&renderer,frame,&[]);
        let plan=overview_plan(display_mips::Plan::at(extent,0));
        let page_count=coordinates(plan.bounds,plan.level).count();
        assert!((16..=64).contains(&page_count));
        let mut cache=Cache::new(&renderer,Request {plan,evaluation:Evaluation::Display},document.scene().order().len());
        let mut commands=RegionRecords::new(&renderer);
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        scene.begin_frame();cache.prepare_graph(&renderer,frame,&scene).unwrap();
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        {let mut work=crate::test_support::work(false);cache.render_graph(&mut scene,&mut renderer,frame,&mut Encoding {encoder:&mut encoder,commands:&mut commands, retained:0, budget:&mut work},Destination::View).unwrap();}
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);renderer.wait_idle().unwrap();
        let coverage=cache.coverage_snapshot();
        assert!(coverage.ready(bounds([0;2],plan.level,extent),Accuracy::Display),"one complete page must publish without the rest of its level");
        assert!(!coverage.ready(plan.bounds,Accuracy::Display),"the other source chunks are still gated");
        renderer.scene=Some(scene);renderer.scale_display=Some(cache);
        frame.view.width_px=512;frame.view.height_px=512;
        let output=create_color_target(&renderer.device,[512;2],"partial overview");
        let mut presenter=crate::test_support::float_presenter(&renderer);
        presenter.present(&mut renderer,&output.1,frame.view,[0.18,0.18,0.18,1.]).unwrap();
        let surround=crate::test_support::float_pixels(&renderer,&output.0);
        renderer.presentation.showing=true;
        let validation=renderer.device.push_error_scope(wgpu::ErrorFilter::Validation);
        presenter.present(&mut renderer,&output.1,frame.view,[0.18,0.18,0.18,1.]).unwrap();
        let picture=crate::test_support::float_pixels(&renderer,&output.0);
        assert_ne!(picture[128*512+128],surround[128*512+128],"the ready page contains real source pixels");
        assert_eq!(picture[384*512+384],surround[384*512+384],"unwritten pages stay surround, without sampling their allocation");
        assert!(pollster::block_on(validation.pop()).is_none());
    }

    #[test]
    fn cropped_content_presents_while_a_global_fallback_prepares() {
        use crate::submission::ColorPass;
        let extent=[4096,2048];
        let document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        let mut scene=Scene::new(&renderer).unwrap();
        let frame=display_frame(&document,0);
        scene.scale_sources.prepare(&renderer,frame,&[]);
        let mut cache=Cache::new(&renderer,Request {plan:display_mips::Plan::window(extent,0,PixelRect::full([PAGE_SIZE;2])),
            evaluation:Evaluation::Display},document.scene().order().len());
        let mut commands=RegionRecords::new(&renderer);
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        pollster::block_on(renderer.compilation.prepare_all([&renderer.scene_pipelines.scale.output(cache.precision.format()).reduce_pair])).unwrap();
        let key=cache.page_key(cache.graph.root.clone().unwrap(),cache.plan.bounds);
        let image=cache.pages.lock().unwrap().claim(&renderer,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        let write=cache.pages.lock().unwrap().prepare_write(key.clone(),image.plan.bounds,Accuracy::Display,key.level).unwrap();
        drop(encoder.color_pass("first cropped page",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
        cache.pages.lock().unwrap().commit(write,0,&mut encoder);
        cache.copy_level(&scene,&mut renderer,&mut encoder,&mut commands).unwrap();
        assert!(cache.presentation().is_some());
        assert!(cache.presentable().is_some(),"committed content can present before the rest of the overview");
        let overview=cache.overview.as_mut().unwrap();
        for coordinate in coordinates(overview.plan.bounds,overview.plan.level) {
            let region=bounds(coordinate,overview.plan.level,extent);
            let key=overview.page_key(overview.graph.root.clone().unwrap(),region);
            let image=overview.pages.lock().unwrap().claim(&renderer,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
            let write=overview.pages.lock().unwrap().prepare_write(key.clone(),image.plan.bounds,Accuracy::Display,key.level).unwrap();
            drop(encoder.color_pass("global fallback page",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
            overview.pages.lock().unwrap().commit(write,0,&mut encoder);
        }
        overview.copy_level(&scene,&mut renderer,&mut encoder,&mut commands).unwrap();
        assert!(cache.presentable().is_some());
        let plan=display_mips::Plan::window(extent,0,PixelRect::new(768,256,1024,512));
        let (mut next,_)=Cache::select(Some(cache),&renderer,frame,Request {plan,evaluation:Evaluation::Display});
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        next.prepare_graph(&renderer,frame,&scene).unwrap();
        next.copy_level(&scene,&mut renderer,&mut encoder,&mut commands).unwrap();
        assert!(next.presentable().is_some());
        assert!(!next.retained_ready() && !next.output_complete());
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
        let presented=next.presentable().unwrap().pixels.committed_root().unwrap();
        assert!(crate::test_support::float_pixels(&renderer,&presented.image().texture).iter().all(|pixel|*pixel==[1.;4]));
    }

    #[test]
    fn a_replaced_window_with_holes_stays_presented_until_the_new_window_commits() {
        use crate::submission::ColorPass;
        let extent=[4096,2048];
        let document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        let mut scene=Scene::new(&renderer).unwrap();
        let frame=display_frame(&document,0);
        scene.scale_sources.prepare(&renderer,frame,&[]);
        let mut commands=RegionRecords::new(&renderer);
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        let write_first_cell=|cache:&mut Cache,renderer:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,commands:&mut RegionRecords| {
            crate::scene::scale::tests::prepare_fixture_frame(renderer,frame,&scene);
            cache.prepare_graph(renderer,frame,&scene).unwrap();
            let region=bounds([cache.plan.bounds.min_x()/PAGE_SIZE,cache.plan.bounds.min_y()/PAGE_SIZE],0,extent);
            let key=cache.page_key(cache.graph.root.clone().unwrap(),region);
            let image=cache.pages.lock().unwrap().claim(renderer,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
            let write=cache.pages.lock().unwrap().prepare_write(key,image.plan.bounds,Accuracy::Display,0).unwrap();
            drop(encoder.color_pass("one written cell",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
            cache.pages.lock().unwrap().commit(write,0,encoder);
            cache.copy_level(&scene,renderer,encoder,commands).unwrap();
        };
        let shown=PixelRect::new(0,0,512,256);
        let mut cache=Cache::new(&renderer,Request {plan:display_mips::Plan::window(extent,0,shown),evaluation:Evaluation::Display},document.scene().order().len());
        pollster::block_on(renderer.compilation.prepare_all([&renderer.scene_pipelines.scale.output(cache.precision.format()).reduce_pair])).unwrap();
        write_first_cell(&mut cache,&mut renderer,&mut encoder,&mut commands);
        assert!(cache.picture().is_none() && cache.pixels.committed_root().is_some_and(|root|!root.complete()),"the shown window has one written cell and a hole");
        let plan=display_mips::Plan::window(extent,0,PixelRect::new(1024,512,1536,768));
        let (mut next,replaced)=Cache::select(Some(cache),&renderer,frame,Request {plan,evaluation:Evaluation::Display});
        assert!(replaced && next.pixels.committed_root().is_none(),"the new window has committed nothing");
        assert_eq!(next.presentable().map(|cache|cache.plan.bounds),Some(shown),"the replaced window stays presented until the new window commits");
        write_first_cell(&mut next,&mut renderer,&mut encoder,&mut commands);
        next.release_previous(&renderer);
        assert!(next.previous.is_none(),"a replaced window with holes is released once the new window commits");
        assert_eq!(next.presentable().map(|cache|cache.plan.bounds),Some(plan.bounds),"the new window presents once it commits");
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
    }

    #[test]
    fn exact_children_rebuild_an_evicted_parent_without_source_work() {
        let extent=[4096,512];
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let source=layer_core::color::source::rgba8_source(extent,|_,_|[40,90,170,255]);
        paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
        let mut renderer=display(&document,&source,1);
        let frame=display_frame(&document,1);
        for _ in 0..64 {
            if renderer.scale_display.as_ref().unwrap().output_complete() {break;}
            refine(&mut renderer,frame);
        }
        assert!(renderer.scale_display.as_ref().unwrap().output_complete());
        let mut scene=renderer.scene.take().unwrap();
        let frame=FramePacket {composite_all:false,..display_frame(&document,2)};
        scene.scale_sources.prepare(&renderer,frame,&[]);
        let (mut cache,_)=Cache::select(renderer.scale_display.take(),&renderer,frame,
            Request {plan:display_mips::Plan::at(extent,2),evaluation:Evaluation::Display});
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        let mut key=cache.page_key(cache.graph.root.clone().unwrap(),cache.plan.bounds);
        key.format=wgpu::TextureFormat::Rgba32Float;
        {
            let mut store=cache.pages.lock().unwrap();
            if store.pages.contains_key(&key) {let page=store.remove(&key);assert_eq!(Arc::strong_count(&page.image),1);page.image.texture.destroy();}
            assert!(key.finer().all(|child|store.get(&child,child.plan().bounds,Accuracy::Exact,0,crate::residency::Class::Visible).is_some()));
        }
        let before=[renderer.metrics.source_upload_submissions,renderer.source_tiles.borrow().misses,renderer.metrics.composited_pixels];
        renderer.scale_display=Some(cache);renderer.scene=Some(scene);
        refine(&mut renderer,FramePacket {composite_all:false,..display_frame(&document,2)});
        assert!(renderer.scale_display.as_ref().unwrap().pages.lock().unwrap().get(&key,key.plan().bounds,Accuracy::Exact,0,crate::residency::Class::Visible).is_some(),
            "ready exact children must rebuild a missing ancestor even when no native page is missing");
        assert_eq!([renderer.metrics.source_upload_submissions,renderer.source_tiles.borrow().misses,renderer.metrics.composited_pixels],before);
    }

    #[test]
    fn kept_levels_fill_window_pages_without_satisfying_evaluation() {
        let extent=[1024,512];
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let source=layer_core::color::source::rgba8_source(extent,|_,_|[40,90,170,255]);
        paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
        let mut renderer=display(&document,&source,2);
        let uploads=renderer.metrics.source_upload_submissions;
        let decoded=renderer.source_tiles.borrow().misses;
        let composed=renderer.metrics.composited_pixels;
        let frame=display_frame(&document,0);
        let scene=renderer.scene.take().unwrap();
        let (mut cache,_)=Cache::select(renderer.scale_display.take(),&renderer,frame,
            Request {plan:display_mips::Plan::at(extent,0),evaluation:Evaluation::Display});
        let mut commands=RegionRecords::new(&renderer);
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        cache.copy_level(&scene,&mut renderer,&mut encoder,&mut commands).unwrap();
        for coordinate in coordinates(cache.plan.bounds,cache.plan.level) {
            let region=bounds(coordinate,cache.plan.level,extent);
            let key=cache.page_key(cache.graph.root.clone().unwrap(),region);
            let store=cache.pages.lock().unwrap();
            let page=store.pages.get(&key).expect("each window page must have written fallback pixels");
            assert!(Coverage::covers(&page.coverage.written,region));
            assert!(page.versions.is_empty() && page.coverage.exact.is_empty());
        }
        assert!(cache.presentation().is_some());assert!(!cache.retained_ready() && !cache.output_complete());
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
        let expected=[40.,90.,170.].map(|value|layer_core::color::RgbSpace::Srgb.decode(value/255.) as f32);
        assert!(crate::test_support::float_pixels(&renderer,cache.texture()).iter().all(|pixel|
            pixel[..3].iter().zip(expected).all(|(actual,expected)|(*actual-expected).abs()<0.001) && pixel[3]==1.));
        assert_eq!(renderer.metrics.source_upload_submissions,uploads);
        assert_eq!(renderer.source_tiles.borrow().misses,decoded);
        assert_eq!(renderer.metrics.composited_pixels,composed);
    }

    #[test]
    fn a_new_level_projects_committed_finer_pages_without_source_work() {
        let extent=[1024,512];
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let source=layer_core::color::source::rgba8_source(extent,|_,_|[40,90,170,255]);
        paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
        let mut renderer=display(&document,&source,0);
        let before=renderer.scale_display.as_ref().unwrap().coverage_snapshot().written.regions(PixelRect::full(extent))
            .iter().map(|region|region.area()).sum::<u64>();
        let expected=crate::test_support::float_pixels(&renderer,renderer.scale_display.as_ref().unwrap().texture())[0];
        let uploads=renderer.metrics.source_upload_submissions;
        let decoded=renderer.source_tiles.borrow().misses;
        let composed=renderer.metrics.composited_pixels;
        let frame=display_frame(&document,1);
        let mut scene=renderer.scene.take().unwrap();
        scene.begin_frame();scene.scale_sources.prepare(&renderer,frame,&[]);
        let request=Request {plan:display_mips::Plan::at(extent,1),evaluation:Evaluation::Display};
        let (mut cache,_)=Cache::select(renderer.scale_display.take(),&renderer,frame,request);
        let mut commands=RegionRecords::new(&renderer);
        crate::scene::scale::tests::prepare_fixture_frame(&renderer,frame,&scene);
        cache.prepare_graph(&renderer,frame,&scene).unwrap();
        let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
        assert!(!cache.copy_level(&scene,&mut renderer,&mut encoder,&mut commands).unwrap().is_empty());
        assert_eq!(encoder.pass_count(),1,"eight retained inputs must share one projection pass");
        let coverage=cache.coverage_snapshot();
        assert!(Coverage::covers(&coverage.written,cache.plan.bounds));
        assert!(coverage.exact.is_empty(),"finer display fallbacks cannot grant exact permission");
        assert_eq!(coverage.written.regions(cache.plan.bounds).iter().map(|region|region.area()).sum::<u64>(),before);
        assert!(cache.presentation().is_some());
        renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);crate::test_support::complete(&renderer);
        assert!(crate::test_support::float_pixels(&renderer,cache.texture()).iter().all(|pixel|
            pixel.iter().zip(expected).all(|(actual,expected)|(*actual-expected).abs()<3e-5)));
        assert_eq!(renderer.metrics.source_upload_submissions,uploads);
        assert_eq!(renderer.source_tiles.borrow().misses,decoded);
        assert_eq!(renderer.metrics.composited_pixels,composed);
    }

    #[test]
    fn accepted_direct_pages_share_storage_and_keep_their_frame_candidate() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let parent=Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent:[2048;2],color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:2,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
        let child=parent.finer().next().unwrap();let bytes=256*256*16;
        let direct=std::collections::HashSet::from([parent.clone(),child.clone(),child.clone()]).into_iter().collect::<Vec<_>>();
        let document=super::super::tests::document_at(parent.extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),parent.extent));
        let (held,missing,extra)=store.reserve_inputs(&direct);
        assert!(held.is_empty());assert_eq!(missing.len(),2);assert_eq!(extra,2*bytes);
        let descendants=parent.finer().flat_map(|child|std::iter::once(child.clone()).chain(child.finer().collect::<Vec<_>>())).count();
        assert!((descendants as u64+1)*bytes>2*bytes);
        for key in missing {drop(store.claim(&r,key,2*bytes,WindowId::default(),crate::residency::Class::Visible).unwrap());}
        let other=Key {coordinate:[1,0],..parent.clone()};drop(store.claim(&r,other.clone(),3*bytes,WindowId::default(),crate::residency::Class::Visible).unwrap());
        for key in [&parent,&other] {
            let shared=store.allocated(&child,crate::residency::Class::Visible).unwrap();
            accept_fixture_inputs(&mut store,key,key.plan().bounds,Evaluation::Display,vec![(child.clone(),child.plan().bounds,shared)],key.level);
        }
        assert_eq!(store.bytes(),3*bytes);assert_eq!(store.pinned_bytes(),3*bytes);
        let (held,missing,extra)=store.reserve_inputs(&direct);
        assert!(missing.is_empty());assert_eq!(extra,0);drop(held);
        store.finish_fixture_inputs(&parent,parent.plan().bounds,parent.level);
        let held=store.accepted_coordinates(&parent,parent.level).collect::<Vec<_>>();assert_eq!(held,vec![[1,0]]);
        let view=layer_render::ViewState {width_px:256,height_px:256,document_to_surface:[1.,0.,0.,1.,0.,0.]};
        assert_eq!(nearest_pages(PixelRect::new(0,0,512,256),view,1,held,|_|true),vec![[0,0]],
            "an accepted farther page cannot precede the current focus");
        store.finish_fixture_inputs(&other,other.plan().bounds,other.level);store.trim(0,crate::residency::Class::Authored);
        assert_eq!(store.bytes(),0);assert_eq!(store.pinned_bytes(),0);
    }

    #[test]
    fn panning_releases_off_view_accepted_inputs_before_the_next_view() {
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let old=Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent:[512,256],color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
        let next=Key {coordinate:[1,0],..old.clone()};
        let interests=[(0..32).map(|_|Arc::new(())).collect::<Vec<_>>(),(0..32).map(|_|Arc::new(())).collect::<Vec<_>>()];
        let document=super::super::tests::document_at(old.extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),old.extent));
        for (key,inputs) in [(&old,&interests[0]),(&next,&interests[1])] {
            drop(store.claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());
            hold_fixture_inputs(&mut store,key,key.plan().bounds,Evaluation::Display,inputs.iter().cloned().map(Into::into).collect::<Vec<_>>().into(),key.level);
        }
        assert!(interests.iter().flatten().all(|interest|Arc::strong_count(interest)==2));
        store.set_expected(Arc::new(SignatureFrame::from_snapshot(store.expected().unwrap().snapshot())));
        store.retain_work(&[Interest::new(next.clone(),Evaluation::Display,next.plan().bounds,next.level)]);
        assert!(interests[0].iter().all(|interest|Arc::strong_count(interest)==1),"off-view accepted result interest ends at the wanted-set change");
        assert!(interests[1].iter().all(|interest|Arc::strong_count(interest)==2));
        assert_eq!(store.pinned_bytes(),store.pages[&next].image.bytes());
        assert!(!store.pages.contains_key(&old));
        let inputs=store.take_fixture_inputs(&next,next.plan().bounds,next.level);
        assert_eq!(inputs.len(),32,"the new view retains its admitted inputs and can run");
        let image=store.allocated(&next,crate::residency::Class::Visible).unwrap();
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let write=store.prepare_write(next.clone(),next.plan().bounds,Accuracy::Display,next.level).unwrap();
        drop(encoder.color_pass("next accepted view",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
        crate::scene::sources::Lease::track(inputs.into_iter().collect(),&mut encoder);
        store.commit(write,0,&mut encoder);
        r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
        assert!(store.get(&next,next.plan().bounds,Accuracy::Display,next.level,crate::residency::Class::Visible).is_some());
        assert!(crate::test_support::float_pixels(&r,&image.texture).iter().all(|pixel|*pixel==[1.;4]));
        drop(image);store.trim(0,crate::residency::Class::Authored);
        assert!(interests.iter().flatten().all(|interest|Arc::strong_count(interest)==1));
        assert_eq!(store.pinned_bytes(),0);assert_eq!(store.bytes(),0);
    }

    #[test]
    fn cancelled_parents_release_only_their_raw_chunk_interest() {
        let document=super::super::tests::document_at([1024;2]);
        let id=document.scene().order().iter().find_map(|owner|document.scene().source_target(*owner).filter(|id|matches!(id,SourceTarget::Paint(_)))).unwrap();
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        for self_destination in [false,true] {for finish in [false,true] {
            let parent=Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent:[1024;2],color:r.document_color(),
                blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
            let other=Key {coordinate:[1,0],..parent.clone()};
            let source=Key {source:Some(id),level:2,..parent.clone()};
            let chunks=[page_rect([0,0]),page_rect([1,0])];let interests=[Arc::new(()),Arc::new(())];
            let mut store=fixture_store(crate::test_support::packet(document.scene(),parent.extent));
            for key in [&parent,&other,&source] {drop(store.claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());}
            for (index,key) in [&parent,&other].into_iter().enumerate() {
                let shared=store.allocated(&source,crate::residency::Class::Visible).unwrap();
                accept_fixture_inputs(&mut store,key,key.plan().bounds,Evaluation::Display,vec![(source.clone(),chunks[index],shared.clone())],key.level);
                if self_destination {accept_fixture_inputs(&mut store,&source,chunks[index],Evaluation::Display,vec![(source.clone(),chunks[index],shared.clone())],source.level);}
                hold_fixture_inputs(&mut store,&source,chunks[index],Evaluation::Display,vec![interests[index].clone().into()].into(),source.level);
            }
            assert_eq!(Arc::strong_count(&interests[0]),2);assert_eq!(Arc::strong_count(&interests[1]),2);
            if finish {store.finish_fixture_inputs(&parent,parent.plan().bounds,parent.level);}
            store.set_expected(Arc::new(SignatureFrame::from_snapshot(store.expected().unwrap().snapshot())));
            store.retain_work(&[Interest::new(other.clone(),Evaluation::Display,other.plan().bounds,other.level),Interest::new(source.clone(),Evaluation::Display,source.plan().bounds,source.level)]);
            assert_eq!(Arc::strong_count(&interests[0]),1,"another parent using the same source key must not keep the cancelled chunk: self_destination={self_destination} finish={finish}");
            assert_eq!(Arc::strong_count(&interests[1]),2);assert!(!store.pages.contains_key(&parent));
            assert!(store.accepted(&other,other.plan().bounds,other.level));
            store.finish_fixture_inputs(&other,other.plan().bounds,other.level);
            store.set_expected(Arc::new(SignatureFrame::from_snapshot(store.expected().unwrap().snapshot())));
            store.retain_work(&[Interest::new(source.clone(),Evaluation::Display,source.plan().bounds,source.level)]);
            assert_eq!(Arc::strong_count(&interests[1]),1);store.trim(0,crate::residency::Class::Visible);assert_eq!(store.bytes(),0);
        }}
    }

    #[test]
    fn a_store_trim_reports_its_retired_pages_only_after_releasing_the_store() {
        thread_local! {static TRIMMING:std::cell::Cell<bool>=const {std::cell::Cell::new(false)};}
        let budget=layer_core::MemoryBudget::new();
        let r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()},budget.clone()).unwrap();
        let key=Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent:[PAGE_SIZE;2],color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
        let document=super::super::tests::document_at(key.extent);
        for closed in [false,true] {
            let unreported=r.device.transient_display.charge(1<<(20+u32::from(closed)));
            let transient=r.device.transient_display.used();
            let store=Pages::new(fixture_store(crate::test_support::packet(document.scene(),key.extent)));
            drop(store.lock().unwrap().claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());
            if closed {store.lock().unwrap().retirement.as_mut().unwrap().sender=crate::device_poll::Binding::Recipe(std::sync::Weak::new()).handle(&r.device);}
            let reentered=Arc::new(std::sync::atomic::AtomicU32::new(0));
            let (watched,observed)=(Arc::downgrade(&store.0),reentered.clone());
            let _watch=budget.watch(Arc::new(move|| {
                if TRIMMING.with(std::cell::Cell::get) && watched.upgrade().is_some_and(|store|store.try_lock().is_err()) {observed.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
            }));
            TRIMMING.with(|trimming|trimming.set(true));
            {
                let mut held=store.lock().unwrap();
                held.trim(0,crate::residency::Class::Authored);
                assert_eq!(held.bytes(),0);
            }
            TRIMMING.with(|trimming|trimming.set(false));
            r.device.retirement_handle().flush().unwrap();
            assert_eq!(reentered.load(std::sync::atomic::Ordering::Relaxed),0,"a memory watcher ran while the trimming thread held the page store, closed sink {closed}");
            assert_eq!(r.device.transient_display.used(),transient,"the retired page leaves the transient display after disposal, closed sink {closed}");
            drop(unreported);
        }
    }

    #[test]
    fn a_page_in_retirement_stays_reported_to_the_ledger_until_it_is_disposed() {
        let budget=layer_core::MemoryBudget::new();
        let r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()},budget).unwrap();
        let key=Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent:[PAGE_SIZE;2],color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
        let document=super::super::tests::document_at(key.extent);
        let store=Pages::new(fixture_store(crate::test_support::packet(document.scene(),key.extent)));
        drop(store.lock().unwrap().claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());
        let (sender,memory,total)={let held=store.lock().unwrap();let retirement=held.retirement.as_ref().unwrap();
            (retirement.sender.clone(),retirement.memory.clone().expect("retiring pages report to the transient owner"),retirement.total.clone())};
        let (release,gate)=std::sync::mpsc::channel::<()>();
        sender.run(move||{let _=gate.recv();});
        store.lock().unwrap().trim(0,crate::residency::Class::Authored);
        assert_eq!(memory.reported().pinned_gpu_bytes,total.used(),"a page handed to retirement stays reported as occupied until its disposal");
        release.send(()).unwrap();
        sender.flush().unwrap();
        assert_eq!(memory.reported().pinned_gpu_bytes,total.used(),"its disposal reports the release");
    }

    #[test]
    fn a_page_retired_while_the_display_cells_are_held_is_disposed_only_after_they_release() {
        thread_local! {static TRIMMING:std::cell::Cell<bool>=const {std::cell::Cell::new(false)};}
        let budget=layer_core::MemoryBudget::new();
        let r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()},budget.clone()).unwrap();
        let key=Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent:[PAGE_SIZE;2],color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
        let document=super::super::tests::document_at(key.extent);
        let unreported=r.device.transient_display.charge(1<<21);
        let store=Pages::new(fixture_store(crate::test_support::packet(document.scene(),key.extent)));
        drop(store.lock().unwrap().claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());
        store.lock().unwrap().retirement.as_mut().unwrap().sender=crate::device_poll::Binding::Recipe(std::sync::Weak::new()).handle(&r.device);
        let cells=Arc::new(std::sync::Mutex::new(Cells::default()));
        let reentered=Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (watched,observed)=(Arc::downgrade(&cells),reentered.clone());
        let _watch=budget.watch(Arc::new(move|| {
            if TRIMMING.with(std::cell::Cell::get) && watched.upgrade().is_some_and(|cells|cells.try_lock().is_err()) {observed.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
        }));
        TRIMMING.with(|trimming|trimming.set(true));
        {
            let (mut held,_cells)=lock_cells(&cells,&store);
            held.trim(0,crate::residency::Class::Authored);
            assert_eq!(held.bytes(),0);
        }
        TRIMMING.with(|trimming|trimming.set(false));
        assert_eq!(reentered.load(std::sync::atomic::Ordering::Relaxed),0,"a closed sink disposed a retired page while the display cells were locked");
        drop(unreported);
    }

    #[test]
    fn keyed_pages_stay_physical_until_submission_and_read_leases_finish() {
        use crate::submission::ColorPass;
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let key=Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent:[PAGE_SIZE;2],color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float};
        let document=super::super::tests::document_at(key.extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),key.extent));let bounds=key.plan().bounds;
        let image=store.claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
        let bytes=image.bytes();
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let write=store.prepare_write(key.clone(),bounds,Accuracy::Exact,0).unwrap();
        drop(encoder.color_pass("committed page lifetime",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
        store.commit(write,0,&mut encoder);
        drop(image);
        store.trim(0,crate::residency::Class::Authored);
        assert_eq!(store.bytes(),bytes);
        assert_eq!(store.pinned_bytes(),bytes,"encoding storage cannot disappear before submission");
        encoder.submit(&r.queue);crate::test_support::complete(&r);
        assert_eq!(store.pinned_bytes(),0);
        let lease=store.get(&key,bounds,Accuracy::Exact,0,crate::residency::Class::Visible).unwrap();
        store.trim(0,crate::residency::Class::Authored);
        assert_eq!(store.bytes(),bytes);assert_eq!(store.pinned_bytes(),bytes);
        let discarded=submission::CommandEncoder::new(&r.device,&Default::default());
        lease.track(&discarded);store.trim(0,crate::residency::Class::Authored);
        assert_eq!(store.bytes(),bytes,"an encoded reader retains its physical input");
        drop(discarded);
        assert!(store.get(&key,bounds,Accuracy::Exact,0,crate::residency::Class::Visible).is_some(),"discarding a reader cannot invalidate its completed producer");
        store.trim(0,crate::residency::Class::Authored);
        assert_eq!(store.bytes(),0);assert_eq!(store.pinned_bytes(),0);
    }

    #[test]
    fn a_held_unit_is_reused_across_frames_while_its_key_inputs_and_areas_are_unchanged() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let extent=[4*PAGE_SIZE;2];
        let document=super::super::tests::document_at(extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),extent));
        let key=|coordinate:[u32;2]|Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent,color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:0,coordinate,format:wgpu::TextureFormat::Rgba32Float};
        let (parent,child)=(key([0,0]),key([1,0]));
        for key in [&parent,&child] {drop(store.claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());}
        let areas=[(child.clone(),child.plan().bounds)];
        let input=store.allocated(&child,crate::residency::Class::Visible).unwrap();
        accept_fixture_inputs(&mut store,&parent,parent.plan().bounds,Evaluation::Display,vec![(child.clone(),child.plan().bounds,input)],0);
        let accepted=store.fixture_handle(&parent,parent.plan().bounds,0);
        let native=crate::scene::sources::Inputs::default().for_unit(accepted.input_unit()).unwrap();
        assert!(store.holds(&accepted,&native,&areas),"an unchanged held unit is reused");
        assert!(!store.holds(&accepted,&native,&[(parent.clone(),parent.plan().bounds)]),"other input areas re-accept");
        let sources=store.take_inputs(&accepted).unwrap();
        assert!(!store.holds(&accepted,&native,&areas),"consumed inputs re-accept");
        store.hold_inputs(&accepted,sources).unwrap();
        assert!(store.holds(&accepted,&native,&areas));
        store.set_expected(Arc::new(SignatureFrame::from_snapshot(store.expected().unwrap().snapshot())));
        assert!(store.holds(&accepted,&native,&areas),"a later frame with the same key keeps the held unit");
        store.set_expected(Arc::new(SignatureFrame::from_scene(crate::test_support::packet(document.scene(),[2*PAGE_SIZE;2]),Declared::default()).unwrap()));
        assert!(!store.holds(&accepted,&native,&areas),"a later frame that changes the unit's key re-accepts");
    }

    #[test]
    fn reports_read_counters_and_no_page_at_50k_store_entries() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let extent=[1;2];let document=super::super::tests::document_at([PAGE_SIZE;2]);
        let scene=Scene::new(&r).unwrap();
        *scene.scale_pages.lock().unwrap()=fixture_store(crate::test_support::packet(document.scene(),[PAGE_SIZE;2]));
        let held={let mut store=scene.scale_pages.lock().unwrap();(0..50_000u32).map(|index|store.claim(&r,Key {source:None,expression:Arc::new(graph::Expression::Color([index,0,0,0])),
            extent,color:r.document_color(),blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[0;2],format:wgpu::TextureFormat::R8Unorm},u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap())
            .step_by(7).collect::<Vec<_>>()};
        let report=|r:&WgpuRasterizer,scene:&Scene|{let report=r.display_memory_report(Some(scene),scene.retained_scale_bytes());[report.gpu_bytes,report.pinned_gpu_bytes]};
        let with=report(&r,&scene);
        let page=held[0].bytes();
        assert_eq!(scene.retained_scale_bytes(),[50_000*page,held.len() as u64*page]);
        let pages=std::mem::take(&mut scene.scale_pages.lock().unwrap().pages);
        assert_eq!(report(&r,&scene),with,"a report reads counters, not entries");
        scene.scale_pages.lock().unwrap().pages=pages;
    }

    #[test]
    fn evicting_k_victims_pops_o_k_log_e_entries_and_leaves_other_pages_untouched() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let side=16;let extent=[side*PAGE_SIZE;2];
        let document=super::super::tests::document_at(extent);
        let key=|coordinate:[u32;2]|Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent,color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:0,coordinate,format:wgpu::TextureFormat::Rgba8Unorm};
        for entries in [64u32,256] {
            let mut store=fixture_store(crate::test_support::packet(document.scene(),extent));
            let mut order=(0..entries).map(|index|[index%side,index/side]).collect::<Vec<_>>();
            let mut seed=0x2545_f491_u32;
            for index in (1..order.len()).rev() {seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;order.swap(index,seed as usize%(index+1));}
            for coordinate in &order {drop(store.claim(&r,key(*coordinate),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());}
            for coordinate in order.iter().step_by(3) {drop(store.get(&key(*coordinate),key(*coordinate).plan().bounds,Accuracy::Display,0,crate::residency::Class::Visible));}
            let held=store.allocated(&key(order[1]),crate::residency::Class::Visible).unwrap();
            let page=held.bytes();let victims=8;
            let mut ranked=store.pages.iter().filter(|(_,page)|!page.pinned()).map(|(key,page)|(page.used,key.clone())).collect::<Vec<_>>();
            ranked.sort_by_key(|(used,_)|*used);
            let before=store.pages.iter().map(|(key,page)|(key.clone(),(page.used,page.written_at))).collect::<std::collections::HashMap<_,_>>();
            let queued=store.victims.len();let pinned=store.pages.values().filter(|page|page.pinned()).count();
            store.trim(store.bytes()-victims as u64*page,crate::residency::Class::Visible);
            let pops=queued-store.victims.len()+pinned;
            assert_eq!(store.pages.len(),entries as usize-victims);
            assert!(ranked[..victims].iter().all(|(_,key)|!store.pages.contains_key(key)),"the least recently used unpinned pages leave first");
            assert!(store.pages.contains_key(&key(order[1])),"a held page stays");
            assert!(store.pages.iter().all(|(key,page)|before[key]==(page.used,page.written_at)),"a trim leaves every other page untouched");
            assert!(pops<=victims*2*(u32::BITS-entries.leading_zeros()) as usize,"{entries} entries: {pops} pops for {victims} victims");
        }
    }

    #[test]
    fn store_counters_match_a_page_walk_after_seeded_holds_commits_completions_and_evictions() {
        use crate::submission::ColorPass;
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let extent=[4*PAGE_SIZE;2];
        let document=super::super::tests::document_at(extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),extent));
        let key=|coordinate:[u32;2]|Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent,color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:0,coordinate,format:wgpu::TextureFormat::Rgba32Float};
        let pinned=|store:&Store,reading:&[Key]|store.pages.iter().filter(|(key,page)|Arc::strong_count(&page.image)>1 || store.inputs.contains_key(*key)
            || page.coverage.submissions.values().any(submission::Submission::pending) || reading.contains(key)).map(|(_,page)|page.image.bytes()).sum::<u64>();
        let bytes=store.claim(&r,key([0;2]),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap().bytes();
        let mut seed=0x9e37_79b9_7f4a_7c15u64;let mut counts=[0;8];
        let mut held=Vec::<Held>::new();let mut pending=Vec::<submission::CommandEncoder>::new();let mut reading=Vec::<Key>::new();
        for _ in 0..256 {
            seed=seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let key=key([(seed>>33) as u32%4,(seed>>41) as u32%4]);
            let op=((seed>>58)%8) as usize;
            let applied=match op {
                0=>{drop(store.claim(&r,key,u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());true}
                1=>store.allocated(&key,crate::residency::Class::Visible).map(|image|held.push(image)).is_some(),
                2=>held.pop().is_some(),
                3 if store.pages.contains_key(&key)=>{
                    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
                    let write=store.prepare_write(key.clone(),key.plan().bounds,Accuracy::Exact,0).unwrap();
                    drop(encoder.color_pass("seeded version",&store.pages[&key].image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
                    store.commit(write,0,&mut encoder);pending.push(encoder);true
                }
                4=>match (held.pop(),pending.last()) {
                    (Some(image),Some(encoder))=>{
                        reading.extend(store.pages.iter().find(|(_,page)|Arc::ptr_eq(&page.image,image.image())).map(|(key,_)|key.clone()));
                        image.track(encoder);true
                    }
                    (image,_)=>{held.extend(image);false}
                },
                5 if !pending.is_empty()=>{
                    if seed&1==0 {for encoder in pending.drain(..) {encoder.submit(&r.queue);}crate::test_support::complete(&r);}else {pending.clear();}
                    reading.clear();true
                }
                6 if store.pages.contains_key(&key) && !held.is_empty()=>{
                    let input=held[(seed>>8) as usize%held.len()].clone();let child=input.plan.bounds;
                    let child=store.pages.keys().find(|candidate|candidate.plan().bounds==child).unwrap().clone();
                    accept_fixture_inputs(&mut store,&key,key.plan().bounds,Evaluation::Display,vec![(child.clone(),child.plan().bounds,input)],0);true
                }
                7=>{let before=store.pages.len();store.finish_fixture_inputs(&key,key.plan().bounds,0);store.trim((seed>>8)%(8*bytes),crate::residency::Class::Visible);before!=store.pages.len()}
                _=>false,
            };
            counts[op]+=usize::from(applied);
            assert_eq!(store.version_slots,store.pages.values().map(|page|page.versions.capacity() as u64).sum::<u64>());
            assert_eq!(store.pinned_bytes(),pinned(&store,&reading),"pinned bytes after op {op}");
        }
        drop((held,pending));crate::test_support::complete(&r);
        assert_eq!(store.pinned_bytes(),pinned(&store,&[]));
        assert!(counts.iter().all(|count|*count>0),"the seeded sequence must hold, read, commit, complete, discard, accept and evict pages: {counts:?}");
    }

    #[test]
    fn complete_native_pages_advance_before_the_coarse_parent_is_exact() {
        let extent=[4352,512];
        let mut artwork=layer_core::authored::Artwork::new(extent).unwrap();
        crate::test_support::add_paint(&mut artwork,"Pixels",extent);
        let composition=artwork.compositions.get_mut(artwork.root).unwrap();
        composition.color.depth=layer_core::color::SampleDepth::U8;composition.blend=layer_core::BlendSpace::Linear;
        let mut document=layer_core::Document::from_artwork(artwork).unwrap();
        let source=layer_core::color::source::rgba8_source(extent,|x,y|[x as u8,y as u8,91,255]);
        paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
        let mut renderer=display(&document,&source,1);
        let frame=display_frame(&document,1);
        let cache=renderer.scale_display.as_ref().unwrap();
        assert_eq!(cache.precision,crate::display_precision::DisplayPrecision::Half);
        assert!(cache.coverage_snapshot().exact.is_empty());
        let mut key=cache.page_key(cache.graph.root.clone().unwrap(),page_rect([0;2]));
        let store=cache.pages.clone();
        assert!(store.lock().unwrap().get(&key,key.plan().bounds,Accuracy::Display,key.level,crate::residency::Class::Visible).is_some());
        key.format=wgpu::TextureFormat::Rgba32Float;
        refine(&mut renderer,frame);
        let cache=renderer.scale_display.as_ref().unwrap();
        let strip=cache.exact_tile.as_ref().unwrap();
        assert!(u64::from(strip.texture.width())*u64::from(strip.texture.height())<=SOURCE_SLOTS as u64*u64::from(PAGE_SIZE).pow(2));
        let native=Key {level:0,coordinate:[0,0],..key.clone()};
        let native_count=|store:&mut Store|page_coordinates(PixelRect::full(extent)).filter(|coordinate| {
            let native=Key {coordinate:*coordinate,..native.clone()};
            store.get(&native,native.plan().bounds,Accuracy::Exact,0,crate::residency::Class::Visible).is_some()
        }).count();
        for coordinate in [[0,0],[1,0]] {
            let native=Key {coordinate,..native.clone()};
            assert!(store.lock().unwrap().get(&native,native.plan().bounds,Accuracy::Exact,0,crate::residency::Class::Visible).is_some());
        }
        assert!(store.lock().unwrap().get(&key,key.plan().bounds,Accuracy::Exact,0,crate::residency::Class::Visible).is_none(),"a coarse parent owes all four complete native children before exact reduction");
        assert!(cache.coverage_snapshot().exact.is_empty(),"complete native top pages cannot grant a partial coarse exact permission");
        let mut completed=native_count(&mut store.lock().unwrap());
        assert_eq!(completed,2,"the first strip commits two whole native pages");
        let count=page_coordinates(PixelRect::full(extent)).count();
        for _ in 1..count {
            if !renderer.scale_display.as_ref().unwrap().has_pending_work(&renderer) {break;}
            refine(&mut renderer,frame);
            let cache=renderer.scale_display.as_ref().unwrap();
            let next=native_count(&mut store.lock().unwrap());
            assert!(next>completed,"ready native pages must advance without recapturing the same page");
            assert!(cache.presentable_pages()>0);completed=next;
        }
        let cache=renderer.scale_display.as_ref().unwrap();
        assert_eq!(completed,count);assert!(!cache.has_pending_work(&renderer));
        for column in 0..extent[0].div_ceil(2*PAGE_SIZE) {
            let key=Key {coordinate:[column,0],..key.clone()};
            assert!(store.lock().unwrap().get(&key,key.plan().bounds,Accuracy::Exact,0,crate::residency::Class::Visible).is_some());
        }
    }

    #[test]
    fn region_state_updates_work_before_any_cell_query() {
        use crate::frame_demand::{Input,Outstanding};
        let extent=[1024,512];let document=super::super::tests::document_at(extent);
        let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        r.ensure_document_metadata(extent,document.scene()).unwrap();
        let frame=display_frame(&document,0);
        crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
        let mut scene=Scene::new(&r).unwrap();scene.scale_sources.prepare(&r,frame,&[]);
        crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
        let display=r.scene_tiers.display.clone();
        let _held=display.charge(display.limit().saturating_sub(display.used()));
        let refused=display.blocked(Class::Visible,1).expect("the constrained tier refuses visible work");
        for update in ["block","refuse","shown","evict"] {
            let mut cache=Cache::new(&r,Request {plan:display_mips::Plan::at(extent,0),evaluation:Evaluation::Display},document.scene().order().len());
            cache.prepare_graph(&r,frame,&scene).unwrap();
            match update {
                "block"=>cache.block(cache.plan.bounds,None),
                "refuse"=>cache.refuse(cache.plan.bounds,&refused,crate::frame_demand::Allowance::Fresh),
                "shown"=>assert!(cache.shown_from(cache.plan.bounds,&cache.expected_frame().unwrap()).is_empty()),
                "evict"=>cache.evict_window(cache.plan.bounds),
                _=>unreachable!(),
            }
            let expected=if update=="refuse" {Outstanding::Blocked(Input::Residency)}else {Outstanding::Runnable};
            assert_eq!(cache.cells_demand(&r,true),Some(expected),"{update} preserves the current visible demand");
        }
    }

    #[test]
    fn a_cell_blocked_on_a_declaration_wakes_on_a_publication_after_its_frame_was_captured() {
        use crate::frame_demand::{Input,Outstanding,Producer,UnitWait};
        let extent=[1024,512];let document=super::super::tests::document_at(extent);
        let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        r.set_preparation_wake(Arc::new(||{}));
        r.ensure_document_metadata(extent,document.scene()).unwrap();
        let frame=display_frame(&document,0);
        crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
        let mut scene=Scene::new(&r).unwrap();scene.scale_sources.prepare(&r,frame,&[]);
        crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
        let mut cache=Cache::new(&r,Request {plan:display_mips::Plan::at(extent,0),evaluation:Evaluation::Display},document.scene().order().len());
        cache.prepare_graph(&r,frame,&scene).unwrap();
        let publish=r.raster_name_waker().expect("the raster runtime publishes declarations");
        cache.block(cache.plan.bounds,Some(UnitWait::input(Producer::Declaration(r.raster_declarations()))));
        assert_eq!(cache.cells_demand(&r,true),Some(Outstanding::Blocked(Input::Name)),"the cell sleeps until a declaration is published");
        publish.wake_by_ref();
        assert_eq!(cache.cells_demand(&r,true),Some(Outstanding::Runnable),"the publication wakes the cell");
        let captured=r.raster_declarations();
        publish.wake_by_ref();
        cache.block(cache.plan.bounds,Some(UnitWait::input(Producer::Declaration(captured))));
        assert_eq!(cache.cells_demand(&r,true),Some(Outstanding::Runnable),"a publication after the frame was captured wakes the cell the frame blocks");
    }

    #[test]
    fn a_cell_refused_its_object_windows_stays_parked_through_an_unrelated_arrival_and_revives_on_room() {
        use crate::frame_demand::Outstanding;
        let extent=[1024,512];let document=super::super::tests::document_at(extent);
        let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        r.ensure_document_metadata(extent,document.scene()).unwrap();
        let frame=display_frame(&document,0);
        crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
        let mut scene=Scene::new(&r).unwrap();scene.scale_sources.prepare(&r,frame,&[]);
        crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
        let display=r.scene_tiers.display.clone();
        let held=display.charge(display.limit().saturating_sub(display.used()));
        let refused=display.blocked(Class::Navigator,1).expect("the constrained tier refuses navigator object windows").alone();
        let mut cache=Cache::new(&r,Request {plan:display_mips::Plan::at(extent,0),evaluation:Evaluation::Display},document.scene().order().len());
        cache.prepare_graph(&r,frame,&scene).unwrap();
        cache.refuse(cache.plan.bounds,&refused,crate::frame_demand::Allowance::Fresh);
        assert_eq!(cache.residual_demand(&r),Some(Outstanding::Blocked(crate::frame_demand::Input::Residency)),"the cell refused its object windows parks on residency");
        let slot=Arc::new(std::sync::Mutex::new(crate::object_image_mips::CoordinateSlot::new(&Default::default())));
        r.source_results.push_back((r.device.working_space(),crate::object_image_mips::PreparedImageWork::Coordinates {
            destination:Arc::downgrade(&slot),pixels:Ok(Arc::new(vec![0;8]))},r.scene_tiers.ram.charge(0)));
        r.drain_image_decodes(&mut crate::submission::CommandEncoder::new(&r.device,&Default::default())).unwrap();
        assert!(!refused.fits(),"the stored refusal still cannot fit");
        assert_eq!(cache.residual_demand(&r),Some(Outstanding::Blocked(crate::frame_demand::Input::Residency)),"an unrelated arrival leaves the parked cell parked");
        drop(held);display.set_grant(u64::MAX);display.set_floor(0);
        assert!(refused.fits(),"the release and the raised grant give the navigator room");
        assert_eq!(cache.residual_demand(&r),Some(Outstanding::Runnable),"the room a release returns revives the parked cell");
    }

    #[test]
    fn full_edge_store_fill_and_exact_reduction_stay_inside_the_display_grant() {
        use crate::submission::ColorPass;
        use layer_core::{DeviceMemoryFacts,MemoryBudget,MemoryFacts};
        let mut failures=Vec::new();
        for exact in [false,true] {
            let extent=[8192;2];
            let mut document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
                layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
            document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth=layer_core::color::SampleDepth::F32;
            let mut r=crate::memory::memory_tests::renderer_in_budget(document.composition().color,
                DeviceMemoryFacts {budget_bytes:Some(768<<20),unified:false,..Default::default()},
                MemoryBudget::with_facts(MemoryFacts {host_default_bytes:Some(2<<30),override_bytes:Some(2<<30),..Default::default()})).unwrap();
            r.ensure_document_metadata(extent,document.scene()).unwrap();
            let mut frame=display_frame(&document,1);frame.view.width_px=2048;frame.view.height_px=2048;
            r.memory_demand(frame);r.prepare_moving_projection(frame).unwrap();r.publish_memory();
            r.factory.memory_budget().flush_reports();r.refresh_memory_limits();
            let mut scene=Scene::new(&r).unwrap();scene.scale_sources.prepare(&r,frame,&[]);
            let mut commands=RegionRecords::with_capacity(&r,1<<20,&Default::default());
            let window=if r.memory.display_grant().saturating_sub(scene.working_bytes()+commands.storage_bytes())>=80<<20 {4096}else {2048};
            frame.view.width_px=window/2;frame.view.height_px=window/2;
            r.memory_demand(frame);r.prepare_moving_projection(frame).unwrap();r.publish_memory();
            r.factory.memory_budget().flush_reports();r.refresh_memory_limits();
            let mut cache=Cache::new(&r,Request {plan:display_mips::Plan::window(extent,1,PixelRect::full([window;2])),evaluation:Evaluation::Display},document.scene().order().len());
            crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
            cache.prepare_graph(&r,frame,&scene).unwrap();cache.overview=None;
            let retained=commands.storage_bytes();
            let root=cache.graph.root.clone().unwrap();
            let mut key=cache.page_key(root,page_rect([0;2]));key.format=wgpu::TextureFormat::Rgba32Float;
            let page_bytes=key.plan().size.map(u64::from).into_iter().product::<u64>()*16;
            let window_bytes=cache.plan.size.map(u64::from).into_iter().product::<u64>()*16
                +cache.plan.level_size(cache.plan.level+1).map(u64::from).into_iter().product::<u64>()*16;
            let initial_scene=r.memory.display_grant();let initial_display=r.memory.display_owner().grant().gpu_bytes;
            let reservation=cache.reserve_window(&r,&scene,window_bytes,crate::frame_demand::Allowance::Fresh).unwrap_or_else(|_|panic!("projection setup: Scene={initial_scene} Display={initial_display} window_bytes={window_bytes}"));
            let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
            cache.pixels.ensure_root(&r,cache.plan,wgpu::TextureFormat::Rgba32Float,"composition window",reservation.charge());
            cache.pixels.ensure(&r,cache.plan,wgpu::TextureFormat::Rgba32Float,r.scene_tiers.windows.charge(0));
            for image in [cache.pixels.root().unwrap(),cache.pixels.next().unwrap()] {
                drop(encoder.color_pass("initialized retained projection",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
            }
            cache.pixels.commit_root(encoder.submission(),Vec::new(),Vec::new(),None,None);cache.pixels.commit_next(encoder.submission(),PixelRect::full(cache.pixels.next().unwrap().plan.size));
            encoder.submit(&r.queue);crate::test_support::complete(&r);
            let mut held=Vec::new();
            let mut group=0usize;
            let parents=coordinates(cache.plan.bounds,cache.plan.level).collect::<Vec<_>>();
            if exact {
                loop {
                    let report=r.display_memory_report(Some(&scene),cache.frame_memory(&r,retained));
                    let grant=r.memory.display_owner().grant().gpu_bytes;
                    if report.gpu_bytes+4*page_bytes>grant {break;}
                    let Some(&parent)=parents.get(group) else {break;};
                    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
                    for coordinate in [[parent[0]*2,parent[1]*2],[parent[0]*2+1,parent[1]*2],
                        [parent[0]*2,parent[1]*2+1],[parent[0]*2+1,parent[1]*2+1]] {
                        let child=Key {level:0,coordinate,..key.clone()};
                        let image=cache.pages.lock().unwrap().claim(&r,child.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
                        let write=cache.pages.lock().unwrap().prepare_write(child.clone(),image.plan.bounds,Accuracy::Exact,0).unwrap();
                        drop(encoder.color_pass("ready exact child",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
                        cache.pages.lock().unwrap().commit(write,0,&mut encoder);held.push(image);
                    }
                    encoder.submit(&r.queue);crate::test_support::complete(&r);
                    let report=r.display_memory_report(Some(&scene),cache.frame_memory(&r,retained));
                    assert!(report.gpu_bytes<=r.memory.display_owner().grant().gpu_bytes,"exact-child setup must fit the real Core grant");
                    group+=1;
                }
                assert!(group>0);
            } else {
                let coarse=Key {level:5,..key.clone()};
                let image=cache.pages.lock().unwrap().claim(&r,coarse.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
                let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
                let write=cache.pages.lock().unwrap().prepare_write(coarse.clone(),image.plan.bounds,Accuracy::Display,coarse.level).unwrap();
                drop(encoder.color_pass("committed coarse fallback",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
                cache.pages.lock().unwrap().commit(write,0,&mut encoder);held.push(image);
                encoder.submit(&r.queue);crate::test_support::complete(&r);
            }
            let id=document.active_target().unwrap();let mut coordinate=0u32;
            loop {
                let report=r.display_memory_report(Some(&scene),cache.frame_memory(&r,retained));
                let grant=r.memory.display_owner().grant().gpu_bytes;
                assert!(report.gpu_bytes<=grant,"retention setup must stay inside the actual Display grant");
                if report.gpu_bytes+page_bytes>grant {break;}
                let raw=Key {source:Some(id),expression:Arc::new(graph::Expression::Color([0;4])),level:0,
                    coordinate:[coordinate%32,coordinate/32],..key.clone()};
                assert!(raw.coordinate[1]<32);
                let image=cache.pages.lock().unwrap().claim(&r,raw.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap();
                let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
                let write=cache.pages.lock().unwrap().prepare_write(raw.clone(),image.plan.bounds,Accuracy::Exact,0).unwrap();
                drop(encoder.color_pass("retained raw read lease",&image.view,wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
                cache.pages.lock().unwrap().commit(write,0,&mut encoder);held.push(image);
                encoder.submit(&r.queue);crate::test_support::complete(&r);coordinate+=1;
            }
            let before=r.display_memory_report(Some(&scene),cache.frame_memory(&r,retained));
            assert!(r.memory.display_owner().grant().gpu_bytes-before.gpu_bytes<page_bytes,"a new page needs a genuine working admission");
            let store_bytes=cache.pages.lock().unwrap().bytes();
            assert!(store_bytes<r.memory.display_owner().grant().gpu_bytes,"plain Store capacity: Store={store_bytes} capacity={} initial_Scene={initial_scene} initial_Display={initial_display} window_bytes={window_bytes}",r.memory.display_owner().grant().gpu_bytes);
            for frame_index in 0..3 {
                scene.begin_frame();commands.begin();
                let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
                let result=if exact {{let mut work=crate::test_support::work(false);cache.refine(&mut scene,&mut r,frame,&mut encoder,&mut commands,&mut work)}}
                    else {cache.copy_level(&scene,&mut r,&mut encoder,&mut commands)};
                r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
                let report=r.display_memory_report(Some(&scene),cache.frame_memory(&r,commands.storage_bytes()));
                let grant=r.memory.display_owner().grant().gpu_bytes;
                if report.gpu_bytes>grant {
                    failures.push(format!("exact={exact} frame={frame_index}: physical={} grant={grant} pinned={} Store={} projection={} initial_Scene={initial_scene} initial_Display={initial_display} window_bytes={window_bytes} result={result:?}",
                        report.gpu_bytes,report.pinned_gpu_bytes,cache.pages.lock().unwrap().bytes(),cache.window_bytes()));
                    break;
                }
                assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady|GpuRasterError::Waiting(_))),"{result:?}");
            }
            drop(held);
        }
        assert!(failures.is_empty(),"{}",failures.join("\n"));
    }

    #[test]
    fn refinement_replaces_display_pages_in_the_same_keyed_store() {
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),512,512,
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        let root=document.artwork.root;
        document.artwork.compositions.get_mut(root).unwrap().blend=layer_core::BlendSpace::Linear;
        let source=layer_core::color::source::rgba8_source([512;2],|x,y|[x as u8,y as u8,91,255]);
        paint_mut(&mut document,0).base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
        let mut renderer=display(&document,&source,0);
        let frame=display_frame(&document,0);
        let cache=renderer.scale_display.as_ref().unwrap();
        let key=cache.page_key(cache.graph.root.clone().unwrap(),page_rect([0;2]));
        let store=cache.pages.clone();
        assert!(store.lock().unwrap().get(&key,page_rect([0;2]),Accuracy::Display,key.level,crate::residency::Class::Visible).is_some());
        assert!(store.lock().unwrap().get(&key,page_rect([0;2]),Accuracy::Exact,0,crate::residency::Class::Visible).is_none(),
            "Display composition is not an exact capture");
        let coarse=Key {level:1,coordinate:[0;2],..key.clone()};
        let (_,accuracy)=store.lock().unwrap().reduction(&coarse,0,crate::residency::Class::Visible).unwrap();
        assert_eq!(accuracy,Accuracy::Display);
        let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
        loop {
            let mut scene=renderer.scene.take().unwrap();
            let mut encoder=submission::CommandEncoder::new(&renderer.device,&Default::default());
            let result={let mut work=crate::test_support::work(false);scene.refine_display(&mut renderer,frame,&mut encoder,&mut work)};
            renderer.scene=Some(scene);
            assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady|GpuRasterError::Waiting(_))),"{result:?}");
            renderer.uploads.finish(&encoder);encoder.submit(&renderer.queue);renderer.wait_idle().unwrap();
            if store.lock().unwrap().get(&key,page_rect([0;2]),Accuracy::Exact,0,crate::residency::Class::Visible).is_some() {break;}
            assert!(std::time::Instant::now()<deadline,"approximate pages must remain owed exact refinement");
        }
        assert!(store.same(&renderer.scale_display.as_ref().unwrap().pages));
    }
}

#[cfg(test)]
pub(super) struct RetainedPage {
    pub key: Key,
    pub texture: wgpu::Texture,
    pub written:PageSet,
    pub current:PageSet,
    pub exact:PageSet,
}
#[cfg(test)]
impl Store {
    pub(super) fn pinned(&self,key:&Key)->bool {self.pages.get(key).is_some_and(Page::pinned)}
    pub(super) fn remove_composites(&mut self) {
        let keys=self.pages.keys().filter(|key|key.source.is_none()).cloned().collect::<Vec<_>>();
        for key in keys {self.remove(&key);}
    }
    pub(super) fn remove_source_level(&mut self,id:SourceTarget,level:u32) {
        let keys=self.pages.keys().filter(|key|key.source==Some(id) && key.level==level).cloned().collect::<Vec<_>>();
        for key in keys {self.remove(&key);}
    }
    pub(super) fn discard_source_chunks(&mut self,id:SourceTarget,level:u32,area:PixelRect) {
        for (key,page) in &mut self.pages {
            if key.source!=Some(id) || key.level!=level {continue;}
            for coordinate in coordinates(area.intersect(key.plan().bounds),page.coverage.written.level) {
                page.versions.remove(&coordinate);page.coverage.written.entries.remove(&coordinate);page.coverage.exact.entries.remove(&coordinate);page.coverage.submissions.remove(&coordinate);
            }
        }
        for (key,inputs) in &mut self.inputs {
            if key.source==Some(id) && key.level==level {inputs.retain(|input|area.intersect(input.region).is_empty());}
        }
    }
    pub(super) fn retained_pages(&self) -> Vec<RetainedPage> {
        self.pages.iter().filter(|(_,page)|page.coverage.written.is_empty() || page.coverage.valid_written().next().is_some())
            .map(|(key,page)|RetainedPage {key:key.clone(),texture:page.image.texture.clone(),
                written:PageSet {level:page.coverage.written.level,entries:page.coverage.valid_written().copied().collect()},current:PageSet {level:page.coverage.written.level,entries:page.coverage.valid_written().copied().filter(|coordinate|
                    page.matches(self.expected.as_deref(),bounds(*coordinate,page.coverage.written.level,key.extent),key.level)==Some(true)).collect()},exact:page.coverage.exact.clone()}).collect()
    }
}

#[cfg(test)]
mod required_window_publication_tests {
    use super::*;

    #[test]
    fn current_exact_store_publishes_required_window_before_idle() {
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),4096,4096,
            layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth=layer_core::color::SampleDepth::F32;
        let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {
            host_default_bytes:Some(2<<30),override_bytes:Some(2<<30),..Default::default()
        });
        let mut r=crate::memory::memory_tests::renderer_in_budget(document.composition().color,
            layer_core::DeviceMemoryFacts {budget_bytes:Some(768<<20),unified:false,..Default::default()},budget).unwrap();
        let mut frame=crate::test_support::packet(document.scene(),[4096,4096]);
        frame.view=layer_render::ViewState {width_px:1024,height_px:1024,document_to_surface:[1.,0.,0.,1.,0.,0.]};
        r.ensure_document_metadata([4096,4096],document.scene()).unwrap();
        let page_bytes=u64::from(PAGE_SIZE).pow(2)*16;
        let optional_pages=page_coordinates(PixelRect::full(frame.document_extent)).count() as u64;
        let _budget=crate::memory::memory_tests::display_frame_with_budget(&mut r,frame,optional_pages*page_bytes);
        let mut scene=Scene::new(&r).unwrap();scene.scale_sources.prepare(&r,frame,&[]);
        let mut cache=Cache::new(&r,Request {plan:display_mips::Plan::window([4096,4096],0,PixelRect::new(0,0,1024,1024)),evaluation:Evaluation::Display},document.scene().order().len());
        crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
        cache.prepare_graph(&r,frame,&scene).unwrap();
        pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.scale.output(cache.precision.format()).reduce_pair])).unwrap();
        assert_ne!(cache.plan.level,overview_plan(cache.plan).level);
        let mut required=Vec::new();
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        for current in std::iter::once(&cache).chain(cache.overview.as_deref()) {
            for coordinate in coordinates(current.plan.bounds,current.plan.level) {
                let region=bounds(coordinate,current.plan.level,current.plan.extent);
                let mut key=current.page_key(current.graph.root.clone().unwrap(),region);
                key.format=wgpu::TextureFormat::Rgba32Float;
                let image=current.claim_page(&scene,&r,key.clone(),crate::residency::Class::Visible).unwrap().unwrap();
                let write=current.pages.lock().unwrap().prepare_write(key.clone(),region,Accuracy::Exact,0).unwrap();
                drop(encoder.color_pass("current exact required page",&image.view,wgpu::LoadOp::Clear(wgpu::Color::WHITE)));
                current.pages.lock().unwrap().commit(write,0,&mut encoder);
                current.pages.lock().unwrap().set_visible(&key);
                required.push((key,image));
            }
        }
        encoder.submit(&r.queue);crate::test_support::complete(&r);
        let mut commands=RegionRecords::new(&r);
        let mut overview=cache.overview.take().unwrap();
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        overview.copy_level(&scene,&mut r,&mut encoder,&mut commands).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        assert!(overview.pixels.committed_root().is_some());cache.overview=Some(overview);
        let branch=Arc::new(graph::Expression::Color([0,1f32.to_bits(),0,1f32.to_bits()]));
        let mut index=0u32;
        while r.scene_tiers.display.used().saturating_add(page_bytes)<=r.memory.display_owner().grant().gpu_bytes {
            let region=page_rect([index%16,index/16]);index+=1;
            assert!(u64::from(index)<=optional_pages,"bounded optional Store fixture");
            let key=cache.page_key(branch.clone(),region);
            let image=cache.claim_page(&scene,&r,key.clone(),crate::residency::Class::Visible).unwrap().unwrap();
            let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
            let write=cache.pages.lock().unwrap().prepare_write(key.clone(),region,Accuracy::Display,key.level).unwrap();
            drop(encoder.color_pass("optional subexpression retention",&image.view,wgpu::LoadOp::Clear(wgpu::Color::GREEN)));
            cache.pages.lock().unwrap().commit(write,0,&mut encoder);
            cache.pages.lock().unwrap().set_optional(&key);
            encoder.submit(&r.queue);crate::test_support::complete(&r);
        }
        let before=cache.pages.lock().unwrap().bytes();
        let optional_before=cache.pages.lock().unwrap().pages.iter().filter(|(key,page)|key.level==0 && page.optional).map(|(_,page)|page.image.bytes()).sum::<u64>();
        let window_bytes=cache.plan.size.map(u64::from).into_iter().product::<u64>()*16;
        assert!(optional_before>=window_bytes,"optional victims can pay for the whole required window");
        assert!(required.iter().all(|(key,_)|cache.pages.lock().unwrap().get(key,key.plan().bounds,Accuracy::Exact,0,crate::residency::Class::Visible).is_some()));
        assert!(cache.pixels.root().is_none());
        assert!(required.iter().all(|(_,image)|Arc::strong_count(image.image())>1));
        let required_bytes=required.iter().map(|(_,image)|image.bytes()).sum::<u64>();
        let grant_before=r.memory.display_owner().grant().gpu_bytes;
        assert!(required_bytes+cache.window_bytes()+commands.storage_bytes()+scene.working_bytes()+window_bytes<=grant_before,"required inputs and window fit after optional victims leave");
        assert!(r.scene_tiers.display.used()+window_bytes>grant_before,"the full display must require room for publication");
        let pending_before=cache.has_pending_work(&r);
        let mut attempts=Vec::new();
        for _ in 0..2 {
            let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
            attempts.push(cache.copy_level(&scene,&mut r,&mut encoder,&mut commands));
            r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
            let report=r.display_memory_report(Some(&scene),cache.frame_memory(&r,0));
            assert!(report.gpu_bytes<=r.memory.display_owner().grant().gpu_bytes,"every publication attempt remains admitted");
            if cache.pixels.committed_root().is_some() {break;}
        }
        let committed=cache.pixels.committed_root().is_some();
        let optional_after=cache.pages.lock().unwrap().pages.iter().filter(|(key,page)|key.level==0 && page.optional).map(|(_,page)|page.image.bytes()).sum::<u64>();
        let report=r.display_memory_report(Some(&scene),cache.frame_memory(&r,0));
        let grant=r.memory.display_owner().grant().gpu_bytes;
        for(key,image)in &required {assert!(cache.pages.lock().unwrap().pages.get(key).is_some_and(|page|Arc::ptr_eq(&page.image,image.image())),"projection reads must survive optional eviction");}
        assert!(pending_before && attempts.iter().any(|result|result.is_ok()) && committed && optional_after<optional_before && report.gpu_bytes<=grant,
            "required publication cannot sleep behind optional retention: pending={pending_before}, attempts={attempts:?}, committed={committed}, store_before={before}, optional={optional_before}->{optional_after}, window={window_bytes}, physical={}, grant={grant_before}->{grant}",report.gpu_bytes);
    }
}

#[cfg(test)]
mod freshness_tests {
    use super::*;
    use layer_core::{Document, authored::{Artwork,SourceTarget}, raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileKey}};

    fn fixture()->(Document,SourceTarget) {
        let mut artwork=Artwork::new([1024,512]).unwrap();
        let (_,target)=crate::test_support::add_paint(&mut artwork,"Paint",[1024,512]);
        (Document::from_artwork(artwork).unwrap(),target)
    }
    fn install(document:&mut Document,target:SourceTarget,tile:RasterTile) {
        let SourceTarget::Paint(handle)=target else {unreachable!()};
        document.artwork.paint.get_mut(handle).unwrap().raster=RasterRevision::backed(RasterData {
            tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0;2]},tile)].into(),..Default::default()});
    }
    fn frame(document:&Document)->Arc<SignatureFrame> {
        Arc::new(SignatureFrame::from_scene(crate::test_support::packet(document.scene(),document.composition().size),
            Declared {name_wake:Some(std::task::Waker::from(Arc::new(PackingNotification::default()))),..Default::default()}).unwrap())
    }
    #[derive(Default)]
    struct PackingNotification(std::sync::atomic::AtomicUsize);
    impl std::task::Wake for PackingNotification {
        fn wake(self:Arc<Self>) {self.wake_by_ref();}
        fn wake_by_ref(self:&Arc<Self>) {self.0.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
    }
    fn key(target:SourceTarget)->Key {
        Key {source:None,expression:Arc::new(graph::Expression::Source {id:target,frame:SourceFrame::at([0;2],[0;2]),extent:[1024,512],outside:0}),
            extent:[1024,512],color:Default::default(),blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[0;2],format:wgpu::TextureFormat::Rgba32Float}
    }
    fn accepted(store:&Store,key:&Key)->Recorded {
        store.prepare_write(key.clone(),key.plan().bounds,Accuracy::Exact,0).unwrap().accepted.unwrap().recorded
    }
    fn coverage(recorded:&Recorded,frame:&SignatureFrame)->Coverage {
        let mut coverage=Coverage::new(0);coverage.written.entries.insert([0;2]);
        coverage.include([0;2],recorded.matches(frame,0),true);coverage
    }
    #[test]
    fn stale_composite_spatial_recipes_do_not_acquire_a_new_authored_witness() {
        use layer_core::authored::{PortableId,Occurrence,OccurrenceContent,ImageObject,Image};
        use layer_core::authored::vector::*;
        let mut failures=Vec::new();
        for kind in ["paint","vector","objects"] {
            let (mut document,target)=fixture();
            install(&mut document,target,RasterTile::pending(RasterPlane::Color.descriptor(Default::default())));
            let mut object=None;
            let owner=match kind {
                "paint"=>document.scene().source_owner(target).unwrap(),
                "vector"=> {
                    let geometry=PathGeometry::new(vec![Subpath::new(&[[0.,0.],[256.,128.]].map(Anchor::corner),false)]).unwrap();
                    let path=PathItem::new(ItemId(1),Arc::new(geometry),Arc::new(Style::default()));
                    let vector=document.artwork.vectors.insert(PortableId::random(),VectorObject::from_records(vec![Record::Path(path)])).unwrap();
                    document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Vector(vector),"Vector")).unwrap()
                },
                _=> {
                    let image=Image::new(layer_core::color::source::rgba8_source([1;2],|_,_|[255;4]));
                    let handle=document.artwork.objects.insert(PortableId::random(),ImageObject::new(image)).unwrap();object=Some(handle);
                    document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Object")).unwrap()
                },
            };
            if kind!="paint" {document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.push(owner);document=Document::from_artwork(document.artwork.clone()).unwrap();}
            let mut index=object_spatial::SpatialIndex::default();
            let expression=match kind {
                "paint"=>key(target).expression,
                "vector"=>Arc::new(graph::Expression::Vector {owner,offset:[0;2],moving:None}),
                _=>Arc::new(graph::Expression::Objects {content:index.content(document.scene(),owner).unwrap(),preview:false}),
            };
            let key=Key {expression,..key(target)};let before=frame(&document);
            let mut store=Store::default();store.set_expected(before.clone());let written=accepted(&store,&key);
            let raw=if kind=="paint" {Some(accepted(&store,&Key {source:Some(target),..key.clone()}))}else {None};
            if let Some(object)=object {document.artwork.objects.get_mut(object).unwrap().affine.0[4]=128.;document.artwork.objects.get_mut(object).unwrap().affine.0[5]=64.;}
            else {document.artwork.occurrences.get_mut(owner).unwrap().offset=[128,64];}
            let after=frame(&document);let authored=after.authored_revision(&key.expression).unwrap();
            assert_ne!(written.authored,authored,"{kind}: the selected model actually moved");
            let expected=after.expected_key(&key,key.plan().bounds,0).unwrap();
            if written.version.matches(expected.version())!=Some(false) {failures.push(format!("{kind}: old spatial pixel recipe still matches after selected move"));}
            if written.matches(&after,0)!=Some(false) {failures.push(format!("{kind}: recorded page is incorrectly current"));}
            if authored_matches(std::slice::from_ref(&written),&after,authored,key.plan().bounds) {failures.push(format!("{kind}: old pixels acquired the new authored witness"));}
            if let Some(raw)=raw {assert_eq!(raw.matches(&after,0),Some(true),"unchanged local source pixels remain reusable");}
        }
        assert!(failures.is_empty(),"{}",failures.join("\n"));
    }
    #[test]
    fn accepted_page_versions_are_not_replaced_by_a_later_frame_and_undo_reuses_the_witness() {
        let (mut document,target)=fixture();let descriptor=RasterPlane::Color.descriptor(Default::default());
        install(&mut document,target,RasterTile::pending(descriptor));let before=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(before.clone());let written=accepted(&store,&key);
        install(&mut document,target,RasterTile::pending(descriptor));let after=frame(&document);store.set_expected(after.clone());
        assert_eq!(written.matches(&after,0),Some(false));
        assert!(coverage(&written,&after).needs(key.plan().bounds,Accuracy::Display));
        store.set_expected(before.clone());assert_eq!(written.matches(store.expected().unwrap(),0),Some(true));
        assert!(coverage(&written,&before).ready(key.plan().bounds,Accuracy::Exact));
    }
    #[test]
    fn a_source_write_after_its_held_acceptance_goes_stale_records_the_current_frame() {
        let (mut document,target)=fixture();let descriptor=RasterPlane::Color.descriptor(Default::default());
        install(&mut document,target,RasterTile::pending(descriptor));let source=Key {source:Some(target),..key(target)};let region=source.plan().bounds;
        let mut store=Store::default();store.set_expected(frame(&document));
        hold_fixture_inputs(&mut store,&source,region,Evaluation::Display,Default::default(),0);
        install(&mut document,target,RasterTile::pending(descriptor));let after=frame(&document);store.set_expected(after.clone());
        assert_eq!(store.fixture_handle(&source,region,0).recorded.matches(&after,0),Some(false),"the held acceptance belongs to the earlier frame");
        assert_eq!(accepted(&store,&source).matches(&after,0),Some(true),"a write records the version its frame expects, not a held earlier one");
    }
    #[test]
    fn undeclared_expected_inputs_need_preparation_instead_of_waiting_for_packing() {
        let (mut document,target)=fixture();let before=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(before);let written=accepted(&store,&key);
        let SourceTarget::Paint(handle)=target else {unreachable!()};
        document.artwork.paint.get_mut(handle).unwrap().raster=RasterRevision::pending();
        let expected=frame(&document);
        assert!(matches!(expected.expected_key(&key,key.plan().bounds,0),Err(GpuRasterError::NotReady|GpuRasterError::Waiting(_))));
        let coverage=coverage(&written,&expected);
        assert!(coverage.pending.is_empty(),"missing expected inputs are not an in-flight packing comparison");
        assert!(coverage.needs(key.plan().bounds,Accuracy::Display));
    }
    #[test]
    fn unresolved_packing_neither_requests_recomposition_nor_relabels_written_pixels() {
        let (mut document,target)=fixture();let tile=RasterTile::pending(RasterPlane::Color.descriptor(Default::default()));
        install(&mut document,target,tile.clone());let before=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(before);let written=accepted(&store,&key);let identity=written.version.identity();
        let (canonical,comparison)=RasterTile::pending_from_live(tile.descriptor(),tile.content_version());
        install(&mut document,target,canonical);let packing=frame(&document);
        assert_eq!(written.matches(&packing,0),None);let pending=coverage(&written,&packing);
        assert!(Coverage::covers(&pending.written,key.plan().bounds));
        assert!(!pending.ready(key.plan().bounds,Accuracy::Display));assert!(!pending.needs(key.plan().bounds,Accuracy::Exact));
        comparison.resolve(false).unwrap();assert_eq!(written.matches(&packing,0),Some(true));
        assert!(coverage(&written,&packing).ready(key.plan().bounds,Accuracy::Exact));assert_eq!(written.version.identity(),identity);
    }
    #[test]
    fn packing_is_pending_only_after_subscribing_to_its_comparison() {
        let (mut document,target)=fixture();let tile=RasterTile::pending(RasterPlane::Color.descriptor(Default::default()));
        install(&mut document,target,tile.clone());let before=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(before);let written=accepted(&store,&key);
        let (canonical,comparison)=RasterTile::pending_from_live(tile.descriptor(),tile.content_version());
        install(&mut document,target,canonical);
        let packet=crate::test_support::packet(document.scene(),document.composition().size);
        let unregistered=SignatureFrame::from_scene(packet,Declared::default()).unwrap();
        assert_eq!(written.matches(&unregistered,0),Some(false));
        assert!(coverage(&written,&unregistered).needs(key.plan().bounds,Accuracy::Display));
        let notification=Arc::new(PackingNotification::default());
        let registered=SignatureFrame::from_scene(packet,Declared {
            name_wake:Some(std::task::Waker::from(notification.clone())),..Default::default()
        }).unwrap();
        assert_eq!(written.matches(&registered,0),None);
        assert!(!coverage(&written,&registered).needs(key.plan().bounds,Accuracy::Display));
        comparison.resolve(false).unwrap();
        assert_eq!(notification.0.load(std::sync::atomic::Ordering::Relaxed),1);
        assert_eq!(written.matches(&registered,0),Some(true));
    }
    #[test]
    fn quantized_packing_is_realization_work_and_keeps_the_authored_origin() {
        let (mut document,target)=fixture();let tile=RasterTile::pending(RasterPlane::Color.descriptor(Default::default()));
        install(&mut document,target,tile.clone());let before=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(before);let written=accepted(&store,&key);
        let (canonical,comparison)=RasterTile::pending_from_live(tile.descriptor(),tile.content_version());
        install(&mut document,target,canonical);let packing=frame(&document);comparison.resolve(true).unwrap();
        assert_eq!(written.matches(&packing,0),Some(false));assert!(coverage(&written,&packing).needs(key.plan().bounds,Accuracy::Display));
        assert_eq!(written.version.authored(),packing.expected_key(&key,key.plan().bounds,0).unwrap().version().authored());
    }
    #[test]
    fn a_coarser_reader_publishes_a_derived_finer_window_and_a_target_level_producer() {
        let (document,target)=fixture();let frame=frame(&document);let source=key(target);
        let mut store=Store::default();store.set_expected(frame.clone());let written=accepted(&store,&source);
        let output=Key {level:1,..source.clone()};let area=source.plan().bounds;
        let authored=frame.authored_revision(&output.expression).unwrap();
        assert_eq!(written.matches(&frame,2),Some(true));
        assert_eq!(publication_matches(&frame,&output,area,2,Some((authored,std::slice::from_ref(&written)))),Some(true),
            "a coarser reader derived from a current finer window publishes its records");
        let coarser=Recorded {key:Key {level:2,..source.clone()},..written.clone()};
        assert_eq!(publication_matches(&frame,&output,area,2,Some((authored,&[coarser]))),Some(false),
            "a coarser record never publishes a finer reader");
        let same_level=accepted(&store,&output);
        assert_eq!(same_level.input_level,0);
        assert_eq!(publication_matches(&frame,&output,area,2,Some((authored,&[same_level]))),Some(true),
            "a target-level producer may retain finer exact inputs");
    }

    #[test]
    fn a_publication_cannot_match_a_new_root_only_because_its_old_expression_still_matches() {
        let (document,target)=fixture();let frame=frame(&document);let mut key=key(target);
        key.expression=Arc::new(graph::Expression::Opacity {input:key.expression.clone(),opacity:0.5f32.to_bits()});
        let mut store=Store::default();store.set_expected(frame.clone());let written=accepted(&store,&key);
        let authored=frame.authored_revision(&key.expression).unwrap();
        let next=Key {expression:Arc::new(graph::Expression::Opacity {input:key.expression.clone(),opacity:0.25f32.to_bits()}),..key.clone()};
        assert_eq!(written.matches(&frame,0),Some(true));
        assert_eq!(publication_matches(&frame,&key,key.plan().bounds,0,Some((authored,std::slice::from_ref(&written)))),Some(true));
        assert_eq!(publication_matches(&frame,&next,next.plan().bounds,0,Some((authored,&[written]))),Some(false));
    }
    #[test]
    fn clipped_copies_keep_the_input_footprint_without_claiming_unwritten_window_pixels() {
        let (document,target)=fixture();let frame=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(frame.clone());let written=accepted(&store,&key);
        let clipped=written.clipped(PixelRect::new(0,0,128,256)).unwrap();assert_eq!(clipped.region,written.region);
        assert_eq!(recorded_matches(std::slice::from_ref(&clipped),&frame,clipped.written,0),Some(true));
        assert_eq!(recorded_matches(&[clipped],&frame,key.plan().bounds,0),Some(false));
    }
    #[test]
    fn a_new_authored_input_is_needed_even_while_its_canonical_comparison_is_unresolved() {
        let (mut document,target)=fixture();let descriptor=RasterPlane::Color.descriptor(Default::default());
        install(&mut document,target,RasterTile::pending(descriptor));let before=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(before.clone());let old=accepted(&store,&key);
        let next=RasterTile::pending(descriptor);let (canonical,_comparison)=RasterTile::pending_from_live(descriptor,next.content_version());
        install(&mut document,target,canonical);let pending=frame(&document);
        assert!(matches!(pending.expected_key(&key,key.plan().bounds,0).unwrap(),signatures::ExpectedKey::Unresolved(_)));
        assert_eq!(old.matches(&pending,0),Some(false));assert!(coverage(&old,&pending).needs(key.plan().bounds,Accuracy::Display));
        let authored=before.authored_revision(&key.expression).unwrap();
        assert_eq!(publication_matches(&pending,&key,key.plan().bounds,0,Some((authored,&[old]))),Some(false));
    }
    #[test]
    fn current_required_quality_is_not_inferred_from_a_historical_coarse_witness() {
        let (document,target)=fixture();let frame=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(frame.clone());
        let written=store.prepare_write(key.clone(),key.plan().bounds,Accuracy::Display,2).unwrap().accepted.unwrap().recorded;
        let authored=frame.authored_revision(&key.expression).unwrap();
        assert_eq!(written.matches(&frame,2),Some(true));assert_eq!(written.matches(&frame,0),Some(false));
        assert_eq!(publication_matches(&frame,&key,key.plan().bounds,0,Some((authored,std::slice::from_ref(&written)))),Some(false));
        assert_eq!(publication_matches(&frame,&key,key.plan().bounds,2,Some((authored,&[written]))),Some(true));
    }

    #[test]
    fn authored_projection_allows_camera_crops_but_rejects_a_mixed_edit() {
        let (mut document,target)=fixture();let descriptor=RasterPlane::Color.descriptor(Default::default());
        install(&mut document,target,RasterTile::pending(descriptor));let before=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(before.clone());let old=accepted(&store,&key);
        let left=old.clipped(PixelRect::new(0,0,128,256)).unwrap();
        let right=old.clipped(PixelRect::new(128,0,256,256)).unwrap();
        assert!(authored_matches(&[left.clone(),right],&before,old.authored,key.plan().bounds));
        assert!(authored_matches(std::slice::from_ref(&left),&before,old.authored,left.written));
        install(&mut document,target,RasterTile::pending(descriptor));let after=frame(&document);store.set_expected(after.clone());
        let next=accepted(&store,&key);let right=next.clipped(PixelRect::new(128,0,256,256)).unwrap();
        assert!(!authored_matches(&[left,right],&after,next.authored,key.plan().bounds));
    }
    #[test]
    fn authored_projection_keeps_live_pixels_during_same_authored_packing() {
        let (mut document,target)=fixture();let tile=RasterTile::pending(RasterPlane::Color.descriptor(Default::default()));
        install(&mut document,target,tile.clone());let before=frame(&document);let key=key(target);
        let mut store=Store::default();store.set_expected(before);let written=accepted(&store,&key);
        let (canonical,comparison)=RasterTile::pending_from_live(tile.descriptor(),tile.content_version());
        install(&mut document,target,canonical);let packing=frame(&document);let authored=packing.authored_revision(&key.expression).unwrap();
        assert_eq!(written.matches(&packing,0),None);
        assert!(authored_matches(std::slice::from_ref(&written),&packing,authored,key.plan().bounds));
        comparison.resolve(true).unwrap();
        assert_eq!(written.matches(&packing,0),Some(false));
        assert!(authored_matches(&[written],&packing,authored,key.plan().bounds));
    }
}

#[cfg(test)]
mod fit_return_tests {
    use super::*;
    #[test]
    fn a_claim_takes_a_background_window_first_and_keeps_an_active_window_within_its_share() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let side=8;let extent=[side*PAGE_SIZE;2];
        let document=super::super::tests::document_at(extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),extent));
        let key=|index:u32|Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent,color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level:0,coordinate:[index%side,index/side],format:wgpu::TextureFormat::Rgba8Unorm};
        let [active,other,background]=[WindowId::new(),WindowId::new(),WindowId::new()];
        let page=(0..4).map(|index|store.claim(&r,key(index),u64::MAX,background,crate::residency::Class::Visible).unwrap().bytes()).max().unwrap();
        for _ in 0..2 {let frame=store.expected().unwrap().clone();store.set_expected(frame);}
        for index in 4..8 {drop(store.claim(&r,key(index),u64::MAX,active,crate::residency::Class::Visible).unwrap());}
        for index in 8..12 {drop(store.claim(&r,key(index),u64::MAX,other,crate::residency::Class::Visible).unwrap());}
        let present=|store:&Store,range:std::ops::Range<u32>|range.filter(|index|store.pages.contains_key(&key(*index))).count();
        for index in 12..16 {drop(store.claim(&r,key(index),12*page,active,crate::residency::Class::Visible).unwrap());}
        assert_eq!([present(&store,0..4),present(&store,4..8),present(&store,8..12)],[0,4,4],"the background window ages out first");
        for index in 16..20 {drop(store.claim(&r,key(index),12*page,active,crate::residency::Class::Visible).unwrap());}
        assert_eq!(present(&store,8..12),4,"an active window within its equal share keeps its pages");
        assert_eq!([present(&store,4..8),present(&store,12..20)],[0,8],"the active window evicts its own oldest pages");
        assert!(store.bytes()<=12*page);
    }
    #[test]
    fn a_claim_evicts_a_stale_coarse_page_before_the_visible_pages_the_current_frame_touched() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let side=8;let extent=[side*PAGE_SIZE;2];
        let document=super::super::tests::document_at(extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),extent));
        let key=|level:u32,coordinate:[u32;2]|Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent,color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level,coordinate,format:wgpu::TextureFormat::Rgba8Unorm};
        let stale=key(3,[0,0]);
        let page=store.claim(&r,stale.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap().bytes();
        let visible=(0..4).map(|index|key(0,[index,0])).collect::<Vec<_>>();
        for key in &visible {drop(store.claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap());}
        for _ in 0..3 {
            let frame=store.expected().unwrap().clone();store.set_expected(frame);
            for key in &visible {drop(store.allocated(key,crate::residency::Class::Visible).unwrap());}
        }
        drop(store.claim(&r,key(0,[4,0]),5*page,WindowId::default(),crate::residency::Class::Visible).unwrap());
        let evicted=visible.iter().filter(|key|!store.pages.contains_key(key)).count();
        assert_eq!(evicted,0,"the claim evicted {evicted} visible pages the current frame touched");
        assert!(!store.pages.contains_key(&stale),"the stale coarse page stays");
    }
    #[test]
    fn an_optional_victim_never_raises_inflation_above_a_resident_page() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let side=8;let extent=[side*PAGE_SIZE;2];
        let document=super::super::tests::document_at(extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),extent));
        let key=|level:u32,coordinate:[u32;2]|Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent,color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level,coordinate,format:wgpu::TextureFormat::Rgba8Unorm};
        let page=store.claim(&r,key(0,[0,0]),u64::MAX,WindowId::default(),Class::Visible).unwrap().bytes();
        drop(store.claim(&r,key(3,[0,0]),u64::MAX,WindowId::default(),Class::Visible).unwrap());store.set_optional(&key(3,[0,0]));
        drop(store.claim(&r,key(0,[1,0]),2*page,WindowId::default(),Class::Visible).unwrap());
        assert!(!store.pages.contains_key(&key(3,[0,0])),"the optional page goes first");
        let lowest=store.pages.values().map(|page|page.priority).min().unwrap();
        assert!(store.victims.inflation<=lowest,"inflation {} exceeds a resident priority {lowest}",store.victims.inflation);
    }
    #[test]
    fn an_inactive_window_is_evicted_before_an_active_window_over_its_share() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let side=8;let extent=[side*PAGE_SIZE;2];
        let document=super::super::tests::document_at(extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),extent));
        let key=|level:u32,coordinate:[u32;2]|Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent,color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level,coordinate,format:wgpu::TextureFormat::Rgba8Unorm};
        let [active,other,background]=[WindowId::new(),WindowId::new(),WindowId::new()];
        let page=store.claim(&r,key(3,[0,0]),u64::MAX,background,Class::Visible).unwrap().bytes();
        for _ in 0..2 {let frame=store.expected().unwrap().clone();store.set_expected(frame);}
        for index in 0..3 {drop(store.claim(&r,key(0,[index,0]),u64::MAX,active,Class::Visible).unwrap());}
        drop(store.claim(&r,key(0,[0,1]),u64::MAX,other,Class::Visible).unwrap());
        drop(store.claim(&r,key(0,[1,1]),5*page,other,Class::Visible).unwrap());
        assert!(!store.pages.contains_key(&key(3,[0,0])),"the inactive window keeps its page while the active window over its share loses one");
        assert_eq!((0..3).filter(|index|store.pages.contains_key(&key(0,[*index,0]))).count(),3);
    }
    #[test]
    fn fit_return_after_a_pan_to_pressure_needs_no_rebuild() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let side=16;let extent=[side*PAGE_SIZE;2];let fit=3;
        let document=super::super::tests::document_at(extent);
        let mut store=fixture_store(crate::test_support::packet(document.scene(),extent));
        let key=|level:u32,coordinate:[u32;2]|Key {source:None,expression:Arc::new(graph::Expression::Color([0;4])),extent,color:r.document_color(),
            blend_space:layer_core::BlendSpace::Linear,level,coordinate,format:wgpu::TextureFormat::Rgba8Unorm};
        let fit_keys=coordinates(PixelRect::full(extent),fit).map(|coordinate|key(fit,coordinate)).collect::<Vec<_>>();
        let page=fit_keys.iter().map(|key|store.claim(&r,key.clone(),u64::MAX,WindowId::default(),crate::residency::Class::Visible).unwrap().bytes()).max().unwrap();
        let limit=(fit_keys.len() as u64+16)*page;
        for index in 0..96u32 {
            let coordinate=[index%side,(index/side*5)%side];
            drop(store.claim(&r,key(0,coordinate),limit,WindowId::default(),crate::residency::Class::Visible).unwrap());
            assert!(store.bytes()<=limit);
        }
        let missing=fit_keys.iter().filter(|key|!store.pages.contains_key(key)).count();
        assert_eq!(missing,0,"returning to Fit rebuilds {missing} of {} pages the pan pushed out",fit_keys.len());
    }
}
