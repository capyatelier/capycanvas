//! Raw artwork access shared by queries and pixel operations. Painted tiles
//! override an immutable original; absent pages are not always transparent.
use super::*;
use layer_core::authored::PaintBase;

pub(super) fn placed_targets(scene: SceneView<'_>) -> impl Iterator<Item=SourceTarget> + '_ {
    scene.targets().filter(move |target|scene.source_owner(*target).is_some_and(|owner|scene.includes(owner)))
}

pub(super) fn pending_bakes<'a>(scene: SceneView<'a>) -> impl Iterator<Item=(SceneView<'a>, &'a layer_core::SceneScope, Option<SourceTarget>)> + 'a {
    placed_targets(scene).filter_map(move |target| scene.operations(target)).flatten().filter_map(|operation| match &operation.kind {
        layer_core::RasterOperationKind::Bake { scene, scope, .. } => Some((scene.view(), scope, None)),
        layer_core::RasterOperationKind::FrequencyDetail { scene, scope, low, .. } => Some((scene.view(), scope, Some(SourceTarget::Paint(*low)))),
        _ => None,
    })
}

pub(super) fn retained_targets(scene: SceneView<'_>) -> std::collections::BTreeMap<SourceTarget, SceneView<'_>> {
    let mut targets: std::collections::BTreeMap<_, _> = placed_targets(scene).map(|target| (target, scene)).collect();
    for (snapshot, scope, low) in pending_bakes(scene) {
        for target in crate::snapshot::capture_targets(snapshot.with_scope(scope), scope).into_iter().chain(low) {
            targets.entry(target).or_insert(snapshot);
        }
    }
    targets
}

pub(super) fn neighborhood<const N:usize>(coordinate:[u32;2],offsets:[Option<[i32;2]>;N])->impl Iterator<Item=[u32;2]> {
    offsets.into_iter().flatten().filter_map(move|[dx,dy]| {
        let [x,y]=[i64::from(coordinate[0])+i64::from(dx),i64::from(coordinate[1])+i64::from(dy)];
        Some([u32::try_from(x).ok()?,u32::try_from(y).ok()?])
    })
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct TileTexture { texture: wgpu::Texture, layer: u32 }
impl From<wgpu::Texture> for TileTexture { fn from(texture: wgpu::Texture) -> Self { Self { texture, layer: 0 } } }
impl From<&wgpu::Texture> for TileTexture { fn from(texture: &wgpu::Texture) -> Self { texture.clone().into() } }
impl From<&TileTexture> for TileTexture { fn from(texture: &TileTexture) -> Self { texture.clone() } }
impl TileTexture {
    pub fn layer_of(texture: wgpu::Texture, layer: u32) -> Self { Self { texture, layer } }
    pub fn raw(&self) -> &wgpu::Texture { &self.texture }
    pub fn format(&self) -> wgpu::TextureFormat { self.texture.format() }
    pub fn width(&self) -> u32 { self.texture.width() }
    pub fn height(&self) -> u32 { self.texture.height() }
    pub fn size(&self) -> wgpu::Extent3d { wgpu::Extent3d { depth_or_array_layers: 1, ..self.texture.size() } }
    pub fn as_image_copy(&self) -> wgpu::TexelCopyTextureInfo<'_> { self.copy_at([0; 2]) }
    pub fn copy_at(&self, [x, y]: [u32; 2]) -> wgpu::TexelCopyTextureInfo<'_> {
        wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x, y, z: self.layer }, ..self.texture.as_image_copy() }
    }
    pub fn create_view(&self, descriptor: &wgpu::TextureViewDescriptor<'_>) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: self.layer, array_layer_count: Some(1), ..descriptor.clone() })
    }
}
#[derive(Clone)]
pub(super) struct RawTile {
    pub texture: TileTexture,
    pub view: wgpu::TextureView,
}
impl resident::Storage for RawTile {fn storage_bytes(&self)->u64 {texture_bytes(&self.texture.texture)/u64::from(self.texture.texture.depth_or_array_layers())}}
impl resident::TextureStorage for RawTile {
    fn texture(&self)->&wgpu::Texture {&self.texture.texture}
    fn view(&self)->&wgpu::TextureView {&self.view}
}

