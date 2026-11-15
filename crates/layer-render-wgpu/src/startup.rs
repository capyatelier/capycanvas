//! Cold-start dependency ordering shared by native and browser compilers. The compiler owns only cloned GPU
//! handles and recipes, never the session, swapchain, input queue or live pixels.
use super::*;
use layer_core::{BrushSnapshot, Document, StrokeTool};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

pub(super) use crate::compile::{Class, Needs};
pub(super) use Class::{Required as DOCUMENT, Required as BRUSH, Validation as VALIDATION, Visible as OTHER, Anticipated as ANTICIPATED, Background as BACKGROUND};
#[cfg(not(target_arch = "wasm32"))]
pub use crate::compile::{Activity as ShaderActivity, finish_shader_compiler_shutdown, stop_shader_compilers};

#[cfg(all(feature = "test-support", not(target_arch = "wasm32")))]
pub fn enqueue_compiler_failure(renderer: &WgpuRasterizer, cause: &'static str) -> mpsc::Receiver<()> {
    let startup = renderer.startup.as_ref().expect("staged renderer");
    assert!(startup.finished, "startup must complete before the late failure");
    let (recorded, result) = mpsc::channel();
    startup.compiler.enqueue(BRUSH, move || Err(cause.into()));
    startup.compiler.enqueue(BRUSH, move || { let _ = recorded.send(()); Ok(()) });
    result
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartupProgress {
    pub canvas_ready: bool,
    pub brush_ready: bool,
    pub complete: bool,
}
impl StartupProgress {
    pub const COMPLETE: Self = Self { canvas_ready: true, brush_ready: true, complete: true };
}
/// Only changes that can require different pipelines invalidate readiness.
/// Ordinary raster/parameter edits reuse their prepared dependencies. The
/// revision memo avoids scanning layers on unchanged display callbacks.
#[derive(Debug)]
pub struct ShaderDocument {
    id: layer_core::authored::PortableId,
    revision: std::sync::atomic::AtomicU64,
    key: DocumentKey,
}
#[derive(Debug, PartialEq)]
struct DocumentKey {
    extent: [u32; 2],
    color: layer_core::color::DocumentColor,
    selection: bool,
    mask: bool,
    composition: [bool; 2],
    locked: bool,
    sources: SourceKinds,
    objects: bool,
    integer_preview: bool,
    vectors: bool,
    operations: bool,
    transform: bool,
    mesh: bool,
    watercolor: bool,
    gray: bool,
    blend_space: layer_core::BlendSpace,
    chains: Vec<(Vec<Arc<layer_core::EffectProgram>>, effects::Execution, bool, bool)>,
}
impl DocumentKey {
    fn new(document: &Document) -> Self {
        let scene = document.scene();
        let target = document.drawing_target().or(document.working.target);
        let mask = target.is_some_and(SourceTarget::is_coverage);
        let operations = || scene.targets().filter_map(|t| scene.operations(t)).flatten();
        let objects=scene.order().iter().any(|&h|scene.object_layer(h).is_some());
        Self {
            extent: document.composition().size, color: document.composition().color,
            selection: document.working.selection.is_some(), mask,
            composition: scene::composition_pipelines(scene),
            locked: !mask && target.and_then(|t| document.target_owner(t)).and_then(|h| scene.occurrence(h)).is_some_and(|o| o.alpha_locked),
            sources: SourceKinds::for_scene(scene),
            objects,
            integer_preview: integer_object_preview(scene),
            vectors: scene.order().iter().any(|&h| scene.vector_layer(h).is_some()),
            operations: scene.order().iter().any(|&h| scene.mask(h).is_some()) || operations().next().is_some(),
            transform: operations().any(|op| matches!(op.kind, layer_core::RasterOperationKind::Transform(_))),
            mesh: operations().any(|op| matches!(&op.kind, layer_core::RasterOperationKind::Transform(t) if t.placement.mesh.is_some())),
            gray: scene.targets().any(|t|scene.color_mode(t)!=layer_core::color::LayerColorMode::FullColor),
            watercolor: has_watercolor(scene),
            blend_space: document.composition().blend,
            chains: scene::effect_chains(scene,scene::EffectPreparation::Display).into_iter().map(|(handles, execution)| {
                let identity=effects::identity_chain(scene,&handles);
                let normal=effects::normal_chain(scene,&handles);
                (handles.into_iter().filter_map(|h| {
                    let application = scene.effect_application(h)?;
                    Some(application.program.clone())
                }).collect(), execution, identity, normal)
            }).collect(),
        }
    }
}
fn integer_object_preview(scene: SceneView<'_>) -> bool {
    scene.order().iter().filter_map(|&handle| scene.object_layer(handle)).any(|object|
        object.interpolation == layer_core::authored::ImageInterpolation::Nearest
            && object_image_mips::MovingImages::source_format(object.image.storage()) == wgpu::TextureFormat::Rg32Uint)
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceKinds { float: bool, integer: bool }
impl SourceKinds {
    pub(crate) fn for_depth(depth: layer_core::color::SampleDepth) -> Self {
        let float = depth == layer_core::color::SampleDepth::U8;
        Self { float, integer: !float }
    }
    pub(crate) fn for_descriptor(descriptor: layer_core::color::PixelDescriptor) -> Self {
        if descriptor.channels == 1 { Self { float: false, integer: true } } else { Self::for_depth(descriptor.depth()) }
    }
    pub(crate) fn for_source(source: &layer_core::color::source::SourceImage) -> Self {
        if matches!(source.interpretation.profile, layer_core::color::ColorProfile::Builtin(_))
            && source.interpretation.channels != layer_core::color::source::SourceChannels::Cmyk {
            Self::for_depth(source.interpretation.depth)
        } else { Self::default() }
    }
    pub(crate) fn include(&mut self, other: Self) { self.float |= other.float; self.integer |= other.integer; }
    pub(crate) fn for_scene(scene: SceneView<'_>) -> Self {
        let mut kinds = Self::default();
        for target in scene.targets() {
            if let Some(source) = scene.original(target) { kinds.include(Self::for_source(source)); }
            if let Some(raster) = scene.raster(target) {
                let data = raster.try_data();
                if matches!(data, Some(Err(_))) { continue; }
                let plane=if target.is_coverage() {layer_core::raster::RasterPlane::Mask} else {layer_core::raster::RasterPlane::Color};
                kinds.include(Self::for_descriptor(plane.descriptor_for(scene.composition().color,scene.color_mode(target))));
                if let Some(Ok(data)) = data {
                    for plane in [layer_core::raster::RasterPlane::Color,layer_core::raster::RasterPlane::Mask,layer_core::raster::RasterPlane::WatercolorWetness] {
                        let low=layer_core::raster::TileKey {plane,coordinate:[0;2]};
                        let high=layer_core::raster::TileKey {plane,coordinate:[u32::MAX;2]};
                        if let Some((_,tile))=data.tiles.range(low..=high).next() { kinds.include(Self::for_descriptor(tile.descriptor())); }
                    }
                }
            }
        }
        for source in scene.order().iter().filter_map(|&handle| scene.object_layer(handle)) {
            kinds.include(Self::for_source(source.image.storage()));
        }
        kinds
    }
    pub(crate) fn pipelines(self, source: &scene::sources::Pipelines) -> impl Iterator<Item = &Deferred<wgpu::RenderPipeline>> {
        [(self.float, &source.pipeline), (self.integer, &source.integer)].into_iter().filter_map(|(used, pipeline)| used.then_some(pipeline))
    }
}
pub(super) fn has_watercolor(scene: SceneView<'_>) -> bool {
    scene.targets().any(|target| scene.raster(target).is_some_and(|raster|
        raster.try_data().is_none_or(|data| data.is_ok_and(|data| data.watercolor.is_some()))))
}
impl ShaderDocument {
    pub fn new(document: &Document) -> Self {
        Self { id: document.artwork.id, revision: std::sync::atomic::AtomicU64::new(document.revision), key: DocumentKey::new(document) }
    }
    pub fn matches(&self, document: &Document) -> bool {
        if self.id != document.artwork.id { return false; }
        if self.revision.load(std::sync::atomic::Ordering::Relaxed) == document.revision { return true; }
        let mut key = DocumentKey::new(document);
        if key.composition.iter().zip(self.key.composition).any(|(required, prepared)| *required && !prepared) { return false; }
        key.composition = self.key.composition;
        if self.key != key { return false; }
        self.revision.store(document.revision, std::sync::atomic::Ordering::Relaxed);
        true
    }
}
impl layer_render::PreparationKey for ShaderDocument {
    fn matches(&self, document: &Document) -> bool { ShaderDocument::matches(self, document) }
    fn equivalent(&self, other: &dyn layer_render::PreparationKey) -> bool {
        (other as &dyn std::any::Any).downcast_ref::<Self>()
            .is_some_and(|other| self.id == other.id && self.key == other.key)
    }
}

impl Needs {
    pub(super) fn brush(&mut self, r: &WgpuRasterizer, document: &Document, brush: &BrushSnapshot, selection: Option<Arc<layer_core::Selection>>) {
        let mask = document.drawing_target().or(document.working.target).is_some_and(SourceTarget::is_coverage);
        let selection = selection.or_else(|| document.working.selection.clone().map(Arc::new));
        for tool in [StrokeTool::Brush, StrokeTool::Eraser] {
            self.style(r, &layer_render::DabStyle {selection: selection.clone(), ..brush_style(document, brush, tool)}, dry_material::Reach::of(brush), mask, true);
        }
    }
    pub(super) fn sizes(&mut self, r: &WgpuRasterizer, document: &Document, brush: &BrushSnapshot, selection: Option<Arc<layer_core::Selection>>) {
        if document.drawing_target().or(document.working.target).is_some_and(SourceTarget::is_coverage) { return; }
        let selection = selection.or_else(|| document.working.selection.clone().map(Arc::new));
        let reach = dry_material::Reach::of(brush);
        for tool in [StrokeTool::Brush, StrokeTool::Eraser] {
            let style = layer_render::DabStyle {selection: selection.clone(), ..brush_style(document, brush, tool)};
            let plan = BrushPassPlan::for_device(&style, &r.device);
            if pointwise(&style) && plan.direct.is_none() {
                for extra in reach.any_size().batch_variants(&style).filter(|&extra| !reach.required_variants(&style).any(|required| required == extra)) {
                    for coverage in [Some(plan.state.coverage), plan.state.coverage.then_some(false)].into_iter().flatten() {
                        self.compute.push(r.pipelines.dry_material.selected_kernel(&style, plan.material, coverage, extra).clone());
                    }
                }
            }
        }
    }
    pub(super) fn transform(&mut self, r: &WgpuRasterizer) {
        let transforms = r.transforms.as_ref().unwrap();
        self.render.extend(transforms.pipelines());
        self.compute.extend(transforms.display_pipelines());
        self.compute.extend(transforms.linear_affine_placement_pipelines());
        self.render.extend([4, 5].map(|i| r.scene_pipelines.resample.mesh[i].clone()));
        self.compute.extend(r.selection_clip.pipelines().into_iter().cloned());
    }
    pub(super) fn selection_paint(&mut self, r: &WgpuRasterizer) {
        self.compute.extend(r.recipes.selection_paint.pipelines.iter().chain(r.selection_clip.pipelines()).cloned());
    }
    pub(super) fn fill(&mut self, r: &WgpuRasterizer, scene: Option<SceneView<'_>>) {
        if let Some(scene) = scene {
            let required = Self::capture_scene(r, scene, true);
            self.render.extend(required.render);
            self.compute.extend(required.compute);
        }
        self.render.extend(r.scene_pipelines.fill_pipelines(&r.device).into_iter().cloned());
        self.render.push(r.layer_masks.initialize.clone());
        self.compute.extend(r.selection_clip.pipelines().into_iter().cloned());
        self.compute.push(r.selection_clip.resample.clone());
    }
    pub(super) fn regions(&mut self, r: &mut WgpuRasterizer, refinement: layer_render::RegionRefinement, enclosed: bool, contiguous: bool, prepare_ends: bool) {
        if r.regions.is_none() { r.regions = Some(region_requests::RegionRequests::new(r)); }
        let regions = r.regions.as_ref().unwrap();
        self.compute.extend(regions.raw.pipelines().chain(regions.flood.request_pipelines(refinement, enclosed, contiguous, prepare_ends)).cloned());
    }
    pub(super) fn operation(&mut self, r: &WgpuRasterizer, operation: layer_render::OperationUse) {
        self.render.push(r.scene_pipelines.pipeline[scene::operation_index(operation)].clone());
    }
    fn uses(&mut self, r: &WgpuRasterizer, document: &Document, uses: layer_render::PreparationUses) {
        if uses.transform { self.transform(r); }
        if uses.selection_paint { self.selection_paint(r); }
        if uses.fill { self.fill(r, Some(document.scene())); }
        if let Some(operation) = uses.operation { self.operation(r, operation); }
    }
    fn filter_apply(r: &WgpuRasterizer, filter: &layer_render::FilterPreparation) -> Result<Self, GpuRasterError> {
        let Some((scene, handle)) = &filter.scene else { return Ok(Self::default()); };
        let view = scene.view();
        let composition = view.composition();
        let mut effects = r.scene_pipelines.effects(r);
        for (chain, execution) in scene::effect_chains(view, scene::EffectPreparation::Capture) {
            effects.prepare(r, view, &chain, execution, 0., 0, composition.blend)?;
        }
        if filter.program.kind == layer_core::EffectKind::Generator {
            effects.prepare(r, view, &[*handle], effects::Execution::Preview, 0., 0,
                scene::preview_space(view.effect(*handle).unwrap(), composition.blend))?;
        }
        let mut needs = Self::capture_scene(r, view, false);
        let prepared = effects.needs();
        needs.render.extend(prepared.render);
        needs.compute.extend(prepared.compute);
        match filter.program.analysis() {
            Some(layer_core::EffectAnalysisKind::LocalIllumination) => needs.compute.extend(crate::local_tone::pipelines(&r.device).pipelines.iter().cloned()),
            Some(layer_core::EffectAnalysisKind::Dehaze) => needs.compute.extend(crate::dehaze::pipelines(&r.device).pipelines.iter().cloned()),
            None => {}
        }
        Ok(needs)
    }
    pub(super) fn capture_scene(r:&WgpuRasterizer,view:SceneView<'_>,watercolor:bool)->Self {
        let mut required = Self::default();
        required.render.extend(r.scene_pipelines.for_scene(r,view,&[]).cloned());
        required.compute.push(r.scene_pipelines.constant.2.clone());
        required.compute.extend([r.recipes.display.reduce.clone(),r.recipes.display.fused_reduce.clone()]);
        required.compute.extend(r.scene_pipelines.scale_pipelines(wgpu::TextureFormat::Rgba32Float).into_iter().cloned());
        required.compute.extend(r.transforms.as_ref().unwrap().placement_pipelines());
        if r.device.portable_blend() { required.compute.extend(r.portable_blend.pipelines.iter().cloned()); }
        if watercolor {
            required.compute.push(r.pipelines.watercolor_compute.1.clone());
            required.render.push(r.pipelines.watercolor_composite.clone());
        }
        if view.order().iter().any(|&h| view.object_layer(h).is_some()) {
            required.compute.extend(r.scene_pipelines.objects.pipelines().into_iter().cloned());
            let mut sources=SourceKinds::default();
            for object in view.order().iter().filter_map(|&owner|view.object_layer(owner)) {
                sources.include(SourceKinds::for_source(object.image.storage()));
            }
            required.render.extend(sources.pipelines(&r.scene_pipelines.source).cloned());
            if integer_object_preview(view) { required.compute.push(r.scene_pipelines.objects.preview_integer.clone()); }
        }
        if view.order().iter().any(|&h| view.vector_layer(h).is_some()) {
            required.compute.push(r.scene_pipelines.vector.pipeline().clone());
        }
        required
    }
    pub(super) fn stroke(
        &mut self, r: &WgpuRasterizer, style: &layer_render::DabStyle,
        reach: dry_material::Reach, mask: bool, preview: bool,
    ) {
        if mask {
            self.render.push(r.layer_masks.brush[layer_masks::MaskRenderer::brush_index(style)].clone());
            return;
        }
        if style.execution == BrushExecution::Liquify {
            let pipelines = r.recipes.liquify.get_or_init(|| Arc::new(liquify::Pipelines::new(r)));
            self.compute.push(pipelines.update(style).clone());
            return;
        }
        let plan = BrushPassPlan::for_device(style, &r.device);
        if pointwise(style) && plan.direct.is_none() {
            let pipelines = &r.pipelines.dry_material;
            for extra in reach.required_variants(style) {
                self.compute.push(pipelines.selected_kernel(style, plan.material, plan.state.coverage, extra).clone());
                if preview && plan.state.coverage {
                    self.compute.push(pipelines.selected_kernel(style, plan.material, false, extra).clone());
                }
            }
        }
        if let Some(kind) = plan.direct {
            self.render.push(r.pipelines.direct[kind as usize].clone());
        } else if destination_material::transported(style) {
            self.compute.extend(destination_material::selected(plan, preview)
                .map(|entry| r.pipelines.destination_material.kernel(style, entry).clone()));
        }
    }
    pub(super) fn style(
        &mut self, r: &WgpuRasterizer, style: &layer_render::DabStyle,
        reach: dry_material::Reach, mask: bool, preview: bool,
    ) {
        self.stroke(r, style, reach, mask, preview);
        self.style_helpers(r, style, mask, preview);
    }
    fn style_helpers(&mut self, r: &WgpuRasterizer, style: &layer_render::DabStyle, mask: bool, preview: bool) {
        self.styles.push(style.clone());
        if style.selection.is_some() {
            self.compute.extend(r.selection_clip.pipelines().map(Clone::clone));
        }
        if mask { return; }
        if style.execution == BrushExecution::Liquify {
            let pipelines = r.recipes.liquify.get().unwrap();
            self.compute.extend([pipelines.reconstruct.clone(), pipelines.mips.reduce.clone(), pipelines.mips.fused_reduce.clone()]);
            return;
        }
        let plan = BrushPassPlan::for_device(style, &r.device);
        if pointwise(style) && plan.direct.is_none() {
            let pipelines = &r.pipelines.dry_material;
            if preview && pipelines.in_place() {
                self.compute.push(pipelines.copy_kernel(false).clone());
                if plan.state.coverage { self.compute.push(pipelines.copy_kernel(true).clone()); }
            } else if plan.material == MaterialOperation::Deposit && dry_material::dry_flow(style)
                && (preview || (!pipelines.in_place() && !plan.state.coverage)) {
                self.compute.push(pipelines.copy_kernel(false).clone());
            }
            if preview && style.rendering.blend_mode == BrushBlendMode::Normal {
                self.compute.push(pipelines.gather_kernel().clone());
            }
        }
        if plan.stroke_edge { self.render.push(r.pipelines.stroke_edge.clone()); }
        if plan.state.watercolor_wetness {
            self.compute.push(r.pipelines.watercolor_compute.1.clone());
            self.compute.extend(r.pipelines.watercolor_transport.iter().cloned());
            self.render.push(r.pipelines.watercolor_composite.clone());
        }
    }
}
pub(super) struct Startup {
    pub compiler: crate::compile::Scope,
    pub(super) masks: builtin_masks::Masks,
    pub(super) document_key: Option<Arc<ShaderDocument>>,
    brush: Option<ShaderBrushKey>,
    transform: bool,
    pub(super) document: Needs,
    pub(super) current: Needs,
    pub(super) strokes: Needs,
    papers: Vec<(layer_core::paper::PaperBinding, AssetId)>,
    effects: Option<(mpsc::Receiver<Result<effects::Effects, String>>, u64)>,
    effects_ready: bool,
    pub(super) host_catalog_pending: bool,
    pub(super) finished: bool,
    pub(super) brush_ready: bool,
}
#[derive(PartialEq, Eq)]
pub struct ShaderBrushKey {
    textures: TextureSetKey,
    mask_tip: bool,
    execution: BrushExecution,
    liquify: Option<LiquifyMode>,
    paint: BrushPassPlan,
    erase: BrushPassPlan,
    blend: BrushBlendMode,
    dry_material: u32,
    blender: destination_material::Blender,
    reach: dry_material::Reach,
}
fn brush_style(document: &Document, brush: &BrushSnapshot, tool: StrokeTool) -> layer_render::DabStyle {
    let mut style = layer_render::DabStyle::for_brush(brush, tool);
    let target = document.drawing_target().or(document.working.target);
    let mask = target.is_some_and(SourceTarget::is_coverage);
    style.alpha_locked = !mask && target.and_then(|t| document.target_owner(t)).and_then(|h| document.scene().occurrence(h)).is_some_and(|o| o.alpha_locked);
    style.blend_space = if mask { layer_core::BlendSpace::Linear } else { document.composition().blend };
    style.paper = style.grain.and(target).map(|target| document.paper_binding(target));
    style
}
impl ShaderBrushKey {
    pub fn new(document: &Document, brush: &BrushSnapshot) -> Self {
        let mut style = brush_style(document, brush, StrokeTool::Brush);
        let paint = BrushPassPlan::for_style(&style);
        style.mode = DabMode::Erase;
        let erase = BrushPassPlan::for_style(&style);
        Self {
            textures: WgpuRasterizer::texture_set_key(&style),
            mask_tip: matches!(style.tip, BrushTip::Mask(_)),
            execution: style.execution,
            liquify: (style.execution == BrushExecution::Liquify).then_some(style.deform.mode),
            paint,
            erase,
            blend: style.rendering.blend_mode,
            dry_material: dry_material::contact_flags(style.contact) | if style.rendering.accumulation == BrushAccumulation::Uniform { 128 } else { 0 },
            blender: if style.execution == BrushExecution::Smudge { destination_material::blender(&style) } else { destination_material::PLAIN },
            reach: dry_material::Reach::of(brush),
        }
    }
}
impl Startup {
    pub fn new(compiler: crate::compile::Scope) -> Result<Self, GpuRasterError> {
        Ok(Self {
            compiler,
            masks: builtin_masks::Masks::new(),
            document_key: None,
            brush: None,
            transform: false,
            document: Needs::default(),
            current: Needs::default(),
            strokes: Needs::default(),
            papers: Vec::new(),
            effects: None,
            effects_ready: false,
            host_catalog_pending: false,
            finished: false,
            brush_ready: false,
        })
    }
    /// Pipelines a selected tool draws with, compiled before pen-down.
    pub fn require_brush(&mut self, render: &[Deferred<wgpu::RenderPipeline>], compute: &[Deferred<wgpu::ComputePipeline>]) {
        self.compiler.require(render, BRUSH);
        self.compiler.require(compute, BRUSH);
        self.current.render.extend(render.iter().cloned());
        self.current.compute.extend(compute.iter().cloned());
    }
}
impl WgpuRasterizer {
    pub(crate) fn startup_outstanding(&self) -> Option<frame_demand::Outstanding> {
        let startup = self.startup.as_ref()
            .filter(|s| s.brush.is_some() && !s.finished && !(s.brush_ready && s.compiler.pending() == 0))?;
        Some(if startup.papers.iter().any(|(_, id)| self.papers.work_pending(id)) { frame_demand::Outstanding::Runnable }
            else { frame_demand::Outstanding::Blocked(frame_demand::Input::Compile) })
    }
    fn scene_scale_pipelines(&self,scene:SceneView<'_>)->impl Iterator<Item=&Deferred<wgpu::ComputePipeline>> {
        let format=self.display_precision(scene).format();
        let scale=&self.scene_pipelines.scale;
        self.scene_pipelines.scale_pipelines(format).into_iter()
            .chain((format!=wgpu::TextureFormat::Rgba32Float).then_some([&scale.full.reduce,&scale.output(format).project]).into_iter().flatten())
    }
    pub(super) fn scene_scale_ready(&self,scene:SceneView<'_>) -> bool {
        self.compilation.require(self.scene_scale_pipelines(scene).chain([
            &self.scene_pipelines.constant.2,&self.recipes.display.reduce,&self.recipes.display.fused_reduce]), DOCUMENT)
    }

    pub fn shader_input(&self) {
        self.memory.idle.store(false, std::sync::atomic::Ordering::Release);
        self.compilation.input();
    }
    pub fn shader_idle(&self, idle: bool, speculative_idle: bool) {
        self.memory.idle.store(idle, std::sync::atomic::Ordering::Release);
        self.compilation.idle(idle, speculative_idle);
    }
    pub fn shader_wait_ms(&self) -> f64 {
        self.compilation.delay().as_secs_f64() * 1000.
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn shader_activity(&self) -> Option<ShaderActivity> {
        Some(self.compilation.activity())
    }
    #[cfg(target_arch = "wasm32")]
    pub fn compile_startup_step(
        &self,
        allow_optional: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), GpuRasterError>>>> {
        self.compilation.step(allow_optional)
    }
    #[cfg(target_arch = "wasm32")]
    pub fn shader_work_pending(&self, allow_optional: bool) -> bool {
        self.compilation.has_work(allow_optional)
    }

    /// Complete the host's initial filter-catalog admission.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn finish_startup_catalog(&mut self) {
        if let Some(startup) = &mut self.startup {
            startup.host_catalog_pending = false;
        }
    }
    pub fn startup_needs_update(
        &self,
        document: &Document,
        brush: &BrushSnapshot,
        transform: bool,
    ) -> bool {
        self.startup.as_ref().is_some_and(|s| {
            s.document_key.as_ref().is_none_or(|key| !key.matches(document))
                || s.brush.as_ref() != Some(&ShaderBrushKey::new(document, brush))
                || s.transform != transform
        })
    }
    /// Prepares the document, selected brush and current transform before input.
    pub fn prepare_startup(
        &mut self,
        document: &Document,
        brush: &BrushSnapshot,
        transform: bool,
    ) -> Result<(), GpuRasterError> {
        self.activate_presentation(document.scene());
        let Some(mut startup) = self.startup.take() else {
            return Ok(());
        };
        let prepared = self.prepare_startup_into(&mut startup, document, brush, transform);
        self.startup = Some(startup);
        if prepared.is_ok() { self.declare_context(document, brush); }
        prepared
    }
    pub fn prepare_context(&mut self, document: &Document, brush: &BrushSnapshot, context: layer_render::PreparationContext) {
        self.preparation = context;
        self.declare_context(document, brush);
    }
    fn declare_context(&mut self, document: &Document, brush: &BrushSnapshot) {
        if self.startup.is_none() { return; }
        let generation = self.context_compilation.advance();
        let context = self.preparation.clone();
        let selection = if context.next.selection_strokes && document.working.selection.is_none() && !DocumentKey::new(document).mask {
            let [width, height] = document.composition().size.map(|n| n as f32);
            layer_core::Selection::polygon(vec![layer_core::Point {x:0.,y:0.}, layer_core::Point {x:width,y:0.}, layer_core::Point {x:width,y:height}, layer_core::Point {x:0.,y:height}]).ok().map(Arc::new)
        } else { None };
        let mut next = Needs::default();
        if let Some(selection) = &selection { next.brush(self, document, brush, Some(selection.clone())); }
        next.uses(self, document, context.next);
        let mut idle: [Needs; 6] = std::array::from_fn(|_| Needs::default());
        idle[0].sizes(self, document, brush, selection.clone());
        for shown in &context.brushes { idle[1].brush(self, document, shown, selection.clone()); }
        idle[2].transform(self);
        idle[3].selection_paint(self);
        idle[4].fill(self, Some(document.scene()));
        idle[5].regions(self, context.regions, false, true, true);
        next.warm(&self.context_compilation, ANTICIPATED, generation);
        for filter in context.filters {
            if let Ok(needs) = Needs::filter_apply(self, &filter) {
                needs.warm(&self.context_compilation, ANTICIPATED, generation);
            }
        }
        for family in idle { family.warm(&self.context_compilation, BACKGROUND, generation); }
    }
    fn prepare_startup_into(
        &mut self,
        startup: &mut Startup,
        document: &Document,
        brush: &BrushSnapshot,
        transform: bool,
    ) -> Result<(), GpuRasterError> {
        let _batch = startup.compiler.batch();
        startup.finished = false;
        startup.brush_ready = false;
        startup.compiler.advance();
        let styles = [StrokeTool::Brush, StrokeTool::Eraser].map(|tool| layer_render::DabStyle {
            selection: document.working.selection.clone().map(Arc::new),
            ..brush_style(document, brush, tool)
        });
        let mask = document.drawing_target().or(document.working.target).is_some_and(SourceTarget::is_coverage);
        let reach = dry_material::Reach::of(brush);
        let mut strokes = Needs::default();
        for style in &styles {
            strokes.stroke(self, style, reach, mask, true);
            strokes.require(&startup.compiler, BRUSH);
        }
        let mut current = Needs::default();
        // Native publication must be ready before accepting a brush contact,
        // but blank paper does not depend on writeback/promotion/validation.
        if let Some(native) = &self.native_edit {
            let gray = document.scene().targets().any(|target|
                document.scene().color_mode(target) != layer_core::color::LayerColorMode::FullColor);
            current.compute.extend(native.required_pipelines(document.composition().color.depth, gray).cloned());
        }
        for style in &styles {
            strokes.style_helpers(self, style, mask, true);
        }
        strokes.require(&startup.compiler, BRUSH);
        startup.papers.clear();
        for style in &styles {
            startup.masks.style(&startup.compiler, style, BRUSH);
            if let Some(binding) = style.grain.and(style.paper.as_ref()) {
                startup.papers.push((binding.clone(),self.papers.require(&self.device, Some(&startup.compiler), binding, BRUSH)?));
            }
        }
        // Live transforms do not change document revision or brush settings.
        // They still need their own shaders before an interactive frame runs.
        if transform {
            current.transform(self);
        }
        if let Some(retouch) = self.retouch.as_ref().filter(|r| r.prepared()) {
            let (render, compute) = retouch.pipelines();
            current.render.extend(render);
            current.compute.extend(compute);
        }
        current.require(&startup.compiler, BRUSH);
        if startup.document_key.as_ref().is_none_or(|key| !key.matches(document)) {
            let shader = ShaderDocument::new(document);
            let mut required = Needs::default();
            required
                .render
                .extend(self.scene_pipelines.for_scene(self,document.scene(),&[]).cloned());
            if self.device.portable_blend() { required.compute.extend(self.portable_blend.pipelines.iter().cloned()); }
            required.compute.push(self.scene_pipelines.constant.2.clone());
            if shader.key.watercolor {required.compute.push(self.pipelines.watercolor_compute.1.clone());}
            required.compute.extend(self.transforms.as_ref().unwrap().placement_pipelines());
            if self.native_edit.is_some() {
                let mip = self.display_pipelines.get_or_insert_with(|| self.recipes.display.clone());
                required.compute.extend([mip.reduce.clone(), mip.fused_reduce.clone()]);
                required.compute.extend(self.scene_scale_pipelines(document.scene()).cloned());
            }
            let mut sources = shader.key.sources;
            if self.native_edit.is_some() { sources.include(SourceKinds::for_depth(document.composition().color.depth)); }
            required.render.extend(sources.pipelines(&self.scene_pipelines.source).cloned());
            if shader.key.objects {
                required.compute.extend(self.scene_pipelines.objects.pipelines().map(Clone::clone));
                if shader.key.integer_preview { required.compute.push(self.scene_pipelines.objects.preview_integer.clone()); }
                required.compute.extend(self.moving_images.pipelines(&self.device).into_iter().cloned());
            }
            if shader.key.vectors {
                required.compute.extend(self.scene_pipelines.vector.pipelines().map(Clone::clone));
            }
            if shader.key.operations {
                required.render.push(self.layer_masks.initialize.clone());
                required.compute.extend(self.selection_clip.pipelines().map(Clone::clone));
            }
            if shader.key.mesh {
                required.render.extend(self.transforms.as_ref().unwrap().mesh_pipelines());
            }
            if shader.key.transform {
                required.render.extend(self.transforms.as_ref().unwrap().pipelines());
            }
            required.require(&startup.compiler, DOCUMENT);
            startup.document = required;
            startup.document_key = Some(Arc::new(shader));
            let chains = scene::effect_chains(document.scene(),scene::EffectPreparation::Display);
            let cached = self.scene.as_ref().map(|s| &s.effects).or(self.validated_effects.as_ref());
            let blend_space = document.composition().blend;
            if !chains.iter().all(|(layers, execution)| cached.is_some_and(|cache| cache.chain_ready(document.scene(), layers, *execution, blend_space))) {
                let mut candidate = self
                    .scene
                    .as_ref()
                    .map(|s| s.effects.fork())
                    .or_else(|| self.validated_effects.as_ref().map(effects::Effects::fork))
                    .unwrap_or_else(|| self.scene_pipelines.effects(self));
                let gpu = effects::Context { device: self.device.for_compilation() };
                let (tx, rx) = mpsc::channel();
                let snapshot = document.snapshot();
                let work = move || {
                    let result = (|| {
                        for (layers, execution) in chains {
                            candidate.prepare(
                                &gpu,
                                snapshot.view(),
                                &layers,
                                execution,
                                0.,
                                0,
                                blend_space,
                            )?;
                        }
                        Ok::<_, GpuRasterError>(())
                    })()
                    .map_err(|e| e.to_string());
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        candidate.compile();
                        let _ = tx.send(result.map(|()| candidate.fork()));
                        Ok(())
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        let compilation = candidate.compile_async();
                        Box::pin(async move {
                            let compiled = compilation.await;
                            let _ = tx.send(result.and(compiled).map(|()| candidate.fork()));
                            Ok(())
                        })
                            as std::pin::Pin<
                                Box<dyn std::future::Future<Output = Result<(), String>>>,
                            >
                    }
                };
                #[cfg(not(target_arch = "wasm32"))]
                let job = startup.compiler.enqueue(DOCUMENT, work);
                #[cfg(target_arch = "wasm32")]
                let job = startup.compiler.enqueue_async(DOCUMENT, work);
                startup.effects = Some((rx, job));
                startup.effects_ready = false;
            } else {
                startup.effects = None;
                startup.effects_ready = true;
            }
        }
        startup.current = current;
        startup.strokes = strokes;
        startup.brush = Some(ShaderBrushKey::new(document, brush));
        startup.transform = transform;
        Ok(())
    }
    pub fn poll_startup(&mut self) -> Result<StartupProgress, GpuRasterError> {
        if let Some(startup) = &mut self.startup {
            for (binding,id) in &mut startup.papers {
                *id=self.papers.require(&self.device,Some(&startup.compiler),binding,BRUSH)?;
            }
            let masks = startup.masks.take_ready()?;
            for (id, (width, height, pixels)) in masks {
                self.upload_mask(&id, width, height, width, &pixels)?;
            }
            let held = self.paper_overlay.as_ref().filter(|overlay| overlay.moving && paper::PaperTile::new(&overlay.paper.surface)
                .is_ok_and(|tile| self.papers.nearest_slope(&overlay.paper.surface, &tile).is_some())).map(|overlay| &overlay.paper.surface.texture);
            self.papers.advance(&self.device, &self.queue, false, held)?;
        }
        if self.startup.is_some() {
            let shaded: Vec<_> = self.paper_overlay.iter().map(|o| (o.paper.clone(), VALIDATION))
                .chain(self.paper_lighting.iter().map(|lit| (lit.binding.clone(), DOCUMENT))).collect();
            for (binding, priority) in shaded {
                self.require_paper_slope(&binding, priority)?;
            }
        }
        let Some(startup) = &mut self.startup else {
            return Ok(StartupProgress::COMPLETE);
        };
        startup.compiler.check()?;
        if let Some((rx, job)) = &startup.effects && startup.compiler.job_ready(*job) {
            match rx.try_recv() {
                Ok(result) => {
                    let effects = result.map_err(GpuRasterError::Effect)?;
                    if let Some(scene) = &mut self.scene {
                        scene.effects.merge_validated(effects.fork());
                    }
                    if let Some(cache) = &mut self.validated_effects {
                        cache.merge_validated(effects);
                    } else {
                        self.validated_effects = Some(effects);
                    }
                    startup.effects = None;
                    startup.effects_ready = true;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(GpuRasterError::Effect(
                        "Document shader compiler stopped".into(),
                    ));
                }
            }
        }
        let canvas_ready = startup.document.ready() && startup.document_key.is_some() && startup.effects_ready
            && (!startup.transform || startup.current.ready());
        let brush_ready = canvas_ready && startup.current.ready() && startup.strokes.ready() && startup.masks.ready()
            && startup.papers.iter().all(|(_,id)| self.papers.view(id).is_some());
        startup.brush_ready = brush_ready;
        startup.finished = brush_ready
            && !startup.host_catalog_pending
            && startup.compiler.pending() == 0;
        #[cfg(not(target_arch = "wasm32"))]
        if startup.finished {
            startup.compiler.save();
        }
        let progress = StartupProgress {
            canvas_ready,
            brush_ready,
            complete: startup.finished,
        };
        if canvas_ready && self.scene.is_none() {
            self.scene = Some(scene::Scene::new(self)?);
        }
        Ok(progress)
    }
}

#[cfg(test)]
fn object_document(icc:bool)->Document {
    use layer_core::authored::*;
    let mut source=(*layer_core::color::source::rgba8_source([8;2],|_,_|[255,0,0,255])).clone();
    if icc {source.interpretation.profile=layer_core::color::ColorProfile::Icc(
        layer_color::profile_bytes(&layer_core::color::ColorProfile::default()).unwrap().into());}
    let mut artwork=Artwork::new([16;2]).unwrap();
    let object=artwork.objects.insert(PortableId::random(),ImageObject::new(layer_core::authored::Image::new(Arc::new(source)))).unwrap();
    let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Images")).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    Document::from_artwork(artwork).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_normalizer_kinds_follow_depth_and_native_scalar_layout() {
        use layer_core::color::{DocumentColor,SampleDepth,RgbSpace};
        for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {
            let source=crate::test_support::depth_source([8;2],depth,RgbSpace::Srgb,2<<20,|_,_|[0.25,0.5,0.75,1.]);
            let expected=SourceKinds {float:depth==SampleDepth::U8,integer:depth!=SampleDepth::U8};
            assert_eq!(SourceKinds::for_source(&source),expected);
            assert_eq!(SourceKinds::for_descriptor(DocumentColor {depth,..Default::default()}.paint_descriptor()),expected);
            let mut document=object_document(false);
            let object=document.artwork.objects.iter().next().unwrap().0;
            document.artwork.objects.get_mut(object).unwrap().image=layer_core::authored::Image::new(source);
            assert_eq!(SourceKinds::for_scene(document.scene()),expected);
            let mut pending=Document::new(layer_core::authored::PortableId::random(),8,8,
                layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
            pending.artwork.compositions.get_mut(pending.artwork.root).unwrap().color.depth=depth;
            let SourceTarget::Paint(paint)=pending.working.target.unwrap() else {unreachable!()};
            pending.artwork.paint.get_mut(paint).unwrap().raster=layer_core::raster::RasterRevision::pending();
            assert_eq!(SourceKinds::for_scene(pending.scene()),expected);
        }
        assert_eq!(SourceKinds::for_scene(object_document(true).scene()),SourceKinds::default());
        assert_eq!(SourceKinds::for_descriptor(layer_core::raster::RasterPlane::Mask.descriptor(DocumentColor::default())),
            SourceKinds {float:false,integer:true});
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn ordinary_layer_and_selection_changes_keep_startup_prepared() {
        use layer_core::authored::*;
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
        renderer.shader_idle(false, false);
        let mut document = Document::new(PortableId::random(), 64, 64,
            layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() }).without_paper().unwrap();
        let first = document.working.occurrence.unwrap();
        let paint = document.artwork.paint.iter().next().unwrap().2.clone();
        let source = document.artwork.paint.insert(PortableId::random(), paint).unwrap();
        let second = document.artwork.occurrences.insert(PortableId::random(),
            Occurrence::new(OccurrenceContent::Paint(source), "Second")).unwrap();
        let stack = document.composition().result;
        document.artwork.stacks.get_mut(stack).unwrap().entries.push(second);
        crate::artwork_sample_tests::refresh(&mut document);
        document.working.selection = Some(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 32., y: 0. },
            layer_core::Point { x: 0., y: 32. }]).unwrap());
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        renderer.prepare_startup(&document, &brush, false).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("coarse startup key"));
        let generation = renderer.startup.as_ref().unwrap().compiler.generation();
        let mut preparations = 0;
        for change in 0..3 {
            match change {
                0 => { document.working.occurrence = Some(second); document.working.target = Some(SourceTarget::Paint(source)); }
                1 => document.artwork.occurrences.get_mut(first).unwrap().visible = false,
                _ => document.working.selection = Some(layer_core::Selection {
                    shape: layer_core::SelectionShape::Pixels(Arc::new(layer_core::SelectionPixels::new([8, 1], [0, 0, 8, 1], vec![0x44444444]).unwrap())),
                    affine: layer_core::Affine::IDENTITY, inverted: false }),
            }
            document.revision += 1;
            if renderer.startup_needs_update(&document, &brush, false) {
                preparations += 1;
                renderer.prepare_startup(&document, &brush, false).unwrap();
            }
            assert_eq!(preparations, 0);
            assert_eq!(renderer.startup.as_ref().unwrap().compiler.generation(), generation);
        }
    }
    #[test]
    fn shader_document_invalidates_when_source_depth_changes_the_normalizer() {
        use layer_core::color::{RgbSpace,SampleDepth};
        let mut document=object_document(false);
        let key=ShaderDocument::new(&document);
        let object=document.artwork.objects.iter().next().unwrap().0;
        document.artwork.objects.get_mut(object).unwrap().image=layer_core::authored::Image::new(
            crate::test_support::depth_source([8;2],SampleDepth::U16,RgbSpace::Srgb,2<<20,|_,_|[0.25,0.5,0.75,1.]));
        document.revision+=1;
        assert!(!key.matches(&document));
        let key=ShaderDocument::new(&document);
        document.artwork.objects.get_mut(object).unwrap().interpolation=layer_core::authored::ImageInterpolation::Nearest;
        document.revision+=1;
        assert!(!key.matches(&document));
        let key=ShaderDocument::new(&document);
        document.artwork.objects.get_mut(object).unwrap().interpolation=layer_core::authored::ImageInterpolation::Linear;
        document.revision+=1;
        assert!(!key.matches(&document));
        let key=ShaderDocument::new(&document);
        document.artwork.objects.get_mut(object).unwrap().image=layer_core::authored::Image::new(
            crate::test_support::depth_source([8;2],SampleDepth::F16,RgbSpace::Srgb,2<<20,|_,_|[0.25,0.5,0.75,1.]));
        document.revision+=1;
        assert!(key.matches(&document));
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_cold_navigator_prepares_inherited_window_recipes() {
        use layer_render::CanvasRenderer;
        for depth in [layer_core::color::SampleDepth::U8,layer_core::color::SampleDepth::F32] {
            let mut document=crate::scene::scale::tests::document_at([1024,512]);
            let root=document.artwork.root;
            document.artwork.compositions.get_mut(root).unwrap().color.depth=depth;
            let frame=crate::test_support::packet(document.scene(),[1024,512]);
            let mut reference=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
            crate::test_support::submit_settled(&mut reference,frame);
            let mut renderer=crate::test_support::staged_renderer(&reference,document.composition().color);
            assert!(!renderer.scene_pipelines.scale.half.reduce.ready());
            assert!(!renderer.scene_pipelines.scale.full.reduce.ready());
            renderer.adopt_committed_pages(&mut reference);
            renderer.navigator=Default::default();
            crate::test_support::submit_settled(&mut renderer,FramePacket {composite_all:false,..frame});
            let expected=crate::display_precision::DisplayPrecision::for_scene(document.scene()).format();
            let navigator=renderer.navigator.view().expect("the cold Navigator commits");
            assert!(renderer.navigator.scale()>1.);
            assert_eq!(navigator.texture().format(),expected);
            assert!(renderer.scene_pipelines.scale.output(expected).reduce.ready());
            assert!(crate::test_support::float_pixels(&renderer,navigator.texture()).iter().all(|pixel|pixel[3]==1.));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn backdrop_submission_defers_display_recipes_to_the_compiler() {
        use layer_render::CanvasRenderer;
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
        renderer.resize_surface(64, 64).unwrap();
        let document = Document::new(layer_core::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let snapshot = document.snapshot().as_ref().clone().with_scope(layer_core::authored::SceneScope::Members(document.scene().constant_backdrop().to_vec().into()));
        let (entered, entry) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let compiler = &renderer.startup.as_ref().unwrap().compiler;
        compiler.enqueue(DOCUMENT, move || { let _ = entered.send(()); let _ = held.recv(); Ok(()) });
        entry.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(renderer.scene_scale_pipelines(document.scene()).all(|pipeline| !pipeline.ready()));
        let packet = FramePacket { reset_layers: true, ..crate::test_support::packet(snapshot.view(), [64; 2]) };
        let submitted = renderer.submitted_updates();
        assert!(!renderer.raster_dependencies_ready(packet, None), "the first backdrop waits for its display recipes before submission");
        assert_eq!(renderer.submitted_updates(), submitted);
        renderer.submit(packet).unwrap();
        assert_eq!(renderer.submitted_updates(), submitted + 1);
        assert!(crate::scene::scale::tests::publication_pending(&renderer));
        assert!(renderer.presentation_views().is_none());
        assert!(renderer.scene_scale_pipelines(document.scene()).all(|pipeline| !pipeline.ready()));
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while renderer.scene_scale_pipelines(document.scene()).any(|pipeline| !pipeline.ready()) {
            renderer.startup.as_ref().unwrap().compiler.check().unwrap();
            assert!(std::time::Instant::now() < deadline, "Backdrop display recipes did not prepare");
            std::thread::sleep(Duration::from_millis(1));
        }
        crate::test_support::submit_published(&mut renderer, packet);
        assert!(!crate::scene::scale::tests::publication_pending(&renderer));
        assert!(renderer.presentation_views().is_some());
        let pixels = crate::test_support::float_pixels(&renderer, crate::test_support::document_texture(&renderer));
        assert_eq!(pixels.len(), 64 * 64);
        assert!(pixels.iter().all(|pixel| *pixel == [1.; 4]));
    }
    #[test]
    fn shader_document_tracks_typed_source_dependencies_without_tracking_geometry() {
        let mut document=object_document(false);
        let key=ShaderDocument::new(&document);
        assert!(key.key.sources.float && key.key.objects);
        assert!(document.scene().targets().next().is_none());
        let handle=document.artwork.objects.iter().next().unwrap().0;
        let object=document.artwork.objects.get_mut(handle).unwrap();
        object.affine.0[4]=8.;
        let owner=document.scene().object_owner(handle).unwrap();
        document.artwork.occurrences.get_mut(owner).unwrap().visible=false;
        document.revision+=1;
        assert!(key.matches(&document));
        document.artwork.objects.get_mut(handle).unwrap().image=object_document(true).artwork.objects.iter().next().unwrap().2.image.clone();
        document.revision+=1;
        assert!(!key.matches(&document));
        let key=ShaderDocument::new(&document);
        let stack=document.composition().result;
        let original=document.artwork.stacks.get(stack).unwrap().clone();
        let mut removed=original.clone();removed.entries.clear();
        document.apply(layer_core::Edit::Stack(layer_core::RecordChange::replace(&document.artwork.stacks,stack,Some(removed)).unwrap())).unwrap();
        assert!(!key.matches(&document));
        let key=ShaderDocument::new(&document);
        assert_eq!(key.key.sources,SourceKinds::default());
        assert!(!key.key.objects);
        document.apply(layer_core::Edit::Stack(layer_core::RecordChange::replace(&document.artwork.stacks,stack,Some(original)).unwrap())).unwrap();
        assert!(!key.matches(&document));
    }
    #[test]
    fn brush_shader_key_ignores_color_and_size_but_tracks_pipeline_changes() {
        let document = Document::new(layer_core::authored::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        let key = ShaderBrushKey::new(&document, &brush);
        brush.color_rgba_linear = [8., 0.25, 0.5, 1.];
        brush.diameter *= 2.;
        brush.stabilization.input_scale *= 0.5;
        brush.stabilization.pressure_fall_micros += 1;
        assert!(key == ShaderBrushKey::new(&document, &brush));
        brush.rendering.accumulation = if brush.rendering.accumulation == BrushAccumulation::Uniform {
            BrushAccumulation::Flow
        } else {
            BrushAccumulation::Uniform
        };
        assert!(key != ShaderBrushKey::new(&document, &brush));
    }

    #[test]
    fn dither_cell_sizes_prepare_one_shared_coarse_family() {
        let document = Document::new(layer_core::authored::PortableId::random(), 64, 64,
            layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::DitherBlender);
        brush.blender.cell_size = 1.;
        let pixel = ShaderBrushKey::new(&document, &brush);
        brush.blender.cell_size = 2.;
        let cells = ShaderBrushKey::new(&document, &brush);
        assert!(pixel != cells);
        for size in 3..=layer_core::BLENDER_CELL_SIZE_LIMIT {
            brush.blender.cell_size = size as f32;
            assert!(cells == ShaderBrushKey::new(&document, &brush));
        }
    }
    #[test]
    fn dither_cells_prepare_prediction_and_commit_before_pen_down() {
        let color=layer_core::color::DocumentColor::default();
        let reference=WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer=crate::test_support::staged_renderer(&reference,color);
        let document=Document::new(layer_core::authored::PortableId::random(),128,128,
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let mut brush=layer_core::default_brush(layer_core::DefaultBrushPreset::DitherBlender);
        brush.blender.cell_size=2.;
        renderer.prepare_startup(&document,&brush,false).unwrap();
        let deadline=std::time::Instant::now()+Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer,deadline,|progress|progress.brush_ready,
            format_args!("coarse Dither prediction and commit"));
        for size in [2.,50.] {
            brush.blender.cell_size=size;
            assert!(!renderer.startup_needs_update(&document,&brush,false),"all coarse sizes share prepared kernels");
            let style=brush_style(&document,&brush,StrokeTool::Brush);
            let kernels=&renderer.pipelines.destination_material;
            assert!(kernels.kernel(&style,destination_material::Entry::Color).ready(),"N{size} prediction Color kernel is ready");
            assert!(kernels.kernel(&style,destination_material::Entry::Coverage).ready(),"N{size} commit Coverage kernel is ready");
            renderer.prepare_startup(&document,&brush,false).unwrap();
            assert!(renderer.poll_startup().unwrap().brush_ready,"N{size} reuses both coarse kernels immediately");
        }
    }

    #[test]
    fn scatter_cell_sizes_reuse_prepared_prediction_and_commit_kernels() {
        let color=layer_core::color::DocumentColor::default();
        let reference=WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer=crate::test_support::staged_renderer(&reference,color);
        let document=Document::new(layer_core::authored::PortableId::random(),128,128,
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let mut brush=layer_core::default_brush(layer_core::DefaultBrushPreset::ScatterBlender);
        brush.blender.cell_size=1.;
        let key=ShaderBrushKey::new(&document,&brush);
        renderer.prepare_startup(&document,&brush,false).unwrap();
        let deadline=std::time::Instant::now()+Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer,deadline,|progress|progress.brush_ready,format_args!("Scatter cells"));
        for size in [1.,4.,50.] {
            brush.blender.cell_size=size;
            assert!(key==ShaderBrushKey::new(&document,&brush));
            assert!(!renderer.startup_needs_update(&document,&brush,false));
            let style=brush_style(&document,&brush,StrokeTool::Brush);
            let kernels=&renderer.pipelines.destination_material;
            assert!(kernels.kernel(&style,destination_material::Entry::Color).ready());
            assert!(kernels.kernel(&style,destination_material::Entry::Coverage).ready());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn fill_request_requires_stored_dependencies_before_the_mask_is_ready() {
        use layer_render::{CanvasRenderer,RegionRequest,RegionSource,RegionTask};
        let mut reference=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let document=Document::new(layer_core::PortableId::random(),64,64,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        crate::test_support::submit_ready(&mut reference,crate::test_support::packet(document.scene(),[64;2])).unwrap();
        let mut renderer=crate::test_support::staged_renderer(&reference,Default::default());
        renderer.artwork_frame=reference.artwork_frame.clone();renderer.document_extent=[64;2];
        assert!(std::ptr::eq(renderer.scene_pipelines.fill_pipelines(&renderer.device)[0],&renderer.scene_pipelines.pipeline[10]));
        let (entered,entry)=mpsc::channel();let (release,held)=mpsc::channel();
        let compiler=&renderer.startup.as_ref().unwrap().compiler;
        compiler.enqueue(DOCUMENT,move||{let _=entered.send(());let _=held.recv();Ok(())});
        entry.recv_timeout(Duration::from_secs(5)).unwrap();
        let request=RegionRequest {fill_areas:None,colour_mode:Default::default(),fill:Some(Default::default()),reference_count:None,paper:None,
            enclosure:None,contiguous:true,selection:None,request_id:1,source:RegionSource::Composite,position:[0;2],tolerance:0.,refinement:Default::default(),limit:None};
        let accepted=renderer.request_region(RegionTask::Fill {request,target:document.drawing_content().unwrap(),position:layer_core::Point{x:0.,y:0.}});
        let [opaque,fill,scalar]=renderer.scene_pipelines.fill_pipelines(&renderer.device);
        let pipelines=[opaque,fill,scalar,&renderer.pipelines.watercolor_composite,&renderer.layer_masks.initialize];
        let render_required=pipelines.into_iter().all(|pipeline|!pipeline.ready() && compile::tests::requested(&renderer.compilation,pipeline,DOCUMENT));
        let compute_required=renderer.selection_clip.pipelines().into_iter().chain([&renderer.selection_clip.resample,&renderer.pipelines.watercolor_compute.1])
            .all(|pipeline|!pipeline.ready() && compile::tests::requested(&renderer.compilation,pipeline,DOCUMENT));
        let regions=renderer.regions.as_ref().unwrap();
        let refinement=layer_render::RegionRefinement {close_gaps:true,..Default::default()};
        let request_required=regions.raw.pipelines().chain(regions.flood.request_pipelines(refinement,false,true,true))
            .all(|pipeline|!pipeline.ready() && compile::tests::requested(&renderer.compilation,pipeline,DOCUMENT));
        let mask=renderer.take_region();
        release.send(()).unwrap();
        assert!(accepted.unwrap());assert!(mask.is_none());assert!(render_required && compute_required && request_required);
        drop(renderer);
        drop(reference);
        finish_shader_compiler_shutdown();
    }
    #[test]
    fn zero_dab_fill_does_not_prepare_an_unrelated_brush_style() {
        let reference=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut renderer=crate::test_support::staged_renderer(&reference,Default::default());
        let mut document=Document::new(layer_core::PortableId::random(),64,64,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        pollster::block_on(renderer.compilation.prepare_all([&renderer.scene_pipelines.pipeline[3],&renderer.layer_masks.initialize])).unwrap();
        pollster::block_on(renderer.compilation.prepare_all(renderer.selection_clip.pipelines())).unwrap();
        let target=document.drawing_target().unwrap();
        let style=brush_style(&document,&layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil),StrokeTool::Brush);
        let mut required=Needs::default();required.style(&renderer,&style,dry_material::Reach::of_dabs(&[]),false,false);
        assert!(!required.ready());
        crate::layer_tests::placement::paint_mut(&mut document).operations=Arc::new(vec![layer_core::RasterOperation {placement:layer_core::Affine::IDENTITY,
            coverage:crate::layer_tests::placement::reveal_all([64;2],[0;2]),kind:layer_core::RasterOperationKind::Fill {edges:None,underfill:None,color:[0.,0.,1.,1.],alpha_locked:false,texture:Default::default()}}]);
        let batches=[DabBatch {kind:DabBatchKind::RasterOperation(0),dab_count:0,..crate::test_support::dab_batch(target,style,layer_core::Rect::from_extent([64;2]))}];
        assert!(renderer.brush_batches_ready(FramePacket {dab_batches:&batches,..crate::test_support::packet(document.scene(),[64;2])}));
        assert!(!required.ready());
    }
    #[test]
    fn readiness_waits_for_every_batch_texture_and_pipeline() {
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, color);
        let mut document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        renderer.prepare_startup(&document, &layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("G-Pen"));
        document.set_paper_texture(layer_core::paper::built_in_paper("rough").unwrap().texture.clone());
        let style = brush_style(&document, &layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil), StrokeTool::Brush);
        let damage = layer_core::Rect { min: layer_core::Point { x: 1., y: 1. }, max: layer_core::Point { x: 12., y: 12. } };
        let batches = [crate::test_support::dab_batch(document.drawing_target().unwrap(), style, damage)];
        let packet = layer_render::FramePacket { dab_batches: &batches, ..crate::test_support::packet(document.scene(), [128; 2]) };
        assert!(!renderer.raster_dependencies_ready(packet, None));
        while !renderer.raster_dependencies_ready(packet, None) {
            renderer.poll_startup().unwrap();
            assert!(std::time::Instant::now() < deadline, "the new paper and Pencil pipelines prepare");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        renderer.validate_and_prepare_brush_resources(&batches).unwrap();
        document.set_paper_texture(layer_core::paper::built_in_paper("bristol").unwrap().texture.clone());
        let target = document.drawing_target().unwrap();
        let operation = layer_core::RasterOperation { placement: layer_core::Affine::IDENTITY,
            coverage: crate::layer_tests::placement::reveal_all([128; 2], [0; 2]),
            kind: layer_core::RasterOperationKind::Fill { edges: None, underfill: None, color: [0.2, 0.4, 0.8, 1.], alpha_locked: false,
                texture: layer_core::FillTexture { strength: 1., paper: document.nearest_paper_binding(Some(target)) } } };
        crate::layer_tests::placement::paint_mut(&mut document).operations = Arc::new(vec![operation]);
        let batches = [DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0,
            ..crate::test_support::dab_batch(target, brush_style(&document, &layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), StrokeTool::Brush), damage) }];
        let packet = layer_render::FramePacket { dab_batches: &batches, ..crate::test_support::packet(document.scene(), [128; 2]) };
        assert!(!renderer.raster_dependencies_ready(packet, None));
        while !renderer.raster_dependencies_ready(packet, None) {
            renderer.poll_startup().unwrap();
            assert!(std::time::Instant::now() < deadline, "fill paper must prepare before submission");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    fn a_style_change_the_brush_key_ignores_needs_no_new_preparation() {
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, color);
        let document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        renderer.prepare_startup(&document, &brush, false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("Pencil"));
        brush.opacity *= 0.5;
        assert!(!renderer.startup_needs_update(&document, &brush, false));
        let style = brush_style(&document, &brush, StrokeTool::Brush);
        assert!(!renderer.startup.as_ref().unwrap().strokes.covers(&style));
        let damage = layer_core::Rect { min: layer_core::Point { x: 1., y: 1. }, max: layer_core::Point { x: 12., y: 12. } };
        let batches = [crate::test_support::dab_batch(document.drawing_target().unwrap(), style, damage)];
        let packet = layer_render::FramePacket { dab_batches: &batches, ..crate::test_support::packet(document.scene(), [128; 2]) };
        assert!(renderer.raster_dependencies_ready(packet, None), "the prepared brush pipelines and paper serve the changed opacity");
    }
    #[test]
    fn a_failed_paper_preparation_keeps_the_staged_startup() {
        use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, color);
        let mut document = Document::new(layer_core::authored::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let interpretation = SourceInterpretation { channels: SourceChannels::Gray, depth: layer_core::color::SampleDepth::U8,
            profile: Default::default(), profile_assumed: false };
        let mut builder = SourceBuilder::new([4, 4], interpretation, 1 << 20).unwrap();
        for _ in 0..4 { builder.push_row(&[128; 4]).unwrap(); }
        document.set_paper_texture(Some(layer_core::TextureSource::Image(layer_core::Image::new(Arc::new(builder.finish().unwrap())))));
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        assert!(renderer.prepare_startup(&document, &brush, false).is_err());
        assert!(renderer.startup.is_some());
        assert!(!renderer.poll_startup().unwrap().complete);
    }
    #[test]
    fn failed_fill_paper_admission_reaches_fallible_submission() {
        use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, color);
        let mut document = Document::new(layer_core::authored::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() }).without_paper().unwrap();
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        renderer.shader_idle(false, false);
        renderer.prepare_startup(&document, &brush, false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("G-Pen"));
        assert!(renderer.scene_pipelines.pipeline.iter().enumerate().all(|(i,pipeline)| i < 3 || i == 6 || !pipeline.ready()));
        let mut image = SourceBuilder::new([4; 2], SourceInterpretation { channels: SourceChannels::Gray,
            depth: layer_core::color::SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 1 << 20).unwrap();
        for _ in 0..4 { image.push_row(&[128; 4]).unwrap(); }
        let binding = layer_core::paper::PaperBinding { target_offset: [0; 2], surface: Arc::new(layer_core::paper::PaperSurface {
            texture: layer_core::TextureSource::Image(Arc::new(image.finish().unwrap()).into()), mapping: layer_core::Affine64([1., 0., 0., 1., 0., 0.]) }) };
        let target = document.drawing_target().unwrap();
        let before = document.target_raster(target).unwrap().identity();
        let operation = layer_core::RasterOperation { placement: layer_core::Affine::IDENTITY,
            coverage: crate::layer_tests::placement::reveal_all([64; 2], [0; 2]),
            kind: layer_core::RasterOperationKind::Fill { edges: None, underfill: None, color: [0.2, 0.4, 0.8, 1.], alpha_locked: false,
                texture: layer_core::FillTexture { strength: 1., paper: Some(binding) } } };
        let damage = operation.bounds([64; 2]);
        crate::layer_tests::placement::paint_mut(&mut document).operations = Arc::new(vec![operation]);
        let batches = [DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0,
            ..crate::test_support::dab_batch(target, brush_style(&document, &brush, StrokeTool::Brush), damage) }];
        let packet = layer_render::FramePacket { dab_batches: &batches, ..crate::test_support::packet(document.scene(), [64; 2]) };
        while !renderer.raster_dependencies_ready(packet, None) {
            renderer.poll_startup().unwrap();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(renderer.submit(packet).unwrap_err().to_string().contains("physical size"));
        assert_eq!(document.target_raster(target).unwrap().identity(), before);
    }
    #[test]
    fn fill_selection_prepares_the_resolved_paint_target() {
        use layer_core::authored::*;
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        for program in ["paper", "solid_color", "gradient_fill"] {
            let mut document = Document::new(PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
            let ink = document.scene().children(None)[0];
            let selected = if program == "paper" { document.bottom_paper().unwrap() } else {
                crate::artwork_sample_tests::insert_effect(&mut document, layer_core::EffectInstance::new(crate::tests::fixture(program).program()), 1)
            };
            document.apply(document.select_occurrence_edit(selected).unwrap()).unwrap();
            assert!(document.working.target.is_none());
            let target = document.drawing_target().unwrap();
            let style = brush_style(&document, &brush, StrokeTool::Brush);
            assert_eq!(style.paper, Some(document.paper_binding(target)), "{program}");
            let key = ShaderBrushKey::new(&document, &brush);
            document.set_paper_texture(layer_core::paper::built_in_paper("rough").unwrap().texture.clone());
            assert!(key != ShaderBrushKey::new(&document, &brush), "{program} tracks the paper used by its drawing target");
            document.artwork.occurrences.get_mut(ink).unwrap().alpha_locked = true;
            assert!(DocumentKey::new(&document).locked);
            assert!(brush_style(&document, &brush, StrokeTool::Brush).alpha_locked);
            let coverage = document.artwork.coverage.next_handle();
            let mask = layer_core::CoverageSnapshot::reveal_all(coverage, [64; 2], [0; 2]);
            document.artwork.coverage.insert(PortableId::random(), mask.source).unwrap();
            document.artwork.occurrences.get_mut(selected).unwrap().mask = Some(mask.use_);
            crate::artwork_sample_tests::refresh(&mut document);
            assert_eq!(document.drawing_target(), Some(SourceTarget::Coverage(coverage)));
            assert!(DocumentKey::new(&document).mask);
            assert!(!DocumentKey::new(&document).locked);
            let style = brush_style(&document, &brush, StrokeTool::Brush);
            assert!(!style.alpha_locked);
            assert_eq!(style.blend_space, layer_core::BlendSpace::Linear);
        }
    }
    #[test]
    fn shader_document_reuses_raster_and_parameter_edits_but_tracks_new_dependencies() {
        let mut doc = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let key = ShaderDocument::new(&doc);
        let paint = doc.scene().children(None)[0];
        doc.artwork.occurrences.get_mut(paint).unwrap().opacity = 0.5;
        doc.revision += 1;
        assert!(key.matches(&doc));
        doc.working.selection = Some(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 64., y: 0. },
            layer_core::Point { x: 0., y: 64. },
        ]).unwrap());
        doc.revision += 1;
        assert!(!key.matches(&doc));
        let key = ShaderDocument::new(&doc);
        doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().size[0] *= 2;
        doc.revision += 1;
        assert!(!key.matches(&doc));
        use layer_core::authored::*;
        let program = layer_core::bundled_effect_catalog().get("curves").unwrap().program();
        let values = layer_core::EffectInstance::new(program.clone()).values;
        let size=doc.composition().size;
        let effect = doc.artwork.effects.insert(PortableId::random(), EffectApplication::new(program,values,size)).unwrap();
        let occurrence = doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), "curves")).unwrap();
        let stack=doc.composition().result;
        doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(0, occurrence);
        crate::artwork_sample_tests::refresh(&mut doc);
        doc.revision += 1;
        let key = ShaderDocument::new(&doc);
        doc.artwork.effects.get_mut(effect).unwrap().values[0] = layer_core::EffectValue::Curve(vec![[0., 0.], [0.5, 0.6], [1., 1.]]);
        doc.revision += 1;
        assert!(key.matches(&doc), "a parameter edit uses the same pipeline");
        doc.artwork.occurrences.get_mut(occurrence).unwrap().visible = false;
        doc.revision += 1;
        assert!(!key.matches(&doc), "visibility changes the fused chains");
        let coverage = doc.artwork.coverage.next_handle();
        let mask = layer_core::CoverageSnapshot::reveal_all(coverage, doc.composition().size, [0; 2]);
        doc.artwork.coverage.insert(PortableId::random(), mask.source).unwrap();
        doc.artwork.occurrences.get_mut(paint).unwrap().mask = Some(mask.use_);
        crate::artwork_sample_tests::refresh(&mut doc);
        doc.revision += 1;
        let key = ShaderDocument::new(&doc);
        doc.working.target = Some(SourceTarget::Coverage(coverage));
        doc.revision += 1;
        assert!(!key.matches(&doc));
    }
    #[test]
    fn shader_document_tracks_publication_modes_and_reference_axes() {
        let mut doc=Document::new(layer_core::authored::PortableId::random(),128,128,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let key=ShaderDocument::new(&doc);
        let target=doc.scene().targets().find_map(|t|if let SourceTarget::Paint(h)=t {Some(h)}else{None}).unwrap();
        doc.artwork.paint.get_mut(target).unwrap().color_mode=layer_core::color::LayerColorMode::Grayscale;doc.revision+=1;
        assert!(!key.matches(&doc));
        crate::artwork_sample_tests::insert_effect(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program()),0);
        let handle=doc.scene().children(None)[0];let effect=doc.scene().effect_handle(handle).unwrap();
        let key=ShaderDocument::new(&doc);
        doc.artwork.effects.get_mut(effect).unwrap().spatial.as_mut().unwrap().mapping.0[4]+=0.25;doc.revision+=1;
        assert!(key.matches(&doc));
        doc.artwork.effects.get_mut(effect).unwrap().spatial.as_mut().unwrap().mapping.0[0]=2.;doc.revision+=1;
        assert!(!key.matches(&doc));let key=ShaderDocument::new(&doc);
        doc.artwork.occurrences.get_mut(handle).unwrap().blend=layer_core::LayerBlend::Multiply;doc.revision+=1;
        assert!(!key.matches(&doc));
    }
    #[test]
    fn dependencies_precede_speculation_and_promotions_compile_once() {
        let compiler = compile::tests::scope(2);
        let batch = compiler.batch();
        let order = Arc::new(Mutex::new(Vec::new()));
        let pipeline = |id| {
            let order = order.clone();
            Deferred::new(move || {
                order.lock().unwrap().push(id);
                id
            })
        };
        let other = pipeline(4);
        let brush = pipeline(3);
        let document = pipeline(2);
        compiler.require([&other], OTHER);
        compiler.require([&brush], OTHER);
        compiler.require([&brush], BRUSH);
        compiler.require([&document], DOCUMENT);
        assert!(order.lock().unwrap().is_empty());
        drop(batch);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while order.lock().unwrap().len() != 3 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        compiler.check().unwrap();
        let mut order = order.lock().unwrap().clone();
        assert_eq!(order.pop(), Some(4), "speculation follows every dependency");
        order.sort();
        assert_eq!(order, [2, 3], "each dependency compiles once");
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;

    #[test]
    fn context_received_before_startup_is_declared_when_startup_begins() {
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut r = crate::test_support::staged_renderer(&reference, Default::default());
        r.startup = None;
        let _batch = r.compilation.batch();
        let document = Document::new(layer_core::PortableId::random(), 64, 64,
            layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        let context = layer_render::PreparationContext {next:layer_render::PreparationUses {selection_strokes:true,..Default::default()},..Default::default()};
        let mut next = Needs::default();
        let mut selected = layer_core::Selection::empty();
        selected.inverted = true;
        next.brush(&r, &document, &brush, Some(Arc::new(selected)));
        let expected: std::collections::HashSet<_> = next.render.iter().map(Deferred::identity)
            .chain(next.compute.iter().map(Deferred::identity)).collect();
        assert!(!expected.is_empty());
        r.shader_idle(true, false);
        r.prepare_context(&document, &brush, context);
        r.startup = Some(Startup::new(r.compilation.clone()).unwrap());
        r.prepare_startup(&document, &brush, false).unwrap();
        assert_eq!(compile::tests::demands(&r.context_compilation, ANTICIPATED), expected);
    }

    #[test]
    fn optional_context_failure_surfaces_only_when_the_recipe_is_required() {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.startup = Some(Startup::new(r.compilation.clone()).unwrap());
        let recipe = Deferred::new(|| -> () { panic!("optional context recipe failed") });
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            r.context_compilation.blocking(|| { recipe.compile(); });
        }));
        assert!(failed.is_err());
        assert!(recipe.error().is_some());
        r.context_compilation.warm(&recipe, BACKGROUND, r.context_compilation.generation());
        assert!(r.poll_startup().is_ok());
        r.compilation.require([&recipe], BRUSH);
        assert!(r.poll_startup().is_err());
    }
    #[test]
    fn restore_readiness_admits_the_integer_normalizer_without_the_float_recipe() {
        use layer_core::color::{DocumentColor,SampleDepth,RgbSpace};
        let reference=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut renderer=crate::test_support::staged_renderer(&reference,Default::default());
        let descriptor=DocumentColor {depth:SampleDepth::U16,..Default::default()}.paint_descriptor();
        let blob=Arc::new(layer_core::raster::TileBlob::encode(descriptor,
            &[16384u16,32768,49152,65535].into_iter().flat_map(u16::to_le_bytes).collect::<Vec<_>>().repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap());
        let (entered,entry)=mpsc::channel();let (release,held)=mpsc::channel();
        let compiler=&renderer.startup.as_ref().unwrap().compiler;
        compiler.enqueue(DOCUMENT,move||{let _=entered.send(());let _=held.recv();Ok(())});
        entry.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(renderer.prepare_raster_uploads(&[(blob,RgbSpace::Srgb,RgbSpace::Srgb)]),Err(GpuRasterError::NotReady)));
        assert!(!renderer.scene_pipelines.source.integer.ready());
        assert!(compile::tests::requested(&renderer.compilation,&renderer.scene_pipelines.source.integer,VALIDATION));
        release.send(()).unwrap();
        let compiler=&renderer.startup.as_ref().unwrap().compiler;
        let deadline=std::time::Instant::now()+Duration::from_secs(30);
        while compiler.pending()!=0 {
            renderer.startup.as_ref().unwrap().compiler.check().unwrap();
            assert!(std::time::Instant::now()<deadline);
            std::thread::yield_now();
        }
        compiler.check().unwrap();
        assert!(renderer.scene_pipelines.source.integer.ready());
        assert!(!renderer.scene_pipelines.source.pipeline.ready());
    }
    #[test]
    fn object_only_startup_prepares_sampling_and_source_recipes_before_canvas_readiness() {
        let reference=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        for icc in [false,true] {
            let document=object_document(icc);
            let mut renderer=crate::test_support::staged_renderer(&reference,document.composition().color);
            assert!(!renderer.scene_pipelines.source.pipeline.ready());
            assert!(renderer.scene_pipelines.objects.pipelines().into_iter().all(|p|!p.ready()));
            assert!(renderer.moving_images.pipelines(&renderer.device).into_iter().all(|p|!p.ready()));
            renderer.prepare_startup(&document,&layer_core::default_brush(layer_core::DefaultBrushPreset::GPen),false).unwrap();
            let deadline=std::time::Instant::now()+Duration::from_secs(30);
            crate::test_support::wait_startup(&mut renderer,deadline,|progress|progress.canvas_ready,
                format_args!("object-only startup timed out, ICC={icc}"));
            assert!(renderer.scene_pipelines.source.pipeline.ready());
            assert!(renderer.scene_pipelines.objects.pipelines().into_iter().all(|p| p.ready()));
            assert!(renderer.moving_images.pipelines(&renderer.device).into_iter().all(|p| p.ready()));
            let mut encoder=crate::submission::CommandEncoder::new(&renderer.device,&Default::default());
            let source=document.artwork.objects.iter().next().unwrap().2.image.clone();
            let request=crate::object_image_mips::MovingRequest {id:source.id(),source:source.storage().clone(),level:1,nearest:false,bounds:crate::PixelRect::full(source.extent),format:crate::object_image_mips::MovingImages::format(document.scene()),rank:0};
            let mut images=std::mem::take(&mut renderer.moving_images);
            let plan=images.plan(&renderer,std::slice::from_ref(&request),u64::MAX,&mut encoder);
            assert!(plan.is_ok());
            assert!(images.pipelines(&renderer.device).into_iter().all(|p| p.ready()),
                "first source context keeps the startup-prepared reduction recipes");
            renderer.moving_images=images;
            encoder.submit(&renderer.queue);
        }
        drop(reference);
        finish_shader_compiler_shutdown();
    }
    #[test]
    fn native_publication_pipelines_follow_canvas_and_gate_brush() {
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, color);
        assert!(renderer.scene_pipelines.pipeline.iter().all(|p| !p.ready()));
        assert!(
            renderer
                .native_edit
                .as_ref()
                .unwrap()
                .pipelines()
                .all(|p| !p.ready())
        );
        assert!(renderer.pipelines.dry_material.kernels.iter().all(|p| !p.ready()));
        let mut document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        renderer.shader_idle(false, false);
        renderer.prepare_startup(&document, &brush, false).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("Native brush compilation timed out"));
        assert!(renderer.scene_pipelines.source.pipeline.ready(), "native paint restore and Undo require decoded tiles before painting is ready");
        let display = renderer.display_pipelines.as_ref().expect("native canvas display pipelines must exist before input");
        assert!(display.reduce.ready() && display.fused_reduce.ready(),
            "native display hierarchy must not compile mip pipelines on a ready canvas frame");
        let mut recolored = brush.clone();
        recolored.color_rgba_linear = [8., 0.25, 0.5, 1.];
        assert!(!renderer.startup_needs_update(&document, &recolored, false));
        assert!(renderer.poll_startup().unwrap().brush_ready);
        assert!(renderer.scene_pipelines.for_scene(&renderer,document.scene(),&[]).all(|p| p.ready()));
        assert!(renderer.scene_pipelines.pipeline.iter().enumerate().all(|(i,pipeline)| i < 3 || i == 6 || !pipeline.ready()));
        let target = document.drawing_target().unwrap();
        let paper = document.nearest_paper_binding(Some(target)).unwrap();
        for texture in [layer_core::FillTexture { strength: 0., paper: Some(paper) },
            layer_core::FillTexture { strength: 1., paper: None }] {
            let operation = layer_core::RasterOperation { placement: layer_core::Affine::IDENTITY,
                coverage: crate::layer_tests::placement::reveal_all([128; 2], [0; 2]),
                kind: layer_core::RasterOperationKind::Fill { edges: None, underfill: None, color: [0.2, 0.4, 0.8, 1.], alpha_locked: false, texture } };
            let damage = operation.bounds([128; 2]);
            crate::layer_tests::placement::paint_mut(&mut document).operations = Arc::new(vec![operation]);
            document.revision += 1;
            renderer.prepare_startup(&document, &brush, false).unwrap();
            let batches = [DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0,
                ..crate::test_support::dab_batch(target, brush_style(&document, &brush, StrokeTool::Brush), damage) }];
            let packet = layer_render::FramePacket { dab_batches: &batches, ..crate::test_support::packet(document.scene(), [128; 2]) };
            while !renderer.raster_dependencies_ready(packet, None) {
                renderer.poll_startup().unwrap();
                assert!(std::time::Instant::now() < deadline, "flat fill dependencies must prepare without optional compilation");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(renderer.scene_pipelines.for_scene(&renderer,document.scene(),&[]).all(|pipeline| pipeline.ready()));
            assert!(renderer.scene_pipelines.pipeline[3].ready(), "flat fills must prepare their operation family before submission");
            assert!(renderer.scene_pipelines.pipeline.iter().enumerate().all(|(i,pipeline)| i < 4 || i == 6 || !pipeline.ready()), "flat fills must not prepare other operation families");
        }
        crate::layer_tests::placement::paint_mut(&mut document).operations = Arc::default();
        document.revision += 1;
        assert!(
            renderer
                .native_edit
                .as_ref()
                .unwrap()
                .required_pipelines(document.composition().color.depth,false)
                .all(|p| p.ready())
        );
        for coverage in [false, true] {
            assert!(renderer.pipelines.dry_material
                .selected_kernel(&layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush), MaterialOperation::Coverage, coverage,0).ready(),
                "G-Pen commit and prediction kernels must be ready before input is enabled");
        }
        let unselected = [layer_core::DefaultBrushPreset::WatercolorWash,
            layer_core::DefaultBrushPreset::CloneStamp, layer_core::DefaultBrushPreset::LiquifyPush]
            .map(|preset| {
                let brush = layer_core::default_brush(preset);
                let mut required = Needs::default();
                for tool in [StrokeTool::Brush, StrokeTool::Eraser] {
                    let style = layer_render::DabStyle { blend_space: document.composition().blend,
                        ..layer_render::DabStyle::for_brush(&brush, tool) };
                    required.style(&renderer, &style, dry_material::Reach::of(&brush), false, true);
                }
                assert!(!required.ready(), "unselected {preset:?} must begin cold");
                (preset, required)
            });
        renderer.shader_idle(true, false);
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.complete, format_args!("Startup compilation timed out"));
        assert!(renderer.scene_pipelines.pipeline.iter().enumerate().all(|(i,pipeline)| i < 4 || i == 6 || !pipeline.ready()), "unused operation families must remain cold after idle startup");
        for (preset, required) in &unselected {
            assert!(!required.ready(), "unselected {preset:?} must remain cold after idle startup");
        }
        let transforms = renderer.transforms.as_ref().unwrap();
        assert!(transforms.pipelines().into_iter().chain(transforms.mesh_pipelines()).all(|p| !p.ready())
            && transforms.display_pipelines().into_iter().all(|p| !p.ready()),
            "unselected transforms and warps must remain cold after idle startup");
        renderer.shader_idle(false, false);
        for (preset, required) in &unselected {
            renderer.prepare_startup(&document, &layer_core::default_brush(*preset), false).unwrap();
            crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready,
                format_args!("{preset:?} demand compilation timed out"));
            assert!(required.ready(), "{preset:?} painting and physical eraser must be ready before input");
        }
        for (preset, _) in &unselected {
            renderer.prepare_startup(&document, &layer_core::default_brush(*preset), false).unwrap();
            assert!(renderer.poll_startup().unwrap().brush_ready, "revisiting {preset:?} must reuse its prepared dependencies");
        }
        let target = document.scene().targets().find(|target| matches!(target, SourceTarget::Paint(_))).unwrap();
        renderer.prepare_retouch_sources(Some(&layer_render::RetouchPreparation {execution:layer_core::BrushExecution::Clone,
            target, retouch: Default::default(), points: Vec::new(),
        }));
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready,
            format_args!("Retouch source demand compilation timed out"));
        let retouch = renderer.retouch_sources();
        let pipelines = renderer.retouch_pipelines();
        assert!(Arc::ptr_eq(&pipelines, &renderer.retouch_pipelines()));
        let (render, compute) = retouch.pipelines();
        assert!(render.iter().all(|p| p.ready()) && compute.iter().all(|p| p.ready()));
        drop(retouch);
        let (render, compute) = renderer.retouch_sources().pipelines();
        assert!(render.iter().all(|p| p.ready()) && compute.iter().all(|p| p.ready()), "releasing retouch pixels retains its shaders");
        renderer.prepare_startup(&document, &brush, false).unwrap();
        assert!(renderer.regions.as_ref().is_none_or(|regions| regions.flood.pipelines().all(|p| !p.ready())),
            "unused region recipes must remain uncompiled");
        assert!(renderer.startup_needs_update(&document, &brush, true),
            "demand readiness continues after initial completion");
        renderer.prepare_startup(&document, &brush, true).unwrap();
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready,
            format_args!("Transform demand compilation timed out"));
        let transforms = renderer.transforms.as_ref().unwrap();
        assert!(transforms.pipelines().into_iter().all(|p| p.ready())
            && transforms.display_pipelines().into_iter().all(|p| p.ready()),
            "a demanded transform must be ready before drawing");
        assert!(transforms.mesh_pipelines().into_iter().all(|p| !p.ready()),
            "ordinary transforms must not prepare unused warp meshes");
        let meshes = transforms.mesh_pipelines();
        let target = document.scene().targets().find(|target| matches!(target, SourceTarget::Paint(_))).unwrap();
        let mesh = layer_core::MeshMap::identity(layer_core::Rect::from_extent([128; 2]), [3, 3]).unwrap()
            .move_node(1, layer_core::Point { x: 0., y: 40. }).unwrap();
        let warp = layer_render::TransformPreview { transaction: 1, moving: false, target, selection: None,
            transform: layer_core::ImageTransform { placement: layer_core::LayerPlacement { mesh: Some(Arc::new(mesh)),
                ..Default::default() }, ..Default::default() } };
        let packet = || crate::test_support::packet(document.scene(), [128; 2]);
        assert!(!renderer.raster_dependencies_ready(packet(), Some(&warp)), "first warp use must wait for its cold dependencies");
        crate::test_support::wait_startup(&mut renderer, deadline, |_| meshes.iter().all(|p| p.ready()),
            format_args!("Warp mesh demand compilation timed out"));
        while !renderer.raster_dependencies_ready(packet(), Some(&warp)) {
            renderer.poll_startup().unwrap();
            assert!(std::time::Instant::now() < deadline, "Warp preview preparation timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(meshes.iter().all(|p| p.ready()), "prepared warp dependencies must be reused");
        assert!(renderer.scene_pipelines.resample.mesh[..4].iter().all(|p| !p.ready())
            && renderer.scene_pipelines.fine_resample.mesh.iter().all(|p| !p.ready()),
            "a native warp must not prepare unused display resampling");
        renderer.prepare_startup(&document, &brush, false).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready, "cached dependencies resume immediately");
        renderer.prepare_startup(&document, &brush, true).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready, "revisiting the transform must reuse its prepared dependencies");
    }

    #[test]
    fn transform_display_frames_prepare_only_the_resample_entries_they_draw() {
        let extent = [512; 2];
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
        let mut document = crate::layer_tests::placement::paint_document(extent, "Display transform");
        crate::layer_tests::placement::set_source(&mut document,
            layer_core::color::source::rgba8_source(extent, |_, _| [127, 63, 191, 255]));
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        renderer.prepare_startup(&document, &brush, true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready,
            format_args!("Transform startup timed out"));
        let cold = |renderer: &WgpuRasterizer| {
            assert!(renderer.scene_pipelines.resample.mesh[..4].iter().all(|p| !p.ready()));
            assert!(renderer.scene_pipelines.fine_resample.mesh.iter().all(|p| !p.ready()));
        };
        cold(&renderer);
        assert!(renderer.scene_pipelines.resample.mesh[4..].iter().all(|p| p.ready()));
        let mut packet = crate::test_support::packet(document.scene(), extent);
        packet.view = layer_render::ViewState { width_px: 128, height_px: 128,
            document_to_surface: [0.25, 0., 0., 0.25, 0., 0.] };
        let mut preview = layer_render::TransformPreview { transaction: 1, moving: true,
            target: crate::layer_tests::placement::target(&document), selection: None,
            transform: layer_core::ImageTransform::affine(layer_core::Affine::translation(layer_core::Point { x: 1.25, y: 0.5 })) };
        renderer.set_transform_preview(Some(&preview)).unwrap();
        let request = scene::scale::request(&renderer, packet).unwrap();
        assert!(request.plan.level > 0);
        assert!(matches!(request.evaluation, scene::scale::Evaluation::Display));
        while !renderer.raster_dependencies_ready(packet, Some(&preview)) {
            renderer.poll_startup().unwrap();
            assert!(std::time::Instant::now() < deadline, "Display transform preparation timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        crate::test_support::submit_published(&mut renderer, packet);
        cold(&renderer);
        assert!(matches!(renderer.scale_display.as_ref().unwrap().evaluation, scene::scale::Evaluation::Display));
        let previous = preview.drawn().into_owned();
        let mesh = layer_core::MeshMap::identity(layer_core::Rect::from_extent(extent), [3, 3]).unwrap()
            .move_node(1, layer_core::Point { x: 0., y: 40. }).unwrap();
        preview.transform.placement = layer_core::LayerPlacement { mesh: Some(Arc::new(mesh)), ..Default::default() };
        renderer.set_transform_preview(Some(&preview)).unwrap();
        let mut required = Needs::default();
        let transforms = renderer.transforms.as_ref().unwrap();
        transforms.require_frame(&renderer, packet, Some(&preview), &mut required);
        let pass = transforms.placement_pass(false);
        let identity = pass.render_pipeline(&Default::default(), pixel_transform::PREVIEW_TAPS);
        let mut expected: Vec<_> = [
            pass.render_pipeline(&previous, pixel_transform::exact_taps(&previous).min(pixel_transform::PREVIEW_TAPS)),
            identity.clone(),
            pass.render_pipeline(&preview.drawn(), pixel_transform::exact_taps(&preview.drawn()).min(pixel_transform::PREVIEW_TAPS)),
            identity,
        ].into_iter().chain(transforms.mesh_pipelines()).map(|pipeline| pipeline.identity()).collect();
        let mut actual: Vec<_> = required.render.iter().map(Deferred::identity).collect();
        expected.sort_unstable(); actual.sort_unstable();
        assert_eq!(actual, expected);
        assert!(!renderer.prepare_transform_preview(packet, Some(&preview)), "cold warp resampling must defer preparation");
        while !renderer.raster_dependencies_ready(packet, Some(&preview)) {
            renderer.poll_startup().unwrap();
            assert!(std::time::Instant::now() < deadline, "Warp display preparation timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(renderer.scene_pipelines.resample.mesh.iter().all(|p| p.ready()));
        assert!(renderer.scene_pipelines.fine_resample.mesh.iter().all(|p| !p.ready()));
        crate::test_support::submit_published(&mut renderer, packet);
        assert!(renderer.scene_pipelines.fine_resample.mesh.iter().all(|p| !p.ready()));
    }

    #[test]
    fn cold_preview_prepares_the_drawn_filter_without_unused_planes() {
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
        renderer.shader_idle(false, false);
        let document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        renderer.prepare_startup(&document, &brush, false).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("Transform preparation timed out"));
        let target = document.scene().targets().find(|target| matches!(target, SourceTarget::Paint(_))).unwrap();
        let mut preview = layer_render::TransformPreview { transaction: 1, moving: true, target, selection: None,
            transform: layer_core::ImageTransform { placement: layer_core::LayerPlacement { interpolation: layer_core::Interpolation::Bicubic,
                ..layer_core::LayerPlacement::from_affine(layer_core::Affine::translation(layer_core::Point { x: 0.25, y: 0.5 })) }, ..Default::default() } };
        preview.moving = false;
        let mut required = Needs::default();
        renderer.transforms.as_ref().unwrap().require_frame(&renderer, crate::test_support::packet(document.scene(), [128; 2]), Some(&preview), &mut required);
        assert_eq!(required.render.len(), 2);
        let exact = required.render[0].clone();
        let identity = renderer.transforms.as_ref().unwrap().placement_pass(false).render_pipeline(&Default::default(), pixel_transform::PREVIEW_TAPS);
        assert_eq!(required.render[1].identity(), identity.identity());
        assert_ne!(exact.identity(), identity.identity());
        assert!(!exact.ready());
        for moving in [true, false] {
            preview.moving = moving;
            assert!(!renderer.raster_dependencies_ready(crate::test_support::packet(document.scene(), [128; 2]), Some(&preview)));
            while !renderer.raster_dependencies_ready(crate::test_support::packet(document.scene(), [128; 2]), Some(&preview)) {
                renderer.poll_startup().unwrap();
                assert!(std::time::Instant::now() < deadline, "Preview preparation timed out");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(exact.ready(), !moving);
            let mut required = Needs::default();
            renderer.transforms.as_ref().unwrap().require_frame(&renderer, crate::test_support::packet(document.scene(), [128; 2]), Some(&preview), &mut required);
            assert_eq!(required.render.len(), 2);
            assert_eq!(required.render[1].identity(), identity.identity());
        }
        preview.transform.placement.outer.0[2] += 12.;
        assert!(renderer.raster_dependencies_ready(crate::test_support::packet(document.scene(), [128; 2]), Some(&preview)), "numeric edits reuse the prepared variant");
    }

    #[test]
    fn edged_brushes_in_perceptual_documents_compile_their_deposit_before_pen_down() {
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, color);
        let mut document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().blend=layer_core::BlendSpace::Perceptual;
        for preset in [layer_core::DefaultBrushPreset::Airbrush,layer_core::DefaultBrushPreset::Pencil,
            layer_core::DefaultBrushPreset::Charcoal,layer_core::DefaultBrushPreset::PastelBlock] {
            let mut brush=layer_core::default_brush(preset);
            if preset==layer_core::DefaultBrushPreset::Airbrush {
                brush.rendering.accumulation=BrushAccumulation::Flow;
                brush.rendering.wet_edge=0.5;
            }
            renderer.prepare_startup(&document,&brush,false).unwrap();
            let deadline=std::time::Instant::now()+Duration::from_secs(30);
            crate::test_support::wait_startup(&mut renderer,deadline,|progress|progress.brush_ready,format_args!("{preset:?} compilation timed out"));
            let style=layer_render::DabStyle {blend_space:document.composition().blend,..layer_render::DabStyle::for_brush(&brush,StrokeTool::Brush)};
            let plan=BrushPassPlan::for_device(&style,&renderer.device);
            assert!(plan.direct.is_none() && plan.material==MaterialOperation::Deposit);
            let p=&renderer.pipelines.dry_material;
            for extra in dry_material::Reach::of(&brush).required_variants(&style) {
                assert!(p.selected_kernel(&style,plan.material,plan.state.coverage,extra).ready(),"{preset:?} commit variant {extra:#x} is ready");
                assert!(p.selected_kernel(&style,plan.material,false,extra).ready(),"{preset:?} prediction variant {extra:#x} is ready");
            }
            assert!(p.gather_kernel().ready(),"{preset:?} display input gather is ready");
            if brush.contact.is_some_and(|c|c.dry.is_some()) {
                assert!(p.copy_kernel(false).ready());
            }
        }
    }

    #[test]
    fn dry_preview_completions_use_the_ready_committed_kernel() {
        use dry_material::{AGGREGATE, COMPLETION, FINE_TIP};
        let color = Default::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut r = crate::test_support::staged_renderer(&reference, color);
        let doc = Document::new(
            layer_core::PortableId::random(),
            128,
            128,
            layer_core::DocumentNames {
                paint: "Ink".into(),
                paper: "Paper".into(),
            },
        );
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        let style = brush_style(&doc, &brush, StrokeTool::Brush);
        let completion = r
            .pipelines
            .dry_material
            .selected_kernel(
                &style,
                MaterialOperation::Deposit,
                false,
                AGGREGATE | FINE_TIP | COMPLETION,
            )
            .clone();
        r.shader_idle(false, false);
        r.prepare_startup(&doc, &brush, false).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        crate::test_support::wait_startup(
            &mut r,
            deadline,
            |p| p.brush_ready,
            format_args!("Pencil dependencies"),
        );
        assert!(completion.ready());
        let body = Dab {
            radii: [3.; 2],
            previous: [3., 3., 1., 0.],
            motion: [4., 0.],
            contact: [0.7, 0., 0., 0.],
            previous_contact: [0.7, 0., 0., 0.],
            ..crate::layer_tests::dab([0.1, 0.1, 0.1, 1.])
        };
        let end = body.completion([-1., 0.], [0.; 2], 1.);
        let mut batch = crate::test_support::dab_batch(
            doc.working.target.unwrap(),
            style,
            layer_core::Rect::from_extent([128; 2]),
        );
        batch.kind = DabBatchKind::Preview;
        batch.dab_count = 2;
        let dabs = [body, end];
        r.submit(FramePacket {
            dabs: &dabs,
            dab_batches: &[batch],
            ..crate::test_support::packet(doc.scene(), [128; 2])
        })
        .unwrap();
        assert_eq!(
            r.artwork_frame.as_ref().unwrap().packet([128; 2]).dabs,
            dabs
        );
    }

    #[test]
    fn dry_brushes_require_one_kernel_per_tip_class() {
        use dry_material::{AGGREGATE,COMPLETION,FINE_TIP,Reach};
        use layer_core::DefaultBrushPreset::{Pencil,PointyPencil,Charcoal,Chalk,PastelBlock};
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        for preset in [Pencil,PointyPencil,Charcoal,Chalk,PastelBlock] {
            for accumulation in [BrushAccumulation::Flow,BrushAccumulation::Uniform] {
                for diameter in [9.,36.,300.] {
                    let mut brush=layer_core::default_brush(preset);
                    brush.diameter=diameter;
                    brush.rendering.accumulation=accumulation;
                    let style=layer_render::DabStyle::for_brush(&brush,StrokeTool::Brush);
                    let corner=brush.contact.unwrap().dry.unwrap().corner;
                    let tip=if corner==0. {FINE_TIP} else {0};
                    let coarse=corner>0. || brush.conservative_minor_radius()>=dry_material::FINE_TIP_RADIUS;
                    let flow=accumulation==BrushAccumulation::Flow;
                    let base=if flow {AGGREGATE|COMPLETION} else {0};
                    let classes=[(tip!=0).then_some(base|FINE_TIP),(tip==0 || coarse).then_some(base)].into_iter().flatten().collect::<Vec<_>>();
                    let reach=Reach::of(&brush);
                    assert_eq!(reach.required_variants(&style).collect::<Vec<_>>(),classes,"{preset:?} {accumulation:?} {diameter}");
                    let mut required=Needs::default();
                    required.style(&r,&style,reach,false,true);
                    let plan=BrushPassPlan::for_device(&style,&r.device);
                    assert!(plan.direct.is_none());
                    let copies=if r.pipelines.dry_material.in_place() {1+usize::from(plan.state.coverage)} else {usize::from(flow && plan.material==MaterialOperation::Deposit)};
                    let auxiliary=1+copies;
                    assert_eq!(required.compute.len(),auxiliary+classes.len()*(1+usize::from(plan.state.coverage)),"{preset:?} {accumulation:?} {diameter}: one kernel per tip class and entry");
                    if flow {
                        let dabs=[crate::layer_tests::dab([0.1,0.2,0.3,1.]),crate::layer_tests::dab([0.4,0.5,0.6,1.])];
                        let coarse_dabs=[AGGREGATE,0].into_iter().flat_map(|pigment|[(tip!=0).then_some(pigment|FINE_TIP|COMPLETION),Some(pigment|COMPLETION)]).flatten().collect::<Vec<_>>();
                        assert_eq!(Reach::of_dabs(&dabs).required_variants(&style).collect::<Vec<_>>(),coarse_dabs,"{preset:?}: one kernel per pigment and tip class");
                    }
                }
            }
        }
    }

    #[test]
    fn choosing_a_selection_tool_warms_pencil_without_changing_readiness() {
        let color=Default::default();let reference=WgpuRasterizer::new_native_headless(color).unwrap();
        let mut r=crate::test_support::staged_renderer(&reference,color);
        let mut document=Document::new(layer_core::PortableId::random(),64,64,
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let brush=layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        let selection=Arc::new(layer_core::Selection::polygon(vec![layer_core::Point{x:0.,y:0.},
            layer_core::Point{x:64.,y:0.},layer_core::Point{x:0.,y:64.}]).unwrap());
        let mut required=Needs::default();
        for tool in [StrokeTool::Brush,StrokeTool::Eraser] {
            let style=layer_render::DabStyle {selection:Some(selection.clone()),..brush_style(&document,&brush,tool)};
            required.style(&r,&style,dry_material::Reach::of(&brush),false,true);
        }
        r.shader_idle(true,false);r.prepare_startup(&document,&brush,false).unwrap();
        let deadline=std::time::Instant::now()+Duration::from_secs(60);
        crate::test_support::wait_startup(&mut r,deadline,|p|p.complete,format_args!("initial Pencil readiness"));
        assert!(!required.ready(),"Pencil starts without its selection kernels");
        let generation=r.compilation.generation();
        layer_render::CanvasRenderer::prepare_context(&mut r,&document,&brush,layer_render::PreparationContext {next:layer_render::PreparationUses {selection_strokes:true,..Default::default()},..Default::default()});
        assert!(!r.startup_needs_update(&document,&brush,false));
        assert!(r.startup.as_ref().unwrap().brush_ready && r.startup.as_ref().unwrap().finished);
        assert_eq!(r.compilation.generation(),generation);
        assert_eq!(r.startup.as_ref().unwrap().compiler.pending(),0);
        let (drained,done)=mpsc::channel();
        r.compilation.enqueue(BACKGROUND,move||{drained.send(()).unwrap();Ok(())});
        r.shader_idle(true,true);
        crate::test_support::wait_startup(&mut r,deadline,|p|p.brush_ready && done.try_recv().is_ok(),format_args!("selection brush background work"));
        assert!(required.ready(),"choosing a selection tool prepares both Pencil strokes before selection commit");
        document.working.selection=Some((*selection).clone());document.revision+=1;
        r.shader_idle(false,false);r.prepare_startup(&document,&brush,false).unwrap();
        assert!(r.poll_startup().unwrap().brush_ready,"committing the selection needs no new brush compilation");
        assert_eq!(r.startup.as_ref().unwrap().compiler.pending(),0);
        drop(r);drop(reference);finish_shader_compiler_shutdown();
    }

    #[test]
    fn evicted_startup_paper_is_requested_until_brush_ready() {
        let color=Default::default();
        let reference=WgpuRasterizer::new_native_headless(color).unwrap();
        let mut r=crate::test_support::staged_renderer(&reference,color);
        let doc=crate::layer_tests::paper_shade::rough_gray_paper();
        let brush=layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        let style=brush_style(&doc,&brush,StrokeTool::Brush);
        let binding=style.grain.and(style.paper.clone()).expect("Pencil samples the paper");
        let deadline=std::time::Instant::now()+Duration::from_secs(60);
        r.prepare_startup(&doc,&brush,false).unwrap();
        crate::test_support::wait_startup(&mut r,deadline,|p|p.brush_ready,format_args!("Pencil on paper"));
        let papers:Vec<_>=r.startup.as_ref().unwrap().papers.iter().map(|(_,id)|id.clone()).collect();
        assert!(!papers.is_empty());
        for step in 1..=7 {
            let mut surface=(*binding.surface).clone();
            let scale=1.+0.5*f64::from(step);
            surface.mapping.0[0]*=scale;surface.mapping.0[3]*=scale;
            let other=layer_core::paper::PaperBinding {surface:Arc::new(surface),target_offset:[0;2]};
            r.papers.require(&r.device,Some(&r.compilation),&other,OTHER).unwrap();
            r.papers.advance(&r.device,&r.queue,true,None).unwrap();
        }
        let evicted=papers.iter().filter(|id|r.papers.view(id).is_none()).count();
        let settle=std::time::Instant::now()+Duration::from_secs(3);
        while std::time::Instant::now()<settle { r.poll_startup().unwrap(); std::thread::sleep(Duration::from_millis(5)); }
        let progress=r.poll_startup().unwrap();
        assert!(evicted>0,"the fixture must evict the startup paper");
        assert!(!r.startup_needs_update(&doc,&brush,false));
        assert!(progress.brush_ready,"an evicted startup paper bake must not hold the brush gate");
        drop(r);drop(reference);finish_shader_compiler_shutdown();
    }

    #[test]
    fn an_idle_host_finishes_a_paper_brush_startup_from_wakes() {
        let color=Default::default();
        let reference=WgpuRasterizer::new_native_headless(color).unwrap();
        let mut r=crate::test_support::staged_renderer(&reference,color);
        let (wake,woken)=mpsc::channel();
        r.set_preparation_wake(Arc::new(move||{let _=wake.send(());}));
        let mut doc=Document::new(layer_core::authored::PortableId::random(),64,64,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let brush=layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        let deadline=std::time::Instant::now()+Duration::from_secs(60);
        for paper in [None,Some("rough")] {
            if let Some(paper)=paper {doc.set_paper_texture(layer_core::paper::built_in_paper(paper).unwrap().texture.clone());}
            assert!(r.startup_needs_update(&doc,&brush,false),"{paper:?} bakes another paper tile");
            r.prepare_startup(&doc,&brush,false).unwrap();
            assert!(!r.startup.as_ref().unwrap().papers.is_empty(),"Pencil bakes its paper before pen-down");
            loop {
                let progress=r.poll_startup().unwrap();
                if progress.complete {break;}
                if r.frame_demand()!=layer_render::FrameDemand::Wait {std::thread::sleep(Duration::from_millis(1));continue;}
                loop {
                    woken.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).unwrap_or_else(|_| panic!(
                        "unfinished startup on {paper:?} waited without a wake: {progress:?}, terms: {:?}, pending jobs: {}",
                        r.outstanding_terms(),r.startup.as_ref().unwrap().compiler.pending()));
                    if r.frame_preparation_pending() {break;}
                }
            }
        }
        drop(r);drop(reference);finish_shader_compiler_shutdown();
    }

    #[test]
    fn selected_dry_variants_warm_after_brush_readiness() {
        use dry_material::{AGGREGATE,COMPLETION,FINE_TIP,Reach};
        let color=Default::default();
        let reference=WgpuRasterizer::new_native_headless(color).unwrap();
        let mut r=crate::test_support::staged_renderer(&reference,color);
        let mut doc=crate::layer_tests::paper_shade::rough_gray_paper();
        crate::layer_tests::paper_shade::set_paper(&mut doc,&[(layer_core::paper::RELIEF,layer_core::EffectValue::Number(150.))]);
        let mut brush=layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        let style=brush_style(&doc,&brush,StrokeTool::Brush);
        let deadline=std::time::Instant::now()+Duration::from_secs(60);
        r.shader_idle(true,false);
        r.prepare_startup(&doc,&brush,false).unwrap();
        crate::test_support::wait_startup(&mut r,deadline,|p|p.brush_ready,format_args!("Pencil superset"));
        assert!(r.pipelines.dry_material.selected_kernel(&style,MaterialOperation::Deposit,false,AGGREGATE|FINE_TIP|COMPLETION).ready());
        let exact=Reach::of(&brush).any_size().batch_variants(&style)
            .filter(|extra| !Reach::of(&brush).required_variants(&style).any(|required| required == *extra)).map(|extra|
            r.pipelines.dry_material.selected_kernel(&style,MaterialOperation::Deposit,false,extra).clone()).collect::<Vec<_>>();
        assert!(!exact.is_empty());
        crate::test_support::submit_published(&mut r,crate::test_support::packet(doc.scene(),[128;2]));
        let mut presenter=crate::ViewportPresenter::for_surface(&r,wgpu::TextureFormat::Rgba8UnormSrgb,crate::SdrSurfaceColor::Srgb).unwrap();
        while !presenter.prepare_paper_lighting(&r) {
            r.poll_startup().unwrap();
            assert!(std::time::Instant::now() < deadline,"visible paper lighting prepares without speculative idle");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(r.paper_shade().is_some());
        assert!(exact.iter().all(|p| !p.ready()),"paper lighting precedes speculative exact-variant warming");
        r.shader_idle(true,true);
        crate::test_support::wait_startup(&mut r,deadline,|p|p.brush_ready && exact.iter().all(|p|p.ready()),format_args!("selected exact variants warm during speculative idle"));
        brush.diameter=36.;
        assert!(r.startup_needs_update(&doc,&brush,false));
        r.prepare_startup(&doc,&brush,false).unwrap();
        assert!(r.poll_startup().unwrap().brush_ready,"warmed tip classes cover the resized brush");
        doc.working.selection=Some(layer_core::Selection::polygon(vec![
            layer_core::Point{x:0.,y:0.},layer_core::Point{x:64.,y:0.},layer_core::Point{x:0.,y:64.},
        ]).unwrap());
        doc.revision+=1;
        let selected=brush_style(&doc,&brush,StrokeTool::Brush);
        let mixed=r.pipelines.dry_material.selected_kernel(&selected,MaterialOperation::Deposit,false,FINE_TIP|COMPLETION).clone();
        assert!(!mixed.ready(),"a clipped brush needs the other pigment route");
        r.prepare_startup(&doc,&brush,false).unwrap();
        assert!(r.raster_dependencies_ready(crate::test_support::packet(doc.scene(),[128;2]),None),"an empty frame does not wait for the selected brush");
        crate::test_support::wait_startup(&mut r,deadline,|p|p.brush_ready,format_args!("selected Pencil superset"));
        assert!(mixed.ready());
        drop(r);
        drop(reference);
        finish_shader_compiler_shutdown();
    }

    #[test]
    fn dry_frame_admission_and_dispatch_use_the_ready_tip_class() {
        use dry_material::{AGGREGATE,COMPLETION,FINE_TIP,Reach};
        let color=Default::default();
        let reference=WgpuRasterizer::new_native_headless(color).unwrap();
        let mut r=crate::test_support::staged_renderer(&reference,color);
        let doc=Document::new(layer_core::PortableId::random(),128,128,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let deadline=std::time::Instant::now()+Duration::from_secs(60);
        r.prepare_startup(&doc,&layer_core::default_brush(layer_core::DefaultBrushPreset::GPen),false).unwrap();
        crate::test_support::wait_startup(&mut r,deadline,|p|p.brush_ready,format_args!("document dependencies"));
        let brush=layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        let style=crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::Pencil);
        let mut required=Needs::default();
        required.style(&r,&style,Reach::of(&brush).any_size(),false,true);
        pollster::block_on(required.prepare(&r.compilation)).unwrap();
        for (radius,extra,class) in [(3.,AGGREGATE|FINE_TIP,AGGREGATE|FINE_TIP|COMPLETION),(12.,AGGREGATE,AGGREGATE|COMPLETION)] {
            let class=r.pipelines.dry_material.selected_kernel(&style,MaterialOperation::Deposit,false,class).clone();
            let preferred=r.pipelines.dry_material.selected_kernel(&style,MaterialOperation::Deposit,false,extra).clone();
            assert!(!preferred.ready(),"exact {extra:#x} starts cold");
            let selected=r.pipelines.dry_material.dispatch_kernel(&style,MaterialOperation::Deposit,false,extra);
            assert!(std::ptr::eq(selected.compile(),class.compile()),"cold exact {extra:#x} uses its own tip class");
            let dab=Dab {center:layer_core::Point{x:64.,y:64.},radii:[radius;2],previous:[radius,radius,1.,0.],motion:[4.,0.],contact:[0.7,0.,0.,0.],previous_contact:[0.7,0.,0.,0.],..crate::layer_tests::dab([0.1,0.1,0.1,1.])};
            let mut batch=crate::test_support::dab_batch(doc.working.target.unwrap(),style.clone(),layer_core::Rect::from_extent([128;2]));
            batch.dab_count=1;
            batch.stroke_end=false;
            let packet=FramePacket {dabs:&[dab],dab_batches:&[batch],..crate::test_support::packet(doc.scene(),[128;2])};
            assert!(r.raster_dependencies_ready(packet,None));
            assert!(!preferred.ready(),"frame admission requires no exact {extra:#x}");
            r.submit(packet).unwrap();
            assert!(!preferred.ready(),"dispatch does not compile exact {extra:#x}");
            r.compilation.blocking(||preferred.compile());
            let selected=r.pipelines.dry_material.dispatch_kernel(&style,MaterialOperation::Deposit,false,extra);
            assert!(std::ptr::eq(selected.compile(),preferred.compile()),"a ready exact recipe wins");
        }
        drop(r);
        drop(reference);
        finish_shader_compiler_shutdown();
    }

    #[test]
    fn retouching_kernels_compile_before_pen_down() {
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, color);
        let document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        for preset in [layer_core::DefaultBrushPreset::CloneStamp, layer_core::DefaultBrushPreset::HealingBrush] {
            let brush = layer_core::default_brush(preset);
            renderer.prepare_startup(&document, &brush, false).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("{preset:?} compilation timed out"));
            let style = layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush);
            let pipelines = &renderer.pipelines;
            let prediction = pipelines.dry_material.selected_kernel(&style, MaterialOperation::Clone, false,0);
            let commit = pipelines.dry_material.selected_kernel(&style, MaterialOperation::Clone, true,0);
            assert!(prediction.ready() && commit.ready(), "{preset:?} commit and prediction kernels are ready before input");
        }
    }

    #[test]
    fn changing_the_blend_mode_prepares_its_kernel_before_painting() {
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
        let document = Document::new(layer_core::authored::PortableId::random(), 128, 128,
            layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let target = document.scene().targets().find(|target| matches!(target, SourceTarget::Paint(_))).unwrap();
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        for (step, mode) in [BrushBlendMode::Normal, BrushBlendMode::Multiply, BrushBlendMode::Screen, BrushBlendMode::Normal].into_iter().enumerate() {
            brush.rendering.blend_mode = mode;
            assert!(renderer.startup_needs_update(&document, &brush, false), "route change must invalidate readiness");
            renderer.prepare_startup(&document, &brush, false).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready,
                format_args!("{mode:?} brush preparation timed out"));
            let style = layer_render::DabStyle { blend_space: document.composition().blend,
                ..layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush) };
            let plan = BrushPassPlan::for_device(&style, &renderer.device);
            assert!(renderer.pipelines.dry_material.selected_kernel(&style, plan.material, true, 0).ready(),
                "{mode:?} destination kernel must be compiled before painting");
            let mut frame = crate::test_support::packet(document.scene(), [128; 2]);
            frame.blend_space = document.composition().blend;
            frame.reset_layers = step == 0;
            renderer.submit(frame).unwrap();
            let before = renderer.readback_srgb_rgba8().unwrap();
            let x = 24 + step as u32 * 26;
            let mut dab = crate::tests::test_dab([x as f32, 64.], [0.2, 0.4, 0.8, 1.], 1.);
            dab.radii = [8.; 2];
            dab.previous = [8., 8., 1., 0.];
            dab.contact = [1., 0., 0., 0.];
            dab.previous_contact = dab.contact;
            let batch = DabBatch { stroke_id: layer_core::StrokeId(step as u64 + 1),
                ..crate::test_support::dab_batch(target, style, layer_core::Rect::from_extent([128; 2])) };
            renderer.submit(FramePacket { dabs: &[dab], dab_batches: std::slice::from_ref(&batch),
                blend_space: document.composition().blend, ..crate::test_support::packet(document.scene(), [128; 2]) }).unwrap();
            let after = renderer.readback_srgb_rgba8().unwrap();
            let index = (64 * 128 + x as usize) * 4;
            assert!((0..3).any(|channel| before[index + channel] != after[index + channel]),
                "{mode:?}: ready brush must change actual pixels");
            assert_eq!(after[index + 3], 255, "{mode:?}: opaque paper remains visible");
        }
    }
}

#[cfg(test)]
mod mc_transform_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod mc_context_tests;

#[cfg(test)]
mod visible_filter_tests;
