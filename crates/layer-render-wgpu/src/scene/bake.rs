//! Merges: a bake composites its members, isolated, into the empty pages of
//! its target, with the sampling of export rather than of the display.
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
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let layers = layer_core::bake_layers(members, offset);
        let extent = layer.local_extent(packet.document_extent);
        let source = FramePacket {
            view: layer_render::ViewState { background_rgba_linear: [0.; 4], ..packet.view },
            document_extent: extent,
            layers: &layers,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: false,
            composite_all: true,
            time_seconds: packet.time_seconds,
        };
        let pages: Vec<_> = r
            .paint_layers
            .iter()
            .filter(|stored| stored.id == layer.id)
            .flat_map(|stored| &stored.pages)
            .filter(|page| !damage.page_local(page.coordinate).is_empty())
            .map(|page| (page.coordinate, page.active().texture.clone()))
            .collect();
        self.placement_display = false;
        self.stop_before = None;
        let window = PixelRect::full(extent);
        self.image_window = Some(window);
        let result = (|| {
            self.update_images(r, source, window, encoder)?;
            for batch in pages.chunks(TILES_PER_BATCH) {
                self.jobs.clear();
                self.used.fill(false);
                for (coordinate, destination) in batch {
                    let output = self.group(r, source, None, *coordinate)?;
                    self.jobs.push(Job::Copy {
                        source: self.pool[output].texture.clone(),
                        source_origin: [0; 2],
                        destination: destination.clone(),
                        origin: [0; 2],
                        width: PAGE_SIZE,
                        height: PAGE_SIZE,
                    });
                    self.free(output);
                }
                self.encode_jobs(r, encoder)?;
            }
            Ok(())
        })();
        self.image_window = None;
        self.images = images::ImageStages::default();
        result
    }
}
