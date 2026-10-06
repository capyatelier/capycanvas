//! Canvas geometry: one plan turns a new canvas rectangle, and optionally a
//! rotation, flip or scale, into a single reversible batch. The canvas is a window over each
//! layer's local extent, so shrinking it hides pixels and growing it shows
//! them again.
//!
//! Every geometry edit keeps these invariants:
//! - Root offsets carry the canvas origin. Child offsets are relative to their
//!   group and change only when a resample moves the child.
//! - Every paint layer, with or without a base image, and every mask has a
//!   local extent that covers the canvas window, so the whole canvas stays
//!   paintable. Growing the canvas left or up rebases such a layer by whole
//!   tiles: its tiles are re-keyed and still shared, its base moves with them,
//!   and its offset moves the other way.
//! - Stored extents shrink only when Delete Cropped Paint Pixels trims a layer
//!   to the tiles its window touches, and cuts its base image to the kept
//!   samples. Hidden tiles count toward the project's tile and byte limits.
//! - A flip or turn moves a base image and its layer's tiles exactly, sample
//!   for sample, in the base's own interpretation. Any other rotation or scale
//!   resamples paint layers, folding a base into the layer's paint unless an
//!   untouched photo keeps its own interpretation, and masks
//!   into a frame that holds the whole mapped extent, so hidden corners are
//!   kept. Image objects and effect references take the map in double
//!   precision. Effect distances in pixels scale with the image.
use crate::authored::*;
use crate::raster::{RasterData, RasterPlane, RasterRevision, TILE_SIZE, TileKey};
use crate::sample_remap::{GridMap, remap_image, remap_raster};
use std::sync::atomic::AtomicBool;
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
    pub linear: Affine64,
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
        Self { rect, linear: Affine64::default(), interpolation: Interpolation::default(), delete_outside: false }
    }
    /// Flip or turn a `canvas` of that size. Pixels move without resampling.
    pub fn orient(canvas: [u32; 2], orientation: ImageOrientation) -> Self {
        let [w, h] = canvas.map(f64::from);
        let turned = [canvas[1], canvas[0]];
        let (linear, size) = match orientation {
            ImageOrientation::FlipHorizontal => (Affine64([-1., 0., 0., 1., w, 0.]), canvas),
            ImageOrientation::FlipVertical => (Affine64([1., 0., 0., -1., 0., h]), canvas),
            ImageOrientation::Rotate180 => (Affine64([-1., 0., 0., -1., w, h]), canvas),
            ImageOrientation::RotateRight => (Affine64([0., 1., -1., 0., h, 0.]), turned),
            ImageOrientation::RotateLeft => (Affine64([0., -1., 1., 0., 0., w]), turned),
        };
        Self { rect: CanvasRect { origin: [0; 2], size }, linear, interpolation: Interpolation::Nearest, delete_outside: false }
    }
    /// Scale a `canvas` of that size to `size`.
    pub fn resize(canvas: [u32; 2], size: [u32; 2], interpolation: Interpolation) -> Self {
        let [x, y] = std::array::from_fn(|i| f64::from(size[i]) / f64::from(canvas[i].max(1)));
        Self { rect: CanvasRect { origin: [0; 2], size }, linear: Affine64([x, 0., 0., y, 0., 0.]), interpolation, delete_outside: false }
    }
    /// A rotation by `radians` about `center`.
    pub fn rotation(center: [f64; 2], radians: f64) -> Affine64 {
        let (s, c) = radians.sin_cos();
        Affine64([1., 0., 0., 1., center[0], center[1]]).compose(Affine64([c, s, -s, c, 0., 0.])).compose(Affine64([1., 0., 0., 1., -center[0], -center[1]]))
    }
    /// Where a current document point lands on the new canvas.
    pub fn to_canvas64(&self) -> Affine64 {
        let [x, y] = self.rect.origin.map(|v| -f64::from(v));
        Affine64([1., 0., 0., 1., x, y]).compose(self.linear)
    }
    pub fn to_canvas(&self) -> Affine {
        Affine(self.to_canvas64().0.map(|v| v as f32))
    }
    /// The document pixel each new canvas pixel reads, when the map moves
    /// whole pixels without resampling.
    pub fn pixel_map(&self) -> Option<GridMap> {
        let [a, b, c, d, x, y] = self.to_canvas64().0;
        let integral = |v: f64| (v.fract() == 0. && v.abs() < 1e15).then_some(v as i64);
        let [a, b, c, d, x, y] = [integral(a)?, integral(b)?, integral(c)?, integral(d)?, integral(x)?, integral(y)?];
        let rows = [[a, c], [b, d]];
        let t = [x + (a + c).div_euclid(2), y + (b + d).div_euclid(2)];
        let map = GridMap { x_axis: [rows[0][0], rows[0][1]], y_axis: [rows[1][0], rows[1][1]], origin: [0; 2] };
        if !map.is_permutation() { return None; }
        let origin = [-(map.x_axis[0] * t[0] + map.y_axis[0] * t[1]), -(map.x_axis[1] * t[0] + map.y_axis[1] * t[1])];
        Some(GridMap { origin, ..map })
    }
}

