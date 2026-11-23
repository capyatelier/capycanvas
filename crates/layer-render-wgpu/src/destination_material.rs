//! Smudge, Wet and watercolor read only the pre-batch pages around
//! each page they write, so one compute pass dispatches every page job of a
//! batch. A job writes its whole inactive page: the material result inside its
//! dirty rectangle and the unchanged texel elsewhere, so no page copy precedes
//! it. Watercolor wetness is stored only where a contact deposits.
use super::*;
use dry_material::{Entries, MATERIAL_BLENDS, MATERIAL_SLOTS};
use material_sources::{elapsed, record, rect_words};

pub(super) const OPERATIONS: [MaterialOperation; 3] = [MaterialOperation::Smudge,
    MaterialOperation::Wet, MaterialOperation::Watercolor];

/// The workgroup of an operation's page and reservoir kernels.
pub(super) fn workgroup(operation: MaterialOperation) -> [u32; 2] {
    if operation == MaterialOperation::Smudge { [16, 8] } else { [8, 8] }
}

pub(super) const PICKUP_TAPS: u32 = 5;

pub(super) fn reservoir_page(last: &Dab, extent: [u32; 2]) -> [u32; 2] {
    std::array::from_fn(|i| ([last.center.x, last.center.y][i].max(0.) as u32 / PAGE_SIZE).min(extent[i].saturating_sub(1) / PAGE_SIZE))
}

pub(super) fn exchange_reads(style: &layer_render::DabStyle, dabs: &[Dab], extent: [u32; 2]) -> PixelRect {
    let blur = style.wet_mix.blur.clamp(0., 1.) * 4. + 2.;
    dabs.iter().fold(PixelRect::EMPTY, |reads, dab| {
        let [cos, sin] = dab.rotation;
        let half = [(dab.radii[0] * cos).abs() + (dab.radii[1] * sin).abs(), (dab.radii[0] * sin).abs() + (dab.radii[1] * cos).abs()];
        let pull = dab.material[1].clamp(0., 1.);
        let square = |shift: [f32; 2], margin: f32| layer_core::Rect {
            min: layer_core::Point { x: dab.center.x - half[0] + shift[0] - margin, y: dab.center.y - half[1] + shift[1] - margin },
            max: layer_core::Point { x: dab.center.x + half[0] + shift[0] + margin, y: dab.center.y + half[1] + shift[1] + margin },
        };
        let pickup = pixel_rect(square(dab.motion.map(|motion| -motion * pull), blur), extent);
        let averaged = if style.wet_mix.average > 0. { pixel_rect(square([0.; 2], 2.), extent) } else { PixelRect::EMPTY };
        reads.union(pickup).union(averaged)
    })
}

pub(super) fn job_reads(style: &layer_render::DabStyle, dabs: &[Dab], written: PixelRect, extent: [u32; 2]) -> PixelRect {
    let margin = style.wet_mix.blur.clamp(0., 1.) * 4. + 2.;
    let rect = written.to_rect();
    let grown = |low: [f32; 2], high: [f32; 2]| pixel_rect(layer_core::Rect {
        min: layer_core::Point { x: rect.min.x + low[0] - margin, y: rect.min.y + low[1] - margin },
        max: layer_core::Point { x: rect.max.x + high[0] + margin, y: rect.max.y + high[1] + margin },
    }, extent);
    let pulls = dabs.iter().map(|dab| dab.motion.map(|motion| -motion * dab.material[1].clamp(0., 1.)));
    match BrushPassPlan::for_style(style).material {
        MaterialOperation::Wet => pulls.fold(PixelRect::EMPTY, |reads, pull| reads.union(grown(pull, pull))),
        MaterialOperation::Watercolor => {
            let [low, high] = pulls.fold([[0.; 2]; 2], |[low, high], pull| [std::array::from_fn(|axis| low[axis] + pull[axis].min(0.)),
                std::array::from_fn(|axis| high[axis] + pull[axis].max(0.))]);
            grown(low, high)
        },
        _ => PixelRect::EMPTY,
    }
}

fn beyond(reads: PixelRect, around: [u32; 2]) -> bool {
    if reads.is_empty() { return false; }
    let low = [reads.min_x(), reads.min_y()].map(|at| at / PAGE_SIZE);
    let high = [reads.max_x(), reads.max_y()].map(|at| (at - 1) / PAGE_SIZE);
    (0..2).any(|axis| around[axis].saturating_sub(low[axis]) > 1 || high[axis].saturating_sub(around[axis]) > 1)
}

fn exchanges(batches: &[DabBatch], index: usize, extent: [u32; 2]) -> bool {
    let wet = |batch: &DabBatch| BrushPassPlan::for_style(&batch.style).material == MaterialOperation::Wet;
    let batch = &batches[index];
    wet(batch) && (batch.kind != DabBatchKind::Preview || batches[index + 1..].iter().any(|later| later.kind == DabBatchKind::Preview
        && later.stroke_id == batch.stroke_id && wet(later) && crate::paints(later, extent)))
}