pub(super) struct SourceRead {
    pub tile: RawTile,
    pub pending: Vec<Arc<scene::sources::PendingTile>>,
    pub assembly: Option<Arc<scene::sources::PaintBaseAssembly>>,
}
impl SourceRead {
    pub fn ready(&self)->bool {self.pending.iter().all(|pending|pending.ready())}
    pub fn encode(&self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,class:crate::residency::Class)->Result<bool,GpuRasterError> {
        if !self.ready() && (!r.snapshot_worker || cfg!(target_arch="wasm32")) {return Ok(false);}
        for pending in &self.pending {if !scene::sources::encode_pending(r,encoder,pending.clone(),class)? {return Ok(false);}}
        if let Some(assembly)=&self.assembly {assembly.encode(r,encoder);}
        Ok(true)
    }
}

pub(super) fn paint_base_bounds(base: &PaintBase) -> PixelRect {
    PixelRect::new(base.offset[0], base.offset[1], base.offset[0] + base.image.extent[0], base.offset[1] + base.image.extent[1])
}

pub(super) fn paint_base_contains(base: &PaintBase, coordinate: [u32; 2]) -> bool {
    !paint_base_bounds(base).intersect(page_rect(coordinate)).is_empty()
}
pub(super) fn paint_base_source_bounds(base:&PaintBase,coordinate:[u32;2])->PixelRect {
    let bounds=paint_base_bounds(base).intersect(page_rect(coordinate));
    if bounds.is_empty() {PixelRect::EMPTY}else {PixelRect::new(bounds.min_x()-base.offset[0],bounds.min_y()-base.offset[1],bounds.max_x()-base.offset[0],bounds.max_y()-base.offset[1])}
}

pub(super) fn prepare_paint_base(r:&mut WgpuRasterizer,base:&PaintBase,coordinate:[u32;2])->Result<bool,GpuRasterError> {
    let mut ready=true;
    for coordinate in page_coordinates(paint_base_source_bounds(base,coordinate)) {
        if r.source_tiles.borrow().prepared_view(base.image.storage(),coordinate).is_none() {
            ready=false;if !r.request_exact_image_decode(base.image.id(),base.image.storage().clone(),coordinate,crate::residency::Class::Visible)? {break;}
        }
    }
    Ok(ready)
}


impl WgpuRasterizer {
    pub(super) fn source_cache_work(&self) -> [u64; 2] {
        let sources = self.source_tiles.borrow();
        [sources.hits, sources.misses]
    }

    /// Pooled prediction surfaces can outlive their current footprint. Every
    /// reader must use membership as well as coordinates to avoid stale paint.
    pub(super) fn preview_page(&self, coordinate: [u32; 2]) -> Option<&LayerPage> {
        if self.preview_damage.intersect(page_rect(coordinate)).is_empty()
            || self.preview_contact_tiles.as_ref().is_some_and(|set| !set.contains(&coordinate)) {
            return None;
        }
        self.preview_pages.iter().find(|p| p.coordinate == coordinate)
    }

    pub(super) fn preview_wetness_page(&self, target: SourceTarget, coordinate: [u32; 2]) -> Option<&WatercolorWetnessPage> {
        if self.preview_layer_id != Some(target) || self.preview_damage.intersect(page_rect(coordinate)).is_empty() {return None;}
        self.preview_watercolor_wetness_pages.iter().find(|page| page.coordinate == coordinate)
    }

    pub(super) fn preview_input_tiles(&self, target: SourceTarget) -> std::collections::BTreeSet<[u32; 2]> {
        if self.preview_layer_id != Some(target) {return Default::default();}
        self.preview_pages.iter().map(|page| page.coordinate).filter(|c| self.preview_page(*c).is_some())
            .chain(self.preview_watercolor_wetness_pages.iter().map(|page| page.coordinate).filter(|c| self.preview_wetness_page(target, *c).is_some()))
            .collect()
    }

