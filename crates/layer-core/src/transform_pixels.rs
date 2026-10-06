use crate::authored::*;
use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformPixelsRefusal {
    Target,
    Locked,
    Pending,
}
impl std::fmt::Display for TransformPixelsRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Target => "Select a layer or mask",
            Self::Locked => "The layer is locked",
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
    pub map: LayerPlacement,
    /// The pixels of the captured target the map moves, or its whole planes.
    pub source: Option<Rect>,
    pub geometry: ImageTransform,
    /// The document position of the output's first pixel.
    pub origin: [i64; 2],
    pub extent: [u32; 2],
    pub paint: Option<PaintHandle>,
    pub coverage: Option<CoverageHandle>,
}
const OUT_OF_RANGE: &str = "The transformed layer exceeds the editor's range";
fn plane_geometry(geometry: &ImageTransform, scene: SceneView<'_>, capture: SourceTarget, target: SourceTarget) -> ImageTransform {
    if target == capture {
        return geometry.clone();
    }
    let [owner, source] = [scene.target_offset(capture), scene.target_offset(target)];
    let source_from_owner = Projective::from_affine(Affine::translation(offsets::point([owner[0] - source[0], owner[1] - source[1]])));
    ImageTransform { placement: geometry.placement.clone(), source_from_owner: Some(source_from_owner), keep_source: false, source_base: None }
}
/// The pixels of `target`'s plane that `source`, in `capture`'s pixels, covers.
fn plane_domain(scene: SceneView<'_>, capture: SourceTarget, target: SourceTarget, source: Option<Rect>) -> Rect {
    let plane = Rect::from_extent(scene.target_extent(target));
    let [capture, target] = [scene.target_origin(capture), scene.target_origin(target)];
    let shift = offsets::point([capture[0] - target[0], capture[1] - target[1]]);
    source.map_or(plane, |source| plane.intersect(source.translated(shift)))
}
impl TransformPixelsPlan {
    pub fn plane_geometry(&self, target: SourceTarget) -> ImageTransform {
        plane_geometry(&self.geometry, self.scene.view(), self.paint.map(SourceTarget::Paint).unwrap_or(self.target), target)
    }
    pub fn plane_domain(&self, target: SourceTarget) -> Rect {
        plane_domain(self.scene.view(), self.paint.map(SourceTarget::Paint).unwrap_or(self.target), target, self.source)
    }
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
    pub fn layer_transform_refusal(&self, target: SourceTarget) -> Option<TransformPixelsRefusal> {
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
        None
    }
    /// Plan resampling `target`'s pixels through `map`, which needs to be
    /// valid only over `source`, the region of the target holding its
    /// content, or over each whole plane when nothing measured it.
    pub fn layer_transform_plan(
        &self,
        target: SourceTarget,
        map: &LayerPlacement,
        source: Option<Rect>,
        limits: ProjectLimits,
    ) -> Result<TransformPixelsPlan, String> {
        if let Some(reason) = self.layer_transform_refusal(target) {
            return Err(reason.to_string());
        }
        let scene = self.scene();
        let owner = scene.source_owner(target).unwrap();
        let old = scene.occurrence(owner).unwrap();
        let mut o = old.clone();
        let scalar = matches!(target, SourceTarget::Coverage(_));
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
        let capture_offset = scene.target_offset(capture);
        let mut geometry = ImageTransform { placement: map.clone(), ..Default::default() };
        let canvas = offsets::checked_sub([0; 2], capture_offset).ok_or(OUT_OF_RANGE)?;
        let mut bounds = Rect::from_extent(self.composition().size).translated(offsets::point(canvas));
        for t in paint.map(SourceTarget::Paint).into_iter().chain(coverage.map(SourceTarget::Coverage)) {
            let placed = plane_geometry(&geometry, scene, capture, t);
            let domain = plane_domain(scene, capture, t, source);
            if domain.is_empty() { continue; }
            let domain = domain.outset(map.interpolation.support() as f32);
            placed.validate_for(domain).map_err(|e| e.to_string())?;
            bounds = bounds.union(placed.forward_bounds(domain));
        }
        if [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y].iter().any(|v| !v.is_finite()) {
            return Err("Invalid transform geometry".into());
        }
        let local = Point { x: bounds.min.x.floor(), y: bounds.min.y.floor() };
        let extent = [(bounds.max.x.ceil() - local.x) as u32, (bounds.max.y.ceil() - local.y) as u32];
        if extent.contains(&0) || extent.iter().any(|v| *v > limits.dimension) {
            return Err(format!("A target would reach past {} px", limits.dimension));
        }
        let world = offsets::exact(local).and_then(|local| offsets::checked_add(capture_offset, local))
            .filter(|world| offsets::admitted(*world)).ok_or(OUT_OF_RANGE)?;
        let pages = extent.iter().map(|v| u64::from(v.div_ceil(raster::TILE_SIZE))).product::<u64>();
        let color = self.composition().color;
        let color_bytes = color.paint_descriptor().byte_len([raster::TILE_SIZE; 2]).unwrap() as u64;
        let mask_bytes = color.coverage_descriptor().byte_len([raster::TILE_SIZE; 2]).unwrap() as u64;
        let planes = if scalar { 1 } else { 3 + u64::from(coverage.is_some()) };
        let bytes = if scalar { mask_bytes } else { color_bytes + (2 + u64::from(coverage.is_some())) * mask_bytes };
        if pages * planes > limits.tiles as u64 || pages * bytes > limits.raster_bytes {
            return Err("The transformed pixels exceed the drawing's memory limit".into());
        }
        let parents = scene.layer_origin(scene.parent(owner));
        let mut edits = Vec::new();
        if let Some(h) = paint {
            let mut p = self.artwork.paint.get(h).unwrap().clone();
            p.domain = extent;
            p.base = None;
            p.raster = Default::default();
            p.operations = Arc::default();
            o.offset = offsets::checked_sub(world, parents).ok_or(OUT_OF_RANGE)?;
            edits.push(Edit::Paint(RecordChange::replace(&self.artwork.paint, h, Some(p)).map_err(str::to_owned)?));
        }
        if let Some(h) = coverage {
            let mut c = self.artwork.coverage.get(h).unwrap().clone();
            c.domain = extent;
            c.raster = Default::default();
            c.operations = Arc::default();
            let owner_offset = if o.positioned() { o.offset } else { [0; 2] };
            let mask = o.mask.as_mut().unwrap();
            mask.offset = offsets::checked_sub(world, parents).and_then(|frame| offsets::checked_sub(frame, if mask.linked { owner_offset } else { [0; 2] })).ok_or(OUT_OF_RANGE)?;
            edits.push(Edit::Coverage(RecordChange::replace(&self.artwork.coverage, h, Some(c)).map_err(str::to_owned)?));
        }
        edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences, owner, Some(o)).map_err(str::to_owned)?));
        geometry.placement = geometry
            .placement
            .post(Projective::from_affine(Affine::translation(Point { x: -local.x, y: -local.y })))
            .ok_or("Invalid capture origin")?;
        Ok(TransformPixelsPlan {
            target,
            scope,
            scene: self.snapshot(),
            output: Edit::Batch(edits),
            map: map.clone(),
            source,
            geometry,
            origin: world,
            extent,
            paint,
            coverage,
        })
    }
}
