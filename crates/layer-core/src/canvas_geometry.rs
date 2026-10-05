//! Canvas geometry: one plan turns a new canvas rectangle, and optionally a
//! rotation, flip or scale, into a single reversible batch. The canvas is a window over each
//! layer's local extent, so shrinking it hides pixels and growing it shows
//! them again.
//!
//! Every geometry edit keeps these invariants:
//! - Root offsets carry the canvas origin. Child offsets are relative to their
//!   group and change only when a resample moves the child.
//! - A paint layer without a source, and every mask, has a local extent that
//!   covers the canvas window, so the whole canvas stays paintable. Growing
//!   the canvas left or up rebases such a layer by whole tiles: its tiles are
//!   re-keyed and still shared, and its offset moves the other way.
//! - A layer with a source never rebases or resamples. A new strip beside a
//!   photo stays unpaintable, as a moved photo's does, and a rotation turns
//!   its placement.
//! - Stored extents shrink only when Delete Cropped Pixels trims a layer to
//!   the tiles its window touches. Hidden tiles count toward the project's
//!   tile and byte limits like visible ones.
//! - A rotation, flip or scale resamples paint layers and masks without a
//!   source into a frame that holds the whole mapped extent, so hidden
//!   corners are kept. The frame never shrinks below the layer's extent, since
//!   source and result share one tile grid; the renderer then drops the pages
//!   the result left empty, so tile limits count only the result. Effect
//!   distances in pixels scale with the image.
use crate::authored::*;
use crate::raster::{RasterData, RasterPlane, RasterRevision, TILE_SIZE, TileKey};
use crate::*;

/// The new canvas, in the current document's pixels. Its origin is negative
/// where the canvas grows left or up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasRect {
    pub origin: [i32; 2],
    pub size: [u32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasGeometry {
    pub rect: CanvasRect,
    /// Maps the current document onto the one `rect` is cut from: a point
    /// `p` lands at `linear.map(p) - rect.origin` on the new canvas. Anything
    /// but the identity resamples paint layers and their masks.
    pub linear: Affine,
    pub interpolation: Interpolation,
    /// Drop the pixels outside the new canvas instead of keeping them hidden.
    pub delete_outside: bool,
}
/// A flip or turn of the whole image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageOrientation {
    FlipHorizontal,
    FlipVertical,
    RotateLeft,
    RotateRight,
    Rotate180,
}

impl CanvasGeometry {
    /// Keep every pixel, hiding what falls outside `rect`.
    pub fn crop(rect: CanvasRect) -> Self {
        Self { rect, linear: Affine::IDENTITY, interpolation: Interpolation::default(), delete_outside: false }
    }
    /// Flip or turn a `canvas` of that size. Pixels move without resampling.
    pub fn orient(canvas: [u32; 2], orientation: ImageOrientation) -> Self {
        let [w, h] = canvas.map(|v| v as f32);
        let turned = [canvas[1], canvas[0]];
        let (linear, size) = match orientation {
            ImageOrientation::FlipHorizontal => (Affine([-1., 0., 0., 1., w, 0.]), canvas),
            ImageOrientation::FlipVertical => (Affine([1., 0., 0., -1., 0., h]), canvas),
            ImageOrientation::Rotate180 => (Affine([-1., 0., 0., -1., w, h]), canvas),
            ImageOrientation::RotateRight => (Affine([0., 1., -1., 0., h, 0.]), turned),
            ImageOrientation::RotateLeft => (Affine([0., -1., 1., 0., 0., w]), turned),
        };
        Self { rect: CanvasRect { origin: [0; 2], size }, linear, interpolation: Interpolation::Nearest, delete_outside: false }
    }
    /// Scale a `canvas` of that size to `size`.
    pub fn resize(canvas: [u32; 2], size: [u32; 2], interpolation: Interpolation) -> Self {
        let [x, y] = std::array::from_fn(|i| size[i] as f32 / canvas[i].max(1) as f32);
        Self { rect: CanvasRect { origin: [0; 2], size }, linear: Affine([x, 0., 0., y, 0., 0.]), interpolation, delete_outside: false }
    }
    /// Where a current document point lands on the new canvas.
    pub fn to_canvas(&self) -> Affine {
        let [x, y] = self.rect.origin.map(|v| -v as f32);
        self.linear.then(Affine::translation(Point { x, y }))
    }
}

