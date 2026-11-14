//! Smudge, Wet and watercolor read only the pre-batch pages around
//! each page they write, so one compute pass dispatches every page job of a
//! batch. A job writes its whole inactive page: the material result inside its
//! dirty rectangle and the unchanged texel elsewhere, so no page copy precedes
//! it. Watercolor wetness is stored only inside the rectangle.
use super::*;
use dry_material::{Entries, MATERIAL_BLENDS, MATERIAL_SLOTS};
use material_sources::{elapsed, record, rect_words};

pub(super) const OPERATIONS: [MaterialOperation; 3] = [MaterialOperation::Smudge,
    MaterialOperation::Wet, MaterialOperation::Watercolor];

/// The workgroup of an operation's page and reservoir kernels.
pub(super) fn workgroup(operation: MaterialOperation) -> [u32; 2] {
    if operation == MaterialOperation::Smudge { [16, 8] } else { [8, 8] }
}

pub(super) fn transported(style: &layer_render::DabStyle) -> bool {
    OPERATIONS.contains(&BrushPassPlan::for_style(style).material)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Entry {
    Color,
    Coverage,
    Reservoir,
    Watercolor,
}

impl Entry {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Color => "transport_color",
            Self::Coverage => "transport_coverage",
            Self::Reservoir => "reservoir_exchange",
            Self::Watercolor => "transport_watercolor",
        }
    }

    /// The output layout: color, with coverage, or with coverage and wetness.
    fn outputs(self) -> usize {
        match self {
            Self::Coverage => 1,
            Self::Watercolor => 2,
            Self::Color | Self::Reservoir => 0,
        }
    }
}

pub(super) fn entries(operation: MaterialOperation) -> &'static [Entry] {
    match operation {
        MaterialOperation::Wet => &[Entry::Color, Entry::Coverage, Entry::Reservoir],
        MaterialOperation::Watercolor => &[Entry::Watercolor],
        MaterialOperation::Smudge => &[Entry::Color, Entry::Coverage],
        _ => unreachable!("operation uses its own pipeline"),
    }
}

/// A blender kernel: its kind, whether it reads the paper and whether it
/// lifts pigment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Blender {
    pub kind: layer_core::BlenderKind,
    pub paper: bool,
    pub lift: bool,
    pub cells: bool,
}

/// The blender of operations that do not blend.
pub(super) const PLAIN: Blender = Blender { kind: layer_core::BlenderKind::Soft, paper: false, lift: false, cells: false };

/// The blenders an operation compiles a module for: for Smudge, every kind,
/// with and without the paper for kinds that touch it and with and without
/// lifting for kinds that lift.
pub(super) fn blenders(operation: MaterialOperation) -> Vec<Blender> {
    use layer_core::BlenderKind;
    if operation != MaterialOperation::Smudge { return vec![PLAIN]; }
    BlenderKind::ALL.into_iter().flat_map(|kind| {
        let papers = if kind.touches_paper() { &[false, true][..] } else { &[false][..] };
        let lifts = if kind.lifts() { &[false, true][..] } else { &[false][..] };
        let cells = match kind { BlenderKind::Dither => &[false, true][..], BlenderKind::Scatter => &[true][..], _ => &[false][..] };
        papers.iter().flat_map(move |&paper| lifts.iter().flat_map(move |&lift|
            cells.iter().map(move |&cells| Blender { kind, paper, lift, cells })))
    }).collect()
}

/// The blender kernel a style needs: it reads the paper when the kind
/// touches paper and the brush has Texture strength, and lifts when the kind
/// lifts and the brush sets a lift.
pub(super) fn blender(style: &layer_render::DabStyle) -> Blender {
    let kind = style.blender.kind;
    Blender {
        kind,
        paper: kind.touches_paper() && style.grain.is_some_and(|grain| grain.depth > 0.0),
        lift: kind.lifts() && style.blender.lift > 0.0,
        cells: kind == layer_core::BlenderKind::Scatter || (ordered_dither(style) && style.blender.cell_size > 1.),
    }
}

/// The module that provides `entry` of `operation` for one blend code and blender.
pub(super) fn module(operation: MaterialOperation, blend: u32, blender: Blender) -> Entries {
    match operation {
        MaterialOperation::Smudge => Entries::Blend(blender),
        _ => Entries::Transport(blend),
    }
}

