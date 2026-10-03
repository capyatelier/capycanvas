use super::*;

impl Cache {
    pub(in crate::scene) fn render_native(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let Encoding { encoder, commands } = encoding;
        let mut missing = self.invalidate_native(scene, r, packet, dirty, encoder, commands, tiles)?;
        if let Some(overview) = &mut self.overview {
            missing.extend(overview.invalidate_native(scene, r, packet, dirty, encoder, commands, tiles)?);
        }
        if missing.is_empty() { return Ok(PixelRect::EMPTY); }
        let changed = missing.iter().fold(PixelRect::EMPTY, |a, c| a.union(page_rect(*c))).intersect(PixelRect::full(packet.document_extent));
        let budget = r.native_edit.as_ref().map_or(windows::DEFAULT_IMAGE_PIXEL_BYTES, |n| n.image_pixel_budget(self.resident_bytes()));
        let plan = windows::Plan::new(packet.layers, packet.document_extent, budget)?;
        let regions: Vec<_> = plan.map_or_else(|| vec![(changed, PixelRect::full(packet.document_extent))], |p| p.regions(changed).collect());
        let image = self.exact_tile.get_or_insert_with(|| Image::new(r,
            display_mips::Plan::at([PAGE_SIZE; 2], 0), "exact composition working tile"));
        let mut image = image.clone();
        let (texture, view) = (image.texture.clone(), image.view.clone());
        if plan.is_some() {
            Scene::submit_chunk(r, encoder, "before native filter windows")?;
            r.metrics.image_window_submissions += 1;
        }
        for (output, window) in regions {
            scene.prepare_region(r, packet, window, dirty, encoder)?;
            if plan.is_some() {
                r.metrics.image_window_peak_bytes = r.metrics.image_window_peak_bytes.max(scene.images.storage_bytes());
            }
            for coordinate in page_coordinates(output).filter(|c| missing.contains(c)) {
                let region = page_rect(coordinate).intersect(PixelRect::full(packet.document_extent));
                image.plan = display_mips::Plan::window(packet.document_extent, 0, region);
                scene.capture_prepared_region(r, packet, &image, region, scene::Output::Display, encoder)?;
                if let Some(hierarchy) = &mut self.hierarchy { hierarchy.write(encoder, &texture, coordinate, region); }
                self.write_exact(r, encoder, commands, &view, coordinate, region)?;
                if let Some(overview) = &mut self.overview { overview.write_exact(r, encoder, commands, &view, coordinate, region)?; }
                r.metrics.composited_pixels += region.area();
            }
            if let Some(hierarchy) = &mut self.hierarchy { hierarchy.flush(encoder); }
            if plan.is_some() {
                Scene::submit_chunk(r, encoder, "after native filter window")?;
                r.metrics.image_window_submissions += 1;
                scene.retire_images(|scene| scene.images.release_window_pixels());
                scene.image_window = None;
            }
        }
        Ok(changed)
    }

    fn invalidate_native(
        &mut self, scene: &Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>, dirty: PixelRect,
        encoder: &mut crate::submission::CommandEncoder, commands: &mut Commands, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<BTreeSet<[u32; 2]>, GpuRasterError> {
        if self.reuse_output { return Ok(BTreeSet::new()); }
        let copied = self.copy_shifted(r, encoder);
        if !copied.is_empty() {
            let [x, y, width, height] = paint_transform::texel_rect(copied.window_local(self.plan.bounds), 1 << self.plan.level);
            self.reduce_output(r, encoder, PixelRect::new(x, y, x + width, y + height), commands)?;
        }
        let root = self.graph.root.as_ref().expect("prepared composition graph");
        let invalid = if self.unchanged { PixelRect::EMPTY } else {
            dirty.union(root.damage(&scene.scale_sources, display_mips::Plan::window(self.plan.extent, 0, self.plan.bounds)))
        };
        let sparse = tiles.filter(|_| bounded(packet.layers));
        self.refined.retain(|c| page_rect(*c).intersect(invalid).is_empty() || sparse.is_some_and(|s| !s.contains(c)));
        Ok(page_coordinates(self.plan.bounds).filter(|c| !self.refined.contains(c)).collect())
    }

    fn next_exact_page(&self) -> Option<[u32; 2]> {
        page_coordinates(self.plan.bounds).find(|page| !self.refined.contains(page))
    }

    fn next_missing_page(&self) -> Option<[u32; 2]> {
        self.missing_pages().next()
    }

    fn missing_pages(&self) -> impl Iterator<Item = [u32; 2]> + '_ {
        std::iter::once(self).chain(self.overview.as_deref())
            .flat_map(|cache| page_coordinates(cache.plan.bounds).filter(|c| !cache.refined.contains(c)))
            .chain(self.hierarchy.iter().flat_map(|h| h.missing_pages()))
    }

    pub fn has_pending_work(&self, r: &WgpuRasterizer) -> bool {
        self.ready && r.moving_layer.is_none() && !self.transform.as_ref().is_some_and(|p| p.moving)
            && self.next_missing_page().is_some()
    }

