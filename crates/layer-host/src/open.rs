//! Worker-side project preparation shared by the native hosts: limits, import,
//! admission, renderer construction, embedded effect validation and staged startup.
use crate::{GpuContext, Renderer, RendererOptions};
use layer_core::{Document, ProjectLimits};
use layer_render::{CanvasRenderer, EffectValidationRequest};
use layer_ui::UiSession;
use std::io::{Read, Seek};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub const PREPARE_DEADLINE: Duration = Duration::from_secs(120);

pub struct OpenEnvironment {
    pub admission: layer_ui::DocumentAdmission,
    pub platform: layer_ui::Platform,
    pub localization: std::sync::Arc<layer_ui::Localizer>,
    pub gpu: GpuContext,
    pub options: RendererOptions,
    pub viewport: [u32; 2],
    pub brush: layer_core::BrushSnapshot,
    pub new_drawing: Option<layer_ui::NewDrawingOrder>,
    pub photo_policy: layer_ui::PhotoOpenPolicy,
    pub asset_writer:Option<layer_ui::AssetWriter>,
}

impl OpenEnvironment {
    pub fn capture(
        session: &UiSession<Renderer>,
        admission: layer_ui::DocumentAdmission,
        options: RendererOptions,
    ) -> Result<Self, String> {
        let gpu = session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Wait for the canvas to finish starting")?;
        Ok(Self {
            admission,
            platform: session.state().platform,
            localization: session.localization().clone(),
            gpu: GpuContext::of(gpu),
            options,
            viewport: session.state().camera.viewport,
            brush: session.engine().configured_brush().clone(),
            new_drawing: session.new_drawing_order().cloned(),
            photo_policy: session.state().settings.photo_open,
            asset_writer:Some(session.asset_writer()),
        })
    }

    pub fn limits(&self) -> ProjectLimits {
        ProjectLimits {
            dimension: self
                .gpu
                .device
                .limits()
                .max_texture_dimension_2d
                .min(ProjectLimits::default().dimension),
            ..ProjectLimits::for_import()
        }
    }

    pub fn use_store(&mut self,stores:&crate::session_stores::SessionStores,id:u64)->Result<(),String> {
        self.asset_writer=Some(stores.asset_writer(id)?);Ok(())
    }
    pub fn storage_failed(&self,error:String) {if let Some(writer)=&self.asset_writer {writer.storage_failed(error);}}

