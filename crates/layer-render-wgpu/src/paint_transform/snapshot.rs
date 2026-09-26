//! Immutable originals and bounded affine source neighborhoods. Unchanged paint
//! surfaces are shared. Before overwriting one, copy it into a reusable snapshot
//! tile; untouched paint and original-photo tiles require no capture allocation.
use super::*;
use pixel_transform::{TRANSFORM_SLOTS, TransformSource, TransformTile};
use std::collections::BTreeMap;

pub(super) struct SnapshotPage {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}
pub(super) struct TileSnapshot {
    pub pages: BTreeMap<[u32; 2], SnapshotPage>,
    pub original: Option<std::sync::Arc<layer_core::color::source::SourceImage>>,
    pub backing: Option<(
        std::sync::Arc<layer_core::raster::RasterData>,
        layer_core::color::RgbSpace,
    )>,
    pub bounds: PixelRect,
}
/// A page-local destination region and the source pages its samples read.
pub(crate) struct RegionJob {
    pub region: PixelRect,
    pub sources: Vec<[u32; 2]>,
}
/// A destination rectangle and the source pages its samples read.
pub(crate) struct Footprint {
    pub region: PixelRect,
    pub sources: Vec<[u32; 2]>,
}
impl TileSnapshot {
    pub fn new(
        pages: Vec<([u32; 2], SnapshotPage)>,
        original: Option<std::sync::Arc<layer_core::color::source::SourceImage>>,
        backing: Option<(
            std::sync::Arc<layer_core::raster::RasterData>,
            layer_core::color::RgbSpace,
        )>,
        bounds: PixelRect,
    ) -> Self {
        Self {
            pages: pages.into_iter().collect(),
            original,
            backing,
            bounds,
        }
    }
    fn contains(&self, coordinate: [u32; 2]) -> bool {
        self.pages.contains_key(&coordinate)
            || self.backing.as_ref().is_some_and(|(data, _)| {
                data.tiles.contains_key(&layer_core::raster::TileKey {
                    plane: layer_core::raster::RasterPlane::Color,
                    coordinate,
                })
            })
            || self.original.as_ref().is_some_and(|s| {
                coordinate[0] * PAGE_SIZE < s.extent[0] && coordinate[1] * PAGE_SIZE < s.extent[1]
            })
    }
    pub fn aliases(&self, coordinate: [u32; 2], texture: &wgpu::Texture) -> bool {
        self.pages
            .get(&coordinate)
            .is_some_and(|p| p.texture == *texture)
    }
    pub fn splitter(
        &self,
        transform: &layer_core::ImageTransform,
    ) -> Result<Splitter<impl Fn([u32; 2]) -> bool + '_>, GpuRasterError> {
        Splitter::new(self.bounds, transform, |c| self.contains(c))
    }
    pub fn binding(
        &self,
        r: &mut WgpuRasterizer,
        pass: &mut PixelTransform,
        sources: &[[u32; 2]],
        selection: Option<&wgpu::Buffer>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<TransformSource, GpuRasterError> {
        let mut originals = Vec::new();
        for c in sources {
            if self.pages.contains_key(c) {
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some((data, space)) = &self.backing
                && let Some(tile) = data.tiles.get(&layer_core::raster::TileKey {
                    plane: layer_core::raster::RasterPlane::Color,
                    coordinate: *c,
                })
            {
                let blob = tile.wait_backing().map_err(GpuRasterError::Effect)?;
                originals.push((*c, r.backed_raster_tile(&blob, *space, encoder)?));
                continue;
            }
            if let Some(original) = &self.original {
                if let Some(tile) = r.original_source_tile(original, *c, encoder)? {
                    originals.push((*c, tile));
                }
            }
        }
        let views: Vec<_> = sources
            .iter()
            .filter_map(|c| {
                let view = self.pages.get(c).map(|p| &p.view).or_else(|| {
                    originals
                        .iter()
                        .find(|(at, _)| at == c)
                        .map(|(_, tile)| &tile.view)
                })?;
                Some(TransformTile {
                    view,
                    origin: c.map(|v| (v * PAGE_SIZE) as i32),
                    extent: [PAGE_SIZE; 2],
                })
            })
            .collect();
        let source = pass
            .source_views(
                &r.device,
                &views,
                [
                    self.bounds.min_x() as i32,
                    self.bounds.min_y() as i32,
                    self.bounds.width() as i32,
                    self.bounds.height() as i32,
                ],
                selection,
                &r.empty_view,
            )
            .map_err(GpuRasterError::InvalidTransform)?;
        Ok(source)
    }
}

/// Splits destination rectangles until the unchanged destination and the
/// filter footprint of each piece fit the portable sixteen texture bindings.
pub(crate) struct Splitter<F> {
    map: SourceMap,
    bounds: PixelRect,
    contains: F,
}
impl<F: Fn([u32; 2]) -> bool> Splitter<F> {
    pub fn new(
        bounds: PixelRect,
        transform: &layer_core::ImageTransform,
        contains: F,
    ) -> Result<Self, GpuRasterError> {
        Ok(Self {
            map: SourceMap::new(transform, bounds)?,
            bounds,
            contains,
        })
    }
    /// Append the pieces of `start` and the pages each one reads.
    pub fn split(&self, start: PixelRect, jobs: &mut Vec<Footprint>) -> Result<(), GpuRasterError> {
        let mut pending = vec![start];
        while let Some(region) = pending.pop() {
            if region.is_empty() {
                continue;
            }
            let mut required = Vec::with_capacity(TRANSFORM_SLOTS + 1);
            required.extend(page_coordinates(region).filter(|c| (self.contains)(*c)));
            if let Some([x0, y0, x1, y1]) = self.map.footprint(region) {
                let support = self.map.support;
                let footprint = PixelRect::new(
                    (x0 - support).floor().max(0.) as u32,
                    (y0 - support).floor().max(0.) as u32,
                    (x1 + support).ceil().max(0.) as u32,
                    (y1 + support).ceil().max(0.) as u32,
                )
                .intersect(self.bounds);
                if !footprint.is_empty() && required.len() <= TRANSFORM_SLOTS {
                    for c in page_coordinates(footprint) {
                        if !required.contains(&c) && (self.contains)(c) {
                            required.push(c);
                        }
                        if required.len() > TRANSFORM_SLOTS {
                            break;
                        }
                    }
                }
            }
            if required.len() <= TRANSFORM_SLOTS {
                if jobs.len() == 65_536 {
                    return Err(GpuRasterError::InvalidTransform(
                        "Transform requires too many source regions",
                    ));
                }
                required.sort_unstable();
                jobs.push(Footprint {
                    region,
                    sources: required,
                });
            } else if region.width() >= region.height() && region.width() > 1 {
                let middle = split_point(region.min_x(), region.max_x());
                pending.push(PixelRect::new(
                    middle,
                    region.min_y(),
                    region.max_x(),
                    region.max_y(),
                ));
                pending.push(PixelRect::new(
                    region.min_x(),
                    region.min_y(),
                    middle,
                    region.max_y(),
                ));
            } else if region.height() > 1 {
                let middle = split_point(region.min_y(), region.max_y());
                pending.push(PixelRect::new(
                    region.min_x(),
                    middle,
                    region.max_x(),
                    region.max_y(),
                ));
                pending.push(PixelRect::new(
                    region.min_x(),
                    region.min_y(),
                    region.max_x(),
                    middle,
                ));
            } else {
                return Err(GpuRasterError::InvalidTransform(
                    "Transform exceeds stable Float32 coordinate precision",
                ));
            }
        }
        Ok(())
    }
}

/// Halve a span, on a page boundary when it crosses one, so pieces bind as
/// few destination pages as possible.
fn split_point(start: u32, end: u32) -> u32 {
    let middle = start + (end - start) / 2;
    let lower = middle / PAGE_SIZE * PAGE_SIZE;
    [lower, lower + PAGE_SIZE]
        .into_iter()
        .filter(|b| *b > start && *b < end)
        .min_by_key(|b| b.abs_diff(middle))
        .unwrap_or(middle)
}

/// Shared finite, inverse-mapped neighborhoods for pixel edits and retained placement.
pub(crate) fn region_jobs(
    bounds: PixelRect,
    transform: &layer_core::ImageTransform,
    coordinates: impl Iterator<Item = [u32; 2]>,
    regions: &[PixelRect],
    contains: impl Fn([u32; 2]) -> bool,
) -> Result<Vec<RegionJob>, GpuRasterError> {
    let splitter = Splitter::new(bounds, transform, contains)?;
    let mut found = Vec::new();
    let mut owners = Vec::new();
    for coordinate in coordinates {
        let region = regions
            .iter()
            .copied()
            .map(|b| b.intersect(page_rect(coordinate)))
            .fold(PixelRect::EMPTY, PixelRect::union);
        splitter.split(region, &mut found)?;
        owners.resize(found.len(), coordinate);
    }
    Ok(found
        .into_iter()
        .zip(owners)
        .map(|(job, coordinate)| RegionJob {
            region: job.region.page_local(coordinate),
            sources: job.sources,
        })
        .collect())
}

/// How a destination region reaches back into its source, for choosing the
/// source pages its samples read.
struct SourceMap {
    kind: SourceKind,
    /// Source pixels beyond a sample that its filter reads, at least one to
    /// cover rounding at page edges.
    support: f64,
    /// Samples lie at pixel centers, or anywhere in the pixel when filtered
    /// samples spread over a minified pixel.
    inset: f64,
}
enum SourceKind {
    Identity,
    Affine([f32; 6]),
    /// The destination-to-source matrix, and the smallest w' that can still
    /// reach the source bounds.
    Projective {
        inverse: [f64; 9],
        floor: f64,
        bounds: [f64; 4],
    },
}
impl SourceMap {
    fn new(
        transform: &layer_core::ImageTransform,
        bounds: PixelRect,
    ) -> Result<Self, GpuRasterError> {
        let invalid = GpuRasterError::InvalidTransform("Transform must be finite and invertible");
        let support = f64::from(transform.interpolation.support().max(1));
        let inset = if transform.interpolation == layer_core::Interpolation::Nearest {
            0.5
        } else {
            0.
        };
        let map = |kind| Self {
            kind,
            support,
            inset,
        };
        if transform.is_identity() {
            return Ok(map(SourceKind::Identity));
        }
        let projective = match &transform.map {
            layer_core::TransformMap::Affine(affine) => {
                layer_core::Projective::from_affine(*affine)
            }
            layer_core::TransformMap::Projective(projective) => *projective,
            layer_core::TransformMap::Mesh(_) => {
                return Err(GpuRasterError::InvalidTransform("Unsupported transform"));
            }
        };
        if let Some(affine) = projective.as_affine() {
            return Ok(map(SourceKind::Affine(affine.inverse().ok_or(invalid)?.0)));
        }
        let forward = projective.0.map(f64::from);
        let inverse = invert(forward).ok_or(invalid)?;
        let reach = support + 1.;
        let bounds = [
            f64::from(bounds.min_x()) - reach,
            f64::from(bounds.min_y()) - reach,
            f64::from(bounds.max_x()) + reach,
            f64::from(bounds.max_y()) + reach,
        ];
        let largest = [[0, 1], [2, 1], [2, 3], [0, 3]]
            .map(|[x, y]| forward[6] * bounds[x] + forward[7] * bounds[y] + forward[8])
            .into_iter()
            .fold(f64::NEG_INFINITY, f64::max);
        Ok(map(SourceKind::Projective {
            inverse,
            floor: if largest > 0. {
                1. / largest
            } else {
                f64::INFINITY
            },
            bounds,
        }))
    }

    /// Conservative source bounds of every sample the region's pixels take,
    /// including Float32 evaluation error but not interpolation support.
    fn footprint(&self, region: PixelRect) -> Option<[f64; 4]> {
        let centers = [
            region.min_x() as f64 + self.inset,
            region.min_y() as f64 + self.inset,
            region.max_x() as f64 - self.inset,
            region.max_y() as f64 - self.inset,
        ];
        match &self.kind {
            SourceKind::Identity => None,
            SourceKind::Affine(inverse) => {
                let mut low = [f64::INFINITY; 2];
                let mut high = [f64::NEG_INFINITY; 2];
                for x in [centers[0], centers[2]] {
                    for y in [centers[1], centers[3]] {
                        for axis in 0..2 {
                            let terms = [
                                f64::from(inverse[axis]) * x,
                                f64::from(inverse[axis + 2]) * y,
                                f64::from(inverse[axis + 4]),
                            ];
                            let value = terms.into_iter().sum::<f64>();
                            // Include separate-operation/fused Float32 rounding.
                            let error = terms.into_iter().map(f64::abs).sum::<f64>()
                                * f64::from(f32::EPSILON)
                                * 4.;
                            low[axis] = low[axis].min(value - error);
                            high[axis] = high[axis].max(value + error);
                        }
                    }
                }
                Some([low[0], low[1], high[0], high[1]])
            }
            SourceKind::Projective {
                inverse: m,
                floor,
                bounds,
            } => {
                let weight = |p: [f64; 2]| m[6] * p[0] + m[7] * p[1] + m[8];
                let corners =
                    [[0, 1], [2, 1], [2, 3], [0, 3]].map(|[x, y]| [centers[x], centers[y]]);
                let visible = clip(&corners, |p| weight(p) - floor);
                let nearest = visible
                    .iter()
                    .map(|p| weight(*p))
                    .fold(f64::INFINITY, f64::min);
                if visible.is_empty() || !(nearest > 0.) {
                    return None;
                }
                let mapped: Vec<_> = visible
                    .iter()
                    .map(|p| {
                        let w = weight(*p);
                        [0, 3].map(|row| (m[row] * p[0] + m[row + 1] * p[1] + m[row + 2]) / w)
                    })
                    .collect();
                let [x0, y0, x1, y1] = *bounds;
                let mut inside = mapped;
                for (axis, edge, sign) in [(0, x0, 1.), (1, y0, 1.), (0, x1, -1.), (1, y1, -1.)] {
                    inside = clip(&inside, |p| (p[axis] - edge) * sign);
                }
                if inside.is_empty() {
                    return None;
                }
                // Float32 rows over world positions, for every point mapping
                // inside the source bounds: numerator and denominator error
                // divided by the smallest visible w'.
                let scale = [
                    centers[0].abs().max(centers[2].abs()),
                    centers[1].abs().max(centers[3].abs()),
                    1.,
                ];
                let size = |row: usize| (0..3).map(|i| m[row + i].abs() * scale[i]).sum::<f64>();
                let reach = [x0.abs().max(x1.abs()), y0.abs().max(y1.abs())];
                let error = [0, 1].map(|axis| {
                    (size(axis * 3) + reach[axis] * size(6)) * f64::from(f32::EPSILON) * 4.
                        / nearest
                });
                let low = [0, 1].map(|axis| {
                    inside.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min) - error[axis]
                });
                let high = [0, 1].map(|axis| {
                    inside
                        .iter()
                        .map(|p| p[axis])
                        .fold(f64::NEG_INFINITY, f64::max)
                        + error[axis]
                });
                Some([low[0], low[1], high[0], high[1]])
            }
        }
    }
}

