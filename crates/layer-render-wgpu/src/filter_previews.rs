//! Idle-time GPU previews: a chunked source probe per revision, shared source
//! crops and pipelines, and small asynchronous image readbacks.
use super::metadata::PreviewMetadata;
use super::*;
use layer_core::{SceneSnapshot,SceneScope,SceneView};
use layer_core::authored::{SceneIndex,PortableId,Occurrence,OccurrenceContent,EffectApplication};
use layer_render::{FilterPreviewImage, FilterPreviewRequest, FilterPreviewSource};
use std::{collections::HashMap, sync::Arc};
use wgpu::util::DeviceExt;

const PROBE_BUDGET: Duration = Duration::from_millis(2);

fn probe_batch_size(tiles: usize, encoded: Duration, completed: Duration, gpu_timed: bool) -> usize {
    let completion_budget = if gpu_timed { PROBE_BUDGET } else { Duration::from_millis(16) };
    let capacity = |budget: Duration, elapsed: Duration| budget.as_secs_f64() / elapsed.as_secs_f64().max(0.000_001);
    ((tiles as f64 * capacity(PROBE_BUDGET, encoded).min(capacity(completion_budget, completed))) as usize)
        .clamp(1, (tiles * 2).clamp(1, 16))
}
fn probe_tiles(extent: [u32; 2]) -> Vec<([u32; 2], u32)> {
    let mut tiles = Vec::new();
    for y in 0..extent[1].div_ceil(PAGE_SIZE) {
        for x in 0..extent[0].div_ceil(PAGE_SIZE) {
            let tile = [x, y];
            let zig = std::array::from_fn::<_, 2, _>(|i| {
                let center = extent[i] / 2;
                let nearest = center.clamp(tile[i] * PAGE_SIZE, ((tile[i] + 1) * PAGE_SIZE).min(extent[i]) - 1);
                nearest.abs_diff(center) * 2 + u32::from(nearest < center)
            });
            tiles.push((tile, u32::MAX - ((zig[1] << 16) | zig[0])));
        }
    }
    tiles.sort_unstable_by_key(|(_, maximum)| std::cmp::Reverse(*maximum));
    tiles
}

