//! Sparse working visibility masks. Geometry and brush instances stay GPU-resident.
use super::*;
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

#[derive(Clone)]
pub(super) struct MaskPage {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}
impl MaskPage {
    pub fn new(device: &PipelineDevice) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sparse layer mask page"),
            size: wgpu::Extent3d {
                width: PAGE_SIZE,
                height: PAGE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: device.scalar_format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | if device.scalar_format() == wgpu::TextureFormat::R32Float {
                    wgpu::TextureUsages::STORAGE_BINDING
                } else { wgpu::TextureUsages::empty() },
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view }
    }
}
pub(super) type CommandCoverage = (SourceTarget, u32, bool);

pub(super) fn underfill_coverage(operation: &layer_core::RasterOperation) -> Option<layer_core::CoverageSnapshot> {
    let layer_core::RasterOperationKind::Fill { underfill: Some(selection), .. } = &operation.kind else { return None; };
    let mut coverage = layer_core::CoverageSnapshot::reveal_all(operation.coverage.target, operation.coverage.source.domain, [0; 2]);
    coverage.source.default_coverage = f32::from(selection.inverted);
    coverage.selection = Some(selection.clone());
    Some(coverage)
}

#[derive(Clone, Default)]
pub(super) struct SnapshotMasks {
    pub definitions: BTreeMap<SourceTarget, layer_core::CoverageSource>,
    pub pages: BTreeMap<(SourceTarget, [u32; 2]), MaskPage>,
    pub(crate) _lease: Option<Arc<effect_analysis::Lease>>,
}

