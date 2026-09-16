//! Derived live composition: retained zoomed-out levels and a toroidal cache of
//! visible native detail. Filters always run before display reduction.
use super::*;
use layer_render::ViewState;

// Component ceilings; combined host budgets are qualified separately.
// Dense small composites remain economical.
pub(super) const DENSE_BYTES: u64 = 64 * 1024 * 1024;
// A full-resolution 4096² view needs 256 MiB of Float32 detail plus the coarse
// image, reduction scratch and records (261.33 MiB measured/planned together).
// Keep those overheads inside the bound instead of rejecting the existing 4K
// drawing workload or reducing its display resolution to fit 256 MiB.
const DETAIL_BYTES: u64 = 272 * 1024 * 1024;
// Retain the already-computed half-size and smaller Float32 levels of a 60 MP
// photo without taking away the existing 4K detail allowance. This component
// bound is separate from whole-device/process qualification.
pub(super) const CACHE_BYTES: u64 = DETAIL_BYTES + 336 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
struct Window {
    level: u32,
    tiles: PixelRect,
}
impl Window {
    fn new(view: ViewState, coarse: display_mips::Plan) -> Result<Option<Self>, GpuRasterError> {
        let [a, b, c, d, tx, ty] = view.document_to_surface.map(f64::from);
        let determinant = a * d - b * c;
        if ![a, b, c, d, tx, ty].into_iter().all(f64::is_finite)
            || determinant.abs() < 1e-12
            || view.width_px == 0
            || view.height_px == 0
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        // The largest singular value keeps a mip texel no larger than a surface
        // pixel, including reflected, rotated and nonuniform affine views.
        // This equivalent singular-value form avoids subtracting nearly equal
        // fourth powers for ordinary rotation/uniform-scale cameras.
        let scale = ((a + d).hypot(b - c) + (a - d).hypot(b + c)) * 0.5;
        let mut level = 0;
        // The camera arrives as Float32. sin/cos rounding must not flip an exact
        // power-of-two zoom between adjacent levels on every rotation frame.
        // This tolerance is below one millionth of a surface pixel per texel;
        // real zoom changes outside it still select the finer representation.
        let unit = 1. + 4. * f64::from(f32::EPSILON);
        while level < coarse.level && scale * f64::from(1u32 << (level + 1)) <= unit {
            level += 1;
        }
        if level == coarse.level {
            return Ok(None);
        }
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for [x, y] in [
            [0., 0.],
            [f64::from(view.width_px), 0.],
            [0., f64::from(view.height_px)],
            [f64::from(view.width_px), f64::from(view.height_px)],
        ] {
            let p = [
                (d * (x - tx) - c * (y - ty)) / determinant,
                (-b * (x - tx) + a * (y - ty)) / determinant,
            ];
            for i in 0..2 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        let extent = coarse.extent.map(f64::from);
        if (0..2).any(|i| max[i] <= 0. || min[i] >= extent[i]) {
            return Ok(None);
        }
        // Include neighboring samples for bilinear and footprint filtering.
        let pad = f64::from(2u32 << level);
        let span = PAGE_SIZE << level;
        let low = std::array::from_fn::<_, 2, _>(|i| {
            (min[i] - pad).clamp(0., extent[i]).floor() as u32 / span
        });
        let high = std::array::from_fn::<_, 2, _>(|i| {
            ((max[i] + pad).clamp(0., extent[i]).ceil() as u32).div_ceil(span)
        });
        Ok(Some(Self {
            level,
            tiles: PixelRect::new(low[0], low[1], high[0], high[1]),
        }))
    }
}

struct Fine {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    grid: [u32; 2],
    keys: Vec<Option<(u32, [u32; 2])>>,
    pending: Vec<Option<(u32, [u32; 2])>>,
}
impl Fine {
    fn index(&self, coordinate: [u32; 2]) -> usize {
        ((coordinate[1] % self.grid[1]) * self.grid[0] + coordinate[0] % self.grid[0]) as usize
    }
    fn origin(&self, coordinate: [u32; 2]) -> [u32; 2] {
        [
            coordinate[0] % self.grid[0] * PAGE_SIZE,
            coordinate[1] % self.grid[1] * PAGE_SIZE,
        ]
    }
    fn bytes(&self) -> u64 {
        texture_bytes(&self.texture)
    }
}

struct RetainedLevel {
    level: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl RetainedLevel {
    fn new(r: &WgpuRasterizer, plan: display_mips::Plan, budget: u64) -> Vec<Self> {
        // Never create another full-resolution composite. Choose the finest
        // complete pyramid that fits its declared share of the cache. Small
        // explicit test/device budgets retain the existing window-only path.
        let bytes = |level| {
            plan.extent
                .map(|v| u64::from(v.div_ceil(1u32 << level)))
                .into_iter()
                .product::<u64>()
                * 16
        };
        let Some(first) =
            (1..plan.level).find(|&first| (first..plan.level).map(bytes).sum::<u64>() <= budget)
        else {
            return Vec::new();
        };
        (first..plan.level)
            .map(|level| {
                let size = plan.extent.map(|v| v.div_ceil(1 << level));
                let (texture, view) = create_color_target(&r.device, size, "retained display mip");
                Self {
                    level,
                    texture,
                    view,
                }
            })
            .collect()
    }
}

pub(super) struct Cache {
    pub coarse: display_mips::Image,
    retained: Vec<RetainedLevel>,
    retained_level: Option<u32>,
    fine: Option<Fine>,
    window: Option<Window>,
    artwork_changed: bool,
    pub geometry: wgpu::Buffer,
    limit: u64,
}
impl Cache {
    pub fn new(
        r: &WgpuRasterizer,
        pipelines: &display_mips::Pipelines,
        limit: u64,
    ) -> Result<Self, GpuRasterError> {
        let plan = display_mips::Plan::new(r.document_extent)?;
        if Self::base_bound(plan) > limit {
            return Err(GpuRasterError::SizeOverflow);
        }
        Ok(Self {
            coarse: display_mips::Image::new(r, pipelines, plan),
            retained: RetainedLevel::new(r, plan, limit.saturating_sub(DETAIL_BYTES)),
            retained_level: None,
            fine: None,
            window: None,
            artwork_changed: true,
            limit,
            geometry: r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("live display cache geometry"),
                size: 48,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        })
    }
    pub fn storage_bytes(&self) -> u64 {
        self.coarse.storage_bytes()
            + self.retained_bytes()
            + self.geometry.size()
            + self.fine.as_ref().map_or(0, Fine::bytes)
    }
    fn retained_bytes(&self) -> u64 {
        self.retained
            .iter()
            .map(|level| texture_bytes(&level.texture))
            .sum()
    }
    fn selected_retained(&self) -> Option<&RetainedLevel> {
        self.retained_level
            .and_then(|level| self.retained.iter().find(|r| r.level == level))
    }
    fn base_bound(plan: display_mips::Plan) -> u64 {
        // Coarse pixels, scratch mips, both geometry buffers, and all four
        // combinations of immutable full/partial tile reduction records.
        plan.pixel_bytes() + 64 * u64::from(plan.level) + 64
    }
    pub fn detail_view(&self) -> &wgpu::TextureView {
        if let Some(retained) = self.selected_retained() {
            return &retained.view;
        }
        self.fine.as_ref().map_or(&self.coarse.view, |f| &f.view)
    }
    pub fn note_artwork_change(&mut self, changed: bool) {
        self.artwork_changed = changed;
    }
    /// Plan missing display pixels before painting. A limit failure does not
    /// lower the chosen mip or begin an edit that cannot be presented.
    pub fn prepare(
        &mut self,
        r: &mut WgpuRasterizer,
        view: ViewState,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<std::collections::BTreeSet<[u32; 2]>, GpuRasterError> {
        self.artwork_changed = true;
        let requested = Window::new(view, self.coarse.plan)?;
        let retained_level = requested
            .filter(|window| self.retained.iter().any(|r| r.level == window.level))
            .map(|window| window.level);
        let window = requested.filter(|_| retained_level.is_none());
        let prefill_native = self.retained.first().is_some_and(|level| level.level == 1);
        if let Some(fine) = &mut self.fine {
            fine.pending.fill(None);
        }
        // Allocate disposable native detail while the completed photo is being
        // built, rather than on the first zoom-in. Subsequent full composition
        // can seed it with the native pixels it already produced.
        let allocation = window.or_else(|| {
            prefill_native.then_some(Window {
                level: 0,
                tiles: PixelRect::EMPTY,
            })
        });
        if let Some(allocation) = allocation {
            let window = allocation;
            let needed = [window.tiles.width(), window.tiles.height()];
            let current = self.fine.as_ref().map_or([0; 2], |f| f.grid);
            // Reserve the rotation envelope and the sub-octave zoom range when
            // it fits. Otherwise each small growth discards all valid detail.
            // The document bounds cap this reservation for small canvases.
            let side = (((2. * f64::from(view.width_px).hypot(f64::from(view.height_px)) + 4.)
                / f64::from(PAGE_SIZE))
            .ceil() as u32
                + 1)
            .max(if prefill_native { 16 } else { 0 });
            let reserve = self
                .coarse
                .plan
                .extent
                .map(|v| v.div_ceil(PAGE_SIZE << window.level).min(side));
            let mut grid = std::array::from_fn(|i| needed[i].max(current[i]).max(reserve[i]));
            let bytes_for = |grid: [u32; 2]| {
                u64::from(grid[0]) * u64::from(grid[1]) * u64::from(PAGE_SIZE).pow(2) * 16
                    + Self::base_bound(self.coarse.plan)
                    + self.retained_bytes()
            };
            let fits = |grid: [u32; 2]| {
                bytes_for(grid) <= self.limit
                    && grid
                        .into_iter()
                        .all(|v| v * PAGE_SIZE <= r.device.limits().max_texture_dimension_2d)
            };
            let full = self
                .coarse
                .plan
                .extent
                .map(|v| v.div_ceil(PAGE_SIZE << window.level));
            if prefill_native && fits(full) {
                grid = full;
            }
            // A tall view following a wide view need not retain their bounding
            // rectangle. Reallocate the required shape if growth exceeds budget.
            if !fits(grid) {
                grid = std::array::from_fn(|i| needed[i].max(current[i]));
                if !fits(grid) {
                    grid = needed;
                }
            }
            let size = grid.map(|v| v * PAGE_SIZE);
            let bytes = bytes_for(grid);
            if !fits(grid) && requested.is_some_and(|w| Some(w.level) != retained_level) {
                return Err(GpuRasterError::Color(format!(
                    "This view requires {bytes} bytes of display pixels and records; its cache limit is {} bytes",
                    self.limit
                )));
            }
            if !grid.contains(&0) && fits(grid) && (self.fine.is_none() || current != grid) {
                // The cache is disposable. Retire queued use before destroying
                // old storage; retained presenter handles cannot keep it resident.
                if let Some(old) = &self.fine {
                    scene::Scene::submit_chunk(r, encoder, "resize display detail cache")?;
                    old.texture.destroy();
                }
                let (texture, view) =
                    create_color_target(&r.device, size, "visible display detail");
                self.fine = Some(Fine {
                    texture,
                    view,
                    grid,
                    keys: vec![None; (grid[0] * grid[1]) as usize],
                    pending: vec![None; (grid[0] * grid[1]) as usize],
                });
            }
        }
        self.window = window;
        self.retained_level = retained_level;
        let mut missing = std::collections::BTreeSet::new();
        if let Some(window) = window {
            let fine = self.fine.as_ref().unwrap();
            let span = PAGE_SIZE << window.level;
            for y in window.tiles.min_y()..window.tiles.max_y() {
                for x in window.tiles.min_x()..window.tiles.max_x() {
                    let coordinate = [x, y];
                    if fine.keys[fine.index(coordinate)] != Some((window.level, coordinate)) {
                        missing.extend(page_coordinates(PixelRect::new(
                            x * span,
                            y * span,
                            ((x + 1) * span).min(r.document_extent[0]),
                            ((y + 1) * span).min(r.document_extent[1]),
                        )));
                    }
                }
            }
        }
        Ok(missing)
    }
    /// Upload only after independent raster-restoration submissions finish.
    /// Their shared staging-belt completion must not recycle a frame's pending
    /// geometry before that frame itself reaches the queue.
    pub fn geometry_bytes(&self) -> [u8; 48] {
        let mut data = [0; 12];
        data[0] = 1;
        data[1] = 1 << self.coarse.plan.level;
        if let Some(retained) = self.selected_retained() {
            data[2] = 1 << retained.level;
            data[6..8].copy_from_slice(&[retained.texture.width(), retained.texture.height()]);
            data[8..10].copy_from_slice(&[retained.texture.width(), retained.texture.height()]);
        } else if let Some(window) = self.window {
            data[2] = 1 << window.level;
            data[4..8].copy_from_slice(&[
                window.tiles.min_x() * PAGE_SIZE,
                window.tiles.min_y() * PAGE_SIZE,
                window.tiles.max_x() * PAGE_SIZE,
                window.tiles.max_y() * PAGE_SIZE,
            ]);
            data[8..10].copy_from_slice(&self.fine.as_ref().unwrap().grid.map(|v| v * PAGE_SIZE));
        }
        std::array::from_fn(|i| data[i / 4].to_le_bytes()[i % 4])
    }
    pub fn write_tile(
        &mut self,
        r: &WgpuRasterizer,
        pipelines: &display_mips::Pipelines,
        encoder: &mut crate::submission::CommandEncoder,
        source: &wgpu::Texture,
        source_origin: [u32; 2],
        coordinate: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        let native_detail = self.retained.first().is_some_and(|level| level.level == 1);
        // Camera-only misses need native detail, but the completed coarse and
        // retained levels already contain these exact pixels. Do not reduce
        // them again. Edits and constrained caches retain full mip generation.
        if self.artwork_changed || !native_detail {
            self.coarse.write_tile(
                &r.device,
                pipelines,
                encoder,
                source,
                source_origin,
                coordinate,
            )?;
            for retained in &self.retained {
                let span = PAGE_SIZE >> retained.level;
                self.coarse.copy_mip(
                    encoder,
                    retained.level,
                    coordinate,
                    &retained.texture,
                    coordinate.map(|v| v * span),
                );
            }
            // An edit while zoomed out or outside the current detail window must
            // invalidate old detail too. A later camera move cannot reuse it.
            if let Some(fine) = &mut self.fine {
                for key in &mut fine.keys {
                    if key.is_some_and(|(level, tile)| coordinate.map(|v| v >> level) == tile) {
                        *key = None;
                    }
                }
            }
        }
        if native_detail {
            if let Some(fine) = &mut self.fine {
                let visible = |tile: [u32; 2]| {
                    self.window.is_some_and(|window| {
                        tile[0] >= window.tiles.min_x()
                            && tile[0] < window.tiles.max_x()
                            && tile[1] >= window.tiles.min_y()
                            && tile[1] < window.tiles.max_y()
                    })
                };
                let index = fine.index(coordinate);
                let occupant_visible = fine.pending[index]
                    .or(fine.keys[index])
                    .is_some_and(|(_, tile)| visible(tile));
                // Complete scene traversal can wrap the toroidal cache several
                // times. Offscreen seeding must never overwrite visible detail.
                if visible(coordinate) || !occupant_visible {
                    fine.keys[index] = None;
                    let origin = fine.origin(coordinate);
                    let valid: [u32; 2] = std::array::from_fn(|i| {
                        (self.coarse.plan.extent[i] - coordinate[i] * PAGE_SIZE).min(PAGE_SIZE)
                    });
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            origin: wgpu::Origin3d {
                                x: source_origin[0],
                                y: source_origin[1],
                                z: 0,
                            },
                            ..source.as_image_copy()
                        },
                        wgpu::TexelCopyTextureInfo {
                            origin: wgpu::Origin3d {
                                x: origin[0],
                                y: origin[1],
                                z: 0,
                            },
                            ..fine.texture.as_image_copy()
                        },
                        wgpu::Extent3d {
                            width: valid[0],
                            height: valid[1],
                            depth_or_array_layers: 1,
                        },
                    );
                    fine.pending[index] = Some((0, coordinate));
                }
            }
            return Ok(());
        }
        if let Some(window) = self.window {
            let tile = coordinate.map(|v| v >> window.level);
            if tile[0] >= window.tiles.min_x()
                && tile[0] < window.tiles.max_x()
                && tile[1] >= window.tiles.min_y()
                && tile[1] < window.tiles.max_y()
            {
                let fine = self.fine.as_mut().unwrap();
                let index = fine.index(tile);
                fine.keys[index] = None;
                let origin = fine.origin(tile);
                let sub =
                    coordinate.map(|v| (v % (1 << window.level)) * (PAGE_SIZE >> window.level));
                self.coarse.copy_mip(
                    encoder,
                    window.level,
                    coordinate,
                    &fine.texture,
                    [origin[0] + sub[0], origin[1] + sub[1]],
                );
            }
        }
        Ok(())
    }
    /// Publish newly filled slots only after the entire frame was submitted.
    pub fn finish_frame(&mut self) {
        if let Some(fine) = &mut self.fine {
            for (key, pending) in fine.keys.iter_mut().zip(&mut fine.pending) {
                if let Some(written) = pending.take() {
                    *key = Some(written);
                }
            }
        }
        if let Some(window) = self.window {
            let fine = self.fine.as_mut().unwrap();
            for y in window.tiles.min_y()..window.tiles.max_y() {
                for x in window.tiles.min_x()..window.tiles.max_x() {
                    let index = fine.index([x, y]);
                    fine.keys[index] = Some((window.level, [x, y]));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
