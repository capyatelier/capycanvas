//! Warp meshes: tensor-product cubic Bézier patches over a source rectangle.
use crate::{Affine, Point};
use std::sync::Arc;

/// `frame` maps the unit square onto the source rectangle, split into
/// `cells` = [columns, rows] patches. `net` holds the row-major
/// `(3 * columns + 1) × (3 * rows + 1)` control points in destination
/// layer-local pixels.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeshMap {
    pub frame: Affine,
    pub cells: [u16; 2],
    pub net: Arc<[Point]>,
}
