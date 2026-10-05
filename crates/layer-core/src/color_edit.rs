use super::*;
use crate::authored::*;

impl Document {
    pub fn color_edit(
        &self,
        color: color::DocumentColor,
        paint: Vec<RecordChange<PaintSource>>,
        coverage: Vec<RecordChange<CoverageSource>>,
    ) -> Result<Edit, DocumentError> {
        let invalid = |message| DocumentError::InvalidLayerOperation(message);
        if paint.len() != self.artwork.paint.len() || coverage.len() != self.artwork.coverage.len() {
            return Err(invalid("Color changes must preserve every source"));
        }
        let mut seen = BTreeSet::new();
        for change in &paint {
            if !seen.insert(change.handle) || self.artwork.paint.id(change.handle) != Some(change.id) {
                return Err(invalid("Color changes must preserve source identity"));
            }
            let old = self.artwork.paint.get(change.handle).ok_or(invalid("Missing color source"))?;
            let new = change.value.as_ref().ok_or(invalid("Color changes must preserve every source"))?;
            if old.color_mode != new.color_mode || old.domain != new.domain || old.operations != new.operations || !new.operations.is_empty() {
                return Err(invalid("Color changes must preserve source properties and completed edits"));
            }
            if old.base.as_ref().is_some_and(PaintBase::is_original)
                && !old.base.as_ref().zip(new.base.as_ref()).is_some_and(|(a,b)|a.image.id()==b.image.id() && a.image.same_owner(&b.image)) {
                return Err(invalid("Document color changes must preserve source-profile bases"));
            }
            if old.base.as_ref().map(|b|(b.policy,b.offset,b.image.extent))!=new.base.as_ref().map(|b|(b.policy,b.offset,b.image.extent)) {return Err(invalid("Color changes must preserve paint base roles, offsets and full extents"));}
            validate_color_root(&old.raster,&new.raster,new.domain,false,color,new.color_mode)?;
            if let Some(base)=&new.base {base.validate(new.domain,color).map_err(DocumentError::InvalidArtwork)?;}
        }
        let mut seen = BTreeSet::new();
        for change in &coverage {
            if !seen.insert(change.handle) || self.artwork.coverage.id(change.handle) != Some(change.id) {
                return Err(invalid("Color changes must preserve source identity"));
            }
            let old = self.artwork.coverage.get(change.handle).ok_or(invalid("Missing color source"))?;
            let new = change.value.as_ref().ok_or(invalid("Color changes must preserve every source"))?;
            let mut metadata = new.clone();
            metadata.raster = old.raster.clone();
            if metadata != *old || !new.operations.is_empty() {
                return Err(invalid("Color changes must preserve mask properties and completed edits"));
            }
            validate_color_root(&old.raster, &new.raster, new.domain, true, color, Default::default())?;
        }
        let mut composition = self.composition().clone();
        composition.color = color;
        composition.blend = composition.blend.for_depth(color.depth);
        let mut edits = vec![Edit::Composition(RecordChange::replace(&self.artwork.compositions, self.artwork.root, Some(composition))?)];
        edits.extend(paint.into_iter().map(Edit::Paint));
        edits.extend(coverage.into_iter().map(Edit::Coverage));
        Ok(Edit::Batch(edits))
    }
}
fn validate_color_root(
    before: &raster::RasterRevision,
    root: &raster::RasterRevision,
    domain: [u32; 2],
    mask: bool,
    color: color::DocumentColor,
    mode: color::LayerColorMode,
) -> Result<(), DocumentError> {
    let invalid = |message| DocumentError::InvalidLayerOperation(message);
    let completed = |r: &raster::RasterRevision| {
        r.try_data().and_then(Result::ok).filter(|r| r.host_backed()).ok_or(invalid("Color changes require completed raster backing"))
    };
    let before = completed(before)?;
    let data = completed(root)?;
    if before.watercolor != data.watercolor || !before.tiles.keys().eq(data.tiles.keys()) {
        return Err(invalid("Color changes must preserve raster coverage and watercolor state"));
    }
    data.validate_index_mode(domain, mask, color, mode).map_err(|_| invalid("Color backing does not match the new document mode"))
}