/// One reversible batch and the pixel operations that run inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasGeometryPlan {
    /// The canvas size, then every changed layer, the selection and guides.
    pub edits: Vec<Edit>,
    /// Resampling for a rotation and erasing for Delete Cropped Paint Pixels,
    /// applied after `edits` in the same undo step. Coverage ids are placeholders
    /// the caller allocates.
    pub operations: Vec<(SourceTarget, RasterOperation)>,
    /// Exact permutations, crops and retained resamples of paint layers with a
    /// base image; `with_remapped` installs their results.
    pub remaps: Vec<RemapSpec>,
}

/// One paint layer's or mask's sample move: a paint layer's base image, then
/// the target's tiles.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemapSpec {
    pub target: SourceTarget,
    pub base: Option<BaseRemap>,
    pub extent: [u32; 2],
    pub map: GridMap,
    pub keep: [[i64; 2]; 2],
}
/// How a base image moves into the replacement image of `extent`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BaseRemap {
    /// Exactly, sample for sample.
    Exact { extent: [u32; 2], map: GridMap },
    /// Resampled through `to_image`, from base pixels to replacement pixels,
    /// keeping the base's own interpretation.
    Resample { extent: [u32; 2], to_image: Affine64, interpolation: Interpolation },
}
pub type RemapResult = (SourceTarget, Option<Image>, RasterRevision);

/// The sample moves a canvas change needs, read from `scene`.
#[derive(Clone, Debug)]
pub struct RemapPlan {
    pub scene: Arc<SceneSnapshot>,
    pub specs: Arc<[RemapSpec]>,
}
impl RemapPlan {
    pub fn targets(&self) -> impl Iterator<Item = SourceTarget> + '_ {
        self.specs.iter().map(|spec| spec.target)
    }
    /// The base images to resample, with their replacement extent, map and filter.
    pub fn resamples(&self) -> impl Iterator<Item = (SourceTarget, &Image, [u32; 2], Affine64, Interpolation)> + '_ {
        self.specs.iter().filter_map(|spec| match spec.base {
            Some(BaseRemap::Resample { extent, to_image, interpolation }) =>
                Some((spec.target, &self.scene.view().paint_base(spec.target)?.image, extent, to_image, interpolation)),
            _ => None,
        })
    }
    /// Run every move against the paint records of `artwork`, taking resampled
    /// base images from `resampled`.
    pub fn run(&self, artwork: &Artwork, resampled: &BTreeMap<SourceTarget, Image>, cancelled: &AtomicBool) -> Result<Vec<RemapResult>, String> {
        let budget = ProjectLimits::default().asset_bytes as usize;
        self.specs.iter().map(|spec| {
            let changed = || "A layer changed while it was being moved".to_string();
            let (raster, base) = match spec.target {
                SourceTarget::Paint(h) => artwork.paint.get(h).map(|p| (&p.raster, p.base.as_ref())).ok_or_else(changed)?,
                SourceTarget::Coverage(h) => artwork.coverage.get(h).map(|c| (&c.raster, None)).ok_or_else(changed)?,
                SourceTarget::Selection(_) => return Err(changed()),
            };
            let image = match (spec.base, base) {
                (Some(BaseRemap::Exact { extent, map }), Some(base)) => Some(Image::new(Arc::new(remap_image(base.image.storage(), extent, map, budget, cancelled)?))),
                (Some(BaseRemap::Resample { extent, .. }), Some(_)) => Some(resampled.get(&spec.target).filter(|image| image.extent == extent).cloned()
                    .ok_or("The resampled image is missing")?),
                (Some(_), None) => return Err(changed()),
                (None, _) => None,
            };
            let data = raster.wait_data_cancellable(cancelled)?;
            let raster = RasterRevision::backed(remap_raster(&data, spec.map, spec.extent, spec.keep, cancelled)?);
            Ok((spec.target, image, raster))
        }).collect()
    }
}

