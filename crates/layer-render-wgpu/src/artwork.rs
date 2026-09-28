//! Exact artwork queries own bounded captures, independently of presentation.
//! The retained frame contains composition metadata and current preview styles;
//! pixels remain in paint/source backing, never in a second document image.
use super::*;
use layer_render::ViewState;

#[derive(Clone)]
pub(super) struct Frame {
    pub layers: Vec<Layer>,
    pub view: ViewState,
    pub background: [f32; 4],
    pub blend_space: layer_core::BlendSpace,
    pub time: f32,
    pub previews: Vec<DabBatch>,
    preview_dabs: Vec<Dab>,
}
/// Whether two composite snapshots draw the same pixels: the same raster,
/// source, mask, effect, placement and appearance.
pub(super) fn same_layer(a: &Layer, b: &Layer) -> bool {
    a.id == b.id
        && a.kind == b.kind
        && a.visible == b.visible
        && a.opacity == b.opacity
        && a.raster == b.raster
        && a.properties == b.properties
        && a.mask == b.mask
        && a.effect == b.effect
        && match (&a.source, &b.source) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
}

impl Frame {
    /// Selection overlays, navigation and layer labels do not alter raw artwork.
    /// Source identity and raster publication catch edits without scanning pixels.
    pub fn same_artwork(&self, packet: FramePacket<'_>, background: [f32; 4]) -> bool {
        self.placed(packet, background).is_some_and(|moved| moved.is_empty())
    }
    /// The one layer whose placement alone changed since this frame.
    pub fn moved_placement(&self, packet: FramePacket<'_>, background: [f32; 4]) -> Option<LayerId> {
        match self.placed(packet, background)?[..] {
            [layer] => Some(layer),
            _ => None,
        }
    }
    /// Whether nothing but `layer`'s placement changed since this frame.
    pub fn unchanged_except(&self, packet: FramePacket<'_>, background: [f32; 4], layer: LayerId) -> bool {
        self.placed(packet, background).is_some_and(|moved| moved.iter().all(|id| *id == layer))
    }
    /// The layers placed differently, when nothing else about the artwork
    /// changed.
    fn placed(&self, packet: FramePacket<'_>, background: [f32; 4]) -> Option<Vec<LayerId>> {
        let artwork = |l: &&Layer| l.kind != LayerKind::Selection;
        let same = self.background == background
            && self.blend_space == packet.blend_space
            && self.previews.is_empty()
            && packet.dab_batches.is_empty()
            && self.layers.iter().filter(artwork).count()
                == packet.layers.iter().filter(artwork).count();
        let mut moved = Vec::new();
        for (a, b) in self.layers.iter().filter(artwork).zip(packet.layers.iter().filter(artwork)) {
            let same_layer = a.id == b.id
                && a.kind == b.kind
                && a.visible == b.visible
                && a.opacity == b.opacity
                && a.raster == b.raster
                && a.properties
                    == layer_core::LayerProperties {
                        placement: a.properties.placement,
                        offset: a.properties.offset,
                        ..b.properties.clone()
                    }
                && a.mask == b.mask
                && a.effect == b.effect
                && match (&a.source, &b.source) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                };
            if !same_layer {
                return None;
            }
            if a.properties != b.properties {
                moved.push(b.id);
            }
        }
        same.then_some(moved)
    }
    pub fn new(packet: FramePacket<'_>, background: [f32; 4]) -> Self {
        let mut preview_dabs = Vec::new();
        let previews = packet.dab_batches.iter().filter(|b| b.kind == DabBatchKind::Preview)
            .map(|batch| {
                let mut retained = batch.clone();
                retained.first_dab = preview_dabs.len() as u32;
                preview_dabs.extend_from_slice(&packet.dabs[batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize]);
                retained
            }).collect();
        Self {
            layers: packet
                .layers
                .iter()
                .map(Layer::composite_snapshot)
                .collect(),
            view: packet.view,
            background,
            blend_space: packet.blend_space,
            time: packet.time_seconds,
            previews,
            preview_dabs,
        }
    }
    pub fn packet(&self, extent: [u32; 2]) -> FramePacket<'_> {
        FramePacket {
            layers: &self.layers,
            view: self.view,
            document_extent: extent,
            time_seconds: self.time,
            dabs: &self.preview_dabs,
            dab_batches: &self.previews,
            restore_rasters: &[],
            reset_layers: false,
            composite_all: true,
            blend_space: self.blend_space,
        }
    }
}

