use super::*;
use wgpu::util::DeviceExt;

type Document = (u64, layer_core::PortableId);

pub(crate) struct Floor {
    pixels: pages::Pixels,
    retiring: Option<pages::Pixels>,
    geometry: wgpu::Buffer,
    document: Document,
}

impl Floor {
    fn new(r: &WgpuRasterizer, document: Document) -> Self {
        let words = [1u32, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let geometry = r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("floor page geometry"),
            contents: &words.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::STORAGE,
        });
        Self { pixels: Default::default(), retiring: None, geometry, document }
    }
    pub(super) fn image(&self) -> Option<&pages::CommittedImage> {
        self.pixels.committed_root().or_else(|| self.retiring.as_ref()?.committed_root())
    }
    pub(crate) fn document(&self) -> Document { self.document }
    pub(crate) fn page<'a>(&'a self, page: Option<PresentedPage<'a>>) -> Option<PresentedPage<'a>> {
        match (page, self.image()) {
            (Some(page), Some(image)) => Some(page.with_floor(image)),
            (page, None) => page,
            (None, Some(image)) => Some(PresentedPage::floor(image, &self.geometry)),
        }
    }

    fn level(extent: [u32; 2], source: u32) -> u32 {
        display_mips::Plan::new(extent).map_or(source, |plan| plan.level.max(source))
    }
    fn wants(&self, source: &pages::CommittedImage, settled: bool) -> bool {
        let Some(image) = self.image() else { return true; };
        let plan = source.image().plan;
        Self::level(plan.extent, plan.level) < image.image().plan.level
            || settled && source.authored().is_some() && source.authored() != image.authored()
    }
    fn source(r: &WgpuRasterizer) -> Option<&pages::CommittedImage> {
        r.scale_display.as_ref()?.floor_source(r.blend_space)
    }
    fn ready(r: &WgpuRasterizer, source: &pages::CommittedImage) -> bool {
        let image = source.image();
        Self::level(image.plan.extent, image.plan.level) == image.plan.level
            || r.compilation.require([&r.scene_pipelines.scale.output(image.texture.format()).reduce], crate::startup::ANTICIPATED)
    }
    pub(crate) fn demand(r: &WgpuRasterizer) -> Option<crate::frame_demand::Outstanding> {
        let document = r.presentation.document?;
        let source = Self::source(r)?;
        r.floor.as_ref().filter(|floor| floor.document == document).is_none_or(|floor| floor.wants(source, r.idle_frame_settles()))
            .then(|| if Self::ready(r, source) { crate::frame_demand::Outstanding::Runnable } else { crate::frame_demand::Outstanding::Blocked(crate::frame_demand::Input::Compile) })
    }

    pub(crate) fn publish(r: &mut WgpuRasterizer, encoder: &mut submission::CommandEncoder, settled: bool) -> Result<bool, GpuRasterError> {
        let Some(document) = r.presentation.document else { return Ok(false); };
        let mut floor = r.floor.take().filter(|floor| floor.document == document);
        if let Some(floor) = &mut floor
            && floor.pixels.committed_root().is_some_and(|image| image.submission().submitted()) { floor.retiring = None; }
        let result = Self::write(r, encoder, &mut floor, document, settled);
        r.floor = floor;
        result
    }

    fn write(r: &mut WgpuRasterizer, encoder: &mut submission::CommandEncoder, floor: &mut Option<Floor>, document: Document, settled: bool) -> Result<bool, GpuRasterError> {
        let Some(source) = Self::source(r) else { return Ok(false); };
        if floor.as_ref().is_some_and(|floor| !floor.wants(source, settled)) { return Ok(false); }
        if !Self::ready(r, source) { return Err(GpuRasterError::NotReady); }
        let (input, authored) = (source.image().clone(), source.authored());
        let level = Self::level(input.plan.extent, input.plan.level);
        let plan = display_mips::Plan::at(input.plan.extent, level);
        let format = input.texture.format();
        let floor = floor.get_or_insert_with(|| Floor::new(r, document));
        if floor.pixels.root().is_none_or(|image| image.plan != plan || image.texture.format() != format) {
            let previous = std::mem::take(&mut floor.pixels);
            if previous.committed_root().is_some() { floor.retiring = Some(previous); }
            let bytes = plan.size.map(u64::from).into_iter().product::<u64>() * u64::from(format.block_copy_size(None).unwrap());
            floor.pixels.ensure_root(r, plan, format, "display floor", r.scene_tiers.windows.charge(bytes));
        }
        let output = floor.pixels.root().unwrap().clone();
        if input.plan.level == plan.level {
            encoder.copy_texture_to_texture(input.texture.as_image_copy(), output.texture.as_image_copy(),
                wgpu::Extent3d { width: plan.size[0], height: plan.size[1], depth_or_array_layers: 1 });
        } else {
            let mut scene = r.scene.take().ok_or(GpuRasterError::NotReady)?;
            let mut commands = scene.scale_commands.take().unwrap_or_else(|| RegionRecords::charged(r, &scene.tiers.gpu));
            commands.begin();
            let mut values = [0; 20];
            values[..8].copy_from_slice(&[0, 0, plan.size[0], plan.size[1], input.plan.bounds.width(), input.plan.bounds.height(),
                1 << (plan.level - input.plan.level), (input.plan.level << 8) | 8]);
            values[14] = ((input.plan.bounds.min_x() >> input.plan.level) as f32).to_bits();
            values[15] = ((input.plan.bounds.min_y() >> input.plan.level) as f32).to_bits();
            let binding = RegionRecords::binding(r, &input.view, &r.empty_view, &output.view);
            let result = commands.reduce(r, encoder, values, &binding, "reduce the display floor", format);
            scene.scale_commands = Some(commands);
            r.scene = Some(scene);
            result?;
        }
        floor.pixels.commit_whole(encoder.submission(), authored, r.blend_space);
        Ok(true)
    }
}
