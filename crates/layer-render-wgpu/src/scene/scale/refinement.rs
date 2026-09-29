use super::*;

impl Cache {
    fn next_exact_page(&self) -> Option<[u32; 2]> {
        page_coordinates(self.plan.bounds).find(|page| !self.refined.contains(page))
    }

    pub fn has_pending_work(&self) -> bool {
        self.ready && !self.transform.as_ref().is_some_and(|p| p.moving)
            && (self.next_exact_page().is_some()
                || self.overview.as_ref().is_some_and(|overview| overview.next_exact_page().is_some()))
    }

    pub(super) fn refine(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder, commands: &mut Commands,
    ) -> Result<PixelRect, GpuRasterError> {
        let Some(coordinate) = self.next_exact_page().or_else(|| self.overview.as_ref().and_then(|c| c.next_exact_page()))
            else { return Ok(PixelRect::EMPTY) };
        let region = page_rect(coordinate).intersect(PixelRect::full(self.plan.extent));
        let image = self.exact_tile.get_or_insert_with(|| Image::new(r,
            display_mips::Plan::at([PAGE_SIZE; 2], 0), "exact composition working tile"));
        let texture = image.texture.clone();
        let view = image.view.clone();
        r.ensure_exact_preview(encoder)?;
        scene.capture_region(r, packet, &texture, region, scene::Output::Display, encoder)?;
        self.write_exact(r, encoder, commands, &view, coordinate, region)?;
        if let Some(overview) = &mut self.overview {
            overview.write_exact(r, encoder, commands, &view, coordinate, region)?;
        }
        r.metrics.composited_pixels += region.area();
        Ok(region)
    }

    fn write_exact(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        commands: &mut Commands, source: &wgpu::TextureView, coordinate: [u32; 2], region: PixelRect,
    ) -> Result<(), GpuRasterError> {
        let covered = region.intersect(self.plan.bounds);
        if covered.is_empty() || self.refined.contains(&coordinate) { return Ok(()); }
        if self.output.is_empty() { self.selected = self.allocate(r); }
        let [x, y, width, height] = paint_transform::texel_rect(covered.window_local(self.plan.bounds), 1 << self.plan.level);
        let mut values = [0; 20];
        values[..8].copy_from_slice(&[x, y, width, height, region.width(), region.height(), 1 << self.plan.level, 0]);
        let offset = commands.record(r, encoder, values)?;
        let binding = Commands::binding(r, source, &r.empty_view, &self.output[self.selected].view);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("reduce exact composition into display"), timestamp_writes: None,
        });
        pass.set_pipeline(&r.scene_pipelines.scale.reduce);
        pass.set_bind_group(0, &commands.record_binding, &[offset]);
        pass.set_bind_group(1, &binding, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        drop(pass);
        self.refined.insert(coordinate);
        self.valid.insert(coordinate);
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
        let Some(mut cache) = r.scale_display.take() else { return Ok(PixelRect::EMPTY) };
        let mut commands = self.scale_commands.take().unwrap_or_else(|| Commands::new(r));
        commands.begin();
        let result = cache.refine(self, r, packet, encoder, &mut commands);
        self.scale_commands = Some(commands);
        r.scale_display = Some(cache);
        result
    }
}
