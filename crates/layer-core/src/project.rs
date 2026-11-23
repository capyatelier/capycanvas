//! Shared brush resources and artwork admission limits.
use crate::*;
use serde::{Deserialize, Serialize};


#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectAssetFormat {
    R8Unorm,
}
impl ProjectAssetFormat {
    pub fn channels(self) -> u32 {
        match self {
            Self::R8Unorm => 1,
        }
    }
}

/// Packed brush-mask pixels, never a CPU canvas raster. Arc storage shares
/// immutable custom brush textures across the render-worker boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectAsset {
    pub extent: [u32; 2],
    pub format: ProjectAssetFormat,
    pub bytes: Arc<[u8]>,
}
impl ProjectAsset {
    /// Retain only tightly packed source rows, excluding host-buffer padding.
    /// This is an import operation, never a canvas raster or GPU readback.
    pub fn copy_rows(
        extent: [u32; 2],
        format: ProjectAssetFormat,
        stride: usize,
        bytes: &[u8],
    ) -> Result<Self, String> {
        let row = (extent[0] as usize)
            .checked_mul(format.channels() as usize)
            .ok_or("Project image size overflow")?;
        let size = stride
            .checked_mul(extent[1] as usize)
            .ok_or("Project image size overflow")?;
        if extent.contains(&0) || stride < row || bytes.len() < size {
            return Err("Incomplete project image".into());
        }
        let mut packed = Vec::new();
        packed
            .try_reserve_exact(row * extent[1] as usize)
            .map_err(|_| "Project image allocation failed")?;
        for source in bytes[..size].chunks_exact(stride) {
            packed.extend_from_slice(&source[..row]);
        }
        Ok(Self {
            extent,
            format,
            bytes: packed.into(),
        })
    }
}

/// Raster bounds cover decoded instances. Source bounds cover retained tiled
/// source/profile ownership; source decoding is tile/band bounded. Hosts may
/// set stricter limits; GPU limits are checked separately.
#[derive(Clone, Copy, Debug)]
pub struct ProjectLimits {
    pub metadata_bytes: u64,
    pub asset_bytes: u64,
    pub raster_bytes: u64,
    pub dimension: u32,
    pub layers: usize,
}
impl Default for ProjectLimits {
    fn default() -> Self {
        Self {
            metadata_bytes: 64 * 1024 * 1024,
            asset_bytes: u64::MAX,
            raster_bytes: u64::MAX,
            dimension: crate::MAX_EXTENT,
            layers: 4096,
        }
    }
}

const OTHER_RECORDS_SHARE: usize = 16;
impl ProjectLimits {
    pub fn for_import() -> Self {
        Self::default()
    }
    pub fn tile_capacity(&self) -> usize {
        let [bytes, values] = crate::package::codec::tile_record_cost();
        let manifest = crate::package::manifest::ManifestLimits::default();
        let metadata = usize::try_from(self.metadata_bytes).unwrap_or(usize::MAX).min(manifest.metadata_bytes);
        let tiles = [metadata / bytes, manifest.traversal_nodes / values, manifest.resources].into_iter().min().unwrap_or(0);
        tiles - tiles / OTHER_RECORDS_SHARE
    }
}

