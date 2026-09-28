//! Cold-start dependency ordering shared by native and browser compilers. The compiler owns only cloned GPU
//! handles and recipes, never the session, swapchain, input queue or live pixels.
use super::*;
use layer_core::{BrushSnapshot, Document, StrokeTool};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

const DOCUMENT: u8 = 2;
pub(super) const BRUSH: u8 = 3;
pub(super) const VALIDATION: u8 = 4;
pub(super) const OTHER: u8 = 5;
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
    id: Arc<str>,
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
    operations: bool,
    transform: bool,
    mesh: bool,
    blend_space: layer_core::BlendSpace,
    chains: Vec<(Vec<Arc<layer_core::EffectProgram>>, effects::Execution)>,
}
impl DocumentKey {
    fn new(document: &Document) -> Self {
        Self {
            extent: [document.width, document.height], color: document.color,
            selection: document.selection.is_some(), mask: document.active_mask,
            locked: document.layers.iter().any(|l| l.id == document.active_layer && l.properties.alpha_locked),
            source: document.layers.iter().any(|l| l.source.is_some()),
            operations: document.layers.iter().any(|l| l.mask.is_some() || !l.pending_operations.is_empty()),
            transform: document.layers.iter().any(|l| l.pending_operations.iter()
                .chain(l.masks().flat_map(|m| m.pending_operations.iter()))
                .any(|op| matches!(op.kind, layer_core::LayerOperationKind::Transform(_)))),
            mesh: document.layers.iter().any(|l| l.pending_operations.iter()
                .chain(l.masks().flat_map(|m| m.pending_operations.iter()))
                .any(|op| matches!(&op.kind, layer_core::LayerOperationKind::Transform(t)
                    if matches!(t.map, layer_core::TransformMap::Mesh(_))))),
            blend_space: document.blend_space,
            chains: scene::startup_effect_chains(&document.layers).into_iter()
                .map(|(layers, execution)| (layers.into_iter().filter_map(|l| l.effect.as_ref().map(|e| e.program.clone())).collect(), execution)).collect(),
        }
    }
}
impl ShaderDocument {
    pub fn new(document: &Document) -> Self {
        Self { id: document.id.clone(), revision: std::cell::Cell::new(document.revision), key: DocumentKey::new(document) }
    }
    pub fn matches(&self, document: &Document) -> bool {
        if self.id != document.id { return false; }
        if self.revision.get() == document.revision { return true; }
        if self.key != DocumentKey::new(document) { return false; }
        self.revision.set(document.revision);
        true
    }
}
#[derive(Default)]
struct Requirements {
    render: Vec<Deferred<wgpu::RenderPipeline>>,
    compute: Vec<Deferred<wgpu::ComputePipeline>>,
}
impl Requirements {
    fn ready(&self) -> bool {
        self.render.iter().all(Deferred::ready) && self.compute.iter().all(Deferred::ready)
    }
    fn enqueue(&self, compiler: &Compiler, priority: u8) {
        compiler.require(&self.render, priority);
        compiler.require(&self.compute, priority);
    }
    fn style(
        &mut self,
        r: &WgpuRasterizer,
        style: &layer_render::DabStyle,
        mask: bool,
        preview: bool,
    ) {
        if style.selection.is_some() {
            self.compute.extend(r.selection_clip.pipelines().map(Clone::clone));
        }
        if mask {
            let index = usize::from(matches!(style.tip, BrushTip::Mask(_))) * 2
                + usize::from(style.mode == DabMode::Erase);
            self.render.push(r.layer_masks.brush[index].clone());
            return;
        }
        let plan = BrushPassPlan::for_device(style, &r.device);
        if pointwise(style) && plan.direct.is_none() {
            let kernels = &r.pipelines.dry_material;
            self.compute.push(kernels.kernel(style, plan.material, plan.state.coverage).clone());
            if preview { self.compute.push(kernels.kernel(style, plan.material, false).clone()); }
            if preview && dry_material::display_preview_eligible(style) {
                self.compute.push(r.pipelines.dry_display.kernel(style, plan.material, false).clone());
            }
            if let Some(in_place) = &r.pipelines.dry_in_place {
                self.compute.push(in_place.kernel(style, plan.material, plan.state.coverage).clone());
            }
        }
        if let Some(kind) = plan.direct {
            self.render.push(r.pipelines.direct[kind as usize].clone());
        } else {
            let kind = MaterialPipelineKind::for_attachments(
                plan.state.watercolor_wetness,
                plan.state.coverage,
                plan.state.canvas_wetness,
            );
            self.render
                .push(r.pipelines.material[kind.index(plan.material)].clone());
            if preview {
                let variant = MaterialPipelineKind::for_attachments(
                    plan.state.watercolor_wetness,
                    plan.state.coverage,
                    false,
                );
                self.render
                    .push(r.pipelines.material[variant.index(plan.material)].clone());
                // A single predicted destination batch on a simple canvas reads
                // persistent coverage but writes only its disposable color.
                if !plan.state.watercolor_wetness {
                    self.render.push(
                        r.pipelines.material[MaterialPipelineKind::Color.index(plan.material)]
                            .clone(),
                    );
                }
            }
        }
        if plan.reservoir {
            self.render.push(r.pipelines.reservoir.clone());
        }
        if let Some(index) = match style.execution {
            BrushExecution::Liquify => Some(0),
            BrushExecution::Smudge => Some(1),
            _ => None,
        } {
            self.render.push(r.pipelines.material_gather[index].clone());
        }
        if plan.stroke_edge {
            self.render.push(r.pipelines.stroke_edge.clone());
        }
        if plan.state.watercolor_wetness {
            self.render
                .extend(r.pipelines.watercolor_transport.iter().cloned());
            self.render.push(r.pipelines.watercolor_composite.clone());
        }
    }
}
pub(super) struct Startup {
    pub compiler: Compiler,
    masks: builtin_masks::Masks,
    document_key: Option<ShaderDocument>,
    brush: Option<ShaderBrushKey>,
    transform: bool,
    document: Requirements,
    current: Requirements,
    effects: Option<mpsc::Receiver<Result<effects::Effects, String>>>,
    effects_ready: bool,
    pub(super) host_catalog_pending: bool,
    pub(super) finished: bool,
}
#[derive(PartialEq, Eq)]
struct ShaderBrushKey {
    textures: TextureSetKey,
    mask_tip: bool,
    execution: BrushExecution,
    paint: BrushPassPlan,
    erase: BrushPassPlan,
    dry_material: u32,
}
impl ShaderBrushKey {
    fn new(brush: &BrushSnapshot) -> Self {
        let mut style = layer_render::DabStyle::for_brush(brush, StrokeTool::Brush);
        let paint = BrushPassPlan::for_style(&style);
        style.mode = DabMode::Erase;
        let erase = BrushPassPlan::for_style(&style);
        Self {
            textures: WgpuRasterizer::texture_set_key(&style),
            mask_tip: matches!(style.tip, BrushTip::Mask(_)),
            execution: style.execution,
            paint,
            erase,
            dry_material: dry_material::contact_flags(style.contact)
                | if style.rendering.accumulation == BrushAccumulation::Uniform { 128 } else { 0 },
        }
    }
}
impl Startup {
    pub fn new(device: &PipelineDevice) -> Result<Self, GpuRasterError> {
        #[cfg(not(target_arch = "wasm32"))]
        let compiler = {
            let _ = device;
            Compiler::new()?
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
            effects: None,
            effects_ready: false,
            host_catalog_pending: false,
            finished: false,
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
    pub fn shader_input(&self) {
        if let Some(startup) = &self.startup { startup.compiler.input(); }
    }
    pub fn shader_idle(&self, idle: bool) {
        if let Some(startup) = &self.startup { startup.compiler.idle(idle); }
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
    /// Call after the host has submitted its initial filter catalog. Saving runs
    /// behind all shader jobs, never on the canvas/input thread. The worker drops
    /// the driver's cache after saving; runtime edits cannot keep growing it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn finish_startup_cache(&mut self) {
        if let Some(startup) = &mut self.startup {
            if !startup.host_catalog_pending {
                return;
            }
            startup.host_catalog_pending = false;
            let device = self.device.clone();
            startup.compiler.enqueue(OTHER + 1, move || {
                device.finish_cache();
                Ok(())
            });
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
                || s.brush.as_ref() != Some(&ShaderBrushKey::new(brush))
                || s.transform != transform
        })
    }
    /// Called after the blank canvas has been submitted. Document, brush and
    /// live-transform dependencies precede speculative compilation. What a
    /// transform or warp draws with then compiles while input is quiet, so
    /// opening one later need not wait for it. Hosts pass the engine's
    /// preview presence before draining an interactive frame.
    pub fn prepare_startup(
        &mut self,
        document: &Document,
        brush: &BrushSnapshot,
        transform: bool,
    ) -> Result<(), GpuRasterError> {
        let Some(mut startup) = self.startup.take() else {
            return Ok(());
        };
        startup.finished = false;
        if startup.document_key.as_ref().is_none_or(|key| !key.matches(document)) {
            let shader = ShaderDocument::new(document);
            let mut required = Requirements::default();
            required
                .render
                .extend(self.scene_pipelines.pipeline.iter().cloned());
            if self.device.portable_blend() { required.compute.extend(self.portable_blend.pipelines.iter().cloned()); }
            required.compute.push(self.scene_pipelines.constant.2.clone());
            if self.native_edit.is_some() {
                required.compute.push(self.scene_pipelines.scale.reduce.clone());
                required.compute.push(self.scene_pipelines.scale.reduce_pair.clone());
                required.compute.push(self.scene_pipelines.scale.compose.clone());
                required.compute.push(self.scene_pipelines.resample.area.clone());
            }
            if self.native_edit.as_ref().is_some_and(|native| {
                u64::from(document.width) * u64::from(document.height) * 16 > native.display_dense_bytes
            }) {
                let mip = self.display_pipelines
                    .get_or_insert_with(|| display_mips::Pipelines::new(&self.device));
                required.compute.push(mip.reduce.clone());
                required.compute.push(mip.fused_reduce.clone());
            }
            if shader.key.source {
                required.render.push(self.scene_pipelines.source.pipeline.clone());
            }
            if shader.key.operations {
                required.render.push(self.layer_masks.initialize.clone());
                required.compute.extend(self.selection_clip.pipelines().map(Clone::clone));
            }
            if shader.key.mesh {
                required.render.extend(self.transforms.as_ref().unwrap().mesh_pipelines().into_iter().cloned());
            }
            if shader.key.transform {
                required.render.extend(self.transforms.as_ref().unwrap().pipelines().into_iter().cloned());
            }
            required.enqueue(&startup.compiler, DOCUMENT);
            startup.document = required;
            startup.document_key = Some(shader);
            let chains = scene::startup_effect_chains(&document.layers);
            let cached = self.scene.as_ref().map(|s| &s.effects).or(self.validated_effects.as_ref());
            let blend_space = document.blend_space;
            if !chains.iter().all(|(layers, execution)| cached.is_some_and(|cache| cache.chain_ready(layers, *execution, blend_space))) {
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
                let chains: Vec<_> = chains.into_iter().map(|(layers, execution)|
                    (layers.into_iter().cloned().collect::<Vec<_>>(), execution)).collect();
                let work = move || {
                    let result = (|| {
                        for (layers, execution) in chains {
                            candidate.prepare(
                                &gpu,
                                &layers.iter().collect::<Vec<_>>(),
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
        // Native publication must be ready before accepting a brush contact,
        // but blank paper does not depend on writeback/promotion/validation.
        if let Some(native) = &self.native_edit {
            current.compute.extend(native.required_pipelines(document.color.depth).cloned());
        }
        let locked = startup.document_key.as_ref().unwrap().key.locked;
        // A physical eraser can select the erase variant without a UI change.
        for tool in [StrokeTool::Brush, StrokeTool::Eraser] {
            let style = layer_render::DabStyle {
                alpha_locked: locked,
                selection: document.selection.clone().map(Arc::new),
                blend_space: if document.active_mask { layer_core::BlendSpace::Linear } else { document.blend_space },
                ..layer_render::DabStyle::for_brush(brush, tool)
            };
            startup.masks.style(&startup.compiler, &style, BRUSH);
            current.style(self, &style, document.active_mask, true);
        }
        // Live transforms do not change document revision or brush settings.
        // They still need their own shaders before an interactive frame runs.
        if transform {
            let mip = self.display_pipelines
                .get_or_insert_with(|| display_mips::Pipelines::new(&self.device));
            current.compute.extend([mip.reduce.clone(), mip.fused_reduce.clone()]);
            current.render.extend(self.transforms.as_ref().unwrap().pipelines().into_iter().cloned());
            current.compute.extend(self.transforms.as_ref().unwrap().display_pipelines().into_iter().cloned());
            current.compute.extend(self.scene_pipelines.resample.mapped.iter().cloned());
            current.render.extend(self.scene_pipelines.resample.mesh.iter().cloned());
            current.compute.extend(self.selection_clip.pipelines().map(Clone::clone));
        }
        if let Some(retouch) = self.retouch.as_ref().filter(|r| r.prepared()) {
            let (render, compute) = retouch.pipelines();
            current.render.extend(render);
            current.compute.extend(compute);
        }
        current.enqueue(&startup.compiler, BRUSH);
        let transforms = self.transforms.as_ref().unwrap();
        startup.compiler.require(transforms.pipelines(), OTHER);
        startup.compiler.require(self.selection_clip.pipelines(), OTHER);
        startup.compiler.require(transforms.display_pipelines().into_iter().chain(self.scene_pipelines.resample.mapped.iter()).chain([&self.scene_pipelines.resample.area]), OTHER);
        startup.compiler.require(transforms.mesh_pipelines(), OTHER);
        startup.compiler.require(&self.scene_pipelines.resample.mesh, OTHER);
        let mip = self.display_pipelines
            .get_or_insert_with(|| display_mips::Pipelines::new(&self.device));
        startup.compiler.require([&mip.reduce, &mip.fused_reduce], OTHER);
        startup.current = current;
        startup.brush = Some(ShaderBrushKey::new(brush));
        startup.transform = transform;
        startup.compiler.start();
        self.startup = Some(startup);
        Ok(())
    }
    pub fn poll_startup(&mut self) -> Result<StartupProgress, GpuRasterError> {
        if let Some(startup) = &mut self.startup {
            let masks = startup.masks.take_ready()?;
            for (id, (width, height, pixels)) in masks {
                self.upload_mask(&id, width, height, width, &pixels)?;
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
            canvas_ready && startup.current.ready() && startup.masks.ready_through(BRUSH);
        #[cfg(target_arch = "wasm32")]
        let brush_ready = brush_ready && startup.compiler.ready_through(BRUSH);
        startup.finished = brush_ready
            && !startup.host_catalog_pending
            && startup.compiler.pending() == 0
            && startup.masks.ready_through(OTHER);
        let progress = StartupProgress {
            canvas_ready,
            brush_ready,
            complete: startup.finished,
        };
        if canvas_ready && self.scene.is_none() {
            self.scene = Some(scene::Scene::new(self));
        }
        Ok(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn brush_shader_key_ignores_color_and_size_but_tracks_pipeline_changes() {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        let key = ShaderBrushKey::new(&brush);
        brush.color_rgba_linear = [8., 0.25, 0.5, 1.];
        brush.diameter *= 2.;
        assert!(key == ShaderBrushKey::new(&brush));
        brush.rendering.accumulation = if brush.rendering.accumulation == BrushAccumulation::Uniform {
            BrushAccumulation::Flow
        } else {
            BrushAccumulation::Uniform
        };
        assert!(key != ShaderBrushKey::new(&brush));
    }
    #[test]
    fn shader_document_reuses_raster_and_parameter_edits_but_tracks_new_dependencies() {
        let mut doc = Document::new("readiness", 128, 128);
        let key = ShaderDocument::new(&doc);
        doc.layers[0].opacity = 0.5;
        doc.revision += 1;
        assert!(key.matches(&doc));
        doc.selection = Some(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 64., y: 0. },
            layer_core::Point { x: 0., y: 64. },
        ]).unwrap());
        doc.revision += 1;
        assert!(!key.matches(&doc));
        let key = ShaderDocument::new(&doc);
        doc.width *= 2;
        doc.revision += 1;
        assert!(!key.matches(&doc));
        let mut layer = Layer::paint(LayerId(10), "curves");
        layer.kind = LayerKind::Effect;
        layer.effect = Some(Arc::new(layer_core::EffectInstance::new(
            layer_core::bundled_effect_catalog().get("curves").unwrap().program(),
        )));
        doc.layers.insert(0, layer);
        doc.revision += 1;
        let key = ShaderDocument::new(&doc);
        Arc::make_mut(doc.layers[0].effect.as_mut().unwrap()).values[0] = layer_core::EffectValue::Number(0.5);
        doc.revision += 1;
        assert!(key.matches(&doc), "a parameter edit uses the same pipeline");
        doc.layers[0].visible = false;
        doc.revision += 1;
        assert!(!key.matches(&doc), "visibility changes the fused chains");
        let key = ShaderDocument::new(&doc);
        doc.active_mask = true;
        doc.revision += 1;
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
        assert_eq!(*order.lock().unwrap(), [2, 3, 4]);
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;
    #[test]
    fn native_publication_pipelines_follow_canvas_and_gate_brush() {
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer = WgpuRasterizer::from_wgpu_native_staged(
            reference.adapter.clone(),
            reference.device().clone(),
            reference.queue.clone(),
            color,
        )
        .unwrap();
        renderer.finish_startup_cache();
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
        let document = Document::new("native staged startup", 128, 128);
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        renderer.prepare_startup(&document, &brush, false).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !renderer.poll_startup().unwrap().brush_ready {
            assert!(
                std::time::Instant::now() < deadline,
                "Native brush compilation timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut recolored = brush.clone();
        recolored.color_rgba_linear = [8., 0.25, 0.5, 1.];
        assert!(!renderer.startup_needs_update(&document, &recolored, false));
        assert!(renderer.poll_startup().unwrap().brush_ready);
        assert!(
            renderer
                .scene_pipelines
                .pipeline
                .iter()
                .all(Deferred::ready)
        );
        assert!(
            renderer
                .native_edit
                .as_ref()
                .unwrap()
                .required_pipelines(document.color.depth)
                .all(Deferred::ready)
        );
        for coverage in [false, true] {
            assert!(renderer.pipelines.dry_material
                .kernel(&layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush), MaterialOperation::Coverage, coverage).ready(),
                "G-Pen commit and prediction kernels must be ready before input is enabled");
        }
        while !renderer.poll_startup().unwrap().complete {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(renderer.regions.as_ref().is_none_or(|regions| regions.flood.pipelines().all(|p| !p.ready())),
            "unused region recipes must remain uncompiled");
        let transforms = renderer.transforms.as_ref().unwrap();
        assert!(
            transforms.pipelines().into_iter().chain(transforms.mesh_pipelines()).all(Deferred::ready)
                && transforms.display_pipelines().into_iter().all(Deferred::ready),
            "what transforms and warps draw with compiles while idle after startup"
        );
        assert!(renderer.startup_needs_update(&document, &brush, true),
            "demand readiness continues after initial completion");
        renderer.prepare_startup(&document, &brush, true).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready, "opening a transform waits for no compilation");
        renderer.prepare_startup(&document, &brush, false).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready, "cached dependencies resume immediately");

    }

    #[test]
    fn edged_brushes_in_perceptual_documents_compile_their_deposit_before_pen_down() {
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer =
            WgpuRasterizer::from_wgpu_native_staged(reference.adapter.clone(), reference.device().clone(), reference.queue.clone(), color)
                .unwrap();
        renderer.finish_startup_cache();
        let mut document = Document::new("perceptual startup", 128, 128);
        document.blend_space = layer_core::BlendSpace::Perceptual;
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Airbrush);
        brush.rendering.accumulation = BrushAccumulation::Flow;
        brush.rendering.wet_edge = 0.5;
        renderer.prepare_startup(&document, &brush, false).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !renderer.poll_startup().unwrap().brush_ready {
            assert!(std::time::Instant::now() < deadline, "compilation timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        let style = layer_render::DabStyle { blend_space: document.blend_space, ..layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush) };
        let plan = BrushPassPlan::for_device(&style, &renderer.device);
        assert!(plan.direct.is_none() && plan.material == MaterialOperation::Deposit);
        let pipelines = &renderer.pipelines;
        let commit = pipelines.dry_in_place.as_ref().unwrap_or(&pipelines.dry_material).kernel(&style, plan.material, plan.state.coverage);
        assert!(commit.ready() && pipelines.dry_material.kernel(&style, plan.material, false).ready(), "the deposit kernels are ready before input");
    }

    #[test]
    fn retouching_kernels_compile_before_pen_down() {
        let color = layer_core::color::DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut renderer =
            WgpuRasterizer::from_wgpu_native_staged(reference.adapter.clone(), reference.device().clone(), reference.queue.clone(), color)
                .unwrap();
        renderer.finish_startup_cache();
        let document = Document::new("retouching startup", 128, 128);
        for preset in [layer_core::DefaultBrushPreset::CloneStamp, layer_core::DefaultBrushPreset::HealingBrush] {
            let brush = layer_core::default_brush(preset);
            renderer.prepare_startup(&document, &brush, false).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            while !renderer.poll_startup().unwrap().brush_ready {
                assert!(std::time::Instant::now() < deadline, "{preset:?} compilation timed out");
                std::thread::sleep(Duration::from_millis(1));
            }
            let style = layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush);
            let pipelines = &renderer.pipelines;
            let prediction = pipelines.dry_material.kernel(&style, MaterialOperation::Clone, false);
            let commit = pipelines.dry_in_place.as_ref().unwrap_or(&pipelines.dry_material).kernel(&style, MaterialOperation::Clone, true);
            assert!(prediction.ready() && commit.ready(), "{preset:?} commit and prediction kernels are ready before input");
        }
    }
}