    pub(super) fn accept_raw_neighborhood<const N:usize>(
        &mut self,layer:SourceTarget,coordinate:[u32;2],offsets:[Option<[i32;2]>;N],
        preview:Option<(scene::PreviewStage,Option<(scene::PreviewPass,u32)>)>,expected:Option<&raster::contents::Versions>,
        encoder:&mut crate::submission::CommandEncoder,
    )->Result<scene::sources::AcceptedInputs,GpuRasterError> {
        assert!(N<=SOURCE_SLOTS);
        let declaration=scene::sources::Declaration::raw(preview,expected.cloned(),Some(encoder.submission()));
        let accepted=scene::sources::declare(Default::default(),declaration,|declaring|
            self.raw_layer_neighborhood(declaring,layer,coordinate,offsets,None,encoder).map(drop))?.accept_query(self,None)?;
        if !accepted.prepare(self)? {return Err(GpuRasterError::NotReady);}
        accepted.clone().track(encoder);
        Ok(accepted)
    }

    #[cfg(test)]
    pub(super) fn read_raw_neighborhood<const N:usize>(&mut self,layer:SourceTarget,coordinate:[u32;2],offsets:[Option<[i32;2]>;N],
        encoder:&mut crate::submission::CommandEncoder,
    )->Result<(scene::sources::AcceptedInputs,[Option<wgpu::TextureView>;N]),GpuRasterError> {
        let mut accepted=self.accept_raw_neighborhood(layer,coordinate,offsets,None,None,encoder)?;
        let views=self.raw_layer_neighborhood(&mut accepted,layer,coordinate,offsets,None,encoder)?;
        Ok((accepted,views))
    }

    pub(super) fn raw_layer_neighborhood<const N:usize>(
        &mut self,inputs:&mut scene::sources::AcceptedInputs,layer:SourceTarget,coordinate:[u32;2],offsets:[Option<[i32;2]>;N],
        captured:Option<&mut Vec<resident::Input>>,encoder:&mut crate::submission::CommandEncoder,
    )->Result<[Option<wgpu::TextureView>;N],GpuRasterError> {
        self.layer_neighborhood(inputs,layer,coordinate,offsets,captured,encoder,|inputs,r,layer,at,_|inputs.raw_color(r,layer,at))
    }

    pub(super) fn brush_layer_neighborhood<const N:usize>(
        &mut self,inputs:&mut scene::sources::AcceptedInputs,layer:SourceTarget,coordinate:[u32;2],offsets:[Option<[i32;2]>;N],
        preview:Option<(scene::PreviewStage,Option<(scene::PreviewPass,u32)>)>,captured:Option<&mut Vec<resident::Input>>,encoder:&mut crate::submission::CommandEncoder,
    )->Result<[Option<wgpu::TextureView>;N],GpuRasterError> {
        let read=std::cell::RefCell::new(Vec::new());
        let views=self.layer_neighborhood(inputs,layer,coordinate,offsets,captured,encoder,|inputs,r,layer,at,receipt| {
            if let Some(tile)=r.material_reads.as_ref().and_then(|reads|reads.get(&(layer,at,preview))) {
                return Ok([tile.clone().map(|tile|SourceRead {tile,pending:Vec::new(),assembly:None}),None]);
            }
            #[cfg(test)] crate::test_support::record_material_read((at,preview.map(|(stage,_)|stage)));
            let choices=inputs.brush_color(r,layer,at,preview,receipt)?;
            if r.material_reads.is_some() && let [base,None]=&choices && base.as_ref().is_none_or(|base|base.pending.is_empty() && base.assembly.is_none()) {
                read.borrow_mut().push(((layer,at,preview),base.as_ref().map(|base|base.tile.clone())));
            }
            Ok(choices)
        })?;
        if let Some(reads)=self.material_reads.as_mut() {reads.extend(read.into_inner());}
        Ok(views)
    }