pub(super) fn read_pages(batches: &[DabBatch], index: usize, dabs: &[Dab], extent: [u32; 2], mut read: impl FnMut([u32; 2])) {
    let batch = &batches[index];
    let written = crate::batch_pixel_rect(batch, extent);
    for page in page_coordinates(written) {
        let reads = job_reads(&batch.style, dabs, written.intersect(page_rect(page)), extent);
        if beyond(reads, page) { page_coordinates(reads).for_each(&mut read); }
    }
    if let Some(last) = dabs.last().filter(|_| exchanges(batches, index, extent)) {
        let around = reservoir_page(last, extent);
        crate::source_access::neighborhood(around, TRANSPORT_OFFSETS.map(Some)).for_each(&mut read);
        let reads = exchange_reads(&batch.style, dabs, extent);
        if beyond(reads, around) { page_coordinates(reads).for_each(&mut read); }
    }
}

fn page_taps(style: &layer_render::DabStyle) -> u32 {
    let blurred = style.wet_mix.blur.clamp(0., 1.) * 4. >= 0.01;
    match BrushPassPlan::for_style(style).material {
        MaterialOperation::Wet => if blurred { PICKUP_TAPS } else { 1 },
        MaterialOperation::Watercolor => if blurred { 8 } else { 4 },
        _ => 0,
    }
}

fn job_extent(style: &layer_render::DabStyle, coordinate: [u32; 2]) -> [u32; 2] {
    if !blender(style).cells { return [PAGE_SIZE; 2]; }
    let cell = style.blender.cell_size as u32;
    if style.blender.kind == layer_core::BlenderKind::Scatter { [PAGE_SIZE.div_ceil(cell) + 1; 2] }
    else { coordinate.map(|coordinate| ((coordinate * PAGE_SIZE) % cell + PAGE_SIZE).div_ceil(cell)) }
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
    Gather,
}

impl Entry {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Color => "transport_color",
            Self::Coverage => "transport_coverage",
            Self::Reservoir => "reservoir_exchange",
            Self::Watercolor => "transport_watercolor",
            Self::Gather => "transport_gather",
        }
    }

    /// The output layout: color, with coverage, or with coverage and wetness.
    fn outputs(self) -> usize {
        match self {
            Self::Coverage => 1,
            Self::Watercolor => 2,
            Self::Color | Self::Reservoir | Self::Gather => 0,
        }
    }
}

