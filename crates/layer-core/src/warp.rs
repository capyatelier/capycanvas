//! Warp meshes: tensor-product cubic Bézier patches over a source rectangle.
use crate::{Affine, Point, Rect, clip_convex};
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
    pub breakpoints: [Arc<[f32]>; 2],
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
    pub fn source_at(&self,point:Point)->Option<Point> {
        if self.positions.len() != self.sources.len() || (u64::from(self.grid[0])+1)*(u64::from(self.grid[1])+1) != self.positions.len() as u64 {return None;}
        let [w,h]=self.grid;let stride=w as usize+1;let mut result=None;
        for y in 0..h as usize {for x in 0..w as usize {
            let a=y*stride+x;let b=a+1;let c=a+stride;let d=c+1;
            for triangle in [[a,b,d],[a,d,c]] {
                let [p,q,r]=triangle.map(|i|self.positions[i]);
                let cross=|a:Point,b:Point|f64::from(a.x)*f64::from(b.y)-f64::from(a.y)*f64::from(b.x);
                let sub=|a:Point,b:Point|Point{x:a.x-b.x,y:a.y-b.y};let denominator=cross(sub(q,p),sub(r,p));
                if denominator==0.{continue;}
                let u=cross(sub(point,p),sub(r,p))/denominator;let v=cross(sub(q,p),sub(point,p))/denominator;
                if u>=0.&&v>=0.&&u+v<=1. {let [a,b,c]=triangle.map(|i|self.sources[i]);result=Some(Point{x:(f64::from(a.x)*(1.-u-v)+f64::from(b.x)*u+f64::from(c.x)*v) as f32,y:(f64::from(a.y)*(1.-u-v)+f64::from(b.y)*u+f64::from(c.y)*v) as f32});}
            }
        }}result
    }
}

/// Tangent handle directions around a node: along the row, down the column,
/// back along the row, and up the column.
const TANGENT_SIDES: [[i32; 2]; 4] = [[1, 0], [0, 1], [-1, 0], [0, -1]];

fn control_unit(breakpoints: &[f32], index: usize) -> f32 {
    let cell = (index / 3).min(breakpoints.len() - 2);
    let fraction = (index - cell * 3) as f64 / 3.;
    (f64::from(breakpoints[cell]) * (1. - fraction) + f64::from(breakpoints[cell + 1]) * fraction) as f32
}

impl MeshMap {
    /// Grid presets, in cells. Three by three cells has four by four nodes.
    pub const PRESETS: [[u16; 2]; 3] = [[3, 3], [4, 4], [5, 5]];
    pub const MAX_CELLS: u16 = 32;
    /// How far a rendered mesh edge extends beyond its patches, in both
    /// destination and source pixels, so the filter can soften it.
    pub const SKIRT: f32 = 2.;