#[derive(Default)]
pub(super) struct Capture {
    scene: Option<scene::Scene>,
    target: Option<(wgpu::Texture, wgpu::TextureView)>,
    window: Option<PixelRect>,
    pub peak_image_bytes: u64,
    pub image_limit: Option<u64>,
}
impl Capture {
    pub fn source_tile(
        &mut self,
        r: &mut WgpuRasterizer,
        source: &Arc<layer_core::color::source::SourceImage>,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<source_access::RawTile, GpuRasterError> {
        self.scene
            .get_or_insert_with(|| query_scene(r))
            .source_tile_for_query(r, source, coordinate, encoder)
    }
    pub fn storage_bytes(&self) -> u64 {
        self.target.as_ref().map_or(0, |(t, _)| texture_bytes(t))
            + self.scene.as_ref().map_or(0, scene::Scene::scratch_bytes)
    }
    /// The caller consumes this target before requesting another region. Edges
    /// occupy its top-left prefix; no samples outside `region` are meaningful.
    pub fn region(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        region: PixelRect,
        size: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<source_access::RawTile, GpuRasterError> {
        if size.contains(&0) || size[0] > PAGE_SIZE || size[1] > PAGE_SIZE {
            return Err(GpuRasterError::InvalidExtent);
        }
        self.image_bytes(packet, region)?;
        let resized = self
            .target
            .as_ref()
            .is_some_and(|(t, _)| [t.width(), t.height()] != size);
        if resized {
            scene::Scene::submit_chunk(r, encoder, "artwork query target")?;
        }
        if self.target.is_none() || resized {
            self.target = Some(create_color_target(
                &r.device,
                size,
                "bounded artwork query",
            ));
        }
        let (texture, view) = self.target.clone().unwrap();
        self.region_into(r, packet, region, &texture, encoder)?;
        Ok(source_access::RawTile { texture, view })
    }
    /// Capture `region` into the top-left prefix of `destination`, a working
    /// format texture at least as large.
    pub fn region_into(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        region: PixelRect,
        destination: &wgpu::Texture,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if region.width() > destination.width() || region.height() > destination.height() {
            return Err(GpuRasterError::InvalidExtent);
        }
        r.ensure_exact_preview(encoder)?;
        let (window, bytes) = self.image_bytes(packet, region)?;
        // Retire previous image windows before replacing their storage. Exact
        // query scheduling/cancellation will move these drains off interaction.
        if self.retires_window(window) {
            scene::Scene::submit_chunk(r, encoder, "artwork query window")?;
            if let Some(scene) = &mut self.scene {
                scene.release_capture_window(window);
            }
        }
        let scene = self.scene.get_or_insert_with(|| query_scene(r));
        scene.capture_region(r, packet, destination, region, None, encoder)?;
        self.window = Some(window);
        self.peak_image_bytes = self.peak_image_bytes.max(bytes);
        Ok(())
    }
    /// The window capturing `region` reads and the filter images it needs,
    /// refused above the image limit before anything is allocated.
    fn image_bytes(&self, packet: FramePacket<'_>, region: PixelRect) -> Result<(PixelRect, u64), GpuRasterError> {
        let window = scene::Scene::capture_window(packet.layers, region, packet.document_extent);
        let bytes = scene::Scene::capture_image_bound(packet.layers, window);
        let limit = self.image_limit.unwrap_or(256 * 1024 * 1024);
        if bytes > limit {
            return Err(GpuRasterError::Color(format!(
                "Artwork query requires {bytes} bytes of filter images; its limit is {limit} bytes"
            )));
        }
        Ok((window, bytes))
    }
    fn retires_window(&self, window: PixelRect) -> bool {
        self.window.is_some_and(|old| old != window) && self.peak_image_bytes > 0
    }
    /// Whether capturing `region` now could wait for the GPU or upload source
    /// pixels: it needs filter images, retires earlier ones, or decodes tiles.
    pub fn would_block(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, region: PixelRect) -> bool {
        let window = scene::Scene::capture_window(packet.layers, region, packet.document_extent);
        scene::Scene::capture_image_bound(packet.layers, window) > 0
            || self.retires_window(window)
            || page_coordinates(region).any(|tile| self.decodes(r, packet.layers, tile))
    }
    fn decodes(&self, r: &WgpuRasterizer, layers: &[Layer], tile: [u32; 2]) -> bool {
        let Some(scene) = &self.scene else {
            return layers.iter().any(|l| l.visible && (l.source.is_some() || r.native_backing(l.id).is_some()));
        };
        let space = r.document_color().space;
        let resident = |id: LayerId| {
            r.paint_layers.iter().any(|l| l.id == id && l.pages.iter().any(|p| p.coordinate == tile))
        };
        scene.source_decodes(r, layers, tile) > 0
            || scene.uploads_full()
            || layers.iter().filter(|l| l.visible && l.is_artwork()).any(|l| {
                let native = r.native_backing(l.id).is_some()
                    || l.mask.as_ref().is_some_and(|m| r.native_backing(m.id).is_some());
                native
                    && (layer_core::target_transform(layers, l.id) != layer_core::Affine::IDENTITY
                        || l.mask.is_some()
                        || (!resident(l.id)
                            && r.native_color_tile(l.id, tile).map_or(true, |blob| {
                                blob.is_some_and(|blob| scene.prepared_raster_view(&blob, space).is_none())
                            })))
            })
    }
}

impl WgpuRasterizer {
    /// A display prediction owns only coarse pages. Explicit artwork queries
    /// replay its retained contacts through the same exact tile executor, once
    /// per tail. This work is absent from painting/presentation and introduces
    /// no background settling backlog or duplicate permanent preview cache.
    fn ensure_exact_preview(&mut self, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        if self.preview_level == 0 { return Ok(()); }
        let frame = self.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
        let packet = frame.packet(self.document_extent);
        let mut tiles = packet.dab_batches.iter().map(|batch| {
            let dabs = &packet.dabs[batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize];
            brush_tiles::plan(batch, dabs, self.target_extent(batch.layer_id))
        }).collect::<Vec<_>>();
        self.preview_level = 0;
        self.preview_pages.clear();
        self.ensure_preview_pages(self.preview_damage, self.preview_contact_tiles.clone().as_ref());
        self.prepare_uploads(packet, &mut tiles, encoder)?;
        for (index, batch) in packet.dab_batches.iter().enumerate() {
            self.encode_brush_batch(encoder, index, batch, BrushEncodingContext {
                batches: packet.dab_batches, dabs: packet.dabs, tiles: &tiles[index],
                document_extent: self.target_extent(batch.layer_id),
                target: BrushEncodingTarget::Preview { from_persistent: true },
            })?;
        }
        self.refresh_storage_metrics();
        Ok(())
    }
}

fn query_scene(r: &WgpuRasterizer) -> scene::Scene {
    let mut scene = scene::Scene::new(r);
    // An exact query sweeps independent tiles once. Its decoded scratch needs
    // only the bounded upload neighborhood, not the live display's admission.
    scene.admit_native_sources(0);
    scene
}

#[cfg(test)]
mod tests;
