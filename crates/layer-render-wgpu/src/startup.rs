//! Cold-start dependency ordering shared by native and browser compilers. The compiler owns only cloned GPU
//! handles and recipes, never the session, swapchain, input queue or live pixels.
use super::*;
use layer_core::{BrushSnapshot, Document, StrokeTool};
use std::sync::Arc;
#[cfg(all(test, not(target_arch = "wasm32")))]
use std::sync::Mutex;

const DOCUMENT: u8 = 2;
pub(super) const BRUSH: u8 = 3;
pub(super) const OTHER: u8 = 4;
#[cfg(not(target_arch = "wasm32"))]
#[path = "startup_native.rs"]
mod platform;
#[cfg(target_arch = "wasm32")]
#[path = "startup_web.rs"]
mod platform;
pub(super) use platform::Compiler;
#[cfg(not(target_arch = "wasm32"))]
pub use platform::finish_shader_compiler_shutdown;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartupProgress {
    pub canvas_ready: bool,
    pub brush_ready: bool,
    pub complete: bool,
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
        for p in &self.render {
            compiler.pipeline(p, priority);
        }
        for p in &self.compute {
            compiler.pipeline(p, priority);
        }
    }
    fn style(
        &mut self,
        r: &WgpuRasterizer,
        style: &layer_render::DabStyle,
        mask: bool,
        preview: bool,
    ) {
        if style.selection.is_some() {
            self.compute.extend([
                r.selection_clip.crossings.clone(),
                r.selection_clip.fill.clone(),
                r.selection_clip.resample.clone(),
            ]);
        }
        if mask {
            let index = usize::from(matches!(style.tip, BrushTip::Mask(_))) * 2
                + usize::from(style.mode == DabMode::Erase);
            self.render.push(r.layer_masks.brush[index].clone());
            return;
        }
        let plan = BrushPassPlan::for_style(style);
        if let Some(kind) = plan.direct {
            self.render.push(r.pipelines.direct[kind as usize].clone());
        } else {
            let kind = MaterialPipelineKind::for_attachments(
                plan.state.watercolor_wetness,
                plan.state.coverage,
                plan.state.canvas_wetness,
            );
            self.render
                .push(r.pipelines.material[kind as usize].clone());
            if preview {
                let variant = MaterialPipelineKind::for_attachments(
                    plan.state.watercolor_wetness,
                    plan.state.coverage,
                    false,
                );
                self.render
                    .push(r.pipelines.material[variant as usize].clone());
                // A single predicted destination batch on a simple canvas reads
                // persistent coverage but writes only its disposable color.
                if !plan.state.watercolor_wetness {
                    self.render
                        .push(r.pipelines.material[MaterialPipelineKind::Color as usize].clone());
                }
            }
        }
        if plan.reservoir {
            self.render.push(r.pipelines.reservoir.clone());
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
    revision: Option<layer_core::Revision>,
    brush: Option<BrushSnapshot>,
    transform: bool,
    document: Requirements,
    current: Requirements,
    effects: Option<mpsc::Receiver<Result<effects::Effects, String>>>,
    effects_ready: bool,
    others_queued: bool,
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
            revision: None,
            brush: None,
            transform: false,
            document: Requirements::default(),
            current: Requirements::default(),
            effects: None,
            effects_ready: false,
            others_queued: false,
            host_catalog_pending: false,
            finished: false,
        })
    }
}
impl WgpuRasterizer {
    /// Browser hosts mark the catalog submitted after their asynchronous fetch,
    /// then poll one owned compilation future between rendering opportunities.
    #[cfg(target_arch = "wasm32")]
    pub fn wait_for_startup_catalog(&mut self) {
        if let Some(startup) = &mut self.startup {
            startup.host_catalog_pending = true;
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn startup_catalog_submitted(&mut self) {
        if let Some(startup) = &mut self.startup {
            startup.host_catalog_pending = false;
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn compile_startup_step(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), GpuRasterError>>>> {
        self.startup.as_ref().map_or_else(
            || {
                Box::pin(async { Ok(()) })
                    as std::pin::Pin<
                        Box<dyn std::future::Future<Output = Result<(), GpuRasterError>>>,
                    >
            },
            |startup| startup.compiler.step(),
        )
    }
    #[cfg(target_arch = "wasm32")]
    pub fn shader_work_pending(&self) -> bool {
        self.startup
            .as_ref()
            .is_some_and(|s| s.compiler.pending() != 0)
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
            !s.finished
                && (s.revision != Some(document.revision)
                    || s.brush.as_ref() != Some(brush)
                    || s.transform != transform)
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
        if startup.revision != Some(document.revision) {
            let mut required = Requirements::default();
            required
                .render
                .extend(self.scene_pipelines.pipeline.iter().cloned());
            for stroke in document.strokes() {
                let mut style = style(&stroke.brush, stroke.tool, stroke.alpha_locked);
                style.selection = stroke.selection.clone();
                startup.masks.style(&startup.compiler, &style, DOCUMENT);
                required.style(
                    self,
                    &style,
                    layer_masks::MaskRenderer::is_mask(&document.layers, stroke.layer_id),
                    false,
                );
            }
            if document
                .layers
                .iter()
                .any(|l| l.mask.is_some() || !l.operations.is_empty())
            {
                required.render.push(self.layer_masks.initialize.clone());
                required.compute.extend([
                    self.selection_clip.crossings.clone(),
                    self.selection_clip.fill.clone(),
                    self.selection_clip.resample.clone(),
                ]);
            }
            if document.layers.iter().any(|l| {
                l.operations
                    .iter()
                    .chain(l.masks().flat_map(|m| m.operations.iter()))
                    .any(|op| matches!(op.kind, layer_core::LayerOperationKind::Transform(_)))
            }) {
                required.render.extend(
                    self.transforms
                        .as_ref()
                        .unwrap()
                        .pipelines()
                        .into_iter()
                        .cloned(),
                );
            }
            required.enqueue(&startup.compiler, DOCUMENT);
            startup.document = required;
            startup.revision = Some(document.revision);
            let chains = scene::startup_effect_chains(&document.layers);
            if !chains.is_empty() {
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
                startup.compiler.enqueue(DOCUMENT, move || {
                    let result = (|| {
                        for (layers, execution) in chains {
                            candidate.prepare(
                                &gpu,
                                &layers.iter().collect::<Vec<_>>(),
                                execution,
                                0.,
                            )?;
                        }
                        Ok::<_, GpuRasterError>(candidate.fork())
                    })()
                    .map_err(|e| e.to_string());
                    let _ = tx.send(result);
                    Ok(())
                });
                startup.effects = Some(rx);
                startup.effects_ready = false;
            } else {
                startup.effects = None;
                startup.effects_ready = true;
            }
        }
        let mut current = Requirements::default();
        let locked = document
            .layers
            .iter()
            .find(|l| l.id == document.active_layer)
            .is_some_and(|l| l.properties.alpha_locked);
        // A physical eraser can select the erase variant without a UI change.
        for tool in [StrokeTool::Brush, StrokeTool::Eraser] {
            let mut style = style(brush, tool, locked);
            style.selection = document.selection.clone().map(Arc::new);
            startup.masks.style(&startup.compiler, &style, BRUSH);
            current.style(self, &style, document.active_mask, true);
        }
        // Live transforms do not change document revision or brush settings.
        // They still need their own shaders before an interactive frame runs.
        if transform {
            current.render.extend(
                self.transforms
                    .as_ref()
                    .unwrap()
                    .pipelines()
                    .into_iter()
                    .cloned(),
            );
            current.compute.extend([
                self.selection_clip.crossings.clone(),
                self.selection_clip.fill.clone(),
                self.selection_clip.resample.clone(),
            ]);
        }
        current.enqueue(&startup.compiler, BRUSH);
        startup.current = current;
        startup.brush = Some(brush.clone());
        startup.transform = transform;
        if !startup.others_queued {
            startup.masks.remaining(&startup.compiler);
            for p in self
                .pipelines
                .direct
                .iter()
                .chain(&self.pipelines.material)
                .chain(&self.pipelines.watercolor_transport)
                .chain([
                    &self.pipelines.reservoir,
                    &self.pipelines.stroke_edge,
                    &self.pipelines.watercolor_composite,
                    &self.pipelines.export,
                ])
                .chain(self.layer_masks.brush.iter())
                .chain([&self.layer_masks.initialize])
            {
                startup.compiler.pipeline(p, OTHER);
            }
            for p in [
                &self.selection_clip.crossings,
                &self.selection_clip.fill,
                &self.selection_clip.resample,
            ] {
                startup.compiler.pipeline(p, OTHER);
            }
            for p in self.transforms.as_ref().unwrap().pipelines() {
                startup.compiler.pipeline(p, OTHER);
            }
            let regions = self
                .regions
                .get_or_insert_with(|| region_requests::RegionRequests::new(&self.device));
            for p in regions.flood.pipelines() {
                startup.compiler.pipeline(p, OTHER);
            }
            startup.others_queued = true;
        }
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
            return Ok(StartupProgress {
                canvas_ready: true,
                brush_ready: true,
                complete: true,
            });
        };
        startup.compiler.check()?;
        if startup.finished {
            return Ok(StartupProgress {
                canvas_ready: true,
                brush_ready: true,
                complete: true,
            });
        }
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
        let canvas_ready = startup.revision.is_some()
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
fn style(brush: &BrushSnapshot, tool: StrokeTool, alpha_locked: bool) -> layer_render::DabStyle {
    layer_render::DabStyle {
        alpha_locked,
        selection: None,
        tip: brush.tip.clone(),
        mode: match tool {
            StrokeTool::Brush => DabMode::Paint,
            StrokeTool::Eraser => DabMode::Erase,
        },
        execution: brush.execution_class(),
        grain: brush.grain.clone(),
        dual: brush.dual.clone(),
        rendering: brush.rendering,
        wet_mix: brush.wet_mix,
        transport: brush.transport.clone(),
        deform: brush.deform,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
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
                reset_layers: true,
                composite_all: true,
                time_seconds: 0.,
            })
            .unwrap();
        let request = layer_render::RegionRequest {
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
        assert_eq!(
            regions.storage_bytes(),
            52,
            "only the two tiny empty bindings exist"
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
    #[test]
    fn live_transform_promotes_dependencies_without_blocking_the_caller() {
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
        let doc = Document::new("Live transform readiness", 128, 128);
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        renderer.prepare_startup(&doc, &brush, false).unwrap();
        blocked.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready);
        assert!(!renderer.startup_needs_update(&doc, &brush, false));
        assert!(renderer.startup_needs_update(&doc, &brush, true));
        let start = std::time::Instant::now();
        renderer.prepare_startup(&doc, &brush, true).unwrap();
        let progress = renderer.poll_startup().unwrap();
        assert!(!progress.canvas_ready && !progress.brush_ready);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "Never join the compiler on the input/render thread"
        );
        assert!(
            renderer
                .transforms
                .as_ref()
                .unwrap()
                .pipelines()
                .iter()
                .all(|p| !p.ready())
        );
        let pending = renderer.startup.as_ref().unwrap().compiler.pending();
        renderer.prepare_startup(&doc, &brush, true).unwrap();
        assert_eq!(
            renderer.startup.as_ref().unwrap().compiler.pending(),
            pending,
            "Unchanged requirements must not enqueue duplicate preparation"
        );
        // Cancel while compilation is pending. The unchanged canvas can resume.
        renderer.prepare_startup(&doc, &brush, false).unwrap();
        assert!(renderer.poll_startup().unwrap().brush_ready);
        renderer.prepare_startup(&doc, &brush, true).unwrap();
        assert!(!renderer.poll_startup().unwrap().canvas_ready);
        assert_eq!(
            renderer.startup.as_ref().unwrap().compiler.pending(),
            pending
        );
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !renderer.poll_startup().unwrap().brush_ready {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            renderer
                .transforms
                .as_ref()
                .unwrap()
                .pipelines()
                .iter()
                .all(|p| p.ready())
        );
        assert!(renderer.selection_clip.crossings.ready());
        assert!(renderer.selection_clip.fill.ready());
        assert!(renderer.selection_clip.resample.ready());
        while !renderer.poll_startup().unwrap().complete {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!renderer.startup_needs_update(&doc, &brush, false));
        assert!(!renderer.startup_needs_update(&doc, &brush, true));
    }
    #[test]
    fn loaded_filters_and_current_brush_render_before_unused_pipelines() {
        verify_document_startup(0);
    }
    #[test]
    fn saved_transforms_compile_before_document_replay() {
        verify_document_startup(1);
    }
    #[test]
    fn saved_mask_transforms_compile_before_document_replay() {
        verify_document_startup(2);
    }
    fn verify_document_startup(target: u8) {
        let transform = target != 0;
        let mut reference = WgpuRasterizer::new_headless().unwrap();
        let mut renderer = WgpuRasterizer::from_wgpu_staged(
            reference.adapter.clone(),
            reference.device().clone(),
            reference.queue.clone(),
        )
        .unwrap();
        assert!(renderer.pipelines.material.iter().all(|p| !p.ready()));
        assert!(renderer.pipelines.direct.iter().all(|p| !p.ready()));
        assert!(
            renderer
                .transforms
                .as_ref()
                .unwrap()
                .pipelines()
                .iter()
                .all(|p| !p.ready())
        );
        let (release, wait) = mpsc::channel();
        renderer
            .startup
            .as_ref()
            .unwrap()
            .compiler
            .enqueue(OTHER, move || {
                wait.recv_timeout(Duration::from_secs(30))
                    .map_err(|e| e.to_string())
            });
        let mut doc = Document::new("startup filters", 128, 128);
        let asset = AssetId("startup checker".into());
        doc.layers[0].asset = Some(asset.clone());
        if transform {
            doc.layers[0].operations.push(layer_core::LayerOperation {
                after_stroke: 0,
                coverage: layer_core::LayerMask::reveal_all(
                    LayerId(100),
                    layer_core::Point::default(),
                ),
                kind: layer_core::LayerOperationKind::Transform(layer_core::ImageTransform {
                    affine: layer_core::Affine::translation(layer_core::Point { x: 9., y: 13. }),
                    ..Default::default()
                }),
            });
            if target == 2 {
                let mut mask =
                    layer_core::LayerMask::reveal_all(LayerId(9), layer_core::Point::default());
                mask.default_coverage = 0.;
                mask.initial = Some(
                    layer_core::Selection::polygon(vec![
                        layer_core::Point { x: 10., y: 10. },
                        layer_core::Point { x: 110., y: 10. },
                        layer_core::Point { x: 10., y: 110. },
                    ])
                    .unwrap(),
                );
                mask.operations = Arc::new(std::mem::take(&mut doc.layers[0].operations));
                doc.layers[0].mask = Some(mask);
            }
        }
        for (i, id) in ["domain_warp", "curves", "curves"].into_iter().enumerate() {
            let mut layer = Layer::paint(LayerId(10 + i as u64), id);
            layer.kind = LayerKind::Effect;
            layer.effect = Some(Arc::new(layer_core::EffectInstance::new(
                layer_core::bundled_effect_catalog()
                    .get(id)
                    .unwrap()
                    .program(),
            )));
            doc.layers.insert(0, layer);
        }
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        let bytes: Vec<_> = (0..128 * 128)
            .flat_map(|i| {
                if (i % 128 / 16 + i / 128 / 16) % 2 == 0 {
                    [220, 30, 80, 255]
                } else {
                    [20, 190, 140, 255]
                }
            })
            .collect();
        for r in [&mut reference, &mut renderer] {
            r.prepare_asset(
                &asset,
                HostImage {
                    width: 128,
                    height: 128,
                    stride: 512,
                    format: PixelFormat::Rgba8Srgb,
                    bytes: &bytes,
                },
            )
            .unwrap();
        }
        renderer.prepare_startup(&doc, &brush, false).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let progress = renderer.poll_startup().unwrap();
            if progress.brush_ready {
                assert!(!progress.complete);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            renderer.pipelines.material.iter().all(|p| !p.ready()),
            "Unrelated material shaders must not gate current content"
        );
        assert!(
            renderer
                .transforms
                .as_ref()
                .unwrap()
                .pipelines()
                .iter()
                .all(|p| p.ready() == transform),
            "Only loaded transforms belong in document-priority compilation"
        );
        let view = layer_render::ViewState {
            width_px: 128,
            height_px: 128,
            document_to_surface: [1., 0., 0., 1., 0., 0.],
            background_rgba_linear: [1.; 4],
        };
        let dabs = [Dab {
            center: layer_core::Point { x: 64., y: 64. },
            radii: [20., 20.],
            rotation: [1., 0.],
            motion: [0.; 2],
            color_rgba_linear: [0.1, 0.2, 0.9, 1.],
            flow: 1.,
            hardness: 1.,
            texture_sign: [1.; 2],
            material: [0.; 4],
        }];
        let mut batches = vec![DabBatch {
            material_update: 0,
            stroke_id: StrokeId(1),
            layer_id: doc.active_layer,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: style(&brush, StrokeTool::Brush, false),
            damage: layer_core::Rect {
                min: layer_core::Point { x: 40., y: 40. },
                max: layer_core::Point { x: 88., y: 88. },
            },
        }];
        if transform {
            batches.insert(
                0,
                DabBatch {
                    kind: DabBatchKind::LayerOperation(0),
                    layer_id: if target == 2 {
                        LayerId(9)
                    } else {
                        doc.active_layer
                    },
                    dab_count: 0,
                    damage: layer_core::Rect {
                        min: layer_core::Point::default(),
                        max: layer_core::Point { x: 128., y: 128. },
                    },
                    ..batches[0].clone()
                },
            );
        }
        let packet = FramePacket {
            time_seconds: 0.,
            view,
            document_extent: [128, 128],
            layers: &doc.layers,
            dabs: &dabs,
            dab_batches: &batches,
            reset_layers: true,
            composite_all: true,
        };
        let compilations = renderer.scene.as_ref().unwrap().effects.compilations;
        renderer.submit(packet).unwrap();
        assert_eq!(
            renderer.scene.as_ref().unwrap().effects.compilations,
            compilations,
            "Document warmup must include actual fused/image pipelines"
        );
        assert!(renderer.pipelines.material.iter().all(|p| !p.ready()));
        reference.submit(packet).unwrap();
        assert_eq!(
            renderer.readback_srgb_rgba8().unwrap(),
            reference.readback_srgb_rgba8().unwrap()
        );
        release.send(()).unwrap();
        while !renderer.poll_startup().unwrap().complete {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(renderer.pipelines.material.iter().all(Deferred::ready));
        assert!(
            renderer
                .transforms
                .as_ref()
                .unwrap()
                .pipelines()
                .iter()
                .all(|p| p.ready())
        );
    }
}