/// The kernels a brush selection prepares for committed and predicted pages.
pub(super) fn selected(plan: BrushPassPlan, preview: bool) -> impl Iterator<Item = Entry> {
    entries(plan.material).iter().copied().filter(move |&entry| match entry {
        Entry::Color => preview || !plan.state.coverage,
        Entry::Coverage => plan.state.coverage,
        Entry::Reservoir | Entry::Watercolor => true,
    })
}

/// Wet and watercolor deposit through the brush blend. Smudge only
/// move paint, so every blend shares the normal slot of their contact.
pub(super) fn blended(operation: MaterialOperation) -> bool {
    matches!(operation, MaterialOperation::Wet | MaterialOperation::Watercolor)
}

type KernelKey = (u32, usize, Entry, u8, bool, bool, bool);

#[derive(Clone)]
pub(super) struct Pipelines {
    layouts: [wgpu::BindGroupLayout; 4],
    kernels: std::collections::BTreeMap<KernelKey, Deferred<wgpu::ComputePipeline>>,
}

impl Pipelines {
    /// `shared` holds the target, material source and texture layouts;
    /// `layouts` the write-only color, color-with-coverage and watercolor outputs.
    pub fn new(device: &PipelineDevice, shared: [&wgpu::BindGroupLayout; 3], layouts: [wgpu::BindGroupLayout; 3]) -> Self {
        let [color, coverage, wetness] = layouts;
        let layouts = [color, coverage, wetness, dry_material::dither_output_layout(device)];
        let pipeline_layouts: [_; 4] = std::array::from_fn(|outputs| device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("destination material pages"),
            bind_group_layouts: &[Some(&layouts[outputs]), Some(shared[0]), Some(shared[1]), Some(shared[2])],
            immediate_size: 0,
        }));
        let mut kernels = std::collections::BTreeMap::new();
        for (slot, operation) in (0..MATERIAL_SLOTS).flat_map(|slot| OPERATIONS.map(|operation| (slot, operation))) {
            if !blended(operation) && slot % MATERIAL_BLENDS.len() != 0 { continue; }
            let (flags, blend) = dry_material::material_entries(slot);
            for blender in blenders(operation) {
                let transport = dry_material::material_shader(device, module(operation, blend, blender), flags, operation);
                for &entry in entries(operation) {
                    let outputs = if matches!(blender.kind, layer_core::BlenderKind::Dither | layer_core::BlenderKind::Scatter) && entry == Entry::Coverage { 3 } else { entry.outputs() };
                    kernels.insert((operation as u32, slot, entry, blender.kind as u8, blender.paper, blender.lift, blender.cells), Deferred::compute(device,
                        format!("destination material {} operation={} contact={flags:#x} blend={blend} {blender:?}", entry.name(), operation as u32),
                        &pipeline_layouts[outputs], &transport, entry.name()));
                }
            }
        }
        Self { layouts, kernels }
    }

    pub fn kernel(&self, style: &layer_render::DabStyle, entry: Entry) -> &Deferred<wgpu::ComputePipeline> {
        let operation = BrushPassPlan::for_style(style).material;
        let slot = if blended(operation) { dry_material::material_slot(style) }
            else { dry_material::material_contact_slot(style) * MATERIAL_BLENDS.len() };
        let blender = if operation == MaterialOperation::Smudge { blender(style) } else { PLAIN };
        &self.kernels[&(operation as u32, slot, entry, blender.kind as u8, blender.paper, blender.lift, blender.cells)]
    }

    fn output(&self,device:&PipelineDevice,style:&wgpu::Buffer,color:&resident::RasterWrite<'_,PageSurface>,
        coverage:Option<&resident::RasterWrite<'_,PageSurface>>,wetness:Option<&resident::RasterWrite<'_,PageSurface>>) -> wgpu::BindGroup {
        let outputs = if coverage.is_some_and(|p| p.value().texture.format() == wgpu::TextureFormat::Rgba32Float) { 3 }
            else { usize::from(coverage.is_some()) + usize::from(wetness.is_some()) };
        let layout = &self.layouts[outputs];
        dry_material::output_binding(device,style,layout,color,coverage,wetness,None)
    }
}

pub(super) struct Dispatch {
    entry: Entry,
    output: wgpu::BindGroup,
    sources: wgpu::BindGroup,
    coordinate: [u32; 2],
    record_offset: u32,
}