impl Document {
    pub fn saved_tiles(&self)->usize{
        let mut roots=RootInventory::default();roots.document(self);
        let mut owners=BTreeSet::new();
        let sources=roots.sources.iter().filter(|source|owners.insert(Arc::as_ptr(source) as usize)).map(|source|source.tiles.len()).sum::<usize>();
        self.scene().targets().filter(|target|!matches!(target,SourceTarget::Selection(_))).filter_map(|target|{
            let domain=self.scene().target_extent(target).map(|side|side.div_ceil(raster::TILE_SIZE) as usize);
            let planes=if matches!(target,SourceTarget::Coverage(_)) {1} else {2};
            Some(self.scene().raster(target)?.tile_bound(domain[0].saturating_mul(domain[1]).saturating_mul(planes)))
        }).fold(sources,usize::saturating_add)
    }
    pub fn admit_saved_tiles(&self,before:usize)->Result<(),DocumentError>{
        let tiles=self.saved_tiles();
        if tiles>before && tiles>ProjectLimits::default().tile_capacity() {return Err(DocumentError::InvalidLayerOperation(crate::SAVE_CAPACITY_EXCEEDED));}
        Ok(())
    }
    pub fn validate(&self,limits:ProjectLimits)->Result<(),String>{
        self.validate_payloads().map_err(|e|e.to_string())?;
        self.admit(limits)?;
        self.validate_integrity()
    }
    pub fn validate_integrity(&self)->Result<(),String>{
        self.validate_payloads().map_err(|e|e.to_string())?;
        if self.revision==u64::MAX{return Err("Invalid artwork revision".into());}
        let mut roots=RootInventory::default();roots.document(self);
        for source in roots.sources {source.validate()?;}
        for selection in roots.selections {
            selection.validate().map_err(|e|e.to_string())?;
            if let SelectionShape::Pixels(p)=&selection.shape {p.validate_package()?;}
        }
        for target in self.scene().targets(){
            let Some(revision)=self.scene().raster(target)else{continue;};
            match revision.try_data(){
                Some(Ok(data))=>data.validate_mode(self.scene().target_extent(target),matches!(target,SourceTarget::Coverage(_)),self.composition().color,self.scene().color_mode(target))?,
                Some(Err(error))=>return Err(error),None=>{},
            }
        }
        Ok(())
    }
    pub fn admit(&self,limits:ProjectLimits)->Result<(),String>{
        let composition=self.composition();
        if composition.size.contains(&0)||composition.size.iter().any(|v|*v>limits.dimension)||self.artwork.occurrences.len()>limits.layers{return Err("Invalid or oversized artwork".into());}
        let mut roots=RootInventory::default();roots.document(self);
        let mut sources=color::source::SourceAccounting::default();
        let mut resources=history_budget::Accounting::default();
        let mut asset_bytes=0u64;
        for image in roots.images {asset_bytes=asset_bytes.saturating_add(sources.charge_image(image) as u64);}
        for source in roots.sources {
            if source.extent.iter().any(|v|*v>limits.dimension){return Err("Source image exceeds the dimension limit".into());}
            asset_bytes=asset_bytes.saturating_add(sources.charge(source) as u64);
        }
        for resource in roots.resources {asset_bytes=asset_bytes.saturating_add(resources.charge_resource(resource) as u64);}
        let mut byte_owners=BTreeSet::new();
        for profile in roots.profiles {asset_bytes=asset_bytes.saturating_add(sources.charge_profile(profile) as u64);}
        for block in self.artwork.metadata.blocks().into_iter().flatten(){if byte_owners.insert(block.as_ptr() as usize){asset_bytes=asset_bytes.saturating_add(block.len() as u64);}}
        for program in roots.programs {for shader in std::iter::once(&program.wgsl).chain(program.lookups.iter().map(|lookup|&lookup.wgsl)) {if let Ok(sources)=shader.sources(){for source in sources{if byte_owners.insert(source.as_ptr() as usize){asset_bytes=asset_bytes.saturating_add(source.len() as u64);}}}}}
        if asset_bytes>limits.asset_bytes{return Err(crate::MEMORY_BUDGET_EXCEEDED.into());}
        let mut selection_bytes=0u64;
        for selection in roots.selections {
            if let SelectionShape::Pixels(p)=&selection.shape && p.extent().iter().any(|v|*v>limits.dimension) {
                return Err("Selection exceeds the image limit".into());
            }
            selection_bytes=selection_bytes.saturating_add(resources.charge_selection(selection) as u64);
        }
        if selection_bytes>limits.raster_bytes{return Err(crate::MEMORY_BUDGET_EXCEEDED.into());}
        let mut raster_bytes=selection_bytes;
        for target in self.scene().targets(){
            if self.scene().target_extent(target).iter().any(|v|*v>limits.dimension){return Err("Target exceeds the dimension limit".into());}
            let Some(revision)=self.scene().raster(target) else{continue;};
            match revision.try_data(){
                Some(Ok(data))=>for tile in data.tiles.values(){raster_bytes=raster_bytes.saturating_add(tile.descriptor().byte_len([raster::TILE_SIZE;2]).ok_or("Unsupported raster pixels")? as u64);},
                Some(Err(error))=>return Err(error),None=>raster_bytes=raster_bytes.saturating_add(revision.pending_bytes() as u64),
            }
        }
        if raster_bytes>limits.raster_bytes{return Err(crate::MEMORY_BUDGET_EXCEEDED.into());}
        crate::package::codec::admit_metadata(&self.artwork,limits)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lookup_shader_assets_are_admitted_once_across_effect_applications() {
        let program=crate::bundled_effect_catalog().get("gaussian_blur").unwrap().program();
        let main=program.wgsl.sources().unwrap();
        let lookup=program.lookups[0].wgsl.sources().unwrap();
        assert!(lookup.iter().any(|code|!main.iter().any(|source|Arc::ptr_eq(source.storage(),code.storage()))));
        let mut artwork=crate::Artwork::new([16,16]).unwrap();
        for _ in 0..2 {
            let instance=crate::EffectInstance::new(program.clone());
            let effect=artwork.effects.insert(crate::PortableId::random(),crate::EffectApplication::new(instance.program,instance.values,[16,16])).unwrap();
            let occurrence=artwork.occurrences.insert(crate::PortableId::random(),crate::Occurrence::new(crate::OccurrenceContent::Effect(effect),"Blur")).unwrap();
            let stack=artwork.compositions.get(artwork.root).unwrap().result;
            artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
        }
        let document=Document::from_artwork(artwork).unwrap();
        let mut code=BTreeMap::new();
        for source in main.iter().chain(lookup) {code.insert(source.as_ptr() as usize,source.len() as u64);}
        let bytes=code.values().sum();
        document.admit(ProjectLimits {asset_bytes:bytes,..Default::default()}).unwrap();
        assert!(document.admit(ProjectLimits {asset_bytes:bytes-1,..Default::default()}).is_err());
    }
}

#[cfg(test)]
mod capacity_tests {
    use super::*;
    use crate::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey, TILE_SIZE};
    fn layer(document: &mut Document, tiles: &[RasterTile], grid: [u32; 2]) -> OccurrenceHandle {
        let source = document.working.occurrence.unwrap();
        let (edit, created) = document.duplicate_layers_edit(&[source]).unwrap();
        document.apply(edit).unwrap();
        let target = document.scene().source_target(created[0]).unwrap();
        *document.target_raster_mut(target).unwrap() = RasterRevision::backed(RasterData { tiles: tiles.iter().enumerate().map(|(index, tile)|
            (TileKey { plane: RasterPlane::Color, coordinate: [index as u32 % grid[0], index as u32 / grid[0]] }, tile.clone())).collect(), ..Default::default() });
        created[0]
    }
    fn unique(document: &Document, count: usize) -> Vec<RasterTile> {
        let descriptor = document.composition().color.paint_descriptor();
        let compressed = TileBlob::encode(descriptor, &vec![0; descriptor.byte_len([TILE_SIZE; 2]).unwrap()]).unwrap().compressed_on_worker().unwrap().to_vec();
        (0..count).map(|_| RasterTile::backed(TileBlob::from_verified_resource(PortableId::random(), descriptor, compressed.as_slice().into()).unwrap())).collect()
    }

