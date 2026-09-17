//! Color interpretation and all committed backing are published together.
use super::*;

impl Document {
    pub(super) fn apply_color_edit(
        &mut self,
        color: color::DocumentColor,
        layers: Vec<Layer>,
    ) -> Result<Edit, DocumentError> {
        let invalid = |message| DocumentError::InvalidLayerOperation(message);
        if layers.len() != self.layers.len() {
            return Err(invalid("Color changes must preserve every layer"));
        }
        for (old, new) in self.layers.iter().zip(&layers) {
            let mut metadata = new.clone();
            metadata.raster = old.raster.clone();
            metadata.source = old.source.clone();
            if let (Some(a), Some(b)) = (&old.mask, &mut metadata.mask) {
                b.raster = a.raster.clone();
            }
            if metadata != *old
                || !new.pending_operations.is_empty()
                || new
                    .mask
                    .as_ref()
                    .is_some_and(|m| !m.pending_operations.is_empty())
            {
                return Err(invalid(
                    "Color changes must preserve layer properties and completed edits",
                ));
            }
            // Originals have an independent interpretation. Document color
            // changes may only rewrite an already rasterized image base.
            if old.source.as_ref().is_some_and(|s| s.is_original())
                && !old
                    .source
                    .as_ref()
                    .zip(new.source.as_ref())
                    .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
            {
                return Err(invalid(
                    "Document color changes must preserve retained originals",
                ));
            }
            if old.source.as_ref().map(|s| (s.kind, s.extent))
                != new.source.as_ref().map(|s| (s.kind, s.extent))
            {
                return Err(invalid(
                    "Color changes must preserve image roles and full extents",
                ));
            }
            for (before, root, mask) in std::iter::once((&old.raster, &new.raster, false)).chain(
                old.mask
                    .iter()
                    .zip(&new.mask)
                    .map(|(a, b)| (&a.raster, &b.raster, true)),
            ) {
                let before = before
                    .try_data()
                    .and_then(Result::ok)
                    .filter(|r| r.host_backed())
                    .ok_or_else(|| invalid("Color changes require completed raster backing"))?;
                let data = root
                    .try_data()
                    .and_then(Result::ok)
                    .filter(|r| r.host_backed())
                    .ok_or_else(|| invalid("Color changes require completed raster backing"))?;
                if before.watercolor != data.watercolor
                    || !before.tiles.keys().eq(data.tiles.keys())
                {
                    return Err(invalid(
                        "Color changes must preserve raster coverage and watercolor state",
                    ));
                }
                data.validate_index(new.local_extent([self.width, self.height]), mask, color)
                    .map_err(|_| invalid("Color backing does not match the new document mode"))?;
            }
        }
        let mut candidate = self.clone();
        candidate.color = color;
        candidate.layers = layers;
        project::validate_document(&candidate, ProjectLimits::default())
            .map_err(|_| invalid("Invalid document color candidate"))?;
        let inverse = Edit::SetColor {
            color: self.color,
            layers: std::mem::replace(&mut self.layers, candidate.layers),
        };
        self.color = color;
        Ok(inverse)
    }
}
