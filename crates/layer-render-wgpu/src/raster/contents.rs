use super::*;
use layer_core::raster::{ContentTree, ContentVersion};

struct Fork {
    pages: [ContentTree; 3],
    before: [ContentTree; 3],
    submission: submission::Submission,
}

pub(crate) struct Contents {
    selected: Arc<RasterData>,
    fork: Option<Fork>,
    selection: Arc<()>,
    defaults: [ContentVersion; 3],
    generation: [u64; 2],
}

#[derive(Clone)]
pub(crate) struct Versions {
    pages: [ContentTree; 3],
    defaults: [ContentVersion; 3],
}

impl Versions {
    pub fn authored(&self,plane:RasterPlane,coordinate:[u32;2])->bool {self.pages[index(plane)].tile_version(coordinate).is_some()}
    pub fn tile(&self,plane:RasterPlane,coordinate:[u32;2])->ContentVersion {
        self.pages[index(plane)].tile_version(coordinate).unwrap_or_else(||fallback(&self.defaults[index(plane)],coordinate))
    }
    pub fn region(&self, plane: RasterPlane, region: PixelRect) -> ContentVersion {
        let bounds = [region.min_x() / PAGE_SIZE, region.min_y() / PAGE_SIZE,
            region.max_x().div_ceil(PAGE_SIZE), region.max_y().div_ceil(PAGE_SIZE)];
        ContentVersion::derive(index(plane) as u128,
            &[self.defaults[index(plane)].clone(), self.pages[index(plane)].region_version(bounds)])
    }

    pub fn root(&self, plane: RasterPlane) -> ContentVersion {
        ContentVersion::derive(index(plane) as u128,
            &[self.defaults[index(plane)].clone(), self.pages[index(plane)].root_version()])
    }

    pub fn changes(&self, after: &Self, mut changed: impl FnMut([u32; 2])) -> bool {
        if self.defaults.iter().zip(&after.defaults).any(|(before, after)| before.identity() != after.identity()) { return false; }
        for (before, after) in self.pages.iter().zip(&after.pages) { before.changes(after, &mut changed); }
        true
    }

    #[cfg(test)]
    pub fn authored_changes(&self, after: &Self, plane: RasterPlane, changed: impl FnMut([u32; 2])) -> bool {
        let plane = index(plane);
        if self.defaults[plane].authored() != after.defaults[plane].authored() { return false; }
        self.pages[plane].authored_changes(&after.pages[plane], changed);
        true
    }
}

#[derive(Clone)]
pub(crate) struct Intent {
    key: TileKey,
    version: ContentVersion,
    selection: Arc<()>,
}
impl Intent {
    pub fn version(&self) -> &ContentVersion { &self.version }
    pub fn same_selection(&self,other:&Self)->bool {Arc::ptr_eq(&self.selection,&other.selection)}
}

pub(super) fn planes() -> [RasterPlane; 3] { [RasterPlane::Color, RasterPlane::Mask, RasterPlane::WatercolorWetness] }
pub(crate) fn index(plane: RasterPlane) -> usize {
    match plane { RasterPlane::Color => 0, RasterPlane::Mask => 1, RasterPlane::WatercolorWetness => 2 }
}