impl CanvasGeometryPlan {
    pub fn remap_plan(&self, document: &Document) -> Option<RemapPlan> {
        (!self.remaps.is_empty()).then(|| RemapPlan { scene: document.snapshot(), specs: self.remaps.clone().into() })
    }
    /// Install worker results for every remap, completing the plan.
    pub fn with_remapped(mut self, results: Vec<RemapResult>) -> Result<Self, String> {
        fn install(edit: &mut Edit, results: &mut BTreeMap<SourceTarget, (Option<Image>, RasterRevision)>) -> Result<(), String> {
            match edit {
                Edit::Batch(edits) => edits.iter_mut().try_for_each(|edit| install(edit, results)),
                Edit::Paint(change) => {
                    let Some((image, raster)) = results.remove(&SourceTarget::Paint(change.handle)) else { return Ok(()) };
                    let paint = change.value.as_mut().ok_or("A moved layer was removed")?;
                    if let Some(image) = image {
                        paint.base.as_mut().ok_or("A moved layer lost its image")?.image = image;
                    }
                    paint.raster = raster;
                    Ok(())
                }
                Edit::Coverage(change) => {
                    let Some((_, raster)) = results.remove(&SourceTarget::Coverage(change.handle)) else { return Ok(()) };
                    change.value.as_mut().ok_or("A moved mask was removed")?.raster = raster;
                    Ok(())
                }
                _ => Ok(()),
            }
        }
        let mut results: BTreeMap<_, _> = results.into_iter().map(|(target, image, raster)| (target, (image, raster))).collect();
        if results.len() != self.remaps.len() || self.remaps.iter().any(|spec| !results.contains_key(&spec.target)) {
            return Err("The moved layers do not match the canvas change".into());
        }
        for edit in &mut self.edits {
            install(edit, &mut results)?;
        }
        if !results.is_empty() {
            return Err("The moved layers do not match the canvas change".into());
        }
        self.remaps.clear();
        Ok(self)
    }
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

fn out_of_range() -> CanvasGeometryError {
    CanvasGeometryError::Document(DocumentError::InvalidLayerOperation("A layer offset exceeds the editor's range"))
}

fn invalid_placement() -> CanvasGeometryError {
    CanvasGeometryError::Document(DocumentError::InvalidLayerOperation("Invalid layer placement"))
}

/// The canvas window in a target's local pixels: its first pixel and the
/// one past its last.
fn local_window(scene: SceneView<'_>, target: SourceTarget, canvas: [u32; 2]) -> Result<[[i64; 2]; 2], CanvasGeometryError> {
    let offset = scene.target_offset(target);
    let low = offsets::checked_sub([0; 2], offset).ok_or_else(out_of_range)?;
    Ok([low, offsets::checked_add(low, canvas.map(i64::from)).ok_or_else(out_of_range)?])
}
fn content_bounds(scene: SceneView<'_>, target: SourceTarget) -> Result<Rect, CanvasGeometryError> {
    let raster = scene.raster(target).ok_or(DocumentError::MissingTarget(target))?;
    let mut bounds = Rect::EMPTY;
    if let Some(base) = scene.paint_base(target) {
        let [x, y] = base.offset.map(|v| v as f32);
        bounds = bounds.union(Rect { min: Point { x, y }, max: Point { x: x + base.image.extent[0] as f32, y: y + base.image.extent[1] as f32 } });
    }
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
    let mut coverage = CoverageSnapshot::reveal_all(CoverageHandle::INVALID, [MAX_EXTENT; 2], [0; 2]);
    coverage.source.default_coverage = f32::from(selection.inverted);
    coverage.selection = Some(selection);
    RasterOperation { placement: Affine::IDENTITY, coverage, kind }
}

fn rectangle(rect: Rect) -> Result<Selection, CanvasGeometryError> {
    Ok(Selection::polygon(rect.corners().to_vec())?)
}

const ERASE: RasterOperationKind = RasterOperationKind::Erase { alpha_locked: false };

/// Erase a paint target's pixels outside `window`, within `extent`, as up to
/// four strips so that only the tiles along the edges are rewritten.
fn erase_outside([low, high]: [[i64; 2]; 2], extent: [u32; 2], tiles: &RasterData) -> Result<Vec<RasterOperation>, CanvasGeometryError> {
    let [width, height] = extent.map(i64::from);
    let [x0, y0] = [low[0].clamp(0, width), low[1].clamp(0, height)];
    let [x1, y1] = [high[0].clamp(x0, width), high[1].clamp(y0, height)];
    let size = i64::from(TILE_SIZE);
    let occupied = |[x0, y0, x1, y1]: [i64; 4]| {
        tiles.tiles.keys().any(|key| {
            let [x, y] = key.coordinate.map(|v| i64::from(v) * size);
            x < x1 && x + size > x0 && y < y1 && y + size > y0
        })
    };
    [[0, 0, width, y0], [0, y1, width, height], [0, y0, x0, y1], [x1, y0, width, y1]]
        .into_iter()
        .filter(|&[x0, y0, x1, y1]| x1 > x0 && y1 > y0 && occupied([x0, y0, x1, y1]))
        .map(|[x0, y0, x1, y1]| Ok(pixel_operation(rectangle(Rect { min: Point { x: x0 as f32, y: y0 as f32 }, max: Point { x: x1 as f32, y: y1 as f32 } })?, ERASE)))
        .collect()
}

/// Float noise in placements must not rebase or grow a layer by a pixel.
const PIXEL_TOLERANCE: f32 = 1e-3;

/// Whether `affine` turns the image a quarter turn, so width and height trade places.
fn swaps_axes(affine: Affine) -> bool {
    let [a, b, c, d, ..] = affine.0;
    a.abs() <= 1e-6 && d.abs() <= 1e-6 && b.abs() > 1e-6 && c.abs() > 1e-6
}

/// Where a target of `domain` at document `origin` lands under the exact
/// document map `doc`: its tile-padded domain turns as a whole and grows to
/// cover the `canvas`, so every tile moves to one tile. Returns the new
/// document origin, the new domain and the map from new to old local pixels.
fn turned_layout(origin: [i64; 2], domain: [u32; 2], doc: GridMap, canvas: [u32; 2], limits: GeometryLimits) -> Result<([i64; 2], [u32; 2], GridMap), CanvasGeometryError> {
    let size = i64::from(TILE_SIZE);
    let padded = domain.map(|v| i64::from(v.div_ceil(TILE_SIZE)) * size);
    let [turned, reach] = doc.result_rect(origin, offsets::checked_add(origin, padded).ok_or_else(out_of_range)?);
    let low: [i64; 2] = std::array::from_fn(|i| turned[i] - (turned[i].max(0) + size - 1).div_euclid(size) * size);
    let high: [i64; 2] = std::array::from_fn(|i| reach[i].max(i64::from(canvas[i])));
    let extent: [u32; 2] = std::array::from_fn(|i| ((high[i] - low[i] + size - 1).div_euclid(size) * size).min(i64::from(u32::MAX)) as u32);
    if extent.iter().any(|v| *v > limits.project.dimension) {
        return Err(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension });
    }
    Ok((low, extent, GridMap { origin: offsets::checked_sub(doc.source(low), origin).ok_or_else(out_of_range)?, ..doc }))
}