    pub(super) fn brush_layer_tiles(
        &mut self,inputs:&mut scene::sources::AcceptedInputs,layer:SourceTarget,tiles:&[[u32;2]],
        preview:Option<(scene::PreviewStage,Option<(scene::PreviewPass,u32)>)>,encoder:&mut crate::submission::CommandEncoder,
    )->Result<Vec<Option<wgpu::TextureView>>,GpuRasterError> {
        let mut views=vec![None;tiles.len()];
        self.layer_tiles(inputs,layer,tiles.iter().copied().enumerate(),&mut views,None,encoder,|inputs,r,layer,at,receipt|inputs.brush_color(r,layer,at,preview,receipt))?;
        Ok(views)
    }

    fn layer_neighborhood<const N:usize>(
        &mut self,inputs:&mut scene::sources::AcceptedInputs,layer:SourceTarget,coordinate:[u32;2],offsets:[Option<[i32;2]>;N],
        captured:Option<&mut Vec<resident::Input>>,encoder:&mut crate::submission::CommandEncoder,
        read:impl Fn(&mut scene::sources::AcceptedInputs,&Self,SourceTarget,[u32;2],&submission::Submission)->Result<[Option<SourceRead>;2],GpuRasterError>,
    )->Result<[Option<wgpu::TextureView>;N],GpuRasterError> {
        let mut views=std::array::from_fn(|_|None);
        let tiles=offsets.into_iter().enumerate().filter_map(|(index,offset)|Some((index,neighborhood(coordinate,[offset]).next()?)));
        self.layer_tiles(inputs,layer,tiles,&mut views,captured,encoder,read)?;
        Ok(views)
    }

    fn layer_tiles(
        &mut self,inputs:&mut scene::sources::AcceptedInputs,layer:SourceTarget,tiles:impl Iterator<Item=(usize,[u32;2])>,views:&mut [Option<wgpu::TextureView>],
        mut captured:Option<&mut Vec<resident::Input>>,encoder:&mut crate::submission::CommandEncoder,
        read:impl Fn(&mut scene::sources::AcceptedInputs,&Self,SourceTarget,[u32;2],&submission::Submission)->Result<[Option<SourceRead>;2],GpuRasterError>,
    )->Result<(),GpuRasterError> {
        let receipt=encoder.submission();
        #[cfg(test)]
        let _record=scene::sources::record_reads(inputs);
        for (index,at) in tiles {
            let [base,overlay]=read(inputs,self,layer,at,&receipt)?;
            if let Some(captured)=captured.as_mut() {captured.extend(inputs.witnesses(layer,RasterPlane::Color,at));}
            debug_assert!(overlay.is_none(),"raw neighborhood has a composite preview");
            if let Some(read)=&base && !read.encode(self,encoder,crate::residency::Class::Authored)? {return Err(GpuRasterError::NotReady);}
            views[index]=base.map(|read|read.tile.view);
        }
        if !inputs.declaring() {self.source_tiles.borrow_mut().flush_retired(encoder);}
        Ok(())
    }