    /// The mesh through `f` at every third of each cell of `bounds`, exact
    /// for maps that are cubic across each cell.
    pub fn fit(bounds: Rect, cells: [u16; 2], f: impl Fn(Point) -> Option<Point>) -> Option<Self> {
        const THIRDS: [[f64; 4]; 4] = [
            [1., 0., 0., 0.],
            [-5. / 6., 3., -1.5, 1. / 3.],
            [1. / 3., -1.5, 3., -5. / 6.],
            [0., 0., 0., 1.],
        ];
        let frame = unit_frame(bounds)?;
        let [width, height] = net_size(cells)?;
        let samples = (0..width * height)
            .map(|n| {
                f(frame.map(Point {
                    x: (n % width) as f32 / (width - 1) as f32,
                    y: (n / width) as f32 / (height - 1) as f32,
                }))
            })
            .collect::<Option<Vec<_>>>()?;
        let mut net = samples.clone();
        for cy in 0..usize::from(cells[1]) {
            for cx in 0..usize::from(cells[0]) {
                let at = |i: usize, j: usize| (3 * cy + j) * width + 3 * cx + i;
                for (j, i) in (0..4).flat_map(|j| (0..4).map(move |i| (j, i))) {
                    let [x, y] = (0..16).fold([0f64; 2], |[x, y], n| {
                        let [k, l] = [n % 4, n / 4];
                        let (w, s) = (THIRDS[j][l] * THIRDS[i][k], samples[at(k, l)]);
                        [x + w * f64::from(s.x), y + w * f64::from(s.y)]
                    });
                    net[at(i, j)] = Point {
                        x: x as f32,
                        y: y as f32,
                    };
                }
            }
        }
        let mesh = Self {
            frame,
            breakpoints: cells.map(|n| (0..=n).map(|i| f32::from(i) / f32::from(n)).collect()),
            net: net.into(),
        };
        mesh.valid().then_some(mesh)
    }
    pub fn identity(bounds: Rect, cells: [u16; 2]) -> Option<Self> {
        Self::from_affine(bounds, cells, Affine::IDENTITY)
    }
    pub fn from_affine(bounds: Rect, cells: [u16; 2], affine: Affine) -> Option<Self> {
        let frame = unit_frame(bounds)?;
        let [width, height] = net_size(cells)?;
        let breakpoints: [Arc<[f32]>; 2] = cells.map(|n| (0..=n).map(|i| f32::from(i) / f32::from(n)).collect());
        let net = (0..width * height).map(|n| {
            let unit = Point { x: control_unit(&breakpoints[0], n % width), y: control_unit(&breakpoints[1], n / width) };
            affine.map(frame.map(unit))
        }).collect();
        let mesh = Self { frame, breakpoints, net };
        mesh.valid().then_some(mesh)
    }

    /// Finite, invertible framing, bounded cells and a net of matching size.
    pub fn valid(&self) -> bool {
        self.breakpoints.iter().all(|b| b.len() >= 2 && b.len() <= usize::from(Self::MAX_CELLS) + 1 && b[0] == 0. && b[b.len()-1] == 1. && b.windows(2).all(|w| w[0].is_finite() && w[1]-w[0] >= 1./65536.))
            && net_size(self.cells()).is_some_and(|[w, h]| w * h == self.net.len())
            && self.frame.inverse().is_some()
            && self.net.iter().all(|p| p.x.is_finite() && p.y.is_finite())
    }