type Image = (wgpu::Texture, wgpu::TextureView);
enum Ready {
    Probe { scores: Result<[u32; 2], GpuRasterError>, batch_size: usize },
    Pixels(Result<ReadbackImage, GpuRasterError>),
}
/// Source revision, insertion scope, extent and blend space.
type SourceKey = (u64, (u64,FilterPreviewSource), [u32; 2], layer_core::BlendSpace);
pub(crate) struct FilterPreviews {
    scene: Scene,
    source_scene: Scene,
    probe_next: usize,
    probe_tiles: Vec<([u32; 2], u32)>,
    probe_batch_size: usize,
    probe_timing: Option<(wgpu::QuerySet, wgpu::Buffer)>,
    probe_winner: Option<wgpu::Buffer>,
    cancelled: bool,
    programs: HashMap<Arc<str>, OccurrenceHandle>,
    program_snapshot:Option<Arc<SceneSnapshot>>,
    probe: Deferred<wgpu::ComputePipeline>,
    mask: Image,
    source: Option<Image>,
    key: Option<SourceKey>,
    source_layers: Vec<PreviewMetadata>,
    point: Option<[u32; 2]>,
    scratch: Vec<Image>,
    scratch_size: [u32; 2],
    size: [u32; 2],
    rows: HashMap<Arc<str>, Vec<u8>>,
    request: Option<FilterPreviewRequest>,
    rendering: Vec<Arc<str>>,
    analyses: Vec<Arc<crate::effect_analysis::Prepared>>,
    analysis: Option<crate::effect_analysis::Job>,
    analysis_queries: Vec<layer_core::ArtworkQuery>,
    preparing: bool,
    tx: mpsc::Sender<Ready>,
    rx: mpsc::Receiver<Ready>,
}
impl FilterPreviews {
    pub(crate) fn rendition_changed(&mut self) {
        self.rows.clear();
        self.cancelled |= self.request.is_some();
    }
    fn new(r: &mut WgpuRasterizer) -> Result<Self, GpuRasterError> {
        let scene = Scene::new(r);
        let shader = r.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("filter preview content probe"),
            source: wgpu::ShaderSource::Wgsl(include_str!("filter_probe.wgsl").into()),
        });
        let device = r.device.clone();
        let probe = Deferred::pipeline(move |mode| mode.compute(&device, &wgpu::ComputePipelineDescriptor {
                label: Some("filter preview content probe"),
                layout: None,
                module: &shader,
                entry_point: Some("measure"),
                compilation_options: Default::default(),
                cache: None,
            }));
        // Reuse the original GPU-rendered G-Pen preview's alpha, not a second
        // approximation of its silhouette. Decode/upload only once.
        let png = png::Decoder::new(std::io::Cursor::new(include_bytes!(
            "../../../apps/layer-web/brush-previews/1-light.png"
        )));
        let mut reader = png
            .read_info()
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut pixels)
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        let mask = create_target(
            &r.device,
            [info.width, info.height],
            wgpu::TextureFormat::Rgba8UnormSrgb,
            "G-Pen preview silhouette",
        );
        r.queue.write_texture(
            mask.0.as_image_copy(),
            &pixels[..info.buffer_size()],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(info.width * 4),
                rows_per_image: Some(info.height),
            },
            mask.0.size(),
        );
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            scene,
            source_scene: Scene::new(r),
            probe_next: 0,
            probe_tiles: Vec::new(),
            probe_batch_size: 1,
            probe_timing: r.device.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS).then(|| (
                r.device.create_query_set(&wgpu::QuerySetDescriptor {label: Some("filter probe timing"), ty: wgpu::QueryType::Timestamp, count: 2}),
                r.device.create_buffer(&wgpu::BufferDescriptor {label: Some("filter probe timing resolve"), size: 256,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false}),
            )),
            probe_winner: None,
            cancelled: false,
            programs: HashMap::new(),
            program_snapshot:None,
            probe,
            mask,
            source: None,
            key: None,
            source_layers: Vec::new(),
            point: None,
            scratch: Vec::new(),
            scratch_size: [0, 0],
            size: [0, 0],
            rows: HashMap::new(),
            request: None,
            rendering: Vec::new(),
            analyses: Vec::new(), analysis: None, analysis_queries: Vec::new(), preparing: false,
            tx,
            rx,
        })
    }
    pub(crate) fn note_frame(&mut self, packet: FramePacket<'_>, epoch: u64) {
        if let Some(request) = &self.request {
            self.cancelled |= self
                .key
                .is_none_or(|key| key.0 != epoch || key.1.0 != packet.scene.owner() || key.2 != packet.document_extent || key.3 != packet.blend_space)
                || source_scope(packet.scene.with_scope(&request.scope),request.source).is_none_or(|(_,scope)|{
                    let current:Vec<_>=scope.into_iter().map(|h|PreviewMetadata::new(packet.scene,h)).collect();current!=self.source_layers
                });
        }
    }
    fn start(
        &mut self,
        r: &mut WgpuRasterizer,
        request: FilterPreviewRequest,
    ) -> Result<bool, GpuRasterError> {
        if self.request.is_some() {
            return Ok(false);
        }
        if request.filters.is_empty()
            || request.filters.len() > 8
            || request.size.contains(&0)
            || request.size[0] > 512
            || request.size[1] > 128
            || request.snapshot.view().composition().size != r.document_extent
        {
            return Ok(false);
        }
        let mut artwork=request.snapshot.artwork.clone();
        let mut programs=HashMap::new();
        let mut analyses_changed=false;
        for effect in &request.filters {
            effect.validate().map_err(|e|GpuRasterError::Effect(e.into()))?;
            let id=effect.program.id.clone();
            let previous=self.program_snapshot.as_ref().and_then(|snapshot|self.programs.get(&id).and_then(|h|snapshot.view().effect(*h)));
            analyses_changed|=previous.map(|e|e.program.analysis())!=Some(effect.program.analysis());
            if previous.is_none_or(|old|old.program!=effect.program.as_ref()||old.values!=effect.values){self.rows.remove(&id);}
            let size=artwork.compositions.get(artwork.root).unwrap().size;
            let effect=EffectApplication::new(effect.program.clone(),effect.values.clone(),size);
            let application=artwork.effects.insert(PortableId::random(),effect).map_err(|e|GpuRasterError::Effect(e.into()))?;
            let handle=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(application),"")).map_err(|e|GpuRasterError::Effect(e.into()))?;
            programs.insert(id,handle);
        }
        if analyses_changed||programs!=self.programs{self.clear_analysis();}
        let index=Arc::new(SceneIndex::build(&artwork).map_err(GpuRasterError::Effect)?);
        self.program_snapshot=Some(Arc::new(SceneSnapshot::new(artwork,index,request.snapshot.owner,request.snapshot.revision,request.snapshot.context.clone())));
        self.programs=programs;
        self.cancelled = false;
        let key = (
            r.filter_source_epoch,
            (request.snapshot.owner,request.source),
            request.snapshot.view().composition().size,
            request.snapshot.view().composition().blend,
        );
        let resized = self.size != request.size;
        if resized {
            self.rows.clear();
            self.size = request.size;
        }
        let source_layers=source_scope(request.snapshot.view().with_scope(&request.scope),request.source).map_or_else(Vec::new,|(_,scope)|scope.into_iter().map(|h|PreviewMetadata::new(request.snapshot.view(),h)).collect());
        let changed = self.key != Some(key) || self.source_layers != source_layers || resized;
        // The common UI driver retains delivered rows. Keep only the current
        // bounded request here, rather than a second catalog-sized pixel cache.
        self.rows.retain(|id, _| request.filters.iter().any(|f| f.program.id == *id));
        self.request = Some(request);
        if changed {
            self.clear_analysis();
            self.source_layers = source_layers;
            self.rows.clear();
            self.key = Some(key);
            self.point = None;
            self.probe_next = 0;
            self.probe_tiles = probe_tiles(self.key.unwrap().2);
            self.probe_batch_size = 1;
            self.probe_winner = Some(r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("preview content coordinate"),
                size: 8,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        self.queue_analyses(r)?;
        self.preparing = true;
        self.prepare_source(r)?;
        Ok(true)
    }
    fn with_analyses<T>(&mut self, r: &mut WgpuRasterizer, operation: impl FnOnce(&mut Self, &mut WgpuRasterizer) -> T) -> T {
        let live = std::mem::replace(&mut r.effect_analyses, self.analyses.clone());
        let result = operation(self, r);
        r.effect_analyses = live;
        result
    }
    fn queue_analyses(&mut self,_r:&WgpuRasterizer)->Result<(),GpuRasterError>{
        let request=self.request.as_ref().unwrap();
        let source=source_snapshot(request)?;
        for &h in source.view().order(){
            if source.view().visible(h)&&source.view().effect(h).is_some_and(|e|e.program.analysis().is_some())&&!self.analyses.iter().any(|entry|entry.layer()==h){
                self.analysis_queries.push(layer_core::ArtworkQuery::from_snapshot(source.clone(),layer_core::ArtworkSource::EffectInput(h),None));
            }
        }
        for effect in &request.filters {
            if effect.program.analysis().is_none(){continue;}
            let handle=self.programs[&effect.program.id];
            if self.analyses.iter().any(|entry|entry.layer()==handle){continue;}
            let mut snapshot=self.program_snapshot.as_ref().unwrap().as_ref().clone();
            let target=source_target(request.source);let source_scene=source.view();let stack=source_scene.stack(target).ok_or(GpuRasterError::InvalidExtent)?;
            if matches!(request.source,FilterPreviewSource::OwnerContent(_))
                ||matches!(request.source,FilterPreviewSource::EffectInput(_))&&source_scene.effect_owner(target).is_some(){
                snapshot.artwork.occurrences.get_mut(handle).unwrap().attachment=layer_core::Attachment::Effect;
            }
            let entries=&mut snapshot.artwork.stacks.get_mut(stack).unwrap().entries;
            let position=entries.iter().position(|h|*h==target).ok_or(GpuRasterError::InvalidExtent)?;
            entries.insert(position,handle);
            snapshot.index=Arc::new(SceneIndex::build(&snapshot.artwork).map_err(GpuRasterError::Effect)?);
            let (_,mut members)=source_scope(source_scene,request.source).ok_or(GpuRasterError::InvalidExtent)?;members.push(handle);
            snapshot.scope=SceneScope::Members(members.into());
            self.analysis_queries.push(layer_core::ArtworkQuery::from_snapshot(Arc::new(snapshot),layer_core::ArtworkSource::EffectInput(handle),None));
        }
        Ok(())
    }
    fn prepare_source(&mut self, r: &mut WgpuRasterizer) -> Result<(), GpuRasterError> {
        if r.startup.is_some() && (!r.poll_startup()?.brush_ready || r.effect_validation.is_some()) { return Ok(()); }
        let request = self.request.as_ref().unwrap();
        if let Some(startup) = &r.startup {
            // Visible rows prepare just their own preview variants. No draw or
            // readback is admitted until their asynchronous pipelines are ready.
            for effect in &request.filters {
                self.scene.effects.prepare(r, self.program_snapshot.as_ref().unwrap().view(), &[self.programs[&effect.program.id]], effects::Execution::Preview, 0., 0, preview_space(effect.view(), request.snapshot.view().composition().blend))?;
            }
            let source=source_snapshot(request)?;
            for (layers,execution) in scene::startup_effect_chains(source.view()){
                self.source_scene.effects.prepare(r,source.view(),&layers,execution,source.context.elapsed,0,request.snapshot.view().composition().blend)?;
            }
            let mut ready = self.scene.effects.enqueue_active(&startup.compiler, startup::OTHER);
            ready &= self.source_scene.effects.enqueue_active(&startup.compiler, startup::OTHER);
            startup.compiler.pipeline(&self.probe, startup::OTHER);
            // Preview readback uses the same conversion as document export.
            ready &= r.ui_readback_ready() && self.probe.ready();
            startup.compiler.start();
            if !ready { return Ok(()); }
        }
        if let Some(job) = &mut self.analysis {
            let Some(result) = job.take() else {return Ok(());};
            self.analysis = None;
            for entry in result.map_err(GpuRasterError::Effect)?.entries {
                self.analyses.retain(|old| old.layer() != entry.layer());self.analyses.push(entry);
            }
            self.source_scene.analysis_changed();
        }
        while let Some(query) = self.analysis_queries.pop() {
            let layer = match query.source {layer_core::ArtworkSource::EffectInput(id)=>id,_=>unreachable!()};
            if self.analyses.iter().any(|entry| entry.layer() == layer) {continue;}
            let kind=query.snapshot.view().effect(layer).and_then(|e|e.program.analysis()).unwrap();
            if self.programs.values().any(|program| *program == layer)
                && let Some(resource) = self.analyses.iter().find(|entry| entry.kind == kind
                    && self.programs.values().any(|program| *program == entry.layer())).map(|entry| entry.resource.clone()) {
                self.analyses.push(Arc::new(crate::effect_analysis::Prepared {query,kind,resource}));continue;
            }
            let snapshot=query.snapshot.clone();let scene=snapshot.view();
            let input=crate::effect_analysis::BakeInput{scene:snapshot.clone(),scope:snapshot.scope.clone(),offset:Default::default(),extent:scene.composition().size,color:scene.composition().color,blend:scene.composition().blend,time:snapshot.context.elapsed};
            self.analysis = Some(self.with_analyses(r, |_,r| crate::effect_analysis::Job::frame(r.snapshot_gpu(), input))
                .map_err(GpuRasterError::Effect)?);
            return Ok(());
        }
        self.preparing = false;
        if self.probe_winner.is_some() {self.with_analyses(r,Self::probe_batch)}
        else if self.missing_rows() {self.with_analyses(r,Self::render)} else {Ok(())}
    }
    fn probe_batch(&mut self, r: &mut WgpuRasterizer) -> Result<(), GpuRasterError> {
        let request = self.request.as_ref().unwrap();
        let extent = request.snapshot.view().composition().size;
        let started = web_time::Instant::now();
        let first = self.probe_next;
        // Every tile uses the same queue-ordered window. Allocating a texture
        // per tile leaves gigabytes awaiting browser GC on large documents,
        // even though Rust retains only the most recent handle. Edge windows
        // occupy the top-left prefix; probe samples stay inside the captured
        // region (or outside the document, where the shader rejects them).
        let size = std::array::from_fn(|i| extent[i].min(PAGE_SIZE + request.size[i]));
        if self.source.as_ref().is_none_or(|(texture, _)| {
            [texture.width(), texture.height()] != size
        }) {
            self.source = Some(create_color_target(&r.device, size, "filter probe window"));
        }
        let winner = self.probe_winner.as_ref().unwrap();
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("filter content probe chunk"),
            },
        );
        if let Some((query, _)) = &self.probe_timing { encoder.write_timestamp(query, 0); }
        if self.probe_next == 0 {
            encoder.clear_buffer(winner, 0, None);
        }
        let mut backdrop = [0.; 4];
        let source = source_snapshot(request)?;
        let source_view = source.view();
        let (parent, _) = source_scope(request.snapshot.view().with_scope(&request.scope), request.source)
            .ok_or_else(|| GpuRasterError::Effect("Missing filter insertion layer".into()))?;
        if parent.is_none() {
            for &handle in source_view.constant_backdrop().iter().rev().filter(|&&h| source_view.visible(h)) {
                let [red, green, blue, alpha] = source_view.effect(handle).unwrap().constant_color().unwrap()
                    .linear_in(r.device.working_space()).map_err(GpuRasterError::Color)?;
                let alpha = alpha * source_view.occurrence(handle).unwrap().opacity;
                let color = source_view.composition().blend.composite(r.device.working_space(), [red * alpha, green * alpha, blue * alpha, alpha]);
                backdrop = std::array::from_fn(|c| color[c] + backdrop[c] * (1. - alpha));
            }
        }
        for _ in 0..self.probe_batch_size {
            if self.probe_next == self.probe_tiles.len() || (self.probe_next > first && started.elapsed() >= PROBE_BUDGET) {
                break;
            }
            let tile = self.probe_tiles[self.probe_next].0;
            let core = page_rect(tile).intersect(PixelRect::full(extent));
            let region = PixelRect::new(
                core.min_x().saturating_sub(request.size[0] / 2),
                core.min_y().saturating_sub(request.size[1] / 2),
                core.max_x()
                    .saturating_add(request.size[0] / 2)
                    .min(extent[0]),
                core.max_y()
                    .saturating_add(request.size[1] / 2)
                    .min(extent[1]),
            );
            let (texture, view) = self.source.as_ref().unwrap();
            self.source_scene
                .capture_filter_source(r, request, texture, region, &mut encoder)?;
            let mut data = Vec::with_capacity(64);
            for v in backdrop { data.extend(v.to_le_bytes()); }
            for v in [
                request.size[0],
                request.size[1],
                extent[0],
                extent[1],
                region.min_x(),
                region.min_y(),
                core.min_x(),
                core.min_y(),
                core.width(),
                core.height(),
                0,
                0,
            ] {
                data.extend(v.to_le_bytes());
            }
            let uniform = r
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("preview probe window"),
                    contents: &data,
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let binding = crate::bindings::group(&r.device, "preview probe", &self.probe.get_bind_group_layout(0), [
                wgpu::BindingResource::TextureView(view),
                uniform.as_entire_binding(),
                winner.as_entire_binding(),
            ]);
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("find preview content"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.probe);
                pass.set_bind_group(0, &binding, &[]);
                pass.dispatch_workgroups(core.width().div_ceil(16), core.height().div_ceil(16), 1);
            }
            let next = crate::submission::CommandEncoder::new(
                &r.device,
                &wgpu::CommandEncoderDescriptor {
                    label: Some("next filter probe window"),
                },
            );
            let encoded = std::mem::replace(&mut encoder, next);
            r.uploads.finish(&encoded);
            encoded.submit(&r.queue);
            self.probe_next += 1;
        }
        let tiles = self.probe_next - first;
        let encoded = started.elapsed();
        let timed = self.probe_timing.is_some();
        let period = f64::from(r.queue.get_timestamp_period());
        let read_size = if timed { 24 } else { 8 };
        let read = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("preview coordinate pair"),
            size: read_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(winner, 0, &read, 0, 8);
        if let Some((query, resolve)) = &self.probe_timing {
            encoder.write_timestamp(query, 1);
            encoder.resolve_query_set(query, 0..2, resolve, 0);
            encoder.copy_buffer_to_buffer(resolve, 0, &read, 8, 16);
        }
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        let tx = self.tx.clone();
        crate::raster::map_then(
            &read,
            read_size,
            move |bytes| {
                let scores = std::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
                let gpu = timed.then(|| {
                    let start = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
                    let end = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
                    Duration::from_nanos((end.saturating_sub(start) as f64 * period) as u64)
                }).filter(|duration| !duration.is_zero());
                let batch_size = probe_batch_size(tiles, encoded, gpu.unwrap_or_else(|| started.elapsed()), gpu.is_some());
                Ok((scores, batch_size))
            },
            move |result| {
                let (scores, batch_size) = match result {
                    Ok((scores, batch_size)) => (Ok(scores), batch_size),
                    Err(error) => (Err(error), 1),
                };
                let _ = tx.send(Ready::Probe { scores, batch_size });
            },
        );
        Ok(())
    }
    fn render(&mut self, r: &mut WgpuRasterizer) -> Result<(), GpuRasterError> {
        let request = self.request.as_ref().unwrap();
        self.rendering = request
            .filters
            .iter()
            .map(|f| f.program.id.clone())
            .filter(|f| !self.rows.contains_key(f))
            .collect();
        if self.rendering.is_empty() {
            return Ok(());
        }
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("filter picker previews"),
            },
        );
        self.scene.begin_frame();
        self.scene.jobs.clear();
        let [width, height] = request.size;
        let extent = if self.point.is_some() {
            request.snapshot.view().composition().size
        } else {
            [width, height]
        };
        let center = self.point.unwrap_or([width / 2, height / 2]);
        let origin = std::array::from_fn::<_, 2, _>(|i| {
            if request.size[i]>extent[i] { -(i64::from(request.size[i])-i64::from(extent[i]))/2 }
            else { i64::from(center[i].saturating_sub(request.size[i]/2).min(extent[i]-request.size[i])) }
        });
        let fallback;
        let fallback_source = if self.point.is_some() {
            r.empty_view.clone()
        } else {
            fallback = create_color_target(&r.device, extent, "empty document filter sample");
            let mut data = [0.; 32];
            data[..6].copy_from_slice(&[
                0.,
                0.,
                width as f32,
                height as f32,
                width as f32,
                height as f32,
            ]);
            data[8] = 9.;
            self.scene.jobs.push(Job::Draw {
                target: fallback.1.clone(),
                sources: [r.empty_view.clone(), r.empty_view.clone(), r.empty_view.clone()],
                data,
                over: false,
                clip: None,source_target:None,
            });
            fallback.1.clone()
        };
        let pad = self.rendering.iter().try_fold(0u32, |pad, id| {
            Some(pad.max(self.program_snapshot.as_ref()?.view().effect(self.programs[id])?.damage_radius()?))
        });
        let source_bounds = pad.map_or(PixelRect::full(extent), |pad| {
            DocRect {min:origin,max:std::array::from_fn(|i|origin[i]+i64::from(request.size[i]))}.in_frame(extent)
            .expand(pad, extent)
        });
        let source_grid = display_mips::Plan::window(extent, 0,
            if self.point.is_some() { source_bounds } else { PixelRect::full(extent) });
        let source = if self.point.is_some() {
            self.source = Some(create_color_target(
                &r.device,
                [source_bounds.width(), source_bounds.height()],
                "filter preview source crop",
            ));
            let (texture, view) = self.source.as_ref().unwrap();
            self.source_scene.capture_filter_source(
                r,
                request,
                texture,
                source_bounds,
                &mut encoder,
            )?;
            view.clone()
        } else {
            fallback_source
        };
        let spaces: Vec<_> = self.rendering.iter().map(|id| preview_space(self.program_snapshot.as_ref().unwrap().view().effect(self.programs[id]).unwrap(), request.snapshot.view().composition().blend)).collect();
        let encoded;
        let encoded_source = if spaces.contains(&layer_core::BlendSpace::Perceptual) {
            let size = if self.point.is_some() { [source_bounds.width(), source_bounds.height()] } else { extent };
            encoded = create_color_target(&r.device, size, "encoded filter preview source");
            let [width, height] = size.map(|v| v as f32);
            let mut data = [0.; 32];
            data[..6].copy_from_slice(&[0., 0., width, height, width, height]);
            data[8..10].copy_from_slice(&[1., 1.]);
            data[31] = Convert::Encode.code();
            self.scene.jobs.push(Job::Draw {
                target: encoded.1.clone(),
                sources: [source.clone(), r.empty_view.clone(), r.empty_view.clone()],
                data,
                over: false,
                clip: None,source_target:None,
            });
            encoded.1.clone()
        } else {
            source.clone()
        };
        let atlas = create_color_target(
            &r.device,
            [width, height * self.rendering.len() as u32],
            "filter preview atlas",
        );
        for (row, id) in self.rendering.iter().enumerate() {
            let space = spaces[row];
            let source = if space == layer_core::BlendSpace::Perceptual { encoded_source.clone() } else { source.clone() };
            // Compile only requested rows. A catalog-wide dynamic switch would
            // compile every expensive kernel before showing even the first row.
            let prepared = self.scene.effects.prepare(
                r,
                self.program_snapshot.as_ref().unwrap().view(),
                &[self.programs[id]],
                effects::Execution::Preview,
                0.,
                0,
                space,
            )?;
            let effect=self.program_snapshot.as_ref().unwrap().view().effect(self.programs[id]).unwrap();
            let program=effect.program;
            // A document-remapping pass after another pass genuinely needs its
            // complete input. Bounded programs otherwise render only crop+halo.
            let whole = program
                .passes
                .iter()
                .skip(1)
                .any(|p| p.sampling == layer_core::EffectSampling::Document);
            let pad=effect.damage_radius().unwrap_or(0);
            let crop = if whole {
                [0, 0]
            } else {
                origin.map(|v| if v<0 {v} else {(v-i64::from(pad)).max(0)})
            };
            let size = if whole {
                extent
            } else {
                [
                    width
                        .saturating_add(pad.saturating_mul(2))
                        .min(extent[0].max(width)),
                    height
                        .saturating_add(pad.saturating_mul(2))
                        .min(extent[1].max(height)),
                ]
            };
            let count = program.passes.len().max(1);
            if size[0] > self.scratch_size[0] || size[1] > self.scratch_size[1] {
                self.scratch_size = [
                    size[0].max(self.scratch_size[0]),
                    size[1].max(self.scratch_size[1]),
                ];
                self.scratch.clear();
            }
            while self.scratch.len() < count.min(2) {
                self.scratch.push(create_color_target(
                    &r.device,
                    self.scratch_size,
                    "reused preview crop",
                ));
            }
            let mut grid = display_mips::Plan::window(extent, 0,PixelRect::full(size));
            grid.doc_bounds=DocRect {min:crop,max:std::array::from_fn(|i|crop[i]+i64::from(size[i]))};
            let previous = self.scene.preview_passes(r, prepared, grid, (source, source_grid), &self.scratch, count);
            let mut data = [0.; 32];
            data[..6].copy_from_slice(&[
                0.,
                (row as u32 * height) as f32,
                width as f32,
                height as f32,
                width as f32,
                (height * self.rendering.len() as u32) as f32,
            ]);
            data[8] = 10.;
            data[24..32].copy_from_slice(&r.ui_rendition_parameters());
            data[12..16].copy_from_slice(&[
                (origin[0] - crop[0]) as f32,
                (origin[1] - crop[1]) as f32,
                width as f32,
                height as f32,
            ]);
            let visible=DocRect {min:origin,max:std::array::from_fn(|i|origin[i]+i64::from(request.size[i]))}.in_frame(extent);
            self.scene.jobs.push(Job::Draw {
                target: atlas.1.clone(),
                sources: [previous, self.mask.1.clone(), r.empty_view.clone()],
                data,
                over: false,
                clip: Some(PixelRect::new(
                    (i64::from(visible.min_x())-origin[0]) as u32,
                    row as u32*height+(i64::from(visible.min_y())-origin[1]) as u32,
                    (i64::from(visible.max_x())-origin[0]) as u32,
                    row as u32*height+(i64::from(visible.max_y())-origin[1]) as u32)),
                source_target:None,
            });
        }
        self.scene.encode_jobs(r, &mut encoder)?;
        let image_height = height * self.rendering.len() as u32;
        let binding = create_texture_bind_group(
            &r.device,
            &r.texture_layout,
            &atlas.1,
            &r.sampler,
            "filter preview export",
        );
        let tx = self.tx.clone();
        r.submit_ui_readback(
            encoder,
            &binding,
            [width, image_height],
            request.request_id,
            move |image| {
                let _ = tx.send(Ready::Pixels(image));
            },
        );
        Ok(())
    }
    fn take(
        &mut self,
        r: &mut WgpuRasterizer,
    ) -> Option<Result<FilterPreviewImage, GpuRasterError>> {
        self.request.as_ref()?;
        if self.cancelled {self.clear_analysis();self.request=None;self.key=None;return Some(Err(GpuRasterError::FilterPreviewCancelled));}
        if self.preparing {
            if let Err(error) = self.prepare_source(r) {self.clear_analysis();self.request = None;self.key = None;return Some(Err(error));}
            return None;
        }
        // Even if the next completion arrives during encoding, yield to the
        // host after one chunk. A fast device must not turn polling into an
        // unbounded scan on the input/render owner.
        if let Ok(ready) = self.rx.try_recv() {
            let result = if self.cancelled {
                Err(GpuRasterError::FilterPreviewCancelled)
            } else {
                match ready {
                    Ready::Probe { scores, batch_size } => scores.and_then(|scores| {
                        self.probe_batch_size = batch_size;
                        if self.probe_tiles.get(self.probe_next).is_some_and(|(_, maximum)| scores[0] < *maximum) {
                            return self.with_analyses(r, Self::probe_batch);
                        }
                        let score = if scores[0] > 0 { scores[0] } else { scores[1] };
                        self.probe_winner = None;
                        if score != 0 {
                            let rank = u32::MAX - score;
                            let extent = self.request.as_ref().unwrap().snapshot.view().composition().size;
                            let decode = |v: u32| {
                                if v & 1 == 0 {
                                    (v / 2) as i32
                                } else {
                                    -((v / 2) as i32)
                                }
                            };
                            self.point = Some(
                                [
                                    (extent[0] / 2) as i32 + decode(rank & 65535),
                                    (extent[1] / 2) as i32 + decode(rank >> 16),
                                ]
                                .map(|v| v as u32),
                            );
                        }
                        self.with_analyses(r,Self::render)
                    }),
                    Ready::Pixels(image) => image.map(|image| {
                        let row_bytes = (self.size[0] * self.size[1] * 4) as usize;
                        for (id, bytes) in
                            self.rendering.drain(..).zip(image.bytes.chunks(row_bytes))
                        {
                            self.rows.insert(id, bytes.to_vec());
                        }
                    }),
                }
            };
            if let Err(error) = result {
                self.clear_analysis();
                self.request = None;
                self.probe_winner = None;
                self.key = None;
                return Some(Err(error));
            }
        }
        if self.request.is_none() || self.missing_rows() {
            return None;
        }
        let request = self.request.take().unwrap();
        let mut bytes = Vec::with_capacity(
            self.size[0] as usize * self.size[1] as usize * 4 * request.filters.len(),
        );
        for effect in &request.filters {
            bytes.extend_from_slice(&self.rows[&effect.program.id]);
        }
        Some(Ok(FilterPreviewImage {
            image: ReadbackImage {
                request_id: request.request_id,
                width: self.size[0],
                height: self.size[1] * request.filters.len() as u32,
                stride: self.size[0] * 4,
                bytes,
            },
            filters: request
                .filters
                .iter()
                .map(|f| f.program.id.clone())
                .collect(),
        }))
    }
}