/// One reversible batch and the pixel operations that run inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasGeometryPlan {
    /// The canvas size, then every changed layer, the selection and guides.
    pub edits: Vec<Edit>,
    /// Resampling for a rotation and erasing for Delete Cropped Pixels, run
    /// on each target after `edits` in the same undo step. Coverage ids are
    /// placeholders the caller allocates.
    pub operations: Vec<(SourceTarget, RasterOperation)>,
}

#[derive(Clone, Copy, Debug)]
pub struct GeometryLimits {
    pub project: ProjectLimits,
    /// The largest canvas side the renderer can compose.
    pub device_dimension: u32,
}
impl GeometryLimits {
    pub fn canvas_dimension(&self) -> u32 {
        self.project.dimension.min(self.device_dimension)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CanvasGeometryError {
    Unchanged,
    Empty,
    CanvasTooLarge { limit: u32 },
    ExtentTooLarge { limit: u32 },
    TooManyTiles { limit: usize },
    RasterTooLarge,
    Pending,
    Unsupported(&'static str),
    Document(DocumentError),
}
impl CanvasGeometryError {
    /// Whether dropping hidden pixels could bring the edit within the limits.
    pub fn exceeds_raster_limits(&self) -> bool {
        matches!(self, Self::TooManyTiles { .. } | Self::RasterTooLarge)
    }
}
impl fmt::Display for CanvasGeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unchanged => f.write_str("The canvas already has this size"),
            Self::Empty => f.write_str("The canvas needs at least one pixel on each side"),
            Self::CanvasTooLarge { limit } => {
                write!(f, "The canvas can be at most {limit} px on each side")
            }
            Self::ExtentTooLarge { limit } => write!(f, "A layer would reach past {limit} px, including its hidden pixels"),
            Self::TooManyTiles { limit } => write!(f, "The drawing would need more than {limit} tiles, including hidden pixels"),
            Self::RasterTooLarge => f.write_str("The drawing's pixels, including hidden ones, exceed the memory limit"),
            Self::Pending => f.write_str("Raster backing is busy; retry the edit"),
            Self::Unsupported(message) => f.write_str(message),
            Self::Document(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for CanvasGeometryError {}
impl From<DocumentError> for CanvasGeometryError {
    fn from(error: DocumentError) -> Self {
        Self::Document(error)
    }
}

/// A layer's rebase in whole tiles and its local extent after the edit.
/// `trim` keeps only the tiles inside that extent.
struct LayerChange {
    tiles: [i32; 2],
    extent: [u32; 2],
    trim: bool,
}

fn shift(point: Point, by: Point) -> Point {
    Point { x: point.x - by.x, y: point.y - by.y }
}

/// Move a target's local origin by `delta` pixels without moving its pixels:
/// the offset moves back by the same amount and a placement is conjugated.
fn rebased_placement(placement: &LayerPlacement, delta: Point) -> LayerPlacement {
    let to = Projective::from_affine(Affine::translation(delta));
    let mut result = placement.clone();
    if let Some(mesh) = &placement.mesh {
        let mut mesh = (**mesh).clone();
        mesh.frame = mesh.frame.then(Affine::translation(delta));
        result.mesh = Some(Arc::new(mesh));
        result.outer = result.outer.then(to).unwrap();
    } else {
        result.outer = Projective::from_affine(Affine::translation(Point { x: -delta.x, y: -delta.y }))
            .then(result.outer)
            .and_then(|m| m.then(to))
            .unwrap();
    }
    result
}
fn rebased_mask(placement: Projective, delta: Point) -> Projective {
    Projective::from_affine(Affine::translation(Point { x: -delta.x, y: -delta.y }))
        .then(placement)
        .and_then(|m| m.then(Projective::from_affine(Affine::translation(delta))))
        .unwrap()
}

fn raster_data(raster: &RasterRevision) -> Result<Arc<RasterData>, CanvasGeometryError> {
    match raster.try_data() {
        Some(Ok(data)) => Ok(data),
        Some(Err(_)) => Err(CanvasGeometryError::Document(DocumentError::InvalidLayerOperation("A layer's pixels failed to save"))),
        None => Err(CanvasGeometryError::Pending),
    }
}

fn rebased_raster(raster: &RasterRevision, change: &LayerChange) -> Result<RasterRevision, CanvasGeometryError> {
    if change.tiles == [0; 2] && !change.trim {
        return Ok(raster.clone());
    }
    let data = raster_data(raster)?;
    let columns = change.extent.map(|v| i64::from(v.div_ceil(TILE_SIZE)));
    if change.tiles == [0; 2] && data.tiles.keys().all(|k| (0..2).all(|i| i64::from(k.coordinate[i]) < columns[i])) {
        return Ok(raster.clone());
    }
    Ok(RasterRevision::backed(RasterData {
        tiles: data
            .tiles
            .iter()
            .filter_map(|(key, tile)| {
                let moved: [i64; 2] = std::array::from_fn(|i| i64::from(key.coordinate[i]) + i64::from(change.tiles[i]));
                let inside = (0..2).all(|i| moved[i] >= 0 && (!change.trim || moved[i] < columns[i]));
                inside.then(|| (TileKey { plane: key.plane, coordinate: moved.map(|v| v as u32) }, tile.clone()))
            })
            .collect(),
        watercolor: data.watercolor,
    }))
}

fn invalid_placement() -> CanvasGeometryError {
    CanvasGeometryError::Document(DocumentError::InvalidLayerOperation("Invalid layer placement"))
}

/// The canvas window in a target's local pixels.
fn local_window(scene: SceneView<'_>, target: SourceTarget, canvas: [u32; 2]) -> Result<Rect, CanvasGeometryError> {
    affine_edit_transform(scene, target)
        .and_then(Affine::inverse)
        .map(|map| map.bounds(Rect::from_extent(canvas)))
        .ok_or_else(invalid_placement)
}
fn content_bounds(scene: SceneView<'_>, target: SourceTarget) -> Result<Rect, CanvasGeometryError> {
    let raster = scene.raster(target).ok_or(DocumentError::MissingTarget(target))?;
    let initial = match target {
        SourceTarget::Coverage(h) => scene.coverage(h).and_then(|c| c.initial.as_ref()),
        _ => None,
    };
    let mut bounds = initial.map_or(Rect::EMPTY, |s| s.bounds());
    let size = TILE_SIZE as f32;
    for key in raster_data(raster)?.tiles.keys() {
        let [x, y] = key.coordinate.map(|v| (v * TILE_SIZE) as f32);
        bounds = bounds.union(Rect { min: Point { x, y }, max: Point { x: x + size, y: y + size } });
    }
    if bounds.is_empty() {
        return Ok(bounds);
    }
    Ok(Rect {
        min: Point { x: (bounds.min.x / size).floor() * size, y: (bounds.min.y / size).floor() * size },
        max: Point { x: (bounds.max.x / size).ceil() * size, y: (bounds.max.y / size).ceil() * size },
    })
}

/// Tiles that intersect `bounds` within a target of `extent`.
fn pages(bounds: Rect, extent: [u32; 2]) -> impl Iterator<Item = [u32; 2]> {
    let size = TILE_SIZE as f32;
    let range = |low: f32, high: f32, limit: u32| {
        let start = (low / size).floor().max(0.) as u32;
        let end = ((high / size).ceil().max(0.) as u32).min(limit.div_ceil(TILE_SIZE));
        start..end.max(start)
    };
    let (xs, ys) = if bounds.is_empty() {
        (0..0, 0..0)
    } else {
        (range(bounds.min.x, bounds.max.x, extent[0]), range(bounds.min.y, bounds.max.y, extent[1]))
    };
    ys.flat_map(move |y| xs.clone().map(move |x| [x, y]))
}

/// An operation covering `selection`, in the target's local pixels.
fn pixel_operation(selection: Selection, kind: RasterOperationKind) -> RasterOperation {
    let mut coverage = CoverageSnapshot::reveal_all(CoverageHandle::INVALID, [MAX_EXTENT; 2], Point::default());
    coverage.source.default_coverage = f32::from(selection.inverted);
    coverage.source.initial = Some(selection);
    RasterOperation { placement: Affine::IDENTITY, coverage, kind }
}

fn rectangle(rect: Rect) -> Result<Selection, CanvasGeometryError> {
    Ok(Selection::polygon(rect.corners().to_vec())?)
}

const ERASE: RasterOperationKind = RasterOperationKind::Erase { alpha_locked: false };

/// Erase a paint target's pixels outside `window`, within `extent`, as up to
/// four strips so that only the tiles along the edges are rewritten.
fn erase_outside(window: Rect, extent: [u32; 2], tiles: &RasterData) -> Result<Vec<RasterOperation>, CanvasGeometryError> {
    let [width, height] = extent.map(|v| v as f32);
    let [x0, y0, x1, y1] = [window.min.x.max(0.), window.min.y.max(0.), window.max.x.min(width), window.max.y.min(height)];
    let size = TILE_SIZE as f32;
    let occupied = |strip: Rect| {
        tiles.tiles.keys().any(|key| {
            let [x, y] = key.coordinate.map(|v| (v * TILE_SIZE) as f32);
            x < strip.max.x && x + size > strip.min.x && y < strip.max.y && y + size > strip.min.y
        })
    };
    [[0., 0., width, y0], [0., y1, width, height], [0., y0, x0, y1], [x1, y0, width, y1]]
        .into_iter()
        .map(|[x0, y0, x1, y1]| Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } })
        .filter(|strip| strip.max.x - strip.min.x > PIXEL_TOLERANCE && strip.max.y - strip.min.y > PIXEL_TOLERANCE && occupied(*strip))
        .map(|strip| Ok(pixel_operation(rectangle(strip)?, ERASE)))
        .collect()
}

/// Float noise in placements must not rebase or grow a layer by a pixel.
const PIXEL_TOLERANCE: f32 = 1e-3;

/// Whether `affine` turns the image a quarter turn, so width and height trade places.
fn swaps_axes(affine: Affine) -> bool {
    let [a, b, c, d, ..] = affine.0;
    a.abs() <= 1e-6 && d.abs() <= 1e-6 && b.abs() > 1e-6 && c.abs() > 1e-6
}

fn is_translation(affine: Affine) -> bool {
    let [a, b, c, d, ..] = affine.0;
    [a - 1., b, c, d - 1.].iter().all(|v| v.abs() <= 1e-6)
}

pub(crate) fn geometry_edits(before: &Document, after: &Document) -> Result<Vec<Edit>, DocumentError> {
    let mut edits = Vec::new();
    macro_rules! changed {
        ($store:ident,$variant:ident) => {
            for (h, _, value) in after.artwork.$store.iter() {
                if before.artwork.$store.get(h) != Some(value) {
                    edits.push(Edit::$variant(RecordChange::replace(&before.artwork.$store, h, Some(value.clone()))?));
                }
            }
        };
    }
    changed!(occurrences, Occurrence);
    changed!(paint, Paint);
    changed!(coverage, Coverage);
    changed!(effects, Effect);
    changed!(selections, SavedSelection);
    changed!(guides, Guides);
    Ok(edits)
}
impl Document {
    pub fn check_canvas_geometry(&self, geometry: &CanvasGeometry, limits: GeometryLimits) -> Result<(), CanvasGeometryError> {
        self.canvas_geometry_plan(geometry, limits).map(drop)
    }
    pub fn canvas_geometry_plan(
        &self,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
    ) -> Result<CanvasGeometryPlan, CanvasGeometryError> {
        let rect = geometry.rect;
        if rect.size.contains(&0) {
            return Err(CanvasGeometryError::Empty);
        }
        let dimension = limits.canvas_dimension();
        if rect.size.iter().any(|v| *v > dimension) {
            return Err(CanvasGeometryError::CanvasTooLarge { limit: dimension });
        }
        if rect.origin.iter().any(|v| v.unsigned_abs() > 2 * MAX_EXTENT) {
            return Err(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension });
        }
        let resampled = geometry.linear != Affine::IDENTITY;
        if resampled && geometry.linear.inverse().is_none() {
            return Err(CanvasGeometryError::Unsupported("The canvas can't be mapped this way"));
        }
        let same = !resampled && rect.origin == [0; 2] && rect.size == self.composition().size;
        if same && !geometry.delete_outside {
            return Err(CanvasGeometryError::Unchanged);
        }
        let (mut candidate, operations) =
            if resampled { self.resampled_candidate(geometry, limits)? } else { self.crop_candidate(geometry, limits, None)? };
        let to_canvas = geometry.to_canvas();
        let mut composition = self.composition().clone();
        composition.size = rect.size;
        composition.origin.x += rect.origin[0] as f32;
        composition.origin.y += rect.origin[1] as f32;
        if let Some(resolution) = composition.resolution.filter(|_| swaps_axes(geometry.linear)) {
            composition.resolution = Some(resolution.swapped());
        }
        for (h, _, guides) in self.artwork.guides.iter() {
            if to_canvas != Affine::IDENTITY {
                let mut guides = guides.clone();
                for (_, geometry) in &mut guides.rulers {
                    *geometry = geometry.transformed(to_canvas);
                }
                *candidate.artwork.guides.get_mut(h).unwrap() = guides;
            }
        }
        let mut edits = geometry_edits(self, &candidate)?;
        if same && edits.is_empty() && operations.is_empty() {
            return Err(CanvasGeometryError::Unchanged);
        }
        edits.insert(
            0,
            Edit::Composition(
                RecordChange::replace(&self.artwork.compositions, self.artwork.root, Some(composition)).map_err(DocumentError::from)?,
            ),
        );
        if let Some(selection) = &self.working.selection {
            let moved = selection.transformed(to_canvas)?;
            if moved != *selection {
                let mut working = self.working.clone();
                working.selection = Some(moved);
                edits.push(Edit::Working(working));
            }
        }
        Ok(CanvasGeometryPlan { edits, operations })
    }
    pub fn paint_extent_plan(&self, targets: &[SourceTarget], limits: GeometryLimits) -> Result<Vec<Edit>, CanvasGeometryError> {
        let geometry = CanvasGeometry::crop(CanvasRect { origin: [0; 2], size: self.composition().size });
        let (candidate, _) = self.crop_candidate(&geometry, limits, Some(targets))?;
        Ok(geometry_edits(self, &candidate)?)
    }
    pub fn validate_paint_extents(&self, targets: &[SourceTarget], limits: GeometryLimits) -> Result<(), CanvasGeometryError> {
        self.paint_extent_plan(targets, limits).map(drop)
    }
    pub fn extents_cover_canvas(&self) -> bool {
        let scene = self.scene();
        let canvas = self.composition().size;
        scene.order().iter().copied().all(|h| {
            let source = scene.paint_source(h);
            let paint = scene.source_target(h).filter(|_| source.is_some_and(|p| p.base.is_none()));
            let mask = scene.mask(h).filter(|_| source.is_none_or(|p| p.base.is_none())).map(|(m, _)| SourceTarget::Coverage(m.source));
            paint.into_iter().chain(mask).filter(|t| scene.target_geometry(*t).as_affine().is_some()).all(|t| {
                let extent = scene.target_extent(t);
                local_window(scene, t, canvas).is_ok_and(|w| {
                    w.min.x >= -PIXEL_TOLERANCE
                        && w.min.y >= -PIXEL_TOLERANCE
                        && w.max.x <= extent[0] as f32 + PIXEL_TOLERANCE
                        && w.max.y <= extent[1] as f32 + PIXEL_TOLERANCE
                })
            })
        })
    }
    fn shifted_roots(&self, origin: [i32; 2]) -> Document {
        let mut candidate = self.clone();
        let origin = Point { x: origin[0] as f32, y: origin[1] as f32 };
        for h in self.scene().children(None) {
            let o = candidate.artwork.occurrences.get_mut(*h).unwrap();
            o.translation = shift(o.translation, origin);
            if let Some(mask) = &mut o.mask {
                mask.translation = shift(mask.translation, origin);
            }
        }
        let map = Affine64([1., 0., 0., 1., -origin.x as f64, -origin.y as f64]);
        for (handle, _, application) in self.artwork.effects.iter() {
            if let Some(spatial) = application.spatial {
                candidate.artwork.effects.get_mut(handle).unwrap().spatial.as_mut().unwrap().mapping = map.compose(spatial.mapping);
            }
        }
        candidate
    }
    fn canvas_changes(
        &self,
        candidate: &Document,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
        targets: Option<&[SourceTarget]>,
    ) -> Result<BTreeMap<SourceTarget, LayerChange>, CanvasGeometryError> {
        let scene = candidate.scene();
        let mut changes = BTreeMap::new();
        for h in scene.order() {
            let o = scene.occurrence(*h).unwrap();
            if matches!(o.kind(), LayerKind::Selection) {
                continue;
            }
            let source = scene.paint_source(*h);
            let paint = scene.source_target(*h).filter(|t| matches!(t, SourceTarget::Paint(_)));
            let mask = o.mask.as_ref().map(|m| SourceTarget::Coverage(m.source));
            for target in paint.into_iter().chain(mask) {
                if targets.is_some_and(|ids| {
                    !ids.contains(&target)
                        && !ids
                            .iter()
                            .any(|selected| matches!(selected, SourceTarget::Paint(_)) && scene.source_owner(*selected) == Some(*h))
                }) {
                    continue;
                }
                let extent = scene.target_extent(target);
                if source.is_some_and(|p| p.base.is_some()) || scene.target_geometry(target).as_affine().is_none() {
                    continue;
                }
                let window = local_window(scene, target, geometry.rect.size)?;
                let low = [window.min.x, window.min.y];
                let high = [window.max.x, window.max.y];
                let mut tiles = [0; 2];
                let mut grown = extent;
                let limit = limits.project.dimension as f32;
                let size = TILE_SIZE as f32;
                for axis in 0..2 {
                    let before = (-low[axis] - PIXEL_TOLERANCE).ceil().max(0.);
                    let after = (high[axis] - PIXEL_TOLERANCE).ceil().max(0.);
                    if before > limit || after > 2. * limit {
                        return Err(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension });
                    }
                    if geometry.delete_outside {
                        let first = ((low[axis] + PIXEL_TOLERANCE) / size).floor();
                        let last = ((high[axis] - PIXEL_TOLERANCE) / size).ceil();
                        tiles[axis] = -first as i32;
                        grown[axis] = ((last - first).max(1.) * size) as u32;
                    } else {
                        tiles[axis] = (before as u32).div_ceil(TILE_SIZE) as i32;
                        let delta = tiles[axis] as u32 * TILE_SIZE;
                        grown[axis] = (extent[axis] + delta).max((after as u32).saturating_add(delta));
                    }
                }
                if grown.iter().any(|v| *v > limits.project.dimension) {
                    return Err(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension });
                }
                if targets.is_none() || tiles != [0; 2] || grown != extent {
                    changes.insert(target, LayerChange { tiles, extent: grown, trim: geometry.delete_outside });
                }
            }
        }
        Ok(changes)
    }
    fn crop_candidate(
        &self,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
        targets: Option<&[SourceTarget]>,
    ) -> Result<(Document, Vec<(SourceTarget, RasterOperation)>), CanvasGeometryError> {
        let mut candidate = self.shifted_roots(geometry.rect.origin);
        let changes = self.canvas_changes(&candidate, geometry, limits, targets)?;
        for (target, change) in &changes {
            let owner = self.scene().source_owner(*target).ok_or(DocumentError::MissingTarget(*target))?;
            let delta = Point { x: (change.tiles[0] * TILE_SIZE as i32) as f32, y: (change.tiles[1] * TILE_SIZE as i32) as f32 };
            match *target {
                SourceTarget::Paint(h) => {
                    let p = candidate.artwork.paint.get_mut(h).unwrap();
                    p.raster = rebased_raster(&p.raster, change)?;
                    p.domain = change.extent;
                    if change.tiles != [0; 2] {
                        let o = candidate.artwork.occurrences.get_mut(owner).unwrap();
                        o.translation = shift(o.translation, delta);
                        o.placement = rebased_placement(&o.placement, delta);
                    }
                }
                SourceTarget::Coverage(h) => {
                    let c = candidate.artwork.coverage.get_mut(h).unwrap();
                    c.raster = rebased_raster(&c.raster, change)?;
                    c.domain = change.extent;
                    if change.tiles != [0; 2] {
                        c.initial = c.initial.as_ref().map(|s| s.translated(delta));
                        let mask = candidate.artwork.occurrences.get_mut(owner).unwrap().mask.as_mut().unwrap();
                        mask.translation = shift(mask.translation, delta);
                        mask.placement = rebased_mask(mask.placement, delta);
                    }
                }
                _ => {}
            }
        }
        candidate.check_raster_limits(&BTreeMap::new(), limits.project)?;
        let mut operations = Vec::new();
        for (target, change) in changes.iter().filter(|(t, c)| c.trim && matches!(t, SourceTarget::Paint(_))) {
            let tiles = raster_data(candidate.target_raster(*target).ok_or(DocumentError::MissingTarget(*target))?)?;
            if tiles.tiles.is_empty() {
                continue;
            }
            let transform = candidate.affine_edit_transform(*target).ok_or_else(invalid_placement)?;
            let erase = if is_translation(transform) {
                erase_outside(local_window(candidate.scene(), *target, geometry.rect.size)?, change.extent, &tiles)?
            } else {
                let inverse = transform.inverse().ok_or_else(invalid_placement)?;
                let mut outside = Selection::polygon(Rect::from_extent(geometry.rect.size).corners().map(|p| inverse.map(p)).to_vec())?;
                outside.inverted = true;
                vec![pixel_operation(outside, ERASE)]
            };
            operations.extend(erase.into_iter().map(|op| (*target, op)));
        }
        for (_, op) in &mut operations {
            op.coverage.target = self.artwork.coverage.next_handle();
            op.coverage.use_.source = op.coverage.target;
        }
        Ok((candidate, operations))
    }
    fn resampled_candidate(
        &self,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
    ) -> Result<(Document, Vec<(SourceTarget, RasterOperation)>), CanvasGeometryError> {
        let mut candidate = self.shifted_roots(geometry.rect.origin);
        let scene = self.scene();
        let to_canvas = geometry.to_canvas();
        let mut operations = Vec::new();
        let mut predicted = BTreeMap::new();
        for h in scene.order() {
            let old = scene.occurrence(*h).unwrap();
            let world = candidate.layer_offset(*h);
            let parents = shift(world, candidate.scene().occurrence(*h).unwrap().translation);
            if let OccurrenceContent::Selection(s) = old.content {
                let target = SourceTarget::Selection(s);
                let after = candidate.affine_edit_transform(target).and_then(Affine::inverse).ok_or_else(invalid_placement)?;
                let map = self.affine_edit_transform(target).ok_or_else(invalid_placement)?.then(to_canvas).then(after);
                let selection = candidate.artwork.selections.get_mut(s).unwrap();
                selection.selection = selection.selection.transformed(map)?;
                continue;
            }
            if scene.paint_source(*h).is_some_and(|p| p.base.is_some()) || old.placement.as_affine().is_none() {
                let desired = self.retained_transform_edit(&[*h], Projective::from_affine(to_canvas))?;
                let Edit::Batch(edits) = desired else { unreachable!() };
                for edit in edits {
                    if let Edit::Occurrence(change) = edit {
                        if change.handle != *h {
                            continue;
                        }
                        let mut replacement = change.value.unwrap();
                        let original_world = self.layer_offset(*h);
                        let delta = shift(original_world, world);
                        replacement.placement = replacement
                            .placement
                            .post(Projective::from_affine(Affine::translation(delta)))
                            .ok_or_else(invalid_placement)?;
                        replacement.translation = candidate.scene().occurrence(*h).unwrap().translation;
                        if let Some(mask) = replacement.mask.as_mut() {
                            if mask.linked {
                                mask.translation.x += replacement.translation.x - old.translation.x;
                                mask.translation.y += replacement.translation.y - old.translation.y;
                            } else {
                                let new_parent = shift(world, replacement.translation);
                                mask.placement = self.target_geometry(SourceTarget::Coverage(mask.source)).projective()
                                    .and_then(|map| map.then(Projective::from_affine(to_canvas)))
                                    .and_then(|map| map.then(Projective::from_affine(Affine::translation(Point {
                                        x: -mask.translation.x - new_parent.x, y: -mask.translation.y - new_parent.y,
                                    }))))
                                    .ok_or_else(invalid_placement)?;
                            }
                        }
                        *candidate.artwork.occurrences.get_mut(*h).unwrap() = replacement;
                    }
                }
                continue;
            }
            let paint = scene.source_target(*h).filter(|t| matches!(t, SourceTarget::Paint(_)));
            let targets: Vec<_> = paint.into_iter().chain(old.mask.as_ref().map(|m| SourceTarget::Coverage(m.source))).collect();
            if targets.is_empty() {
                continue;
            }
            let extent = scene.local_extent(*h);
            let mut rotated = Rect::EMPTY;
            for target in &targets {
                rotated = rotated.union(
                    scene
                        .target_geometry(*target)
                        .placement
                        .post(Projective::from_affine(to_canvas))
                        .map_or(Rect::UNBOUNDED, |p| p.forward_bounds(Rect::from_extent(scene.target_extent(*target)))),
                );
            }
            let frame = if geometry.delete_outside {
                [0.; 2]
            } else {
                [rotated.min.x, rotated.min.y].map(|v| (v + PIXEL_TOLERANCE).floor().min(0.))
            };
            let far = [rotated.max.x, rotated.max.y];
            let grown: [u32; 2] = std::array::from_fn(|i| {
                let size = geometry.rect.size[i] as f32;
                let reach = if geometry.delete_outside { size } else { far[i].max(size) };
                ((reach - frame[i] - PIXEL_TOLERANCE).ceil().max(0.) as u32).max(extent[i])
            });
            if grown.iter().any(|v| *v > limits.project.dimension) {
                return Err(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension });
            }
            let world = Point { x: frame[0], y: frame[1] };
            let o = candidate.artwork.occurrences.get_mut(*h).unwrap();
            if paint.is_some() {
                o.translation = shift(world, parents);
                o.placement = LayerPlacement::IDENTITY;
            }
            if let Some(mask) = o.mask.as_mut() {
                mask.translation = shift(world, parents);
                mask.placement = Projective::IDENTITY;
            }
            for target in targets {
                match target {
                    SourceTarget::Paint(h) => candidate.artwork.paint.get_mut(h).unwrap().domain = grown,
                    SourceTarget::Coverage(h) => candidate.artwork.coverage.get_mut(h).unwrap().domain = grown,
                    _ => {}
                }
                let mut transform = scene.target_geometry(target);
                transform.placement = transform
                    .placement
                    .post(Projective::from_affine(to_canvas.then(Affine::translation(shift(Point::default(), world)))))
                    .ok_or_else(invalid_placement)?;
                transform.placement.interpolation = geometry.interpolation;
                let content = content_bounds(scene, target)?;
                let existing = raster_data(scene.raster(target).ok_or(DocumentError::MissingTarget(target))?)?;
                let written: BTreeSet<_> = if content.is_empty() {
                    existing.tiles.keys().map(|k| k.coordinate).collect()
                } else {
                    pages(transform.affected_regions(content)[1], grown).collect()
                };
                predicted.insert(target, written.len());
                if !content.is_empty() {
                    operations.push((target, pixel_operation(rectangle(content)?, RasterOperationKind::Transform(transform))));
                }
                if geometry.delete_outside && paint == Some(target) && !content.is_empty() {
                    let size = geometry.rect.size.map(|v| v as f32);
                    let [width, height] = grown.map(|v| v as f32);
                    for [x0, y0, x1, y1] in [[size[0], 0., width, height], [0., size[1], size[0], height]] {
                        if x1 - x0 > PIXEL_TOLERANCE && y1 - y0 > PIXEL_TOLERANCE {
                            operations.push((
                                target,
                                pixel_operation(rectangle(Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } })?, ERASE),
                            ));
                        }
                    }
                }
            }
        }
        let map = Affine64(geometry.to_canvas().0.map(f64::from));
        for (handle, _, application) in self.artwork.effects.iter() {
            if let Some(spatial) = application.spatial {
                candidate.artwork.effects.get_mut(handle).unwrap().spatial.as_mut().unwrap().mapping = map.compose(spatial.mapping);
            }
        }
        candidate.check_raster_limits(&predicted, limits.project)?;
        for (_, op) in &mut operations {
            op.coverage.target = self.artwork.coverage.next_handle();
            op.coverage.use_.source = op.coverage.target;
        }
        Ok((candidate, operations))
    }
    fn check_raster_limits(&self, predicted: &BTreeMap<SourceTarget, usize>, limits: ProjectLimits) -> Result<(), CanvasGeometryError> {
        let mut tiles = 0usize;
        let mut bytes = 0u64;
        let mut sources = BTreeSet::new();
        let color = self.composition().color;
        for (_, _, p) in self.artwork.paint.iter() {
            if let Some(source) = p.base.as_ref().map(|b|b.image.storage()).filter(|s| sources.insert(Arc::as_ptr(s) as usize)) {
                tiles = tiles.saturating_add(source.tiles.len());
            }
        }
        for target in self.scene().targets().filter(|t| !matches!(t, SourceTarget::Selection(_))) {
            if let Some(count) = predicted.get(&target) {
                let plane = if matches!(target, SourceTarget::Coverage(_)) { RasterPlane::Mask } else { RasterPlane::Color };
                let page = plane.descriptor(color).byte_len([TILE_SIZE; 2]).unwrap_or(0) as u64;
                tiles = tiles.saturating_add(*count);
                bytes = bytes.saturating_add(page.saturating_mul(*count as u64));
            } else if let Some(Ok(data)) = self.target_raster(target).and_then(|r| r.try_data()) {
                tiles = tiles.saturating_add(data.tiles.len());
                for tile in data.tiles.values() {
                    bytes = bytes.saturating_add(tile.descriptor().byte_len([TILE_SIZE; 2]).unwrap_or(0) as u64);
                }
            }
        }
        if tiles > limits.tiles {
            return Err(CanvasGeometryError::TooManyTiles { limit: limits.tiles });
        }
        if bytes > limits.raster_bytes {
            return Err(CanvasGeometryError::RasterTooLarge);
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "canvas_geometry_tests.rs"]
mod tests;
