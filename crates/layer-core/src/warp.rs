//! Warp meshes: tensor-product cubic Bézier patches over a source rectangle.
use crate::{Affine, Point, Projective, Rect};
use std::sync::Arc;

/// `frame` maps the unit square onto the source rectangle, split into
/// `cells` = [columns, rows] patches. `net` holds the row-major
/// `(3 * columns + 1) × (3 * rows + 1)` control points in destination
/// layer-local pixels. Nodes are the patch corners, every third point in
/// both directions; the points beside a node along a row or column are its
/// tangent handles, and the four points inside each patch shape its interior.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeshMap {
    pub frame: Affine,
    pub cells: [u16; 2],
    pub net: Arc<[Point]>,
}

/// A grid of quads over a mesh's destination surface, each vertex with the
/// source position that maps to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tessellation {
    /// Quads per row and per column; vertices are row-major, one more each way.
    pub grid: [u32; 2],
    pub positions: Vec<Point>,
    pub sources: Vec<Point>,
}

impl Tessellation {
    /// The two triangles of each quad, row by row, as vertex indices.
    pub fn triangles(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        let [columns, rows] = self.grid;
        (0..rows).flat_map(move |j| {
            (0..columns).flat_map(move |i| {
                let a = j * (columns + 1) + i;
                let [b, c, d] = [a + 1, a + columns + 1, a + columns + 2];
                [[a, b, d], [a, d, c]]
            })
        })
    }
}

/// Tangent handle directions around a node: along the row, down the column,
/// back along the row, and up the column.
pub const TANGENT_SIDES: [[i32; 2]; 4] = [[1, 0], [0, 1], [-1, 0], [0, -1]];

impl MeshMap {
    /// Grid presets, in cells. Three by three cells has four by four nodes.
    pub const PRESETS: [[u16; 2]; 3] = [[3, 3], [4, 4], [5, 5]];
    pub const MAX_CELLS: u16 = 32;
    /// How far a rendered mesh edge extends beyond its patches, in both
    /// destination and source pixels, so the filter can soften it.
    pub const SKIRT: f32 = 2.;

    /// The mesh that leaves `bounds` in place.
    pub fn identity(bounds: Rect, cells: [u16; 2]) -> Option<Self> {
        Self::from_affine(bounds, cells, Affine::IDENTITY)
    }

    /// The mesh equal to an affine map over `bounds`. Bézier patches are affine
    /// invariant, so this is exact.
    pub fn from_affine(bounds: Rect, cells: [u16; 2], affine: Affine) -> Option<Self> {
        let frame = unit_frame(bounds)?;
        let map = frame.then(affine);
        let [width, height] = net_size(cells)?;
        let net = (0..height)
            .flat_map(|j| {
                (0..width).map(move |i| {
                    map.map(Point {
                        x: i as f32 / (width - 1) as f32,
                        y: j as f32 / (height - 1) as f32,
                    })
                })
            })
            .collect();
        let mesh = Self { frame, cells, net };
        mesh.valid().then_some(mesh)
    }

    /// A mesh through the perspective image of each node, with tangents and
    /// interior points from the map's derivatives at the nodes (bicubic
    /// Hermite with twist vectors). Exact when the map is affine.
    pub fn from_projective(bounds: Rect, cells: [u16; 2], map: &Projective) -> Option<Self> {
        let frame = unit_frame(bounds)?;
        let [width, height] = net_size(cells)?;
        if !map.covers(bounds) {
            return None;
        }
        let [du, dv] = [1. / f64::from(cells[0]), 1. / f64::from(cells[1])];
        let mut net = vec![Point::default(); width * height];
        for row in 0..=usize::from(cells[1]) {
            for column in 0..=usize::from(cells[0]) {
                let [u, v] = [column as f64 * du, row as f64 * dv];
                let [f, fu, fv, fuv] = jet(map, frame, u, v)?;
                for (di, dj) in [
                    (-1i32, -1i32),
                    (-1, 0),
                    (-1, 1),
                    (0, -1),
                    (0, 0),
                    (0, 1),
                    (1, -1),
                    (1, 0),
                    (1, 1),
                ] {
                    let [i, j] = [column as i32 * 3 + di, row as i32 * 3 + dj];
                    if i < 0 || j < 0 || i >= width as i32 || j >= height as i32 {
                        continue;
                    }
                    let [a, b] = [f64::from(di) * du / 3., f64::from(dj) * dv / 3.];
                    let point = [0, 1].map(|k| f[k] + a * fu[k] + b * fv[k] + a * b * fuv[k]);
                    net[j as usize * width + i as usize] = Point {
                        x: point[0] as f32,
                        y: point[1] as f32,
                    };
                }
            }
        }
        let mesh = Self {
            frame,
            cells,
            net: net.into(),
        };
        mesh.valid().then_some(mesh)
    }