pub(crate) fn defaults_for(extent:[u32;2],base:Option<&layer_core::authored::PaintBase>,coverage:Option<f32>)->[ContentVersion;3] {
    let [width,height]=extent;
    let domain=ContentVersion::derive(u128::from(width)|u128::from(height)<<32,&[]);
    let mut defaults=std::array::from_fn(|plane|ContentVersion::derive(plane as u128,std::slice::from_ref(&domain)));
    if let Some(base)=base {
        let settings=u128::from(base.offset[0])|u128::from(base.offset[1])<<32|(base.policy as u128)<<64;
        defaults[0]=ContentVersion::derive(settings,&[base.image.content_version(),domain]);
    }
    if let Some(coverage)=coverage {defaults[1]=ContentVersion::derive(u128::from(coverage.to_bits()),std::slice::from_ref(&defaults[1]));}
    defaults
}
pub(crate) fn defaults(scene:SceneView<'_>,target:SourceTarget)->[ContentVersion;3] {
    let coverage=if let SourceTarget::Coverage(coverage)=target {scene.coverage(coverage).map(|source|source.default_coverage)}else {None};
    defaults_for(scene.target_extent(target),scene.paint_base(target),coverage)
}
pub(crate) fn fallback(default:&ContentVersion,coordinate:[u32;2])->ContentVersion {
    ContentVersion::derive(u128::from(coordinate[0])|u128::from(coordinate[1])<<32,std::slice::from_ref(default))
}
pub(crate) fn selected_version(scene:SceneView<'_>,target:SourceTarget,data:&RasterData,key:TileKey)->ContentVersion {
    data.tiles.get(&key).map_or_else(||fallback(&defaults(scene,target)[index(key.plane)],key.coordinate),RasterTile::content_version)
}

pub(crate) fn mask_default(domain:[u32;2], coverage:f32, coordinate: [u32;2]) -> ContentVersion {
    let [width,height]=domain;
    let domain=ContentVersion::derive(u128::from(width)|u128::from(height)<<32,&[]);
    let plane=ContentVersion::derive(index(RasterPlane::Mask) as u128,&[domain]);
    let default=ContentVersion::derive(u128::from(coverage.to_bits()),&[plane]);
    ContentVersion::derive(u128::from(coordinate[0])|u128::from(coordinate[1])<<32,&[default])
}

pub(crate) fn command_mask(source:&layer_core::CoverageSource,selection:Option<&layer_core::Selection>,coordinate:[u32;2])
    ->Result<ContentVersion,GpuRasterError> {
    if let Some(selection)=selection {
        return Ok(ContentVersion::derive(0x6d61736b,&[mask_default(source.domain,source.default_coverage,coordinate),selection.content_version()]));
    }
    let data=source.raster.try_data().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?;
    Ok(data.tiles.get(&TileKey {plane:RasterPlane::Mask,coordinate}).map_or_else(||mask_default(source.domain,source.default_coverage,coordinate),RasterTile::content_version))
}