/// Keep the part of a convex polygon where `side` is non-negative.
fn clip(polygon: &[[f64; 2]], side: impl Fn([f64; 2]) -> f64) -> Vec<[f64; 2]> {
    let mut kept = Vec::with_capacity(polygon.len() + 2);
    for (i, p) in polygon.iter().enumerate() {
        let q = polygon[(i + 1) % polygon.len()];
        let [a, b] = [side(*p), side(q)];
        if a >= 0. {
            kept.push(*p);
        }
        if (a >= 0.) != (b >= 0.) {
            let t = a / (a - b);
            kept.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
        }
    }
    kept
}

fn invert(m: [f64; 9]) -> Option<[f64; 9]> {
    let [a, b, c, d, e, f, g, h, i] = m;
    let adjugate = [
        e * i - f * h,
        c * h - b * i,
        b * f - c * e,
        f * g - d * i,
        a * i - c * g,
        c * d - a * f,
        d * h - e * g,
        b * g - a * h,
        a * e - b * d,
    ];
    let determinant = a * adjugate[0] + b * adjugate[3] + c * adjugate[6];
    (determinant != 0. && determinant.is_finite()).then(|| adjugate.map(|v| v / determinant))
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{ImageTransform, Point, Projective, Rect, TransformMap};

    const EXTENT: [u32; 2] = [6000, 4000];

    fn perspective(quad: [[f32; 2]; 4]) -> (ImageTransform, [f64; 9]) {
        let source = Rect {
            min: Point::default(),
            max: Point {
                x: EXTENT[0] as f32,
                y: EXTENT[1] as f32,
            },
        };
        let map = Projective::rect_to_quad(source, quad.map(|[x, y]| Point { x, y })).unwrap();
        let transform = ImageTransform {
            map: TransformMap::Projective(map),
            ..Default::default()
        };
        (transform, map.0.map(f64::from))
    }

    /// Solve H(u, v) = (x, y) directly; None without a preimage where w > 0.
    fn preimage(h: [f64; 9], x: f64, y: f64) -> Option<[f64; 2]> {
        let [a, b, c, d] = [
            h[0] - x * h[6],
            h[1] - x * h[7],
            h[3] - y * h[6],
            h[4] - y * h[7],
        ];
        let [e, f] = [x * h[8] - h[2], y * h[8] - h[5]];
        let det = a * d - b * c;
        let [u, v] = [(e * d - b * f) / det, (a * f - e * c) / det];
        (h[6] * u + h[7] * v + h[8] > 0.).then_some([u, v])
    }

    /// Split 2x2 page blocks as the renderer does. Every job binds at most
    /// TRANSFORM_SLOTS pages, the jobs cover the layer exactly once, and every
    /// tap of a sample anywhere in a pixel lies in a bound page.
    fn check(quad: [[f32; 2]; 4], interpolation: layer_core::Interpolation) -> Vec<Footprint> {
        let bounds = PixelRect::full(EXTENT);
        let (mut transform, h) = perspective(quad);
        transform.interpolation = interpolation;
        let splitter = Splitter::new(bounds, &transform, |_| true).unwrap();
        let mut jobs = Vec::new();
        for y in (0..EXTENT[1]).step_by(512) {
            for x in (0..EXTENT[0]).step_by(512) {
                let block = PixelRect::new(x, y, x + 512, y + 512).intersect(bounds);
                splitter.split(block, &mut jobs).unwrap();
            }
        }
        let reach = interpolation.support() as f64;
        let positions: &[[f64; 2]] = if interpolation == layer_core::Interpolation::Nearest {
            &[[0.5, 0.5]]
        } else {
            &[[0.01, 0.01], [0.99, 0.99], [0.01, 0.99], [0.99, 0.01]]
        };
        let mut area = 0;
        for job in &jobs {
            assert!(job.sources.len() <= TRANSFORM_SLOTS);
            area += job.region.area();
            let [x0, y0] = [job.region.min_x(), job.region.min_y()];
            let [x1, y1] = [job.region.max_x() - 1, job.region.max_y() - 1];
            for x in (x0..=x1).step_by(7).chain([x1]) {
                for y in (y0..=y1).step_by(7).chain([y1]) {
                    for [dx, dy] in positions {
                        let world = [x as f64 + dx, y as f64 + dy];
                        let Some([u, v]) = preimage(h, world[0], world[1]) else {
                            continue;
                        };
                        for [tx, ty] in [[-1., -1.], [1., 1.], [-1., 1.], [1., -1.]] {
                            let [tx, ty] = [(u + tx * reach).floor(), (v + ty * reach).floor()];
                            if tx < 0.
                                || ty < 0.
                                || tx >= EXTENT[0] as f64
                                || ty >= EXTENT[1] as f64
                            {
                                continue;
                            }
                            let page = [tx as u32 / PAGE_SIZE, ty as u32 / PAGE_SIZE];
                            assert!(
                                job.sources.contains(&page),
                                "{quad:?} {interpolation:?}: pixel {world:?} reads {page:?}"
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(area, bounds.area());
        jobs
    }

    #[test]
    fn perspective_jobs_bind_every_sampled_page_within_the_view_limit() {
        use layer_core::Interpolation::*;
        for interpolation in [Nearest, Linear, Bicubic] {
            let keystone = check(
                [[300., 200.], [5700., 400.], [5900., 3900.], [100., 3700.]],
                interpolation,
            );
            let deep = check(
                [[2900., 100.], [3100., 100.], [5990., 3990.], [10., 3990.]],
                interpolation,
            );
            let mirrored = check(
                [[5700., 400.], [300., 200.], [100., 3700.], [5900., 3900.]],
                interpolation,
            );
            for jobs in [&keystone, &deep, &mirrored] {
                assert!(jobs.len() < 4096, "{} jobs", jobs.len());
            }
            assert!(
                keystone.len() < 384,
                "{interpolation:?}: jobs span several pages"
            );
            assert!(
                deep.len() > keystone.len(),
                "foreshortened regions split further"
            );
        }
    }

    #[test]
    fn regions_beyond_the_horizon_bind_only_their_own_pages() {
        let quad = [[2950., 2000.], [3050., 2000.], [5990., 3990.], [10., 3990.]];
        let jobs = check(quad, layer_core::Interpolation::Bicubic);
        let (_, h) = perspective(quad);
        let mut beyond = 0;
        for job in &jobs {
            let r = job.region;
            let unmapped = [
                [r.min_x(), r.min_y()],
                [r.max_x(), r.min_y()],
                [r.min_x(), r.max_y()],
                [r.max_x(), r.max_y()],
            ]
            .iter()
            .all(|[x, y]| preimage(h, *x as f64, *y as f64).is_none());
            if unmapped {
                let mut own: Vec<_> = page_coordinates(r).collect();
                own.sort_unstable();
                assert_eq!(job.sources, own);
                beyond += 1;
            }
        }
        assert!(beyond > 0, "some regions lie wholly beyond the horizon");
    }

    #[test]
    fn splits_fall_on_page_boundaries() {
        assert_eq!(split_point(0, 512), 256);
        assert_eq!(split_point(250, 760), 512);
        assert_eq!(split_point(0, 300), 256);
        assert_eq!(split_point(10, 200), 105);
        assert_eq!(split_point(300, 1300), 768);
    }
}