    /// Finite, invertible framing, bounded cells and a net of matching size.
    pub fn valid(&self) -> bool {
        net_size(self.cells).is_some_and(|[w, h]| w * h == self.net.len())
            && self.frame.inverse().is_some()
            && self.net.iter().all(|p| p.x.is_finite() && p.y.is_finite())
    }

    fn width(&self) -> usize {
        usize::from(self.cells[0]) * 3 + 1
    }
    fn point(&self, i: usize, j: usize) -> Point {
        self.net[j * self.width() + i]
    }

    /// The destination of a source point inside the mesh's rectangle.
    pub fn map(&self, p: Point) -> Option<Point> {
        let unit = self.frame.inverse()?.map(p);
        let range = -1e-5..=1. + 1e-5;
        if !range.contains(&unit.x) || !range.contains(&unit.y) {
            return None;
        }
        Some(self.evaluate(unit.x.clamp(0., 1.), unit.y.clamp(0., 1.)))
    }

    /// The surface at unit coordinates.
    fn evaluate(&self, u: f32, v: f32) -> Point {
        let [columns, rows] = self.cells.map(f32::from);
        let [cu, cv] = [
            (u * columns).floor().clamp(0., columns - 1.),
            (v * rows).floor().clamp(0., rows - 1.),
        ];
        self.patch([cu as usize, cv as usize], u * columns - cu, v * rows - cv)
    }

    /// One patch at its own parameters in [0, 1].
    fn patch(&self, [cu, cv]: [usize; 2], s: f32, t: f32) -> Point {
        let [bu, bv] = [bernstein(s), bernstein(t)];
        let mut sum = [0f32; 2];
        for (j, wv) in bv.iter().enumerate() {
            for (i, wu) in bu.iter().enumerate() {
                let p = self.point(cu * 3 + i, cv * 3 + j);
                sum[0] += wu * wv * p.x;
                sum[1] += wu * wv * p.y;
            }
        }
        Point {
            x: sum[0],
            y: sum[1],
        }
    }

    /// Bounds of the destination surface: the control net's hull.
    pub fn bounds(&self) -> Rect {
        let mut bounds = Rect::EMPTY;
        for p in self.net.iter() {
            bounds.include_circle(*p, 0.);
        }
        bounds
    }

    /// Bounds of everything a renderer draws for the mesh: the control hull
    /// widened by the skirt, which reaches SKIRT destination pixels or SKIRT
    /// source pixels at the steepest rate the patches can reach.
    pub fn drawn_bounds(&self) -> Rect {
        let width = self.width();
        let height = self.net.len() / width;
        let source = self.source();
        let per_cell = [
            (source.max.x - source.min.x) / f32::from(self.cells[0]),
            (source.max.y - source.min.y) / f32::from(self.cells[1]),
        ];
        let mut rate = 1f32;
        for j in 0..height {
            for i in 0..width {
                let p = self.point(i, j);
                for (axis, q) in [
                    (0, (i + 1 < width).then(|| self.point(i + 1, j))),
                    (1, (j + 1 < height).then(|| self.point(i, j + 1))),
                ] {
                    if let Some(q) = q {
                        rate = rate.max(3. * (q.x - p.x).hypot(q.y - p.y) / per_cell[axis]);
                    }
                }
            }
        }
        let reach = Self::SKIRT * rate;
        let hull = self.bounds();
        Rect {
            min: Point {
                x: hull.min.x - reach,
                y: hull.min.y - reach,
            },
            max: Point {
                x: hull.max.x + reach,
                y: hull.max.y + reach,
            },
        }
    }

    /// The source rectangle the mesh covers.
    pub fn source(&self) -> Rect {
        self.frame.bounds(Rect {
            min: Point::default(),
            max: Point { x: 1., y: 1. },
        })
    }