pub(super) struct MaskRenderer {
    pub definitions: BTreeMap<SourceTarget, layer_core::authored::CoverageSource>,
    pub pages: BTreeMap<(SourceTarget, [u32; 2]), MaskPage>,
    pub command_pages: BTreeMap<(CommandCoverage, [u32; 2]), MaskPage>,
    pub(super) preview_bases: BTreeMap<(SourceTarget, [u32; 2]), MaskPage>,
    command_definitions: BTreeMap<CommandCoverage, layer_core::CoverageSnapshot>,
    pub snapshots: BTreeMap<CommandCoverage, SnapshotMasks>,
    pipelines: Arc<Pipelines>,
}
pub(super) struct Pipelines {
    pub(super) brush: [Deferred<wgpu::RenderPipeline>; 6],
    pub(super) initialize: Deferred<wgpu::RenderPipeline>,
    init_layout: wgpu::BindGroupLayout,
    empty_selection: wgpu::Buffer,
}
impl MaskRenderer {
    pub(super) fn brush_index(style: &layer_render::DabStyle) -> usize {
        let shape = if style.contact.is_some_and(|c| c.dry.is_some()) { 4 }
            else { usize::from(matches!(style.tip, BrushTip::Mask(_)))*2 };
        shape+usize::from(style.mode == DabMode::Erase)
    }
    pub fn from_pipelines(pipelines: Arc<Pipelines>) -> Self {
        Self { definitions: BTreeMap::new(), pages: BTreeMap::new(), command_pages: BTreeMap::new(),
            preview_bases: BTreeMap::new(), command_definitions: BTreeMap::new(), snapshots: BTreeMap::new(), pipelines }
    }
}
impl std::ops::Deref for MaskRenderer {
    type Target = Pipelines;
    fn deref(&self) -> &Self::Target { &self.pipelines }
}
impl Pipelines {
    pub fn new(device: &PipelineDevice, style: &wgpu::BindGroupLayout, target: &wgpu::BindGroupLayout,
        texture: &wgpu::BindGroupLayout, advanced_texture: &wgpu::BindGroupLayout) -> Self {
        let shader = {
            let device = device.clone();
            Deferred::new(move || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("mask coverage brush"),
                    source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                        include_str!("analytic_coverage.wgsl"),
                    include_str!("brush.wgsl"),
                        include_str!("selection_clip.wgsl"),
                    ])),
                })
            })
        };
        let analytic = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask analytic layout"),
            bind_group_layouts: &[Some(style), Some(target)],
            immediate_size: 0,
        });
        let tip = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask tip layout"),
            bind_group_layouts: &[Some(style), Some(target), Some(texture)],
            immediate_size: 0,
        });
        let contact_shader = advanced_brush_shader(device);
        let contact = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask relief contact layout"),
            bind_group_layouts: &[Some(style), Some(target), Some(advanced_texture)],
            immediate_size: 0,
        });
        let brush = std::array::from_fn(|i| {
            let blend = wgpu::BlendComponent {
                src_factor: if i % 2 == 0 {
                    wgpu::BlendFactor::One
                } else {
                    wgpu::BlendFactor::Zero
                },
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            };
            let (device, analytic, tip, shader) = (
                device.clone(),
                analytic.clone(),
                tip.clone(),
                if i < 4 { shader.clone() } else { contact_shader.clone() },
            );
            let contact = contact.clone();
            Deferred::pipeline(move |mode| {
                brush_pipeline_format_recipe(
                    mode,
                    &device,
                    if i < 2 { &analytic } else if i < 4 { &tip } else { &contact },
                    &shader,
                    if i < 2 {
                        "analytic_fragment"
                    } else if i < 4 {
                        "mask_fragment"
                    } else {
                        "coverage_fragment"
                    },
                    wgpu::BlendState {
                        color: blend,
                        alpha: blend,
                    },
                    if device.portable_blend() { wgpu::TextureFormat::Rgba32Float } else { device.scalar_format() },
                    "mask coverage brush",
                )
            })
        });
        let init_layout = crate::bindings::layout(device, "selection coverage layout", &[
            crate::bindings::buffer(
                0,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BufferBindingType::Uniform,
                false,
                None,
            ),
            crate::bindings::buffer(
                1,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BufferBindingType::Storage { read_only: true },
                false,
                None,
            ),
        ]);
        let init_shader = {
            let device = device.clone();
            Deferred::new(move || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("polygon selection coverage"),
                    source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                        include_str!("selection.wgsl"),
                        &include_str!("selection_clip.wgsl").replace("@group(1)", "@group(0)"),
                    ])),
                })
            })
        };
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection initialization"),
            bind_group_layouts: &[Some(&init_layout)],
            immediate_size: 0,
        });
        let initialize = {
            let (device, layout, init_shader) =
                (device.clone(), layout.clone(), init_shader.clone());
            Deferred::pipeline(move |mode| {
                fullscreen_pipeline_recipe(
                    mode,
                    &device,
                    &layout,
                    &init_shader,
                    "fragment_main",
                    None,
                    device.scalar_format(),
                    "antialiased mask selection",
                )
            })
        };
        Self {
            brush,
            initialize,
            init_layout,
            empty_selection: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("empty mask selection"),
                size: 48,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
        }
    }
}
impl MaskRenderer {
    pub fn bind_snapshot(&mut self, command: CommandCoverage) -> Option<SnapshotMasks> {
        let snapshot = self.snapshots.get_mut(&command)?;
        Some(SnapshotMasks {
            definitions: std::mem::replace(&mut self.definitions, std::mem::take(&mut snapshot.definitions)),
            pages: std::mem::replace(&mut self.pages, std::mem::take(&mut snapshot.pages)),
            _lease: None,
        })
    }
    pub fn restore_snapshot(&mut self, command: CommandCoverage, live: SnapshotMasks) {
        let snapshot = self.snapshots.get_mut(&command).expect("prepared bake mask inventory");
        snapshot.definitions = std::mem::replace(&mut self.definitions, live.definitions);
        snapshot.pages = std::mem::replace(&mut self.pages, live.pages);
    }
    pub fn is_mask(_scene: SceneView<'_>, target: SourceTarget) -> bool { matches!(target, SourceTarget::Coverage(_)) }
    pub fn prepare(
        &mut self,
        device: &PipelineDevice,
        encoder: &mut crate::submission::CommandEncoder,
        inputs: (SceneView<'_>, &[DabBatch],Option<&layer_render::FillPreview>),
        extent: [u32; 2],
        reset: bool,
        selections: &mut selection_clip::SelectionClip,
    ) -> Result<(), GpuRasterError> {
        self.prepare_regions(device, encoder, inputs, extent, reset, selections, None)
    }
    /// Prepare only the mask-local pages needed by an isolated region capture.
    /// Existing GPU polygon/pixel coverage rules are shared with live editing.
    #[expect(clippy::too_many_arguments, reason = "Mask preparation borrows device, inputs, selection storage, and dirty regions independently")]
    pub fn prepare_regions(
        &mut self,
        device: &PipelineDevice,
        encoder: &mut crate::submission::CommandEncoder,
        inputs: (SceneView<'_>, &[DabBatch],Option<&layer_render::FillPreview>),
        _extent: [u32; 2],
        reset: bool,
        selections: &mut selection_clip::SelectionClip,
        regions: Option<&std::collections::HashMap<SourceTarget, PixelRect>>,
    ) -> Result<(), GpuRasterError> {
        let (scene,batches,fill)=inputs;
        self.pages.extend(mem::take(&mut self.preview_bases));
        if reset {
            self.pages.clear();
            self.command_pages.clear();
        }
        let commands: BTreeMap<_, _> = batches.iter().filter_map(|batch| {
            let DabBatchKind::RasterOperation(index) = batch.kind else { return None; };
            let operation = scene.operations(batch.target)?.get(index as usize)?;
            Some(((batch.target, index, false), operation))
        }).chain(fill.map(|fill|((fill.target,u32::MAX,false),&fill.operation))).collect();
        let coverages: BTreeMap<_, _> = commands.iter().flat_map(|(key, operation)| {
            std::iter::once((*key, operation.coverage.clone())).chain(underfill_coverage(operation).map(|coverage| ((key.0, key.1, true), coverage)))
        }).collect();
        let same = |stored: Option<&layer_core::CoverageSnapshot>, coverage: &layer_core::CoverageSnapshot|
            stored.is_some_and(|stored| stored.source == coverage.source && stored.selection == coverage.selection);
        self.command_pages.retain(|(key, _), _| coverages.get(key)
            .is_some_and(|coverage| same(self.command_definitions.get(key), coverage)));
        for (key, coverage) in &coverages {
            let source = &coverage.source;
            let authored = SourceTarget::Coverage(coverage.use_.source);
            if !same(self.command_definitions.get(key), coverage) && coverage.selection.is_none() && self.definitions.get(&authored) == Some(source) {
                for ((target, coordinate), page) in &self.pages {
                    if *target == authored { self.command_pages.insert((*key, *coordinate), page.clone()); }
                }
            }
            let mut needed:std::collections::BTreeSet<_> = coverage.selection.as_ref().map_or_else(Default::default,
                |selection| page_coordinates(pixel_rect(selection.bounds(), source.domain)).collect());
            if key.2 && source.default_coverage==0. && coverage.use_.offset==[0;2] && !coverage.use_.inverted
                && let Some(selection)=&coverage.selection && !selection.inverted && selection.affine==layer_core::Affine::IDENTITY
                && let layer_core::SelectionShape::Pixels(pixels)=&selection.shape {
                needed.retain(|coordinate| {let page=page_rect(*coordinate);
                    pixels.coverage_bounds_in([page.min_x(),page.min_y(),page.max_x(),page.max_y()])!=[0;4]});
            }
            initialize_pages(&mut self.command_pages, *key, source, coverage.selection.as_ref(), needed, regions.is_some(),
                device, encoder, selections, &self.pipelines.initialize, &self.pipelines.init_layout, &self.pipelines.empty_selection)?;
        }
        self.snapshots.retain(|key, _| commands.get(key).is_some_and(|operation| matches!(operation.kind,
            layer_core::RasterOperationKind::Bake { .. } | layer_core::RasterOperationKind::FrequencyDetail { .. })));
        for (key, operation) in &commands {
            let (snapshot, scope) = match &operation.kind {
                layer_core::RasterOperationKind::Bake { scene, scope, .. }
                | layer_core::RasterOperationKind::FrequencyDetail { scene, scope, .. } => (scene, scope),
                _ => continue,
            };
            let retained = snapshot.view().with_scope(scope);
            let definitions: BTreeMap<_, _> = retained.artwork().coverage.iter().filter(|(handle, _, _)| {
                retained.source_owner(SourceTarget::Coverage(*handle)).is_some_and(|owner| retained.visible(owner))
            }).map(|(handle, _, source)| (SourceTarget::Coverage(handle), source.clone())).collect();
            let stored = self.snapshots.entry(*key).or_default();
            stored.pages.retain(|(target, _), _| stored.definitions.get(target) == definitions.get(target)
                && definitions.contains_key(target));
            for (&target, source) in &definitions {
                if stored.definitions.get(&target) != Some(source) && self.definitions.get(&target) == Some(source) {
                    for ((id, coordinate), page) in &self.pages {
                        if *id == target { stored.pages.insert((target, *coordinate), page.clone()); }
                    }
                }
            }
            stored.definitions = definitions;
        }
        self.command_definitions = coverages;
        let definitions: BTreeMap<_, _> = scene.artwork().coverage.iter()
            .filter(|(h, _, _)| scene.source_owner(SourceTarget::Coverage(*h)).is_some())
            .map(|(h, _, source)| (SourceTarget::Coverage(h), source.clone())).collect();
        self.pages.retain(|(id, coordinate), _| {
            definitions.contains_key(id)
                && regions.is_none_or(|regions| {
                    regions.get(id).is_some_and(|region| !page_rect(*coordinate).intersect(*region).is_empty())
                })
        });
        self.definitions = definitions;
        for (&target, mask) in &self.definitions {
            let extent = mask.domain;
            let mut needed = std::collections::BTreeSet::new();
            let transforms = |batch: &DabBatch| {
                matches!(batch.kind, DabBatchKind::RasterOperation(index)
                    if mask.operations.get(index as usize)
                        .is_some_and(|op| matches!(op.kind, layer_core::RasterOperationKind::Transform(_))))
            };
            for batch in batches.iter().filter(|b| b.target == target && !transforms(b)) {
                needed.extend(page_coordinates(batch_pixel_rect(batch, extent)));
            }
            initialize_pages(&mut self.pages, target, mask, None, needed, regions.is_some(),
                device, encoder, selections, &self.pipelines.initialize, &self.pipelines.init_layout, &self.pipelines.empty_selection)?;
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn initialize_pages<K: Copy + Ord>(
    pages: &mut BTreeMap<(K, [u32; 2]), MaskPage>, target: K,
    source: &layer_core::CoverageSource, selection: Option<&layer_core::Selection>, needed: std::collections::BTreeSet<[u32; 2]>, bounded: bool,
    device: &PipelineDevice, encoder: &mut crate::submission::CommandEncoder,
    selections: &mut selection_clip::SelectionClip, initialize: &Deferred<wgpu::RenderPipeline>,
    init_layout: &wgpu::BindGroupLayout, empty_selection: &wgpu::Buffer,
) -> Result<(), GpuRasterError> {
    let extent = source.domain;
    let missing: Vec<_> = needed
        .into_iter()
        .filter(|c| !pages.contains_key(&(target, *c)))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    if let Some(selection) = selection {
        let region = bounded.then(|| {
            missing
                .iter()
                .fold(PixelRect::EMPTY, |region, c| region.union(page_rect(*c)))
                .intersect(PixelRect::full(extent))
        });
        selections.prepare_region(
            device,
            encoder,
            extent,
            &std::sync::Arc::new(selection.clone()),
            region,
        )?;
    }
    let coverage = if selection.is_some() {
        selections.buffer.as_ref().unwrap()
    } else {
        empty_selection
    };
    for coordinate in missing {
        let MaskPage { texture, view } = MaskPage::new(device);
        let data = [
            coordinate[0] as f32 * PAGE_SIZE as f32,
            coordinate[1] as f32 * PAGE_SIZE as f32,
            f32::from(selection.is_some()),
            source.default_coverage,
        ];
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mask page initialization"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let binding = crate::bindings::group(device, "mask initial coverage", init_layout, [
            buffer.as_entire_binding(),
            coverage.as_entire_binding(),
        ]);
        let mut pass = encoder.color_pass(
            "mask initialize page",
            &view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        pass.set_pipeline(initialize);
        pass.set_bind_group(0, &binding, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        pages
            .insert((target, coordinate), MaskPage { texture, view });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer_tests::placement;

    #[test]
    fn background_underfill_keeps_only_band_pages_and_matches_explicit_zero_pages() {
        let extent=[768;2];
        let selection=|contains:fn(u32,u32)->bool| {
            let mut words=vec![0;extent[0] as usize*extent[1] as usize/4];
            for y in 0..extent[1] {for x in 0..extent[0] {if contains(x,y) {
                words[((y*extent[0]+x)/4) as usize]|=255<<((x%4)*8);
            }}}
            layer_core::Selection::pixels(Arc::new(layer_core::SelectionPixels::bytes(extent,[0,0,768,768],words).unwrap()))
        };
        let background=selection(|_,_|true);
        let band=selection(|x,y|(x==128 || x==640) && (128..=640).contains(&y)
            || (y==128 || y==640) && (128..=640).contains(&x));
        let mut results=Vec::new();
        for explicit_zero in [false,true] {
            let mut document=placement::paint_document(extent,"Background band");
            let target=placement::target(&document);
            placement::set_source(&mut document,layer_core::color::source::rgba8_source(extent,|_,_|[128,64,32,128]));
            let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            crate::test_support::submit_ready(&mut r,FramePacket {reset_layers:true,..crate::test_support::packet(document.scene(),extent)}).unwrap();
            let mut coverage=layer_core::CoverageSnapshot::reveal_all(document.artwork.coverage.next_handle(),extent,[0;2]);
            coverage.source.default_coverage=0.;coverage.selection=Some(background.clone());
            let operation=layer_core::RasterOperation {placement:layer_core::Affine::IDENTITY,coverage,
                kind:layer_core::RasterOperationKind::Fill {color:[0.1,0.4,0.7,1.],texture:Default::default(),alpha_locked:false,
                    edges:None,underfill:Some(band.clone())}};
            let batch=DabBatch {kind:DabBatchKind::RasterOperation(0),dab_count:0,damage:operation.bounds(extent),
                ..crate::test_support::dab_batch(target,crate::tests::test_style(BrushExecution::Dry),layer_core::Rect::from_extent(extent))};
            placement::paint_mut(&mut document).operations=Arc::new(vec![operation]);
            let packet=FramePacket {dab_batches:std::slice::from_ref(&batch),..crate::test_support::packet(document.scene(),extent)};
            r.layer_masks.pipelines.initialize.compile();
            let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
            r.layer_masks.prepare(&r.device,&mut encoder,(packet.scene,packet.dab_batches,None),extent,false,&mut r.selection_clip).unwrap();
            assert_eq!(r.layer_masks.command_pages.keys().filter(|(key,_)|!key.2).count(),9);
            assert_eq!(r.layer_masks.command_pages.keys().filter(|(key,_)|key.2).count(),8);
            assert!(!r.layer_masks.command_pages.contains_key(&((target,0,true),[1,1])));
            if explicit_zero {
                let page=MaskPage::new(&r.device);
                drop(encoder.color_pass("explicit empty underfill",&page.view,wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)));
                r.layer_masks.command_pages.insert(((target,0,true),[1,1]),page);
            }
            r.uploads.finish(&encoder);encoder.submit(&r.queue);
            crate::test_support::submit_ready(&mut r,packet).unwrap();
            let pixels=r.readback_srgb_rgba8().unwrap();
            let center=((384*extent[0]+384)*4) as usize;
            let edge=((384*extent[0]+128)*4) as usize;
            assert_eq!(pixels[center+3],255);
            assert_eq!(pixels[((384*extent[0]+128)*4+3) as usize],255);
            assert_ne!(&pixels[center..center+4],&pixels[edge..edge+4]);
            results.push(pixels);
        }
        assert_eq!(results[0],results[1]);
    }
}

impl WgpuRasterizer {
    pub(super) fn encode_mask_dabs(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        packet: FramePacket<'_>,
        batches: &[DabBatch],
        committed: &[(SourceTarget, u32)],
    ) -> Result<(), GpuRasterError> {
        let scene = packet.scene;
        for (index, batch) in batches
            .iter()
            .enumerate()
            .filter(|(_, b)| MaskRenderer::is_mask(scene, b.target))
        {
            if let DabBatchKind::RasterOperation(op) = batch.kind {
                if !committed.contains(&(batch.target, op)) {
                    let operation = &scene.operations(batch.target)
                        .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?
                        [op as usize];
                    if operation.kind == layer_core::RasterOperationKind::Coverage {
                        for coordinate in page_coordinates(batch_pixel_rect(batch, self.target_extent(batch.target))) {
                            if let Some(page) = self.layer_masks.command_pages.get(&((batch.target, op, false), coordinate)).cloned() {
                                self.layer_masks.pages.insert((batch.target, coordinate), page);
                            }
                        }
                        continue;
                    }
                    if !matches!(operation.kind, layer_core::RasterOperationKind::Transform(_)) {
                        let mut compositor = self.scene.take().map(Ok).unwrap_or_else(|| scene::Scene::new(self))?;
                        let damage = batch_pixel_rect(batch, self.target_extent(batch.target));
                        let result = compositor.apply_operation(self, packet, batch.target, scene::PaintOperation::Stored(op as usize), damage, encoder);
                        self.scene = Some(compositor);
                        result?;
                        continue;
                    }
                    let mut transforms = self.transforms.take().unwrap();
                    let result = transforms.apply(
                        self,
                        encoder,
                        batch.target,
                        operation,
                    );
                    self.transforms = Some(transforms);
                    result?;
                }
                continue;
            }
            if batch.dab_count == 0 {
                continue;
            }
            self.prepare_target_selection(encoder, &batch.style, self.target_extent(batch.target))?;
            let damage = batch_pixel_rect(batch, self.target_extent(batch.target));
            for coordinate in page_coordinates(damage) {
                let key = (batch.target, coordinate);
                if batch.kind == DabBatchKind::Preview && !self.layer_masks.preview_bases.contains_key(&key)
                    && let Some(base) = self.layer_masks.pages.get(&key).cloned()
                {
                    let preview = MaskPage::new(&self.device);
                    encoder.copy_texture_to_texture(base.texture.as_image_copy(), preview.texture.as_image_copy(),
                        wgpu::Extent3d { width: PAGE_SIZE, height: PAGE_SIZE, depth_or_array_layers: 1 });
                    self.layer_masks.preview_bases.insert(key, base);
                    self.layer_masks.pages.insert(key, preview);
                }
                let Some(page) = self.layer_masks.pages.get(&(batch.target, coordinate)) else {
                    continue;
                };
                let dry = batch.style.contact.is_some_and(|c| c.dry.is_some());
                let pipeline = &self.layer_masks.brush[MaskRenderer::brush_index(&batch.style)];
                let source = self.device.portable_blend().then(|| self.portable_blend.source(&self.device,&page.view,wgpu::TextureFormat::Rgba32Float));
                let count = if source.is_some() { batch.dab_count } else { 1 };
                for dab in 0..count {
                let mut pass = encoder.color_pass(
                    "incremental visibility mask brush",
                    source.as_ref().unwrap_or(&page.view),
                    if source.is_some() { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load },
                );
                let local = damage
                    .intersect(page_rect(coordinate))
                    .page_local(coordinate);
                pass.set_scissor_rect(local.min_x(), local.min_y(), local.width(), local.height());
                pass.set_pipeline(pipeline);
                pass.set_bind_group(
                    0,
                    &self.style_bind_group,
                    &[index as u32 * self.style_stride as u32],
                );
                pass.set_bind_group(
                    1,
                    self.paint_target_binding(&batch.style),
                    &[self.layer_target_offset(batch.target, coordinate)],
                );
                if dry {
                    let key = Self::texture_set_key(&batch.style);
                    pass.set_bind_group(2, &self.texture_sets.iter().find(|s| s.key == key)
                        .expect("mask contact textures are prepared before encoding").bind_group, &[]);
                } else if let BrushTip::Mask(id) = &batch.style.tip {
                    pass.set_bind_group(2, &self.mask(id)?.bind_group, &[]);
                }
                let start = batch.first_dab as u64 * mem::size_of::<DabGpu>() as u64;
                pass.set_vertex_buffer(
                    0,
                    self.dab_buffer.slice(
                        start..start + batch.dab_count as u64 * mem::size_of::<DabGpu>() as u64,
                    ),
                );
                pass.draw(0..4, if source.is_some() { dab..dab+1 } else { 0..batch.dab_count });
                drop(pass);
                if let Some(source) = &source {
                    self.portable_blend.apply(&self.device,encoder,source,&page.view,local,u32::from(batch.style.mode == DabMode::Erase));
                }
                }
            }
        }
        Ok(())
    }
}
