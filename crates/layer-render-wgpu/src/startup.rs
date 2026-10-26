//! Cold-start dependency ordering shared by native and browser compilers. The compiler owns only cloned GPU
//! handles and recipes, never the session, swapchain, input queue or live pixels.
use super::*;
use layer_core::{BrushSnapshot, Document, StrokeTool};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

pub(super) const DOCUMENT: u8 = 2;
pub(super) const BRUSH: u8 = 3;
pub(super) const VALIDATION: u8 = 4;
pub(super) const OTHER: u8 = 5;
const BACKGROUND: u8 = OTHER + 1;
#[path = "shader_admission.rs"]
mod admission;
#[cfg(not(target_arch = "wasm32"))]
#[path = "startup_native.rs"]
mod platform;
#[cfg(target_arch = "wasm32")]
#[path = "startup_web.rs"]
mod platform;
pub(super) use platform::Compiler;
#[cfg(not(target_arch = "wasm32"))]
pub use platform::finish_shader_compiler_shutdown;
#[cfg(not(target_arch = "wasm32"))]
pub use platform::Activity as ShaderActivity;

#[cfg(all(feature = "test-support", not(target_arch = "wasm32")))]
pub fn enqueue_compiler_failure(renderer: &WgpuRasterizer, cause: &'static str) -> mpsc::Receiver<()> {
    let startup = renderer.startup.as_ref().expect("staged renderer");
    assert!(startup.finished, "startup must complete before the late failure");
    let (recorded, result) = mpsc::channel();
    startup.compiler.enqueue(BRUSH, move || Err(cause.into()));
    startup.compiler.enqueue(BRUSH, move || { let _ = recorded.send(()); Ok(()) });
    startup.compiler.start();
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
pub struct ShaderDocument {
    id: layer_core::authored::PortableId,
    revision: std::cell::Cell<layer_core::Revision>,
    key: DocumentKey,
}
#[derive(PartialEq)]
struct DocumentKey {
    extent: [u32; 2],
    color: layer_core::color::DocumentColor,
    selection: bool,
    mask: bool,
    locked: bool,
    source: bool,
    objects: bool,
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
            locked: !mask && target.and_then(|t| document.target_owner(t)).and_then(|h| scene.occurrence(h)).is_some_and(|o| o.alpha_locked),
            source: objects || scene.targets().any(|t| scene.original(t).is_some()),
            objects,
            vectors: scene.order().iter().any(|&h| scene.vector_layer(h).is_some()),
            operations: scene.order().iter().any(|&h| scene.mask(h).is_some()) || operations().next().is_some(),
            transform: operations().any(|op| matches!(op.kind, layer_core::RasterOperationKind::Transform(_))),
            mesh: operations().any(|op| matches!(&op.kind, layer_core::RasterOperationKind::Transform(t) if t.placement.mesh.is_some())),
            gray: scene.targets().any(|t|scene.color_mode(t)!=layer_core::color::LayerColorMode::FullColor),
            watercolor: scene.targets().any(|t|scene.raster(t).is_some_and(|r|r.try_data().is_none_or(|data|data.is_ok_and(|data|data.watercolor.is_some())))),
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
impl ShaderDocument {
    pub fn new(document: &Document) -> Self {
        Self { id: document.artwork.id, revision: std::cell::Cell::new(document.revision), key: DocumentKey::new(document) }
    }
    pub fn matches(&self, document: &Document) -> bool {
        if self.id != document.artwork.id { return false; }
        if self.revision.get() == document.revision { return true; }
        if self.key != DocumentKey::new(document) { return false; }
        self.revision.set(document.revision);
        true
    }
}
#[derive(Default)]
pub(super) struct Requirements {
    pub render: Vec<Deferred<wgpu::RenderPipeline>>,
    pub compute: Vec<Deferred<wgpu::ComputePipeline>>,
    styles: Vec<layer_render::DabStyle>,
}
impl Requirements {
    pub(super) fn capture_scene(r:&WgpuRasterizer,view:SceneView<'_>,watercolor:bool)->Self {
        let mut required = Self::default();
        required.render.extend(r.scene_pipelines.pipeline.iter().cloned());
        required.compute.push(r.scene_pipelines.constant.2.clone());
        required.compute.extend(r.scene_pipelines.scale_pipelines().into_iter().cloned());
        required.compute.extend(r.transforms.as_ref().unwrap().placement_pipelines());
        if r.device.portable_blend() { required.compute.extend(r.portable_blend.pipelines.iter().cloned()); }
        if watercolor {
            required.compute.push(r.pipelines.watercolor_compute.1.clone());
            required.render.push(r.pipelines.watercolor_composite.clone());
        }
        if view.order().iter().any(|&h| view.object_layer(h).is_some()) {
            required.compute.extend(r.scene_pipelines.objects.pipelines().into_iter().cloned());
            required.render.extend(r.scene_pipelines.source.required().into_iter().cloned());
        }
        if view.order().iter().any(|&h| view.vector_layer(h).is_some()) {
            required.compute.push(r.scene_pipelines.vector.pipeline().clone());
        }
        required
    }
    pub async fn prepare(&self) -> Result<(), String> {
        for pipelines in self.render.chunks(4) {
            Deferred::prepare_all(pipelines.iter()).await?;
        }
        for pipelines in self.compute.chunks(4) {
            Deferred::prepare_all(pipelines.iter()).await?;
        }
        Ok(())
    }
    pub fn ready(&self) -> bool {
        self.render.iter().all(|p| p.ready()) && self.compute.iter().all(|p| p.ready())
    }
    pub fn covers(&self, style: &layer_render::DabStyle) -> bool {
        self.styles.contains(style) && self.ready()
    }
    fn enqueue(&self, compiler: &Compiler, priority: u8) {
        compiler.require(&self.render, priority);
        compiler.require(&self.compute, priority);
    }
    pub(super) fn require(&self, compiler: &Compiler, priority: u8) -> bool {
        let ready = compiler.require(&self.render, priority) & compiler.require(&self.compute, priority);
        if !ready { compiler.start(); }
        ready
    }
    pub(super) fn style(
        &mut self,
        r: &WgpuRasterizer,
        style: &layer_render::DabStyle,
        reach: dry_material::Reach,
        mask: bool,
        preview: bool,
    ) {
        self.styles.push(style.clone());
        if style.selection.is_some() {
            self.compute.extend(r.selection_clip.pipelines().map(Clone::clone));
        }
        if mask {
            let index = layer_masks::MaskRenderer::brush_index(style);
            self.render.push(r.layer_masks.brush[index].clone());
            return;
        }
        if style.execution == BrushExecution::Liquify {
            let pipelines = r.liquify_pipelines.get_or_init(|| Arc::new(liquify::Pipelines::new(r)));
            self.compute.extend([pipelines.update(style).clone(), pipelines.reconstruct.clone(), pipelines.mips.reduce.clone(), pipelines.mips.fused_reduce.clone()]);
            return;
        }
        let plan = BrushPassPlan::for_device(style, &r.device);
        if pointwise(style) && plan.direct.is_none() {
            let pipelines = &r.pipelines.dry_material;
            if plan.material == MaterialOperation::Deposit && dry_material::dry_flow(style)
                && (preview || (!pipelines.in_place() && !plan.state.coverage)) {
                self.compute.push(pipelines.copy_kernel(false).clone());
            }
            if preview && style.rendering.blend_mode == BrushBlendMode::Normal { self.compute.push(pipelines.gather_kernel().clone()); }
            for extra in reach.batch_variants(style) {
                let kernel = |coverage| pipelines.selected_kernel(style, plan.material, coverage, extra).clone();
                self.compute.push(kernel(plan.state.coverage));
                if preview { self.compute.push(kernel(false)); }
            }
        }
        if let Some(kind) = plan.direct {
            self.render.push(r.pipelines.direct[kind as usize].clone());
        } else if destination_material::transported(style) {
            let kernels = &r.pipelines.destination_material;
            self.compute.extend(destination_material::selected(plan, preview).map(|entry| kernels.kernel(style, entry).clone()));
        }
        if plan.stroke_edge {
            self.render.push(r.pipelines.stroke_edge.clone());
        }
        if plan.state.watercolor_wetness {
            self.compute.push(r.pipelines.watercolor_compute.1.clone());
            self.compute
                .extend(r.pipelines.watercolor_transport.iter().cloned());
            self.render.push(r.pipelines.watercolor_composite.clone());
        }
    }
}
#[cfg(not(target_arch="wasm32"))]
pub(crate) type CompilerHandle = Arc<Compiler>;
#[cfg(target_arch="wasm32")]
pub(crate) type CompilerHandle = Compiler;

pub(super) struct Startup {
    pub compiler: CompilerHandle,
    pub(super) masks: builtin_masks::Masks,
    document_key: Option<ShaderDocument>,
    brush: Option<ShaderBrushKey>,
    transform: bool,
    pub(super) document: Requirements,
    pub(super) current: Requirements,
    resized: Requirements,
    preparation: Arc<std::sync::atomic::AtomicU64>,
    effects: Option<mpsc::Receiver<Result<effects::Effects, String>>>,
    effects_ready: bool,
    pub(super) host_catalog_pending: bool,
    pub(super) finished: bool,
    pub(super) brush_ready: bool,
}
#[derive(PartialEq, Eq)]
struct ShaderBrushKey {
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
    fn new(document: &Document, brush: &BrushSnapshot) -> Self {
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
    pub fn new(device: &PipelineDevice) -> Result<Self, GpuRasterError> {
        #[cfg(not(target_arch = "wasm32"))]
        let compiler = {
            let _ = device;
            Arc::new(Compiler::new()?)
        };
        #[cfg(target_arch = "wasm32")]
        let compiler = Compiler::new(device)?;
        Ok(Self {
            compiler,
            masks: builtin_masks::Masks::new(),
            document_key: None,
            brush: None,
            transform: false,
            document: Requirements::default(),
            current: Requirements::default(),
            resized: Requirements::default(),
            preparation: Default::default(),
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
    pub(super) fn scene_scale_ready(&self) -> bool {
        self.startup.as_ref().is_none_or(|startup| {
            let ready = startup.compiler.require(self.scene_pipelines.scale_pipelines(), DOCUMENT);
            if !ready { startup.compiler.start(); }
            ready
        })
    }
    pub fn shader_input(&self) {
        if let Some(startup) = &self.startup { startup.compiler.input(); }
    }
    pub fn shader_idle(&self, idle: bool, speculative_idle: bool) {
        if let Some(startup) = &self.startup { startup.compiler.idle(idle, speculative_idle); }
    }
    pub fn shader_wait_ms(&self) -> f64 {
        self.startup.as_ref().map_or(0., |s| s.compiler.delay().as_secs_f64() * 1000.)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn shader_activity(&self) -> Option<ShaderActivity> {
        self.startup.as_ref().map(|s| s.compiler.activity())
    }
    #[cfg(target_arch = "wasm32")]
    pub fn compile_startup_step(
        &self,
        allow_optional: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), GpuRasterError>>>> {
        self.startup.as_ref().map_or_else(
            || {
                Box::pin(async { Ok(()) })
                    as std::pin::Pin<
                        Box<dyn std::future::Future<Output = Result<(), GpuRasterError>>>,
                    >
            },
            |startup| startup.compiler.step(allow_optional),
        )
    }
    #[cfg(target_arch = "wasm32")]
    pub fn shader_work_pending(&self, allow_optional: bool) -> bool {
        self.startup
            .as_ref()
            .is_some_and(|s| s.compiler.has_work(allow_optional))
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
        let Some(mut startup) = self.startup.take() else {
            return Ok(());
        };
        let prepared = self.prepare_startup_into(&mut startup, document, brush, transform);
        self.startup = Some(startup);
        prepared
    }
    fn prepare_startup_into(
        &mut self,
        startup: &mut Startup,
        document: &Document,
        brush: &BrushSnapshot,
        transform: bool,
    ) -> Result<(), GpuRasterError> {
        startup.finished = false;
        startup.brush_ready = false;
        if startup.document_key.as_ref().is_none_or(|key| !key.matches(document)) {
            let shader = ShaderDocument::new(document);
            let mut required = Requirements::default();
            required
                .render
                .extend(self.scene_pipelines.pipeline.iter().cloned());
            if self.device.portable_blend() { required.compute.extend(self.portable_blend.pipelines.iter().cloned()); }
            required.compute.push(self.scene_pipelines.constant.2.clone());
            if shader.key.watercolor {required.compute.push(self.pipelines.watercolor_compute.1.clone());}
            required.compute.extend(self.transforms.as_ref().unwrap().placement_pipelines());
            if self.native_edit.is_some() {
                let mip = self.display_pipelines.get_or_insert_with(|| display_mips::Pipelines::new(&self.device));
                required.compute.extend([mip.reduce.clone(), mip.fused_reduce.clone()]);
                required.compute.extend(self.scene_pipelines.scale_pipelines().into_iter().cloned());
            }
            if self.native_edit.is_some() || shader.key.source {
                required.render.extend(self.scene_pipelines.source.required().into_iter().cloned());
            }
            if shader.key.objects {
                required.compute.extend(self.scene_pipelines.objects.pipelines().map(Clone::clone));
                required.compute.extend(self.moving_images.pipelines(&self.device).map(Clone::clone));
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
            required.enqueue(&startup.compiler, DOCUMENT);
            startup.document = required;
            startup.document_key = Some(shader);
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
                let gpu = effects::Context {
                    device: self.device.clone(),
                    queue: self.queue.clone(),
                };
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
                startup.compiler.enqueue(DOCUMENT, work);
                #[cfg(target_arch = "wasm32")]
                startup.compiler.enqueue_async(DOCUMENT, work);
                startup.effects = Some(rx);
                startup.effects_ready = false;
            } else {
                startup.effects = None;
                startup.effects_ready = true;
            }
        }
        let mut current = Requirements::default();
        let mut resized = Requirements::default();
        // Native publication must be ready before accepting a brush contact,
        // but blank paper does not depend on writeback/promotion/validation.
        if let Some(native) = &self.native_edit {
            current.compute.extend(native.required_pipelines(document.composition().color.depth,startup.document_key.as_ref().unwrap().key.gray).cloned());
        }
        // A physical eraser can select the erase variant without a UI change.
        for tool in [StrokeTool::Brush, StrokeTool::Eraser] {
            let style = layer_render::DabStyle {
                selection: document.working.selection.clone().map(Arc::new),
                ..brush_style(document, brush, tool)
            };
            startup.masks.style(&startup.compiler, &style, BRUSH);
            if let Some(binding) = style.grain.and(style.paper.as_ref()) {
                self.papers.require(&self.device, Some(&startup.compiler), binding, BRUSH)?;
            }
            let mask = startup.document_key.as_ref().unwrap().key.mask;
            current.style(self, &style, dry_material::Reach::of(brush), mask, true);
            resized.style(self, &style, dry_material::Reach::of(brush).any_size(), mask, true);
        }
        // Live transforms do not change document revision or brush settings.
        // They still need their own shaders before an interactive frame runs.
        if transform {
            current.render.extend(self.transforms.as_ref().unwrap().pipelines());
            current.compute.extend(self.transforms.as_ref().unwrap().display_pipelines());
            current.render.extend(self.scene_pipelines.resample.mesh.iter().cloned());
            current.render.extend(self.scene_pipelines.fine_resample.mesh.iter().cloned());
            current.compute.extend(self.selection_clip.pipelines().map(Clone::clone));
        }
        if let Some(retouch) = self.retouch.as_ref().filter(|r| r.prepared()) {
            let (render, compute) = retouch.pipelines();
            current.render.extend(render);
            current.compute.extend(compute);
        }
        current.enqueue(&startup.compiler, BRUSH);
        startup.current = current;
        startup.resized = resized;
        startup.preparation.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        startup.brush = Some(ShaderBrushKey::new(document, brush));
        startup.transform = transform;
        startup.compiler.start();
        Ok(())
    }
    pub fn poll_startup(&mut self) -> Result<StartupProgress, GpuRasterError> {
        if let Some(startup) = &mut self.startup {
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
                .chain(self.paper_lighting.iter().map(|lit| (lit.binding.clone(), OTHER))).collect();
            for (binding, priority) in shaded {
                self.require_paper_slope(&binding, priority)?;
            }
        }
        let Some(startup) = &mut self.startup else {
            return Ok(StartupProgress::COMPLETE);
        };
        startup.compiler.check()?;
        if let Some(rx) = &startup.effects {
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
        let canvas_ready = startup.document_key.is_some()
            && startup.effects_ready
            && startup.document.ready()
            && (!startup.transform || startup.current.ready())
            && startup.masks.ready_through(DOCUMENT);
        #[cfg(target_arch = "wasm32")]
        let canvas_ready = canvas_ready && startup.compiler.ready_through(DOCUMENT);
        let brush_ready =
            canvas_ready && startup.current.ready() && startup.masks.ready_through(BRUSH) && self.papers.ready_through(BRUSH);
        #[cfg(target_arch = "wasm32")]
        let brush_ready = brush_ready && startup.compiler.ready_through(BRUSH);
        startup.brush_ready = brush_ready;
        startup.finished = brush_ready
            && !startup.host_catalog_pending
            && startup.compiler.pending() == 0;
        if startup.finished {
            let generation = startup.preparation.load(std::sync::atomic::Ordering::Relaxed);
            for pipeline in std::mem::take(&mut startup.resized.compute).into_iter().filter(|p| !p.ready()) {
                let preparation = startup.preparation.clone();
                startup.compiler.background(&pipeline, BACKGROUND, move || preparation.load(std::sync::atomic::Ordering::Relaxed) == generation);
            }
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
        compiler.start();
        entry.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(renderer.scene_pipelines.scale_pipelines().iter().all(|pipeline| !pipeline.ready()));
        let packet = FramePacket { reset_layers: true, ..crate::test_support::packet(snapshot.view(), [64; 2]) };
        let submitted = renderer.submitted_updates();
        assert!(!renderer.raster_dependencies_ready(packet, None), "the first backdrop waits for its display recipes before submission");
        assert_eq!(renderer.submitted_updates(), submitted);
        renderer.submit(packet).unwrap();
        assert_eq!(renderer.submitted_updates(), submitted + 1);
        assert!(renderer.object_deferred);
        assert!(renderer.presentation_views().is_none());
        assert!(renderer.scene_pipelines.scale_pipelines().iter().all(|pipeline| !pipeline.ready()));
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while renderer.scene_pipelines.scale_pipelines().iter().any(|pipeline| !pipeline.ready()) {
            renderer.startup.as_ref().unwrap().compiler.check().unwrap();
            assert!(std::time::Instant::now() < deadline, "Backdrop display recipes did not prepare");
            std::thread::sleep(Duration::from_millis(1));
        }
        renderer.submit(packet).unwrap();
        assert!(!renderer.object_deferred);
        assert!(renderer.presentation_views().is_some());
        let pixels = crate::test_support::float_pixels(&renderer, crate::test_support::document_texture(&renderer));
        assert_eq!(pixels.len(), 64 * 64);
        assert!(pixels.iter().all(|pixel| *pixel == [1.; 4]));
    }
    #[test]
    fn shader_document_tracks_object_dependencies_without_tracking_geometry_or_profiles() {
        let mut document=object_document(false);
        let key=ShaderDocument::new(&document);
        assert!(key.key.source && key.key.objects);
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
        assert!(key.matches(&document));
        let stack=document.composition().result;
        let original=document.artwork.stacks.get(stack).unwrap().clone();
        let mut removed=original.clone();removed.entries.clear();
        document.apply(layer_core::Edit::Stack(layer_core::RecordChange::replace(&document.artwork.stacks,stack,Some(removed)).unwrap())).unwrap();
        assert!(!key.matches(&document));
        let key=ShaderDocument::new(&document);
        assert!(!key.key.source && !key.key.objects);
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
        assert!(!renderer.startup.as_ref().unwrap().current.covers(&style));
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
        document.set_paper_texture(layer_core::TextureSource::Image(layer_core::Image::new(Arc::new(builder.finish().unwrap()))));
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        assert!(renderer.prepare_startup(&document, &brush, false).is_err());
        assert!(renderer.startup.is_some());
        assert!(!renderer.poll_startup().unwrap().complete);
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
        let compiler = Compiler::new().unwrap();
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
        compiler.pipeline(&other, OTHER);
        compiler.pipeline(&brush, OTHER);
        compiler.pipeline(&brush, BRUSH);
        compiler.pipeline(&document, DOCUMENT);
        assert!(order.lock().unwrap().is_empty()); // First presentation opens the gate.
        compiler.start();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while compiler.pending() != 0 {
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
            let request=crate::object_image_mips::MovingRequest {id:source.id(),source:source.storage().clone(),level:1,nearest:false};
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
        let document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
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
        assert!(
            renderer
                .scene_pipelines
                .pipeline
                .iter()
                .all(|p| p.ready())
        );
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
                let mut required = Requirements::default();
                for tool in [StrokeTool::Brush, StrokeTool::Eraser] {
                    let style = layer_render::DabStyle { blend_space: document.composition().blend,
                        ..layer_render::DabStyle::for_brush(&brush, tool) };
                    required.style(&renderer, &style, dry_material::Reach::of(&brush), false, true);
                }
                assert!(!required.ready(), "unselected {preset:?} must begin cold");
                (preset, required)
            });
        renderer.shader_idle(true, true);
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.complete, format_args!("Startup compilation timed out"));
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
        let meshes: Vec<_> = transforms.mesh_pipelines().into_iter()
            .chain(renderer.scene_pipelines.resample.mesh.iter().cloned())
            .chain(renderer.scene_pipelines.fine_resample.mesh.iter().cloned()).collect();
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
        renderer.prepare_startup(&document, &brush, false).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready, "cached dependencies resume immediately");
        renderer.prepare_startup(&document, &brush, true).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready, "revisiting the transform must reuse its prepared dependencies");
    }

    #[test]
    fn cold_preview_prepares_the_drawn_filter_without_unused_planes() {
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
        let document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        renderer.prepare_startup(&document, &brush, true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("Transform preparation timed out"));
        let target = document.scene().targets().find(|target| matches!(target, SourceTarget::Paint(_))).unwrap();
        let mut preview = layer_render::TransformPreview { transaction: 1, moving: true, target, selection: None,
            transform: layer_core::ImageTransform { placement: layer_core::LayerPlacement { interpolation: layer_core::Interpolation::Bicubic,
                ..layer_core::LayerPlacement::from_affine(layer_core::Affine::translation(layer_core::Point { x: 0.25, y: 0.5 })) }, ..Default::default() } };
        preview.moving = false;
        let mut required = Requirements::default();
        renderer.transforms.as_ref().unwrap().require_frame(&renderer, crate::test_support::packet(document.scene(), [128; 2]), Some(&preview), &mut required);
        assert_eq!(required.render.len(), 1);
        let exact = required.render[0].clone();
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
            let mut required = Requirements::default();
            renderer.transforms.as_ref().unwrap().require_frame(&renderer, crate::test_support::packet(document.scene(), [128; 2]), Some(&preview), &mut required);
            assert_eq!(required.render.len(), 1);
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
            for extra in dry_material::Reach::of(&brush).batch_variants(&style) {
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
    fn dry_brushes_prepare_only_the_variants_their_strokes_can_select() {
        use dry_material::{AGGREGATE, FINE_TIP};
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, color);
        let mut document = Document::new(layer_core::authored::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        let style = layer_render::DabStyle { blend_space: document.composition().blend, ..layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush) };
        let plan = BrushPassPlan::for_device(&style, &renderer.device);
        let ready = |renderer: &WgpuRasterizer, extra: u32| {
            let p = &renderer.pipelines.dry_material;
            [p.selected_kernel(&style, plan.material, plan.state.coverage, extra),
                p.selected_kernel(&style, plan.material, false, extra)].iter().all(|k| k.ready())
        };
        renderer.shader_idle(false, false);
        renderer.prepare_startup(&document, &brush, false).unwrap();
        crate::test_support::wait_startup(&mut renderer, deadline, |p| p.brush_ready, format_args!("small Pencil"));
        assert!(ready(&renderer, AGGREGATE | FINE_TIP));
        assert!(!ready(&renderer, AGGREGATE), "a small tip never selects the coarse kernel");
        assert!(!ready(&renderer, FINE_TIP), "single-pigment strokes never select the mixed-pigment kernel");
        let mut any_size = Requirements::default();
        for tool in [StrokeTool::Brush, StrokeTool::Eraser] {
            let style = layer_render::DabStyle { blend_space: document.composition().blend, ..brush_style(&document, &brush, tool) };
            any_size.style(&renderer, &style, dry_material::Reach::of(&brush).any_size(), false, true);
        }
        renderer.shader_idle(true, true);
        crate::test_support::wait_startup(&mut renderer, deadline, |_| any_size.ready(),
            format_args!("an idle renderer prepares the other tip size in the background"));
        brush.diameter = 36.;
        assert!(renderer.startup_needs_update(&document, &brush, false), "a coarse-capable size prepares again before input");
        renderer.prepare_startup(&document, &brush, false).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready, "the background preparation covers the new size");
        assert!(ready(&renderer, AGGREGATE) && ready(&renderer, AGGREGATE | FINE_TIP));
        assert!(!ready(&renderer, FINE_TIP));
        document.working.selection = Some(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 64., y: 0. }, layer_core::Point { x: 0., y: 64. },
        ]).unwrap());
        document.revision += 1;
        renderer.prepare_startup(&document, &brush, false).unwrap();
        crate::test_support::wait_startup(&mut renderer, deadline, |p| p.brush_ready, format_args!("selected Pencil"));
        let selected = layer_render::DabStyle { selection: document.working.selection.clone().map(Arc::new), ..style.clone() };
        let committed = &renderer.pipelines.dry_material;
        assert!([FINE_TIP, 0].into_iter().all(|extra| committed.selected_kernel(&selected, plan.material, plan.state.coverage, extra).ready()),
            "a selection clips each dab, so its strokes use the mixed-pigment kernels");
        drop(renderer);
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