/// Delete Cropped Paint Pixels on a layer with a base: cut the base down to
/// the kept `window` of local pixels and clear its tiles outside it.
fn recut(paint: &mut PaintSource, handle: PaintHandle, change: &LayerChange, window: [[i64; 2]; 2], delta: [i64; 2]) -> Result<Option<RemapSpec>, CanvasGeometryError> {
    let keep = [offsets::checked_add(window[0], delta).ok_or_else(out_of_range)?, offsets::checked_add(window[1], delta).ok_or_else(out_of_range)?];
    let mut base_map = None;
    if let Some(base) = paint.base.as_mut() {
        let low = base.offset.map(i64::from);
        let high = std::array::from_fn(|i| low[i] + i64::from(base.image.extent[i]));
        let kept = [std::array::from_fn(|i| low[i].max(window[0][i])), std::array::from_fn::<i64, 2, _>(|i| high[i].min(window[1][i]))];
        if (0..2).any(|i| kept[0][i] >= kept[1][i]) {
            paint.base = None;
        } else {
            let offset = offsets::checked_add(kept[0], delta).ok_or_else(out_of_range)?;
            base.offset = std::array::from_fn(|i| u32::try_from(offset[i]).unwrap_or(u32::MAX));
            if kept != [low, high] {
                base_map = Some(BaseRemap::Exact { extent: std::array::from_fn(|i| (kept[1][i] - kept[0][i]) as u32), map: GridMap::translation(offsets::checked_sub(kept[0], low).ok_or_else(out_of_range)?) });
            }
        }
    }
    let data = raster_data(&paint.raster)?;
    let size = i64::from(TILE_SIZE);
    let trims = data.tiles.keys().any(|key| (0..2).any(|i| {
        let start = i64::from(key.coordinate[i]) * size + delta[i];
        start < keep[0][i] || start + size > keep[1][i]
    }));
    if base_map.is_none() && !trims {
        paint.raster = rebased_raster(&paint.raster, change)?;
        return Ok(None);
    }
    Ok(Some(RemapSpec { target: SourceTarget::Paint(handle), base: base_map, extent: change.extent, map: GridMap::translation(delta.map(|v| -v)), keep }))
}