impl WgpuRasterizer {
    pub(crate) fn coverage_versions(&self,coverage:&layer_core::CoverageSnapshot)->Result<Versions,GpuRasterError> {
        let target=SourceTarget::Coverage(coverage.target);
        let mut versions=if let Some(current)=self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target)).filter(|current|current.revision==coverage.source.raster) {current.contents.versions()}
            else {let data=coverage.source.raster.try_data().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?;Contents::new(data).versions()};
        versions.defaults=defaults_for(coverage.source.domain,None,Some(coverage.source.default_coverage));Ok(versions)
    }

    pub(crate) fn raster_versions(&self,scene:SceneView<'_>,target:SourceTarget)->Result<Versions,GpuRasterError> {
        let revision=scene.raster(target).ok_or(GpuRasterError::NotReady)?;
        if let Some(current)=self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target))
            && &current.revision==revision {return Ok(current.contents.versions());}
        declared_versions(scene,target)
    }
    pub(crate) fn current_raster_versions(&self,target:SourceTarget)->Result<Versions,GpuRasterError> {
        self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target)).map(|target|target.contents.versions())
            .ok_or(GpuRasterError::NotReady)
    }
    pub(crate) fn current_raster_version(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Result<ContentVersion,GpuRasterError> {
        self.raster_intent(target,plane,coordinate).map(|intent|intent.version().clone())
    }
    pub(crate) fn read_layer_color(&self,target:SourceTarget,coordinate:[u32;2],expected:&ContentVersion,consumer:Option<&submission::Submission>)
        ->Option<crate::resident::Read<'_,PageSurface>> {
        self.paint_layers.iter().find(|layer|layer.id==target)?.pages.iter().find(|page|page.coordinate==coordinate)?.active().read(expected,consumer)
    }

    pub(crate) fn raster_version_for(&self,scene:SceneView<'_>,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Result<ContentVersion,GpuRasterError> {
        let root=scene.raster(target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
        if let Some(current)=self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target)).filter(|current|current.revision==*root) {
            return Ok(current.contents.version(TileKey {plane,coordinate}));
        }
        let data=root.try_data().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?;
        Ok(selected_version(scene,target,&data,TileKey {plane,coordinate}))
    }
    fn frame_version_for(&self,scene:SceneView<'_>,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],wanted:Option<&scene::SignatureFrame>)->Result<ContentVersion,GpuRasterError> {
        match wanted.and_then(|frame|frame.tile_version(target,plane,coordinate)) {Some(version)=>Ok(version),None=>self.raster_version_for(scene,target,plane,coordinate)}
    }
    pub(crate) fn raster_tile_for(&self,scene:SceneView<'_>,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Result<Option<Arc<TileBlob>>,GpuRasterError> {
        let root=scene.raster(target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
        let current=self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target)).filter(|current|current.revision==*root);
        let data=if let Some(current)=current {current.contents.data().clone()}else {
            root.try_data().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?};
        let key=TileKey {plane,coordinate};
        let Some(tile)=data.tiles.get(&key) else {
            if let Some(current)=current && current.contents.version(key).matches(&current.contents.selected_version(key))!=Some(true) {return Err(GpuRasterError::NotReady);}
            return Ok(None);
        };
        if tile.content_version().matches(&self.raster_version_for(scene,target,plane,coordinate)?)!=Some(true) {return Err(GpuRasterError::NotReady);}
        tile.try_backing().ok_or(GpuRasterError::NotReady)?.map(Some).map_err(GpuRasterError::Effect)
    }
    fn preview_input_version(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],wanted:Option<&scene::SignatureFrame>)->Result<ContentVersion,GpuRasterError> {
        if let Some(frame)=wanted {
            frame.preview_for(target).ok_or(GpuRasterError::NotReady)?.version(target,plane,coordinate,scene::PreviewStage::Final)
        }else {self.preview_expected(target,plane,coordinate,scene::PreviewStage::Final)}
    }
    pub(crate) fn read_scene_color_input<'a>(&'a self,scene:SceneView<'_>,target:SourceTarget,page:&'a LayerPage,predicted:bool,wanted:Option<&scene::SignatureFrame>,consumer:Option<&submission::Submission>)
        ->Result<Option<crate::resident::Read<'a,PageSurface>>,GpuRasterError> {
        if predicted {
            let expected=self.preview_input_version(target,RasterPlane::Color,page.coordinate,wanted)?;
            return page.active().read(expected,consumer).map(Some).ok_or(GpuRasterError::NotReady);
        }
        let expected=self.frame_version_for(scene,target,RasterPlane::Color,page.coordinate,wanted)?;
        Ok(page.active().read(expected,consumer))
    }
    pub(crate) fn read_scene_wetness_input<'a>(&'a self,scene:SceneView<'_>,target:SourceTarget,page:&'a WatercolorWetnessPage,predicted:bool,wanted:Option<&scene::SignatureFrame>,consumer:Option<&submission::Submission>)
        ->Result<Option<crate::resident::Read<'a,PageSurface>>,GpuRasterError> {
        if predicted {
            let expected=self.preview_input_version(target,RasterPlane::WatercolorWetness,page.coordinate,wanted)?;
            return page.active().read(expected,consumer).map(Some).ok_or(GpuRasterError::NotReady);
        }
        let expected=self.frame_version_for(scene,target,RasterPlane::WatercolorWetness,page.coordinate,wanted)?;
        Ok(page.active().read(expected,consumer))
    }
    pub(crate) fn read_wetness<'a>(&self,target:SourceTarget,page:&'a WatercolorWetnessPage,consumer:Option<&submission::Submission>)
        ->Result<crate::resident::Read<'a,PageSurface>,GpuRasterError> {
        let expected=self.current_raster_version(target,RasterPlane::WatercolorWetness,page.coordinate)?;
        page.active().read(expected,consumer).ok_or(GpuRasterError::NotReady)
    }
    pub(crate) fn read_color_page<'a>(&self,target:SourceTarget,page:&'a LayerPage,preview:bool,consumer:Option<&submission::Submission>)
        ->Result<crate::resident::Read<'a,PageSurface>,GpuRasterError> {
        let intent=self.raster_intent(target,RasterPlane::Color,page.coordinate)?;
        let expected=if preview {self.preview_expected(target,RasterPlane::Color,page.coordinate,scene::PreviewStage::Final)?}else {intent.version().clone()};
        page.active().read(expected,consumer).ok_or(GpuRasterError::NotReady)
    }
    pub(crate) fn raster_intent(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Result<Intent,GpuRasterError> {
        self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target))
            .map(|target|target.contents.intent(TileKey {plane,coordinate})).ok_or(GpuRasterError::Invariant("authored target is not registered"))
    }
    pub(crate) fn mask_version(&self,target:SourceTarget,source:&layer_core::CoverageSource,coordinate:[u32;2])->Result<ContentVersion,GpuRasterError> {
        if let Some(current)=self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target))
            && current.revision==source.raster {
            return Ok(current.contents.version(TileKey {plane:RasterPlane::Mask,coordinate}));
        }
        command_mask(source,None,coordinate)
    }
    pub(crate) fn mask_data(&self,target:SourceTarget,source:&layer_core::CoverageSource)->Result<Arc<RasterData>,GpuRasterError> {
        if let Some(data)=source.raster.try_data() {return data.map_err(GpuRasterError::Effect);}
        self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target)).filter(|target|target.revision==source.raster)
            .map(|target|target.contents.data().clone()).ok_or(GpuRasterError::NotReady)
    }
    pub(crate) fn current_mask_version(&self,target:SourceTarget,coordinate:[u32;2])->Result<ContentVersion,GpuRasterError> {
        let source=self.layer_masks.definitions.get(&target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
        self.mask_version(target,source,coordinate)
    }
    pub(crate) fn read_mask(&self,target:SourceTarget,source:&layer_core::CoverageSource,coordinate:[u32;2],wanted:Option<&scene::SignatureFrame>,
        receipt:Option<&submission::Submission>)->Result<Option<crate::resident::Read<'_,crate::layer_masks::MaskPixels>>,GpuRasterError> {
        let selected=self.mask_version(target,source,coordinate)?;
        let expected=if let Some(frame)=wanted {
            if let Some(capture)=frame.capture_for(target) {
                self.transforms.as_ref().ok_or(GpuRasterError::NotReady)?.requested_version_for_capture(self,capture,RasterPlane::Mask,coordinate)?
            } else if let Some(preview)=frame.preview_for(target).filter(|preview|preview.affects(page_rect(coordinate))) {
                preview.version(target,RasterPlane::Mask,coordinate,scene::PreviewStage::Final)?
            } else {frame.tile_version(target,RasterPlane::Mask,coordinate).unwrap_or_else(||selected.clone())}
        } else {selected.clone()};
        if expected.matches(&selected)!=Some(true) {
            let preview=self.layer_masks.previews.get(&(target,coordinate)).filter(|preview|preview.matches(&selected))
                .ok_or(GpuRasterError::NotReady)?;
            return preview.page.read(expected,receipt).map(Some);
        }
        let read=self.layer_masks.pages.get(&(target,coordinate)).and_then(|page|page.read(&selected,receipt).ok());
        if read.is_none() && selected.matches(&command_mask(source,None,coordinate)?)!=Some(true) {return Err(GpuRasterError::NotReady);}
        Ok(read)
    }
    pub(crate) fn reconcile_mask_previews(&mut self) {
        let runtime=&self.raster;
        self.layer_masks.previews.retain(|(target,coordinate),preview|runtime.as_ref().and_then(|runtime|runtime.targets.get(target))
            .is_some_and(|target|preview.matches(&target.contents.version(TileKey {plane:RasterPlane::Mask,coordinate:*coordinate}))));
    }
    pub(crate) fn validate_raster_intent(&self,target:SourceTarget,intent:&Intent)->Result<(),GpuRasterError> {
        if self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target)).is_some_and(|target|target.contents.accepts(intent)) {Ok(())}
        else {crate::resident::write_refused();Err(GpuRasterError::Invariant("accepted write selection changed"))}
    }
    pub(crate) fn raster_generation(&self,target:SourceTarget)->Option<[u64;3]> {
        let runtime=self.raster.as_ref()?;let current=runtime.targets.get(&target)?.contents.generation();
        Some([runtime.generation+runtime.targets.values().map(|target|target.contents.generation()[0]).sum::<u64>(),current[0],current[1]])
    }
    pub(crate) fn record_raster_write(&mut self,target:SourceTarget,written:crate::resident::Written)->Result<(),GpuRasterError> {
        if self.raster.as_mut().and_then(|runtime|runtime.targets.get_mut(&target)).is_some_and(|target|target.contents.written(written)) {Ok(())}
        else {Err(GpuRasterError::Invariant("authored publication does not match its target"))}
    }
}

