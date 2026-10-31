//! Idle-time GPU previews of catalog filters on a fixed tile scene, with
//! shared pipelines and small asynchronous image readbacks.
use super::*;
use layer_core::SceneSnapshot;
use layer_core::authored::{Artwork, SceneIndex, PortableId, Occurrence, OccurrenceContent, EffectApplication, PaintBase, PaintSource};
use layer_render::{FilterPreviewImage, FilterPreviewRequest};
use std::{collections::HashMap, sync::Arc};
#[path = "filter_preview_tiles.rs"]
mod tiles;

type Image = (wgpu::Texture, wgpu::TextureView);
pub(crate) struct FilterPreviews {
    scene: Box<Scene>,
    tiles: Image,
    programs: HashMap<Arc<str>, OccurrenceHandle>,
    program_snapshot: Option<Arc<SceneSnapshot>>,
    scratch: Vec<Image>,
    rows: HashMap<Arc<str>, Vec<u8>>,
    request: Option<FilterPreviewRequest>,
    rendering: Vec<Arc<str>>,
    analyses: Vec<Arc<crate::effect_analysis::Prepared>>,
    analysis: Option<crate::effect_analysis::Job>,
    analysis_queries: Vec<layer_core::ArtworkQuery>,
    preparing: bool,
    cancelled: bool,
    tx: mpsc::Sender<Result<ReadbackImage, GpuRasterError>>,
    rx: mpsc::Receiver<Result<ReadbackImage, GpuRasterError>>,
}
impl FilterPreviews {
    pub(crate) fn rendition_changed(&mut self) {
        self.rows.clear();
        self.cancelled |= self.request.is_some();
    }
    fn new(r: &mut WgpuRasterizer) -> Result<Self, GpuRasterError> {
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            scene: Scene::new(r)?,
            tiles: tiles::texture(r),
            programs: HashMap::new(),
            program_snapshot: None,
            scratch: Vec::new(),
            rows: HashMap::new(),
            request: None,
            rendering: Vec::new(),
            analyses: Vec::new(), analysis: None, analysis_queries: Vec::new(), preparing: false,
            cancelled: false,
            tx,
            rx,
        })
    }
    fn start(
        &mut self,
        r: &mut WgpuRasterizer,
        request: FilterPreviewRequest,
    ) -> Result<bool, GpuRasterError> {
        if self.request.is_some() {
            return Ok(false);
        }
        if request.filters.is_empty() || request.filters.len() > 8 {
            return Ok(false);
        }
        let mut artwork=Artwork::new(tiles::EXTENT).map_err(|e|GpuRasterError::Effect(e.into()))?;
        let mut programs=HashMap::new();
        let mut analyses_changed=false;
        for effect in &request.filters {
            effect.validate().map_err(|e|GpuRasterError::Effect(e.into()))?;
            let id=effect.program.id.clone();
            let previous=self.program_snapshot.as_ref().and_then(|snapshot|self.programs.get(&id).and_then(|h|snapshot.view().effect(*h)));
            analyses_changed|=previous.map(|e|e.program.analysis())!=Some(effect.program.analysis());
            if previous.is_none_or(|old|old.program!=effect.program.as_ref()||old.values!=effect.values){self.rows.remove(&id);}
            let effect=EffectApplication::new(effect.program.clone(),effect.values.clone(),tiles::EXTENT);
            let application=artwork.effects.insert(PortableId::random(),effect).map_err(|e|GpuRasterError::Effect(e.into()))?;
            let handle=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(application),"")).map_err(|e|GpuRasterError::Effect(e.into()))?;
            programs.insert(id,handle);
        }
        if analyses_changed||programs!=self.programs{self.clear_analysis();}
        let index=Arc::new(SceneIndex::build(&artwork).map_err(GpuRasterError::Effect)?);
        self.program_snapshot=Some(Arc::new(SceneSnapshot::new(artwork,index,u64::MAX,0,Default::default())));
        self.programs=programs;
        self.cancelled = false;
        // The common UI driver retains delivered rows. Keep only the current
        // bounded request here, rather than a second catalog-sized pixel cache.
        self.rows.retain(|id, _| request.filters.iter().any(|f| f.program.id == *id));
        self.request = Some(request);
        self.queue_analyses()?;
        self.preparing = true;
        self.prepare(r)?;
        Ok(true)
    }
    fn with_analyses<T>(&mut self, r: &mut WgpuRasterizer, operation: impl FnOnce(&mut Self, &mut WgpuRasterizer) -> T) -> T {
        let live = std::mem::replace(&mut r.effect_analyses, self.analyses.clone());
        let result = operation(self, r);
        r.effect_analyses = live;
        result
    }
    fn queue_analyses(&mut self)->Result<(),GpuRasterError>{
        let filters=self.request.as_ref().unwrap().filters.clone();
        for effect in filters {
            if effect.program.analysis().is_none(){continue;}
            let handle=self.programs[&effect.program.id];
            if self.analyses.iter().any(|entry|entry.layer()==handle){continue;}
            let mut snapshot=self.program_snapshot.as_ref().unwrap().as_ref().clone();
            let artwork=&mut snapshot.artwork;
            let paint=artwork.paint.insert(PortableId::random(),PaintSource{color_mode:Default::default(),domain:tiles::EXTENT,raster:Default::default(),
                base:Some(PaintBase::new(tiles::source()?.into())),operations:Arc::default()}).map_err(|e|GpuRasterError::Effect(e.into()))?;
            let layer=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),"")).map_err(|e|GpuRasterError::Effect(e.into()))?;
            let stack=artwork.compositions.get(artwork.root).unwrap().result;
            artwork.stacks.get_mut(stack).unwrap().entries=vec![handle,layer];
            snapshot.index=Arc::new(SceneIndex::build(&snapshot.artwork).map_err(GpuRasterError::Effect)?);
            self.analysis_queries.push(layer_core::ArtworkQuery::from_snapshot(Arc::new(snapshot),layer_core::ArtworkSource::EffectInput(handle),None));
        }
        Ok(())
    }
    fn prepare(&mut self, r: &mut WgpuRasterizer) -> Result<(), GpuRasterError> {
        if r.startup.is_some() && (!r.poll_startup()?.brush_ready || r.effect_validation.is_some()) { return Ok(()); }
        let request = self.request.as_ref().unwrap();
        let snapshot = self.program_snapshot.clone().unwrap();
        if let Some(startup) = &r.startup {
            // Visible rows prepare just their own preview variants. No draw or
            // readback is admitted until their asynchronous pipelines are ready.
            for effect in &request.filters {
                self.scene.effects.prepare(r, snapshot.view(), &[self.programs[&effect.program.id]], effects::Execution::Preview, 0., 0, preview_space(effect.view(), snapshot.view().composition().blend))?;
            }
            let mut ready = self.scene.effects.enqueue_active(&startup.compiler, startup::OTHER);
            // Preview readback uses the same conversion as document export.
            ready &= r.ui_readback_ready();
            startup.compiler.start();
            if !ready { return Ok(()); }
        }
        if let Some(job) = &mut self.analysis {
            let Some(result) = job.take() else {return Ok(());};
            self.analysis = None;
            for entry in result.map_err(GpuRasterError::Effect)?.entries {
                self.analyses.retain(|old| old.layer() != entry.layer());self.analyses.push(entry);
            }
        }
        while let Some(query) = self.analysis_queries.pop() {
            let layer = match query.source {layer_core::ArtworkSource::EffectInput(id)=>id,_=>unreachable!()};
            if self.analyses.iter().any(|entry| entry.layer() == layer) {continue;}
            let kind=query.snapshot.view().effect(layer).and_then(|e|e.program.analysis()).unwrap();
            if let Some(resource) = self.analyses.iter().find(|entry| entry.kind == kind).map(|entry| entry.resource.clone()) {
                self.analyses.push(Arc::new(crate::effect_analysis::Prepared {query,kind,resource}));continue;
            }
            let snapshot=query.snapshot.clone();let scene=snapshot.view();
            let input=crate::effect_analysis::BakeInput{scene:snapshot.clone(),scope:snapshot.scope.clone(),offset:Default::default(),extent:scene.composition().size,color:scene.composition().color,blend:scene.composition().blend,time:snapshot.context.elapsed};
            self.analysis = Some(self.with_analyses(r, |_,r| crate::effect_analysis::Job::frame(r.snapshot_gpu(), input))
                .map_err(GpuRasterError::Effect)?);
            return Ok(());
        }
        self.preparing = false;
        if self.missing_rows() {self.with_analyses(r,Self::render)} else {Ok(())}
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
        let snapshot = self.program_snapshot.clone().unwrap();
        let scene = snapshot.view();
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("filter picker previews"),
            },
        );
        self.scene.begin_frame();
        self.scene.jobs.clear();
        let [width, height] = layer_render::FILTER_PREVIEW_SIZE;
        let extent = tiles::EXTENT;
        let source = self.tiles.1.clone();
        let grid = display_mips::Plan::window(extent, 0, PixelRect::full(extent));
        let spaces: Vec<_> = self.rendering.iter().map(|id| preview_space(scene.effect(self.programs[id]).unwrap(), scene.composition().blend)).collect();
        let encoded;
        let encoded_source = if spaces.contains(&layer_core::BlendSpace::Perceptual) {
            encoded = create_color_target(&r.device, extent, "encoded filter preview source");
            let [width, height] = extent.map(|v| v as f32);
            let mut data = [0.; 32];
            data[..6].copy_from_slice(&[0., 0., width, height, width, height]);
            data[8..10].copy_from_slice(&[1., 1.]);
            data[31] = Convert::Encode.code();
            self.scene.jobs.push(Job::Draw {
                target: encoded.1.clone(),
                sources: [source.clone(), r.empty_view.clone(), r.empty_view.clone(), r.empty_paper_view.clone()],
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
            let prepared = self.scene.effects.prepare(r, scene, &[self.programs[id]], effects::Execution::Preview, 0., 0, space)?;
            let count = scene.effect(self.programs[id]).unwrap().program.passes.len().max(1);
            while self.scratch.len() < count.min(2) {
                self.scratch.push(create_color_target(&r.device, extent, "reused preview scene"));
            }
            let previous = self.scene.preview_passes(r, prepared, grid, (source, grid), &self.scratch, count);
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
            data[12..16].copy_from_slice(&[0., 0., extent[0] as f32, extent[1] as f32]);
            self.scene.jobs.push(Job::Draw {
                target: atlas.1.clone(),
                sources: [previous, r.empty_view.clone(), r.empty_view.clone(), r.empty_paper_view.clone()],
                data,
                over: false,
                clip: None,
                source_target:None,
            });
        }
        self.scene.encode_jobs(r, &mut encoder)?;
        let image_height = height * self.rendering.len() as u32;
        let binding = create_texture_bind_group(
            &r.device,
            &r.float_texture_layout,
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
                let _ = tx.send(image);
            },
        );
        Ok(())
    }
    fn take(
        &mut self,
        r: &mut WgpuRasterizer,
    ) -> Option<Result<FilterPreviewImage, GpuRasterError>> {
        self.request.as_ref()?;
        if self.cancelled {self.clear_analysis();self.request=None;return Some(Err(GpuRasterError::FilterPreviewCancelled));}
        if self.preparing {
            if let Err(error) = self.prepare(r) {self.clear_analysis();self.request = None;return Some(Err(error));}
            return None;
        }
        if let Ok(image) = self.rx.try_recv() {
            match image {
                Ok(image) => {
                    let row_bytes = layer_render::FILTER_PREVIEW_SIZE.iter().product::<u32>() as usize * 4;
                    for (id, bytes) in self.rendering.drain(..).zip(image.bytes.chunks(row_bytes)) {
                        self.rows.insert(id, bytes.to_vec());
                    }
                }
                Err(error) => {
                    self.clear_analysis();
                    self.request = None;
                    return Some(Err(error));
                }
            }
        }
        if self.missing_rows() {
            return None;
        }
        let request = self.request.take().unwrap();
        let [width, height] = layer_render::FILTER_PREVIEW_SIZE;
        let mut bytes = Vec::with_capacity((width * height * 4) as usize * request.filters.len());
        for effect in &request.filters {
            bytes.extend_from_slice(&self.rows[&effect.program.id]);
        }
        Some(Ok(FilterPreviewImage {
            image: ReadbackImage {
                request_id: request.request_id,
                width,
                height: height * request.filters.len() as u32,
                stride: width * 4,
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
            self.jobs.push(Job::Effect { target: target.clone(), sources: [previous, source.clone(), r.empty_view.clone(), r.empty_paper_view.clone()],
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
impl WgpuRasterizer {
    pub(crate) fn cancel_filter_preview_request(&mut self) {
        let Some(previews) = self.filter_previews.as_mut() else { return; };
        previews.request = None;previews.clear_analysis();
        // Old callbacks retain only their sender. Dropping the receiver keeps
        // them from advancing or completing the next request, without a wait.
        (previews.tx, previews.rx) = mpsc::channel();
        previews.rendering.clear();
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
            None => Box::new(FilterPreviews::new(self)?),
        };
        let result = previews.start(self, request);
        if result.is_err() {
            previews.clear_analysis();
            previews.request = None;
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
        self.scene.jobs.clear();
        self.scene.effects.discard_instances();
    }
    fn missing_rows(&self) -> bool {
        self.request.as_ref().is_some_and(|r| r.filters.iter().any(|f| !self.rows.contains_key(&f.program.id)))
    }
    pub(crate) fn storage_bytes(&self) -> u64 {
        let bytes = |(texture, _): &Image| texture_bytes(texture);
        bytes(&self.tiles)
            + self.scratch.iter().map(bytes).sum::<u64>()
            + self.scene.scratch_bytes()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixture;
    use layer_core::Document;

    fn finish(r: &mut WgpuRasterizer) -> FilterPreviewImage {
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            r.device.poll(wgpu::PollType::Wait {submission_index:None,timeout:Some(Duration::from_secs(10))}).unwrap();
            if let Some(result) = r.take_filter_previews() {return result.unwrap();}
            assert!(std::time::Instant::now() < deadline, "preview did not complete");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn request(request_id: u64, filters: &[&str]) -> FilterPreviewRequest {
        FilterPreviewRequest { request_id, filters: filters.iter().map(|id| Arc::new(fixture(id).preview().unwrap())).collect() }
    }
    fn document(extent: [u32; 2], color: [u8; 4]) -> Document {
        let mut artwork = Artwork::new(extent).unwrap();
        let (_, target) = crate::test_support::add_paint(&mut artwork, "paint", extent);
        let SourceTarget::Paint(source) = target else { unreachable!() };
        artwork.paint.get_mut(source).unwrap().base = Some(PaintBase::new(layer_core::color::source::rgba8_source(extent, |_, _| color).into()));
        Document::from_artwork(artwork).unwrap()
    }
    fn ready(doc: &Document) -> WgpuRasterizer {
        let extent = doc.composition().size;
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.submit(crate::test_support::packet(doc.scene(), extent)).unwrap();
        r
    }
    fn staged(doc: &Document) -> WgpuRasterizer {
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut r = crate::test_support::staged_renderer(&reference, doc.composition().color);
        r.prepare_startup(doc, &layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), false).unwrap();
        crate::test_support::wait_startup(&mut r, std::time::Instant::now() + Duration::from_secs(30), |p| p.complete, format_args!("Preview startup"));
        r.submit(crate::test_support::packet(doc.scene(), doc.composition().size)).unwrap();
        r
    }
    fn alpha(image: &ReadbackImage, [x, y]: [u32; 2]) -> u8 { image.bytes[((y * image.width + x) * 4 + 3) as usize] }

    #[test]
    fn idle_filter_preview_requests_refresh_completed_startup_work() {
        let mut r = staged(&document([64; 2], [20, 240, 30, 255]));
        r.shader_idle(false, false);
        assert!(r.request_filter_previews(request(1, &["curves"])).unwrap());
        assert!(r.filter_previews_pending());
        assert!(r.take_filter_previews().is_none());
        assert!(r.startup.as_ref().unwrap().compiler.pending() > 0);
        assert!(!r.poll_startup().unwrap().complete);
        r.shader_idle(true, true);
        let image = finish(&mut r).image;
        assert_eq!(image.request_id, 1);
        assert_eq!([image.width, image.height], layer_render::FILTER_PREVIEW_SIZE);
        assert_eq!(alpha(&image, [220, 20]), 255, "Tiles fill the row");
        assert_eq!(alpha(&image, [420, 20]), 0, "One tile is missing");
    }
    #[test]
    fn cancelled_preview_categories_do_not_block_ready_rows() {
        let mut r = staged(&document([64; 2], [20, 40, 230, 128]));
        r.shader_idle(true, true);
        let curves = request(1, &["curves"]);
        assert!(r.request_filter_previews(curves.clone()).unwrap());
        let original = finish(&mut r).image;
        let preparations = r.filter_previews.as_ref().unwrap().scene.effects.preparation_count();
        r.cancel_filter_previews();r.shader_idle(false, false);
        assert!(r.request_filter_previews(request(2, &["gaussian_blur"])).unwrap());
        assert!(r.filter_previews_pending());
        assert!(r.take_filter_previews().is_none());
        assert!(r.startup.as_ref().unwrap().compiler.pending() > 0);
        let progress = r.poll_startup().unwrap();
        assert!(progress.brush_ready);assert!(!progress.complete);
        r.cancel_filter_previews();
        let mut reopened = curves;reopened.request_id = 3;
        assert!(!r.filter_previews_pending());
        assert!(r.request_filter_previews(reopened).unwrap());
        let completed = finish(&mut r).image;
        assert_eq!(completed.request_id, 3);
        assert_eq!(completed.bytes, original.bytes);
        assert!(r.startup.as_ref().unwrap().compiler.pending() > 0, "Unrelated optional shaders remain unready");
        assert_eq!(r.filter_previews.as_ref().unwrap().scene.effects.preparation_count(), preparations, "Cancelled lookup dispatches never run");
        r.shader_idle(true, true);
    }
    #[test]
    fn previews_show_the_same_tiles_whatever_the_document_holds() {
        let rows = [([64, 64], [230, 40, 20, 128]), ([300, 90], [20, 40, 230, 255])].map(|(extent, color)| {
            let mut r = ready(&document(extent, color));
            assert!(r.request_filter_previews(request(1, &["invert", "gaussian_blur"])).unwrap());
            finish(&mut r).image.bytes
        });
        assert_eq!(rows[0], rows[1]);
    }
    #[test]
    fn an_identity_preview_keeps_the_tiles_colors() {
        let mut r = ready(&document([64; 2], [0; 4]));
        let identity = Arc::new(layer_core::EffectInstance::new(fixture("exposure").program()));
        assert!(r.request_filter_previews(FilterPreviewRequest { request_id: 1, filters: vec![identity] }).unwrap());
        let decode = |v: u8| layer_core::color::RgbSpace::Srgb.decode(f64::from(v) / 255.);
        let mean = |pixels: &[u8]| {
            let opaque: Vec<_> = pixels.as_chunks::<4>().0.iter().filter(|p| p[3] == 255).collect();
            std::array::from_fn::<f64, 3, _>(|c| layer_core::color::RgbSpace::Srgb.encode(opaque.iter().map(|p| decode(p[c])).sum::<f64>() / opaque.len() as f64) * 255.)
        };
        let (row, tiles) = (mean(&finish(&mut r).image.bytes), mean(tiles::TILES));
        assert!((0..3).all(|c| (row[c] - tiles[c]).abs() < 1.5), "{row:?} against {tiles:?}");
    }
    fn rows(r: &mut WgpuRasterizer) -> Vec<(Arc<str>, Vec<u8>)> {
        let filters: Vec<_> = layer_core::bundled_effect_catalog().filters().iter().map(|f| Arc::new(f.preview().unwrap())).collect();
        let row = layer_render::FILTER_PREVIEW_SIZE.iter().product::<u32>() as usize * 4;
        filters.chunks(8).enumerate().flat_map(|(batch, filters)| {
            assert!(r.request_filter_previews(FilterPreviewRequest { request_id: batch as u64, filters: filters.to_vec() }).unwrap());
            let image = finish(r);
            image.filters.into_iter().zip(image.image.bytes.chunks(row).map(<[u8]>::to_vec)).collect::<Vec<_>>()
        }).collect()
    }
    fn bundled(id: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/layer-web/filter-previews").join(format!("{id}.webp"))
    }
    #[cfg(feature = "bake")]
    #[test]
    #[ignore = "writes apps/layer-web/filter-previews"]
    fn write_bundled_filter_previews() {
        let mut r = ready(&document([64; 2], [0; 4]));
        std::fs::create_dir_all(bundled("").parent().unwrap()).unwrap();
        let [width, height] = layer_render::FILTER_PREVIEW_SIZE;
        for (id, pixels) in rows(&mut r) {
            std::fs::write(bundled(&id), crate::bundled_preview_webp(width, height, &pixels).unwrap()).unwrap();
        }
    }
    #[test]
    fn bundled_filter_previews_match_their_bakes() {
        let mut r = ready(&document([64; 2], [0; 4]));
        let [width, height] = layer_render::FILTER_PREVIEW_SIZE;
        for (id, fresh) in rows(&mut r) {
            let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(std::fs::read(bundled(&id)).unwrap())).unwrap();
            assert_eq!(decoder.dimensions(), (width, height));
            let mut baked = vec![0; decoder.output_buffer_size().unwrap()];
            decoder.read_image(&mut baked).unwrap();
            if !decoder.has_alpha() { baked = baked.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(); }
            let block = |pixels: &[u8], x: u32, y: u32, c: usize| (0..16).map(|k| {
                let i = (((y * 4 + k / 4) * width + x * 4 + k % 4) * 4) as usize;
                let alpha = f64::from(pixels[i + 3]);
                if c == 3 { alpha } else { f64::from(pixels[i + c]) * alpha / 255. }
            }).sum::<f64>() / 16.;
            let mut total = 0.;
            for y in 0..height / 4 { for x in 0..width / 4 { for c in 0..4 { total += (block(&fresh, x, y, c) - block(&baked, x, y, c)).abs(); } } }
            let difference = total / f64::from(width * height / 4);
            assert!(difference < 4., "{id}: preview is {difference:.2} codes from its bake; run write_bundled_filter_previews");
        }
    }
    #[test]
    fn analysis_rows_share_a_private_guide_until_hidden() {
        let mut r = ready(&document([64; 2], [12, 20, 28, 255]));
        let rows = |request_id, amount| FilterPreviewRequest { request_id, filters: [("shadows_highlights", "shadows"), ("clarity", "amount")]
            .into_iter().map(|(id, key)| { let mut effect = fixture(id).preview().unwrap(); effect.set(key, layer_core::EffectValue::Number(amount)).unwrap(); Arc::new(effect) }).collect() };
        assert!(r.request_filter_previews(rows(1, 50.)).unwrap());
        let adjusted = finish(&mut r).image.bytes;
        let previews = r.filter_previews.as_ref().unwrap();
        assert_eq!(previews.analyses.len(), 2);
        assert!(Arc::ptr_eq(&previews.analyses[0].resource, &previews.analyses[1].resource));
        let guide = Arc::downgrade(&previews.analyses[0].resource);
        assert!(r.effect_analyses.is_empty(), "Private preview guides never replace live document guides");
        assert!(r.request_filter_previews(rows(2, 0.)).unwrap());
        let unadjusted = finish(&mut r).image.bytes;
        for row in 0..2 {
            let size = layer_render::FILTER_PREVIEW_SIZE.iter().product::<u32>() as usize * 4;
            let range = row * size..(row + 1) * size;
            assert_ne!(adjusted[range.clone()], unadjusted[range], "Row {row} uses its guide");
        }
        assert!(std::sync::Weak::ptr_eq(&guide, &Arc::downgrade(&r.filter_previews.as_ref().unwrap().analyses[0].resource)), "The fixed scene keeps its guide");
        r.cancel_filter_previews();
        assert!(r.filter_previews.as_ref().unwrap().analyses.is_empty());
        assert!(guide.upgrade().is_none(), "Hiding releases the private guide");
    }
}