    pub fn admit_sessions<'a>(&self, editors: impl IntoIterator<Item = &'a layer_core::Editor>) -> Result<(), String> {
        self.admission.admit_sessions(editors).map_err(|reason| reason.message(&self.localization))
    }

    pub fn read(
        &self,
        input: impl Read + Seek,
        intent: layer_ui::ImportIntent,
        name: &str,
        cancel: &AtomicBool,
        decode: crate::photo_reader::Decoder,
    ) -> Result<layer_ui::ImportOutcome, String> {
        layer_ui::read_import_with_decoder(
            input,
            intent,
            self.photo_policy,
            layer_ui::photo_name(name, &self.localization),
            self.limits(),
            layer_ui::photo_decode_limits(),
            cancel,
            |mut input, limits, cancelled| decode(&mut input, limits, cancelled),
        )
    }

    /// The drawing New chose, on its paper from `papers` when it is stored there.
    pub fn new_drawing(&self, papers: Option<&std::path::Path>) -> Result<(Document, Option<String>), String> {
        let order = self.new_drawing.as_ref().ok_or("Choose New drawing first")?;
        let paper = crate::papers::resolve(&order.paper, papers, &self.localization);
        Ok((order.project(paper.texture, &self.localization)?, paper.warning))
    }

    /// Returns a candidate whose canvas and configured brush are ready to draw.
    pub fn prepare(
        &self,
        mut project: Document,
        cancelled: impl Fn() -> bool,
    ) -> Result<Box<UiSession<Renderer>>, String> {
        let check = || {
            if cancelled() {Err("Document operation cancelled".to_string())}else {Ok(())}
        };
        check()?;
        project.validate(ProjectLimits { dimension: self.limits().dimension, ..Default::default() })?;
        let writer=self.asset_writer.as_ref().map(|writer|writer.for_document(&project)).transpose()?;
        if let Some(writer)=&writer {pollster::block_on(writer.prepare_document(&mut project))?;}
        else {project.reserve_import()?;}
        self.admission.admit(&project).map_err(|reason| reason.message(&self.localization))?;
        check()?;
        layer_color::validate_document_color(&project)?;
        let mut gpu = self
            .gpu
            .rasterizer(project.composition().color, &self.options, true)?;
        let mut programs = Vec::new();
        for (_, _, effect) in project.artwork.effects.iter() {
            if !programs.contains(&effect.program) { programs.push(effect.program.clone()); }
        }
        let mut validating = !programs.is_empty();
        if validating {
            gpu.request_effect_validation(EffectValidationRequest {
                request_id: 1,
                retained_programs: programs.clone(),
                programs,
            })
            .map_err(|e| e.to_string())?;
        }
        // Preparing startup starts the native compiler. Waiting for validation
        // before this call leaves all embedded shader jobs permanently queued.
        gpu.prepare_startup(&project, &self.brush, false)
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + PREPARE_DEADLINE;
        loop {
            check()?;
            if validating && let Some(result) = gpu.take_effect_validation() {
                result.result?;
                validating = false;
            }
            let ready = gpu.poll_startup().map_err(|e| e.to_string())?;
            if !validating && ready.canvas_ready && ready.brush_ready {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Project canvas preparation timed out".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut candidate =
            UiSession::from_project_localized(Renderer(Some(gpu)), project, None, self.viewport, self.platform, self.localization.clone())?;
        loop {
            check()?;
            candidate.frame(0, 0)?;
            if candidate.can_park_document() {break;}
            candidate.renderer_mut().0.as_mut().ok_or("Canvas unavailable")?.poll_startup().map_err(|error|error.to_string())?;
            if Instant::now()>=deadline {return Err("Project canvas preparation timed out".into());}
            std::thread::sleep(Duration::from_millis(2));
        }
        check()?;
        if let Some(writer)=writer {candidate.bind_asset_writer(writer)?;}
        Ok(Box::new(candidate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_render_wgpu::WgpuRasterizer;
    #[test]
    fn warp_floor_admits_a_blank_4800_by_3600_canvas_but_not_an_exhausted_working_set() {
        use layer_core::{MemoryBudget,MemoryFacts,DeviceMemoryFacts,MemoryOwnerKind,MemoryClass,MemoryReport};
        use layer_ui::{DrawingSetup,LengthUnit,Measure,PaperSetup,CanvasLimits,SetupProblem};
        const MIB:u64=1024*1024;const GIB:u64=1024*MIB;
        let budget=MemoryBudget::with_facts(MemoryFacts {total_bytes:Some(81*GIB/10),available_bytes:Some(5*GIB/4),process_limit_bytes:Some(384*MIB),..Default::default()});
        budget.register_device(DeviceMemoryFacts {unified:true,..Default::default()});
        assert_eq!(budget.ram_limit_bytes(),384*MIB);
        let working=layer_render_wgpu::new_drawing_memory_requirement([2560,1600],true);
        let setup=DrawingSetup {unit:LengthUnit::Px,width:Measure::new(4800.),height:Measure::new(3600.),resolution:300,
            color:Default::default(),blend_space:Default::default(),paper:PaperSetup {texture:None,color:layer_core::color::RgbColor::WHITE,transparent:false,relief:0}};
        let limits=||CanvasLimits::new(16384,Some(budget.import_headroom_bytes())).with_working_bytes(Some(working));
        assert!(working>0 && working<384*MIB);assert_eq!(setup.validate(limits()),Ok(()));
        let owner=budget.register_owner(MemoryOwnerKind::Other,MemoryClass::Pinned,None);
        owner.report(MemoryReport {ram_bytes:384*MIB,pinned_ram_bytes:384*MIB,..Default::default()});
        assert_eq!(setup.validate(limits()),Err(SetupProblem::Memory));
    }

    #[test]
    fn open_prepares_a_ready_candidate_and_honours_cancellation() {
        let mut project = layer_ui::NewDocumentOptions::default().project(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let layer_core::SourceTarget::Paint(target)=project.working.target.unwrap() else {panic!()};
        let descriptor=project.composition().color.paint_descriptor();
        let pixels=[23,47,83,255].repeat((layer_core::raster::TILE_SIZE*layer_core::raster::TILE_SIZE) as usize);
        let blob=std::sync::Arc::new(layer_core::raster::TileBlob::encode(descriptor,&pixels).unwrap());
        let tile=layer_core::raster::RasterTile::backed_shared(blob.clone());
        project.artwork.paint.get_mut(target).unwrap().raster=layer_core::raster::RasterRevision::backed(layer_core::raster::RasterData {
            tiles:[(layer_core::raster::TileKey {plane:layer_core::raster::RasterPlane::Color,coordinate:[0;2]},tile)].into(),
            ..Default::default()
        });
        let gpu = WgpuRasterizer::new_native_headless(project.composition().color).unwrap();
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let session = UiSession::from_project_localized(
            Renderer(Some(gpu.into())),
            layer_ui::new_drawing(64, 48, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),
            None,
            [640, 480],
            layer_ui::Platform::Mac,
            localization.clone(),
        )
        .unwrap();
        let admission = layer_ui::DocumentSessions::<()>::default()
            .admission(&session.retained_document_tiles());
        let environment =
            OpenEnvironment::capture(&session, admission, RendererOptions::default()).unwrap();
        let directory=std::env::temp_dir().join(format!("capy-open-storage-{}",layer_core::PortableId::random()));
        let stores=crate::session_stores::SessionStores::native(directory.clone(),std::sync::Arc::new(||{}));
        assert!(std::sync::Arc::ptr_eq(&environment.localization, &localization));
        assert_eq!(environment.viewport, session.state().camera.viewport);
        let inventory=layer_core::Editor::new(project.clone()).retained_tiles();
        let bytes=inventory.total_bytes() as u64;
        let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
        let pinned=budget.register_owner(layer_core::MemoryOwnerKind::Other,layer_core::MemoryClass::Pinned,None);
        let occupied=budget.ram_limit_bytes()-bytes+1;
        pinned.report(layer_core::MemoryReport {ram_bytes:occupied,pinned_ram_bytes:occupied,..Default::default()});
        let refused=project.clone().with_memory(layer_core::DocumentMemory::new(budget.clone(),None));
        assert_eq!(environment.prepare(refused,||false).err().as_deref(),Some(layer_core::MEMORY_BUDGET_EXCEEDED));
        assert_eq!(budget.snapshot().reserved_bytes,0);
        let mut candidate=std::thread::scope(|scope| {
            let (sent,received)=std::sync::mpsc::channel();
            let environment=&environment;let project=project.clone();
            let worker=scope.spawn(move || {
                let result=environment.prepare(project,||false);
                sent.send(()).unwrap();result
            });
            let completed=received.recv_timeout(Duration::from_secs(30)).is_ok();
            assert!(!directory.exists());assert!(blob.resident_bytes()>0);
            stores.install_storage(1,&session).unwrap();
            let candidate=worker.join().unwrap();
            assert!(completed,"preparation waited for the startup storage binding");candidate.unwrap()
        });
        assert!(candidate.can_park_document());
        assert!(std::sync::Arc::ptr_eq(candidate.localization(), &localization));
        let gpu = candidate.renderer_mut().0.as_mut().unwrap();
        assert!(gpu.device() == &environment.gpu.device);
        let ready = gpu.poll_startup().unwrap();
        assert!(ready.canvas_ready && ready.brush_ready);
        let binding=stores.install_storage(2,&candidate).unwrap();
        let reply=stores.checkpoint(2,candidate.capture_session().unwrap(),candidate.session_stamp(),true).unwrap();
        assert!(pollster::block_on(reply).is_ok());
        assert_eq!(blob.resident_bytes(),0);
        assert!(binding.path().join("head.0").exists() || binding.path().join("head.1").exists());
        assert_eq!(blob.decode_on_worker().unwrap(),pixels);
        assert_eq!(
            environment.prepare(project, || true).err().as_deref(),
            Some("Document operation cancelled")
        );
        drop((candidate, environment, session,stores));
        let _=std::fs::remove_dir_all(directory);
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
