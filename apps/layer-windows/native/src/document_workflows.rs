//! Windows scheduling around shared candidate policies. No WinUI callback owns
//! artwork, converts profiles, or decides how an edit enters history.
use layer_core::Project;
use layer_host::{NativeHost, Renderer};
use layer_render::CanvasRenderer;
use layer_render_wgpu::{
    WgpuRasterizer,
    snapshot::{CaptureControl, SnapshotGpu},
};
use layer_ui::{DocumentRequest, HostRequestKind, UiSession};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::Write,
    time::{Duration, Instant},
};

#[path = "document_color.rs"]
mod color;
#[path = "document_export.rs"]
mod export;
#[path = "document_source.rs"]
mod source;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Describe,
    ExportOptions {
        recipe: layer_ui::ExportRecipe,
        profile_id: Option<String>,
    },
    ExportWrite {
        path: String,
    },
    ExportPreset {
        action: layer_ui::ExportPresetAction,
        profile_id: Option<String>,
    },
    ProfileImport {
        path: String,
    },
    ProfileRemove {
        id: String,
    },
    ReadImages {
        paths: Vec<String>,
    },
    InterpretImage {
        profile: crate::color_storage::ProfileChoice,
    },
    Prepare {
        choice: Value,
        #[serde(default)]
        copy: bool,
    },
    Compare,
    Commit,
    Cancel,
    SaveCopy {
        path: String,
    },
}
struct Import {
    images: layer_ui::ImageImportBatch,
    context: layer_ui::ImagePlacementContext,
    device: wgpu::Device,
    paths: std::collections::VecDeque<String>,
    clipboard: Option<std::path::PathBuf>,
}
impl Drop for Import {
    fn drop(&mut self) {
        if let Some(path) = &self.clipboard {
            let _ = std::fs::remove_file(path);
        }
    }
}
static NEXT_CLIPBOARD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
enum Payload {
    Profiles,
    Export(Box<export::Task>),
    Import(Import),
    Color(Box<color::Task>),
    Source(Box<source::Task>),
    Info(layer_color::DocumentInfo),
    Histogram {
        project: Option<Project>,
        gpu: SnapshotGpu,
        background: [f32; 4],
        time: f32,
    },
}
pub(crate) struct Task {
    pub id: u32,
    pub control: CaptureControl,
    pub stage: &'static str,
    pub serial: u32,
    kind: &'static str,
    details: Value,
    preset_view: Value,
    error: Option<String>,
    payload: Payload,
}
impl Task {
    pub fn capture(host: &NativeHost, id: u32) -> Result<Box<Self>, String> {
        let session = &host.session;
        if id == 0 {
            return Ok(Box::new(Self {
                id,
                control: Default::default(),
                stage: "options",
                serial: 0,
                kind: "profiles",
                details: Value::Null,
                preset_view: Value::Null,
                error: None,
                payload: Payload::Profiles,
            }));
        }
        let request = &session
            .state()
            .requests
            .iter()
            .find(|r| r.id == id)
            .ok_or("Request expired")?
            .kind;
        let (kind, payload) = match request {
            HostRequestKind::Document {
                request: DocumentRequest::Export { .. },
            } => (
                "export",
                Payload::Export(Box::new(export::Task::capture(session, id)?)),
            ),
            HostRequestKind::Document {
                request: DocumentRequest::Place | DocumentRequest::Paste,
            } => {
                let kind = if matches!(
                    request,
                    HostRequestKind::Document {
                        request: DocumentRequest::Paste
                    }
                ) {
                    "paste"
                } else {
                    "place"
                };
                (
                    kind,
                    Payload::Import(Import {
                        images: layer_ui::ImageImportBatch::new(
                            session.state().settings.photo_open,
                            session.engine().document().color.space,
                            Default::default(),
                        ),
                        context: session.image_placement_context(None, None)?,
                        device: session
                            .engine()
                            .backend()
                            .0
                            .as_ref()
                            .ok_or("Canvas unavailable")?
                            .device()
                            .clone(),
                        paths: Default::default(),
                        clipboard: if kind == "paste" {
                            Some(crate::settings::data_directory()?.join("clipboard").join(
                                format!(
                                        "{}-{}.png",
                                        std::process::id(),
                                        NEXT_CLIPBOARD
                                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                    ),
                            ))
                        } else {
                            None
                        },
                    }),
                )
            }
            HostRequestKind::Document {
                request: DocumentRequest::ChangeColor { operation },
            } => (
                match operation {
                    layer_ui::DocumentColorOperation::Assign => "assign",
                    layer_ui::DocumentColorOperation::Convert => "convert",
                    layer_ui::DocumentColorOperation::Depth => "depth",
                },
                Payload::Color(Box::new(color::Task::capture(session)?)),
            ),
            HostRequestKind::Document {
                request: DocumentRequest::ColorHistory { .. },
            } => (
                "history",
                Payload::Color(Box::new(color::Task::capture(session)?)),
            ),
            HostRequestKind::Document {
                request:
                    DocumentRequest::RepairSourceProfile { .. }
                    | DocumentRequest::RasterizeSource { .. },
            } => {
                let kind = if matches!(
                    request,
                    HostRequestKind::Document {
                        request: DocumentRequest::RasterizeSource { .. }
                    }
                ) {
                    "rasterize"
                } else {
                    "repair"
                };
                (
                    kind,
                    Payload::Source(Box::new(source::Task::capture(session)?)),
                )
            }
            HostRequestKind::Document {
                request: DocumentRequest::Properties,
            } => (
                "properties",
                Payload::Info(layer_color::DocumentInfo::capture(
                    session.engine().document(),
                )),
            ),
            HostRequestKind::Histogram => {
                session.require_document_snapshot_idle()?;
                let gpu = session
                    .engine()
                    .backend()
                    .0
                    .as_ref()
                    .ok_or("Canvas unavailable")?;
                (
                    "histogram",
                    Payload::Histogram {
                        project: Some(session.capture_project_recovery()?),
                        gpu: gpu.snapshot_gpu(),
                        background: session.engine().view().background_rgba_linear,
                        time: session.engine().animation_time(),
                    },
                )
            }
            _ => return Err("Not a document inspection or color request".into()),
        };
        Ok(Box::new(Self {
            id,
            control: Default::default(),
            stage: "options",
            serial: 0,
            kind,
            details: Value::Null,
            preset_view: Value::Null,
            error: None,
            payload,
        }))
    }
    fn describe(&mut self) -> Result<(), String> {
        self.details = match &mut self.payload {
            Payload::Profiles => {
                json!({"profiles":crate::color_storage::list(self.control.cancellation_flag())?})
            }
            Payload::Export(task) => {
                if self.preset_view.is_null() {
                    self.preset_view = serde_json::to_value(crate::color_storage::presets(
                        layer_ui::ExportPresetAction::List,
                        &task.original.project.document,
                        self.control.cancellation_flag(),
                    )?)
                    .map_err(|e| e.to_string())?;
                }
                let mut details = task.details()?;
                details["presets"] = self.preset_view.clone();
                details["profiles"] = serde_json::to_value(crate::color_storage::list(
                    self.control.cancellation_flag(),
                )?)
                .map_err(|e| e.to_string())?;
                details
            }
            Payload::Import(task) => {
                if let Some(path) = &task.clipboard {
                    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| {
                        crate::document_io::io_error("prepare clipboard storage", e)
                    })?;
                }
                json!({"pending":task.images.pending_source().map(|s| &s.interpretation),"extensions":layer_color::photo::extensions().collect::<Vec<_>>(),
                    "profiles":crate::color_storage::list(self.control.cancellation_flag())?, "clipboard":task.clipboard.as_ref().map(|p| json!({"path":p,"folder":p.parent(),"name":p.file_name().and_then(|s| s.to_str())}))})
            }
            Payload::Color(task) => json!({"color":task.workflow.original.document.color,
                "result":task.workflow.candidate.as_ref().map(|p| p.document.color), "clipped_channels":task.clipped,
                "copy":task.workflow.is_copy()}),
            Payload::Source(task) => {
                let mut details = task.details()?;
                details["profiles"] = serde_json::to_value(crate::color_storage::list(
                    self.control.cancellation_flag(),
                )?)
                .map_err(|e| e.to_string())?;
                details
            }
            Payload::Info(info) => {
                serde_json::to_value(info.describe()?).map_err(|e| e.to_string())?
            }
            Payload::Histogram {
                project,
                gpu,
                background,
                time,
            } => {
                let project = project.take().ok_or("Histogram was already captured")?;
                let sampled_time = project.document.has_animated_effects().then_some(*time);
                let mut renderer = gpu
                    .capture(
                        project,
                        *background,
                        *time,
                        Default::default(),
                        self.control.clone(),
                    )
                    .map_err(|e| e.to_string())?;
                json!({"histogram":renderer.histogram().map_err(|e| e.to_string())?,"sampled_time":sampled_time})
            }
        };
        Ok(())
    }
    /// Only the file worker calls this; errors keep a reviewable task to cancel.
    pub fn work(&mut self, action: Action) {
        let result = (|| {
            if self.control.is_cancelled() {
                return Err("Document operation cancelled".into());
            }
            match action {
                Action::Describe if self.kind == "history" => {
                    if let Payload::Color(task) = &mut self.payload {
                        task.work(None, false, self.control.clone())?;
                    }
                    self.stage = "commit";
                    self.describe()
                }
                Action::Describe => self.describe(),
                Action::ProfileImport { path } => {
                    crate::color_storage::import(
                        std::path::Path::new(&path),
                        self.control.cancellation_flag(),
                    )?;
                    self.describe()
                }
                Action::ProfileRemove { id } => {
                    crate::color_storage::remove(&id, self.control.cancellation_flag())?;
                    self.describe()
                }
                Action::ExportOptions {
                    mut recipe,
                    profile_id,
                } => {
                    let Payload::Export(task) = &mut self.payload else {
                        return Err("No export is pending".into());
                    };
                    if let Some(id) = profile_id {
                        let profile =
                            crate::color_storage::profile(&id, self.control.cancellation_flag())?;
                        recipe.profile = layer_ui::ExportProfile {
                            channels: layer_color::profile_channels(&profile)?,
                            name: layer_color::profile_description(&profile)?,
                            profile,
                        };
                    }
                    task.configure(recipe)?;
                    task.compare(self.control.clone())?;
                    self.stage = "preview";
                    self.describe()
                }
                Action::ExportWrite { path } => {
                    if self.stage != "preview" {
                        return Err("Preview the export first".into());
                    }
                    let Payload::Export(task) = &mut self.payload else {
                        return Err("No export is pending".into());
                    };
                    crate::document_io::location(&path)?;
                    crate::document_io::atomic_write_seek(
                        std::path::Path::new(&path),
                        self.control.cancellation_flag(),
                        |file| task.write(file, self.control.clone()),
                    )?;
                    self.stage = "saved";
                    Ok(())
                }
                Action::ExportPreset {
                    mut action,
                    profile_id,
                } => {
                    if let Some(id) = profile_id {
                        let profile =
                            crate::color_storage::profile(&id, self.control.cancellation_flag())?;
                        let value = layer_ui::ExportProfile {
                            channels: layer_color::profile_channels(&profile)?,
                            name: layer_color::profile_description(&profile)?,
                            profile,
                        };
                        match &mut action {
                            layer_ui::ExportPresetAction::Save { recipe, .. }
                            | layer_ui::ExportPresetAction::Update { recipe, .. }
                            | layer_ui::ExportPresetAction::Remember { recipe, .. } => {
                                recipe.profile = value
                            }
                            _ => {}
                        }
                    }
                    let Payload::Export(task) = &mut self.payload else {
                        return Err("No export is pending".into());
                    };
                    let view = crate::color_storage::presets(
                        action,
                        &task.original.project.document,
                        self.control.cancellation_flag(),
                    )?;
                    if let Some(recipe) = &view.recipe {
                        task.configure(recipe.clone())?;
                    }
                    self.preset_view = serde_json::to_value(view).map_err(|e| e.to_string())?;
                    self.stage = "options";
                    self.describe()
                }
                Action::ReadImages { paths } => {
                    let Payload::Import(task) = &mut self.payload else {
                        return Err("No image placement is pending".into());
                    };
                    if self.stage != "options" || paths.is_empty() {
                        return Err("Choose images to place".into());
                    }
                    for path in &paths {
                        crate::document_io::location(path)?;
                    }
                    task.paths = paths.into();
                    self.read_images()
                }
                Action::InterpretImage { profile } => {
                    let Payload::Import(task) = &mut self.payload else {
                        return Err("No image interpretation is pending".into());
                    };
                    task.images.interpret(
                        profile.resolve(self.control.cancellation_flag())?,
                        self.control.is_cancelled(),
                    )?;
                    self.read_images()
                }
                Action::Prepare { choice, copy } => {
                    match &mut self.payload {
                        Payload::Color(task) => {
                            task.work(
                                serde_json::from_value(choice).map_err(|e| e.to_string())?,
                                copy,
                                self.control.clone(),
                            )?;
                            self.stage = "preview";
                        }
                        Payload::Source(task) if !copy => {
                            task.work(serde_json::from_value::<Option<crate::color_storage::ProfileChoice>>(choice).map_err(|e| e.to_string())?.map(|p| p.resolve(self.control.cancellation_flag())).transpose()?, self.control.clone())?;
                            self.stage = "source_candidate";
                        }
                        _ => return Err("This request cannot prepare an edit".into()),
                    }
                    self.describe()
                }
                Action::Compare => {
                    let Payload::Source(task) = &mut self.payload else {
                        return Err("No source comparison is pending".into());
                    };
                    task.compare(self.control.clone())?;
                    self.stage = "preview";
                    self.describe()
                }
                Action::SaveCopy { path } => {
                    let Payload::Color(task) = &self.payload else {
                        return Err("No converted copy is ready".into());
                    };
                    crate::document_io::location(&path)?;
                    crate::document_io::atomic_write(
                        std::path::Path::new(&path),
                        self.control.cancellation_flag(),
                        |file| task.write_copy(file, self.control.is_cancelled()),
                    )?;
                    self.stage = "saved";
                    Ok(())
                }
                _ => Err("This action belongs on the document owner".into()),
            }
        })();
        self.error = result.err();
        if self.error.is_some() {
            self.stage = "error";
        }
        self.serial = self.serial.saturating_add(1);
    }
    fn read_images(&mut self) -> Result<(), String> {
        let Payload::Import(task) = &mut self.payload else {
            return Err("No image batch".into());
        };
        while let Some(path) = task.paths.pop_front() {
            let path = std::path::Path::new(&path);
            let file = std::fs::File::open(path)
                .map_err(|e| crate::document_io::io_error("read image", e))?;
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Image");
            task.images.read(
                crate::document_io::Stream {
                    inner: file,
                    cancel: self.control.cancellation_flag(),
                },
                name,
                self.control.cancellation_flag(),
            )?;
            if task.images.pending_source().is_some() {
                self.stage = "interpret_image";
                return self.describe();
            }
        }
        self.stage = "commit";
        self.describe()
    }
    pub fn place_at(
        &mut self,
        host: &NativeHost,
        screen: Option<layer_core::Point>,
        layer: Option<(u64, f32)>,
    ) -> Result<(), String> {
        let Payload::Import(task) = &mut self.payload else {
            return Err("No image batch".into());
        };
        if screen.is_some() && layer.is_some() {
            return Err("Choose one image drop target".into());
        }
        let destination = layer
            .map(|(target, fraction)| {
                let position = host
                    .session
                    .image_layer_drop_hint(target, fraction)
                    .ok_or("Images cannot be placed at this layer position")?;
                Ok::<_, String>(layer_ui::ImageLayerDestination {
                    target: layer_core::LayerId(target),
                    position,
                })
            })
            .transpose()?;
        task.context = host.session.image_placement_context(screen, destination)?;
        Ok(())
    }
    pub fn awaits_placement_ui(&self) -> bool {
        matches!(self.payload, Payload::Import(_))
    }
    pub fn prepare_owner(&mut self, host: &NativeHost) -> Result<bool, String> {
        if self.stage != "source_candidate" {
            return Ok(false);
        }
        let Payload::Source(task) = &mut self.payload else {
            return Err("No source candidate".into());
        };
        task.prepare(host, &self.control)?;
        Ok(true)
    }
    pub fn commit(&mut self, host: &mut NativeHost) -> Result<(), String> {
        if !matches!(self.stage, "preview" | "commit") || self.error.is_some() {
            return Err("Preview the result before applying it".into());
        }
        match &mut self.payload {
            Payload::Import(task) => {
                let session = &mut host.session;
                session.validate_image_placement(&task.context)?;
                if self.control.is_cancelled()
                    || session.engine().backend().0.as_ref().map(|g| g.device())
                        != Some(&task.device)
                    || !session.state().requests.iter().any(|r| r.id == self.id)
                {
                    return Err("The canvas or import request changed; try again".into());
                }
                let previous = session.state().revision;
                session.place_layer_sources(
                    task.images.take_sources(self.control.is_cancelled())?,
                    task.context.center,
                    task.context.destination,
                )?;
                let mut change = session.complete_document_request(self.id, Ok(true))?;
                change.canvas_wake = true;
                change.regions |= 255;
                host.apply_change(previous, change);
                Ok(())
            }
            Payload::Color(task) => task.adopt(host, &self.control),
            Payload::Source(task) => task.adopt(host, &self.control),
            _ => Err("No document edit is ready".into()),
        }
    }
    pub fn complete(&self, host: &mut NativeHost, saved: bool) -> Result<(), String> {
        if self.id == 0 {
            return Ok(());
        }
        if self.kind == "histogram" {
            host.dispatch(layer_ui::UiAction::CompleteRequest {
                id: self.id,
                error: None,
            })
        } else {
            let previous = host.session.state().revision;
            let change = host.session.complete_document_request(self.id, Ok(saved))?;
            host.apply_change(previous, change);
            Ok(())
        }
    }
    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.stage = "error";
        self.serial = self.serial.saturating_add(1);
    }
    pub fn status(&self) -> Value {
        json!({"type":"workflow", "id":self.id,"serial":self.serial,"kind":self.kind,
            "stage":self.stage,"details":self.details,"error":self.error,
            "spaces":layer_core::color::RgbSpace::ALL.map(|s| (s,s.name()))})
    }
    pub fn preview(&self, index: usize) -> Result<crate::previews::CapyPreview, String> {
        let previews = match &self.payload {
            Payload::Color(task) => &task.previews,
            Payload::Source(task) => &task.previews,
            Payload::Export(task) => &task.previews,
            _ => return Err("No comparison preview".into()),
        };
        let preview = previews
            .get(index)
            .ok_or("Comparison preview is not ready")?;
        crate::previews::CapyPreview::packet(json!({"id":self.id,"serial":self.serial,"width":preview.extent[0],"height":preview.extent[1]}), preview.pixels.clone()).map_err(|e| e.to_string())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{ColorProfile, IntegerDepth, RgbSpace};
    use layer_ui::{CommandId, Platform, UiAction};
    fn begin(host: &mut NativeHost, command: CommandId) -> Box<Task> {
        host.dispatch(UiAction::Invoke { command }).unwrap();
        let id = host
            .session
            .state()
            .requests
            .last()
            .expect("command must request native UI")
            .id;
        Task::capture(host, id).unwrap()
    }
    fn ready(task: &mut Task, action: Action) {
        task.work(action);
        assert!(task.error.is_none(), "{}: {:?}", task.kind, task.error);
    }
    fn settle(host: &mut NativeHost) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            host.prepare_canvas_frame(0, 0, true).unwrap();
            if host.startup.complete && !host.session.engine().has_pending_document_edits() {
                break;
            }
            assert!(Instant::now() < deadline, "canvas did not settle");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    #[ignore = "Requires hardware D3D12 and an isolated CAPY_SETTINGS_DIRECTORY"]
    fn d3d12_native_color_import_export_source_and_history_round_trip() {
        let directory = std::path::PathBuf::from(
            std::env::var_os("CAPY_SETTINGS_DIRECTORY").expect("use isolated storage"),
        );
        std::fs::create_dir_all(&directory).unwrap();
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .unwrap();
        let features = adapter.features()
            & (wgpu::Features::FLOAT32_FILTERABLE
                | wgpu::Features::FLOAT32_BLENDABLE
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .unwrap();
        let mut gpu =
            WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue, Default::default())
                .unwrap();
        gpu.finish_startup_cache();
        assert_eq!(gpu.adapter().get_info().backend, wgpu::Backend::Dx12);
        assert_ne!(gpu.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
        let project = layer_ui::new_drawing(24, 18).unwrap();
        let master = directory.join("native-master.capy");
        project
            .write(std::fs::File::create(&master).unwrap())
            .unwrap();
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.session =
            UiSession::from_project(Renderer(Some(gpu)), project, None, [48, 36]).unwrap();
        let environment = crate::documents::Environment::capture(&host).unwrap();
        host.session =
            *crate::documents::prepare_recovery(environment, master, &Default::default()).unwrap();
        host.session.set_platform(Platform::Windows);
        host.session.set_document_replacement(true);
        host.document_adopted();
        settle(&mut host);
        let mut info = begin(&mut host, CommandId::DocumentProperties);
        ready(&mut info, Action::Describe);
        assert!(info.details.is_array());
        info.complete(&mut host, false).unwrap();
        drop(info);
        let initial = host.session.engine().document().color;
        for (command, choice) in [
            (CommandId::AssignProfile, json!({"Assign":"DisplayP3"})),
            (
                CommandId::ConvertColorSpace,
                json!({"Convert":{"space":"ProPhoto","options":{"intent":"RelativeColorimetric","black_point_compensation":false}}}),
            ),
            (
                CommandId::ChangeBitDepth,
                json!({"Depth":{"depth":"U16","dither":"None"}}),
            ),
        ] {
            let mut task = begin(&mut host, command);
            ready(
                &mut task,
                Action::Prepare {
                    choice,
                    copy: false,
                },
            );
            assert_eq!(task.stage, "preview");
            assert!(task.preview(0).is_ok());
            assert!(task.preview(1).is_ok());
            task.commit(&mut host).unwrap();
            drop(task);
            settle(&mut host);
        }
        assert_eq!(
            host.session.engine().document().color.space,
            RgbSpace::ProPhoto
        );
        assert_eq!(
            host.session.engine().document().color.depth,
            IntegerDepth::U16
        );
        for _ in 0..3 {
            let mut undo = begin(&mut host, CommandId::Undo);
            ready(&mut undo, Action::Describe);
            undo.commit(&mut host).unwrap();
            drop(undo);
            settle(&mut host);
        }
        assert_eq!(host.session.engine().document().color, initial);
        let mut redo = begin(&mut host, CommandId::Redo);
        ready(&mut redo, Action::Describe);
        redo.commit(&mut host).unwrap();
        drop(redo);
        settle(&mut host);
        let revision = host.session.engine().document().revision;
        let mut photo = None;
        for format in [
            layer_ui::ExportFormat::Png,
            layer_ui::ExportFormat::Jpeg,
            layer_ui::ExportFormat::Tiff,
        ] {
            let mut task = begin(&mut host, CommandId::ExportDocument);
            ready(&mut task, Action::Describe);
            let mut recipe = layer_ui::ExportRecipe::web_share();
            recipe.format = format;
            recipe.background = layer_ui::ExportBackground::White;
            recipe.size = layer_ui::ExportSize::Fit {
                bounds: [12, 12],
                enlarge: false,
            };
            recipe.resolution = layer_ui::ExportResolution::Ppi(300);
            ready(
                &mut task,
                Action::ExportOptions {
                    recipe,
                    profile_id: None,
                },
            );
            let path = directory.join(format!("delivery.{}", format.extension()));
            ready(
                &mut task,
                Action::ExportWrite {
                    path: path.to_str().unwrap().into(),
                },
            );
            task.complete(&mut host, true).unwrap();
            let imported = layer_ui::read_import(
                std::fs::File::open(&path).unwrap(),
                layer_ui::ImportIntent::Open,
                Default::default(),
                "Delivery",
                Default::default(),
                Default::default(),
                &Default::default(),
            )
            .unwrap();
            assert_eq!(
                [
                    imported.project.document.width,
                    imported.project.document.height
                ],
                [12, 9]
            );
            assert!(
                imported
                    .project
                    .document
                    .layers
                    .iter()
                    .any(|l| l.source.is_some())
            );
            assert!(
                imported
                    .source
                    .adoption_location(Some(layer_ui::DocumentLocation {
                        uri: path.to_str().unwrap().into(),
                        name: "photo".into()
                    }))
                    .is_none()
            );
            if matches!(format, layer_ui::ExportFormat::Png) {
                photo = Some(path);
            }
        }
        assert_eq!(host.session.engine().document().revision, revision);
        let before = host.session.engine().document().layers.len();
        let mut import = begin(&mut host, CommandId::ImportImage);
        let path = photo.unwrap().to_str().unwrap().to_string();
        ready(
            &mut import,
            Action::ReadImages {
                paths: vec![path.clone(), path],
            },
        );
        assert_eq!(import.stage, "commit");
        import.commit(&mut host).unwrap();
        drop(import);
        settle(&mut host);
        assert_eq!(host.session.engine().document().layers.len(), before + 2);
        host.dispatch(UiAction::Invoke {
            command: CommandId::ApplyTransform,
        })
        .unwrap();
        settle(&mut host);
        for command in [CommandId::RepairSourceProfile, CommandId::RasterizeSource] {
            let mut task = begin(&mut host, command);
            ready(
                &mut task,
                Action::Prepare {
                    choice: if command == CommandId::RepairSourceProfile {
                        json!({"Builtin":"DisplayP3"})
                    } else {
                        Value::Null
                    },
                    copy: false,
                },
            );
            assert!(task.prepare_owner(&host).unwrap());
            ready(&mut task, Action::Compare);
            task.commit(&mut host).unwrap();
            drop(task);
            settle(&mut host);
        }
        let mut histogram = begin(&mut host, CommandId::Histogram);
        ready(&mut histogram, Action::Describe);
        assert!(histogram.details.get("histogram").is_some());
        histogram.complete(&mut host, false).unwrap();
        let profile =
            layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
        let profile_path = directory.join("test.icc");
        std::fs::write(&profile_path, &profile).unwrap();
        let cancel = Default::default();
        crate::color_storage::import(&profile_path, &cancel).unwrap();
        let inventory = crate::color_storage::list(&cancel).unwrap();
        let id = layer_ui::profile_library::profile_identity(&profile);
        assert!(inventory.iter().any(|p| p.id == id && p.issue.is_none()));
        crate::color_storage::remove(&id, &cancel).unwrap();
    }
}