    pub fn source_at(&self, point:Point, tolerance:f32)->Option<Point> {
        if !self.valid() || !tolerance.is_finite() || tolerance<=0. {return None;}
        self.tessellate(tolerance).source_at(point)
    }
    pub fn hit(&self,point:Point,tolerance:f32)->bool {self.source_at(point,tolerance).is_some()}
    pub fn cells(&self) -> [u16; 2] { self.breakpoints.each_ref().map(|b| b.len().saturating_sub(1) as u16) }
    pub fn split(&self, axis: usize, unit: f32) -> Option<Self> {
        let breaks = self.breakpoints.get(axis)?;
        if !self.valid() || !unit.is_finite() || self.cells()[axis] >= Self::MAX_CELLS { return None; }
        let cell = breaks.partition_point(|p| *p < unit).checked_sub(1)?;
        if cell+1 >= breaks.len() || unit-breaks[cell] < 1./65536. || breaks[cell+1]-unit < 1./65536. { return None; }
        let t = (f64::from(unit)-f64::from(breaks[cell]))/(f64::from(breaks[cell+1])-f64::from(breaks[cell]));
        let [w,h] = [self.width(),self.net.len()/self.width()];
        let mut out = vec![Point::default(); if axis==0 {(w+3)*h} else {w*(h+3)}];
        let split = |p: [Point;4]| {
            let mut q = p.map(|p| [f64::from(p.x),f64::from(p.y)]);
            let mut left=[q[0];4];let mut right=[q[3];4];
            for level in 1..4 { for i in 0..4-level { q[i]=std::array::from_fn(|a| q[i][a]*(1.-t)+q[i+1][a]*t); } left[level]=q[0];right[3-level]=q[3-level]; }
            [left[0],left[1],left[2],left[3],right[1],right[2],right[3]].map(|p| Point{x:p[0] as f32,y:p[1] as f32})
        };
        for line in 0..if axis==0 {h} else {w} {
            let at=|n:usize| if axis==0 {line*w+n} else {n*w+line};
            let put=|n:usize| if axis==0 {line*(w+3)+n} else {n*w+line};
            for n in 0..cell*3 {out[put(n)]=self.net[at(n)];}
            for (n,p) in split(std::array::from_fn(|n| self.net[at(cell*3+n)])).into_iter().enumerate(){out[put(cell*3+n)]=p;}
            for n in cell*3+4..if axis==0 {w} else {h} {out[put(n+3)]=self.net[at(n)];}
        }
        let mut result=self.clone();let mut b=breaks.to_vec();b.insert(cell+1,unit);result.breakpoints[axis]=b.into();result.net=out.into();result.valid().then_some(result)
    }
    pub fn is_identity(&self) -> bool {
        self.valid() && self.net.iter().enumerate().all(|(n, point)| {
            *point == self.frame.map(Point { x: control_unit(&self.breakpoints[0], n % self.width()), y: control_unit(&self.breakpoints[1], n / self.width()) })
        })
    }
    pub fn can_refine(&self, cells: [u16; 2]) -> bool {
        self.valid() && net_size(cells).is_some() && (0..2).all(|axis| {
            self.breakpoints[axis].iter().all(|value| (0..=cells[axis]).any(|i| f32::from(i) / f32::from(cells[axis]) == *value))
        })
    }
    pub fn refine(&self, cells: [u16;2]) -> Option<Self> {
        if !self.can_refine(cells) { return None; }
        let mut result=self.clone();
        for (axis, count) in cells.into_iter().enumerate() {for i in 1..count {let unit=f32::from(i)/f32::from(count);if !result.breakpoints[axis].contains(&unit){result=result.split(axis,unit)?;}}}
        (result.cells()==cells).then_some(result)
    }
    pub fn move_nodes(&self, nodes: &std::collections::BTreeSet<u32>, delta: Point) -> Option<Self> {
        if !delta.x.is_finite() || !delta.y.is_finite() {return None;}
        let mut points=std::collections::BTreeSet::new();
        for node in nodes {points.insert(self.node_grid(*node)?);points.extend((0..4).filter_map(|s|self.tangent_grid(*node,s)));}
        let result=self.edit(&points.into_iter().collect::<Vec<_>>(),delta);result.valid().then_some(result)
    }
    pub fn magnification(&self) -> f32 {
        let Some(inverse) = self.frame.inverse() else { return f32::INFINITY; };
        let [a, b, c, d, _, _] = inverse.0.map(f64::from);
        let mut largest = 0f64;
        for row in 0..usize::from(self.cells()[1]) { for column in 0..usize::from(self.cells()[0]) {
            let derivative = |axis: usize, x: usize, y: usize| {
                let p = self.point(column * 3 + x, row * 3 + y);
                let q = self.point(column * 3 + x + usize::from(axis == 0), row * 3 + y + usize::from(axis == 1));
                let index = if axis == 0 { column } else { row };
                let scale = 3. / (f64::from(self.breakpoints[axis][index + 1]) - f64::from(self.breakpoints[axis][index]));
                [(f64::from(q.x) - f64::from(p.x)) * scale, (f64::from(q.y) - f64::from(p.y)) * scale]
            };
            for y in 0..4 { for x in 0..4 {
                let elevate = |axis: usize, index: usize| {
                    let value = |n| if axis == 0 { derivative(axis, n, y) } else { derivative(axis, x, n) };
                    match index {
                        0 => value(0), 3 => value(2),
                        _ => { let before = value(index - 1); let after = value(index); let weight = index as f64 / 3.;
                            std::array::from_fn(|i| before[i] * weight + after[i] * (1. - weight)) }
                    }
                };
                let du = elevate(0, x); let dv = elevate(1, y);
                let [j00, j10, j01, j11] = [du[0] * a + dv[0] * b, du[1] * a + dv[1] * b,
                    du[0] * c + dv[0] * d, du[1] * c + dv[1] * d];
                let norm = ((j00 + j11).hypot(j10 - j01) + (j00 - j11).hypot(j10 + j01)) * 0.5;
                largest = largest.max(norm);
            }}
        }}
        (largest as f32).next_up()
    }
    fn width(&self) -> usize {
        usize::from(self.cells()[0]) * 3 + 1
    }
    fn point(&self, i: usize, j: usize) -> Point {
        self.net[j * self.width() + i]
    }

