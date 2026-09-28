//! Canvas geometry: one plan turns a new canvas rectangle into a single
//! reversible batch. The canvas is a window over each layer's local extent,
//! so shrinking it hides pixels and growing it shows them again.
//!
//! Every geometry edit keeps these invariants:
//! - Root offsets carry the canvas origin. Child offsets are relative to their
//!   group and never change.
//! - A paint layer without a source, and every mask, has a local extent that
//!   covers the canvas window, so the whole canvas stays paintable. Growing
//!   the canvas left or up rebases such a layer by whole tiles: its tiles are
//!   re-keyed and still shared, and its offset moves the other way.
//! - A layer with a source never rebases. A new strip beside a photo stays
//!   unpaintable, as a moved photo's does.
//! - Stored extents never shrink. Hidden tiles count toward the project's
//!   tile and byte limits like visible ones.
use crate::raster::{RasterData, RasterRevision, TILE_SIZE, TileKey};
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
    /// Maps the cropped canvas onto the new one. Only the identity is supported.
    pub linear: Affine,
    pub interpolation: Interpolation,
    /// Drop the pixels outside the new canvas instead of keeping them hidden.
    pub delete_outside: bool,
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
struct LayerChange {
    tiles: [u32; 2],
    extent: [u32; 2],
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

fn rebased_raster(raster: &RasterRevision, tiles: [u32; 2]) -> Result<RasterRevision, CanvasGeometryError> {
    if tiles == [0; 2] {
        return Ok(raster.clone());
    }
    let data = match raster.try_data() {
        Some(Ok(data)) => data,
        Some(Err(_)) => return Err(CanvasGeometryError::Document(DocumentError::InvalidLayerOperation("A layer's pixels failed to save"))),
        None => return Err(CanvasGeometryError::Pending),
    };
    Ok(RasterRevision::backed(RasterData {
        tiles: data
            .tiles
            .iter()
            .map(|(key, tile)| {
                let coordinate = [key.coordinate[0] + tiles[0], key.coordinate[1] + tiles[1]];
                (TileKey { plane: key.plane, coordinate }, tile.clone())
            })
            .collect(),
        watercolor: data.watercolor,
    }))
}

/// The canvas window in a target's local pixels.
fn local_window(layers: &[Layer], id: LayerId, canvas: [u32; 2]) -> Result<Rect, CanvasGeometryError> {
    let inverse = target_transform(layers, id)
        .inverse()
        .ok_or(DocumentError::InvalidLayerOperation("Invalid layer placement"))?;
    Ok(inverse.bounds(Rect {
        min: Point::default(),
        max: Point { x: canvas[0] as f32, y: canvas[1] as f32 },
    }))
}

/// Float noise in placements must not rebase or grow a layer by a pixel.
const PIXEL_TOLERANCE: f32 = 1e-3;

impl Document {
    /// Validate a canvas change against the limits without building it.
    pub fn check_canvas_geometry(
        &self,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
    ) -> Result<(), CanvasGeometryError> {
        self.canvas_layout(geometry, limits).map(drop)
    }

