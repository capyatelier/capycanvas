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
        Self {
            rect,
            linear: Affine::IDENTITY,
            interpolation: Interpolation::default(),
            delete_outside: false,
        }
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
        Self {
            rect: CanvasRect { origin: [0; 2], size },
            linear,
            interpolation: Interpolation::Nearest,
            delete_outside: false,
        }
    }
    /// Scale a `canvas` of that size to `size`.
    pub fn resize(canvas: [u32; 2], size: [u32; 2], interpolation: Interpolation) -> Self {
        let [x, y] = std::array::from_fn(|i| size[i] as f32 / canvas[i].max(1) as f32);
        Self {
            rect: CanvasRect { origin: [0; 2], size },
            linear: Affine([x, 0., 0., y, 0., 0.]),
            interpolation,
            delete_outside: false,
        }
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
    pub operations: Vec<(LayerId, LayerOperation)>,
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
            Self::CanvasTooLarge { limit } => write!(f, "The canvas can be at most {limit} px on each side"),
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
fn rebased_placement(placement: Affine, delta: Point) -> Affine {
    Affine::translation(Point { x: -delta.x, y: -delta.y })
        .then(placement)
        .then(Affine::translation(delta))
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

fn canvas_rect(size: [u32; 2]) -> Rect {
    Rect {
        min: Point::default(),
        max: Point { x: size[0] as f32, y: size[1] as f32 },
    }
}

fn invalid_placement() -> CanvasGeometryError {
    CanvasGeometryError::Document(DocumentError::InvalidLayerOperation("Invalid layer placement"))
}

/// The canvas window in a target's local pixels.
fn local_window(layers: &[Layer], id: LayerId, canvas: [u32; 2]) -> Result<Rect, CanvasGeometryError> {
    let inverse = target_transform(layers, id).inverse().ok_or_else(invalid_placement)?;
    Ok(inverse.bounds(canvas_rect(canvas)))
}

/// Local pixels a target holds, in whole tiles: its tiles and the tiles a
/// mask's initial coverage fills when the renderer draws it.
fn content_bounds(layer: &Layer, id: LayerId) -> Result<Rect, CanvasGeometryError> {
    let (raster, initial) = if id == layer.id {
        (&layer.raster, None)
    } else {
        let mask = layer.mask.as_ref().filter(|m| m.id == id).ok_or(DocumentError::MissingLayer(id))?;
        (&mask.raster, mask.initial.as_ref())
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
fn pixel_operation(selection: Selection, kind: LayerOperationKind) -> LayerOperation {
    let mut coverage = LayerMask::reveal_all(LayerId(0), Point::default());
    coverage.default_coverage = f32::from(selection.inverted);
    coverage.initial = Some(selection);
    LayerOperation { placement: Affine::IDENTITY, coverage, kind }
}

fn rectangle(rect: Rect) -> Result<Selection, CanvasGeometryError> {
    Ok(Selection::polygon(rect.corners().to_vec())?)
}

const ERASE: LayerOperationKind = LayerOperationKind::Erase { alpha_locked: false };

/// Erase a paint target's pixels outside `window`, within `extent`, as up to
/// four strips so that only the tiles along the edges are rewritten.
fn erase_outside(window: Rect, extent: [u32; 2], tiles: &RasterData) -> Result<Vec<LayerOperation>, CanvasGeometryError> {
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
        .filter(|strip| {
            strip.max.x - strip.min.x > PIXEL_TOLERANCE && strip.max.y - strip.min.y > PIXEL_TOLERANCE && occupied(*strip)
        })
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

/// How much `affine` scales areas, as a length: the factor for distances
/// declared in pixels.
fn length_scale(affine: Affine) -> f32 {
    let [a, b, c, d, ..] = affine.0;
    (a * d - b * c).abs().sqrt()
}

fn is_translation(affine: Affine) -> bool {
    let [a, b, c, d, ..] = affine.0;
    [a - 1., b, c, d - 1.].iter().all(|v| v.abs() <= 1e-6)
}

impl Document {
    /// Validate a canvas change against the limits without building it.
    pub fn check_canvas_geometry(
        &self,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
    ) -> Result<(), CanvasGeometryError> {
        self.canvas_geometry_plan(geometry, limits).map(drop)
    }

    /// One reversible batch, and its pixel operations, that move the canvas
    /// to `geometry`. Locked layers follow: document geometry is not a
    /// content edit.
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
        let same = !resampled && rect.origin == [0; 2] && rect.size == [self.width, self.height];
        if same && !geometry.delete_outside {
            return Err(CanvasGeometryError::Unchanged);
        }
        let mut plan = if resampled { self.resampled_plan(geometry, limits)? } else { self.crop_plan(geometry, limits)? };
        if same && plan.edits.is_empty() && plan.operations.is_empty() {
            return Err(CanvasGeometryError::Unchanged);
        }
        if let Some(resolution) = self.resolution
            && swaps_axes(geometry.linear)
            && resolution.swapped() != resolution
        {
            plan.edits.push(Edit::SetResolution(Some(resolution.swapped())));
        }
        let to_canvas = geometry.to_canvas();
        if let Some(selection) = &self.selection {
            let moved = selection.transformed(to_canvas)?;
            if moved != *selection {
                plan.edits.push(Edit::SetSelection(Some(moved)));
            }
        }
        if !self.rulers.is_empty() && to_canvas != Affine::IDENTITY {
            plan.edits.push(Edit::SetRulers(
                self.rulers.iter().map(|r| Ruler { id: r.id, geometry: r.geometry.transformed(to_canvas) }).collect(),
            ));
        }
        plan.edits.insert(0, Edit::SetCanvasSize { size: rect.size, origin: rect.origin });
        Ok(plan)
    }

    /// Whether every paint layer without a source, and every mask, can be
    /// painted across the whole canvas.
    pub fn extents_cover_canvas(&self) -> bool {
        let canvas = [self.width, self.height];
        self.layers.iter().all(|layer| {
            let extent = layer.local_extent(canvas);
            let paint = (layer.kind == LayerKind::Paint && layer.source.is_none()).then_some(layer.id);
            let mask = layer.mask.as_ref().filter(|_| layer.source.is_none()).map(|m| m.id);
            paint.into_iter().chain(mask).all(|id| {
                local_window(&self.layers, id, canvas).is_ok_and(|window| {
                    window.min.x >= -PIXEL_TOLERANCE
                        && window.min.y >= -PIXEL_TOLERANCE
                        && window.max.x <= extent[0] as f32 + PIXEL_TOLERANCE
                        && window.max.y <= extent[1] as f32 + PIXEL_TOLERANCE
                })
            })
        })
    }

    /// Root layers move by the canvas origin; their masks move with them.
    fn shifted_roots(&self, origin: [i32; 2]) -> Vec<Layer> {
        let origin = Point { x: origin[0] as f32, y: origin[1] as f32 };
        let mut layers = self.layers.clone();
        for layer in layers.iter_mut().filter(|l| l.properties.parent.is_none() && l.kind != LayerKind::Background) {
            layer.properties.offset = shift(layer.properties.offset, origin);
            if let Some(mask) = &mut layer.mask {
                mask.offset = shift(mask.offset, origin);
            }
        }
        layers
    }

    /// A crop or growth without resampling: offsets and rebases, and with
    /// Delete Cropped Pixels, trimmed tiles and erased edges.
    fn crop_plan(&self, geometry: &CanvasGeometry, limits: GeometryLimits) -> Result<CanvasGeometryPlan, CanvasGeometryError> {
        let rect = geometry.rect;
        let (mut layers, changes) = self.canvas_layout(geometry, limits)?;
        for (layer, change) in layers.iter_mut().zip(&changes) {
            let Some(change) = change else { continue };
            let size = TILE_SIZE as i32;
            let delta = Point { x: (change.tiles[0] * size) as f32, y: (change.tiles[1] * size) as f32 };
            let moved = change.tiles != [0; 2];
            if layer.kind == LayerKind::Paint {
                layer.raster = rebased_raster(&layer.raster, change)?;
                if moved {
                    layer.properties.offset = shift(layer.properties.offset, delta);
                    layer.properties.placement = rebased_placement(layer.properties.placement, delta);
                }
            }
            if let Some(mask) = &mut layer.mask {
                mask.raster = rebased_raster(&mask.raster, change)?;
                if moved {
                    mask.offset = shift(mask.offset, delta);
                    mask.placement = rebased_placement(mask.placement, delta);
                    mask.initial = mask.initial.as_ref().map(|s| s.translated(delta));
                }
            }
            let base = layer.source.as_ref().map_or(rect.size, |source| {
                std::array::from_fn(|i| rect.size[i].max(source.extent[i]))
            });
            layer.properties.extent = (0..2).any(|i| change.extent[i] > base[i]).then_some(change.extent);
        }
        self.check_raster_limits(&layers, &BTreeMap::new(), limits.project)?;
        let mut operations = Vec::new();
        for (layer, change) in layers.iter().zip(&changes) {
            let Some(change) = change.as_ref().filter(|c| c.trim && layer.kind == LayerKind::Paint) else {
                continue;
            };
            let tiles = raster_data(&layer.raster)?;
            if tiles.tiles.is_empty() {
                continue;
            }
            let transform = target_transform(&layers, layer.id);
            let erase = if is_translation(transform) {
                erase_outside(local_window(&layers, layer.id, rect.size)?, change.extent, &tiles)?
            } else {
                let inverse = transform.inverse().ok_or_else(invalid_placement)?;
                let mut outside = Selection::polygon(canvas_rect(rect.size).corners().map(|p| inverse.map(p)).to_vec())?;
                outside.inverted = true;
                vec![pixel_operation(outside, ERASE)]
            };
            operations.extend(erase.into_iter().map(|op| (layer.id, op)));
        }
        let edits = layers
            .into_iter()
            .zip(&self.layers)
            .filter(|(layer, before)| layer != *before)
            .map(|(layer, _)| Edit::ReplaceLayer(Box::new(layer)))
            .collect();
        Ok(CanvasGeometryPlan { edits, operations })
    }

    /// Root offsets after the move, and each layer's rebase and extent.
    fn canvas_layout(
        &self,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
    ) -> Result<(Vec<Layer>, Vec<Option<LayerChange>>), CanvasGeometryError> {
        let rect = geometry.rect;
        let layers = self.shifted_roots(rect.origin);
        let canvas = [self.width, self.height];
        let mut changes = Vec::with_capacity(layers.len());
        for layer in &layers {
            let paint = (layer.kind == LayerKind::Paint).then_some(layer.id);
            let targets: Vec<_> = paint.into_iter().chain(layer.mask.as_ref().map(|m| m.id)).collect();
            if targets.is_empty() || matches!(layer.kind, LayerKind::Background | LayerKind::Selection) {
                changes.push(None);
                continue;
            }
            let extent = layer.local_extent(canvas);
            let change = if layer.source.is_some() {
                LayerChange { tiles: [0; 2], extent, trim: false }
            } else {
                let mut window = Rect::EMPTY;
                for id in targets {
                    window = window.union(local_window(&layers, id, rect.size)?);
                }
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
                LayerChange { tiles, extent: grown, trim: geometry.delete_outside }
            };
            if change.extent.iter().any(|v| *v > limits.project.dimension) {
                return Err(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension });
            }
            changes.push(Some(change));
        }
        Ok((layers, changes))
    }

    /// A rotated, flipped or scaled canvas. Paint layers and masks without a
    /// source resample into a new frame; photos, Selection Layers, the
    /// selection and guides change as metadata.
    fn resampled_plan(&self, geometry: &CanvasGeometry, limits: GeometryLimits) -> Result<CanvasGeometryPlan, CanvasGeometryError> {
        let rect = geometry.rect;
        let to_canvas = geometry.to_canvas();
        let canvas = [self.width, self.height];
        let mut layers = self.shifted_roots(rect.origin);
        let mut operations = Vec::new();
        let mut predicted = BTreeMap::new();
        for (index, old) in self.layers.iter().enumerate() {
            if old.kind == LayerKind::Background {
                continue;
            }
            let parents = shift(target_offset(&layers, old.id), layers[index].properties.offset);
            if old.kind == LayerKind::Selection {
                if let Some(selection) = &old.selection {
                    let after = target_transform(&layers, old.id).inverse().ok_or_else(invalid_placement)?;
                    let map = target_transform(&self.layers, old.id).then(to_canvas).then(after);
                    layers[index].selection = Some(selection.transformed(map)?);
                }
                continue;
            }
            if old.source.is_some() {
                let world = target_offset(&layers, old.id);
                layers[index].properties.placement =
                    target_transform(&self.layers, old.id).then(to_canvas).then(Affine::translation(shift(Point::default(), world)));
                if let Some(mask) = &old.mask {
                    let desired = target_transform(&self.layers, mask.id).then(to_canvas);
                    layers[index].mask.as_mut().unwrap().placement = Affine::IDENTITY;
                    let rest = target_transform(&layers, mask.id).inverse().ok_or_else(invalid_placement)?;
                    layers[index].mask.as_mut().unwrap().placement = desired.then(rest);
                }
                continue;
            }
            let paint = (old.kind == LayerKind::Paint).then_some(old.id);
            let targets: Vec<_> = paint.into_iter().chain(old.mask.as_ref().map(|m| m.id)).collect();
            if targets.is_empty() {
                continue;
            }
            let extent = old.local_extent(canvas);
            let mut rotated = Rect::EMPTY;
            for &id in &targets {
                rotated = rotated.union(target_transform(&self.layers, id).then(to_canvas).bounds(canvas_rect(extent)));
            }
            let frame = if geometry.delete_outside {
                [0.; 2]
            } else {
                [rotated.min.x, rotated.min.y].map(|v| (v + PIXEL_TOLERANCE).floor().min(0.))
            };
            let far = [rotated.max.x, rotated.max.y];
            let grown: [u32; 2] = std::array::from_fn(|i| {
                let size = rect.size[i] as f32;
                let reach = if geometry.delete_outside { size } else { far[i].max(size) };
                ((reach - frame[i] - PIXEL_TOLERANCE).ceil().max(0.) as u32).max(extent[i])
            });
            if grown.iter().any(|v| *v > limits.project.dimension) {
                return Err(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension });
            }
            let world = Point { x: frame[0], y: frame[1] };
            let layer = &mut layers[index];
            if paint.is_some() {
                layer.properties.offset = shift(world, parents);
                layer.properties.placement = Affine::IDENTITY;
            }
            layer.properties.extent = (0..2).any(|i| grown[i] > rect.size[i]).then_some(grown);
            if let Some(mask) = &mut layer.mask {
                mask.placement = Affine::IDENTITY;
                mask.offset = shift(world, parents);
            }
            for id in targets {
                let map = target_transform(&self.layers, id)
                    .then(to_canvas)
                    .then(Affine::translation(shift(Point::default(), world)));
                let content = content_bounds(old, id)?;
                let existing = raster_data(self.target_raster(id).ok_or(DocumentError::MissingLayer(id))?)?;
                let transform = ImageTransform { map: TransformMap::Affine(map), interpolation: geometry.interpolation, ..Default::default() };
                let written: BTreeSet<[u32; 2]> = if content.is_empty() {
                    existing.tiles.keys().map(|k| k.coordinate).collect()
                } else {
                    pages(transform.affected_regions(content)[1], grown).collect()
                };
                predicted.insert(id, written.len());
                if !content.is_empty() {
                    operations.push((id, pixel_operation(rectangle(content)?, LayerOperationKind::Transform(transform))));
                }
                if geometry.delete_outside && paint == Some(id) && !content.is_empty() {
                    let size = rect.size.map(|v| v as f32);
                    let [width, height] = grown.map(|v| v as f32);
                    for [x0, y0, x1, y1] in [[size[0], 0., width, height], [0., size[1], size[0], height]] {
                        if x1 - x0 > PIXEL_TOLERANCE && y1 - y0 > PIXEL_TOLERANCE {
                            let strip = Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } };
                            operations.push((id, pixel_operation(rectangle(strip)?, ERASE)));
                        }
                    }
                }
            }
        }
        let factor = length_scale(geometry.linear);
        if (factor - 1.).abs() > 1e-4 {
            for layer in layers.iter_mut().filter(|l| l.kind == LayerKind::Effect) {
                if let Some(scaled) = layer.effect.as_ref().and_then(|e| e.scaled_px(factor)) {
                    layer.effect = Some(Arc::new(scaled));
                }
            }
        }
        self.check_raster_limits(&layers, &predicted, limits.project)?;
        let edits = layers
            .into_iter()
            .zip(&self.layers)
            .filter(|(layer, before)| layer != *before)
            .map(|(layer, _)| Edit::ReplaceLayer(Box::new(layer)))
            .collect();
        Ok(CanvasGeometryPlan { edits, operations })
    }

    /// Hidden tiles count like visible ones; `predicted` gives the tiles a
    /// resampled target will hold. Rasters still being captured are admitted
    /// by their producer and are not counted here.
    fn check_raster_limits(
        &self,
        layers: &[Layer],
        predicted: &BTreeMap<LayerId, usize>,
        limits: ProjectLimits,
    ) -> Result<(), CanvasGeometryError> {
        let mut tiles = 0usize;
        let mut bytes = 0u64;
        let mut sources = BTreeSet::new();
        for layer in layers {
            if let Some(source) = &layer.source
                && sources.insert(Arc::as_ptr(source) as usize)
            {
                tiles = tiles.saturating_add(source.tiles.len());
            }
            let targets = std::iter::once((layer.id, &layer.raster, RasterPlane::Color))
                .chain(layer.mask.iter().map(|m| (m.id, &m.raster, RasterPlane::Mask)));
            for (id, raster, plane) in targets {
                if let Some(count) = predicted.get(&id) {
                    let page = plane.descriptor(self.color).byte_len([TILE_SIZE; 2]).unwrap_or(0) as u64;
                    tiles = tiles.saturating_add(*count);
                    bytes = bytes.saturating_add(page.saturating_mul(*count as u64));
                } else if let Some(Ok(data)) = raster.try_data() {
                    tiles = tiles.saturating_add(data.tiles.len());
                    for tile in data.tiles.values() {
                        bytes = bytes.saturating_add(tile.descriptor().byte_len([TILE_SIZE; 2]).unwrap_or(0) as u64);
                    }
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