    /// The destination of a source point inside the mesh's rectangle.
    pub fn map(&self, p: Point) -> Option<Point> {
        let unit = self.frame.inverse()?.map(p);
        let range = -1e-5..=1. + 1e-5;
        (range.contains(&unit.x) && range.contains(&unit.y))
            .then(|| self.evaluate(unit.x.clamp(0., 1.), unit.y.clamp(0., 1.)))
    }

    /// The surface at unit coordinates.
    fn evaluate(&self, u: f32, v: f32) -> Point {
        let locate = |axis: usize, value: f32| {
            let b = &self.breakpoints[axis];
            let cell = b.partition_point(|p| *p <= value).saturating_sub(1).min(b.len()-2);
            (cell, (value-b[cell])/(b[cell+1]-b[cell]))
        };
        let (cu,s) = locate(0,u);
        let (cv,t) = locate(1,v);
        self.patch([cu,cv],s,t)
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
        Rect::around(self.net.iter().copied())
    }

    /// Bounds of everything a renderer draws for the mesh: the control hull
    /// widened by the skirt, which reaches SKIRT destination pixels or SKIRT
    /// source pixels at the steepest rate the patches can reach.
    pub fn drawn_bounds(&self) -> Rect {
        let width = self.width();
        let [a, b, c, d, _, _] = self.frame.0;
        let per_cell = [
            a.hypot(b) * self.breakpoints[0].windows(2).map(|w| w[1]-w[0]).fold(f32::INFINITY,f32::min),
            c.hypot(d) * self.breakpoints[1].windows(2).map(|w| w[1]-w[0]).fold(f32::INFINITY,f32::min),
        ];
        let mut rate = 1f32;
        for (n, p) in self.net.iter().enumerate() {
            for (axis, q) in [
                (0, (n % width + 1 < width).then(|| self.net[n + 1])),
                (1, self.net.get(n + width).copied()),
            ] {
                if let Some(q) = q {
                    rate = rate.max(3. * (q.x - p.x).hypot(q.y - p.y) / per_cell[axis]);
                }
            }
        }
        self.bounds().outset(Self::SKIRT * rate)
    }

    /// Map closed polygons placed by `placement` in source pixels, clipped to
    /// the mesh's rectangle and subdivided by destination curvature.
    pub(crate) fn map_polygons(
        &self,
        polygons: &[Arc<[Point]>],
        placement: Affine,
    ) -> Option<Vec<Arc<[Point]>>> {
        let to_unit = placement.then(self.frame.inverse()?);
        let steps = self.subdivisions(0.25);
        let grid: [Vec<f64>;2]=[0,1].map(|axis|self.breakpoints[axis].windows(2).flat_map(|w|(0..steps[axis]).map(move |i|f64::from(w[0])+(f64::from(w[1])-f64::from(w[0]))*f64::from(i)/f64::from(steps[axis]))).chain(std::iter::once(1.)).collect());
        Some(
            polygons
                .iter()
                .filter_map(|ring| {
                    let mut clipped: Vec<[f64; 2]> = ring
                        .iter()
                        .map(|p| {
                            let unit = to_unit.map(*p);
                            [unit.x, unit.y].map(f64::from)
                        })
                        .collect();
                    for edge in 0..4 {
                        let axis = edge % 2;
                        clipped = clip_convex(&clipped, |p| if edge < 2 { p[axis] } else { 1. - p[axis] });
                    }
                    let mut dense = Vec::with_capacity(clipped.len());
                    for (i, p) in clipped.iter().enumerate() {
                        let q = clipped[(i + 1) % clipped.len()];
                        let mut cuts = vec![0.];
                        for axis in 0..2 {
                            let [a,b]=[p[axis],q[axis]];
                            for k in &grid[axis] {if *k>a.min(b)&&*k<a.max(b){cuts.push((*k-a)/(b-a));}}

                        }
                        cuts.sort_by(f64::total_cmp);
                        cuts.dedup();
                        dense.extend(cuts.into_iter().map(|t| {
                            let [u, v] = [0, 1].map(|axis| (p[axis] + (q[axis] - p[axis]) * t).clamp(0., 1.) as f32);
                            self.evaluate(u, v)
                        }));
                    }
                    (dense.len() >= 3).then(|| dense.into())
                })
                .collect(),
        )
    }

