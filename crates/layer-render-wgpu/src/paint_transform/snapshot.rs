//! Immutable originals and bounded affine source neighborhoods. Unchanged paint
//! surfaces are shared. Before overwriting one, copy it into a reusable snapshot
//! tile; untouched paint and original-photo tiles require no capture allocation.
use super::*;
use pixel_transform::{TRANSFORM_SLOTS, TransformTile};
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
pub(super) struct RegionJob {
    pub coordinate: [u32; 2],
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
    /// Split output regions until both the unchanged destination and four-tap
    /// source footprints fit the portable sixteen texture bindings.
    pub fn jobs(
        &self,
        transform: layer_core::ImageTransform,
        coordinates: impl Iterator<Item = [u32; 2]>,
        regions: &[PixelRect],
    ) -> Result<Vec<RegionJob>, GpuRasterError> {
        let inverse = transform
            .affine
            .inverse()
            .ok_or(GpuRasterError::InvalidTransform(
                "Transform must be finite and invertible",
            ))?
            .0;
        let mut jobs = Vec::new();
        for coordinate in coordinates {
            let region = regions
                .iter()
                .copied()
                .map(|b| b.intersect(page_rect(coordinate)))
                .fold(PixelRect::EMPTY, PixelRect::union);
            if region.is_empty() {
                continue;
            }
            let mut pending = vec![region];
            while let Some(region) = pending.pop() {
                let mut required = Vec::with_capacity(TRANSFORM_SLOTS + 1);
                if self.contains(coordinate) {
                    required.push(coordinate);
                }
                if transform.affine != layer_core::Affine::IDENTITY {
                    let mut low = [f64::INFINITY; 2];
                    let mut high = [f64::NEG_INFINITY; 2];
                    for x in [region.min_x() as f64 + 0.5, region.max_x() as f64 - 0.5] {
                        for y in [region.min_y() as f64 + 0.5, region.max_y() as f64 - 0.5] {
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
                                low[axis] = low[axis].min(value - error - 1.);
                                high[axis] = high[axis].max(value + error + 1.);
                            }
                        }
                    }
                    let footprint = PixelRect::new(
                        low[0].floor().max(0.) as u32,
                        low[1].floor().max(0.) as u32,
                        high[0].ceil().max(0.) as u32,
                        high[1].ceil().max(0.) as u32,
                    )
                    .intersect(self.bounds);
                    if !footprint.is_empty() {
                        for c in page_coordinates(footprint) {
                            if c != coordinate && self.contains(c) {
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
                    jobs.push(RegionJob {
                        coordinate,
                        region: region.page_local(coordinate),
                        sources: required,
                    });
                } else if region.width() >= region.height() && region.width() > 1 {
                    let middle = region.min_x() + region.width() / 2;
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
                    let middle = region.min_y() + region.height() / 2;
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
        }
        Ok(jobs)
    }
    pub fn binding(
        &self,
        r: &mut WgpuRasterizer,
        pass: &mut PixelTransform,
        job: &RegionJob,
        selection: Option<&wgpu::Buffer>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<TransformSource, GpuRasterError> {
        let mut originals = Vec::new();
        for c in &job.sources {
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
        let views: Vec<_> = job
            .sources
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