pub(crate) fn declared_versions(scene:SceneView<'_>,target:SourceTarget)->Result<Versions,GpuRasterError> {
    let data=scene.raster(target).and_then(|revision|revision.try_data()).ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?;
    let mut contents=Contents::new(data);contents.configure(scene,target);Ok(contents.versions())
}

pub(crate) fn declared_coverage_versions(source:&layer_core::authored::CoverageSource)->Result<Versions,GpuRasterError> {
    let data=source.raster.try_data().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?;
    let mut versions=Contents::new(data).versions();versions.defaults=defaults_for(source.domain,None,Some(source.default_coverage));Ok(versions)
}

impl Contents {
    pub fn new(selected: Arc<RasterData>) -> Self {
        Self { selected, fork: None, selection: Arc::new(()), defaults: std::array::from_fn(|plane| ContentVersion::derive(plane as u128, &[])), generation: [0; 2] }
    }
    pub fn data(&self) -> &Arc<RasterData> { &self.selected }
    pub fn generation(&self) -> [u64; 2] { self.generation }
    pub fn select(&mut self, selected: Arc<RasterData>) { self.published(selected); self.selection = Arc::new(()); }
    pub fn published(&mut self, selected: Arc<RasterData>) { self.selected = selected; self.fork = None; self.generation[0] += 1; }
    pub fn versions(&self) -> Versions { Versions { pages: self.pages(), defaults: self.defaults.clone() } }

