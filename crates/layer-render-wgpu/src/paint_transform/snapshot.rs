//! Immutable originals and bounded source neighborhoods. Unchanged paint
//! surfaces are shared. Before overwriting one, copy it into a reusable snapshot
//! tile; untouched paint and original-photo tiles require no capture allocation.
use super::*;
use pixel_transform::{TRANSFORM_SLOTS, TransformSource, TransformTile};
use std::collections::BTreeMap;

pub(super) struct SnapshotPage {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}
impl SnapshotPage {
    pub fn of(texture: &wgpu::Texture, view: &wgpu::TextureView) -> Self {
        Self { texture: texture.clone(), view: view.clone() }
    }
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
/// A destination rectangle and the source pages its samples read.
pub(crate) struct Footprint {
    pub region: PixelRect,
    pub sources: Vec<[u32; 2]>,
}
impl TileSnapshot {
    pub fn source_bounds(&self) -> [i32; 4] {
        let b = self.bounds;
        [b.min_x() as i32, b.min_y() as i32, b.width() as i32, b.height() as i32]
    }
    pub fn contains(&self, coordinate: [u32; 2]) -> bool {
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
        mesh: Option<std::sync::Arc<super::mesh::MeshGeometry>>,
    ) -> Result<Splitter<impl Fn([u32; 2]) -> bool + '_>, GpuRasterError> {
        Splitter::new(self.bounds, transform, mesh, |c| self.contains(c))
    }
    pub fn binding(
        &self,
        r: &mut WgpuRasterizer,
        pass: &mut PixelTransform,
        sources: &[[u32; 2]],
        selection: Option<&wgpu::Buffer>,
        positions: Option<&wgpu::TextureView>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<TransformSource, GpuRasterError> {
        let mut tiles = Vec::with_capacity(sources.len());
        for c in sources {
            if let Some(tile) = self.original_page(r, *c, encoder)? {
                tiles.push((*c, tile));
            }
        }
        let views: Vec<_> = tiles
            .iter()
            .map(|(c, tile)| TransformTile {
                view: &tile.view,
                origin: c.map(|v| (v * PAGE_SIZE) as i32),
                extent: [PAGE_SIZE; 2],
            })
            .collect();
        let source = pass
            .source_views(
                &r.device,
                &views,
                self.source_bounds(),
                selection,
                positions,
                &r.empty_view,
            )
            .map_err(GpuRasterError::InvalidTransform)?;
        Ok(source)
    }
    /// The original page at `coordinate`, decoded if it is not captured, or
    /// None where the layer has no pixels.
    pub fn original_page(
        &self,
        r: &mut WgpuRasterizer,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<crate::source_access::RawTile>, GpuRasterError> {
        if let Some(page) = self.pages.get(&coordinate) {
            return Ok(Some(crate::source_access::RawTile {
                texture: page.texture.clone(),
                view: page.view.clone(),
            }));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((data, space)) = &self.backing
            && let Some(tile) = data.tiles.get(&layer_core::raster::TileKey {
                plane: layer_core::raster::RasterPlane::Color,
                coordinate,
            })
        {
            let blob = tile.wait_backing().map_err(GpuRasterError::Effect)?;
            return r.backed_raster_tile(&blob, *space, encoder).map(Some);
        }
        match &self.original {
            Some(original) => r.original_source_tile(original, coordinate, encoder),
            None => Ok(None),
        }
    }
}

/// Splits destination rectangles until the unchanged destination and the
/// filter footprint of each piece fit the portable sixteen texture bindings.
pub(crate) struct Splitter<F> {
    map: SourceMap,
    bounds: PixelRect,
    contains: F,
    /// Pieces start and end on multiples of this, unless at the region's edge.
    align: u32,
}
impl<F: Fn([u32; 2]) -> bool> Splitter<F> {
    /// A mesh transform needs its geometry, which bounds each region's source.
    pub fn new(
        bounds: PixelRect,
        transform: &layer_core::ImageTransform,
        mesh: Option<std::sync::Arc<super::mesh::MeshGeometry>>,
        contains: F,
    ) -> Result<Self, GpuRasterError> {
        Ok(Self {
            map: SourceMap::new(transform, bounds, mesh)?,
            bounds,
            contains,
            align: 1,
        })
    }
    /// Keep every split on a multiple of `align` pixels.
    pub fn aligned(self, align: u32) -> Self {
        Self { align, ..self }
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
                if !footprint.is_empty() && required.len() <= self.map.slots {
                    for c in page_coordinates(footprint) {
                        if !required.contains(&c) && (self.contains)(c) {
                            required.push(c);
                        }
                        if required.len() > self.map.slots {
                            break;
                        }
                    }
                }
            }
            if required.len() <= self.map.slots {
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
            } else if region.width() >= region.height() && region.width() > self.align {
                let middle = split_point(region.min_x(), region.max_x(), self.align);
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
            } else if region.height() > self.align {
                let middle = split_point(region.min_y(), region.max_y(), self.align);
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
/// few destination pages as possible, and otherwise on a multiple of `align`.
fn split_point(start: u32, end: u32, align: u32) -> u32 {
    let middle = start + (end - start) / 2;
    let lower = middle / PAGE_SIZE * PAGE_SIZE;
    let aligned = middle / align * align;
    [lower, lower + PAGE_SIZE]
        .into_iter()
        .filter(|b| *b > start && *b < end)
        .min_by_key(|b| b.abs_diff(middle))
        .unwrap_or(if aligned > start {
            aligned
        } else {
            aligned + align
        })
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
    /// Source views a job may bind; a mesh keeps one for its positions.
    slots: usize,
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
    /// Triangles of the tessellated mesh, binned by destination page.
    Mesh(std::sync::Arc<super::mesh::MeshGeometry>),
}
impl SourceMap {
    fn new(
        transform: &layer_core::ImageTransform,
        bounds: PixelRect,
        mesh: Option<std::sync::Arc<super::mesh::MeshGeometry>>,
    ) -> Result<Self, GpuRasterError> {
        let invalid = GpuRasterError::InvalidTransform("Transform must be finite and invertible");
        let support = f64::from(transform.interpolation.support().max(1));
        let inset = if transform.interpolation == layer_core::Interpolation::Nearest {
            0.5
        } else {
            0.
        };
        let slots = TRANSFORM_SLOTS
            - usize::from(matches!(transform.map, layer_core::TransformMap::Mesh(_)));
        let map = |kind| Self {
            kind,
            support,
            inset,
            slots,
        };
        if transform.is_identity() {
            return Ok(map(SourceKind::Identity));
        }
        let Some(projective) = transform.map.projective() else {
            let geometry = mesh.ok_or(GpuRasterError::InvalidTransform("Unsupported transform"))?;
            return Ok(map(SourceKind::Mesh(geometry)));
        };
        if let Some(affine) = projective.as_affine() {
            return Ok(map(SourceKind::Affine(affine.inverse().ok_or(invalid)?.0)));
        }
        let forward = projective.0.map(f64::from);
        let inverse = layer_core::Projective::invert(forward).ok_or(invalid)?;
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
            SourceKind::Mesh(geometry) => geometry.footprint(region).map(|[x0, y0, x1, y1]| {
                [x0 - 1., y0 - 1., x1 + 1., y1 + 1.]
            }),
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
                let visible = layer_core::clip_convex(&corners, |p| weight(p) - floor);
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
                    inside = layer_core::clip_convex(&inside, |p| (p[axis] - edge) * sign);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::preimage;
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

    /// Split 2x2 page blocks as the renderer does. Every job binds at most
    /// TRANSFORM_SLOTS pages, the jobs cover the layer exactly once, and every
    /// tap of a sample anywhere in a pixel lies in a bound page.
    fn check(quad: [[f32; 2]; 4], interpolation: layer_core::Interpolation) -> Vec<Footprint> {
        let bounds = PixelRect::full(EXTENT);
        let (mut transform, h) = perspective(quad);
        transform.interpolation = interpolation;
        let splitter = Splitter::new(bounds, &transform, None, |_| true).unwrap();
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
                        let Some([u, v]) = preimage(h, world) else {
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
        for interpolation in [Nearest, Linear, Bicubic, Lanczos] {
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
            .all(|[x, y]| preimage(h, [*x as f64, *y as f64]).is_none());
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
    fn splits_fall_on_page_boundaries_or_the_alignment() {
        assert_eq!(split_point(0, 512, 1), 256);
        assert_eq!(split_point(250, 760, 1), 512);
        assert_eq!(split_point(0, 300, 1), 256);
        assert_eq!(split_point(10, 200, 1), 105);
        assert_eq!(split_point(300, 1300, 1), 768);
        assert_eq!(split_point(0, 200, 8), 96);
        assert_eq!(split_point(4, 12, 4), 8);
        assert_eq!(split_point(300, 1300, 4), 768);
    }
}

pub(crate) struct DisplayInputs {
    pub level: u32,
    pub image: display_mips::Image,
    pub kept: Option<display_mips::Image>,
    pub sampling: [wgpu::TextureView; 2],
    uniforms: wgpu::Buffer,
    binding: Option<wgpu::BindGroup>,
}
impl DisplayInputs {
    pub fn new(
        r: &WgpuRasterizer,
        level: u32,
        extent: [u32; 2],
        kept: bool,
    ) -> Self {
        let plan = display_mips::Plan::at(extent, level);
        let image = display_mips::Image::with_mips(r, plan, display_mips::MAX_LEVEL);
        let kept = kept.then(|| display_mips::Image::with_mips(r, plan, display_mips::MAX_LEVEL));
        let sampling = [image.sampling_view(), kept.as_ref().unwrap_or(&image).sampling_view()];
        Self {
            level,
            image, kept, sampling,
            uniforms: r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("display resample"),
                size: resample::UNIFORM_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            binding: None,
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.image.storage_bytes() + self.kept.as_ref().map_or(0, display_mips::Image::storage_bytes) + resample::UNIFORM_BYTES
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        r: &mut WgpuRasterizer,
        pass: &resample::Resample,
        encoder: &mut crate::submission::CommandEncoder,
        level: &wgpu::TextureView,
        values: &[u8; resample::UNIFORM_BYTES as usize],
        texels: [u32; 4],
        mesh: Option<&MeshBuffers>,
    ) -> Result<(), GpuRasterError> {
        r.uploads.write(encoder, &self.uniforms, values)?;
        let binding = self.binding.get_or_insert_with(|| pass.mesh_binding(&r.device, &self.uniforms, &self.sampling));
        pass.encode_mesh(encoder, binding, level, texels, mesh, self.kept.is_some());
        Ok(())
    }
}