    /// Apply `affine` after the mesh, exactly.
    pub fn post(&self, affine: Affine) -> Self {
        Self {
            net: self.net.iter().map(|p| affine.map(*p)).collect(),
            ..self.clone()
        }
    }

    fn subdivisions(&self, tolerance: f32) -> [u32; 2] {
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
        steps.map(|n| (n.ceil() as u32).clamp(1, 64))
    }

    pub fn tessellation_grid(&self, tolerance: f32) -> [u32; 2] {
        let steps = self.subdivisions(tolerance);
        let cells = self.cells().map(u32::from);
        std::array::from_fn(|axis| cells[axis] * steps[axis].min(1024 / cells[axis].max(1)))
    }

    /// A triangle surface whose vertices lie on the patches, spaced so that
    /// its chords stay within `tolerance` destination pixels of them.
    pub fn tessellate(&self, tolerance: f32) -> Tessellation {
        let [columns, rows] = self.tessellation_grid(tolerance);
        let along = |steps: u32, breaks: &[f32]| -> Vec<(usize, [f32; 4], f32)> {
            let per = steps / (breaks.len() as u32 - 1);
            (0..breaks.len()-1).flat_map(|cell| (0..per).map(move |i| {
                let t = i as f32/per as f32;
                (cell*3,bernstein(t),breaks[cell]+(breaks[cell+1]-breaks[cell])*t)
            })).chain(std::iter::once(((breaks.len()-2)*3,bernstein(1.),1.))).collect()
        };
        let [across, down] = [along(columns, &self.breakpoints[0]), along(rows, &self.breakpoints[1])];
        let vertices = across.len() * down.len();
        let mut tessellation = Tessellation {
            grid: [columns, rows],
            positions: Vec::with_capacity(vertices),
            sources: Vec::with_capacity(vertices),
        };
        let mut curves = vec![[0f32; 2]; self.width()];
        for (row, weights, v) in &down {
            for (i, curve) in curves.iter_mut().enumerate() {
                *curve = weights.iter().enumerate().fold([0.; 2], |sum, (n, w)| {
                    let p = self.point(i, row + n);
                    [sum[0] + w * p.x, sum[1] + w * p.y]
                });
            }
            for (column, weights, u) in &across {
                let [x, y] = weights.iter().enumerate().fold([0.; 2], |sum, (n, w)| {
                    let p = curves[column + n];
                    [sum[0] + w * p[0], sum[1] + w * p[1]]
                });
                tessellation.positions.push(Point { x, y });
                tessellation.sources.push(self.frame.map(Point {
                    x: *u,
                    y: *v,
                }));
            }
        }
        tessellation
    }

    /// Nodes are numbered row by row, `columns + 1` per row.
    pub fn node_count(&self) -> u32 {
        (u32::from(self.cells()[0]) + 1) * (u32::from(self.cells()[1]) + 1)
    }
    fn node_grid(&self, node: u32) -> Option<[usize; 2]> {
        let columns = u32::from(self.cells()[0]) + 1;
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
        let mut moved = vec![self.node_grid(node)?];
        moved.extend((0..4).filter_map(|side| self.tangent_grid(node, side)));
        Some(self.edit(&moved, delta))
    }