    pub(super) fn refine(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, commands: &mut Commands,
    ) -> Result<PixelRect, GpuRasterError> {
        self.admit_hierarchy(r, encoder, commands)?;
        let mut image = match &self.hierarchy {
            Some(hierarchy) => hierarchy.root().clone(),
            None => self.exact_tile.get_or_insert_with(|| Image::new(r,
                display_mips::Plan::at([PAGE_SIZE; 2], 0), "exact composition working tile")).clone(),
        };
        let texture = image.texture.clone();
        let view = image.view.clone();
        let mut seen = BTreeSet::new();
        let pages: Vec<_> = self.missing_pages().filter(|c| seen.insert(*c)).take(4).collect();
        let regions: Vec<_> = pages.iter().map(|c| page_rect(*c).intersect(PixelRect::full(self.plan.extent))).collect();
        r.ensure_exact_preview(encoder)?;
        let mut prepared = if bounded(packet.layers) {
            pages.first().map(|coordinate| {
                    PixelRect::new(coordinate[0] * PAGE_SIZE, coordinate[1] * PAGE_SIZE,
                        (coordinate[0] + pages.len() as u32) * PAGE_SIZE, (coordinate[1] + 1) * PAGE_SIZE)
                        .intersect(PixelRect::full(self.plan.extent))
                })
        } else { None };
        if let Some(region) = prepared {
            scene.prepare_region(r, packet, Scene::capture_window(packet.layers, region, packet.document_extent), PixelRect::EMPTY, encoder)?;
        }
        let batched = self.hierarchy.is_some()
            && prepared.is_some_and(|prepared| regions.iter().all(|r| r.intersect(prepared) == *r));
        if batched { scene.capture_prepared_regions(r, packet, &image, &regions, scene::Output::Display, false, encoder)?; }
        let mut changed = PixelRect::EMPTY;
        for (coordinate, region) in pages.into_iter().zip(regions) {
            if !prepared.is_some_and(|prepared| region.intersect(prepared) == region) {
                prepared = None;
                scene.prepare_region(r, packet, Scene::capture_window(packet.layers, region, packet.document_extent), PixelRect::EMPTY, encoder)?;
            }
            if self.hierarchy.is_none() { image.plan = display_mips::Plan::window(packet.document_extent, 0, region); }
            if !batched { scene.capture_prepared_region(r, packet, &image, region, scene::Output::Display, encoder)?; }
            if let Some(hierarchy) = &mut self.hierarchy { hierarchy.write(encoder, &texture, coordinate, region); }
            self.write_exact(r, encoder, commands, &view, coordinate, region)?;
            if let Some(overview) = &mut self.overview {
                overview.write_exact(r, encoder, commands, &view, coordinate, region)?;
            }
            r.metrics.composited_pixels += region.area();
            changed = changed.union(region);
        }
        if let Some(hierarchy) = &mut self.hierarchy { hierarchy.flush(encoder); }
        Ok(changed)
    }

    fn write_exact(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        commands: &mut Commands, source: &wgpu::TextureView, coordinate: [u32; 2], region: PixelRect,
    ) -> Result<(), GpuRasterError> {
        let covered = region.intersect(self.plan.bounds);
        if covered.is_empty() || self.refined.contains(&coordinate) { return Ok(()); }
        self.pixels.ensure(r, self.plan);
        self.refined.insert(coordinate);
        self.valid.insert(coordinate);
        if matches!(self.pixels, hierarchy::Pixels::Resident { .. }) {
            if self.next_exact_page().is_none() { self.placed = None; }
            return Ok(());
        }
        let [x, y, width, height] = paint_transform::texel_rect(covered.window_local(self.plan.bounds), 1 << self.plan.level);
        let mut values = [0; 20];
        values[..8].copy_from_slice(&[x, y, width, height, region.width(), region.height(), 1 << self.plan.level, 0]);
        let binding = Commands::binding(r, source, &r.empty_view, &self.pixels.root().unwrap().view);
        commands.reduce(r, encoder, values, &binding, "reduce exact composition into display")?;
        let mut changed = PixelRect::new(x, y, x + width, y + height);
        if self.placed.is_some() {
            if self.next_exact_page().is_some() { return Ok(()); }
            self.placed = None;
            changed = PixelRect::full(self.plan.size);
        }
        self.reduce_output(r, encoder, changed, commands)
    }
}

impl Scene {
    pub fn refine_display(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PixelRect, GpuRasterError> {
        let write = self.begin_write(r);
        let Some(mut cache) = r.scale_display.take() else { return Ok(PixelRect::EMPTY) };
        cache.submission_valid = Some(self.valid.clone());
        let mut commands = self.scale_commands.take().unwrap_or_else(|| Commands::new(r));
        commands.begin();
        let result = cache.refine(self, r, packet, encoder, &mut commands);
        self.scale_commands = Some(commands);
        r.scale_display = Some(cache);
        if result.is_ok() { write.track(encoder); }
        result
    }
}
