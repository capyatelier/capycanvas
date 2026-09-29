//! Bakes, such as merges and Frequency Separation: a bake composites its
//! members, isolated, into the empty pages of its target, with the sampling of
//! export rather than of the display. A bake keeps no filter images, so when
//! its members' filters would need more than the default image budget they
//! run in bounded windows with their halos, each retired before the next, as
//! the display composes them.
use super::*;

/// Tiles recorded before their jobs are encoded.
const TILES_PER_BATCH: usize = 64;

impl Scene {
    /// Composite `members` into `layer`'s pages within `damage`. Watercolor
    /// members settle into the result, which keeps no wet state.
    #[allow(clippy::too_many_arguments)] // The operation's own operands.
    pub(super) fn bake(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        layer: &Layer,
        members: &[Layer],
        offset: layer_core::Point,
        damage: PixelRect,
        low: Option<&Layer>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let layers = layer_core::bake_layers(members, offset);
        let extent = layer.local_extent(packet.document_extent);
        let source = FramePacket {
            commit_rasters: true,
            view: layer_render::ViewState { background_rgba_linear: [0.; 4], ..packet.view },
            document_extent: extent,
            layers: &layers,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: false,
            composite_all: true,
            time_seconds: packet.time_seconds,
            blend_space: packet.blend_space,
        };
        let pages: Vec<_> = r
            .paint_layers
            .iter()
            .filter(|stored| stored.id == layer.id)
            .flat_map(|stored| &stored.pages)
            .filter(|page| !damage.page_local(page.coordinate).is_empty())
            .map(|page| (page.coordinate, Image {
                texture: page.active().texture.clone(), view: page.active().view.clone(),
                plan: display_mips::Plan::window(extent, 0, page_rect(page.coordinate).intersect(PixelRect::full(extent))),
            }))
            .collect();
        self.placement_display = false;
        self.stop_before = None;
        let budget = match &r.native_edit {
            Some(native) => native.image_pixel_budget(r.scale_display.as_ref().map_or(0, |c| c.resident_bytes())).min(windows::DEFAULT_IMAGE_PIXEL_BYTES),
            None => windows::DEFAULT_IMAGE_PIXEL_BYTES,
        };
        let plan = windows::Plan::new(&layers, extent, budget)?;
        let regions: Vec<_> = plan.map_or_else(|| vec![(damage, Self::capture_window(&layers, damage, extent))], |plan| plan.regions(damage).collect());
        let multiple = regions.len() > 1;
        let result = (|| {
            for (output, window) in regions {
                let pages: Vec<_> = pages.iter().filter(|(c, _)| !output.page_local(*c).is_empty()).collect();
                if pages.is_empty() {
                    continue;
                }
                self.retire_images(|scene| scene.images = images::ImageStages::default());
                self.image_window = Some(window);
                self.update_images(r, source, window, encoder)?;
                self.bake_pages(r, source, &pages, low, encoder)?;
                if plan.is_some() {
                    r.metrics.image_window_peak_bytes = r.metrics.image_window_peak_bytes.max(self.images.storage_bytes());
                    if multiple {
                        Self::submit_chunk(r, encoder, "after bounded bake window")?;
                        r.metrics.image_window_submissions += 1;
                    }
                }
            }
            Ok(())
        })();
        self.image_window = None;
        self.retire_images(|scene| scene.images = images::ImageStages::default());
        result
    }

    /// Composite `pages`, keeping each to the part inside `extent`; the rest
    /// of a page stays transparent.
    fn bake_pages(
        &mut self,
        r: &mut WgpuRasterizer,
        source: FramePacket<'_>,
        pages: &[&([u32; 2], Image)],
        low: Option<&Layer>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        for batch in pages.chunks(TILES_PER_BATCH) {
            self.jobs.clear();
            self.used.fill(false);
            for (coordinate, destination) in batch.iter().copied() {
                let mut output = self.group(r, source, None, *coordinate)?;
                if let Some(low) = low {
                    let view = r.paint_layers.iter().find(|layer| layer.id == low.id)
                        .and_then(|layer| layer.pages.iter().find(|page| page.coordinate == *coordinate))
                        .map(|page| page.active().view.clone());
                    let low = match view { Some(view) => view, None => self.source_tile(r, low, *coordinate)?.unwrap_or_else(|| r.empty_view.clone()) };
                    let detail = self.alloc(r, wgpu::Color::TRANSPARENT);
                    self.draw(r, detail, self.pool[output].view.clone(), Some(low),
                        [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32], [17., 1., 0., 0.], false, Convert::None);
                    self.free(output);
                    output = detail;
                }
                let output = self.converted(r, output, Convert::stored(source));
                self.copy_window_tile(output, destination, *coordinate);
            }
            self.encode_jobs(r, encoder)?;
        }
        Ok(())
    }
}