    /// Place one tangent handle, leaving the node and its other handles.
    pub fn move_tangent(&self, node: u32, side: u8, position: Point) -> Option<Self> {
        let [i, j] = self.tangent_grid(node, side)?;
        let old = self.point(i, j);
        let delta = Point {
            x: position.x - old.x,
            y: position.y - old.y,
        };
        Some(self.edit(&[[i, j]], delta))
    }

    /// Move boundary control points, then shift every patch's interior
    /// points by the change in its Coons interior.
    fn edit(&self, points: &[[usize; 2]], delta: Point) -> Self {
        let width = self.width();
        let mut deltas = vec![Point::default(); self.net.len()];
        for [i, j] in points {
            deltas[j * width + i] = delta;
        }
        let mut net: Vec<Point> = self
            .net
            .iter()
            .zip(&deltas)
            .map(|(p, d)| Point {
                x: p.x + d.x,
                y: p.y + d.y,
            })
            .collect();
        for cj in 0..usize::from(self.cells()[1]) {
            for ci in 0..usize::from(self.cells()[0]) {
                let at = |i: usize, j: usize| (cj * 3 + j) * width + ci * 3 + i;
                for (i, j, coons) in coons_interior(|i, j| deltas[at(i, j)]) {
                    net[at(i, j)].x += coons.x;
                    net[at(i, j)].y += coons.y;
                }
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
    use crate::Projective;
    use crate::affine::tests::{distance, rect, samples};
    use crate::{ImageTransform, Selection, LayerPlacement};

    #[test]
    fn affine_fits_are_exact_and_post_stays_exact() {
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
            let flip = Affine([-1., 0., 0., 1., 900., 0.]);
            let posted = mesh.post(flip);
            for p in samples(bounds) {
                assert!(distance(identity.map(p).unwrap(), p) < 2e-3);
                assert!(
                    distance(mesh.map(p).unwrap(), affine.map(p)) < 2e-3,
                    "{cells:?} {p:?}"
                );
                assert!(distance(posted.map(p).unwrap(), flip.map(affine.map(p))) < 2e-3);
            }
            assert!(
                mesh.map(Point { x: 0., y: 0. }).is_none(),
                "outside the source"
            );
        }
        assert!(MeshMap::identity(bounds, [0, 3]).is_none());
        assert!(MeshMap::identity(bounds, [MeshMap::MAX_CELLS + 1, 3]).is_none());
        assert!(MeshMap::identity(rect(0., 0., 0., 10.), [3, 3]).is_none());
        assert!(MeshMap::fit(bounds, [3, 3], |p| (p.x < 500.).then_some(p)).is_none());
    }

    #[test]
    fn fits_follow_a_perspective_even_over_a_warp() {
        let bounds = rect(0., 0., 1200., 800.);
        let keystone = [[400., 0.], [800., 0.], [1200., 800.], [0., 800.]].map(|[x, y]| Point { x, y });
        let map = Projective::rect_to_quad(bounds, keystone).unwrap();
        let worst = |mesh: &MeshMap, truth: &dyn Fn(Point) -> Option<Point>| {
            samples(rect(7., 5., 1193., 795.))
                .map(|p| distance(mesh.map(p).unwrap(), truth(p).unwrap()))
                .fold(0f32, f32::max)
        };
        for (cells, tolerance) in [([3, 3], 0.6), ([4, 4], 0.25), ([8, 8], 0.05)] {
            let mesh = MeshMap::fit(bounds, cells, |p| map.map(p)).unwrap();
            let columns = u32::from(cells[0]) + 1;
            for node in 0..mesh.node_count() {
                let p = Point {
                    x: (node % columns) as f32 / cells[0] as f32 * 1200.,
                    y: (node / columns) as f32 / cells[1] as f32 * 800.,
                };
                assert!(distance(mesh.node(node).unwrap(), map.map(p).unwrap()) < 1e-2);
            }
            let error = worst(&mesh, &|p| map.map(p));
            assert!(error < tolerance, "{cells:?}: {error}px from the perspective map");
        }
        let warp = MeshMap::identity(bounds, [3, 3])
            .unwrap()
            .move_node(5, Point { x: 90., y: -60. })
            .unwrap();
        let keystone = Projective::rect_to_quad(warp.bounds(), keystone).unwrap();
        let truth = |p| keystone.map(warp.map(p)?);
        let fitted = MeshMap::fit(bounds, [3, 3], truth).unwrap();
        let pushed = MeshMap {
            net: warp.net.iter().map(|p| keystone.map(*p).unwrap()).collect(),
            ..warp.clone()
        };
        let [fitted, pushed] = [worst(&fitted, &truth), worst(&pushed, &truth)];
        assert!(fitted < 1. && fitted * 20. < pushed, "{fitted}px fitted, {pushed}px pushed");
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
    fn mesh_bounds_reach_the_skirt_and_contours_follow_the_patches() {
        let bounds = rect(100., 50., 500., 350.);
        let affine = Affine::around(Point::default(), [1.2, 0.8], 0.25, Point { x: 30., y: 10. });
        let mesh = MeshMap::from_affine(bounds, [3, 3], affine)
            .unwrap()
            .move_node(5, Point { x: 25., y: -15. })
            .unwrap();
        let [hull, drawn] = [mesh.bounds(), mesh.drawn_bounds()];
        assert!(
            drawn.min.x <= hull.min.x - MeshMap::SKIRT
                && drawn.max.y >= hull.max.y + MeshMap::SKIRT
        );
        let ring = vec![
            Point { x: 0., y: 100. },
            Point { x: 400., y: 60. },
            Point { x: 450., y: 300. },
            Point { x: 150., y: 320. },
        ];
        let placement = Affine::translation(Point { x: 20., y: 5. });
        let map = LayerPlacement { mesh: Some(Arc::new(mesh.clone())), ..Default::default() };
        let selection = Selection::polygon(ring).unwrap().transformed(placement).unwrap();
        let mapped = selection.mapped(&map).unwrap();
        let [contour] = mapped.contours() else {
            panic!("one ring")
        };
        assert!((3..800).contains(&contour.len()));
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
        assert!(outside.mapped(&map).unwrap().contours().is_empty());
    }

    #[test]
    fn mapped_contours_bound_destination_error_without_sampling_every_source_pixel() {
        let bounds = rect(0., 0., 4248., 2832.);
        let identity = MeshMap::identity(bounds, [3, 3]).unwrap();
        let bent = identity.move_node(1, Point { x: 190., y: 270. }).unwrap()
            .move_tangent(5, 1, Point { x: -360., y: 150. }).unwrap();
        let ring = [[0., 0.], [4248., 0.], [4248., 2832.], [0., 2832.]].map(|[x, y]| Point { x, y });
        for mesh in [&identity, &bent] {
            for ring in [ring.to_vec(), vec![ring[0], ring[2], ring[3]]] {
                let selection = Selection::polygon(ring.clone()).unwrap();
                let mapped = selection.mapped(&LayerPlacement { mesh: Some(Arc::new(mesh.clone())), ..Default::default() }).unwrap();
                let contour = &mapped.contours()[0];
                assert!(contour.len() < 1000, "{} segments for four source edges", contour.len());
                if std::ptr::eq(mesh, &identity) { assert!(contour.len() <= 24); }
                for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                    for k in 0..=2000 {
                        let t = k as f32 / 2000.;
                        let p = mesh.map(Point { x: a.x + (b.x - a.x)*t, y: a.y + (b.y - a.y)*t }).unwrap();
                        let error = contour.iter().zip(contour.iter().cycle().skip(1)).map(|(a, b)| {
                            let d = Point { x: b.x - a.x, y: b.y - a.y };
                            let t = (((p.x-a.x)*d.x + (p.y-a.y)*d.y) / (d.x*d.x + d.y*d.y).max(1e-12)).clamp(0., 1.);
                            distance(p, Point { x: a.x+t*d.x, y: a.y+t*d.y })
                        }).fold(f32::INFINITY, f32::min);
                        assert!(error <= 0.26, "mapped contour error {error} at {p:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn rotated_frames_keep_the_skirt_and_selection_of_their_mesh() {
        let bounds = rect(0., 0., 600., 600.);
        let mesh = MeshMap::from_affine(bounds, [3, 3], Affine([3., 0., 0., 3., 0., 0.])).unwrap();
        let to = Affine::around(Point { x: 300., y: 300. }, [1., 1.], 0.7, Point { x: 40., y: -20. });
        let transform = ImageTransform { placement: LayerPlacement { mesh: Some(Arc::new(mesh.clone())), ..Default::default() }, ..Default::default() };
        let moved = transform.conjugate(to).unwrap();
        let rotated = moved.placement.mesh.as_ref().unwrap();
        assert!(Arc::ptr_eq(rotated,transform.placement.mesh.as_ref().unwrap()));
        let reach = |mesh: &MeshMap| mesh.bounds().min.x - mesh.drawn_bounds().min.x;
        assert!((reach(&mesh) - 3. * MeshMap::SKIRT).abs() < 1e-3);
        assert!((reach(rotated) - reach(&mesh)).abs() < 1e-2, "{} != {}", reach(rotated), reach(&mesh));
        let ring = [[100., 100.], [500., 120.], [300., 550.]].map(|[x, y]| Point { x, y });
        let placed = Selection::polygon(ring.map(|p| to.map(p)).to_vec()).unwrap();
        let [contour] = placed.transformed(to.inverse().unwrap()).unwrap().mapped(&moved.placement).unwrap().contours().to_vec().try_into().unwrap();
        for v in ring {
            let expected = to.map(mesh.map(v).unwrap());
            assert!(contour.iter().any(|p| distance(*p, expected) < 1e-2), "{v:?}");
        }
    }

    #[test]
    fn tessellation_follows_the_patches_within_tolerance() {
        let bounds = rect(0., 0., 1200., 800.);
        let quad = [[80., 40.], [1100., 120.], [1180., 760.], [20., 700.]];
        let map = Projective::rect_to_quad(bounds, quad.map(|[x, y]| Point { x, y })).unwrap();
        let mesh = MeshMap::fit(bounds, [4, 4], |p| map.map(p))
            .unwrap()
            .move_node(7, Point { x: 60., y: 90. })
            .unwrap();
        for tolerance in [2., 0.5] {
            let t = mesh.tessellate(tolerance);
            assert_eq!(t.positions.len(), t.sources.len());
            assert_eq!(t.positions.len(), (t.grid[0] as usize + 1) * (t.grid[1] as usize + 1));
            for (position, source) in t.positions.iter().zip(&t.sources) {
                assert!(distance(mesh.map(*source).unwrap(), *position) < 1e-3);
            }
            let width = t.grid[0] as usize + 1;
            for (a, next) in (0..t.positions.len()).flat_map(|a| [(a, 1), (a, width), (a, width + 1)]) {
                let b = a + next;
                if b >= t.positions.len() || (next != width && (a + 1) % width == 0) {
                    continue;
                }
                let middle = |v: &[Point]| Point {
                    x: (v[a].x + v[b].x) / 2.,
                    y: (v[a].y + v[b].y) / 2.,
                };
                let chord = middle(&t.positions);
                assert!(distance(mesh.map(middle(&t.sources)).unwrap(), chord) <= tolerance * 1.5);
            }
        }
        let hull = mesh.bounds();
        for p in mesh.tessellate(0.5).positions {
            assert!(
                p.x >= hull.min.x && p.x <= hull.max.x && p.y >= hull.min.y && p.y <= hull.max.y
            );
        }
    }
}
