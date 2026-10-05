use crate::authored::*;
use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformPixelsRefusal {
    Target,
    Locked,
    Unchanged,
    Pending,
}
impl std::fmt::Display for TransformPixelsRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Target => "Select a layer or mask",
            Self::Locked => "The layer is locked",
            Self::Unchanged => "This target has no transform to apply",
            Self::Pending => "Wait for the current edit",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransformPixelsScope {
    Paint { linked_mask: bool },
    Mask,
}
#[derive(Clone, Debug)]
pub struct TransformPixelsPlan {
    pub target: SourceTarget,
    pub scope: TransformPixelsScope,
    pub scene: Arc<SceneSnapshot>,
    pub output: Edit,
    pub geometry: ImageTransform,
    pub origin: Point,
    pub extent: [u32; 2],
    pub paint: Option<PaintHandle>,
    pub coverage: Option<CoverageHandle>,
}
impl TransformPixelsPlan {
    pub fn reserved_edit(&self) -> Edit {
        let pages = self.extent.iter().map(|v| u64::from(v.div_ceil(raster::TILE_SIZE))).product::<u64>();
        let color = self.scene.view().composition().color;
        let reserve = |planes: &[raster::RasterPlane]| {
            raster::RasterRevision::pending_within(
                pages * planes.iter().map(|p| raster::TileBlob::max_compressed_len(p.descriptor(color)).unwrap() as u64 + 96).sum::<u64>(),
            )
        };
        fn reserve_records(
            edit: &mut Edit,
            paint: Option<PaintHandle>,
            coverage: Option<CoverageHandle>,
            reserve: &impl Fn(&[raster::RasterPlane]) -> raster::RasterRevision,
        ) {
            match edit {
                Edit::Paint(c) if Some(c.handle) == paint => {
                    if let Some(p) = c.value.as_mut() {
                        p.raster =
                            reserve(&[raster::RasterPlane::Color, raster::RasterPlane::WatercolorWetness]);
                    }
                }
                Edit::Coverage(c) if Some(c.handle) == coverage => {
                    if let Some(p) = c.value.as_mut() {
                        p.raster = reserve(&[raster::RasterPlane::Mask]);
                    }
                }
                Edit::Batch(edits) => {
                    for edit in edits {
                        reserve_records(edit, paint, coverage, reserve);
                    }
                }
                _ => {}
            }
        }
        let mut output = self.output.clone();
        reserve_records(&mut output, self.paint, self.coverage, &reserve);
        output
    }
}
impl Document {
    pub fn transform_pixels_refusal(&self, target: SourceTarget) -> Option<TransformPixelsRefusal> {
        use TransformPixelsRefusal::*;
        let scene = self.scene();
        let Some(owner) = scene.source_owner(target) else {
            return Some(Target);
        };
        let o = scene.occurrence(owner).unwrap();
        if self.is_locked(owner) {
            return Some(Locked);
        }
        if !matches!(target, SourceTarget::Paint(_) | SourceTarget::Coverage(_)) {
            return Some(Target);
        }
        if scene.source_target(owner).and_then(|t| self.target_operations(t)).is_some_and(|ops| !ops.is_empty())
            || o.mask.as_ref().and_then(|m| self.target_operations(SourceTarget::Coverage(m.source))).is_some_and(|ops| !ops.is_empty())
        {
            return Some(Pending);
        }
        let identity = if matches!(target, SourceTarget::Coverage(_)) {
            self.target_geometry(target).as_affine() == Some(Affine::translation(self.target_offset(target)))
        } else {
            o.placement.as_affine() == Some(Affine::IDENTITY)
        };
        identity.then_some(Unchanged)
    }
    pub fn transform_pixels_plan(
        &self,
        target: SourceTarget,
        interpolation: Interpolation,
        limits: ProjectLimits,
    ) -> Result<TransformPixelsPlan, String> {
        if let Some(reason) = self.transform_pixels_refusal(target) {
            return Err(reason.to_string());
        }
        let scene = self.scene();
        let owner = scene.source_owner(target).unwrap();
        let old = scene.occurrence(owner).unwrap();
        let mut o = old.clone();
        let active_mask = matches!(target, SourceTarget::Coverage(_));
        let paired = active_mask && old.mask.as_ref().unwrap().linked && old.placement.as_affine().is_none();
        let scalar = active_mask && !paired;
        let paint = if scalar {
            None
        } else {
            match old.content {
                OccurrenceContent::Paint(h) => Some(h),
                _ => return Err("Select a paint layer".into()),
            }
        };
        let coverage = old.mask.as_ref().filter(|m| scalar || m.linked).map(|m| m.source);
        let scope = if scalar { TransformPixelsScope::Mask } else { TransformPixelsScope::Paint { linked_mask: coverage.is_some() } };
        let capture = paint.map(SourceTarget::Paint).unwrap_or(target);
        let mut geometry = scene.target_geometry(capture);
        geometry.placement.interpolation = interpolation;
        let mut bounds = Rect::from_extent(self.composition().size);
        for t in paint.map(SourceTarget::Paint).into_iter().chain(coverage.map(SourceTarget::Coverage)) {
            let map = scene.target_geometry(t);
            let domain = Rect::from_extent(scene.target_extent(t)).outset(interpolation.support() as f32);
            map.validate_for(domain).map_err(|e| e.to_string())?;
            bounds = bounds.union(map.forward_bounds(domain));
        }
        if [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y].iter().any(|v| !v.is_finite()) {
            return Err("Invalid transform geometry".into());
        }
        let origin = Point { x: bounds.min.x.floor(), y: bounds.min.y.floor() };
        let extent = [(bounds.max.x.ceil() - origin.x) as u32, (bounds.max.y.ceil() - origin.y) as u32];
        if extent.contains(&0) || extent.iter().any(|v| *v > limits.dimension) {
            return Err(format!("A target would reach past {} px", limits.dimension));
        }
        let pages = extent.iter().map(|v| u64::from(v.div_ceil(raster::TILE_SIZE))).product::<u64>();
        let color = self.composition().color;
        let color_bytes = color.paint_descriptor().byte_len([raster::TILE_SIZE; 2]).unwrap() as u64;
        let mask_bytes = color.coverage_descriptor().byte_len([raster::TILE_SIZE; 2]).unwrap() as u64;
        let planes = if scalar { 1 } else { 3 + u64::from(coverage.is_some()) };
        let bytes = if scalar { mask_bytes } else { color_bytes + (2 + u64::from(coverage.is_some())) * mask_bytes };
        if pages * planes > limits.tiles as u64 || pages * bytes > limits.raster_bytes {
            return Err("The transformed pixels exceed the drawing's memory limit".into());
        }
        let world = self.layer_offset(owner);
        let parents = Point { x: world.x - old.translation.x, y: world.y - old.translation.y };
        let mut edits = Vec::new();
        if let Some(h) = paint {
            let mut p = self.artwork.paint.get(h).unwrap().clone();
            p.domain = extent;
            p.base = None;
            p.raster = Default::default();
            p.operations = Arc::default();
            o.translation = Point { x: origin.x - parents.x, y: origin.y - parents.y };
            o.placement = LayerPlacement::IDENTITY;
            edits.push(Edit::Paint(RecordChange::replace(&self.artwork.paint, h, Some(p)).map_err(str::to_owned)?));
        }
        if let Some(h) = coverage {
            let mut c = self.artwork.coverage.get(h).unwrap().clone();
            c.domain = extent;
            c.initial = None;
            c.raster = Default::default();
            c.operations = Arc::default();
            let mask = o.mask.as_mut().unwrap();
            if scalar && mask.linked {
                let owner_map = scene
                    .target_geometry(scene.source_target(owner).ok_or("Apply the layer transform to edit its linked mask")?)
                    .projective()
                    .ok_or("Apply the layer transform to edit its linked mask")?;
                mask.translation = old.translation;
                mask.placement = Projective::from_affine(Affine::translation(origin))
                    .then(owner_map.inverse().ok_or("Invalid owner transform")?)
                    .ok_or("Invalid mask transform")?;
            } else {
                mask.translation = Point { x: origin.x - parents.x, y: origin.y - parents.y };
                mask.placement = Projective::IDENTITY;
            }
            edits.push(Edit::Coverage(RecordChange::replace(&self.artwork.coverage, h, Some(c)).map_err(str::to_owned)?));
        }
        edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences, owner, Some(o)).map_err(str::to_owned)?));
        geometry.placement = geometry
            .placement
            .post(Projective::from_affine(Affine::translation(Point { x: -origin.x, y: -origin.y })))
            .ok_or("Invalid capture origin")?;
        Ok(TransformPixelsPlan {
            target,
            scope,
            scene: self.snapshot(),
            output: Edit::Batch(edits),
            geometry,
            origin,
            extent,
            paint,
            coverage,
        })
    }
}
