//! Raw artwork access shared by queries and pixel operations. Painted tiles
//! override an immutable original; absent pages are not always transparent.
use super::*;
use layer_core::authored::PaintBase;

pub(super) fn placed_targets(scene: SceneView<'_>) -> impl Iterator<Item=SourceTarget> + '_ {
    scene.targets().filter(move |target|scene.source_owner(*target).is_some())
}

pub(super) struct RawTile {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

pub(super) struct PaintBaseRead {
    pub tile: RawTile,
    pub pending: Vec<scene::sources::PendingTile>,
    pub assembly: Option<PaintBaseAssembly>,
}

pub(super) struct PaintBaseCopy {
    source: RawTile,
    source_origin: [u32; 2],
    origin: [u32; 2],
    extent: [u32; 2],
}

pub(super) struct PaintBaseAssembly {
    tile: RawTile,
    copies: Vec<PaintBaseCopy>,
    write: crate::submission::CacheWrite,
}

pub(super) fn paint_base_bounds(base: &PaintBase) -> PixelRect {
    PixelRect::new(base.offset[0], base.offset[1], base.offset[0] + base.image.extent[0], base.offset[1] + base.image.extent[1])
}

pub(super) fn paint_base_contains(base: &PaintBase, coordinate: [u32; 2]) -> bool {
    !paint_base_bounds(base).intersect(page_rect(coordinate)).is_empty()
}

pub(super) fn plan_paint_base(r: &WgpuRasterizer, base: &PaintBase, coordinate: [u32; 2]) -> Result<Option<PaintBaseRead>, GpuRasterError> {
    let bounds = paint_base_bounds(base).intersect(page_rect(coordinate));
    if bounds.is_empty() { return Ok(None); }
    let mut cache = r.source_tiles.borrow_mut();
    if base.offset.iter().all(|v| v % PAGE_SIZE == 0) {
        let source_coordinate = std::array::from_fn(|i| coordinate[i] - base.offset[i] / PAGE_SIZE);
        let (tile, pending) = cache.plan(r, base.image.storage(), source_coordinate)?;
        return Ok(Some(PaintBaseRead { tile, pending: pending.into_iter().collect(), assembly: None }));
    }
    let (tile, write) = cache.plan_base(r, base, coordinate)?;
    let Some(write) = write else { return Ok(Some(PaintBaseRead {tile,pending:Vec::new(),assembly:None})); };
    let mut leases = vec![cache.lease(&tile.view).unwrap()];
    let source_bounds = PixelRect::new(bounds.min_x() - base.offset[0], bounds.min_y() - base.offset[1], bounds.max_x() - base.offset[0], bounds.max_y() - base.offset[1]);
    let mut pending = Vec::with_capacity(4);
    let mut copies = Vec::with_capacity(4);
    for source_coordinate in page_coordinates(source_bounds) {
        let overlap = source_bounds.intersect(page_rect(source_coordinate));
        let (source, decode) = cache.plan(r, base.image.storage(), source_coordinate)?;
        leases.push(cache.lease(&source.view).unwrap());
        pending.extend(decode);
        copies.push(PaintBaseCopy {
            source, source_origin: [overlap.min_x() % PAGE_SIZE, overlap.min_y() % PAGE_SIZE],
            origin: [overlap.min_x() + base.offset[0] - coordinate[0] * PAGE_SIZE, overlap.min_y() + base.offset[1] - coordinate[1] * PAGE_SIZE],
            extent: [overlap.width(),overlap.height()],
        });
    }
    let assembly = PaintBaseAssembly {tile:RawTile {texture:tile.texture.clone(),view:tile.view.clone()},copies,write};
    Ok(Some(PaintBaseRead {tile,pending,assembly:Some(assembly)}))
}

pub(super) fn encode_paint_base(assembly: &PaintBaseAssembly, encoder: &mut crate::submission::CommandEncoder) {
    { let _pass = encoder.color_pass("paint base window", &assembly.tile.view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)); }
    for copy in &assembly.copies {
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:copy.source_origin[0],y:copy.source_origin[1],z:0},..copy.source.texture.as_image_copy()},
            wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:copy.origin[0],y:copy.origin[1],z:0},..assembly.tile.texture.as_image_copy()},
            wgpu::Extent3d {width:copy.extent[0],height:copy.extent[1],depth_or_array_layers:1},
        );
    }
    assembly.write.track(encoder);
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

    /// Prepare at most one job's neighborhood before borrowing its views.
    /// Ordinary paint has no source preparation or resource-handle cloning.
    pub(super) fn prepare_raw_neighborhood<const N: usize>(
        &mut self,
        layer: SourceTarget,
        coordinate: [u32; 2],
        offsets: [Option<[i32; 2]>; N],
        preview: bool,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        assert!(N <= SOURCE_SLOTS);
        if !self.tiled_sources.contains_key(&layer) && self.native_backing(layer).is_none() {
            return Ok(());
        }
        let mut leases = Vec::with_capacity(N);
        for [dx, dy] in offsets.into_iter().flatten() {
            let [x, y] = [coordinate[0] as i32 + dx, coordinate[1] as i32 + dy];
            if x < 0 || y < 0 {
                continue;
            }
            let neighbor = [x as u32, y as u32];
            if preview && self.preview_page(neighbor).is_some()
            {
                continue;
            }
            if let Some(tile) = self.raw_layer_tile(layer, neighbor, encoder)? {
                leases.extend(self.source_tiles.borrow().lease(&tile.view));
            }
        }
        Ok(())
    }

    /// Consume the resulting binding before preparing another neighborhood;
    /// no cache eviction may intervene. All returned resources are borrowed.
    pub(super) fn raw_layer_neighborhood<'a, const N: usize>(
        &'a self,
        sources: &'a scene::sources::DecodedTiles,
        layer: &'a PaintLayer,
        coordinate: [u32; 2],
        offsets: [Option<[i32; 2]>; N],
        preview: bool,
    ) -> [&'a wgpu::TextureView; N] {
        offsets.map(|offset| {
            let Some([dx, dy]) = offset else { return &self.empty_view; };
            let [x, y] = [coordinate[0] as i32 + dx, coordinate[1] as i32 + dy];
            if x < 0 || y < 0 {
                return &self.empty_view;
            }
            let neighbor = [x as u32, y as u32];
            let predicted = if preview {
                self.preview_page(neighbor)
            } else {
                None
            };
            if let Some(page) =
                predicted.or_else(|| layer.pages.iter().find(|p| p.coordinate == neighbor))
            {
                return &page.active().view;
            }
            if let Ok(Some(blob)) = self.native_color_tile(layer.id, neighbor)
                && let Some(view) = sources.prepared_raster_view(&blob, self.document_color().space)
            {
                return view;
            }
            if let Some(source) = self.tiled_sources.get(&layer.id)
                && let Some(view) = sources.prepared_base_view(source, neighbor)
            {
                return view;
            }
            &self.empty_view
        })
    }

    /// Consume this texture in queue order before the source cache can evict it.
    /// It can be encoded sRGB8 paint or linear Float32 original-source data.
    pub(super) fn raw_layer_tile(
        &mut self,
        layer: SourceTarget,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<RawTile>, GpuRasterError> {
        if let Some(page) = self
            .paint_layers
            .iter()
            .find(|l| l.id == layer)
            .and_then(|l| l.pages.iter().find(|p| p.coordinate == coordinate))
        {
            return Ok(Some(RawTile {
                texture: page.active().texture.clone(),
                view: page.active().view.clone(),
            }));
        }
        if let Some(blob) = self.native_color_tile(layer, coordinate)? {
            return self
                .backed_raster_tile(&blob, self.document_color().space, encoder)
                .map(Some);
        }
        let Some(source) = self.tiled_sources.get(&layer).cloned() else {
            return Ok(None);
        };
        self.paint_base_tile(&source, coordinate, encoder)
    }

    pub(super) fn paint_base_tile(&mut self, base: &PaintBase, coordinate: [u32; 2], encoder: &mut crate::submission::CommandEncoder) -> Result<Option<RawTile>, GpuRasterError> {
        let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
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
        let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
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

    #[test]
    fn paint_base_four_tile_window_and_aligned_lookup_preserve_edges_and_codes() {
        let source = rgba8_source([300,270],codes);
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let base = PaintBase {image:source.clone().into(),offset:[17,31],policy:Default::default()};
        let mut encoder = crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan = plan_paint_base(&r,&base,[1,1]).unwrap().unwrap();
        assert_eq!(plan.pending.len(),4);
        drop(plan);
        let tile = r.paint_base_tile(&base,[1,1],&mut encoder).unwrap().unwrap();
        submit(&mut r,encoder);
        for (i,actual) in crate::test_support::float_pixels(&r,&tile.texture).into_iter().enumerate() {
            let [x,y]=[i as u32%PAGE_SIZE+239,i as u32/PAGE_SIZE+225];
            let expected=if x<300 && y<270 {expected(x,y)} else {[0.;4]};
            assert!(actual.into_iter().zip(expected).all(|(a,b)|(a-b).abs()<2e-6),"{x},{y}: {actual:?} != {expected:?}");
        }
        let aligned=PaintBase {offset:[PAGE_SIZE;2],..base};
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let direct=r.paint_base_tile(&PaintBase::new(source.clone().into()),[0,0],&mut encoder).unwrap().unwrap();
        let shifted=r.paint_base_tile(&aligned,[1,1],&mut encoder).unwrap().unwrap();
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
    fn thumbnail(r:&mut WgpuRasterizer,target:SourceTarget)->Vec<u8>{r.request_thumbnail(1,ThumbnailTarget::Source(target)).unwrap();crate::test_support::complete(r);r.take_thumbnail().unwrap().unwrap().bytes}
    fn sample(r:&mut WgpuRasterizer,target:SourceTarget,position:[u32;2])->[f32;4]{
        assert!(r.request_color_sample(ColorSampleRequest {request_id:1,source:ColorSampleSource::Source(target),position,area:ColorSampleArea::Average5}).unwrap());
        crate::test_support::complete(r);r.take_color_sample().unwrap().unwrap().rgba
    }
    #[test]
    fn paint_base_offset_display_exact_reads_regions_thumbnails_and_material_match_authored_pixels() {
        let (mut offset,target)=document(true);let (mut baked,baked_target)=document(false);let extent=offset.composition().size;
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let mut reference=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.submit(crate::test_support::packet(offset.scene(),extent)).unwrap();reference.submit(crate::test_support::packet(baked.scene(),extent)).unwrap();
        let pixels=|r:&WgpuRasterizer|crate::test_support::float_pixels(r,crate::test_support::document_texture(r));
        assert_eq!(pixels(&r),pixels(&reference));
        assert_eq!(thumbnail(&mut r,target),thumbnail(&mut reference,baked_target));
        for at in [[17,31],[255,255],[256,256],[316,250],[317,250]] {assert_eq!(sample(&mut r,target,at),sample(&mut reference,baked_target,at));}
        let query=|r:&mut WgpuRasterizer,target|crate::test_support::receive_request(r,RegionRequest {request_id:1,source:RegionSource::Source(target),position:[260,260],tolerance:0.,contiguous:true,selection:None,refinement:Default::default(),limit:None,enclosure:None}).pixels;
        assert_eq!(query(&mut r,target),query(&mut reference,baked_target));
        let mut capture=r.snapshot_gpu().capture_scene(offset.snapshot(),SceneScope::Raw(target),Default::default()).unwrap();
        assert!(capture.read_region([256,256,61,45]).unwrap().iter().all(|p|*p==[0.;4]),"transparent override replaces the base tile");
        let dabs=[crate::tests::test_dab([250.,240.],[0.13,0.72,0.41,0.37],0.5)];
        for (document,r,target) in [(&mut offset,&mut r,target),(&mut baked,&mut reference,baked_target)] {
            let SourceTarget::Paint(handle)=target else {unreachable!()};document.artwork.paint.get_mut(handle).unwrap().raster=RasterRevision::pending();
            let batches=[crate::test_support::dab_batch(target,crate::tests::test_style(BrushExecution::Dry),Rect {min:Point {x:230.,y:220.},max:Point {x:270.,y:260.}})];
            r.submit(FramePacket {dabs:&dabs,dab_batches:&batches,composite_all:true,..crate::test_support::packet(document.scene(),extent)}).unwrap();
        }
        assert_eq!(pixels(&r),pixels(&reference));
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
        let mip_bytes=r.source_tiles.borrow().mip_budget();assert!(r.source_tiles.borrow_mut().reserve_mips(mip_bytes,&encoder).unwrap());
        let offsets=std::array::from_fn::<_,9,_>(|i|Some([3*(i as i32%3),3*(i as i32/3)]));
        r.prepare_raw_neighborhood(target,[1,1],offsets,false,&mut encoder).unwrap();
        let sources=r.source_tiles.borrow();let layer=r.paint_layers.iter().find(|l|l.id==target).unwrap();
        let views=r.raw_layer_neighborhood(&sources,layer,[1,1],offsets,false);
        assert!(views.iter().all(|view|**view!=r.empty_view));
        let textures=views.map(|view|view.texture().clone());drop(sources);submit(&mut r,encoder);
        for (i,texture) in textures.into_iter().enumerate() {
            let pixels=crate::test_support::float_pixels(&r,&texture);
            let coordinate=[1+3*(i as u32%3),1+3*(i as u32/3)];
            let expected=expected(coordinate[0]*PAGE_SIZE+128-17,coordinate[1]*PAGE_SIZE+128-31);
            assert!(pixels[128*256+128].into_iter().zip(expected).all(|(a,b)|(a-b).abs()<2e-6));
        }
    }

}
