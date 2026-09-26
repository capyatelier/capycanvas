//! Cold-start dependency ordering shared by native and browser compilers. The compiler owns only cloned GPU
//! handles and recipes, never the session, swapchain, input queue or live pixels.
use super::*;
use layer_core::{BrushSnapshot, Document, StrokeTool};
use std::sync::Arc;
#[cfg(all(test, not(target_arch = "wasm32")))]
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
        if style.execution == BrushExecution::Dry && plan.direct.is_none()
            && let Some(dry) = &r.pipelines.dry_material {
            let kernels = dry.for_style(style);
            self.compute.push(kernels[plan.material as usize * 2 + usize::from(plan.state.coverage)].clone());
            if preview { self.compute.push(kernels[plan.material as usize * 2].clone()); }
            if let Some(in_place) = &r.pipelines.dry_in_place {
                self.compute.push(in_place.for_style(style)[plan.material as usize * 2 + usize::from(plan.state.coverage)].clone());
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
    brush: Option<BrushSnapshot>,
    transform: bool,
    document: Requirements,
    current: Requirements,
    effects: Option<mpsc::Receiver<Result<effects::Effects, String>>>,
    effects_ready: bool,
    pub(super) host_catalog_pending: bool,
    pub(super) finished: bool,
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
                || s.brush.as_ref() != Some(brush)
                || s.transform != transform
        })
    }
    /// Called after the blank canvas has been submitted. Document, brush and
    /// live-transform dependencies precede speculative compilation. Hosts pass
    /// the engine's preview presence before draining an interactive frame.
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
            if let Some((_, _, pipeline)) = &self.scene_pipelines.constant { required.compute.push(pipeline.clone()); }
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
            if shader.key.transform {
                required.render.extend(self.transforms.as_ref().unwrap().pipelines().into_iter().cloned());
            }
            required.enqueue(&startup.compiler, DOCUMENT);
            startup.document = required;
            startup.document_key = Some(shader);
            let chains = scene::startup_effect_chains(&document.layers);
            let cached = self.scene.as_ref().map(|s| &s.effects).or(self.validated_effects.as_ref());
            if !chains.iter().all(|(layers, execution)| cached.is_some_and(|cache| cache.chain_ready(layers, *execution))) {
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
                ..layer_render::DabStyle::for_brush(brush, tool)
            };
            startup.masks.style(&startup.compiler, &style, BRUSH);
            current.style(self, &style, document.active_mask, true);
        }
        // Live transforms do not change document revision or brush settings.
        // They still need their own shaders before an interactive frame runs.
        if transform {
            current.render.extend(self.transforms.as_ref().unwrap().pipelines().into_iter().cloned());
            current.compute.extend(self.selection_clip.pipelines().map(Clone::clone));
        }
        current.enqueue(&startup.compiler, BRUSH);
        startup.current = current;
        startup.brush = Some(brush.clone());
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
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

#[cfg(all(test, not(target_arch = "wasm32")))]
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
        assert!(renderer.pipelines.dry_material.as_ref().unwrap().kernels.iter().all(|p| !p.ready()));
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
        for index in [2, 3] {
            assert!(renderer.pipelines.dry_material.as_ref().unwrap()
                .for_style(&layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush))[index].ready(),
                "G-Pen commit and prediction kernels must be ready before input is enabled");
        }
        while !renderer.poll_startup().unwrap().complete {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(renderer.regions.as_ref().is_none_or(|regions| regions.flood.pipelines().all(|p| !p.ready())),
            "unused region recipes must remain uncompiled");
        assert!(renderer.startup_needs_update(&document, &brush, true),
            "demand readiness continues after initial completion");
        renderer.prepare_startup(&document, &brush, true).unwrap();
        while !renderer.poll_startup().unwrap().brush_ready {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(renderer.transforms.as_ref().unwrap().pipelines().iter().all(|p| p.ready()));
        renderer.prepare_startup(&document, &brush, false).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready, "cached dependencies resume immediately");

    }
    #[test]
    fn region_requests_wait_for_compilation_without_blocking_or_allocating_images() {
        let reference = WgpuRasterizer::new_headless().unwrap();
        let mut renderer = WgpuRasterizer::from_wgpu_staged(
            reference.adapter.clone(),
            reference.device().clone(),
            reference.queue.clone(),
        )
        .unwrap();
        let (release, wait) = mpsc::channel();
        let (entered, blocked) = mpsc::channel();
        renderer
            .startup
            .as_ref()
            .unwrap()
            .compiler
            .enqueue(OTHER, move || {
                entered.send(()).map_err(|e| e.to_string())?;
                wait.recv_timeout(Duration::from_secs(20))
                    .map_err(|e| e.to_string())
            });
        let doc = Document::new("Region startup", 128, 128);
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        renderer.prepare_startup(&doc, &brush, false).unwrap();
        blocked.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready);
        renderer
            .submit(FramePacket {
                view: layer_render::ViewState {
                    width_px: 128,
                    height_px: 128,
                    document_to_surface: [1., 0., 0., 1., 0., 0.],
                    background_rgba_linear: [1.; 4],
                },
                document_extent: [128, 128],
                layers: &doc.layers,
                dabs: &[],
                dab_batches: &[],
                restore_rasters: &[],
                reset_layers: true,
                composite_all: true,
                time_seconds: 0.,
            })
            .unwrap();
        let request = layer_render::RegionRequest {
            contiguous: true,
            selection: None,
            request_id: 27,
            source: layer_render::RegionSource::Composite,
            position: [64, 64],
            tolerance: 0.,
            refinement: layer_render::RegionRefinement {
                gap_closing: 4,
                expansion: 3,
                smoothing: 1.,
            },
            limit: Some(Arc::new(
                layer_core::Selection::polygon(vec![
                    layer_core::Point { x: 10., y: 10. },
                    layer_core::Point { x: 100., y: 10. },
                    layer_core::Point { x: 100., y: 100. },
                    layer_core::Point { x: 10., y: 100. },
                ])
                .unwrap(),
            )),
        };
        let start = std::time::Instant::now();
        assert!(renderer.request_region(request.clone()).unwrap());
        assert!(
            !renderer.request_region(request).unwrap(),
            "single-flight includes waiting requests"
        );
        let pending = renderer.startup.as_ref().unwrap().compiler.pending();
        for _ in 0..3 {
            assert!(renderer.take_region().is_none());
        }
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "never wait for shader compilation"
        );
        assert_eq!(
            pending,
            renderer.startup.as_ref().unwrap().compiler.pending()
        );
        let regions = renderer.regions.as_ref().unwrap();
        assert!(regions.flood.pipelines().all(|p| !p.ready()));
        assert!(regions.raw.pipelines().all(|p| !p.ready()));
        assert_eq!(
            regions.storage_bytes(),
            116 + ((region_sources::TONAL_PARAMETER_WORDS + tonal::STAT_WORDS) * 4) as u64,
            "only empty bindings, fixed tonal bindings and the seed color exist"
        );
        assert!(renderer.region_pending());
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            renderer.poll_startup().unwrap();
            if let Some(result) = renderer.take_region() {
                let result = result.unwrap();
                assert_eq!(result.request_id, 27);
                assert_eq!(result.pixels.bounds(), [10, 10, 100, 100]);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!renderer.region_pending());
        while !renderer.poll_startup().unwrap().complete {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            renderer
                .regions
                .as_ref()
                .unwrap()
                .flood
                .pipelines()
                .all(Deferred::ready)
        );
    }
}
