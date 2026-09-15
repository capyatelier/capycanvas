//! Derived live composition: a complete coarse image and a toroidal cache of
//! visible detail. World tile identity survives pans without copying an image.
use super::*;
use layer_render::ViewState;

// Component ceilings; combined host budgets are qualified separately.
// Dense small composites remain economical.
pub(super) const DENSE_BYTES: u64 = 64 * 1024 * 1024;
// A full-resolution 4096² view needs 256 MiB of Float32 detail plus the coarse
// image, reduction scratch and records (261.33 MiB measured/planned together).
// Keep those overheads inside the bound instead of rejecting the existing 4K
// drawing workload or reducing its display resolution to fit 256 MiB.
pub(super) const CACHE_BYTES: u64 = 272 * 1024 * 1024;

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
        let squares = a * a + b * b + c * c + d * d;
        let scale = ((squares
            + (squares * squares - 4. * determinant * determinant)
                .max(0.)
                .sqrt())
            * 0.5)
            .sqrt();
        let mut level = 0;
        while level < coarse.level && scale * f64::from(1u32 << (level + 1)) <= 1. {
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

pub(super) struct Cache {
    pub coarse: display_mips::Image,
    fine: Option<Fine>,
    window: Option<Window>,
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
            fine: None,
            window: None,
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
            + self.geometry.size()
            + self.fine.as_ref().map_or(0, Fine::bytes)
    }
    fn base_bound(plan: display_mips::Plan) -> u64 {
        // Coarse pixels, scratch mips, both geometry buffers, and all four
        // combinations of immutable full/partial tile reduction records.
        plan.pixel_bytes() + 64 * u64::from(plan.level) + 64
    }
    pub fn detail_view(&self) -> &wgpu::TextureView {
        self.fine.as_ref().map_or(&self.coarse.view, |f| &f.view)
    }
    /// Plan missing display pixels before painting. A limit failure does not
    /// lower the chosen mip or begin an edit that cannot be presented.
    pub fn prepare(
        &mut self,
        r: &mut WgpuRasterizer,
        view: ViewState,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<std::collections::BTreeSet<[u32; 2]>, GpuRasterError> {
        let window = Window::new(view, self.coarse.plan)?;
        if let Some(window) = window {
            let needed = [window.tiles.width(), window.tiles.height()];
            let current = self.fine.as_ref().map_or([0; 2], |f| f.grid);
            let mut grid = std::array::from_fn(|i| needed[i].max(current[i]));
            let bytes_for = |grid: [u32; 2]| {
                u64::from(grid[0]) * u64::from(grid[1]) * u64::from(PAGE_SIZE).pow(2) * 16
                    + Self::base_bound(self.coarse.plan)
            };
            let fits = |grid: [u32; 2]| {
                bytes_for(grid) <= self.limit
                    && grid
                        .into_iter()
                        .all(|v| v * PAGE_SIZE <= r.device.limits().max_texture_dimension_2d)
            };
            // A tall view following a wide view need not retain their bounding
            // rectangle. Reallocate the required shape if growth exceeds budget.
            if !fits(grid) {
                grid = needed;
            }
            let size = grid.map(|v| v * PAGE_SIZE);
            let bytes = bytes_for(grid);
            if !fits(grid) {
                return Err(GpuRasterError::Color(format!(
                    "This view requires {bytes} bytes of display pixels and records; its cache limit is {} bytes",
                    self.limit
                )));
            }
            if self.fine.is_none() || current != grid {
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
                });
            }
        }
        self.window = window;
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
        if let Some(window) = self.window {
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
        self.coarse.write_tile(
            &r.device,
            pipelines,
            encoder,
            source,
            source_origin,
            coordinate,
        )?;
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
