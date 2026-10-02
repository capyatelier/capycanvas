use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformPixelsRefusal { Target, Locked, Unchanged, Pending }

impl std::fmt::Display for TransformPixelsRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Target => "Select a paint or photo layer",
            Self::Locked => "The layer is locked",
            Self::Unchanged => "This layer has no transform to apply",
            Self::Pending => "Wait for the current edit",
        })
    }
}

#[derive(Clone, Debug)]
pub struct TransformPixelsPlan {
    pub input: Project,
    pub output: Layer,
    pub interpolation: Interpolation,
    pub linked_mask: bool,
}

impl TransformPixelsPlan {
    pub fn reserved_edit(&self) -> Edit {
        let mut output = self.output.clone();
        let pages = output.local_extent([self.input.document.width, self.input.document.height]).iter()
            .map(|v| u64::from(v.div_ceil(raster::TILE_SIZE))).product::<u64>();
        let reserve = |planes: &[raster::RasterPlane]| raster::RasterRevision::pending_within(pages * planes.iter()
            .map(|plane| raster::TileBlob::max_compressed_len(plane.descriptor(self.input.document.color)).unwrap() as u64 + 96)
            .sum::<u64>());
        output.raster = reserve(&[raster::RasterPlane::Color, raster::RasterPlane::Wetness, raster::RasterPlane::WatercolorWetness]);
        if self.linked_mask { output.mask.as_mut().unwrap().raster = reserve(&[raster::RasterPlane::Mask]); }
        Edit::ReplaceLayer(Box::new(output))
    }
}

impl Document {
    pub fn transform_pixels_refusal(&self, id: LayerId) -> Option<TransformPixelsRefusal> {
        use TransformPixelsRefusal::*;
        match self.layer(id) {
            None => Some(Target),
            Some(layer) if layer.kind != LayerKind::Paint => Some(Target),
            Some(_) if self.is_locked(id) => Some(Locked),
            Some(layer) if layer.properties.placement == Affine::IDENTITY => Some(Unchanged),
            Some(layer) if !layer.pending_operations.is_empty() => Some(Pending),
            _ => None,
        }
    }

    pub fn transform_pixels_plan(&self, id: LayerId, interpolation: Interpolation, limits: ProjectLimits)
        -> Result<TransformPixelsPlan, String> {
        if let Some(reason) = self.transform_pixels_refusal(id) { return Err(reason.to_string()); }
        let original = self.layer(id).unwrap();
        let local = original.local_extent([self.width, self.height]);
        let linked_mask = original.mask.as_ref().is_some_and(|mask| mask.linked);
        let targets = std::iter::once(id).chain(original.mask.iter().filter(|mask| mask.linked).map(|mask| mask.id));
        let mut bounds = Rect::from_extent([self.width, self.height]);
        for target in targets {
            let transform = self.layer_transform(target);
            let mapped = transform.bounds(Rect::from_extent(local).outset(interpolation.support() as f32));
            if transform.inverse().is_none() || [mapped.min.x, mapped.min.y, mapped.max.x, mapped.max.y].iter().any(|v| !v.is_finite()) {
                return Err("The layer has invalid transform geometry".into());
            }
            bounds = bounds.union(mapped);
        }
        let origin = Point { x: bounds.min.x.floor(), y: bounds.min.y.floor() };
        let mut extent = [(bounds.max.x.ceil() - origin.x) as u32, (bounds.max.y.ceil() - origin.y) as u32];
        if original.mask.as_ref().is_some_and(|mask| !mask.linked) {
            extent = std::array::from_fn(|i| extent[i].max(local[i]));
        }
        if extent.contains(&0) || extent.iter().any(|v| *v > limits.dimension) {
            return Err(format!("A layer would reach past {} px, including its hidden pixels", limits.dimension));
        }
        let pages = extent.iter().map(|v| u64::from(v.div_ceil(raster::TILE_SIZE))).product::<u64>();
        let color = self.color;
        let bytes = pages * (color.paint_descriptor().byte_len([raster::TILE_SIZE; 2]).unwrap() as u64
            + (2 + u64::from(linked_mask)) * color.coverage_descriptor().byte_len([raster::TILE_SIZE; 2]).unwrap() as u64);
        if pages * (3 + u64::from(linked_mask)) > limits.tiles as u64 || bytes > limits.raster_bytes {
            return Err("The transformed pixels exceed the drawing's memory limit".into());
        }
        let offset = self.layer_offset(id);
        let mut output = original.clone();
        output.properties.offset = Point { x: origin.x - offset.x + original.properties.offset.x,
            y: origin.y - offset.y + original.properties.offset.y };
        output.properties.placement = Affine::IDENTITY;
        output.properties.extent = Some(extent);
        output.source = None;
        output.raster = Default::default();
        if let Some(mask) = output.mask.as_mut().filter(|mask| mask.linked) {
            mask.offset = output.properties.offset;
            mask.placement = Affine::IDENTITY;
            mask.initial = None;
            mask.raster = Default::default();
        }
        let mut input = original.composite_snapshot();
        input.properties.parent = None;
        input.properties.extent = Some(local);
        input.properties.offset = Point { x: offset.x - origin.x, y: offset.y - origin.y };
        if let Some(mask) = &mut input.mask {
            let world = self.layer_offset(mask.id);
            mask.offset = Point { x: world.x - origin.x, y: world.y - origin.y };
        }
        let mut document = self.clone();
        document.width = extent[0];
        document.height = extent[1];
        document.layers = vec![input];
        document.active_layer = id;
        document.active_mask = false;
        document.selection = None;
        document.rulers.clear();
        document.reference_layers.clear();
        Ok(TransformPixelsPlan { input: Project { document }, output, interpolation, linked_mask })
    }
}
