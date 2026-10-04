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
        target: SourceTarget,
        operation: u32,
        snapshot: &Arc<layer_core::SceneSnapshot>,
        scope: &layer_core::SceneScope,
        offset: layer_core::Point,
        damage: PixelRect,
        coverage: &layer_core::CoverageSnapshot,
        low: Option<SourceTarget>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let scene = snapshot.view().with_scope(scope).with_offset(offset);
        let extent = packet.scene.target_extent(target);
        let source = FramePacket {
            commit_rasters: true,
            view: packet.view,
            document_extent: extent,
            scene,
            selection_overlays: None,
            inspect_mask: None,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: false,
            composite_all: true,
            time_seconds: snapshot.context.elapsed,
            blend_space: scene.composition().blend,
        };
        let pages: Vec<_> = r
            .paint_layers
            .iter()
            .filter(|stored| stored.id == target)
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
        let plan = windows::Plan::new(scene, extent, budget)?;
        let regions: Vec<_> = plan.map_or_else(|| vec![(damage, Self::capture_window(scene, damage, extent))], |plan| plan.regions(damage).collect());
        let multiple = regions.len() > 1;
        let analysis = r.bake_analysis_entries(snapshot, scope, offset, extent)?;
        let prepared_masks = r.bake_mask_pages(snapshot, scope, offset, extent)?;
        if let Some(prepared) = prepared_masks {
            let masks = r.layer_masks.snapshots.get_mut(&(target, operation)).expect("prepared bake mask inventory");
            masks.pages = prepared.pages.clone();
            masks._lease = prepared._lease.clone();
        }
        let previous = analysis.map(|entries| std::mem::replace(&mut r.effect_analyses, entries));
        let live_masks = r.layer_masks.bind_snapshot((target, operation));
        let result = (|| {
            for (output, window) in regions {
                let pages: Vec<_> = pages.iter().filter(|(c, _)| !output.page_local(*c).is_empty()).collect();
                if pages.is_empty() {
                    continue;
                }
                self.retire_images(|scene| scene.images = images::ImageStages::default());
                self.image_window = Some(window);
                self.update_images(r, source, window, encoder)?;
                self.bake_pages(r, source, &pages, coverage, (target, operation), low, encoder)?;
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
        if let Some(live) = live_masks { r.layer_masks.restore_snapshot((target, operation), live); }
        if let Some(previous) = previous { r.effect_analyses = previous; }
        self.image_window = None;
        self.retire_images(|scene| scene.images = images::ImageStages::default());
        result
    }

    /// Composite `pages`, keeping each to the part inside `extent`; the rest
    /// of a page stays transparent.
    #[expect(clippy::too_many_arguments, reason = "Bake operands retain independent source, coverage and command ownership")]
    fn bake_pages(
        &mut self,
        r: &mut WgpuRasterizer,
        source: FramePacket<'_>,
        pages: &[&([u32; 2], Image)],
        coverage: &layer_core::CoverageSnapshot,
        command: layer_masks::CommandCoverage,
        low: Option<SourceTarget>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        for batch in pages.chunks(TILES_PER_BATCH) {
            self.jobs.clear();
            self.used.fill(false);
            for (coordinate, destination) in batch.iter().copied() {
                let mut output = self.group(r, source, None, *coordinate)?;
                if let Some(low) = low {
                    let view = r.paint_layers.iter().find(|layer| layer.id == low)
                        .and_then(|layer| layer.pages.iter().find(|page| page.coordinate == *coordinate))
                        .map(|page| page.active().view.clone());
                    let low = match view { Some(view) => view, None => self.source_tile(r, source.scene, low, *coordinate)?.unwrap_or_else(|| r.empty_view.clone()) };
                    let detail = self.alloc(r, wgpu::Color::TRANSPARENT);
                    self.draw(r, detail, self.pool[output].view.clone(), Some(low),
                        [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32], [17., 1., 0., 0.], false, Convert::None);
                    self.free(output);
                    output = detail;
                }
                if coverage.source.default_coverage != 1. || coverage.use_.inverted || coverage.source.initial.is_some()
                    || !coverage.source.raster.is_empty() || !coverage.source.operations.is_empty() {
                    let geometry = layer_core::ImageTransform { placement: layer_core::LayerPlacement::from_projective(
                        coverage.use_.placement.then(layer_core::Projective::from_affine(layer_core::Affine::translation(coverage.use_.translation)))
                            .ok_or(GpuRasterError::InvalidTransform("Invalid bake coverage"))?), ..Default::default() };
                    let mask = self.command_mask_at(r, coverage, geometry, *coordinate, command)?;
                    let clipped = self.alloc(r, wgpu::Color::TRANSPARENT);
                    self.draw(r, clipped, self.pool[output].view.clone(), Some(self.pool[mask].view.clone()),
                        [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32], [3., 1., 0., 0.], false, Convert::None);
                    self.free(output); self.free(mask); output = clipped;
                }
                let output = self.converted(r, output, Convert::stored(source));
                self.copy_window_tile(output, destination, *coordinate);
            }
            self.encode_jobs(r, encoder)?;
        }
        Ok(())
    }
}