    /// One reversible batch that moves the canvas to `geometry`. Locked layers
    /// follow: document geometry is not a content edit.
    pub fn canvas_geometry_edit(
        &self,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
    ) -> Result<Edit, CanvasGeometryError> {
        let (mut layers, changes) = self.canvas_layout(geometry, limits)?;
        let rect = geometry.rect;
        let origin = Point { x: rect.origin[0] as f32, y: rect.origin[1] as f32 };
        let mut edits = vec![Edit::SetCanvasSize { size: rect.size, origin: rect.origin }];
        for ((layer, change), before) in layers.iter_mut().zip(&changes).zip(&self.layers) {
            if let Some(change) = change {
                let delta = Point {
                    x: (change.tiles[0] * TILE_SIZE) as f32,
                    y: (change.tiles[1] * TILE_SIZE) as f32,
                };
                if layer.kind == LayerKind::Paint {
                    layer.raster = rebased_raster(&layer.raster, change.tiles)?;
                    if change.tiles != [0; 2] {
                        layer.properties.offset = shift(layer.properties.offset, delta);
                        layer.properties.placement = rebased_placement(layer.properties.placement, delta);
                    }
                }
                if let Some(mask) = &mut layer.mask {
                    mask.raster = rebased_raster(&mask.raster, change.tiles)?;
                    if change.tiles != [0; 2] {
                        mask.offset = shift(mask.offset, delta);
                        mask.placement = rebased_placement(mask.placement, delta);
                        mask.initial = mask.initial.as_ref().map(|s| s.translated(delta));
                    }
                }
                let base = layer.source.as_ref().map_or(rect.size, |source| {
                    std::array::from_fn(|i| rect.size[i].max(source.extent[i]))
                });
                layer.properties.extent =
                    (0..2).any(|i| change.extent[i] > base[i]).then_some(change.extent);
            }
            if layer != before {
                edits.push(Edit::ReplaceLayer(Box::new(layer.clone())));
            }
        }
        if rect.origin != [0; 2] {
            if let Some(selection) = &self.selection {
                edits.push(Edit::SetSelection(Some(selection.translated(Point { x: -origin.x, y: -origin.y }))));
            }
            if !self.rulers.is_empty() {
                edits.push(Edit::SetRulers(
                    self.rulers
                        .iter()
                        .map(|r| Ruler { id: r.id, geometry: r.geometry.translated(Point { x: -origin.x, y: -origin.y }) })
                        .collect(),
                ));
            }
        }
        Ok(Edit::Batch(edits))
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

    /// Root offsets after the move, and each layer's rebase and extent.
    fn canvas_layout(
        &self,
        geometry: &CanvasGeometry,
        limits: GeometryLimits,
    ) -> Result<(Vec<Layer>, Vec<Option<LayerChange>>), CanvasGeometryError> {
        if geometry.linear != Affine::IDENTITY {
            return Err(CanvasGeometryError::Unsupported("Resampling the canvas is not available yet"));
        }
        if geometry.delete_outside {
            return Err(CanvasGeometryError::Unsupported("Deleting cropped pixels is not available yet"));
        }
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
        if rect.origin == [0; 2] && rect.size == [self.width, self.height] {
            return Err(CanvasGeometryError::Unchanged);
        }
        let origin = Point { x: rect.origin[0] as f32, y: rect.origin[1] as f32 };
        let mut layers = self.layers.clone();
        for layer in layers.iter_mut().filter(|l| l.properties.parent.is_none() && l.kind != LayerKind::Background) {
            layer.properties.offset = shift(layer.properties.offset, origin);
            if let Some(mask) = &mut layer.mask {
                mask.offset = shift(mask.offset, origin);
            }
        }
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
                LayerChange { tiles: [0; 2], extent }
            } else {
                let mut window = Rect::EMPTY;
                for id in targets {
                    let local = local_window(&layers, id, rect.size)?;
                    window.min.x = window.min.x.min(local.min.x);
                    window.min.y = window.min.y.min(local.min.y);
                    window.max.x = window.max.x.max(local.max.x);
                    window.max.y = window.max.y.max(local.max.y);
                }
                let low = [window.min.x, window.min.y];
                let high = [window.max.x, window.max.y];
                let mut tiles = [0; 2];
                let mut grown = extent;
                for axis in 0..2 {
                    let before = (-low[axis] - PIXEL_TOLERANCE).ceil().max(0.);
                    let after = (high[axis] - PIXEL_TOLERANCE).ceil().max(0.);
                    let limit = limits.project.dimension as f32;
                    if before > limit || after > 2. * limit {
                        return Err(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension });
                    }
                    tiles[axis] = (before as u32).div_ceil(TILE_SIZE);
                    let delta = tiles[axis] * TILE_SIZE;
                    grown[axis] = (extent[axis] + delta).max((after as u32).saturating_add(delta));
                }
                LayerChange { tiles, extent: grown }
            };
            if change.extent.iter().any(|v| *v > limits.project.dimension) {
                return Err(CanvasGeometryError::ExtentTooLarge { limit: limits.project.dimension });
            }
            changes.push(Some(change));
        }
        self.check_raster_limits(limits.project)?;
        Ok((layers, changes))
    }

    /// Hidden tiles count like visible ones. Rasters still being captured are
    /// admitted by their producer and are not counted here.
    fn check_raster_limits(&self, limits: ProjectLimits) -> Result<(), CanvasGeometryError> {
        let mut tiles = 0usize;
        let mut bytes = 0u64;
        let mut sources = BTreeSet::new();
        for layer in &self.layers {
            if let Some(source) = &layer.source
                && sources.insert(Arc::as_ptr(source) as usize)
            {
                tiles = tiles.saturating_add(source.tiles.len());
            }
            for raster in std::iter::once(&layer.raster).chain(layer.mask.iter().map(|m| &m.raster)) {
                if let Some(Ok(data)) = raster.try_data() {
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