/// Record edits for every target a remap replaces, even when its record
/// changes only once the worker's results are installed.
fn remap_edits(before: &Document, after: &Document, remaps: &[RemapSpec], edits: &mut Vec<Edit>) -> Result<(), DocumentError> {
    for spec in remaps {
        let present = edits.iter().any(|edit| matches!((edit, spec.target),
            (Edit::Paint(change), SourceTarget::Paint(h)) if change.handle == h) || matches!((edit, spec.target), (Edit::Coverage(change), SourceTarget::Coverage(h)) if change.handle == h));
        match spec.target {
            SourceTarget::Paint(h) if !present => edits.push(Edit::Paint(RecordChange::replace(&before.artwork.paint, h, after.artwork.paint.get(h).cloned())?)),
            SourceTarget::Coverage(h) if !present => edits.push(Edit::Coverage(RecordChange::replace(&before.artwork.coverage, h, after.artwork.coverage.get(h).cloned())?)),
            _ => {}
        }
    }
    Ok(())
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
    changed!(objects, ImageObject);
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
        let resampled = geometry.linear != Affine64::default();
        if resampled && geometry.linear.inverse().is_none() {
            return Err(CanvasGeometryError::Unsupported("The canvas can't be mapped this way"));
        }
        let same = !resampled && rect.origin == [0; 2] && rect.size == self.composition().size;
        if same && !geometry.delete_outside {
            return Err(CanvasGeometryError::Unchanged);
        }
        let (mut candidate, operations, remaps) =
            if resampled { self.resampled_candidate(geometry, limits)? } else { self.crop_candidate(geometry, limits, None)? };
        let to_canvas = geometry.to_canvas();
        let mut composition = self.composition().clone();
        composition.size = rect.size;
        if let Some(resolution) = composition.resolution.filter(|_| swaps_axes(geometry.to_canvas())) {
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
        remap_edits(self, &candidate, &remaps, &mut edits)?;
        if same && edits.is_empty() && operations.is_empty() && remaps.is_empty() {
            return Err(CanvasGeometryError::Unchanged);
        }
        edits.insert(
            0,
            Edit::Composition(
                RecordChange::replace(&self.artwork.compositions, self.artwork.root, Some(composition)).map_err(DocumentError::from)?,
            ),
        );
        let mut working = self.working.clone();
        working.view_origin = offsets::checked_add(working.view_origin, rect.origin.map(i64::from))
            .filter(|origin| offsets::admitted(*origin)).ok_or(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension })?;
        working.selection = working.selection.as_ref().map(|selection| selection.transformed(to_canvas)).transpose()?;
        if working != self.working {
            edits.push(Edit::Working(working));
        }
        Ok(CanvasGeometryPlan { edits, operations, remaps })
    }
    pub fn paint_extent_plan(&self, targets: &[SourceTarget], limits: GeometryLimits) -> Result<Vec<Edit>, CanvasGeometryError> {
        let geometry = CanvasGeometry::crop(CanvasRect { origin: [0; 2], size: self.composition().size });
        let (candidate, ..) = self.crop_candidate(&geometry, limits, Some(targets))?;
        Ok(geometry_edits(self, &candidate)?)
    }
    pub fn validate_paint_extents(&self, targets: &[SourceTarget], limits: GeometryLimits) -> Result<(), CanvasGeometryError> {
        self.paint_extent_plan(targets, limits).map(drop)
    }
    pub fn extents_cover_canvas(&self) -> bool {
        let scene = self.scene();
        let canvas = self.composition().size;
        scene.order().iter().copied().all(|h| {
            let paint = scene.source_target(h).filter(|t| matches!(t, SourceTarget::Paint(_)));
            let mask = scene.mask(h).map(|(m, _)| SourceTarget::Coverage(m.source));
            paint.into_iter().chain(mask).all(|t| {
                let extent = scene.target_extent(t);
                local_window(scene, t, canvas).is_ok_and(|[low, high]| (0..2).all(|i| low[i] >= 0 && high[i] <= i64::from(extent[i])))
            })
        })
    }
    fn shifted_roots(&self, origin: [i32; 2]) -> Result<Document, CanvasGeometryError> {
        let mut candidate = self.clone();
        let shift = origin.map(|v| -i64::from(v));
        for h in self.scene().children(None) {
            let o = candidate.artwork.occurrences.get_mut(*h).unwrap();
            *o = o.shifted(shift).map_err(CanvasGeometryError::Document)?;
            if let OccurrenceContent::Selection(s) = o.content {
                let saved = candidate.artwork.selections.get_mut(s).unwrap();
                saved.selection = saved.selection.translated(offsets::point(shift));
            }
        }
        let map = Affine64([1., 0., 0., 1., -f64::from(origin[0]), -f64::from(origin[1])]);
        for (handle, _, application) in self.artwork.effects.iter() {
            if let Some(spatial) = application.spatial {
                candidate.artwork.effects.get_mut(handle).unwrap().spatial.as_mut().unwrap().mapping = map.compose(spatial.mapping);
            }
        }
        Ok(candidate)
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
                let [low, high] = local_window(scene, target, geometry.rect.size)?;
                let mut tiles = [0; 2];
                let mut grown = extent;
                let limit = i64::from(limits.project.dimension);
                let size = i64::from(TILE_SIZE);
                let too_large = || CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension };
                for axis in 0..2 {
                    let before = (-low[axis]).max(0);
                    let after = high[axis].max(0);
                    if before > limit || after > 2 * limit {
                        return Err(too_large());
                    }
                    if geometry.delete_outside {
                        let first = low[axis].div_euclid(size);
                        let last = high[axis].div_euclid(size) + i64::from(high[axis].rem_euclid(size) != 0);
                        tiles[axis] = i32::try_from(-first).map_err(|_| too_large())?;
                        grown[axis] = u32::try_from((last - first).max(1) * size).map_err(|_| too_large())?;
                    } else {
                        tiles[axis] = i32::try_from((before + size - 1) / size).map_err(|_| too_large())?;
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
    ) -> Result<(Document, Vec<(SourceTarget, RasterOperation)>, Vec<RemapSpec>), CanvasGeometryError> {
        let mut candidate = self.shifted_roots(geometry.rect.origin)?;
        let changes = self.canvas_changes(&candidate, geometry, limits, targets)?;
        let mut remaps = Vec::new();
        for (target, change) in &changes {
            let owner = self.scene().source_owner(*target).ok_or(DocumentError::MissingTarget(*target))?;
            let delta = change.tiles.map(|v| i64::from(v) * i64::from(TILE_SIZE));
            let window = local_window(candidate.scene(), *target, geometry.rect.size)?;
            match *target {
                SourceTarget::Paint(h) => {
                    let p = candidate.artwork.paint.get_mut(h).unwrap();
                    if change.trim && p.base.is_some() {
                        remaps.extend(recut(p, h, change, window, delta)?);
                    } else {
                        p.raster = rebased_raster(&p.raster, change)?;
                        if let Some(base) = p.base.as_mut() {
                            base.offset = std::array::from_fn(|i| u32::try_from(i64::from(base.offset[i]) + delta[i]).unwrap_or(u32::MAX));
                        }
                    }
                    p.domain = change.extent;
                    if change.tiles != [0; 2] {
                        let o = candidate.artwork.occurrences.get_mut(owner).unwrap();
                        o.offset = offsets::checked_sub(o.offset, delta).ok_or_else(out_of_range)?;
                        if let Some(mask) = o.mask.as_mut().filter(|m| m.linked) {
                            mask.offset = offsets::checked_add(mask.offset, delta).ok_or_else(out_of_range)?;
                        }
                    }
                }
                SourceTarget::Coverage(h) => {
                    let c = candidate.artwork.coverage.get_mut(h).unwrap();
                    c.raster = rebased_raster(&c.raster, change)?;
                    c.domain = change.extent;
                    if change.tiles != [0; 2] {
                        let mask = candidate.artwork.occurrences.get_mut(owner).unwrap().mask.as_mut().unwrap();
                        mask.offset = offsets::checked_sub(mask.offset, delta).ok_or_else(out_of_range)?;
                    }
                }
                _ => {}
            }
        }
        candidate.check_raster_limits(&BTreeMap::new(), limits.project)?;
        let mut operations = Vec::new();
        for (target, change) in changes.iter().filter(|(t, c)| c.trim && matches!(t, SourceTarget::Paint(_)) && self.scene().paint_base(**t).is_none()) {
            let tiles = raster_data(candidate.target_raster(*target).ok_or(DocumentError::MissingTarget(*target))?)?;
            if tiles.tiles.is_empty() {
                continue;
            }
            let erase = erase_outside(local_window(candidate.scene(), *target, geometry.rect.size)?, change.extent, &tiles)?;
            operations.extend(erase.into_iter().map(|op| (*target, op)));
        }
        for (_, op) in &mut operations {
            op.coverage.target = self.artwork.coverage.next_handle();
            op.coverage.use_.source = op.coverage.target;
        }
        Ok((candidate, operations, remaps))
    }
    fn resampled_candidate(
        &self,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
    ) -> Result<(Document, Vec<(SourceTarget, RasterOperation)>, Vec<RemapSpec>), CanvasGeometryError> {
        let mut candidate = self.shifted_roots(geometry.rect.origin)?;
        let scene = self.scene();
        let to_canvas = geometry.to_canvas();
        let to_canvas64 = geometry.to_canvas64();
        let pixel_map = geometry.pixel_map();
        let translation = |offset: [i64; 2]| Affine64([1., 0., 0., 1., offset[0] as f64, offset[1] as f64]);
        let mut operations = Vec::new();
        let mut remaps = Vec::new();
        let mut predicted = BTreeMap::new();
        for h in scene.order() {
            let old = scene.occurrence(*h).unwrap();
            let parents = candidate.scene().layer_origin(scene.parent(*h));
            if let OccurrenceContent::Selection(s) = old.content {
                let target = SourceTarget::Selection(s);
                let after = candidate.local_to_document(target).inverse().ok_or_else(invalid_placement)?;
                let map = self.local_to_document(target).then(to_canvas).then(after);
                let transformed = self.artwork.selections.get(s).ok_or(DocumentError::MissingTarget(target))?.selection.transformed(map)?;
                candidate.artwork.selections.get_mut(s).unwrap().selection = transformed;
                continue;
            }
            if let OccurrenceContent::Objects(layer) = old.content {
                let map = translation(offsets::checked_sub([0; 2], candidate.scene().layer_origin(Some(*h))).ok_or_else(out_of_range)?).compose(to_canvas64).compose(translation(scene.layer_origin(Some(*h))));
                for child in &self.artwork.object_layers.get(layer).ok_or(DocumentError::MissingOccurrence(*h))?.children {
                    let object = candidate.artwork.objects.get_mut(*child).ok_or(DocumentError::MissingOccurrence(*h))?;
                    object.affine = map.compose(object.affine);
                    object.admit_affine().map_err(|_| CanvasGeometryError::Unsupported("A placed image would exceed the editor's range"))?;
                }
            }
            let paint = scene.source_target(*h).filter(|t| matches!(t, SourceTarget::Paint(_)));
            let exact = pixel_map.filter(|_| paint.is_some_and(|t| scene.paint_base(t).is_some()));
            if let (Some(map), Some(SourceTarget::Paint(handle))) = (exact, paint) {
                remaps.push(self.oriented(&mut candidate, *h, handle, map, geometry.rect.size, limits)?);
                predicted.insert(SourceTarget::Paint(handle), raster_data(scene.raster(SourceTarget::Paint(handle)).ok_or(DocumentError::MissingTarget(SourceTarget::Paint(handle)))?)?.tiles.len());
            }
            let framed = paint.filter(|_| exact.is_none());
            let targets: Vec<_> = framed.into_iter().chain(old.mask.as_ref().map(|m| SourceTarget::Coverage(m.source))).collect();
            if targets.is_empty() {
                continue;
            }
            let extent = scene.local_extent(*h);
            let mut rotated = Rect::EMPTY;
            for target in &targets {
                rotated = rotated.union(
                    LayerPlacement::from_affine(Affine::translation(offsets::point(scene.target_offset(*target))))
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
            let origin = [frame[0] as i64, frame[1] as i64];
            let o = candidate.artwork.occurrences.get_mut(*h).unwrap();
            if framed.is_some() {
                o.offset = offsets::checked_sub(origin, parents).ok_or_else(out_of_range)?;
            }
            let owner = if o.positioned() { o.offset } else { [0; 2] };
            if let Some(mask) = o.mask.as_mut() {
                mask.offset = offsets::checked_sub(origin, parents).and_then(|frame| offsets::checked_sub(frame, if mask.linked { owner } else { [0; 2] })).ok_or_else(out_of_range)?;
            }
            for target in targets {
                match target {
                    SourceTarget::Paint(h) => candidate.artwork.paint.get_mut(h).unwrap().domain = grown,
                    SourceTarget::Coverage(h) => candidate.artwork.coverage.get_mut(h).unwrap().domain = grown,
                    _ => {}
                }
                let mut transform = ImageTransform::affine(Affine::translation(offsets::point(scene.target_offset(target))));
                transform.placement = transform
                    .placement
                    .post(Projective::from_affine(to_canvas.then(Affine::translation(Point { x: -world.x, y: -world.y }))))
                    .ok_or_else(invalid_placement)?;
                transform.placement.interpolation = geometry.interpolation;
                if let (SourceTarget::Paint(handle), Some(base)) = (target, scene.paint_base(target)) {
                    let to_local = translation(offsets::checked_sub([0; 2], origin).ok_or_else(out_of_range)?).compose(to_canvas64).compose(translation(scene.layer_origin(Some(*h))));
                    if let Some(spec) = self.retained_resample(&mut candidate, handle, to_local, geometry)? {
                        let tiles = match spec.base { Some(BaseRemap::Resample { extent, .. }) => extent.map(|v| v.div_ceil(TILE_SIZE) as usize).iter().product(), _ => 0 };
                        predicted.insert(target, tiles);
                        remaps.push(spec);
                        continue;
                    }
                    transform.source_base = Some(base.clone());
                    candidate.artwork.paint.get_mut(handle).unwrap().base = None;
                }
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
                        if x1 > x0 && y1 > y0 {
                            operations.push((
                                target,
                                pixel_operation(rectangle(Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } })?, ERASE),
                            ));
                        }
                    }
                }
            }
        }
        for (handle, _, application) in self.artwork.effects.iter() {
            if let Some(spatial) = application.spatial {
                candidate.artwork.effects.get_mut(handle).unwrap().spatial.as_mut().unwrap().mapping = to_canvas64.compose(spatial.mapping);
            }
        }
        candidate.check_raster_limits(&predicted, limits.project)?;
        for (_, op) in &mut operations {
            op.coverage.target = self.artwork.coverage.next_handle();
            op.coverage.use_.source = op.coverage.target;
        }
        Ok((candidate, operations, remaps))
    }
    /// Rebase an untouched photo layer in `candidate` for a base resample that
    /// keeps its interpretation, with `to_local` mapping old to new local
    /// pixels. `None` for a layer with paint or material, a working-pixel base,
    /// a reduced color mode or CMYK samples.
    fn retained_resample(&self, candidate: &mut Document, handle: PaintHandle, to_local: Affine64, geometry: &CanvasGeometry) -> Result<Option<RemapSpec>, CanvasGeometryError> {
        let target = SourceTarget::Paint(handle);
        let source = self.artwork.paint.get(handle).ok_or(DocumentError::MissingTarget(target))?;
        let Some(base) = source.base.as_ref() else { return Ok(None) };
        let untouched = raster_data(&source.raster)?.tiles.is_empty() && source.operations.is_empty();
        if !untouched || base.policy != PaintBasePolicy::SourceProfile || source.color_mode != color::LayerColorMode::FullColor
            || base.image.interpretation.channels == color::source::SourceChannels::Cmyk {
            return Ok(None);
        }
        let placed = to_local.compose(Affine64([1., 0., 0., 1., f64::from(base.offset[0]), f64::from(base.offset[1])]));
        let [min, max] = placed.bounds(base.image.extent);
        let paint = candidate.artwork.paint.get_mut(handle).unwrap();
        let low: [i64; 2] = std::array::from_fn(|i| ((min[i] + f64::from(PIXEL_TOLERANCE)).floor() as i64).max(0));
        let high: [i64; 2] = std::array::from_fn(|i| {
            let reach = (max[i] - f64::from(PIXEL_TOLERANCE)).ceil() as i64;
            if geometry.delete_outside { reach.min(i64::from(paint.domain[i])) } else { reach }
        });
        if (0..2).any(|i| low[i] >= high[i]) {
            paint.base = None;
            return Ok(Some(RemapSpec { target, base: None, extent: paint.domain, map: GridMap::IDENTITY, keep: [[0; 2], paint.domain.map(i64::from)] }));
        }
        let extent: [u32; 2] = std::array::from_fn(|i| u32::try_from(high[i] - low[i]).unwrap_or(u32::MAX));
        if extent.iter().any(|v| *v > MAX_EXTENT) {
            return Err(CanvasGeometryError::ExtentTooLarge { limit: MAX_EXTENT });
        }
        paint.domain = std::array::from_fn(|i| paint.domain[i].max(u32::try_from(high[i]).unwrap_or(u32::MAX)));
        paint.base.as_mut().unwrap().offset = low.map(|v| v as u32);
        let to_image = Affine64([1., 0., 0., 1., -(low[0] as f64), -(low[1] as f64)]).compose(placed);
        let domain = paint.domain;
        Ok(Some(RemapSpec { target, base: Some(BaseRemap::Resample { extent, to_image, interpolation: geometry.interpolation }),
            extent: domain, map: GridMap::IDENTITY, keep: [[0; 2], domain.map(i64::from)] }))
    }
    /// An exact flip or turn of a paint layer with a base: the layer's
    /// tile-padded domain turns as a whole, so every tile moves to one tile and
    /// the base moves to a new image in its own interpretation.
    fn oriented(&self, candidate: &mut Document, owner: OccurrenceHandle, handle: PaintHandle, doc: GridMap, canvas: [u32; 2], limits: GeometryLimits) -> Result<RemapSpec, CanvasGeometryError> {
        let target = SourceTarget::Paint(handle);
        let source = self.artwork.paint.get(handle).ok_or(DocumentError::MissingTarget(target))?;
        let base = source.base.as_ref().ok_or(DocumentError::MissingTarget(target))?;
        let origin = self.scene().layer_origin(Some(owner));
        let (low, extent, local) = turned_layout(origin, source.domain, doc, canvas, limits)?;
        let image_low = base.offset.map(i64::from);
        let image_high = std::array::from_fn(|i| image_low[i] + i64::from(base.image.extent[i]));
        let [placed_low, placed_high] = local.result_rect(image_low, image_high);
        let image = GridMap { origin: offsets::checked_sub(local.source(placed_low), image_low).ok_or_else(out_of_range)?, ..local };
        let image_extent = std::array::from_fn(|i| (placed_high[i] - placed_low[i]) as u32);
        let parents = candidate.scene().layer_origin(self.scene().parent(owner));
        let paint = candidate.artwork.paint.get_mut(handle).unwrap();
        paint.domain = extent;
        paint.base.as_mut().unwrap().offset = placed_low.map(|v| v as u32);
        candidate.artwork.occurrences.get_mut(owner).unwrap().offset = offsets::checked_sub(low, parents).ok_or_else(out_of_range)?;
        Ok(RemapSpec { target, base: Some(BaseRemap::Exact { extent: image_extent, map: image }), extent, map: local, keep: local.result_rect([0; 2], source.domain.map(i64::from)) })
    }
    /// The same exact move for the mask of `owner`.
    fn oriented_mask(&self, candidate: &mut Document, owner: OccurrenceHandle, doc: GridMap, canvas: [u32; 2], limits: GeometryLimits) -> Result<RemapSpec, CanvasGeometryError> {
        let mask = self.scene().occurrence(owner).and_then(|o| o.mask.as_ref()).ok_or(DocumentError::MissingOccurrence(owner))?;
        let target = SourceTarget::Coverage(mask.source);
        let source = self.artwork.coverage.get(mask.source).ok_or(DocumentError::MissingTarget(target))?;
        let origin = self.scene().mask_origin(owner).ok_or(DocumentError::MissingOccurrence(owner))?;
        let (low, extent, local) = turned_layout(origin, source.domain, doc, canvas, limits)?;
        let parents = candidate.scene().layer_origin(self.scene().parent(owner));
        candidate.artwork.coverage.get_mut(mask.source).unwrap().domain = extent;
        let occurrence = candidate.artwork.occurrences.get_mut(owner).unwrap();
        let owner_offset = if occurrence.positioned() { occurrence.offset } else { [0; 2] };
        let mask = occurrence.mask.as_mut().unwrap();
        mask.offset = offsets::checked_sub(low, parents).and_then(|frame| offsets::checked_sub(frame, if mask.linked { owner_offset } else { [0; 2] })).ok_or_else(out_of_range)?;
        Ok(RemapSpec { target, base: None, extent, map: local, keep: local.result_rect([0; 2], source.domain.map(i64::from)) })
    }
    /// A whole-layer flip or turn of a paint layer with a base, with its linked
    /// mask: exact sample moves instead of a resample. `map` takes the layer's
    /// local pixels where they go, keeping its offset. Other layers and maps
    /// have no exact plan.
    pub fn exact_layer_transform_plan(&self, target: SourceTarget, map: &LayerPlacement, limits: GeometryLimits) -> Option<Result<CanvasGeometryPlan, String>> {
        let SourceTarget::Paint(handle) = target else { return None };
        self.scene().paint_base(target)?;
        let [a, b, c, d, x, y] = map.as_affine()?.0.map(f64::from);
        let offset = self.scene().layer_origin(self.scene().source_owner(target)).map(|v| v as f64);
        let doc = CanvasGeometry { linear: Affine64([a, b, c, d, x + offset[0] - a * offset[0] - c * offset[1], y + offset[1] - b * offset[0] - d * offset[1]]), ..CanvasGeometry::crop(CanvasRect { origin: [0; 2], size: self.composition().size }) }.pixel_map()?;
        Some((|| {
            if let Some(reason) = self.layer_transform_refusal(target) {
                return Err(reason.to_string());
            }
            let owner = self.scene().source_owner(target).ok_or("Select a paint layer")?;
            let canvas = self.composition().size;
            let mut candidate = self.clone();
            let mut remaps = vec![self.oriented(&mut candidate, owner, handle, doc, canvas, limits).map_err(|e| e.to_string())?];
            if self.scene().occurrence(owner).and_then(|o| o.mask.as_ref()).is_some_and(|m| m.linked) {
                remaps.push(self.oriented_mask(&mut candidate, owner, doc, canvas, limits).map_err(|e| e.to_string())?);
            }
            let predicted = remaps.iter().map(|spec| Ok((spec.target, raster_data(self.target_raster(spec.target).ok_or(DocumentError::MissingTarget(spec.target))?)?.tiles.len()))).collect::<Result<BTreeMap<_, _>, CanvasGeometryError>>().map_err(|e| e.to_string())?;
            candidate.check_raster_limits(&predicted, limits.project).map_err(|e| e.to_string())?;
            let mut edits = geometry_edits(self, &candidate).map_err(|e| e.to_string())?;
            remap_edits(self, &candidate, &remaps, &mut edits).map_err(|e| e.to_string())?;
            Ok(CanvasGeometryPlan { edits, operations: Vec::new(), remaps })
        })())
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