    pub fn configure(&mut self, scene: SceneView<'_>, target: SourceTarget) {
        let defaults=defaults(scene,target);
        if self.defaults.iter().zip(&defaults).any(|(a,b)| a.identity() != b.identity()) {
            self.defaults = defaults;
            self.selection = Arc::new(());
            self.generation[0] += 1;
        }
    }

    pub fn retain(&mut self, keep: impl Fn([u32; 2]) -> bool) {
        self.generation[0] += 1;
        if self.selected.tiles.keys().any(|key| !keep(key.coordinate)) {
            Arc::make_mut(&mut self.selected).tiles.retain(|key, _| keep(key.coordinate));
        }
        if let Some(fork) = &mut self.fork {
            for tree in fork.pages.iter_mut().chain(&mut fork.before) {
                let mut removed = Vec::new();
                tree.changes(&ContentTree::default(), |coordinate| { if !keep(coordinate) { removed.push(coordinate); } });
                for coordinate in removed { tree.set(coordinate, None); }
            }
        }
    }

    fn pages(&self) -> [ContentTree; 3] {
        match &self.fork {
            Some(fork) if fork.submission.discarded() => fork.before.clone(),
            Some(fork) => fork.pages.clone(),
            None => planes().map(|plane| self.selected.tiles.content(plane).clone()),
        }
    }

