//! Derived live composition: retained zoomed-out levels and an atlas of
//! visible detail. Filters always run before display reduction.
use super::*;
use layer_render::ViewState;
use std::collections::{BTreeSet, HashMap};

// Bounded fallback ceilings; a complete display is admitted separately from
// available device headroom. Combined host use is measured independently.
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

impl Window {
    fn visible(self, view: ViewState, extent: [u32; 2]) -> BTreeSet<[u32; 2]> {
        let [a, b, c, d, tx, ty] = view.document_to_surface.map(f64::from);
        let span = PAGE_SIZE << self.level;
        let pad = f64::from(2u32 << self.level);
        let mut visible = BTreeSet::new();
        for y in self.tiles.min_y()..self.tiles.max_y() {
            for x in self.tiles.min_x()..self.tiles.max_x() {
                let low = [f64::from(x * span) - pad, f64::from(y * span) - pad];
                let high = [
                    f64::from(((x + 1) * span).min(extent[0])) + pad,
                    f64::from(((y + 1) * span).min(extent[1])) + pad,
                ];
                let mut min = [f64::INFINITY; 2];
                let mut max = [f64::NEG_INFINITY; 2];
                for [u, v] in [
                    [low[0], low[1]],
                    [high[0], low[1]],
                    [low[0], high[1]],
                    [high[0], high[1]],
                ] {
                    let p = [a * u + c * v + tx, b * u + d * v + ty];
                    for i in 0..2 {
                        min[i] = min[i].min(p[i]);
                        max[i] = max[i].max(p[i]);
                    }
                }
                // The document-axis bounds were tested by Window::new. These
                // two surface-axis projections complete the separating-axis
                // test, including reflection/shear and the sampling halo.
                if max[0] >= 0.
                    && max[1] >= 0.
                    && min[0] <= f64::from(view.width_px)
                    && min[1] <= f64::from(view.height_px)
                {
                    visible.insert([x, y]);
                }
            }
        }
        visible
    }
}

type Key = (u32, [u32; 2]);
struct Fine {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    grid: [u32; 2],
    keys: Vec<Option<Key>>,
    pending: Vec<Option<Key>>,
    owners: Vec<Option<Key>>,
    slots: HashMap<Key, usize>,
    next: usize,
}
impl Fine {
    fn reset_pending(&mut self) {
        self.pending.fill(None);
        self.owners.clone_from(&self.keys);
        self.slots.clear();
        self.slots.extend(
            self.keys
                .iter()
                .enumerate()
                .filter_map(|(i, k)| k.map(|k| (k, i))),
        );
    }
    fn reserve(&mut self, key: Key, level: u32, visible: &BTreeSet<[u32; 2]>) -> Option<usize> {
        if let Some(&index) = self.slots.get(&key) {
            return Some(index);
        }
        for _ in 0..self.keys.len() {
            let index = self.next;
            self.next = (self.next + 1) % self.keys.len();
            if self.owners[index].is_some_and(|(l, c)| l == level && visible.contains(&c)) {
                continue;
            }
            if let Some(old) = self.owners[index].replace(key) {
                self.slots.remove(&old);
            }
            self.slots.insert(key, index);
            self.keys[index] = None;
            self.pending[index] = None;
            return Some(index);
        }
        None
    }
    fn origin(&self, index: usize) -> [u32; 2] {
        let index = index as u32;
        [
            index % self.grid[0] * PAGE_SIZE,
            index / self.grid[0] * PAGE_SIZE,
        ]
    }
    fn bytes(&self) -> u64 {
        texture_bytes(&self.texture)
    }
}

fn atlas_grid(required: u32, capacity: u32, dimension: u32) -> Option<[u32; 2]> {
    (1..=dimension)
        .filter_map(|width| {
            let height = required.div_ceil(width);
            (height > 0 && height <= dimension && width * height <= capacity)
                .then_some([width, height])
        })
        .min_by_key(|[w, h]| (w * h, w.abs_diff(*h)))
}

