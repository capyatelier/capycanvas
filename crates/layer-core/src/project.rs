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
    pub tiles: usize,
    pub dimension: u32,
    pub layers: usize,
}
impl Default for ProjectLimits {
    fn default() -> Self {
        Self {
            metadata_bytes: 64 * 1024 * 1024,
            asset_bytes: 512 * 1024 * 1024,
            raster_bytes: 1024 * 1024 * 1024,
            tiles: 16384,
            dimension: crate::MAX_EXTENT,
            layers: 4096,
        }
    }
}

impl Document {
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
                Some(Ok(data))=>data.validate(self.scene().target_extent(target),matches!(target,SourceTarget::Coverage(_)),self.composition().color)?,
                Some(Err(error))=>return Err(error),None=>{},
            }
        }
        Ok(())
    }
    pub fn admit(&self,limits:ProjectLimits)->Result<(),String>{
        let composition=self.composition();
        if composition.size.contains(&0)||composition.size.iter().any(|v|*v>limits.dimension)||self.artwork.occurrences.len()>limits.layers{return Err("Invalid or oversized artwork".into());}
        if self.artwork.occurrences.iter().any(|(_,_,o)|o.name.len()>4096){return Err("Oversized occurrence name".into());}
        let mut roots=RootInventory::default();roots.document(self);
        let mut sources=color::source::SourceAccounting::default();
        let mut resources=history_budget::Accounting::default();
        let mut asset_bytes=0u64;let mut source_tiles=0usize;let mut source_owners=BTreeSet::new();
        for source in roots.sources {
            if source.extent.iter().any(|v|*v>limits.dimension){return Err("Source image exceeds the dimension limit".into());}
            if source_owners.insert(Arc::as_ptr(source) as usize){source_tiles=source_tiles.saturating_add(source.tiles.len());}
            asset_bytes=asset_bytes.saturating_add(sources.charge(source) as u64);
        }
        for resource in roots.resources {asset_bytes=asset_bytes.saturating_add(resources.charge_resource(resource) as u64);}
        let mut byte_owners=BTreeSet::new();
        for profile in roots.profiles {if let color::ColorProfile::Icc(bytes)=profile && byte_owners.insert(bytes.as_ptr() as usize){asset_bytes=asset_bytes.saturating_add(bytes.len() as u64);}}
        for block in self.artwork.metadata.blocks().into_iter().flatten(){if byte_owners.insert(block.as_ptr() as usize){asset_bytes=asset_bytes.saturating_add(block.len() as u64);}}
        for program in roots.programs {if let Ok(sources)=program.wgsl.sources(){for source in sources{if byte_owners.insert(source.as_ptr() as usize){asset_bytes=asset_bytes.saturating_add(source.len() as u64);}}}}
        if asset_bytes>limits.asset_bytes{return Err("Artwork resources exceed the memory limit".into());}
        let mut selection_bytes=0u64;
        for selection in roots.selections {
            if let SelectionShape::Pixels(p)=&selection.shape && p.extent().iter().any(|v|*v>limits.dimension) {
                return Err("Selection exceeds the image limit".into());
            }
            selection_bytes=selection_bytes.saturating_add(resources.charge_selection(selection) as u64);
        }
        if selection_bytes>limits.raster_bytes{return Err("Selection coverage exceeds the artwork memory limit".into());}
        let mut tiles=source_tiles;let mut raster_bytes=selection_bytes;
        for target in self.scene().targets(){
            if self.scene().target_extent(target).iter().any(|v|*v>limits.dimension){return Err("Target exceeds the dimension limit".into());}
            let Some(revision)=self.scene().raster(target) else{continue;};
            match revision.try_data(){
                Some(Ok(data))=>{
                    tiles=tiles.saturating_add(data.tiles.len());
                    for tile in data.tiles.values(){raster_bytes=raster_bytes.saturating_add(tile.descriptor().byte_len([raster::TILE_SIZE;2]).ok_or("Unsupported raster pixels")? as u64);}
                }
                Some(Err(error))=>return Err(error),None=>raster_bytes=raster_bytes.saturating_add(revision.pending_bytes() as u64),
            }
        }
        if tiles>limits.tiles || raster_bytes>limits.raster_bytes{return Err("Artwork pixels exceed the memory limit".into());}
        Ok(())
    }
}