    pub fn version(&self, key: TileKey) -> ContentVersion {
        match &self.fork {
            Some(fork) => if fork.submission.discarded() { &fork.before[index(key.plane)] } else { &fork.pages[index(key.plane)] },
            None => self.selected.tiles.content(key.plane),
        }.tile_version(key.coordinate).unwrap_or_else(|| {
            fallback(&self.defaults[index(key.plane)],key.coordinate)
        })
    }

    pub fn modified(&self, coordinate:[u32;2])->bool {
        planes().into_iter().any(|plane| {
            let key=TileKey {plane,coordinate};
            self.version(key).matches(&self.selected_version(key))!=Some(true)
        })
    }

    pub fn selected_version(&self,key:TileKey)->ContentVersion {
        self.selected.tiles.get(&key).map_or_else(||fallback(&self.defaults[index(key.plane)],key.coordinate),RasterTile::content_version)
    }

    pub fn modified_coordinates(&self)->std::collections::BTreeSet<[u32;2]> {
        let pages=self.pages();let mut coordinates=std::collections::BTreeSet::new();
        for plane in planes() {
            self.selected.tiles.content(plane).changes(&pages[index(plane)],|coordinate| {coordinates.insert(coordinate);});
        }
        coordinates.retain(|coordinate|self.modified(*coordinate));coordinates
    }

    pub fn restorable_data(&self)->RasterData {
        let mut data=(*self.selected).clone();
        for coordinate in self.modified_coordinates() {
            for plane in planes() {data.tiles.remove(&TileKey {plane,coordinate});}
        }
        data
    }

    pub fn selection_regions(&self,after:&RasterData,extent:[u32;2])->Vec<PixelRect> {
        let mut coordinates=std::collections::BTreeSet::new();let pages=self.pages();
        for plane in planes() {pages[index(plane)].changes(after.tiles.content(plane),|coordinate| {coordinates.insert(coordinate);});}
        if self.selected.watercolor!=after.watercolor {
            coordinates.extend(self.selected.tiles.keys().chain(after.tiles.keys()).map(|key|key.coordinate));
        }
        let radius=if self.selected.watercolor.is_some() || after.watercolor.is_some() {PAGE_SIZE}else {0};
        coordinates.into_iter().flat_map(|coordinate|page_coordinates(page_rect(coordinate).intersect(PixelRect::full(extent)).expand(radius,extent)))
            .collect::<std::collections::BTreeSet<_>>().into_iter().map(|coordinate|page_rect(coordinate).intersect(PixelRect::full(extent))).collect()
    }

    pub fn intent(&self, key: TileKey) -> Intent {
        Intent { key, version: self.version(key), selection: self.selection.clone() }
    }

    pub fn accepts(&self,intent:&Intent)->bool {
        Arc::ptr_eq(&self.selection,&intent.selection) && intent.version.matches(&self.version(intent.key))==Some(true)
    }

    pub fn written(&mut self, written: crate::resident::Written) -> bool {
        let Some(intent) = written.intent() else { return false; };
        if !self.accepts(intent) || written.submission().discarded() {return false;}
        let submission = written.submission();
        if !self.fork.as_ref().is_some_and(|fork| fork.submission.same(submission)) {
            let pages = self.pages();
            self.fork = Some(Fork { before: pages.clone(), pages, submission: submission.clone() });
        }
        self.fork.as_mut().unwrap().pages[index(intent.key.plane)].set_version(intent.key.coordinate, written.version().clone());
        self.generation = self.generation.map(|count| count + 1);
        true
    }
}
