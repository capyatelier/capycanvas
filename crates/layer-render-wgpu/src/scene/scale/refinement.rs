use super::*;

impl Cache {
    pub(in crate::scene) fn render_native(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let mut missing = self.invalidate_native(scene, r, packet, dirty, encoding, tiles)?;
        if let Some(overview) = &mut self.overview {
            missing.extend(overview.invalidate_native(scene, r, packet, dirty, encoding, tiles)?);
        }
        if missing.is_empty() { return Ok(PixelRect::EMPTY); }
        let Encoding { encoder, commands } = encoding;
        self.admit_hierarchy(r, encoder, commands)?;
        let reverse = scene.native_reverse;
        scene.native_reverse = !reverse;
        let changed = missing.iter().fold(PixelRect::EMPTY, |a, c| a.union(page_rect(*c))).intersect(PixelRect::full(packet.document_extent));
        let budget = r.native_edit.as_ref().map_or(windows::DEFAULT_IMAGE_PIXEL_BYTES, |n| n.image_pixel_budget(self.resident_bytes()));
        let plan = windows::Plan::new(packet.layers, packet.document_extent, budget)?;
        let mut regions: Vec<_> = plan.map_or_else(|| vec![(changed, PixelRect::full(packet.document_extent))], |p| p.regions(changed).collect());
        if reverse { regions.reverse(); }
        let mut image = match &self.hierarchy {
            Some(hierarchy) => hierarchy.root().clone(),
            None => self.exact_tile.get_or_insert_with(|| Image::new(r,
                display_mips::Plan::at(exact_strip(packet.document_extent), 0), "exact composition working strip")).clone(),
        };
        let texture = image.texture.clone();
        let mut batch = Vec::with_capacity(SOURCE_SLOTS);
        if plan.is_some() {
            Scene::submit_chunk(r, encoder, "before native filter windows")?;
            r.metrics.image_window_submissions += 1;
        }
        for (output, window) in regions {
            scene.prepare_region(r, packet, window, dirty, encoder)?;
            if plan.is_some() {
                r.metrics.image_window_peak_bytes = r.metrics.image_window_peak_bytes.max(scene.images.storage_bytes());
            }
            let mut coordinates: Vec<_> = page_coordinates(output).filter(|c| missing.contains(c)).collect();
            if reverse {coordinates.reverse();}
            let mut coordinates = coordinates.into_iter().peekable();
            loop {
                batch.clear();let mut bounds = PixelRect::EMPTY;
                while batch.len() < SOURCE_SLOTS {
                    let Some(&coordinate) = coordinates.peek() else {break;};
                    let region = page_rect(coordinate).intersect(PixelRect::full(packet.document_extent));
                    let combined = bounds.union(region);
                    if self.hierarchy.is_none() && (combined.width() > image.texture.width() || combined.height() > image.texture.height()) {break;}
                    coordinates.next();bounds = combined;batch.push(region);
                }
                if batch.is_empty() {break;}
                if self.hierarchy.is_none() {image.plan = display_mips::Plan::window(packet.document_extent, 0, bounds);}
                scene.capture_prepared_regions(r, packet, &image, &batch, scene::Output::Display, false, encoder)?;
                for &region in &batch {
                    let coordinate = [region.min_x() / PAGE_SIZE, region.min_y() / PAGE_SIZE];
                    if let Some(hierarchy) = &mut self.hierarchy { hierarchy.write(encoder, &texture, coordinate, region); }
                    r.metrics.composited_pixels += region.area();
                }
                self.write_exact(r, encoder, commands, &image, &batch)?;
                if let Some(overview) = &mut self.overview { overview.write_exact(r, encoder, commands, &image, &batch)?; }
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
        &mut self, scene: &Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        dirty: PixelRect, encoding: &mut Encoding<'_>, tiles: Option<&BTreeSet<[u32; 2]>>,
    ) -> Result<BTreeSet<[u32; 2]>, GpuRasterError> {
        if self.reuse_output { return Ok(BTreeSet::new()); }
        let Encoding { encoder, commands } = encoding;
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
                display_mips::Plan::at(exact_strip(packet.document_extent), 0), "exact composition working strip")).clone(),
        };
        let texture = image.texture.clone();
        let mut seen = BTreeSet::new();
        let budget = r.native_edit.as_ref().map_or(windows::DEFAULT_IMAGE_PIXEL_BYTES, |n| n.image_pixel_budget(self.resident_bytes()));
        let mut bounds = PixelRect::EMPTY;
        let pages: Vec<_> = self.missing_pages().filter(|c| seen.insert(*c)).take(4).take_while(|c| {
            let combined = bounds.union(page_rect(*c)).intersect(PixelRect::full(self.plan.extent));
            let window = Scene::capture_window(packet.layers, combined, packet.document_extent);
            if !bounds.is_empty() && Scene::capture_image_bound(packet.layers, window) > budget { return false; }
            bounds = combined;
            true
        }).collect();
        let regions: Vec<_> = pages.iter().map(|c| page_rect(*c).intersect(PixelRect::full(self.plan.extent))).collect();
        r.ensure_exact_preview(encoder)?;
        if !bounds.is_empty() {
            scene.prepare_region(r, packet, Scene::capture_window(packet.layers, bounds, packet.document_extent), PixelRect::EMPTY, encoder)?;
        }
        let batched = self.hierarchy.is_some();
        if batched { scene.capture_prepared_regions(r, packet, &image, &regions, scene::Output::Display, false, encoder)?; }
        let mut changed = PixelRect::EMPTY;
        for (coordinate, region) in pages.into_iter().zip(regions) {
            if self.hierarchy.is_none() { image.plan = display_mips::Plan::window(packet.document_extent, 0, region); }
            if !batched { scene.capture_prepared_region(r, packet, &image, region, scene::Output::Display, encoder)?; }
            if let Some(hierarchy) = &mut self.hierarchy { hierarchy.write(encoder, &texture, coordinate, region); }
            self.write_exact(r, encoder, commands, &image, &[region])?;
            if let Some(overview) = &mut self.overview {
                overview.write_exact(r, encoder, commands, &image, &[region])?;
            }
            r.metrics.composited_pixels += region.area();
            changed = changed.union(region);
        }
        if let Some(hierarchy) = &mut self.hierarchy { hierarchy.flush(encoder); }
        Ok(changed)
    }

    fn write_exact(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        commands: &mut Commands, source: &Image, regions: &[PixelRect],
    ) -> Result<(), GpuRasterError> {
        let mut regions: Vec<PixelRect> = regions.iter().filter_map(|region| {
            let covered = region.intersect(self.plan.bounds);
            let coordinate = [region.min_x() / PAGE_SIZE, region.min_y() / PAGE_SIZE];
            if covered.is_empty() || !self.refined.insert(coordinate) {return None;}
            self.valid.insert(coordinate);Some(covered)
        }).collect();
        if regions.is_empty() {return Ok(());}
        self.pixels.ensure(r, self.plan);
        if matches!(self.pixels, hierarchy::Pixels::Resident { .. }) {
            if self.placed.is_some() && self.next_exact_page().is_none() { self.placed = None; }
            return Ok(());
        }
        regions.dedup_by(|next, previous| {
            let combined = previous.union(*next);
            if combined.area() == previous.area() + next.area() {*previous = combined;true} else {false}
        });
        let binding = Commands::binding(r, &source.view, &r.empty_view, &self.pixels.root().unwrap().view);
        let mut changed = PixelRect::EMPTY;
        let mut records = Vec::with_capacity(regions.len());
        for covered in regions {
            let [x, y, width, height] = paint_transform::texel_rect(covered.window_local(self.plan.bounds), 1 << self.plan.level);
            let mut values = [0; 20];
            values[..8].copy_from_slice(&[x, y, width, height, source.plan.bounds.width(), source.plan.bounds.height(), 1 << self.plan.level, 0]);
            values[14] = (-(covered.min_x() as f32 - source.plan.bounds.min_x() as f32)).to_bits();
            values[15] = (-(covered.min_y() as f32 - source.plan.bounds.min_y() as f32)).to_bits();
            let offset = commands.record(r, encoder, values)?;records.push((offset,[width,height]));
            changed = changed.union(PixelRect::new(x, y, x + width, y + height));
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("reduce exact composition into display"),timestamp_writes:None});
        pass.set_pipeline(&r.scene_pipelines.scale.reduce);pass.set_bind_group(1,&binding,&[]);
        for (offset,size) in records {
            pass.set_bind_group(0,&commands.record_binding,&[offset]);pass.dispatch_workgroups(size[0].div_ceil(8),size[1].div_ceil(8),1);
        }
        drop(pass);
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