    pub(super) fn raw_layer_tile(
        &mut self,
        layer: SourceTarget,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<RawTile>, GpuRasterError> {
        let read=|r:&WgpuRasterizer,accepted:&mut scene::sources::AcceptedInputs|if layer.is_coverage() {accepted.raw_scalar(r,layer,RasterPlane::Mask,coordinate)}
            else {accepted.raw_color(r,layer,coordinate).map(|[base,_]|base)};
        let declaration=scene::sources::Declaration::raw(None,None,Some(encoder.submission()));
        let mut accepted=scene::sources::declare(Default::default(),declaration,|declaring|read(self,declaring).map(drop))?.accept_query(self,None)?;
        if !accepted.prepare(self)? {return Err(GpuRasterError::NotReady);}
        let read=read(self,&mut accepted)?;
        accepted.clone().track(encoder);
        if let Some(read)=&read && !read.encode(self,encoder,crate::residency::Class::Authored)? {return Err(GpuRasterError::NotReady);}
        accepted.track(encoder);Ok(read.map(|read|read.tile))
    }

    pub(super) fn paint_base_tile(&mut self, base: &PaintBase, coordinate: [u32; 2], encoder: &mut crate::submission::CommandEncoder) -> Result<Option<RawTile>, GpuRasterError> {
        let mut scene = self.scene.take().map(Ok).unwrap_or_else(|| scene::Scene::new(self))?;
        let result = scene.paint_base_tile_for_query(self, base, coordinate, encoder);
        self.scene = Some(scene);
        result
    }

    pub(super) fn backed_raster_tile(
        &mut self,
        blob: &std::sync::Arc<layer_core::raster::TileBlob>,
        space: layer_core::color::RgbSpace,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<RawTile, GpuRasterError> {
        let mut scene = self.scene.take().map(Ok).unwrap_or_else(|| scene::Scene::new(self))?;
        let result = scene.raster_tile_for_query(self, blob, space, encoder);
        self.scene = Some(scene);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{Document, Point, Rect, authored::*, color::{RgbSpace, source::rgba8_source}, raster::*};
    use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource, RegionRequest, RegionSource, ThumbnailTarget};

    fn codes(x: u32, y: u32) -> [u8; 4] { [(x % 251) as u8, (y % 241) as u8, ((x / 3 + y / 5) % 256) as u8, ((x + 3 * y) % 256) as u8] }
    fn expected(x: u32, y: u32) -> [f32; 4] {
        let p = codes(x,y); let alpha = f32::from(p[3])/255.;
        std::array::from_fn(|i| if i==3 {alpha} else {RgbSpace::Srgb.decode(f64::from(p[i])/255.) as f32*alpha})
    }
    fn submit(r: &mut WgpuRasterizer, encoder: crate::submission::CommandEncoder) { r.uploads.finish(&encoder); encoder.submit(&r.queue); }

    fn cold_source_query(raster: bool) {
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let base=PaintBase::new(rgba8_source([PAGE_SIZE;2],codes).into());
        let samples=(0..PAGE_SIZE).flat_map(|y|(0..PAGE_SIZE).flat_map(move|x|codes(x,y))).collect::<Vec<_>>();
        let blob=Arc::new(TileBlob::encode(r.document_color().paint_descriptor(),&samples).unwrap());
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let tile=crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder| {
            if raster {r.backed_raster_tile(&blob,RgbSpace::Srgb,encoder)}
            else {r.paint_base_tile(&base,[0;2],encoder).map(|tile|tile.unwrap())}
        });
        submit(&mut r,encoder);
        let pixels=crate::test_support::float_pixels(&r,&tile.texture);
        assert_eq!(pixels.len(),PAGE_SIZE.pow(2) as usize);
        for (index,pixel) in pixels.into_iter().enumerate() {
            let expected=expected(index as u32%PAGE_SIZE,index as u32/PAGE_SIZE);
            assert!(pixel.into_iter().zip(expected).all(|(actual,expected)|(actual-expected).abs()<2e-6),
                "cold raster={raster}, pixel={index}: {pixel:?} != {expected:?}");
        }
    }

    #[test]
    fn a_cold_paint_base_query_requests_and_reads_its_pixels() {cold_source_query(false);}

    #[test]
    fn a_cold_backed_raster_query_requests_and_reads_its_pixels() {cold_source_query(true);}

    #[test]
    fn paint_base_four_tile_window_and_aligned_lookup_preserve_edges_and_codes() {
        let source = rgba8_source([300,270],codes);
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let base = PaintBase {image:source.clone().into(),offset:[17,31],policy:Default::default()};
        let mut encoder = crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let mut requested=scene::sources::Inputs::default();
        requested.base(&base,[1,1],r.document_color().space);
        let inputs=requested.accept_query(&mut r,None).unwrap();
        let plan=scene::sources::accepted_base(&inputs,&base,[1,1]).unwrap().unwrap();
        assert_eq!(plan.pending.len(),4);
        drop((plan,inputs));
        let tile = crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|r.paint_base_tile(&base,[1,1],encoder)).unwrap();
        submit(&mut r,encoder);
        for (i,actual) in crate::test_support::float_pixels(&r,&tile.texture).into_iter().enumerate() {
            let [x,y]=[i as u32%PAGE_SIZE+239,i as u32/PAGE_SIZE+225];
            let expected=if x<300 && y<270 {expected(x,y)} else {[0.;4]};
            assert!(actual.into_iter().zip(expected).all(|(a,b)|(a-b).abs()<2e-6),"{x},{y}: {actual:?} != {expected:?}");
        }
        let aligned=PaintBase {offset:[PAGE_SIZE;2],..base};
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let direct=crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|r.paint_base_tile(&PaintBase::new(source.clone().into()),[0,0],encoder)).unwrap();
        let shifted=crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|r.paint_base_tile(&aligned,[1,1],encoder)).unwrap();
        assert_eq!(direct.view,shifted.view);
        assert!(r.paint_base_tile(&aligned,[0,0],&mut encoder).unwrap().is_none());
        submit(&mut r,encoder);
    }

    fn document(offset: bool) -> (Document,SourceTarget) {
        let extent=[600,520];let mut artwork=Artwork::new(extent).unwrap();
        let (_,target)=crate::test_support::add_paint(&mut artwork,"Photo",extent);
        let SourceTarget::Paint(handle)=target else {unreachable!()};
        let paint=artwork.paint.get_mut(handle).unwrap();
        paint.base=Some(if offset {PaintBase {image:rgba8_source([300,270],codes).into(),offset:[17,31],policy:Default::default()}}
            else {PaintBase::new(rgba8_source(extent,|x,y|if (17..317).contains(&x)&&(31..301).contains(&y){codes(x-17,y-31)}else{[0;4]}).into())});
        paint.raster=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[1,1]},RasterTile::backed(TileBlob::encode(layer_core::color::DocumentColor::default().paint_descriptor(),&vec![0;256*256*4]).unwrap()))].into(),..Default::default()});
        (Document::from_artwork(artwork).unwrap(),target)
    }
    fn thumbnail(r:&mut WgpuRasterizer,target:SourceTarget)->Vec<u8>{crate::test_support::thumbnail(r,1,ThumbnailTarget::Source(target)).bytes}
    fn sample(r:&mut WgpuRasterizer,target:SourceTarget,position:[u32;2])->[f32;4]{
        crate::test_support::color_sample(r, ColorSampleRequest {request_id:1,source:ColorSampleSource::Source(target),position,area:ColorSampleArea::Average5}).rgba
    }
    #[test]
    fn paint_base_offset_display_exact_reads_regions_thumbnails_and_material_match_authored_pixels() {
        let (mut offset,target)=document(true);let (mut baked,baked_target)=document(false);let extent=offset.composition().size;
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let mut reference=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        crate::test_support::submit_settled(&mut r,crate::test_support::packet(offset.scene(),extent));crate::test_support::submit_settled(&mut reference,crate::test_support::packet(baked.scene(),extent));
        let pixels=|r:&WgpuRasterizer|crate::test_support::float_pixels(r,crate::test_support::document_texture(r));
        assert_eq!(pixels(&r),pixels(&reference));
        assert_eq!(thumbnail(&mut r,target),thumbnail(&mut reference,baked_target));
        for at in [[17,31],[255,255],[256,256],[316,250],[317,250]] {assert_eq!(sample(&mut r,target,at),sample(&mut reference,baked_target,at));}
        let query=|r:&mut WgpuRasterizer,target|crate::test_support::receive_request(r,RegionRequest { fill_areas: None, colour_mode: Default::default(), fill: None, reference_count: None, paper: None,request_id:1,source:RegionSource::Source(target),position:[260,260],tolerance:0.,contiguous:true,selection:None,refinement:Default::default(),limit:None,enclosure:None}).pixels;
        assert_eq!(query(&mut r,target),query(&mut reference,baked_target));
        let mut capture=r.snapshot_gpu().capture_scene(offset.snapshot(),SceneScope::Raw(target),Default::default()).unwrap();
        assert!(capture.read_region([256,256,61,45]).unwrap().iter().all(|p|*p==[0.;4]),"transparent override replaces the base tile");
        let dabs=[crate::tests::test_dab([250.,240.],[0.13,0.72,0.41,0.37],0.5)];
        for (document,r,target) in [(&mut offset,&mut r,target),(&mut baked,&mut reference,baked_target)] {
            let SourceTarget::Paint(handle)=target else {unreachable!()};document.artwork.paint.get_mut(handle).unwrap().raster=RasterRevision::pending();
            let batches=[crate::test_support::dab_batch(target,crate::tests::test_style(BrushExecution::Dry),Rect {min:Point {x:230.,y:220.},max:Point {x:270.,y:260.}})];
            crate::test_support::submit_settled(r,FramePacket {dabs:&dabs,dab_batches:&batches,composite_all:true,..crate::test_support::packet(document.scene(),extent)});
        }
        assert_eq!(pixels(&r),pixels(&reference));
    }
    #[test]
    fn a_bound_raw_neighborhood_survives_source_retirement_before_submission() {
        let (document,target)=document(true);
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.ensure_document_metadata(document.composition().size,document.scene()).unwrap();
        let coordinate=[0,0];let offsets=[Some([0,0])];
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let (accepted,views)=crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|r.read_raw_neighborhood(target,coordinate,offsets,encoder));
        let source=views[0].as_ref().unwrap().texture().clone();
        assert_eq!([source.width(),source.height()],[PAGE_SIZE;2]);
        submit(&mut r,encoder);crate::test_support::complete(&r);
        crate::test_support::evict_sources(&mut r);
        let validation=r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let output=r.device.create_texture(&wgpu::TextureDescriptor {label:None,size:source.size(),mip_level_count:1,sample_count:1,
            dimension:wgpu::TextureDimension::D2,format:source.format(),usage:wgpu::TextureUsages::COPY_DST|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        encoder.copy_texture_to_texture(source.as_image_copy(),output.as_image_copy(),source.size());accepted.track(&mut encoder);submit(&mut r,encoder);
        let error=pollster::block_on(validation.pop());
        assert!(error.is_none(),"a bound neighborhood must keep its selected source alive: {error:?}");
        for (index,pixel) in crate::test_support::float_pixels(&r,&output).into_iter().enumerate() {
            let [x,y]=[index as u32%PAGE_SIZE,index as u32/PAGE_SIZE];
            let reference=if x>=17 && y>=31 {expected(x-17,y-31)}else {[0.;4]};
            assert!(pixel.into_iter().zip(reference).all(|(actual,expected)|(actual-expected).abs()<2e-6));
        }
    }
    #[test]
    fn arbitrary_base_neighborhood_keeps_outputs_when_source_slots_are_reserved_for_mips() {
        let extent=[2304;2];let mut artwork=Artwork::new(extent).unwrap();
        let (_,target)=crate::test_support::add_paint(&mut artwork,"Photo",extent);
        let SourceTarget::Paint(handle)=target else {unreachable!()};
        artwork.paint.get_mut(handle).unwrap().base=Some(PaintBase {image:rgba8_source([2048;2],codes).into(),offset:[17,31],policy:Default::default()});
        let document=Document::from_artwork(artwork).unwrap();
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.ensure_document_metadata(extent,document.scene()).unwrap();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let offsets=std::array::from_fn::<_,9,_>(|i|Some([3*(i as i32%3),3*(i as i32/3)]));
        let (accepted,views)=crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|r.read_raw_neighborhood(target,[1,1],offsets,encoder));
        let textures=views.map(|view|view.expect("every neighbor has a source").texture().clone());accepted.track(&mut encoder);submit(&mut r,encoder);
        for (i,texture) in textures.into_iter().enumerate() {
            let pixels=crate::test_support::float_pixels(&r,&texture);
            let coordinate=[1+3*(i as u32%3),1+3*(i as u32/3)];
            let expected=expected(coordinate[0]*PAGE_SIZE+128-17,coordinate[1]*PAGE_SIZE+128-31);
            assert!(pixels[128*256+128].into_iter().zip(expected).all(|(a,b)|(a-b).abs()<2e-6));
        }
    }

}