    /// Map closed polygons in source pixels, clipped to the mesh's rectangle
    /// and subdivided to at most one source pixel per edge so they follow the
    /// patches. Even-odd interiors survive the convex clip.
    pub(crate) fn map_polygons(&self, polygons: &[Arc<[Point]>]) -> Vec<Arc<[Point]>> {
        let source = self.source();
        polygons
            .iter()
            .filter_map(|ring| {
                let mut clipped: Vec<Point> = ring.to_vec();
                for (axis, edge, sign) in [
                    (0, source.min.x, 1.),
                    (1, source.min.y, 1.),
                    (0, source.max.x, -1.),
                    (1, source.max.y, -1.),
                ] {
                    let side = |p: Point| (if axis == 0 { p.x } else { p.y } - edge) * sign;
                    let mut kept = Vec::with_capacity(clipped.len() + 2);
                    for (i, p) in clipped.iter().enumerate() {
                        let q = clipped[(i + 1) % clipped.len()];
                        let [a, b] = [side(*p), side(q)];
                        if a >= 0. {
                            kept.push(*p);
                        }
                        if (a >= 0.) != (b >= 0.) {
                            let t = a / (a - b);
                            kept.push(Point {
                                x: p.x + (q.x - p.x) * t,
                                y: p.y + (q.y - p.y) * t,
                            });
                        }
                    }
                    clipped = kept;
                }
                let mut dense = Vec::with_capacity(clipped.len());
                for (i, p) in clipped.iter().enumerate() {
                    let q = clipped[(i + 1) % clipped.len()];
                    let steps = (p.x - q.x).hypot(p.y - q.y).ceil().max(1.) as usize;
                    for k in 0..steps {
                        let t = k as f32 / steps as f32;
                        dense.push(Point {
                            x: p.x + (q.x - p.x) * t,
                            y: p.y + (q.y - p.y) * t,
                        });
                    }
                }
                let mapped: Option<Vec<Point>> = dense.into_iter().map(|p| self.map(p)).collect();
                mapped.filter(|ring| ring.len() >= 3).map(Into::into)
            })
            .collect()
    }

    /// Apply `affine` after the mesh, exactly.
    pub fn post(&self, affine: Affine) -> Self {
        Self {
            net: self.net.iter().map(|p| affine.map(*p)).collect(),
            ..self.clone()
        }
    }

    /// The same motion in the space that `to` maps this mesh's space into.
    pub fn conjugate(&self, to: Affine) -> Self {
        Self {
            frame: self.frame.then(to),
            ..self.post(to)
        }
    }

    /// A triangle surface whose vertices lie on the patches, spaced so that
    /// its chords stay within `tolerance` destination pixels of them.
    pub fn tessellate(&self, tolerance: f32) -> Tessellation {
        let [width, height] = [self.width(), self.net.len() / self.width()];
        let mut bend = [0f32; 3];
        let length = |p: Point| p.x.hypot(p.y);
        for j in 0..height {
            for i in 0..width {
                let p = self.point(i, j);
                let second = |a: Point, b: Point| {
                    length(Point {
                        x: a.x - 2. * p.x + b.x,
                        y: a.y - 2. * p.y + b.y,
                    })
                };
                if i > 0 && i + 1 < width {
                    bend[0] = bend[0].max(second(self.point(i - 1, j), self.point(i + 1, j)));
                }
                if j > 0 && j + 1 < height {
                    bend[1] = bend[1].max(second(self.point(i, j - 1), self.point(i, j + 1)));
                }
                if i + 1 < width && j + 1 < height {
                    let [a, b, c] = [
                        self.point(i + 1, j),
                        self.point(i, j + 1),
                        self.point(i + 1, j + 1),
                    ];
                    bend[2] = bend[2].max(length(Point {
                        x: c.x - a.x - b.x + p.x,
                        y: c.y - a.y - b.y + p.y,
                    }));
                }
            }
        }
        // Within a patch |B_uu| <= 6 bend_u, |B_vv| <= 6 bend_v and
        // |B_uv| <= 9 twist. A chord over steps (1/n_u, 1/n_v) then deviates
        // by at most (6 bend_u / n_u^2 + 18 twist / (n_u n_v) + 6 bend_v / n_v^2) / 8.
        // Give each term a third of the tolerance.
        let tolerance = tolerance.max(1e-3);
        let mut steps = [bend[0], bend[1]].map(|b| (2.25 * b / tolerance).sqrt().max(1.));
        let twist = 6.75 * bend[2] / tolerance;
        if steps[0] * steps[1] < twist {
            let scale = (twist / (steps[0] * steps[1])).sqrt();
            steps = steps.map(|n| n * scale);
        }
        let steps = steps.map(|n| (n.ceil() as u32).clamp(1, 64));
        let [columns, rows] = [
            (u32::from(self.cells[0]) * steps[0]).min(1024),
            (u32::from(self.cells[1]) * steps[1]).min(1024),
        ];
        let along = |steps: u32, cells: u16| -> Vec<(usize, [f32; 4])> {
            let cells = f32::from(cells);
            (0..=steps)
                .map(|i| {
                    let t = i as f32 / steps as f32 * cells;
                    let cell = t.floor().clamp(0., cells - 1.);
                    (cell as usize * 3, bernstein(t - cell))
                })
                .collect()
        };
        let [across, down] = [along(columns, self.cells[0]), along(rows, self.cells[1])];
        let vertices = across.len() * down.len();
        let mut tessellation = Tessellation {
            grid: [columns, rows],
            positions: Vec::with_capacity(vertices),
            sources: Vec::with_capacity(vertices),
        };
        let mut curves = vec![[0f32; 2]; width];
        for (j, (row, weights)) in down.iter().enumerate() {
            for (i, curve) in curves.iter_mut().enumerate() {
                *curve = weights.iter().enumerate().fold([0.; 2], |sum, (n, w)| {
                    let p = self.point(i, row + n);
                    [sum[0] + w * p.x, sum[1] + w * p.y]
                });
            }
            let v = j as f32 / rows as f32;
            for (i, (column, weights)) in across.iter().enumerate() {
                let [x, y] = weights.iter().enumerate().fold([0.; 2], |sum, (n, w)| {
                    let p = curves[column + n];
                    [sum[0] + w * p[0], sum[1] + w * p[1]]
                });
                tessellation.positions.push(Point { x, y });
                tessellation.sources.push(self.frame.map(Point {
                    x: i as f32 / columns as f32,
                    y: v,
                }));
            }
        }
        tessellation
    }