impl Scene {
    fn preview_passes(&mut self, r: &WgpuRasterizer, prepared: effects::PreparedEffect,
        grid: display_mips::Plan, (source, source_grid): (wgpu::TextureView, display_mips::Plan),
        targets: &[Image], count: usize,
    ) -> wgpu::TextureView {
        let mut previous = source.clone();
        for stage in 0..count {
            let (texture, target) = &targets[stage % targets.len()];
            let mut data = effects::image_grid(grid, if stage == 0 { source_grid } else { grid }, source_grid);
            data[4..8].copy_from_slice(&[texture.width() as f32, texture.height() as f32, 0., stage as f32]);
            self.jobs.push(Job::Effect { target: target.clone(), sources: [previous, source.clone(), r.empty_view.clone()],
                data, prepared: prepared.clone(), masks: Box::new(std::array::from_fn(|_| r.empty_view.clone())),source_target:None,changed_cells:None });
            previous = target.clone();
        }
        previous
    }

    pub(crate) fn generator_preview(&mut self, r: &mut WgpuRasterizer, handle: OccurrenceHandle, grid: display_mips::Plan,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<Vec<Image>>, GpuRasterError> {
        self.begin_frame();
        self.jobs.clear();
        let frame = r.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
        let scene = frame.scene.view();
        let effect = scene.effect(handle).ok_or(GpuRasterError::InvalidExtent)?;
        self.effects.retain(scene);
        let prepared = self.effects.prepare(r, scene, &[handle], effects::Execution::Preview, frame.time, grid.level,
            preview_space(effect, frame.blend_space))?;
        if let Some(startup) = &r.startup {
            let ready = self.effects.enqueue(&startup.compiler, startup::VALIDATION);
            startup.compiler.start();
            if !ready { return Ok(None); }
        }
        let count = effect.program.passes.len().max(1);
        let targets: Vec<_> = (0..count.min(2)).map(|_| create_color_target(&r.device, grid.size, "generator thumbnail")).collect();
        self.preview_passes(r, prepared, grid, (r.empty_view.clone(), grid), &targets, count);
        self.encode_jobs(r, encoder)?;
        Ok(Some(targets))
    }
}
/// The blend space a filter's preview compiles for: the document's when the
/// filter reads the document's encoded values, otherwise Linear.
fn preview_space(effect: layer_core::EffectView<'_>, blend: layer_core::BlendSpace) -> layer_core::BlendSpace {
    if effect.program.space.encoded(blend) { blend } else { layer_core::BlendSpace::Linear }
}
// Keep only the insertion scope and its ancestors: the target, its subtree
// and what composites below it, through Pass Through groups. An excluded
// global effect above the target must not force a full-document dependency.
fn source_target(source: FilterPreviewSource) -> OccurrenceHandle {
    match source { FilterPreviewSource::LayerStack(h) | FilterPreviewSource::EffectInput(h) | FilterPreviewSource::OwnerContent(h) => h }
}
fn source_scope(scene:SceneView<'_>,source:FilterPreviewSource)->Option<(Option<OccurrenceHandle>,Vec<OccurrenceHandle>)>{
    let target=source_target(source);
    scene.occurrence(target)?;
    let mut members=match source {
        FilterPreviewSource::LayerStack(_)=>layer_core::backdrop_layers(scene,target),
        FilterPreviewSource::EffectInput(_)=>layer_core::composite_input_layers(scene,target),
        FilterPreviewSource::OwnerContent(_)=>Vec::new(),
    };
    if matches!(source,FilterPreviewSource::LayerStack(_)|FilterPreviewSource::OwnerContent(_)) {
        members.extend(scene.order().iter().copied().filter(|h|*h==target||layer_core::descends_from(scene,*h,Some(target))));
    }
    let mut parent=scene.parent(target);
    while let Some(h)=parent{if !scene.occurrence(h)?.passes_through(){members.push(h);}parent=scene.parent(h);}
    members.retain(|h|scene.includes(*h));members.sort_by_key(|h|scene.position(*h));members.dedup();
    let parent=match source {
        FilterPreviewSource::LayerStack(_)=>layer_core::isolated_scope(scene,scene.parent(target)),
        FilterPreviewSource::EffectInput(_)=>layer_core::composite_input_scope(scene,target),
        FilterPreviewSource::OwnerContent(_)=>Some(target),
    };
    Some((parent,members))
}
fn source_snapshot(request:&FilterPreviewRequest)->Result<Arc<SceneSnapshot>,GpuRasterError>{
    let (_,members)=source_scope(request.snapshot.view().with_scope(&request.scope),request.source).ok_or_else(||GpuRasterError::Effect("Missing filter insertion layer".into()))?;
    Ok(Arc::new(request.snapshot.as_ref().clone().with_scope(SceneScope::Members(members.into()))))
}
impl Scene {
    fn capture_filter_source(&mut self,r:&mut WgpuRasterizer,request:&FilterPreviewRequest,destination:&wgpu::Texture,region:PixelRect,encoder:&mut crate::submission::CommandEncoder)->Result<(),GpuRasterError>{
        let snapshot=source_snapshot(request)?;let scene=snapshot.view();
        let (parent,_)=source_scope(request.snapshot.view(),request.source).ok_or(GpuRasterError::InvalidExtent)?;
        self.jobs.clear();self.used.fill(false);
        let packet=FramePacket{commit_rasters:true,time_seconds:snapshot.context.elapsed,view:request.view,document_extent:scene.composition().size,scene,selection_overlays:None,inspect_mask:None,dabs:&[],dab_batches:&[],restore_rasters:&[],reset_layers:false,composite_all:false,blend_space:scene.composition().blend};
        let output=match request.source {
            FilterPreviewSource::LayerStack(_)=>scene::Output::Artwork(parent),
            FilterPreviewSource::EffectInput(target)=>scene::Output::EffectInput(target),
            FilterPreviewSource::OwnerContent(target)=>scene::Output::OwnerContent(target),
        };
        self.capture_region(r,packet,destination,region,output,encoder)
    }
}
impl WgpuRasterizer {
    pub(crate) fn cancel_filter_preview_request(&mut self) {
        let Some(previews) = self.filter_previews.as_mut() else { return; };
        previews.request = None;previews.clear_analysis();
        // Old callbacks retain only their sender. Dropping the receiver keeps
        // them from advancing or completing the next request, without a wait.
        (previews.tx, previews.rx) = mpsc::channel();
        previews.probe_winner = None;
        previews.rendering.clear();
        previews.key = None;
        previews.point = None;
        previews.rows.clear();
    }
    pub fn filter_previews_pending(&self) -> bool {
        self.filter_previews
            .as_ref()
            .is_some_and(|p| p.request.is_some())
    }
    pub(crate) fn start_filter_previews(
        &mut self,
        request: FilterPreviewRequest,
    ) -> Result<bool, GpuRasterError> {
        let mut previews = match self.filter_previews.take() {
            Some(p) => p,
            None => FilterPreviews::new(self)?,
        };
        let result = previews.start(self, request);
        if result.is_err() {
            previews.clear_analysis();
            previews.request = None;
            previews.key = None;
        }
        self.filter_previews = Some(previews);
        result
    }
    pub(crate) fn poll_filter_previews(
        &mut self,
    ) -> Option<Result<FilterPreviewImage, GpuRasterError>> {
        let mut previews = self.filter_previews.take()?;
        if previews.missing_rows() {
            let _ = self.device.poll(wgpu::PollType::Poll);
        }
        let result = previews.take(self);
        self.filter_previews = Some(previews);
        result
    }
}

impl FilterPreviews {
    fn clear_analysis(&mut self) {
        self.analysis = None;self.analysis_queries.clear();self.analyses.clear();self.preparing = false;
        self.scene.jobs.clear();self.source_scene.jobs.clear();
        self.scene.effects.discard_instances();self.source_scene.effects.discard_instances();
    }
    fn missing_rows(&self) -> bool {
        self.request.as_ref().is_some_and(|r| r.filters.iter().any(|f| !self.rows.contains_key(&f.program.id)))
    }
    pub(crate) fn storage_bytes(&self) -> u64 {
        let bytes = |(texture, _): &Image| texture_bytes(texture);
        self.source.as_ref().map_or(0, bytes)
            + bytes(&self.mask)
            + self.scratch.iter().map(bytes).sum::<u64>()
            + self.scene.scratch_bytes()
            + self.source_scene.scratch_bytes()
            + self.probe_winner.as_ref().map_or(0, wgpu::Buffer::size)
            + self.probe_timing.as_ref().map_or(0, |(_, buffer)| buffer.size())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixture;

    fn finish(r: &mut WgpuRasterizer) -> FilterPreviewImage {
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            r.device.poll(wgpu::PollType::Wait {submission_index:None,timeout:Some(Duration::from_secs(10))}).unwrap();
            if let Some(result) = r.take_filter_previews() {return result.unwrap();}
            assert!(std::time::Instant::now() < deadline, "preview did not complete");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    use layer_core::{Document, Edit, RecordChange};
    use layer_core::authored::{Artwork,Stack};

    fn paint(artwork: &mut Artwork, original: Option<Arc<layer_core::color::source::SourceImage>>) -> OccurrenceHandle {
        let extent=artwork.compositions.get(artwork.root).unwrap().size;
        let (handle,target)=crate::test_support::add_paint(artwork,"paint",extent);
        let SourceTarget::Paint(source)=target else {unreachable!()};
        artwork.paint.get_mut(source).unwrap().base=original.map(|source| layer_core::authored::PaintBase::new(source.into()));
        handle
    }
    fn effect(artwork: &mut Artwork, effect: layer_core::EffectInstance) -> OccurrenceHandle {
        let size=artwork.compositions.get(artwork.root).unwrap().size;
        let application=artwork.effects.insert(PortableId::random(),EffectApplication::new(effect.program,effect.values,size)).unwrap();
        artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(application), "effect")).unwrap()
    }
    fn generated(artwork: &mut Artwork, code: &str) -> OccurrenceHandle {
        let mut program = (*fixture("exposure").program()).clone();
        program.kind = layer_core::EffectKind::Generator;
        program.entry = "pattern".into();
        program.wgsl = code.into();
        effect(artwork, layer_core::EffectInstance::new(Arc::new(program)))
    }
    fn pattern(artwork: &mut Artwork) -> OccurrenceHandle {
        generated(artwork, "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(p.x/fx_extent().x,p.y/fx_extent().y,.2,1.);}")
    }
    fn document(mut artwork: Artwork, entries: Vec<OccurrenceHandle>) -> Document {
        let stack = artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries = entries;
        Document::from_artwork(artwork).unwrap()
    }
    fn group_document(extent: [u32;2], blend: layer_core::LayerBlend, upper: &str) -> (Document, OccurrenceHandle) {
        let mut artwork = Artwork::new(extent).unwrap();
        let target = paint(&mut artwork, None);
        let above = effect(&mut artwork, fixture(upper).preview().unwrap());
        let stack = artwork.stacks.insert(PortableId::random(), Stack { entries: vec![above,target] }).unwrap();
        let mut group = Occurrence::new(OccurrenceContent::Stack(stack), "group");
        group.blend = blend; group.opacity = 0.5;
        let group = artwork.occurrences.insert(PortableId::random(), group).unwrap();
        let source = pattern(&mut artwork);
        (document(artwork,vec![group,source]),target)
    }
    fn flat_document(extent: [u32;2]) -> (Document, OccurrenceHandle) {
        let mut artwork = Artwork::new(extent).unwrap();
        let target = paint(&mut artwork, None);
        let source = pattern(&mut artwork);
        (document(artwork,vec![target,source]),target)
    }
    fn request(document: &Document, target: OccurrenceHandle, request_id: u64, size: [u32;2], view: layer_render::ViewState, filters: Vec<Arc<layer_core::EffectInstance>>) -> FilterPreviewRequest {
        FilterPreviewRequest {request_id,source:FilterPreviewSource::LayerStack(target),size,view,snapshot:document.snapshot(),scope:SceneScope::All,filters}
    }
    fn occurrence_edit(document: &mut Document, handle: OccurrenceHandle, f: impl FnOnce(&mut Occurrence)) {
        let mut value=document.artwork.occurrences.get(handle).unwrap().clone();f(&mut value);
        document.apply(Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences,handle,Some(value)).unwrap())).unwrap();
    }
    #[test]
    fn idle_filter_preview_requests_refresh_completed_startup_work() {
        let extent=[64;2];let mut artwork=Artwork::new(extent).unwrap();
        let owner=paint(&mut artwork,Some(layer_core::color::source::rgba8_source(extent,|_,_|[230,40,20,128])));
        artwork.occurrences.get_mut(owner).unwrap().attachment=layer_core::Attachment::Clip;
        artwork.occurrences.get_mut(owner).unwrap().opacity=0.35;
        let base=paint(&mut artwork,Some(layer_core::color::source::rgba8_source(extent,|_,_|[20,240,30,255])));
        let target=effect(&mut artwork,fixture("motion_blur").preview().unwrap());
        artwork.occurrences.get_mut(target).unwrap().attachment=layer_core::Attachment::Effect;
        let doc=document(artwork,vec![target,owner,base]);
        let reference=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut r=crate::test_support::staged_renderer(&reference,doc.composition().color);
        r.prepare_startup(&doc,&layer_core::default_brush(layer_core::DefaultBrushPreset::GPen),false).unwrap();
        crate::test_support::wait_startup(&mut r,std::time::Instant::now()+Duration::from_secs(30),|p|p.complete,format_args!("Preview startup"));
        r.submit(crate::test_support::packet(doc.scene(),extent)).unwrap();r.shader_idle(false);
        for (id,source) in [(1,FilterPreviewSource::OwnerContent(owner)),(2,FilterPreviewSource::EffectInput(target))] {
            let mut candidate=request(&doc,owner,id,[414,68],crate::test_support::view(extent),vec![Arc::new(fixture("curves").preview().unwrap())]);candidate.source=source;
            assert!(r.request_filter_previews(candidate).unwrap());
            if id==1 {
                assert!(r.filter_previews_pending());
                assert!(r.take_filter_previews().is_none());
                assert!(r.startup.as_ref().unwrap().compiler.pending()>0);
                assert!(!r.poll_startup().unwrap().complete);
                r.shader_idle(true);
            }
            let image=finish(&mut r).image;
            assert_eq!(image.request_id,id);
            let alpha=image.bytes[((68/2*414+414/2)*4+3) as usize];
            assert!((f32::from(alpha)/255.-128./255.).abs()<0.02,"{source:?} keeps its owner-local alpha");
            r.cancel_filter_previews();
        }
    }
    #[test]
    fn cancelled_preview_categories_do_not_block_ready_rows() {
        let extent=[64;2];let mut artwork=Artwork::new(extent).unwrap();
        let owner=paint(&mut artwork,Some(layer_core::color::source::rgba8_source(extent,|_,_|[20,40,230,128])));
        let doc=document(artwork,vec![owner]);
        let reference=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut r=crate::test_support::staged_renderer(&reference,doc.composition().color);
        r.prepare_startup(&doc,&layer_core::default_brush(layer_core::DefaultBrushPreset::GPen),false).unwrap();
        crate::test_support::wait_startup(&mut r,std::time::Instant::now()+Duration::from_secs(30),|p|p.complete,format_args!("Preview startup"));
        r.submit(crate::test_support::packet(doc.scene(),extent)).unwrap();r.shader_idle(true);
        let curves=request(&doc,owner,1,[414,68],crate::test_support::view(extent),vec![Arc::new(fixture("curves").preview().unwrap())]);
        assert!(r.request_filter_previews(curves.clone()).unwrap());
        let original=finish(&mut r).image;
        let preparations=r.filter_previews.as_ref().unwrap().scene.effects.preparation_count();
        r.cancel_filter_previews();r.shader_idle(false);
        let other=request(&doc,owner,2,[414,68],crate::test_support::view(extent),vec![Arc::new(fixture("gaussian_blur").preview().unwrap())]);
        assert!(r.request_filter_previews(other).unwrap());
        assert!(r.filter_previews_pending());
        assert!(r.take_filter_previews().is_none());
        assert!(r.startup.as_ref().unwrap().compiler.pending()>0);
        let progress=r.poll_startup().unwrap();
        assert!(progress.brush_ready);assert!(!progress.complete);
        r.cancel_filter_previews();
        let mut reopened=curves;reopened.request_id=3;
        assert!(!r.filter_previews_pending());
        assert!(r.request_filter_previews(reopened).unwrap());
        let completed=finish(&mut r).image;
        assert_eq!(completed.request_id,3);
        assert_eq!(completed.bytes,original.bytes);
        assert!(r.startup.as_ref().unwrap().compiler.pending()>0,"Unrelated optional shaders remain unready");
        assert_eq!(r.filter_previews.as_ref().unwrap().scene.effects.preparation_count(),preparations,"Cancelled lookup dispatches never run");
        r.shader_idle(true);
    }
    #[test]
    fn attached_curves_preview_retains_owner_alpha_when_the_row_exceeds_the_document() {
        let extent=[64;2];
        let mut artwork=Artwork::new(extent).unwrap();
        let owner=paint(&mut artwork,Some(layer_core::color::source::rgba8_source(extent,|x,_|
            if (30..34).contains(&x) {[20,40,230,128]} else {[230,40,20,128]})));
        let occurrence=artwork.occurrences.get_mut(owner).unwrap();
        occurrence.attachment=layer_core::Attachment::Clip;occurrence.opacity=0.35;
        let base=paint(&mut artwork,Some(layer_core::color::source::rgba8_source(extent,|_,_|[20,240,30,255])));
        let target=effect(&mut artwork,fixture("motion_blur").preview().unwrap());
        artwork.occurrences.get_mut(target).unwrap().attachment=layer_core::Attachment::Effect;
        let doc=document(artwork,vec![target,owner,base]);
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.submit(crate::test_support::packet(doc.scene(),extent)).unwrap();
        let curves=Arc::new(fixture("curves").preview().unwrap());
        let mut whole=(*fixture("exposure").program()).clone();
        whole.id="document_preview_probe".into();whole.entry="preview_copy".into();
        whole.wgsl="fn preview_copy(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return c;}".into();
        whole.passes=vec![layer_core::EffectPass {entry:"preview_copy".into(),sampling:layer_core::EffectSampling::Neighborhood {radius:0}},
            layer_core::EffectPass {entry:"preview_copy".into(),sampling:layer_core::EffectSampling::Document}].into();
        let whole=Arc::new(layer_core::EffectInstance::new(Arc::new(whole)));
        let mut request_id=0;
        for filter in [curves,whole] {
        for size in [[64,40],[206,46],[384,80]] {
            for source in [FilterPreviewSource::OwnerContent(owner),FilterPreviewSource::EffectInput(target)] {
                request_id+=1;
                let mut request=request(&doc,owner,request_id,size,crate::test_support::view(extent),vec![filter.clone()]);
                request.source=source;
                r.request_filter_previews(request).unwrap();
                let image=finish(&mut r);
                let index=((size[1]/2*size[0]+size[0]/2)*4) as usize;
                let pixel=&image.image.bytes[index..index+4];
                assert!((f32::from(pixel[3])/255.-128./255.).abs()<0.02,"{source:?} {size:?} center={pixel:?}");
                assert!(pixel[2]>pixel[0].max(pixel[1]),"The unblurred blue stripe remains centered without the clipping base");
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        let position=[x,y];
                        let outside=(0..2).any(|axis|size[axis]>extent[axis]&&{
                            let start=(size[axis]-extent[axis])/2;
                            position[axis]<start||position[axis]>=start+extent[axis]
                        });
                        if outside {assert_eq!(image.image.bytes[((y*size[0]+x)*4+3) as usize],0,"{} {size:?}: finite owner padding stays transparent at {position:?}",filter.program.id);}
                    }
                }
            }
        }
        }
    }
    #[test]
    fn attached_motion_blur_preview_reads_its_clipped_owner_before_outer_composition() {
        let extent=[32;2];
        let mut artwork=Artwork::new(extent).unwrap();
        let owner=paint(&mut artwork,Some(layer_core::color::source::rgba8_source(extent,|_,_|[230,40,20,128])));
        let occurrence=artwork.occurrences.get_mut(owner).unwrap();
        occurrence.attachment=layer_core::Attachment::Clip;occurrence.opacity=0.35;
        let base=paint(&mut artwork,Some(layer_core::color::source::rgba8_source(extent,|_,_|[20,240,30,255])));
        let paper=effect(&mut artwork,fixture("solid_color").preview().unwrap());
        let target=effect(&mut artwork,fixture("motion_blur").preview().unwrap());
        artwork.occurrences.get_mut(target).unwrap().attachment=layer_core::Attachment::Effect;
        let mut doc=document(artwork,vec![target,owner,base,paper]);
        let owner_source=doc.scene().source_target(owner).unwrap();
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.submit(crate::test_support::packet(doc.scene(),extent)).unwrap();
        let capture=|r:&mut WgpuRasterizer,doc:&Document,source:Option<FilterPreviewSource>,output:scene::Output|{
            let (texture,_)=create_color_target(&r.device,extent,"preview input regression");
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            let mut scene=Scene::new(r);
            if let Some(source)=source {
                let mut request=request(doc,owner,1,extent,crate::test_support::view(extent),vec![Arc::new(fixture("exposure").preview().unwrap())]);
                request.source=source;
                scene.capture_filter_source(r,&request,&texture,PixelRect::full(extent),&mut encoder).unwrap();
            } else {
                scene.capture_region(r,crate::test_support::packet(doc.scene(),extent),&texture,PixelRect::full(extent),output,&mut encoder).unwrap();
            }
            r.uploads.finish(&encoder);encoder.submit(&r.queue);
            crate::test_support::float_pixels(r,&texture)
        };
        let raw=capture(&mut r,&doc,None,scene::Output::Source(owner_source));
        let preview=capture(&mut r,&doc,Some(FilterPreviewSource::EffectInput(target)),scene::Output::Artwork(None));
        assert!(crate::test_support::max_error(&preview,&raw)<1e-6,"attached input preserves owner pixels and alpha");
        assert!((preview[0][3]-128./255.).abs()<1e-6);
        let analysis=Arc::new(fixture("shadows_highlights").preview().unwrap());
        let mut artwork=doc.artwork.clone();let candidate=effect(&mut artwork,analysis.as_ref().clone());
        let index=Arc::new(SceneIndex::build(&artwork).unwrap());
        let mut previews=FilterPreviews::new(&mut r).unwrap();
        previews.program_snapshot=Some(Arc::new(SceneSnapshot::new(artwork,index,doc.scene().owner(),doc.scene().revision(),Default::default())));
        previews.programs.insert(analysis.program.id.clone(),candidate);
        let mut analysis_request=request(&doc,owner,2,extent,crate::test_support::view(extent),vec![analysis]);
        analysis_request.source=FilterPreviewSource::EffectInput(target);previews.request=Some(analysis_request);
        previews.queue_analyses(&r).unwrap();
        let query=previews.analysis_queries.last().unwrap();
        let (texture,_)=create_color_target(&r.device,extent,"preview analysis input regression");
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        Scene::new(&r).capture_region(&mut r,FramePacket{scene:query.snapshot.view(),..crate::test_support::packet(doc.scene(),extent)},
            &texture,PixelRect::full(extent),scene::Output::EffectInput(candidate),&mut encoder).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);
        assert!(crate::test_support::max_error(&crate::test_support::float_pixels(&r,&texture),&raw)<1e-6,
            "replacement analysis reads the same owner input");
        occurrence_edit(&mut doc,target,|o|o.attachment=layer_core::Attachment::None);
        let preview=capture(&mut r,&doc,Some(FilterPreviewSource::EffectInput(target)),scene::Output::Artwork(None));
        occurrence_edit(&mut doc,target,|o|o.visible=false);
        let backdrop=capture(&mut r,&doc,None,scene::Output::Artwork(None));
        assert!(crate::test_support::max_error(&preview,&backdrop)<1e-6,"standalone replacement reads the composed backdrop");
        assert!(crate::test_support::max_error(&raw,&backdrop)>0.1);
        let mut artwork=doc.artwork.clone();
        let mask=artwork.coverage.insert(PortableId::random(),layer_core::authored::CoverageSource {
            domain:extent,raster:Default::default(),initial:None,default_coverage:0.5,operations:Default::default(),
        }).unwrap();
        artwork.occurrences.get_mut(owner).unwrap().mask=Some(layer_core::authored::MaskUse {
            source:mask,enabled:true,linked:true,inverted:false,translation:Default::default(),placement:layer_core::Projective::IDENTITY,
        });
        let existing=effect(&mut artwork,fixture("exposure").preview().unwrap());
        artwork.occurrences.get_mut(existing).unwrap().attachment=layer_core::Attachment::Effect;
        let stack=artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries.insert(1,existing);
        let doc=Document::from_artwork(artwork).unwrap();
        let expected:Vec<_>=raw.iter().map(|pixel|pixel.map(|v|v*0.5)).collect();
        let preview=capture(&mut r,&doc,Some(FilterPreviewSource::OwnerContent(owner)),scene::Output::Artwork(None));
        assert!(crate::test_support::max_error(&preview,&expected)<1e-6,"new attached filter reads masked owner before opacity and existing effects");
        let exposed=capture(&mut r,&doc,None,scene::Output::Artwork(None));
        assert!(crate::test_support::max_error(&preview,&exposed)>0.1);
        let analysis=Arc::new(fixture("shadows_highlights").preview().unwrap());
        let mut artwork=doc.artwork.clone();let candidate=effect(&mut artwork,analysis.as_ref().clone());
        let index=Arc::new(SceneIndex::build(&artwork).unwrap());
        previews.program_snapshot=Some(Arc::new(SceneSnapshot::new(artwork,index,doc.owner,doc.revision,Default::default())));
        previews.programs.insert(analysis.program.id.clone(),candidate);previews.analysis_queries.clear();
        let mut analysis_request=request(&doc,owner,3,extent,crate::test_support::view(extent),vec![analysis]);
        analysis_request.source=FilterPreviewSource::OwnerContent(owner);previews.request=Some(analysis_request);
        previews.queue_analyses(&r).unwrap();
        let query=previews.analysis_queries.last().unwrap();
        let (texture,_)=create_color_target(&r.device,extent,"new attached analysis input regression");
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        Scene::new(&r).capture_region(&mut r,FramePacket{scene:query.snapshot.view(),..crate::test_support::packet(doc.scene(),extent)},
            &texture,PixelRect::full(extent),scene::Output::EffectInput(candidate),&mut encoder).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);
        assert!(crate::test_support::max_error(&crate::test_support::float_pixels(&r,&texture),&expected)<1e-6,
            "new attached analysis bypasses the owner's existing effects");
    }
    #[test]
    fn replacing_the_bottom_fill_captures_transparent_input() {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut artwork=Artwork::new([64;2]).unwrap();
        let mut fill=fixture("solid_color").preview().unwrap();
        fill.set("color",layer_core::EffectValue::Color(layer_core::color::RgbColor::WHITE)).unwrap();
        let fill=effect(&mut artwork,fill);
        let document=document(artwork,vec![fill]);
        r.submit(crate::test_support::packet(document.scene(), [64; 2])).unwrap();
        let source = FilterPreviewSource::EffectInput(fill);
        assert!(source_scope(document.scene(), source).unwrap().1.is_empty());
        r.request_filter_previews(FilterPreviewRequest {
            request_id: 1, source, size: [120, 40], view: crate::test_support::view([64; 2]),
            snapshot:document.snapshot(),scope:SceneScope::All,filters: vec![Arc::new(fixture("exposure").preview().unwrap())],
        }).unwrap();
        let atlas = finish(&mut r);
        assert_eq!(atlas.image.bytes.len(), 120 * 40 * 4);
        assert!(atlas.image.bytes.chunks_exact(4).any(|p| p[3] != 0));
        assert!(atlas.image.bytes.chunks_exact(4).any(|p| p[3] == 0));
    }
    #[test]
    fn a_filter_that_follows_the_documents_blending_previews_its_live_result() {
        let extent = [300, 200];
        let view = layer_render::ViewState {
            width_px: extent[0],
            height_px: extent[1],
            document_to_surface: [1., 0., 0., 1., 0., 0.],
        };
        let stripes = layer_core::color::source::rgba8_source(extent, |x, y| {
            let v = if (x / 9) % 2 == 0 { 20 } else { 235 };
            [v, (y / 2) as u8, 255 - v, 255]
        });
        let definition = fixture("gaussian_blur");
        assert_eq!(definition.program().space, layer_core::EffectSpace::Blending);
        for space in layer_core::BlendSpace::ALL {
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let mut artwork=Artwork::new(extent).unwrap();
            artwork.compositions.get_mut(artwork.root).unwrap().blend=space;
            let target=paint(&mut artwork,None);let source=paint(&mut artwork,Some(stripes.clone()));
            let mut doc=document(artwork,vec![target,source]);
            r.submit(FramePacket { view, blend_space: space, ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
            let size = [120, 40];
            r.request_filter_previews(request(&doc,target,1,size,view,vec![Arc::new(definition.preview().unwrap())])).unwrap();
            let preview = finish(&mut r).image.bytes;
            let point = r.filter_previews.as_ref().unwrap().point.unwrap();
            let origin: [u32; 2] = std::array::from_fn(|i| (point[i] - size[i] / 2).min(extent[i] - size[i]));
            let mut authored=doc.artwork.clone();
            let replacement=effect(&mut authored,definition.preview().unwrap());
            let content=authored.occurrences.get(replacement).unwrap().content.clone();
            let OccurrenceContent::Effect(application)=content else {unreachable!()};
            doc.apply(Edit::Batch(vec![
                Edit::Effect(RecordChange::replace(&authored.effects,application,authored.effects.get(application).cloned()).unwrap()),
                Edit::Occurrence({let mut value=doc.artwork.occurrences.get(target).unwrap().clone();value.content=content;RecordChange::replace(&doc.artwork.occurrences,target,Some(value)).unwrap()}),
            ])).unwrap();
            r.submit(FramePacket { view, blend_space: space, ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
            let live = r.readback_srgb_rgba8().unwrap();
            let mut compared = 0;
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let shown = &preview[((y * size[0] + x) * 4) as usize..][..4];
                    if shown[3] < 255 {
                        continue;
                    }
                    let document = &live[(((origin[1] + y) * extent[0] + origin[0] + x) * 4) as usize..][..3];
                    for c in 0..3 {
                        assert!(shown[c].abs_diff(document[c]) <= 1, "{space:?} at {x},{y}: preview {shown:?}, live {document:?}");
                    }
                    compared += 1;
                }
            }
            assert!(compared > 500, "{space:?}: {compared} opaque preview pixels");
        }
    }
    #[test]
    fn a_target_in_a_pass_through_group_previews_the_layers_below_the_group() {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let extent = [300, 200];
        let view = layer_render::ViewState {
            width_px: extent[0],
            height_px: extent[1],
            document_to_surface: [1., 0., 0., 1., 0., 0.],
        };
        let grouped=group_document(extent,layer_core::LayerBlend::PassThrough,"black_white");
        let flat=flat_document(extent);
        let isolated=group_document(extent,layer_core::LayerBlend::Normal,"black_white");
        let mut preview = |(doc,target): &(Document,OccurrenceHandle), request_id| {
            r.submit(FramePacket { view, ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
            r.request_filter_previews(request(doc,*target,request_id,[120,40],view,vec![Arc::new(fixture("exposure").preview().unwrap())])).unwrap();
            finish(&mut r).image.bytes
        };
        let passing = preview(&grouped, 1);
        assert_eq!(passing, preview(&flat, 2), "the source is what lies below, without the group's fade");
        assert_ne!(passing, preview(&isolated, 3), "an isolated group's source holds only its own layers");
    }
    #[test]
    fn filter_probe_budget_grows_gradually_and_yields_on_slow_encoding_or_completion() {
        let ms = Duration::from_millis;
        assert_eq!(probe_batch_size(1, ms(0), ms(0), true), 2);
        assert_eq!(probe_batch_size(8, ms(0), ms(1), true), 16);
        assert_eq!(probe_batch_size(16, ms(0), ms(8), true), 4);
        assert_eq!(probe_batch_size(8, ms(4), ms(1), true), 4);
        assert_eq!(probe_batch_size(4, ms(0), ms(16), false), 4);
        assert_eq!(probe_batch_size(4, ms(0), ms(64), false), 1);
    }

    #[test]
    fn filter_probe_keeps_searching_for_a_full_crop_after_finding_isolated_ink() {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let extent = [1025, 513];
        for (code, expected, all_tiles) in [
            ("let ink=all(floor(p)==vec2<f32>(512.,256.)) || (p.x>=950. && p.y>=400.);", [975,410], false),
            ("let ink=all(floor(p)==vec2<f32>(512.,256.));", [512,256], true),
            ("let ink=false;", [0,0], true),
        ] {
            let mut artwork = Artwork::new(extent).unwrap();
            let source = generated(&mut artwork, &format!("fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{{code}return select(vec4<f32>(0.),vec4<f32>(.2,.4,.6,1.),ink);}}"));
            let doc = document(artwork, vec![source]);
            r.submit(crate::test_support::packet(doc.scene(), extent)).unwrap();
            r.request_filter_previews(request(&doc, source, 1, [50,20], crate::test_support::view(extent), vec![Arc::new(fixture("curves").preview().unwrap())])).unwrap();
            finish(&mut r);
            let previews = r.filter_previews.as_ref().unwrap();
            assert_eq!(previews.point.unwrap_or([0,0]), expected);
            assert_eq!(previews.probe_next == previews.probe_tiles.len(), all_tiles);
            r.cancel_filter_previews();
        }
    }

    #[test]
    fn filter_probe_chunks_bound_sources_and_cancel_changed_documents() {
        use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        })
        .unwrap();
        let extent = [2049, 513]; // 27 tiles, including partial right/bottom edges.
        let mut artwork=Artwork::new(extent).unwrap();
        artwork.compositions.get_mut(artwork.root).unwrap().color=DocumentColor {space:RgbSpace::ProPhoto,depth:SampleDepth::U16};
        let pattern=pattern(&mut artwork);
        let mut program = (*fixture("exposure").program()).clone();
        program.entry = "blur".into();
        program.wgsl = "fn blur(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p+vec2<f32>(3.,0.))+c+fx_sample(p-vec2<f32>(3.,0.)))/3.;}".into();
        program.passes = vec![layer_core::EffectPass {entry:"blur".into(),sampling:layer_core::EffectSampling::Neighborhood {radius:3}}].into();
        let blur=effect(&mut artwork,layer_core::EffectInstance::new(Arc::new(program)));
        let mut upper_effect=fixture("black_white").preview().unwrap();
        Arc::make_mut(&mut upper_effect.program).passes=vec![layer_core::EffectPass {entry:upper_effect.program.entry.clone(),sampling:layer_core::EffectSampling::Document}].into();
        let upper=effect(&mut artwork,upper_effect);
        let mut fill=fixture("solid_color").preview().unwrap();
        fill.set("color",layer_core::EffectValue::Color(layer_core::color::RgbColor::WHITE)).unwrap();
        let paper=effect(&mut artwork,fill);
        artwork.occurrences.get_mut(paper).unwrap().opacity=0.5;
        let mut doc=document(artwork,vec![upper,blur,pattern,paper]);
        let view = layer_render::ViewState {
            width_px: extent[0],
            height_px: extent[1],
            document_to_surface: [1., 0., 0., 1., 0., 0.],
        };
        let frame = |r: &mut WgpuRasterizer, doc: &Document| {
            r.submit(FramePacket {commit_rasters:true,time_seconds:0.,view,composite_all:true,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
        };
        frame(&mut r,&doc);
        let preview_request=|doc:&Document,id|request(doc,blur,id,[200,40],view,vec![Arc::new(fixture("exposure").preview().unwrap())]);
        let mut scoped=preview_request(&doc,1);scoped.scope=SceneScope::Members(vec![blur,pattern,paper].into());
        r.request_filter_previews(scoped).unwrap();
        let p = r.filter_previews.as_ref().unwrap();
        assert_eq!(p.probe_next, 1, "first call submits one bounded tile");
        assert!(p.request.is_some());
        let probe_texture = p.source.as_ref().unwrap().0.clone();
        // A view-only frame must compare the caller's paper color, before the
        // compositor applies paper opacity. It must not cancel this scan.
        frame(&mut r, &doc);
        let mut callbacks = 0;
        let mut previous_probe_next = 1;
        loop {
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
            let result = r.take_filter_previews();
            let p = r.filter_previews.as_ref().unwrap();
            assert!(p.probe_next - previous_probe_next <= 16, "one poll advances at most one chunk");
            previous_probe_next = p.probe_next;
            let (texture, _) = p.source.as_ref().unwrap();
            if p.point.is_none() {
                assert_eq!(texture, &probe_texture, "all probe chunks reuse one texture");
            }
            assert!(texture.width() <= PAGE_SIZE + 200 && texture.height() <= PAGE_SIZE + 40);
            assert!(
                p.source_scene.image_cache_bytes()
                    <= 3 * (PAGE_SIZE + 206) as u64 * (PAGE_SIZE + 46) as u64 * 16
            );
            callbacks += 1;
            if let Some(result) = result {
                assert_eq!(result.unwrap().image.request_id, 1);
                break;
            }
            assert!(callbacks < 50);
        }
        let p = r.filter_previews.as_ref().unwrap();
        assert_eq!(p.probe_next, 1, "center content makes the other 26 tiles unnecessary");
        assert_eq!(p.point, Some([1024, 256]));
        assert!(callbacks <= 3);
        assert!(p.probe_winner.is_none());
        // A metadata edit between chunks cancels the old request after its
        // in-flight callback. It cannot combine different document revisions.
        r.filter_previews.as_mut().unwrap().key = None;
        r.request_filter_previews(preview_request(&doc, 2)).unwrap();
        occurrence_edit(&mut doc,blur,|o|o.opacity=0.5);
        frame(&mut r, &doc);
        r.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(READBACK_TIMEOUT),
            })
            .unwrap();
        assert!(matches!(r.take_filter_previews(), Some(Err(GpuRasterError::FilterPreviewCancelled))));
        assert!(!r.filter_previews_pending());
        assert_eq!(r.filter_previews.as_ref().unwrap().probe_next, 1);
        r.request_filter_previews(preview_request(&doc, 3)).unwrap();
        assert_eq!(finish(&mut r).image.request_id, 3);
        // Explicit hide/input cancellation never waits for the old callback.
        // A replacement request can start immediately on a fresh channel.
        r.filter_previews.as_mut().unwrap().key = None;
        r.request_filter_previews(preview_request(&doc, 30)).unwrap();
        let old_sender = r.filter_previews.as_ref().unwrap().tx.clone();
        r.cancel_filter_previews();
        assert!(!r.filter_previews_pending());
        assert!(r.take_filter_previews().is_none());
        assert!(old_sender.send(Ready::Probe { scores: Ok([0; 2]), batch_size: 1 }).is_err());
        r.request_filter_previews(preview_request(&doc, 31)).unwrap();
        assert_eq!(finish(&mut r).image.request_id, 31);
        // A valid conservative support declaration can exceed the adapter's
        // texture dimension. Its actual dependency still ends at the document.
        let mut wide = preview_request(&doc, 4);
        let mut program = (*fixture("exposure").program()).clone();
        program.id = "test:wide-support".into();
        program.passes = (0..3)
            .map(|_| layer_core::EffectPass {
                entry: program.entry.clone(),
                sampling: layer_core::EffectSampling::Neighborhood { radius: 4096 },
            })
            .collect::<Vec<_>>()
            .into();
        wide.filters = vec![Arc::new(layer_core::EffectInstance::new(Arc::new(program)))];
        r.request_filter_previews(wide).unwrap();
        assert_eq!(finish(&mut r).image.request_id, 4);
        assert_eq!(r.filter_previews.as_ref().unwrap().scratch_size, extent);
    }

    #[test]
    fn p25_full_source_thumbnail_guides_reuse_invalidate_and_cancel() {
        let extent = [512, 256];
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut artwork=Artwork::new(extent).unwrap();
        let source=paint(&mut artwork,Some(layer_core::color::source::rgba8_source(extent, |x,y| {
            let v = if x < 260 {12 + ((x/11+y/9)%2) as u8*55} else {160+(x%71) as u8};
            [v,v.saturating_add(8),v.saturating_add(16),255]
        })));
        let target=paint(&mut artwork,None);let mut doc=document(artwork,vec![target,source]);
        let frame=|r:&mut WgpuRasterizer,doc:&Document|{r.submit(crate::test_support::packet(doc.scene(),extent)).unwrap();};
        frame(&mut r,&doc);
        let preview_request=|doc:&Document,request_id,zero:bool|request(doc,target,request_id,[120,40],crate::test_support::packet(doc.scene(),extent).view,
            [("shadows_highlights","shadows"),("clarity","amount")].into_iter().map(|(id,key)|{
                let mut effect=fixture(id).preview().unwrap();effect.set(key,layer_core::EffectValue::Number(if zero {0.}else{50.})).unwrap();Arc::new(effect)
            }).collect());
        assert!(r.request_filter_previews(preview_request(&doc,1,false)).unwrap());
        let nonzero=finish(&mut r).image.bytes;
        let previews=r.filter_previews.as_ref().unwrap();
        let entries:Vec<_>=previews.analyses.iter().filter(|entry|previews.programs.values().any(|layer|*layer==entry.layer())).collect();
        assert_eq!(entries.len(),2);assert!(Arc::ptr_eq(&entries[0].resource,&entries[1].resource));
        let resource=Arc::downgrade(&entries[0].resource);
        let origin=previews.point.unwrap();assert!(origin[0]>0 || origin[1]>0);
        assert!(r.effect_analyses.is_empty(),"private preview guides never replace live document guides");
        assert!(r.request_filter_previews(preview_request(&doc,2,true)).unwrap());
        let identity=finish(&mut r).image.bytes;
        for row in 0..2 {
            let start=row*120*40*4;let end=start+120*40*4;
            assert_ne!(&nonzero[start..end],&identity[start..end],"row{row} must use source-aware guide");
        }
        let current=&r.filter_previews.as_ref().unwrap().analyses[0].resource;
        assert!(std::sync::Weak::ptr_eq(&resource,&Arc::downgrade(current)),"same full source reuses immutable guide");
        let OccurrenceContent::Paint(paint)=doc.artwork.occurrences.get(source).unwrap().content else {unreachable!()};
        let mut value=doc.artwork.paint.get(paint).unwrap().clone();
        value.base=Some(layer_core::authored::PaintBase::new((layer_core::color::source::rgba8_source(extent,|x,y|[20+(x%101) as u8,35+(y%113) as u8,25,255])).into()));
        doc.apply(Edit::Paint(RecordChange::replace(&doc.artwork.paint,paint,Some(value)).unwrap())).unwrap();
        frame(&mut r,&doc);
        assert!(r.request_filter_previews(preview_request(&doc,3,false)).unwrap());
        let edited=finish(&mut r).image.bytes;
        assert_ne!(nonzero,edited,"source edit invalidates rows");
        let current=&r.filter_previews.as_ref().unwrap().analyses[0].resource;
        assert!(!std::sync::Weak::ptr_eq(&resource,&Arc::downgrade(current)),"source edit replaces guide");
        let released=Arc::downgrade(current);
        r.cancel_filter_previews();
        assert!(!r.filter_previews_pending());
        assert!(r.filter_previews.as_ref().unwrap().analyses.is_empty());
        assert!(released.upgrade().is_none(),"hide cancels and releases completed private guide resources");
        assert!(r.request_filter_previews(preview_request(&doc,4,false)).unwrap());
        r.cancel_filter_previews();
        assert!(!r.filter_previews_pending());
        assert!(r.filter_previews.as_ref().unwrap().analysis.is_none());
        assert!(r.take_filter_previews().is_none());
    }

    #[test]
    fn p25_source_aware_thumbnail_uses_insertion_group_scope() {
        let extent = [300,200];
        for id in ["shadows_highlights", "clarity"] {
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let grouped=group_document(extent,layer_core::LayerBlend::PassThrough,"invert");
            let flat=flat_document(extent);
            let isolated=group_document(extent,layer_core::LayerBlend::Normal,"invert");
            let mut preview=|(doc,target):&(Document,OccurrenceHandle),request_id|{
                let packet=crate::test_support::packet(doc.scene(),extent);let view=packet.view;r.submit(packet).unwrap();
                assert!(r.request_filter_previews(request(doc,*target,request_id,[120,40],view,vec![Arc::new(fixture(id).preview().unwrap())])).unwrap());
                finish(&mut r).image.bytes
            };
            let passing = preview(&grouped,1);
            assert_eq!(passing,preview(&flat,2),"{id}: full-source guide ignores upper effects and pass-through fade");
            assert_ne!(passing,preview(&isolated,3),"{id}: isolated insertion source excludes outside backdrop");
        }
    }

    #[test]
    fn p25_source_aware_thumbnail_preserves_selected_clipping_base() {
        let extent = [300,200];
        let upper_code = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(.08+p.x/fx_extent().x*.15,.04+p.y/fx_extent().y*.12,.1,1.);}";
        let flat_code = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{if p.x>=fx_extent().x*.5{return vec4<f32>(0.);}return vec4<f32>(.08+p.x/fx_extent().x*.15,.04+p.y/fx_extent().y*.12,.1,1.);}";
        let base_code = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{if p.x>=fx_extent().x*.5{return vec4<f32>(0.);}return vec4<f32>(0.,0.,0.,1.);}";
        let mut artwork=Artwork::new(extent).unwrap();let selected=generated(&mut artwork,upper_code);
        artwork.occurrences.get_mut(selected).unwrap().attachment = layer_core::Attachment::Clip;
        let generated_base=generated(&mut artwork,base_code);
        let stack=artwork.stacks.insert(PortableId::random(),Stack {entries:vec![generated_base]}).unwrap();
        let base=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Stack(stack),"base")).unwrap();
        let clipped=(document(artwork,vec![selected,base]),selected);
        let mut artwork=Artwork::new(extent).unwrap();let target=paint(&mut artwork,None);let source=generated(&mut artwork,flat_code);
        let flat=(document(artwork,vec![target,source]),target);
        for id in ["shadows_highlights","clarity"] {
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let mut preview=|(doc,target):&(Document,OccurrenceHandle),request_id|{
                let packet=crate::test_support::packet(doc.scene(),extent);let view=packet.view;r.submit(packet).unwrap();
                assert!(r.request_filter_previews(request(doc,*target,request_id,[120,40],view,vec![Arc::new(fixture(id).preview().unwrap())])).unwrap());
                finish(&mut r).image.bytes
            };
            assert_eq!(preview(&clipped,1),preview(&flat,2),"{id}: clipping input and equivalent composed source match");
        }
    }

}
