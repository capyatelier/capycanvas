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
pub(super) type CommandCoverage = (SourceTarget, u32);

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
    command_definitions: BTreeMap<CommandCoverage, layer_core::CoverageSnapshot>,
    pub snapshots: BTreeMap<CommandCoverage, SnapshotMasks>,
    pub(super) brush: [Deferred<wgpu::RenderPipeline>; 4],
    pub(super) initialize: Deferred<wgpu::RenderPipeline>,
    init_layout: wgpu::BindGroupLayout,
    empty_selection: wgpu::Buffer,
}
impl MaskRenderer {
    pub fn new(
        device: &PipelineDevice,
        style: &wgpu::BindGroupLayout,
        target: &wgpu::BindGroupLayout,
        texture: &wgpu::BindGroupLayout,
    ) -> Self {
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
                shader.clone(),
            );
            Deferred::pipeline(move |mode| {
                brush_pipeline_format_recipe(
                    mode,
                    &device,
                    if i < 2 { &analytic } else { &tip },
                    &shader,
                    if i < 2 {
                        "analytic_fragment"
                    } else {
                        "mask_fragment"
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
            definitions: BTreeMap::new(),
            pages: BTreeMap::new(),
            command_pages: BTreeMap::new(),
            command_definitions: BTreeMap::new(),
            snapshots: BTreeMap::new(),
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
        inputs: (SceneView<'_>, &[DabBatch]),
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
        inputs: (SceneView<'_>, &[DabBatch]),
        _extent: [u32; 2],
        reset: bool,
        selections: &mut selection_clip::SelectionClip,
        regions: Option<&std::collections::HashMap<SourceTarget, PixelRect>>,
    ) -> Result<(), GpuRasterError> {
        let (scene, batches) = inputs;
        if reset {
            self.pages.clear();
            self.command_pages.clear();
        }
        let commands: BTreeMap<_, _> = batches.iter().filter_map(|batch| {
            let DabBatchKind::RasterOperation(index) = batch.kind else { return None; };
            let operation = scene.operations(batch.target)?.get(index as usize)?;
            Some(((batch.target, index), operation))
        }).collect();
        let same = |stored: Option<&layer_core::CoverageSnapshot>, coverage: &layer_core::CoverageSnapshot|
            stored.is_some_and(|stored| stored.source == coverage.source && stored.selection == coverage.selection);
        self.command_pages.retain(|(key, _), _| commands.get(key)
            .is_some_and(|operation| same(self.command_definitions.get(key), &operation.coverage)));
        for (key, operation) in &commands {
            let coverage = &operation.coverage;
            let source = &coverage.source;
            let authored = SourceTarget::Coverage(coverage.use_.source);
            if !same(self.command_definitions.get(key), coverage) && coverage.selection.is_none() && self.definitions.get(&authored) == Some(source) {
                for ((target, coordinate), page) in &self.pages {
                    if *target == authored { self.command_pages.insert((*key, *coordinate), page.clone()); }
                }
            }
            let needed = coverage.selection.as_ref().map_or_else(Default::default,
                |selection| page_coordinates(pixel_rect(selection.bounds(), source.domain)).collect());
            initialize_pages(&mut self.command_pages, *key, source, coverage.selection.as_ref(), needed, regions.is_some(),
                device, encoder, selections, &self.initialize, &self.init_layout, &self.empty_selection)?;
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
        self.command_definitions = commands.into_iter().map(|(key, operation)| (key, operation.coverage.clone())).collect();
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
                device, encoder, selections, &self.initialize, &self.init_layout, &self.empty_selection)?;
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

impl WgpuRasterizer {
    pub(super) fn encode_mask_dabs(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        scene: SceneView<'_>,
        batches: &[DabBatch],
        committed: &[(SourceTarget, u32)],
    ) -> Result<(), GpuRasterError> {
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
                            if let Some(page) = self.layer_masks.command_pages.get(&((batch.target, op), coordinate)).cloned() {
                                self.layer_masks.pages.insert((batch.target, coordinate), page);
                            }
                        }
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
                let Some(page) = self.layer_masks.pages.get(&(batch.target, coordinate)) else {
                    continue;
                };
                let texture_tip = matches!(batch.style.tip, BrushTip::Mask(_));
                let pipeline = &self.layer_masks.brush[usize::from(texture_tip) * 2
                    + usize::from(batch.style.mode == DabMode::Erase)];
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
                if let BrushTip::Mask(id) = &batch.style.tip {
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