    /// Nodes are numbered row by row, `columns + 1` per row.
    pub fn node_count(&self) -> u32 {
        (u32::from(self.cells[0]) + 1) * (u32::from(self.cells[1]) + 1)
    }
    fn node_grid(&self, node: u32) -> Option<[usize; 2]> {
        let columns = u32::from(self.cells[0]) + 1;
        (node < self.node_count())
            .then(|| [(node % columns) as usize * 3, (node / columns) as usize * 3])
    }
    pub fn node(&self, node: u32) -> Option<Point> {
        self.node_grid(node).map(|[i, j]| self.point(i, j))
    }
    /// The tangent handle on `side` (an index into TANGENT_SIDES), if the
    /// node has a neighbor in that direction.
    pub fn tangent(&self, node: u32, side: u8) -> Option<Point> {
        self.tangent_grid(node, side).map(|[i, j]| self.point(i, j))
    }
    fn tangent_grid(&self, node: u32, side: u8) -> Option<[usize; 2]> {
        let [i, j] = self.node_grid(node)?;
        let [di, dj] = *TANGENT_SIDES.get(usize::from(side))?;
        let [i, j] = [i as i32 + di, j as i32 + dj];
        let height = (self.net.len() / self.width()) as i32;
        (i >= 0 && j >= 0 && i < self.width() as i32 && j < height)
            .then_some([i as usize, j as usize])
    }

    /// Move a node with its tangent handles. The interior points of the
    /// patches around it follow the change in their Coons interior, so edits
    /// keep the patches' own shaping and stay continuous across patches.
    pub fn move_node(&self, node: u32, delta: Point) -> Option<Self> {
        let [ni, nj] = self.node_grid(node)?;
        let mut moved = vec![[ni, nj]];
        moved.extend((0..4).filter_map(|side| self.tangent_grid(node, side)));
        Some(self.edit(&moved, |_| delta))
    }

    /// Place one tangent handle, leaving the node and its other handles.
    pub fn move_tangent(&self, node: u32, side: u8, position: Point) -> Option<Self> {
        let [i, j] = self.tangent_grid(node, side)?;
        let old = self.point(i, j);
        let delta = Point {
            x: position.x - old.x,
            y: position.y - old.y,
        };
        Some(self.edit(&[[i, j]], |_| delta))
    }