struct RetainedLevel {
    level: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl RetainedLevel {
    fn new(r: &WgpuRasterizer, plan: display_mips::Plan, budget: u64, complete: bool) -> Vec<Self> {
        // When admitted, one complete pyramid makes every camera view a pure
        // sampling operation. Otherwise retain the finest reduced levels that
        // fit alongside a bounded visible-tile atlas.
        let bytes = |level| {
            plan.extent
                .map(|v| u64::from(v.div_ceil(1u32 << level)))
                .into_iter()
                .product::<u64>()
                * 16
        };
        let Some(first) = (if complete { 0 } else { 1 }..plan.level)
            .find(|&first| {
                plan.extent.into_iter().all(|v|
                    v.div_ceil(1 << first) <= r.device.limits().max_texture_dimension_2d)
                    && (first..plan.level).map(bytes).sum::<u64>() <= budget
            })
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
    visible: BTreeSet<[u32; 2]>,
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
        let pyramid_bytes = (0..plan.level)
            .map(|level| {
                plan.extent
                    .map(|v| u64::from(v.div_ceil(1 << level)))
                    .into_iter()
                    .product::<u64>()
                    * 16
            })
            .sum::<u64>();
        let complete_bytes = pyramid_bytes + Self::base_bound(plan);
        #[cfg(not(target_arch = "wasm32"))]
        let complete = plan.extent.into_iter()
            .all(|v| v <= r.device.limits().max_texture_dimension_2d) && r
            .native_edit
            .as_ref()
            .is_some_and(|native| complete_bytes <= native.display_complete_bytes);
        #[cfg(target_arch = "wasm32")]
        let complete = false;
        let limit = if complete { complete_bytes } else { limit };
        if Self::base_bound(plan) > limit {
            return Err(GpuRasterError::SizeOverflow);
        }
        Ok(Self {
            coarse: display_mips::Image::new(r, pipelines, plan),
            retained: RetainedLevel::new(
                r,
                plan,
                if complete {
                    pyramid_bytes
                } else {
                    limit.saturating_sub(DETAIL_BYTES)
                },
                complete,
            ),
            retained_level: None,
            fine: None,
            window: None,
            visible: BTreeSet::new(),
            artwork_changed: true,
            limit,
            geometry: r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("live display cache geometry"),
                size: Self::geometry_size(plan),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
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
        plan.pixel_bytes() + 64 * u64::from(plan.level) + 16 + Self::geometry_size(plan)
    }
    fn geometry_size(plan: display_mips::Plan) -> u64 {
        let pages = plan
            .extent
            .map(|v| u64::from(v.div_ceil(PAGE_SIZE)))
            .into_iter()
            .product::<u64>();
        (48 + pages * 4).div_ceil(16) * 16
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
        if self.retained.first().is_some_and(|level| level.level == 0) {
            self.retained_level = requested.map(|window| window.level);
            // Complete levels need no page assignment, cache growth, eviction,
            // source decode or filter evaluation for any unchanged camera view.
            return Ok(BTreeSet::new());
        }
        let retained_level = requested
            .filter(|window| self.retained.iter().any(|r| r.level == window.level))
            .map(|window| window.level);
        let window = requested.filter(|_| retained_level.is_none());
        let visible =
            window.map_or_else(BTreeSet::new, |w| w.visible(view, self.coarse.plan.extent));
        let required = visible.len() as u32;
        let page_bytes = u64::from(PAGE_SIZE).pow(2) * 16;
        let base = Self::base_bound(self.coarse.plan);
        let dimension = r.device.limits().max_texture_dimension_2d / PAGE_SIZE;
        // Optional complete mips yield their memory to visible detail. Never
        // relax the component limit, lower resolution, or narrow Float32 pixels.
        // Keep the most useful (finest) complete levels for as long as possible.
        let mut keep = self.retained.len();
        let mut retained_bytes = self.retained_bytes();
        let capacity = loop {
            let capacity = self.limit.saturating_sub(base + retained_bytes) / page_bytes;
            if required == 0 || atlas_grid(required, capacity as u32, dimension).is_some() {
                break capacity as u32;
            }
            if keep == 0 {
                return Err(GpuRasterError::Color(format!(
                    "This view requires {} visible display tiles; its cache limit is {} bytes",
                    required, self.limit
                )));
            }
            keep -= 1;
            retained_bytes -= texture_bytes(&self.retained[keep].texture);
        };
        let prefill_native = self.retained.first().is_some_and(|level| level.level == 1);
        let current = self.fine.as_ref().map_or(0, |f| f.keys.len() as u32);
        let side = (((2. * f64::from(view.width_px).hypot(f64::from(view.height_px)) + 4.)
            / f64::from(PAGE_SIZE))
        .ceil() as u32
            + 1)
        .max(if prefill_native { 16 } else { 0 });
        let level = window.map_or(0, |w| w.level);
        let full = self
            .coarse
            .plan
            .extent
            .map(|v| v.div_ceil(PAGE_SIZE << level));
        let reserve = full.map(|v| v.min(side)).into_iter().product::<u32>();
        let desired = if prefill_native && level == 0 && full[0] * full[1] <= capacity {
            current.max(full[0] * full[1])
        } else {
            required.max(current.max(reserve).min(capacity))
        };
        let grid = if desired > 0 {
            atlas_grid(desired, capacity, dimension)
                .or_else(|| atlas_grid(required.max(1), capacity, dimension))
        } else {
            None
        };
        let resize = grid.is_some_and(|grid| self.fine.as_ref().is_none_or(|f| f.grid != grid));
        if keep != self.retained.len() || (resize && self.fine.is_some()) {
            scene::Scene::submit_chunk(r, encoder, "resize display atlas")?;
        }
        for old in self.retained.drain(keep..) {
            old.texture.destroy();
        }
        if let Some(grid) = grid.filter(|_| resize) {
            let (texture, view) = create_color_target(
                &r.device,
                grid.map(|v| v * PAGE_SIZE),
                "visible display atlas",
            );
            let slots = (grid[0] * grid[1]) as usize;
            let mut fine = Fine {
                texture,
                view,
                grid,
                keys: vec![None; slots],
                pending: vec![None; slots],
                owners: vec![None; slots],
                slots: HashMap::new(),
                next: 0,
            };
            if let Some(old) = &self.fine {
                // A larger viewport may need more slots, but its existing
                // completed pixels remain valid. Copy them on the GPU instead
                // of turning growth into full-resolution filter regeneration.
                // Only replacement overlaps the two bounded atlas allocations;
                // retire the old one before composing or presenting this frame.
                for (index, &key) in old.keys.iter().enumerate().take(slots) {
                    if key.is_none() {
                        continue;
                    }
                    let from = old.origin(index);
                    let to = fine.origin(index);
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            origin: wgpu::Origin3d {
                                x: from[0],
                                y: from[1],
                                z: 0,
                            },
                            ..old.texture.as_image_copy()
                        },
                        wgpu::TexelCopyTextureInfo {
                            origin: wgpu::Origin3d {
                                x: to[0],
                                y: to[1],
                                z: 0,
                            },
                            ..fine.texture.as_image_copy()
                        },
                        wgpu::Extent3d {
                            width: PAGE_SIZE,
                            height: PAGE_SIZE,
                            depth_or_array_layers: 1,
                        },
                    );
                    fine.keys[index] = key;
                }
                scene::Scene::submit_chunk(r, encoder, "preserve display atlas pixels")?;
                old.texture.destroy();
            }
            self.fine = Some(fine);
        }
        self.window = window;
        self.visible = visible;
        self.retained_level = retained_level;
        let mut missing = BTreeSet::new();
        if let Some(fine) = &mut self.fine {
            fine.reset_pending();
            if let Some(window) = window {
                let span = PAGE_SIZE << window.level;
                for &coordinate in &self.visible {
                    let key = (window.level, coordinate);
                    let index = fine
                        .reserve(key, window.level, &self.visible)
                        .expect("visible atlas slots were budgeted");
                    if fine.keys[index] != Some(key) {
                        missing.extend(page_coordinates(PixelRect::new(
                            coordinate[0] * span,
                            coordinate[1] * span,
                            ((coordinate[0] + 1) * span).min(r.document_extent[0]),
                            ((coordinate[1] + 1) * span).min(r.document_extent[1]),
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
    pub fn geometry_bytes(&self) -> Vec<u8> {
        let mut data = vec![0u32; self.geometry.size() as usize / 4];
        data[0] = 1;
        data[1] = 1 << self.coarse.plan.level;
        if let Some(retained) = self.selected_retained() {
            data[2] = 1 << retained.level;
            data[6..8].copy_from_slice(&[retained.texture.width(), retained.texture.height()]);
            data[8..10].copy_from_slice(&[retained.texture.width(), retained.texture.height()]);
        } else if let Some(window) = self.window {
            data[2] = 1 << window.level;
            data[3] = self.coarse.plan.extent[0].div_ceil(PAGE_SIZE << window.level);
            data[4..8].copy_from_slice(&[
                window.tiles.min_x() * PAGE_SIZE,
                window.tiles.min_y() * PAGE_SIZE,
                window.tiles.max_x() * PAGE_SIZE,
                window.tiles.max_y() * PAGE_SIZE,
            ]);
            let fine = self.fine.as_ref().unwrap();
            data[8..10].copy_from_slice(&fine.grid.map(|v| v * PAGE_SIZE));
            data[10] = 1;
            for &coordinate in &self.visible {
                let index = fine.slots[&(window.level, coordinate)];
                let offset = (coordinate[1] * data[3] + coordinate[0]) as usize;
                data[12 + offset] = index as u32 + 1;
            }
        }
        data.into_iter().flat_map(u32::to_le_bytes).collect()
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
        let reduced_detail = self.window.is_some_and(|window| window.level > 0);
        if self.artwork_changed || reduced_detail {
            self.coarse.write_tile(
                &r.device,
                pipelines,
                encoder,
                source,
                source_origin,
                coordinate,
            )?;
            if self.artwork_changed {
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
            }
        }
        if self.artwork_changed
            && let Some(fine) = &mut self.fine
        {
            for key in &mut fine.keys {
                if key.is_some_and(|(level, tile)| coordinate.map(|v| v >> level) == tile) {
                    *key = None;
                }
            }
        }
        if let Some(window) = self.window.filter(|w| w.level > 0) {
            let tile = coordinate.map(|v| v >> window.level);
            if self.visible.contains(&tile) {
                let fine = self.fine.as_mut().unwrap();
                let index = fine.slots[&(window.level, tile)];
                fine.keys[index] = None;
                let origin = fine.origin(index);
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
        } else if let Some(fine) = &mut self.fine {
            // Seed already-produced native pixels without evicting any tile
            // needed by this view. Owners include unsubmitted reservations.
            if let Some(index) = fine.reserve((0, coordinate), 0, &self.visible) {
                fine.keys[index] = None;
                let origin = fine.origin(index);
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
            for &coordinate in &self.visible {
                let key = (window.level, coordinate);
                let index = fine.slots[&key];
                fine.keys[index] = Some(key);
            }
        }
    }
}

#[cfg(test)]
mod tests;