impl WgpuRasterizer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode_transport_jobs(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        dabs: &[Dab],
        jobs: &[MaterialJob],
        layer_index: usize,
        target: BrushEncodingTarget,
    ) -> Result<(), GpuRasterError> {
        let preview = target.is_preview();
        let started = self.telemetry.enabled.then(web_time::Instant::now);
        let extent = self.target_extent(batch.target);
        let operation = BrushPassPlan::for_style(&batch.style).material;
        let exchange = dabs.first().zip(dabs.last()).filter(|_| !preview && operation == MaterialOperation::Wet);
        let seeds: Vec<_> = jobs.iter().map(|job| operation == MaterialOperation::Watercolor && !self.watercolor_written.contains(&job.coordinate)).collect();
        self.watercolor_written.extend(jobs.iter().zip(&seeds).filter(|(_, seed)| **seed).map(|(job, _)| job.coordinate));
        self.metrics.material_cpu_ms[0] += elapsed(started);
        let mut first_record = self.prepare_records(jobs.iter().zip(&seeds).map(|(job, &seed)| {
            record([job.dabs.start, job.dabs.end - job.dabs.start, 0, u32::from(seed)], [rect_words(job.local)])
        }).chain(exchange.map(|(first, _)| {
            let color = first.color_rgba_linear;
            record([0; 4], [[color[0], color[1], color[2], batch.style.wet_mix.amount_of_paint * first.material[2]].map(f32::to_bits)])
        })), encoder)?;
        let mut publications=Vec::with_capacity(jobs.len()*3+usize::from(exchange.is_some()));
        let mut written=Vec::with_capacity(jobs.len()*3+usize::from(exchange.is_some()));
        let mut dispatches = Vec::with_capacity(jobs.len());
        let intents=jobs.iter().map(|job| {
            let intent=self.raster_intent(batch.target,RasterPlane::Color,job.coordinate)?;
            let wet_intent=(operation==MaterialOperation::Watercolor).then(||self.raster_intent(batch.target,RasterPlane::WatercolorWetness,job.coordinate)).transpose()?;
            Ok((intent,wet_intent))
        }).collect::<Result<Vec<_>,GpuRasterError>>()?;
        let source_bindings=jobs.iter().zip(&intents).map(|(job,(intent,_))|self.transport_sources(encoder,batch_index,batch,job.coordinate,target,intent))
            .collect::<Result<Vec<_>,_>>()?;
        for (((index,job),sources),(intent,wet_intent)) in jobs.iter().enumerate().zip(source_bindings).zip(intents) {
            let color_ticket=preview.then(||self.preview_material_version(batch,batch_index,RasterPlane::Color,job.coordinate,scene::PreviewPass::Color,&intent)).transpose()?;
            let coverage_ticket=(preview && job.coverage_index.is_some()).then(||self.preview_material_version(batch,batch_index,RasterPlane::Color,job.coordinate,scene::PreviewPass::Coverage,&intent)).transpose()?;
            let wetness_ticket=wet_intent.as_ref().filter(|_|preview).map(|wet_intent|self.preview_material_version(batch,batch_index,RasterPlane::WatercolorWetness,job.coordinate,scene::PreviewPass::Wetness,wet_intent)).transpose()?;
            let receipt=encoder.submission();
            let layer=&mut self.paint_layers[layer_index];
            let (color_pages,coverage_pages,wetness_pages)=if preview {(&mut self.preview_pages,&mut self.preview_coverage_pages,&mut self.preview_watercolor_wetness_pages)}
                else {(&mut layer.pages,&mut layer.coverage_pages,&mut layer.watercolor_wetness_pages)};
            let page=&mut color_pages[job.page_index];
            let destination=if preview {page.surface_mut(job.destination_secondary).overwrite(&receipt)}
                else {page.write(job.destination_secondary,&intent,&receipt)}.ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
            let coverage=job.coverage_index.zip(job.coverage_destination_secondary).map(|(index,secondary)| {
                let page=&mut coverage_pages[index];let destination=if secondary {&mut page.secondary}else {&mut page.primary};
                destination.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))
            }).transpose()?;
            let wetness=if let Some(wet_intent)=&wet_intent {
                let page=wetness_pages.iter_mut().find(|page|page.coordinate==job.coordinate).ok_or(GpuRasterError::Invariant("prepared encode transport jobs state is inconsistent"))?;
                let secondary=!page.active_secondary;
                let write=if !seeds[index] {
                    let WatercolorWetnessPage {primary,secondary:second,pending,..}=page;
                    let (state,written,_)=pending.as_ref().ok_or(GpuRasterError::Invariant("prepared encode transport jobs state is inconsistent"))?;
                    if !state.input.same_selection(wet_intent) || state.input.version().matches(wet_intent.version())!=Some(true) {return Err(GpuRasterError::Invariant("accepted write selection changed"));}
                    let surface=if secondary {second}else {primary};
                    if preview {surface.revise(written.version(),&receipt)}else {surface.continue_written(written,&receipt)}
                }else if preview {page.surface_mut(secondary).overwrite(&receipt)}else {page.write(secondary,wet_intent,&receipt)};
                Some(write.ok_or(GpuRasterError::Invariant("prepared encode transport jobs state is inconsistent"))?)
            }else {None};
            destination.value().preview.set(None);
            let output=self.pipelines.destination_material.output(&self.device,&self.style_buffer,&destination,coverage.as_ref(),wetness.as_ref());
            let entry=if wetness.is_some() {Entry::Watercolor}else if coverage.is_some() {Entry::Coverage}else {Entry::Color};
            let next_publications=[
                Some(MaterialPublication {kind:MaterialOutputKind::Color,coordinate:job.coordinate,preview:color_ticket.clone(),input:intent.clone(),
                    encoding:destination.encoded(color_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()))}),
                coverage.map(|write|MaterialPublication {kind:MaterialOutputKind::Coverage,coordinate:job.coordinate,preview:coverage_ticket.clone(),input:intent,
                    encoding:write.encoded(coverage_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()))}),
                wetness.map(|write|MaterialPublication {kind:MaterialOutputKind::Wetness,coordinate:job.coordinate,preview:wetness_ticket.clone(),input:wet_intent.unwrap(),
                    encoding:write.encoded(wetness_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()))}),
            ];
            dispatches.push(Dispatch {entry,output,sources,coordinate:job.coordinate,record_offset:self.material_records.offset(first_record)});
            publications.extend(next_publications.into_iter().flatten());
            first_record += 1;
        }
        if let Some((_, last)) = exchange {
            let coordinate = std::array::from_fn(|i| ([last.center.x, last.center.y][i].max(0.) as u32 / PAGE_SIZE).min(extent[i].saturating_sub(1) / PAGE_SIZE));
            let intent=self.raster_intent(batch.target,RasterPlane::Color,coordinate)?;
            let sources=self.transport_sources(encoder,batch_index,batch,coordinate,target,&intent)?;
            let receipt=encoder.submission();
            let destination=if self.reservoir.active_secondary {&mut self.reservoir.primary}else {&mut self.reservoir.secondary};
            let write=destination.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
            let output=self.pipelines.destination_material.output(&self.device,&self.style_buffer,&write,None,None);
            let publication=MaterialPublication {kind:MaterialOutputKind::Reservoir,coordinate,preview:None,input:intent,
                encoding:write.encoded(layer_core::raster::ContentName::fresh())};
            dispatches.push(Dispatch {entry:Entry::Reservoir,output,sources,coordinate,record_offset:self.material_records.offset(first_record)});
            publications.push(publication);
        }
        self.encode_transport(encoder, batch_index, batch, &dispatches);
        let actual=encoder.submission();written.extend(publications.drain(..).map(|publication:MaterialPublication|publication.encoded(&actual)));
        self.publish_material(batch,written)?;
        if exchange.is_some() { self.reservoir.active_secondary = !self.reservoir.active_secondary; }
        Ok(())
    }

    fn transport_sources(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        coordinate: [u32; 2],
        target: BrushEncodingTarget,
        intent: &crate::raster::contents::Intent,
    ) -> Result<wgpu::BindGroup, GpuRasterError> {
        let timing = self.telemetry.enabled;
        let started = timing.then(web_time::Instant::now);
        let (sources, _) = self.material_bind_group(batch,batch_index, coordinate, target, intent, MaterialInputs::Neighborhood, None, encoder)?;
        self.metrics.material_cpu_ms[4] += elapsed(started);
        Ok(sources)
    }

    fn encode_transport(&mut self, encoder: &mut crate::submission::CommandEncoder, batch_index: usize, batch: &DabBatch, dispatches: &[Dispatch]) {
        if dispatches.is_empty() {
            return;
        }
        let started = self.telemetry.enabled.then(web_time::Instant::now);
        let texture_key = Self::texture_set_key(&batch.style);
        let textures = &self.texture_sets.iter().find(|set| set.key == texture_key).unwrap().bind_group;
        let style_offset = batch_index as u32 * self.style_stride as u32;
        let [width, height] = workgroup(BrushPassPlan::for_style(&batch.style).material);
        let pass = encoder.compute_pass();
        pass.set_bind_group(3, textures, &[]);
        for dispatch in dispatches {
            pass.set_pipeline(self.pipelines.destination_material.kernel(&batch.style, dispatch.entry));
            pass.set_bind_group(0, &dispatch.output, &[style_offset]);
            pass.set_bind_group(1, self.paint_target_binding(&batch.style), &[self.layer_target_offset(batch.target, dispatch.coordinate)]);
            pass.set_bind_group(2, &dispatch.sources, &[dispatch.record_offset]);
            let size = if dispatch.entry == Entry::Reservoir { RESERVOIR_SIZE } else { PAGE_SIZE };
            let extent = if blender(&batch.style).cells {
                let cell = batch.style.blender.cell_size as u32;
                if batch.style.blender.kind == layer_core::BlenderKind::Scatter { [PAGE_SIZE.div_ceil(cell) + 1; 2] }
                else { dispatch.coordinate.map(|coordinate| ((coordinate * PAGE_SIZE) % cell + PAGE_SIZE).div_ceil(cell)) }
            } else { [size; 2] };
            pass.dispatch_workgroups(extent[0].div_ceil(width), extent[1].div_ceil(height), 1);
        }
        self.metrics.material_cpu_ms[3] += elapsed(started);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::DefaultBrushPreset as Preset;

    #[test]
    fn selecting_a_transported_brush_compiles_only_its_page_and_reservoir_kernels() {
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let document = layer_core::Document::new(layer_core::authored::PortableId::random(), 128, 128,
            layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        for (preset, expected) in [(Preset::Smudge, 1), (Preset::NaturalBlender, 1), (Preset::WetRound, 2),
            (Preset::OpaqueGouache, 3), (Preset::LoadedOil, 3), (Preset::PaletteKnife, 3), (Preset::WatercolorWash, 1), (Preset::WetWatercolor, 1)] {
            let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
            renderer.shader_idle(false, false);
            let mut brush = layer_core::default_brush(preset);
            renderer.prepare_startup(&document, &brush, false).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("{preset:?} preparation timed out"));
            let pipelines = &renderer.pipelines;
            let transport = &pipelines.destination_material;
            let ready = transport.kernels.values().filter(|p| p.ready()).count();
            assert_eq!(ready, expected, "{preset:?} compiles its commit, prediction and reservoir kernels");
            let normal = layer_render::DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
            brush.rendering.blend_mode = BrushBlendMode::Multiply;
            let multiply = layer_render::DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
            let operation = BrushPassPlan::for_style(&normal).material;
            for &entry in entries(operation) {
                assert_eq!(std::ptr::eq(transport.kernel(&normal, entry), transport.kernel(&multiply, entry)), !blended(operation),
                    "{preset:?} {entry:?} has a module per blend only when it deposits through the blend");
            }
        }
    }

    #[test]
    fn only_brushes_that_need_the_paper_or_lift_compile_those_kernels() {
        for preset in layer_core::DefaultBrushPreset::ALL {
            let brush = layer_core::default_brush(preset);
            let style = layer_render::DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
            if BrushPassPlan::for_style(&style).material != MaterialOperation::Smudge { continue; }
            let chosen = blender(&style);
            assert!(blenders(MaterialOperation::Smudge).contains(&chosen), "{preset:?} has a kernel");
            assert_eq!(chosen.lift, brush.blender.lift > 0.0, "{preset:?} lifts only when it sets a lift");
            assert_eq!(chosen.paper, brush.blender.kind.touches_paper() && brush.grain.is_some_and(|grain| grain.depth > 0.0),
                "{preset:?} reads the paper only with Texture strength");
        }
    }

    #[test]
    fn transport_kernels_declare_the_workgroup_they_are_dispatched_with() {
        for operation in OPERATIONS {
            for (&entry, blender) in entries(operation).iter().flat_map(|entry| blenders(operation).into_iter().map(move |blender| (entry, blender))) {
                let source = dry_material::material_source(Default::default(), crate::pipeline_device::float_sampling_wgsl(true),
                    module(operation, 0, blender), 0, operation);
                let module = naga::front::wgsl::parse_str(&source).unwrap();
                let kernel = module.entry_points.iter().find(|e| e.name == entry.name()).unwrap();
                let [width, height] = workgroup(operation);
                assert_eq!(kernel.workgroup_size, [width, height, 1], "{operation:?} {entry:?} {blender:?}");
                assert_eq!([PAGE_SIZE % width, PAGE_SIZE % height, RESERVOIR_SIZE % width, RESERVOIR_SIZE % height], [0; 4]);
            }
        }
    }
}