    /// Move boundary control points, then shift each touched patch's interior
    /// points by the change in its Coons interior.
    fn edit(&self, points: &[[usize; 2]], delta: impl Fn([usize; 2]) -> Point) -> Self {
        let width = self.width();
        let mut deltas = vec![Point::default(); self.net.len()];
        for p in points {
            deltas[p[1] * width + p[0]] = delta(*p);
        }
        let mut net: Vec<Point> = self.net.to_vec();
        for (point, d) in net.iter_mut().zip(&deltas) {
            point.x += d.x;
            point.y += d.y;
        }
        let touched = |i: usize, j: usize| {
            let [ci, cj] = [i / 3, j / 3];
            let mut cells = vec![[ci, cj]];
            if i % 3 == 0 && ci > 0 {
                cells.push([ci - 1, cj]);
            }
            if j % 3 == 0 && cj > 0 {
                cells.push([ci, cj - 1]);
            }
            if i % 3 == 0 && j % 3 == 0 && ci > 0 && cj > 0 {
                cells.push([ci - 1, cj - 1]);
            }
            cells
        };
        let mut cells: Vec<[usize; 2]> = points.iter().flat_map(|p| touched(p[0], p[1])).collect();
        cells.retain(|c| c[0] < usize::from(self.cells[0]) && c[1] < usize::from(self.cells[1]));
        cells.sort_unstable();
        cells.dedup();
        for [ci, cj] in cells {
            let d = |i: usize, j: usize| deltas[(cj * 3 + j) * width + ci * 3 + i];
            for (i, j, coons) in coons_interior(|i, j| d(i, j)) {
                let point = &mut net[(cj * 3 + j) * width + ci * 3 + i];
                point.x += coons.x;
                point.y += coons.y;
            }
        }
        Self {
            net: net.into(),
            ..self.clone()
        }
    }
}

/// Interior control points (1..=2 in both directions) of the bicubic Coons
/// patch through a patch's boundary control points `b(i, j)`.
fn coons_interior(b: impl Fn(usize, usize) -> Point) -> [(usize, usize, Point); 4] {
    let combine = |terms: &[(f32, usize, usize)]| {
        let mut sum = Point::default();
        for (weight, i, j) in terms {
            let p = b(*i, *j);
            sum.x += weight * p.x / 9.;
            sum.y += weight * p.y / 9.;
        }
        sum
    };
    let corner = |[i0, j0]: [usize; 2], [i1, j1]: [usize; 2]| {
        let (a, far_i, far_j) = ((i0, j0), 3 - i0, 3 - j0);
        combine(&[
            (-4., a.0, a.1),
            (6., i1, j0),
            (6., i0, j1),
            (-2., far_i, j0),
            (-2., i0, far_j),
            (3., far_i, j1),
            (3., i1, far_j),
            (-1., far_i, far_j),
        ])
    };
    [
        (1, 1, corner([0, 0], [1, 1])),
        (2, 1, corner([3, 0], [2, 1])),
        (1, 2, corner([0, 3], [1, 2])),
        (2, 2, corner([3, 3], [2, 2])),
    ]
}

/// A perspective map over the unit square and its first derivatives and
/// twist at (u, v): the numerators and w' are affine in (u, v).
fn jet(map: &Projective, frame: Affine, u: f64, v: f64) -> Option<[[f64; 2]; 4]> {
    let [a, b, c, d, x, y] = frame.0.map(f64::from);
    let m = map.0.map(f64::from);
    let row = |k: usize| {
        let [p, q, r] = [m[k * 3], m[k * 3 + 1], m[k * 3 + 2]];
        let value = p * (a * u + c * v + x) + q * (b * u + d * v + y) + r;
        [value, p * a + q * b, p * c + q * d]
    };
    let [[nx, nxu, nxv], [ny, nyu, nyv], [w, wu, wv]] = [row(0), row(1), row(2)];
    if !(w > 0.) {
        return None;
    }
    let [n, nu, nv] = [[nx, ny], [nxu, nyu], [nxv, nyv]];
    let f = n.map(|v| v / w);
    let fu = std::array::from_fn(|k| (nu[k] * w - n[k] * wu) / (w * w));
    let fv = std::array::from_fn(|k| (nv[k] * w - n[k] * wv) / (w * w));
    let fuv = std::array::from_fn(|k| {
        -(nu[k] * wv + nv[k] * wu) / (w * w) + 2. * n[k] * wu * wv / (w * w * w)
    });
    Some([f, fu, fv, fuv])
}

fn bernstein(t: f32) -> [f32; 4] {
    let s = 1. - t;
    [s * s * s, 3. * s * s * t, 3. * s * t * t, t * t * t]
}