    #[test]
    fn the_tile_capacity_covers_every_unique_tile_record_a_save_or_recovery_writes() {
        let grid = [16, 16];
        let mut document = Document::new(PortableId::random(), grid[0] * TILE_SIZE, grid[1] * TILE_SIZE, DocumentNames { paint: "ink".into(), paper: "paper".into() });
        let empty = crate::package::codec::PreparedPackage::prepare(&document.artwork.capture(crate::authored::CaptureCheckpoint {owner:1,document:document.artwork.id,session_generation:1,artwork_generation:1,working_generation:1,edit_checkpoint:1}).unwrap(), None, &std::sync::atomic::AtomicBool::new(false)).unwrap().manifest().len();
        let tiles = unique(&document, (grid[0] * grid[1]) as usize);
        layer(&mut document, &tiles, grid);
        let full = crate::package::codec::PreparedPackage::prepare(&document.artwork.capture(crate::authored::CaptureCheckpoint {owner:1,document:document.artwork.id,session_generation:1,artwork_generation:1,working_generation:1,edit_checkpoint:2}).unwrap(), None, &std::sync::atomic::AtomicBool::new(false)).unwrap().manifest().len();
        let [bytes, _] = crate::package::codec::tile_record_cost();
        assert!((full - empty) / tiles.len() < bytes, "{} manifest bytes per tile against an estimate of {bytes}", (full - empty) / tiles.len());
        let capacity = ProjectLimits::default().tile_capacity();
        let full_layer = (crate::MAX_EXTENT / TILE_SIZE).pow(2) as usize;
        assert!(capacity > 2 * full_layer && capacity * bytes < 63 * 1024 * 1024, "{capacity} tiles of {bytes} bytes must fit the package and recovery metadata");
    }

    #[test]
    fn edits_that_would_pass_the_save_capacity_are_refused_and_others_still_apply() {
        let grid = [256, 256];
        let mut document = Document::new(PortableId::random(), grid[0] * TILE_SIZE, grid[1] * TILE_SIZE, DocumentNames { paint: "ink".into(), paper: "paper".into() });
        let shared = unique(&document, 1).remove(0);
        let capacity = ProjectLimits::default().tile_capacity();
        let mut layers = Vec::new();
        while document.saved_tiles() < capacity {
            let count = (capacity - document.saved_tiles()).min((grid[0] * grid[1]) as usize);
            layers.push(layer(&mut document, &vec![shared.clone(); count], grid));
        }
        assert_eq!(document.saved_tiles(), capacity);
        let mut editor = Editor::new(document);
        let (duplicate, _) = editor.document().duplicate_layers_edit(&[layers[0]]).unwrap();
        assert_eq!(editor.perform(duplicate).unwrap_err().to_string(), crate::SAVE_CAPACITY_EXCEEDED);
        assert_eq!(editor.document().saved_tiles(), capacity);
        editor.perform(editor.document().delete_layers_edit(&[layers[0]]).unwrap()).unwrap();
        assert!(editor.document().saved_tiles() < capacity);
        let (duplicate, _) = editor.document().duplicate_layers_edit(&[*layers.last().unwrap()]).unwrap();
        editor.perform(duplicate).unwrap();
        assert!(editor.document().saved_tiles() <= capacity);
    }
}