pub(super) fn entries(operation: MaterialOperation) -> &'static [Entry] {
    match operation {
        MaterialOperation::Wet => &[Entry::Color, Entry::Coverage, Entry::Reservoir, Entry::Gather],
        MaterialOperation::Watercolor => &[Entry::Watercolor, Entry::Gather],
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

pub(super) fn rewritten(style: &layer_render::DabStyle, coordinate: [u32; 2], local: PixelRect, extent: [u32; 2]) -> Option<PixelRect> {
    if !blender(style).cells { return Some(local); }
    let cell = style.blender.cell_size as u32;
    if style.blender.kind == layer_core::BlenderKind::Scatter {
        let page = page_rect(coordinate);
        return (page.intersect(PixelRect::full(extent)) == page).then(|| local.expand(cell - 1, [PAGE_SIZE; 2]));
    }
    if local.is_empty() { return Some(local); }
    let origin = coordinate.map(|at| at * PAGE_SIZE);
    let low = |axis: usize, at: u32| ((origin[axis] + at) / cell * cell).saturating_sub(origin[axis]);
    let high = |axis: usize, at: u32| ((origin[axis] + at).div_ceil(cell) * cell - origin[axis]).min(PAGE_SIZE);
    Some(PixelRect::new(low(0, local.min_x()), low(1, local.min_y()), high(0, local.max_x()), high(1, local.max_y())))
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
        Entry::Reservoir | Entry::Watercolor | Entry::Gather => true,
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
    extent: [u32; 2],
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
        let declaration = preview.then(|| self.preview_declaration.clone()).flatten();
        let exchange = dabs.first().zip(dabs.last()).filter(|_| operation == MaterialOperation::Wet
            && (!preview || declaration.as_ref().is_some_and(|declaration| declaration.reservoir_consumed(batch_index))));
        let gathered = exchange.and_then(|(_, last)| {
            let reads = exchange_reads(&batch.style, dabs, extent);
            beyond(reads, reservoir_page(last, extent)).then(|| page_coordinates(reads).collect::<Vec<_>>())
        });
        let taps = page_taps(&batch.style);
        let rows = if operation == MaterialOperation::Wet { dabs.len() as u32 } else { 1 };
        let far: Vec<Option<Vec<[u32; 2]>>> = jobs.iter().map(|job| {
            let reads = job_reads(&batch.style, dabs, job.local.translated(job.coordinate.map(|at| at * PAGE_SIZE)), extent);
            beyond(reads, job.coordinate).then(|| page_coordinates(reads).collect())
        }).collect();
        let seeds: Vec<_> = jobs.iter().map(|job| operation == MaterialOperation::Watercolor && !self.watercolor_written.contains(&job.coordinate)).collect();
        self.watercolor_written.extend(jobs.iter().zip(&seeds).filter(|(_, seed)| **seed).map(|(job, _)| job.coordinate));
        self.metrics.material_cpu_ms[0] += elapsed(started);
        let mut first_record = self.prepare_records(jobs.iter().zip(&seeds).zip(&far).map(|((job, &seed), reads)| {
            record([job.dabs.start, job.dabs.end - job.dabs.start, u32::from(reads.is_some()), u32::from(seed)], [rect_words(job.local)])
        }).chain(exchange.map(|(first, _)| {
            let color = first.color_rgba_linear;
            record([u32::from(gathered.is_some()), 0, 0, 0], [[color[0], color[1], color[2], batch.style.wet_mix.amount_of_paint * first.material[2]].map(f32::to_bits)])
        })).chain(gathered.iter().flatten().enumerate().map(|(index, _)| record([2, 0, 0, u32::from(index == 0)], [])))
        .chain(jobs.iter().zip(&far).filter_map(|(job, reads)| reads.as_ref().map(|reads| (job, reads))).flat_map(|(job, reads)| (0..reads.len())
            .map(move |index| record([0, taps, rows, u32::from(index == 0)], [rect_words(job.local), [job.coordinate[0], job.coordinate[1], 0, 0]])))), encoder)?;
        let mut page_gathers = first_record + jobs.len() + usize::from(exchange.is_some()) + gathered.as_ref().map_or(0, Vec::len);
        let mut publications=Vec::with_capacity(jobs.len()*3+usize::from(exchange.is_some()));
        let mut written=Vec::with_capacity(jobs.len()*3+usize::from(exchange.is_some()));
        let mut dispatches = Vec::with_capacity(jobs.len());
        let intents=jobs.iter().map(|job| {
            let intent=self.raster_intent(batch.target,RasterPlane::Color,job.coordinate)?;
            let wet_intent=(operation==MaterialOperation::Watercolor).then(||self.raster_intent(batch.target,RasterPlane::WatercolorWetness,job.coordinate)).transpose()?;
            Ok((intent,wet_intent))
        }).collect::<Result<Vec<_>,GpuRasterError>>()?;
        let source_keys=jobs.iter().zip(&intents).zip(&far).map(|((job,(intent,_)),reads)| {
            let mut far=Vec::new();
            let gathers=reads.iter().flatten().map(|page| {
                let intent=self.raster_intent(batch.target,RasterPlane::Color,*page)?;
                self.transport_sources(encoder,batch_index,batch,*page,target,&intent,MaterialInputs::Donor,Vec::new(),Some(&mut far))
            }).collect::<Result<Vec<_>,GpuRasterError>>()?;
            Ok((self.transport_sources(encoder,batch_index,batch,job.coordinate,target,intent,MaterialInputs::Neighborhood,far,None)?,gathers))
        }).collect::<Result<Vec<_>,GpuRasterError>>()?;
        for (((index,job),(mut sources,gathers)),(intent,wet_intent)) in jobs.iter().enumerate().zip(source_keys).zip(intents) {
            let color_ticket=preview.then(||self.preview_material_version(batch,batch_index,RasterPlane::Color,job.coordinate,scene::PreviewPass::Color,&intent)).transpose()?;
            let coverage_ticket=(preview && job.coverage_index.is_some()).then(||self.preview_material_version(batch,batch_index,RasterPlane::Color,job.coordinate,scene::PreviewPass::Coverage,&intent)).transpose()?;
            let wetness_ticket=wet_intent.as_ref().filter(|_|preview).map(|wet_intent|self.preview_material_version(batch,batch_index,RasterPlane::WatercolorWetness,job.coordinate,scene::PreviewPass::Wetness,wet_intent)).transpose()?;
            let receipt=encoder.submission();
            if let Some(reads)=&far[index] {
                let size=[taps*PAGE_SIZE,rows*PAGE_SIZE];
                if size.iter().any(|side|*side>self.device.limits().max_texture_dimension_2d) {return Err(GpuRasterError::Invariant("a material job gathers more taps than its tap field holds"));}
                if self.gathered_taps.as_ref().is_none_or(|field|field.size().width<size[0] || field.size().height<size[1]) {
                    self.gathered_taps=Some(resident::Resident::unwritten(create_page_surface(&self.device,&self.float_texture_layout,&self.sampler,size,
                        self.device.working_format(),"layer gathered material taps",&self.raster_tiers.paint)));
                }
                let write=self.gathered_taps.as_mut().ok_or(GpuRasterError::Invariant("gathered material taps are not prepared"))?
                    .overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                let output=self.pipelines.destination_material.output(&self.device,&self.style_buffer,&write,None,None);
                for (page,gather) in reads.iter().zip(gathers) {
                    let (gather,_)=write.value().material.inputs(gather,|key|create_material_inputs(&self.device,&self.material_layout,self.material_records.binding().size,key));
                    dispatches.push(Dispatch {entry:Entry::Gather,output:output.clone(),sources:gather,coordinate:*page,record_offset:self.material_records.offset(page_gathers),extent:size});
                    page_gathers+=1;
                }
                sources.key.1[0]=write.value().view.clone();
                write.publish(layer_core::raster::ContentName::fresh());
            }
            let delta=self.color_delta(batch,false,target,job.coordinate,job.local,&receipt);
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
            let held=&coverage.as_ref().unwrap_or(&destination).value().material;
            self.material_holders.hold((!preview).then_some(batch.target),job.coordinate,held);
            let (sources,_)=held.inputs(sources,|key|create_material_inputs(&self.device,&self.material_layout,self.material_records.binding().size,key));
            let entry=if wetness.is_some() {Entry::Watercolor}else if coverage.is_some() {Entry::Coverage}else {Entry::Color};
            let next_publications=[
                Some(MaterialPublication {kind:MaterialOutputKind::Color,coordinate:job.coordinate,preview:color_ticket.clone(),input:intent.clone(),
                    encoding:destination.encoded(color_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone())).certified(delta)}),
                coverage.map(|write|MaterialPublication {kind:MaterialOutputKind::Coverage,coordinate:job.coordinate,preview:coverage_ticket.clone(),input:intent,
                    encoding:write.encoded(coverage_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()))}),
                wetness.map(|write|MaterialPublication {kind:MaterialOutputKind::Wetness,coordinate:job.coordinate,preview:wetness_ticket.clone(),input:wet_intent.unwrap(),
                    encoding:write.encoded(wetness_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()))}),
            ];
            dispatches.push(Dispatch {entry,output,sources,coordinate:job.coordinate,record_offset:self.material_records.offset(first_record),extent:job_extent(&batch.style,job.coordinate)});
            publications.extend(next_publications.into_iter().flatten());
            first_record += 1;
        }
        let mut predicted_exchange = None;
        if let Some((_, last)) = exchange {
            let coordinate = reservoir_page(last, extent);
            let intent=self.raster_intent(batch.target,RasterPlane::Color,coordinate)?;
            let mut donors=Vec::new();
            let mut sources=self.transport_sources(encoder,batch_index,batch,coordinate,target,&intent,MaterialInputs::Exchange,Vec::new(),Some(&mut donors))?;
            let gathers=gathered.iter().flatten().map(|page| {
                let intent=self.raster_intent(batch.target,RasterPlane::Color,*page)?;
                self.transport_sources(encoder,batch_index,batch,*page,target,&intent,MaterialInputs::Donor,Vec::new(),Some(&mut donors))
            }).collect::<Result<Vec<_>,_>>()?;
            if let Some(declaration)=&declaration {declaration.accept_reservoir_inputs(batch_index,donors)?;}
            let receipt=encoder.submission();
            if let Some(pages)=&gathered {
                let size=[PICKUP_TAPS*RESERVOIR_SIZE,(RESERVOIR_SIZE+1)*dabs.len() as u32];
                if size[1]>self.device.limits().max_texture_dimension_2d {return Err(GpuRasterError::Invariant("a wet exchange gathers more contacts than its tap field holds"));}
                if self.reservoir.taps.as_ref().is_none_or(|taps|taps.size().height<size[1]) {
                    self.reservoir.taps=Some(resident::Resident::unwritten(create_page_surface(&self.device,&self.float_texture_layout,&self.sampler,size,
                        self.device.working_format(),"layer gathered reservoir taps",&self.raster_tiers.paint)));
                }
                let write=self.reservoir.taps.as_mut().ok_or(GpuRasterError::Invariant("gathered reservoir taps are not prepared"))?
                    .overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                let output=self.pipelines.destination_material.output(&self.device,&self.style_buffer,&write,None,None);
                for (index,(page,gather)) in pages.iter().zip(gathers).enumerate() {
                    let (gather,_)=write.value().material.inputs(gather,|key|create_material_inputs(&self.device,&self.material_layout,self.material_records.binding().size,key));
                    dispatches.push(Dispatch {entry:Entry::Reservoir,output:output.clone(),sources:gather,coordinate:*page,
                        record_offset:self.material_records.offset(first_record+1+index),extent:size});
                }
                sources.key.1[9]=write.value().view.clone();
                write.publish(layer_core::raster::ContentName::fresh());
            }
            let write=if let Some(declaration)=&declaration {
                let incoming=declaration.reservoir_input(batch_index)?.map(|(version,_)|version);
                let [first,second]=self.reservoir.predicted.as_mut().ok_or(GpuRasterError::Invariant("predicted reservoir is not prepared"))?;
                if incoming.as_ref().is_some_and(|incoming|first.has_name(incoming)) {second}else {first}
            }else if self.reservoir.active_secondary {&mut self.reservoir.primary}else {&mut self.reservoir.secondary}
                .overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
            let output=self.pipelines.destination_material.output(&self.device,&self.style_buffer,&write,None,None);
            let (sources,_)=write.value().material.inputs(sources,|key|create_material_inputs(&self.device,&self.material_layout,self.material_records.binding().size,key));
            dispatches.push(Dispatch {entry:Entry::Reservoir,output,sources,coordinate,record_offset:self.material_records.offset(first_record),extent:[RESERVOIR_SIZE;2]});
            if let Some(declaration)=&declaration {predicted_exchange=Some(write.encoded(declaration.reservoir_output(batch_index)?));}
            else {publications.push(MaterialPublication {kind:MaterialOutputKind::Reservoir,coordinate,preview:None,input:intent,
                encoding:write.encoded(layer_core::raster::ContentName::fresh())});}
        }
        self.encode_transport(encoder, batch_index, batch, &dispatches);
        let actual=encoder.submission();written.extend(publications.drain(..).map(|publication:MaterialPublication|publication.encoded(&actual)));
        if let Some(encoding)=predicted_exchange {encoding.publish(&actual);}
        self.publish_material(batch,written)?;
        if exchange.is_some() && !preview { self.reservoir.active_secondary = !self.reservoir.active_secondary; }
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
        inputs: MaterialInputs<'_>,
        far: Vec<scene::Donor>,
        donors: Option<&mut Vec<scene::Donor>>,
    ) -> Result<crate::bindings::MaterialSources, GpuRasterError> {
        let timing = self.telemetry.enabled;
        let started = timing.then(web_time::Instant::now);
        let sources = self.material_bind_group(batch,batch_index, coordinate, target, intent, inputs, None, far, donors, encoder)?;
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
            pass.dispatch_workgroups(dispatch.extent[0].div_ceil(width), dispatch.extent[1].div_ceil(height), 1);
        }
        self.metrics.material_cpu_ms[3] += elapsed(started);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::DefaultBrushPreset as Preset;
    use layer_core::{Point, StrokeId};
    use crate::layer_tests::{dab, placement::{paint_document, target}};
    use crate::test_support::{dab_batch, packet, submit_ready};

    fn exchanging_wet() -> layer_render::DabStyle {
        let mut style = crate::tests::test_style(BrushExecution::Wet);
        style.wet_mix = layer_core::BrushWetMix { amount_of_paint: 0.5, ..Default::default() };
        style
    }

    fn wet_contact(x: f32, y: f32, radius: f32, motion: [f32; 2]) -> Dab {
        Dab { center: Point { x, y }, radii: [radius; 2], motion, material: [0., 0., 1., 0.], ..dab([0., 0., 1., 1.]) }
    }

    fn wet_batch(target: SourceTarget, contact: &Dab, first: usize, start: bool) -> DabBatch {
        DabBatch { stroke_id: StrokeId(2), stroke_start: start, stroke_end: false, first_dab: first as u32,
            ..dab_batch(target, exchanging_wet(), contact.bounds()) }
    }

    fn paint_dry(r: &mut WgpuRasterizer, document: &layer_core::Document, center: [f32; 2], radius: f32, color: [f32; 4]) {
        let contact = Dab { center: Point { x: center[0], y: center[1] }, radii: [radius; 2], ..dab(color) };
        let extent = document.composition().size;
        submit_ready(r, FramePacket { dabs: &[contact], dab_batches: &[dab_batch(target(document), crate::tests::test_style(BrushExecution::Dry), contact.bounds())],
            ..packet(document.scene(), extent) }).unwrap();
    }

    fn carried(r: &WgpuRasterizer, texel: [usize; 2]) -> [f32; 4] {
        let state = r.reservoir.state.as_ref().unwrap();
        let surface = if r.reservoir.active_secondary { &r.reservoir.secondary } else { &r.reservoir.primary };
        let pixels = crate::test_support::float_pixels(r, &surface.read(state.version.clone(), None).unwrap().value().texture);
        pixels[texel[1] * RESERVOIR_SIZE as usize + texel[0]]
    }

    fn painted(r: &WgpuRasterizer, target: SourceTarget, at: [u32; 2]) -> [f32; 4] {
        let coordinate = at.map(|value| value / PAGE_SIZE);
        let page = r.paint_layers.iter().find(|layer| layer.id == target).unwrap().pages.iter().find(|page| page.coordinate == coordinate).unwrap();
        let pixels = crate::test_support::float_pixels(r, &r.read_color_page(target, page, false, None).unwrap().value().texture);
        pixels[(at[1] % PAGE_SIZE * PAGE_SIZE + at[0] % PAGE_SIZE) as usize]
    }

    fn predicted(r: &WgpuRasterizer, target: SourceTarget, at: [u32; 2]) -> [f32; 4] {
        let coordinate = at.map(|value| value / PAGE_SIZE);
        let expected = r.preview_expected(target, layer_core::raster::RasterPlane::Color, coordinate, crate::scene::PreviewStage::Final).unwrap();
        let read = r.preview_page(coordinate).unwrap().active().read(expected, None).unwrap();
        crate::test_support::float_pixels(r, &read.value().texture)[(at[1] % PAGE_SIZE * PAGE_SIZE + at[0] % PAGE_SIZE) as usize]
    }

    fn assert_close(actual: [f32; 4], expected: [f32; 4], context: &str) {
        assert!(actual.iter().zip(expected).all(|(a, e)| (a - e).abs() < 0.01), "{context}: {actual:?} != {expected:?}");
    }

    #[test]
    fn a_new_wet_stroke_exchanges_pigment_across_its_whole_reservoir() {
        let extent = [256, 128];
        let document = paint_document(extent, "opening wet exchange");
        let target = target(&document);
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        submit_ready(&mut r, FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
        paint_dry(&mut r, &document, [48., 64.], 40., [1., 0., 0., 1.]);
        let dabs = [wet_contact(48., 64., 20., [0.; 2]), wet_contact(176., 64., 20., [128., 0.])];
        submit_ready(&mut r, FramePacket { dabs: &dabs, dab_batches: &[wet_batch(target, &dabs[0], 0, true)],
            ..packet(document.scene(), extent) }).unwrap();
        assert_close(carried(&r, [32, 32]), [2. / 3., 0., 1. / 3., 0.75], "the opening contact takes red up into the middle of its reservoir");
        submit_ready(&mut r, FramePacket { dabs: &dabs, dab_batches: &[wet_batch(target, &dabs[1], 1, false)],
            ..packet(document.scene(), extent) }).unwrap();
        assert_close(painted(&r, target, [176, 64]), [0.5, 0., 0.25, 0.75], "the next contact lays the red it took up on clear paint");
    }

    fn broad_exchange(style: &layer_render::DabStyle, shift: f32, donor: [f32; 2], texel: [usize; 2]) -> [f32; 4] {
        let extent = [1280, 512];
        let document = paint_document(extent, "broad wet exchange");
        let target = target(&document);
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        submit_ready(&mut r, FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
        paint_dry(&mut r, &document, [donor[0] + shift, donor[1]], 6., [1., 0., 0., 1.]);
        let dabs = [wet_contact(900. + shift, 256., 20., [0.; 2]), wet_contact(600. + shift, 256., 500., [-300., 0.])];
        for (index, contact) in dabs.iter().enumerate() {
            submit_ready(&mut r, FramePacket { dabs: &dabs, dab_batches: &[DabBatch { style: style.clone(), ..wet_batch(target, contact, index, index == 0) }],
                ..packet(document.scene(), extent) }).unwrap();
        }
        carried(&r, texel)
    }

    #[test]
    fn a_broad_wet_contact_takes_up_paint_beyond_the_pages_around_its_center() {
        let bound = broad_exchange(&exchanging_wet(), 167., [202., 264.], [6, 32]);
        assert_close(bound, [2. / 3., 0., 1. / 3., 0.75], "a donor two pages from the contact's center page is taken up when that page is bound");
        assert_close(broad_exchange(&exchanging_wet(), 0., [202., 264.], [6, 32]), bound, "the same donor is taken up wherever the page grid falls");
    }

    #[test]
    fn a_mixing_wet_contact_averages_paint_under_a_far_tap() {
        let mut style = exchanging_wet();
        style.wet_mix.average = 1.;
        let bound = broad_exchange(&style, 167., [173.4, 273.6], [32, 32]);
        assert_close(bound, [1., 0., 0., 1. / 15.], "the one red tap of fifteen sets the averaged load when its page is bound");
        assert_close(broad_exchange(&style, 0., [173.4, 273.6], [32, 32]), bound, "the same far tap is averaged wherever the page grid falls");
    }

    #[test]
    fn a_pulling_wet_contact_picks_up_paint_beyond_the_pages_around_its_page() {
        let extent = [1024, 256];
        let laid = |shift: f32| {
            let document = paint_document(extent, "pulled wet pickup");
            let target = target(&document);
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            submit_ready(&mut r, FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
            paint_dry(&mut r, &document, [212.5 + shift, 128.], 12., [1., 0., 0., 1.]);
            let pulled = Dab { material: [0., 1., 1., 0.], ..wet_contact(512.5 + shift, 128., 16., [300., 0.]) };
            let dabs = [wet_contact(612.5 + shift, 220., 4., [0.; 2]), pulled];
            for (index, contact) in dabs.iter().enumerate() {
                submit_ready(&mut r, FramePacket { dabs: &dabs, dab_batches: &[wet_batch(target, contact, index, index == 0)], ..packet(document.scene(), extent) }).unwrap();
            }
            painted(&r, target, [(512. + shift) as u32, 128])
        };
        let bound = laid(100.);
        assert_close(bound, [0.5, 0., 0.5, 1.], "a pull three hundred pixels back mixes the red there into the paint when its page is bound");
        assert_close(laid(0.), bound, "the same pull mixes the same red wherever the page grid falls");
    }

    #[test]
    fn a_dragging_watercolor_contact_traces_paint_beyond_the_pages_around_its_page() {
        let extent = [1024, 256];
        let laid = |shift: f32| {
            let document = paint_document(extent, "traced watercolor drag");
            let target = target(&document);
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            submit_ready(&mut r, FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
            paint_dry(&mut r, &document, [212.5 + shift, 128.], 12., [1., 0., 0., 1.]);
            paint_dry(&mut r, &document, [512.5 + shift, 128.], 30., [0., 0., 1., 1.]);
            let mut style = crate::tests::test_style(BrushExecution::Watercolor);
            style.wet_mix = layer_core::BrushWetMix { amount_of_paint: 0., dilution: 1., ..Default::default() };
            let dragged = Dab { material: [0., 1., 1., 0.], ..wet_contact(512.5 + shift, 128., 16., [300., 0.]) };
            submit_ready(&mut r, FramePacket { dabs: &[dragged], dab_batches: &[DabBatch { style, ..wet_batch(target, &dragged, 0, true) }],
                ..packet(document.scene(), extent) }).unwrap();
            painted(&r, target, [(512. + shift) as u32, 128])
        };
        let bound = laid(100.);
        assert!(bound[0] > 0.2, "a drag three hundred pixels back carries the red there when its page is bound: {bound:?}");
        assert_close(laid(0.), bound, "the same drag carries the same red across two page edges");
    }

    fn donor_tile(color: [u8; 4]) -> layer_core::raster::RasterTile {
        let plane = layer_core::raster::RasterPlane::Color;
        let bytes: Vec<u8> = (0..PAGE_SIZE * PAGE_SIZE).flat_map(|_| color).collect();
        layer_core::raster::RasterTile::backed(layer_core::raster::TileBlob::encode(plane.descriptor(Default::default()), &bytes).unwrap())
    }

    #[test]
    fn a_predicted_wet_reservoir_is_named_by_where_its_donors_lie() {
        let extent = [2048, 256];
        let mut document = paint_document(extent, "addressed wet donors");
        let target = target(&document);
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let dabs = [Dab { material: [0., 1., 1., 0.], ..wet_contact(1536., 128., 8., [1024., 0.]) }, wet_contact(1600., 128., 8., [64., 0.])];
        let batches: Vec<_> = dabs.iter().enumerate().map(|(index, contact)| DabBatch { kind: DabBatchKind::Preview, ..wet_batch(target, contact, index, index == 0) }).collect();
        let [red, green] = [[255, 0, 0, 255], [0, 255, 0, 255]].map(donor_tile);
        let mut pickup = |left: &layer_core::raster::RasterTile, right: &layer_core::raster::RasterTile| {
            let mut data = layer_core::raster::RasterData::default();
            for (column, tile) in [(1, left), (2, right)] {
                data.tiles.insert(layer_core::raster::TileKey { plane: layer_core::raster::RasterPlane::Color, coordinate: [column, 0] }, tile.clone());
            }
            crate::layer_tests::placement::paint_mut(&mut document).raster = layer_core::raster::RasterRevision::backed(data);
            submit_ready(&mut r, FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
            submit_ready(&mut r, FramePacket { dabs: &dabs, dab_batches: &batches, composite_all: false, ..packet(document.scene(), extent) }).unwrap();
            let name = r.preview_declaration.as_ref().unwrap().reservoir_output(0).unwrap();
            let surface = r.reservoir.predicted.as_ref().unwrap().iter().find(|surface| surface.has_name(&name)).unwrap();
            let texels = crate::test_support::float_pixels(&r, &surface.read(name.clone(), None).unwrap().value().texture);
            (name, [texels[32 * RESERVOIR_SIZE as usize + 8], texels[32 * RESERVOIR_SIZE as usize + 56]])
        };
        let (first, [left, right]) = pickup(&red, &green);
        assert!(left[0] > 0.1 && right[1] > 0.1, "the pickup straddles both donors: {left:?} {right:?}");
        let (swapped, [left, right]) = pickup(&green, &red);
        assert!(left[1] > 0.1 && right[0] > 0.1, "the swapped donors swap the pickup: {left:?} {right:?}");
        assert_ne!(swapped.matches(&first), Some(true), "swapping the donors' pages renames the predicted reservoir");
    }

    #[test]
    fn a_replaced_wet_prediction_gathers_past_a_page_it_no_longer_paints() {
        let extent = [1536, 256];
        let style = layer_render::DabStyle { opacity: 0.5, ..exchanging_wet() };
        let broad = [wet_contact(768., 128., 700., [0.; 2]), wet_contact(776., 128., 700., [8., 0.])];
        let pulled = [Dab { material: [0., 1., 1., 0.], ..wet_contact(1300., 128., 16., [1100., 0.]) }, wet_contact(1350., 128., 16., [50., 0.])];
        let predict = |r: &mut WgpuRasterizer, document: &layer_core::Document, dabs: &[Dab]| {
            let batches: Vec<_> = dabs.iter().enumerate().map(|(index, contact)| DabBatch { kind: DabBatchKind::Preview, style: style.clone(),
                ..wet_batch(target(document), contact, index, index == 0) }).collect();
            submit_ready(r, FramePacket { dabs, dab_batches: &batches, composite_all: false, ..packet(document.scene(), extent) })
        };
        let laid = |replaced: bool| {
            let document = paint_document(extent, "replaced wet prediction");
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            submit_ready(&mut r, FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
            paint_dry(&mut r, &document, [200., 128.], 30., [1., 0., 0., 1.]);
            if replaced { predict(&mut r, &document, &broad).unwrap(); }
            predict(&mut r, &document, &pulled).unwrap_or_else(|error| panic!("replaced={replaced}: the pulled prediction submits: {error}"));
            predicted(&r, target(&document), [1350, 128])
        };
        assert_close(laid(true), laid(false), "a prediction that replaces a broader one lays what a fresh prediction lays");
    }

    #[test]
    fn a_wet_exchange_reads_the_pages_around_its_last_contact_whatever_its_damage() {
        let extent = [1536, 256];
        let document = paint_document(extent, "wet exchange beyond its damage");
        let target = target(&document);
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        submit_ready(&mut r, FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
        let dabs = [wet_contact(384., 128., 8., [0.; 2]), wet_contact(640., 640., 8., [256., 512.])];
        let batch = DabBatch { dab_count: 2, damage: dabs[0].bounds(), ..wet_batch(target, &dabs[0], 0, true) };
        submit_ready(&mut r, FramePacket { dabs: &dabs, dab_batches: &[batch], ..packet(document.scene(), extent) })
            .expect("a Wet batch whose last contact lies off the canvas exchanges around the page it clamps to");
        assert!(r.reservoir.state.is_some(), "the exchange published the stroke's reservoir");
    }

    #[test]
    fn a_wet_prediction_carries_paint_from_one_microbatch_to_the_next() {
        let extent = [512, 256];
        let dabs = [wet_contact(420., 128., 20., [0.; 2]), wet_contact(120., 128., 20., [-300., 0.]), wet_contact(128., 128., 20., [8., 0.]),
            wet_contact(136., 128., 20., [8., 0.]), wet_contact(400., 128., 20., [264., 0.])];
        let receiver = |target: SourceTarget, kind: DabBatchKind| DabBatch { kind, ..wet_batch(target, &dabs[4], 4, false) };
        let split = |target: SourceTarget, kind: DabBatchKind| [DabBatch { dab_count: 3, damage: dabs[1].bounds().union(dabs[3].bounds()), kind,
            ..wet_batch(target, &dabs[1], 1, false) }, receiver(target, kind)];
        let stroke = || {
            let document = paint_document(extent, "wet prediction microbatches");
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            submit_ready(&mut r, FramePacket { reset_layers: true, ..packet(document.scene(), extent) }).unwrap();
            paint_dry(&mut r, &document, [128., 128.], 60., [1., 0., 0., 1.]);
            submit_ready(&mut r, FramePacket { dabs: &dabs, dab_batches: &[wet_batch(target(&document), &dabs[0], 0, true)], ..packet(document.scene(), extent) }).unwrap();
            (document, r)
        };
        let predict = |r: &mut WgpuRasterizer, document: &layer_core::Document, batches: &[DabBatch]| {
            submit_ready(r, FramePacket { dabs: &dabs, dab_batches: batches, composite_all: false, ..packet(document.scene(), extent) }).unwrap();
            predicted(r, target(document), [400, 128])
        };
        let (document, mut twin) = stroke();
        submit_ready(&mut twin, FramePacket { dabs: &dabs, dab_batches: &split(target(&document), DabBatchKind::Persistent), ..packet(document.scene(), extent) }).unwrap();
        let persistent = painted(&twin, target(&document), [400, 128]);
        assert!(persistent[0] > 0.1, "the persistent twin lays red it took up: {persistent:?}");
        let (document, mut r) = stroke();
        let target = target(&document);
        let real = (r.reservoir.state.as_ref().unwrap().version.clone(), r.reservoir.active_secondary, carried(&r, [32, 32]));
        let prediction = predict(&mut r, &document, &split(target, DabBatchKind::Preview));
        assert_close(prediction, persistent, "the predicted fourth contact lays the paint the first three took up");
        assert_eq!(r.reservoir.state.as_ref().unwrap().version.matches(&real.0), Some(true), "a prediction keeps the stroke's reservoir version");
        assert_eq!((r.reservoir.active_secondary, carried(&r, [32, 32])), (real.1, real.2), "a prediction leaves the stroke's reservoir unchanged");
        let pickup = r.preview_declaration.as_ref().unwrap().reservoir_output(0).unwrap();
        assert!(r.reservoir.predicted.as_ref().unwrap().iter().any(|surface| surface.has_name(&pickup)), "the predicted pickup publishes its own reservoir");
        let alone = predict(&mut r, &document, &[receiver(target, DabBatchKind::Preview)]);
        assert!(alone[0] < 0.05, "a prediction without the pickup lays the stroke's reservoir: {alone:?}");
        assert_close(predict(&mut r, &document, &split(target, DabBatchKind::Preview)), prediction, "the original prediction comes back unchanged");
        paint_dry(&mut r, &document, [128., 128.], 60., [0., 1., 0., 1.]);
        let recoloured = predict(&mut r, &document, &split(target, DabBatchKind::Preview));
        assert_ne!(r.preview_declaration.as_ref().unwrap().reservoir_output(0).unwrap().matches(&pickup), Some(true), "a new donor renames the predicted pickup");
        assert!(recoloured[1] > 0.1 && recoloured[0] < 0.05, "the fourth contact lays the new donor: {recoloured:?}");
    }

    #[test]
    fn selecting_a_transported_brush_compiles_only_its_page_and_reservoir_kernels() {
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let document = layer_core::Document::new(layer_core::authored::PortableId::random(), 128, 128,
            layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        for (preset, expected) in [(Preset::Smudge, 1), (Preset::NaturalBlender, 1), (Preset::WetRound, 3),
            (Preset::OpaqueGouache, 4), (Preset::LoadedOil, 4), (Preset::PaletteKnife, 4), (Preset::WatercolorWash, 2), (Preset::WetWatercolor, 2)] {
            let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
            renderer.shader_idle(false, false);
            let mut brush = layer_core::default_brush(preset);
            renderer.prepare_startup(&document, &brush, false).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("{preset:?} preparation timed out"));
            let pipelines = &renderer.pipelines;
            let transport = &pipelines.destination_material;
            let ready = transport.kernels.values().filter(|p| p.ready()).count();
            assert_eq!(ready, expected, "{preset:?} compiles its commit, prediction, reservoir and gather kernels");
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