fn net_size(cells: [u16; 2]) -> Option<[usize; 2]> {
    cells
        .iter()
        .all(|c| (1..=MeshMap::MAX_CELLS).contains(c))
        .then(|| cells.map(|c| usize::from(c) * 3 + 1))
}

/// The affine map of the unit square onto `bounds`.
fn unit_frame(bounds: Rect) -> Option<Affine> {
    let size = [bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y];
    let frame = Affine([size[0], 0., 0., size[1], bounds.min.x, bounds.min.y]);
    (size.iter().all(|s| *s > 0. && s.is_finite()) && frame.inverse().is_some()).then_some(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect {
            min: Point { x: x0, y: y0 },
            max: Point { x: x1, y: y1 },
        }
    }
    fn distance(a: Point, b: Point) -> f32 {
        (a.x - b.x).hypot(a.y - b.y)
    }
    fn samples(bounds: Rect) -> impl Iterator<Item = Point> {
        (0..=12).flat_map(move |j| {
            (0..=12).map(move |i| Point {
                x: bounds.min.x + (bounds.max.x - bounds.min.x) * i as f32 / 12.,
                y: bounds.min.y + (bounds.max.y - bounds.min.y) * j as f32 / 12.,
            })
        })
    }

    #[test]
    fn affine_seeds_are_exact_and_post_and_conjugate_stay_exact() {
        let bounds = rect(40., 30., 1040., 830.);
        let affine = Affine::around(
            Point { x: 500., y: 400. },
            [1.3, -0.7],
            0.4,
            Point { x: 20., y: -9. },
        );
        for cells in MeshMap::PRESETS.into_iter().chain([[1, 1], [7, 2]]) {
            let identity = MeshMap::identity(bounds, cells).unwrap();
            let mesh = MeshMap::from_affine(bounds, cells, affine).unwrap();
            assert_eq!(
                mesh.net.len(),
                (cells[0] as usize * 3 + 1) * (cells[1] as usize * 3 + 1)
            );
            assert_eq!(mesh.source(), bounds);
            for p in samples(bounds) {
                assert!(distance(identity.map(p).unwrap(), p) < 2e-3);
                assert!(
                    distance(mesh.map(p).unwrap(), affine.map(p)) < 2e-3,
                    "{cells:?} {p:?}"
                );
            }
            let flip = Affine([-1., 0., 0., 1., 900., 0.]);
            let posted = mesh.post(flip);
            let to = Affine::around(Point::default(), [2., 2.], 0.3, Point { x: 5., y: 7. });
            let moved = mesh.conjugate(to);
            for p in samples(bounds) {
                assert!(distance(posted.map(p).unwrap(), flip.map(affine.map(p))) < 2e-3);
                let expected = to.map(affine.map(p));
                assert!(distance(moved.map(to.map(p)).unwrap(), expected) < 5e-3);
            }
            assert!(
                mesh.map(Point { x: 0., y: 0. }).is_none(),
                "outside the source"
            );
        }
        assert!(MeshMap::identity(bounds, [0, 3]).is_none());
        assert!(MeshMap::identity(bounds, [MeshMap::MAX_CELLS + 1, 3]).is_none());
        assert!(MeshMap::identity(rect(0., 0., 0., 10.), [3, 3]).is_none());
    }

    #[test]
    fn perspective_seeds_hit_every_node_and_stay_near_the_map() {
        let bounds = rect(0., 0., 1200., 800.);
        let quad = [[80., 40.], [1100., 120.], [1180., 760.], [20., 700.]];
        let map = Projective::rect_to_quad(bounds, quad.map(|[x, y]| Point { x, y })).unwrap();
        for (cells, tolerance) in [([3, 3], 1.5), ([4, 4], 0.6), ([8, 8], 0.1)] {
            let mesh = MeshMap::from_projective(bounds, cells, &map).unwrap();
            for node in 0..mesh.node_count() {
                let columns = u32::from(cells[0]) + 1;
                let p = Point {
                    x: (node % columns) as f32 / cells[0] as f32 * 1200.,
                    y: (node / columns) as f32 / cells[1] as f32 * 800.,
                };
                assert!(distance(mesh.node(node).unwrap(), map.map(p).unwrap()) < 1e-2);
            }
            let worst = samples(bounds)
                .map(|p| distance(mesh.map(p).unwrap(), map.map(p).unwrap()))
                .fold(0f32, f32::max);
            assert!(
                worst < tolerance,
                "{cells:?}: {worst}px from the perspective map"
            );
        }
        let affine = Affine::around(Point::default(), [0.5, 2.], 0.3, Point { x: 7., y: 1. });
        let exact =
            MeshMap::from_projective(bounds, [3, 3], &Projective::from_affine(affine)).unwrap();
        for p in samples(bounds) {
            assert!(distance(exact.map(p).unwrap(), affine.map(p)) < 2e-2);
        }
    }

    #[test]
    fn editing_moves_handles_with_nodes_and_keeps_patches_continuous() {
        let bounds = rect(0., 0., 900., 600.);
        let mesh = MeshMap::from_affine(
            bounds,
            [3, 3],
            Affine::around(Point::default(), [1., 1.], 0.2, Point::default()),
        )
        .unwrap();
        let node = 5;
        let delta = Point { x: 37., y: -21. };
        let moved = mesh.move_node(node, delta).unwrap();
        let before = mesh.node(node).unwrap();
        assert!(
            distance(
                moved.node(node).unwrap(),
                Point {
                    x: before.x + delta.x,
                    y: before.y + delta.y
                }
            ) < 1e-4
        );
        for side in 0..4 {
            let [a, b] = [
                mesh.tangent(node, side).unwrap(),
                moved.tangent(node, side).unwrap(),
            ];
            assert!(
                distance(
                    Point {
                        x: a.x + delta.x,
                        y: a.y + delta.y
                    },
                    b
                ) < 1e-4
            );
        }
        for other in (0..mesh.node_count()).filter(|n| *n != node) {
            assert_eq!(mesh.node(other), moved.node(other));
        }
        let corner = mesh.node(0).unwrap();
        assert!(mesh.tangent(0, 2).is_none() && mesh.tangent(0, 3).is_none());
        assert!(
            distance(
                moved.map(Point { x: 300., y: 200. }).unwrap(),
                moved.node(node).unwrap()
            ) < 1e-3
        );
        assert_eq!(moved.map(Point { x: 0., y: 0. }), Some(corner));
        // Starting from an affine mesh, every patch interior is its boundary's
        // Coons interior, before and after the edit.
        let tangent = moved
            .move_tangent(node, 1, Point { x: 350., y: 330. })
            .unwrap();
        for edited in [&moved, &tangent] {
            let width = 10;
            for cj in 0..3 {
                for ci in 0..3 {
                    let at = |i: usize, j: usize| edited.net[(cj * 3 + j) * width + ci * 3 + i];
                    for (i, j, coons) in coons_interior(at) {
                        assert!(
                            distance(at(i, j), coons) < 1e-3,
                            "patch {ci},{cj} point {i},{j}"
                        );
                    }
                }
            }
        }
        assert_eq!(tangent.tangent(node, 1), Some(Point { x: 350., y: 330. }));
        assert_eq!(tangent.tangent(node, 3), moved.tangent(node, 3));
        // Neighbouring patches share their boundary curves: the surface is
        // continuous across every patch edge.
        for edited in [&moved, &tangent] {
            for k in 1..3 {
                for c in 0..3 {
                    for t in 0..=20 {
                        let t = t as f32 / 20.;
                        let [left, right] =
                            [edited.patch([k - 1, c], 1., t), edited.patch([k, c], 0., t)];
                        let [above, below] =
                            [edited.patch([c, k - 1], t, 1.), edited.patch([c, k], t, 0.)];
                        assert!(distance(left, right) < 1e-3 && distance(above, below) < 1e-3);
                    }
                }
            }
        }
        assert!(mesh.move_node(mesh.node_count(), delta).is_none());
        assert!(mesh.move_tangent(node, 4, delta).is_none());
    }

    #[test]
    fn mesh_transforms_bound_conjugate_and_carry_contour_selections() {
        use crate::{ImageTransform, Selection, SelectionPixels, TransformMap};
        let bounds = rect(100., 50., 500., 350.);
        let affine = Affine::around(Point::default(), [1.2, 0.8], 0.25, Point { x: 30., y: 10. });
        let mesh = MeshMap::from_affine(bounds, [3, 3], affine)
            .unwrap()
            .move_node(5, Point { x: 25., y: -15. })
            .unwrap();
        let transform = ImageTransform {
            map: TransformMap::Mesh(Arc::new(mesh.clone())),
            ..Default::default()
        };
        assert!(transform.validate().is_ok() && !transform.is_identity());
        let [hull, drawn] = [mesh.bounds(), transform.forward_bounds(bounds)];
        assert_eq!(drawn, mesh.drawn_bounds());
        assert!(
            drawn.min.x <= hull.min.x - MeshMap::SKIRT
                && drawn.max.y >= hull.max.y + MeshMap::SKIRT
        );
        let mut broken = mesh.clone();
        broken.net = broken.net[1..].into();
        let broken = ImageTransform {
            map: TransformMap::Mesh(Arc::new(broken)),
            ..Default::default()
        };
        assert!(broken.validate().is_err());
        let to = Affine::around(Point::default(), [2., 2.], -0.4, Point { x: 7., y: -3. });
        let TransformMap::Mesh(moved) = transform.conjugate(to).unwrap().map else {
            panic!("mesh")
        };
        for p in samples(bounds) {
            let expected = to.map(mesh.map(p).unwrap());
            assert!(distance(moved.map(to.map(p)).unwrap(), expected) < 5e-3);
        }
        let ring = vec![
            Point { x: 0., y: 100. },
            Point { x: 400., y: 60. },
            Point { x: 450., y: 300. },
            Point { x: 150., y: 320. },
        ];
        let placement = Affine::translation(Point { x: 20., y: 5. });
        let mut selection = Selection::polygon(ring)
            .unwrap()
            .transformed(placement)
            .unwrap();
        selection.inverted = true;
        let mapped = selection.mapped(&transform.map).unwrap();
        assert!(mapped.inverted && mapped.affine == Affine::IDENTITY);
        let [contour] = mapped.contours() else {
            panic!("one ring")
        };
        assert!(
            contour.len() > 800,
            "edges follow the patches pixel by pixel"
        );
        let hull = mesh.bounds();
        assert!(contour.iter().all(|p| p.x >= hull.min.x - 1e-3
            && p.x <= hull.max.x + 1e-3
            && p.y >= hull.min.y - 1e-3
            && p.y <= hull.max.y + 1e-3));
        let corner = mesh.map(Point { x: 420., y: 65. }).unwrap();
        assert!(
            contour.iter().any(|p| distance(*p, corner) < 1e-3),
            "vertices map exactly"
        );
        let outside = Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 50., y: 0. },
            Point { x: 50., y: 40. },
        ])
        .unwrap();
        assert!(
            outside
                .mapped(&transform.map)
                .unwrap()
                .contours()
                .is_empty()
        );
        let pixels = Selection::pixels(Arc::new(
            SelectionPixels::new([8, 1], [0, 0, 8, 1], vec![0x4444]).unwrap(),
        ));
        assert!(pixels.mapped(&transform.map).is_err());
    }

    #[test]
    fn tessellation_follows_the_patches_within_tolerance() {
        let bounds = rect(0., 0., 1200., 800.);
        let quad = [[80., 40.], [1100., 120.], [1180., 760.], [20., 700.]];
        let map = Projective::rect_to_quad(bounds, quad.map(|[x, y]| Point { x, y })).unwrap();
        let mesh = MeshMap::from_projective(bounds, [4, 4], &map)
            .unwrap()
            .move_node(7, Point { x: 60., y: 90. })
            .unwrap();
        for tolerance in [2., 0.5] {
            let t = mesh.tessellate(tolerance);
            assert_eq!(t.positions.len(), t.sources.len());
            for (position, source) in t.positions.iter().zip(&t.sources) {
                assert!(distance(mesh.map(*source).unwrap(), *position) < 1e-3);
            }
            for [a, b, c] in t.triangles() {
                for (p, q) in [(a, b), (b, c), (c, a)] {
                    let [p, q] = [p as usize, q as usize];
                    let chord = Point {
                        x: (t.positions[p].x + t.positions[q].x) / 2.,
                        y: (t.positions[p].y + t.positions[q].y) / 2.,
                    };
                    let source = Point {
                        x: (t.sources[p].x + t.sources[q].x) / 2.,
                        y: (t.sources[p].y + t.sources[q].y) / 2.,
                    };
                    assert!(distance(mesh.map(source).unwrap(), chord) <= tolerance * 1.5);
                }
            }
            assert!(
                t.triangles()
                    .flatten()
                    .all(|v| (v as usize) < t.positions.len())
            );
        }
        let hull = mesh.bounds();
        for p in mesh.tessellate(0.5).positions {
            assert!(
                p.x >= hull.min.x && p.x <= hull.max.x && p.y >= hull.min.y && p.y <= hull.max.y
            );
        }
    }
}
