//! Windows scheduling around shared candidate policies. No WinUI callback owns
//! artwork, converts profiles, or decides how an edit enters history.
use layer_core::{Project, color::RgbSpace};
use layer_host::{
    NativeHost,
    clipboard::ClipTask,
    export::ExportTask,
    tasks::{ColorTask, SourceTask},
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use layer_ui::{DocumentRequest, HostRequestKind};
use serde::Deserialize;
use serde_json::{Value, json};

#[path = "document_proof.rs"]
mod proof;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Describe,
    ProofOptions {
        settings: layer_ui::proof_panel::PrintProofSettings,
        profile_id: Option<String>,
    },
    ProofPreserve,
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
    ProfileVisibility {
        id: String,
        visible: bool,
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
    PasteClip {
        nonce: String,
    },
}
struct Import {
    images: layer_ui::ImageImportBatch,
    context: layer_ui::ImagePlacementContext,
    device: wgpu::Device,
    paths: std::collections::VecDeque<String>,
    clipboard: Option<std::path::PathBuf>,
    clip_nonce: Option<String>,
}
impl Drop for Import {
    fn drop(&mut self) {
        if let Some(path) = &self.clipboard {
            let _ = std::fs::remove_file(path);
        }
    }
}
struct Clip {
    task: Option<Box<ClipTask>>,
    clip: Option<Box<layer_ui::PixelClip>>,
    file: std::path::PathBuf,
    large: bool,
}
impl Drop for Clip {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}
static NEXT_CLIPBOARD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn clipboard_file() -> Result<std::path::PathBuf, String> {
    Ok(crate::settings::data_directory()?.join("clipboard").join(format!(
        "{}-{}.png",
        std::process::id(),
        NEXT_CLIPBOARD.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )))
}
use layer_ui::DocumentHostErrorCopy as FeatureFailure;
fn retain_failure(slot: &mut Option<FeatureFailure>, reason: FeatureFailure, localization: &layer_ui::Localizer) -> String {
    let text=reason.message(localization);*slot=Some(reason);text
}
enum Payload {
    Profiles,
    Proof(Box<proof::Task>),
    Export {
        task: Box<ExportTask>,
        destination: usize,
        notice: Option<layer_ui::ColorFeatureError>,
    },
    Import(Import),
    Clip(Clip),
    Color(Box<ColorTask>),
    Source(Box<SourceTask>),
    Info(layer_color::DocumentInfo),
    Histogram {
        project: Option<Box<Project>>,
        gpu: SnapshotGpu,
        background: [f32; 4],
        time: f32,
    },
}
pub(crate) struct Task {
    pub id: u32,
    localization: std::sync::Arc<layer_ui::Localizer>,
    pub control: CaptureControl,
    pub stage: &'static str,
    pub serial: u32,
    kind: &'static str,
    details: Value,
    preset_view: Value,
    feature_copy: Value,
    converted_name: String,
    original_name: String,
    inspection: Option<layer_color::InspectedDocumentInfo>,
    profiles: Vec<layer_ui::profile_library::ProfileEntry>,
    hidden: Vec<String>,
    error: Option<String>,
    error_reason: Option<FeatureFailure>,
    payload: Payload,
}
impl Task {
    pub fn capture(host: &mut NativeHost, id: u32) -> Result<Box<Self>, String> {
        let mut copy = (id != 0 && matches!(host.session.document_request(id), Ok(DocumentRequest::Copy { .. })))
            .then(|| ClipTask::capture(&mut host.session, id).map(Box::new))
            .transpose()?;
        let session = &host.session;
        if id == 0 {
            return Ok(Box::new(Self {
                id,
                localization: session.localization().clone(),
                control: Default::default(),
                stage: "options",
                serial: 0,
                kind: "profiles",
                details: Value::Null,
                preset_view: Value::Null,
                feature_copy: Self::feature_copy(session.localization()),
                inspection: None,
                profiles: Vec::new(), hidden: Vec::new(),
                original_name: session.state().document_file.title().to_string(),
                converted_name: layer_ui::DocumentDeliveryMessage::ConvertedName { name: session.state().document_file.title().to_string() }.message(session.localization()),
                error: None,
                error_reason: None,
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
            HostRequestKind::SoftProofSetup => ("proof", Payload::Proof(Box::new(proof::Task::capture(session, id).map_err(|reason|reason.proof_message(session.localization()))?))),
            HostRequestKind::Document {
                request: DocumentRequest::Export { name },
            } => (
                "export",
                Payload::Export {
                    task: Box::new(ExportTask::capture(session, id, name, RgbSpace::Srgb)?),
                    destination: 0,
                    notice: None,
                },
            ),
            HostRequestKind::Document {
                request: DocumentRequest::Place | DocumentRequest::Paste { .. },
            } => {
                let kind = if matches!(
                    request,
                    HostRequestKind::Document {
                        request: DocumentRequest::Paste { .. }
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
                        clipboard: if kind == "paste" { Some(clipboard_file()?) } else { None },
                        clip_nonce: None,
                    }),
                )
            }
            HostRequestKind::Document {
                request: DocumentRequest::Copy { .. },
            } => {
                let task = copy.take().ok_or("The copy was not captured")?;
                let large = task.capture_details().large;
                ("copy", Payload::Clip(Clip { task: Some(task), clip: None, file: clipboard_file()?, large }))
            }
            HostRequestKind::Document {
                request: DocumentRequest::ChangeColor { operation },
            } => (
                match operation {
                    layer_ui::DocumentColorOperation::Assign => "assign",
                    layer_ui::DocumentColorOperation::Convert => "convert",
                    layer_ui::DocumentColorOperation::Depth => "depth",
                },
                Payload::Color(Box::new(ColorTask::capture(
                    session,
                    Some(id),
                    RgbSpace::Srgb,
                )?)),
            ),
            HostRequestKind::Document {
                request: DocumentRequest::ColorHistory { .. },
            } => (
                "history",
                Payload::Color(Box::new(ColorTask::capture(
                    session,
                    Some(id),
                    RgbSpace::Srgb,
                )?)),
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
                    Payload::Source(Box::new(SourceTask::capture(
                        session,
                        Some(id),
                        RgbSpace::Srgb,
                    )?)),
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
                        project: Some(Box::new(session.capture_project_recovery()?)),
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
            localization: session.localization().clone(),
            control: Default::default(),
            stage: "options",
            serial: 0,
            kind,
            details: Value::Null,
            preset_view: Value::Null,
            feature_copy: Self::feature_copy(session.localization()),
            inspection: None,
            profiles: Vec::new(), hidden: Vec::new(),
            original_name: session.state().document_file.title().to_string(),
            converted_name: layer_ui::DocumentDeliveryMessage::ConvertedName { name: session.state().document_file.title().to_string() }.message(session.localization()),
            error: None,
            error_reason: None,
            payload,
        }))
    }
    fn feature_copy(localization: &layer_ui::Localizer) -> Value {
        use layer_ui::color_feature_copy::{DocumentColorCopy, ExportCopy, ProfileCopy, ProofCopy};
        json!({"color": DocumentColorCopy::new(localization), "export": ExportCopy::new(localization),
            "profile": ProfileCopy::new(localization), "proof": ProofCopy::new(localization)})
    }
    fn profile_views(&self) -> Value {
        json!(self.profiles.iter().map(|entry| {
            let visible = !self.hidden.contains(&entry.id);
            let mut view = entry.localized_view(&self.localization);
            view["visible"] = json!(visible);
            view["state"] = layer_ui::color_feature_copy::profile_visibility(&self.localization, entry.channels, visible).into();
            view
        }).collect::<Vec<_>>())
    }
    fn describe(&mut self) -> Result<(), String> {
        if matches!(self.payload, Payload::Profiles) {
            (self.profiles, self.hidden) = crate::color_storage::library(self.control.cancellation_flag())?;
        } else if matches!(self.payload, Payload::Proof(_) | Payload::Export { .. } | Payload::Import(_) | Payload::Source(_)) {
            self.profiles = crate::color_storage::list(self.control.cancellation_flag())?;
        }
        let profiles = self.profile_views();
        self.details = match &mut self.payload {
            Payload::Profiles => {
                json!({"profiles":profiles.clone()})
            }
            Payload::Proof(task) => task.details(profiles.clone())?,
            Payload::Export { task, .. } => {
                if self.preset_view.is_null() {
                    let view = crate::color_storage::presets(
                        layer_ui::ExportPresetAction::Get { index: 0 },
                        task.document(),
                        self.control.cancellation_flag(),
                        &self.localization,
                    )
                    .or_else(|_| {
                        crate::color_storage::presets(
                            layer_ui::ExportPresetAction::List,
                            task.document(),
                            self.control.cancellation_flag(),
                            &self.localization,
                        )
                    }).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Preset(reason),&self.localization))?;
                    if let Some(recipe) = &view.recipe
                        && task.configure(recipe.clone()).is_err()
                    {
                        task.configure(layer_ui::ExportRecipe::web_share()).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Color(reason),&self.localization))?;
                    }
                    self.preset_view = serde_json::to_value(view).map_err(|e| e.to_string())?;
                }
                let mut details = task.details_localized(&self.localization)?;
                details["presets"] = self.preset_view.clone();
                details["profiles"] = profiles.clone();
                details
            }
            Payload::Import(task) => {
                if let Some(path) = &task.clipboard {
                    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| {
                        crate::document_io::io_error("prepare clipboard storage", e)
                    })?;
                }
                json!({"pending":task.images.pending_source().map(|s| &s.interpretation),"extensions":layer_color::photo::extensions().collect::<Vec<_>>(),
                    "profiles":profiles.clone(), "clipboard":task.clipboard.as_ref().map(|p| json!({"path":p,"folder":p.parent(),"name":p.file_name().and_then(|s| s.to_str())})),
                    "clip_nonce":task.clip_nonce, "delivery":layer_ui::DocumentDeliveryCopy::new(&self.localization)})
            }
            Payload::Clip(clip) => {
                json!({"nonce":clip.clip.as_ref().map(|c| &c.nonce),"file":clip.file,"delivery":layer_ui::DocumentDeliveryCopy::new(&self.localization)})
            }
            Payload::Color(task) => task.details(),
            Payload::Source(task) => {
                task.prepare_metadata()?;
                let mut details = task.details_localized(&self.localization)?;
                details["profiles"] = profiles.clone();
                details
            }
            Payload::Info(info) => {
                self.inspection = Some(info.inspect()?);
                serde_json::to_value(layer_ui::document_properties(self.inspection.as_ref().unwrap(), &self.localization)).map_err(|e| e.to_string())?
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
                        *project,
                        *background,
                        *time,
                        self.control.clone(),
                    )
                    .map_err(|e| e.to_string())?;
                let histogram=renderer.histogram().map_err(|e| e.to_string())?;
                json!({"axis":histogram.axis(),"histogram":histogram,"sampled_time":sampled_time})
            }
        };
        self.details["feature_copy"] = self.feature_copy.clone();
        if matches!(self.payload, Payload::Color(_)) { self.details["suggested_name"] = json!(self.converted_name); }
        Ok(())
    }
    /// Only the file worker calls this; errors keep a reviewable task to cancel.
    pub fn work(&mut self, action: Action) {
        self.error_reason = None;
        let result = (|| {
            if self.control.is_cancelled() {
                return Err("Document operation cancelled".into());
            }
            match action {
                Action::Describe if self.kind == "copy" => {
                    let Payload::Clip(clip) = &mut self.payload else { return Err("No copy is pending".into()) };
                    let task = clip.task.take().ok_or("This copy has already run")?;
                    let copied = task.run(layer_workspace::new_id(), self.control.clone())?;
                    std::fs::create_dir_all(clip.file.parent().ok_or("Clipboard storage is missing")?)
                        .and_then(|_| std::fs::write(&clip.file, &copied.png))
                        .map_err(|e| crate::document_io::io_error("write the copied image", e))?;
                    clip.clip = Some(Box::new(copied));
                    self.stage = "commit";
                    self.describe()
                }
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
                        &self.localization,
                    ).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Profile(reason),&self.localization))?;
                    self.describe()
                }
                Action::ProfileRemove { id } => {
                    crate::color_storage::remove(&id, self.control.cancellation_flag(), &self.localization).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Profile(reason),&self.localization))?;
                    self.describe()
                }
                Action::ProfileVisibility { id, visible } => {
                    crate::color_storage::show(&id, visible, self.control.cancellation_flag()).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Profile(reason),&self.localization))?;
                    self.describe()
                }
                Action::ProofOptions { settings, profile_id } => {
                    let Payload::Proof(task) = &mut self.payload else { return Err("No proof setup is pending".into()); };
                    task.work(settings, profile_id, &self.control).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Proof(reason),&self.localization))?;
                    self.stage = "proof_candidate";
                    self.describe()
                }
                Action::ProofPreserve => {
                    let Payload::Proof(task) = &mut self.payload else { return Err("No proof setup is pending".into()); };
                    task.preserve(&self.control).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Proof(reason),&self.localization))?;
                    self.stage = "commit";
                    Ok(())
                }
                Action::ExportOptions {
                    mut recipe,
                    profile_id,
                } => {
                    let Payload::Export { task, .. } = &mut self.payload else {
                        return Err("No export is pending".into());
                    };
                    if let Some(id) = profile_id {
                        recipe.profile = crate::color_storage::export_profile(&id, self.control.cancellation_flag(), &self.localization).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Profile(reason),&self.localization))?;
                    }
                    task.configure(recipe).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Color(reason),&self.localization))?;
                    task.compare(self.control.clone()).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Color(reason),&self.localization))?;
                    self.stage = "preview";
                    self.describe()
                }
                Action::ExportWrite { path } => {
                    if self.stage != "preview" {
                        return Err("Preview the export first".into());
                    }
                    let Payload::Export {
                        task,
                        destination,
                        notice,
                    } = &mut self.payload
                    else {
                        return Err("No export is pending".into());
                    };
                    crate::document_io::location(&path)?;
                    crate::document_io::atomic_write_seek(
                        std::path::Path::new(&path),
                        self.control.cancellation_flag(),
                        |file| task.write(file, self.control.clone()).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Color(reason),&self.localization)),
                    )?;
                    let remember = layer_ui::ExportPresetAction::Remember {
                        index: (*destination).min(3),
                        recipe: task.recipe().clone(),
                    };
                    if let Err(error) = crate::color_storage::presets(
                        remember,
                        task.document(),
                        self.control.cancellation_flag(),
                        &self.localization,
                    ) {
                        *notice = Some(error);
                    }
                    self.stage = "saved";
                    Ok(())
                }
                Action::ExportPreset {
                    mut action,
                    profile_id,
                } => {
                    if let Some(id) = profile_id {
                        let value = crate::color_storage::export_profile(&id, self.control.cancellation_flag(), &self.localization).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Profile(reason),&self.localization))?;
                        match &mut action {
                            layer_ui::ExportPresetAction::Save { recipe, .. }
                            | layer_ui::ExportPresetAction::Update { recipe, .. }
                            | layer_ui::ExportPresetAction::Remember { recipe, .. } => {
                                recipe.profile = value
                            }
                            _ => {}
                        }
                    }
                    let Payload::Export {
                        task, destination, ..
                    } = &mut self.payload
                    else {
                        return Err("No export is pending".into());
                    };
                    let view = crate::color_storage::presets(
                        action,
                        task.document(),
                        self.control.cancellation_flag(),
                        &self.localization,
                    ).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Preset(reason),&self.localization))?;
                    if let Some(recipe) = &view.recipe {
                        task.configure(recipe.clone()).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Color(reason),&self.localization))?;
                    }
                    if let Some(index) = view.index {
                        *destination = index;
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
                        profile.resolve(self.control.cancellation_flag(), &self.localization).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Profile(reason),&self.localization))?,
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
                            task.work(serde_json::from_value::<Option<crate::color_storage::ProfileChoice>>(choice).map_err(|e| e.to_string())?.map(|p| p.resolve(self.control.cancellation_flag(), &self.localization)).transpose().map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Profile(reason),&self.localization))?, || self.control.is_cancelled())?;
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
            let name = if task.clipboard.as_deref() == Some(path) {
                "Pasted image"
            } else {
                path.file_stem().and_then(|s| s.to_str()).unwrap_or("Image")
            };
            task.images.read(
                layer_core::Cancellable {
                    inner: file,
                    cancelled: || self.control.is_cancelled(),
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
        matches!(self.payload, Payload::Import(_) | Payload::Clip(_))
    }
    pub fn offer_clip(&mut self, nonce: Option<String>) {
        if let Payload::Import(task) = &mut self.payload { task.clip_nonce = nonce; }
    }
    pub fn awaits_clipboard(&self) -> bool {
        matches!(self.payload, Payload::Clip(_))
    }
    pub fn quiet(&self) -> bool {
        matches!(&self.payload, Payload::Clip(clip) if !clip.large)
    }
    pub fn progress_title(&self, host: &NativeHost) -> Option<String> {
        if !matches!(self.payload, Payload::Clip(_)) { return None; }
        host.session.document_request(self.id).ok().map(|request| request.title(host.session.localization()).to_string())
    }
    pub fn take_clip(&mut self) -> Option<layer_ui::PixelClip> {
        match &mut self.payload { Payload::Clip(clip) => clip.clip.take().map(|clip| *clip), _ => None }
    }
    pub fn prepare_owner(&mut self, host: &NativeHost) -> Result<bool, String> {
        if self.stage == "proof_candidate" {
            self.error_reason=None;
            let Payload::Proof(task) = &mut self.payload else { return Err("No proof candidate".into()); };
            task.validate(host, &self.control).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Proof(reason),&self.localization))?;
            self.stage = "proof_preserve";
            return Ok(true);
        }
        if self.stage != "source_candidate" {
            return Ok(false);
        }
        self.error_reason=None;
        let Payload::Source(task) = &mut self.payload else {
            return Err("No source candidate".into());
        };
        task.prepare(host, self.control.is_cancelled())?;
        Ok(true)
    }
    pub fn commit(&mut self, host: &mut NativeHost) -> Result<(), String> {
        self.error_reason=None;
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
                let sources = task.images.take_sources(self.control.is_cancelled())?;
                match session.document_request(self.id)? {
                    DocumentRequest::Paste { mode } => { let mode = *mode; session.paste_layer_sources(sources, mode, &task.context)? }
                    _ => session.place_layer_sources(sources, task.context.center, task.context.destination)?,
                }
                let mut change = session.complete_document_request(self.id, Ok(true))?;
                change.canvas_wake = true;
                change.regions |= 255;
                host.apply_change(previous, change);
                Ok(())
            }
            Payload::Proof(task) => task.adopt(host, &self.control).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Proof(reason),&self.localization)),
            Payload::Color(task) => task.adopt(host, self.control.is_cancelled(), || true),
            Payload::Source(task) => task.adopt(host, self.control.is_cancelled(), || true),
            _ => Err("No document edit is ready".into()),
        }
    }
    pub fn complete(&self, host: &mut NativeHost, saved: bool) -> Result<(), String> {
        if self.id == 0 {
            return Ok(());
        }
        if matches!(self.kind, "histogram" | "proof") {
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
    pub fn notice(&self) -> Option<layer_ui::DocumentHostErrorCopy> {
        match &self.payload {
            Payload::Export { notice, .. } => notice.clone().map(layer_ui::DocumentHostErrorCopy::ExportPreferences),
            _ => None,
        }
    }
    pub fn retain_proof(&mut self, view: &mut layer_ui::proof_workflow::ProofView) -> Result<(), String> {
        if let Payload::Proof(task) = &self.payload { task.retain(view).map_err(|reason| retain_failure(&mut self.error_reason,FeatureFailure::Proof(reason),&self.localization))?; }
        Ok(())
    }
    pub fn retained_failure(&self) -> bool {self.error_reason.is_some()}
    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.stage = "error";
        self.serial = self.serial.saturating_add(1);
    }
    pub(crate) fn set_localization(&mut self, localization: std::sync::Arc<layer_ui::Localizer>) -> Result<(), String> {
        if std::sync::Arc::ptr_eq(&self.localization, &localization) { return Ok(()); }
        self.localization = localization;
        if let Some(reason)=&self.error_reason { self.error=Some(reason.message(&self.localization)); }
        self.feature_copy = Self::feature_copy(&self.localization);
        self.converted_name = layer_ui::DocumentDeliveryMessage::ConvertedName { name:self.original_name.clone() }.message(&self.localization);
        match &mut self.payload {
            Payload::Export { task, .. } => {
                let captions: Vec<layer_ui::ExportProfileCaption> = self.details["form"].get("profile_captions")
                    .map(|value| serde_json::from_value(value.clone())).transpose().map_err(|error| error.to_string())?.unwrap_or_default();
                let next = task.copy_localized(&self.localization, &captions);
                for key in ["copy", "profile_names", "recipe_profile_name"] { self.details["form"][key] = next[key].clone(); }
                for key in ["suggested_name", "format_name"] { self.details[key] = next[key].clone(); }
                if !self.preset_view.is_null() {
                    let names = serde_json::from_value(self.preset_view["names"].clone()).map_err(|error| error.to_string())?;
                    let mut view = layer_ui::ExportPresetView { names, index:None, recipe:None, changed:false };
                    view.localize_names(task.document().color, &self.localization);
                    self.preset_view["names"] = json!(view.names);
                    self.details["presets"]["names"] = self.preset_view["names"].clone();
                }
            }
            Payload::Source(task) => {
                let next = task.details_localized(&self.localization)?;
                for (key, value) in next.as_object().ok_or("Invalid source presentation")? { self.details[key] = value.clone(); }
            }
            Payload::Proof(task) => task.relocalize(&mut self.details, self.localization.clone()),
            Payload::Info(_) => if let Some(inspection) = &self.inspection {
                self.details = serde_json::to_value(layer_ui::document_properties(inspection, &self.localization)).map_err(|error| error.to_string())?;
            },
            _ => {},
        }
        if self.details.get("profiles").is_some() { self.details["profiles"] = self.profile_views(); }
        self.details["feature_copy"] = self.feature_copy.clone();
        Ok(())
    }
    pub fn status(&self) -> Value {
        json!({"type":"workflow", "id":self.id,"serial":self.serial,"kind":self.kind,
            "stage":self.stage,"details":self.details,"error":self.error,
            "spaces":layer_core::color::RgbSpace::ALL.map(|s| (s,s.name()))})
    }
    pub fn preview(&self, index: usize) -> Result<crate::previews::CapyPreview, String> {
        let previews = match &self.payload {
            Payload::Color(task) => task.previews(),
            Payload::Source(task) => task.previews(),
            Payload::Export { task, .. } => task.previews(),
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
    use layer_core::color::{ColorProfile, SampleDepth};
    use layer_host::Renderer;
    use layer_render_wgpu::WgpuRasterizer;
    use layer_ui::{CommandId, Platform, UiAction, UiSession};
    use std::time::{Duration, Instant};
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
    #[test]
    fn proof_worker_refusal_survives_owner_noop_and_language_refresh() {
        let mut host=NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.dispatch(layer_ui::UiAction::Invoke {command:layer_ui::CommandId::SoftProofSetup}).unwrap();
        let id=host.session.state().requests.last().unwrap().id;
        let mut task=Task::capture(&mut host,id).unwrap();
        task.work(Action::ProofOptions {settings:layer_ui::proof_panel::PrintProofSettings::default(),profile_id:None});
        assert_eq!(task.error_reason,Some(FeatureFailure::Proof(layer_ui::ColorFeatureError::ProofChoosePrintProfile)));
        assert!(!task.prepare_owner(&host).unwrap());
        let reason=task.error_reason.clone().unwrap();
        let checkpoint=host.session.engine().checkpoint();
        for language in layer_ui::UiLanguage::ALL {
            let localizer=layer_ui::Localizer::shared(language);
            task.set_localization(localizer.clone()).unwrap();
            assert_eq!(task.error,Some(reason.message(&localizer)));
            assert_eq!(task.error_reason,Some(reason.clone()));
            assert_eq!(host.session.engine().checkpoint(),checkpoint);
        }
        let literal=layer_ui::ColorFeatureError::Diagnostic("{\"ProofChooseProfile\":true} literal CMM text".into());
        task.error_reason=Some(FeatureFailure::Proof(literal.clone()));
        task.fail(literal.proof_message(host.session.localization()));
        task.set_localization(layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese)).unwrap();
        assert_eq!(task.error,Some(literal.proof_message(host.session.localization())));
    }
    #[test]
    fn properties_reproject_cached_inspection_without_restarting_the_workflow() {
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        let checkpoint = host.session.state().document_file.clone();
        let mut task = begin(&mut host, CommandId::DocumentProperties);
        ready(&mut task, Action::Describe);
        let identity = (task.id, task.serial, task.stage);
        let inspected = task.inspection.as_ref().unwrap() as *const _;
        let english = task.details.clone();
        let japanese = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        host.set_localization(japanese.clone());task.set_localization(japanese.clone()).unwrap();
        assert_eq!((task.id, task.serial, task.stage), identity);
        assert_eq!(task.inspection.as_ref().unwrap() as *const _, inspected);
        assert_ne!(task.details["title"], english["title"]);
        assert_eq!(task.details["title"], layer_ui::document_properties(task.inspection.as_ref().unwrap(), &japanese).title.as_ref());
        assert_eq!(host.session.state().document_file.epoch, checkpoint.epoch);
        assert_eq!(host.session.state().document_file.revision, checkpoint.revision);
        task.set_localization(layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        assert_eq!(task.details, english);
    }
    #[test]
    fn late_profile_inventory_reprojects_semantic_issues_without_reinspection() {
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        let mut task = Task::capture(&mut host, 0).unwrap();
        let entry = layer_ui::profile_library::ProfileEntry {
            id: "user-profile".into(), bytes: 0, name: "My literal name".into(), channels: None, profile: None,
            issue: Some(layer_ui::ColorFeatureError::ProfileMissing),
        };
        task.profiles.push(entry.clone());
        task.hidden.push(entry.id.clone());
        task.details = json!({"profiles":[entry.localized_view(host.session.localization())]});
        let identity = (task.id, task.serial, task.stage);
        let japanese = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        task.set_localization(japanese.clone()).unwrap();
        assert_eq!((task.id, task.serial, task.stage), identity);
        let mut expected = entry.localized_view(&japanese);
        expected["visible"] = json!(false);
        expected["state"] = layer_ui::color_feature_copy::profile_visibility(&japanese, None, false).into();
        assert_eq!(task.details["profiles"][0], expected);
        assert_eq!(task.profiles[0].name, "My literal name");
        assert_eq!(task.profiles[0].issue, entry.issue);
    }
    #[test]
    fn late_profile_failure_reprojects_typed_reason_and_retains_the_task() {
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        let mut task = Task::capture(&mut host, 0).unwrap();
        task.work(Action::ProfileRemove { id: "../invalid".into() });
        let english=task.error.clone().expect("invalid profile must fail before storage");
        let identity=(task.id,task.serial,task.stage);
        let japanese=layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        task.set_localization(japanese.clone()).unwrap();
        assert_eq!((task.id,task.serial,task.stage),identity);
        assert_ne!(task.error.as_ref(),Some(&english));
        assert_eq!(task.error.as_ref(),Some(&task.error_reason.as_ref().unwrap().message(&japanese)));
        task.set_localization(host.session.localization().clone()).unwrap();
        assert_eq!(task.error,Some(english));
    }
    fn pixels(gpu: &mut WgpuRasterizer) -> Vec<u8> {
        gpu.readback_srgb_rgba8().unwrap()
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
        let project = layer_ui::new_drawing(24, 18, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let master = directory.join("native-master.capy");
        project
            .write(std::fs::File::create(&master).unwrap())
            .unwrap();
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.session =
            UiSession::from_project(Renderer(Some(gpu.into())), project, None, [48, 36], Platform::Windows).unwrap();
        let environment = crate::documents::recovery_environment(&host.session).unwrap();
        host.session =
            *crate::documents::prepare_recovery(environment, master, &Default::default()).unwrap();
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
            SampleDepth::U16
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
        let mut remembered = None;
        for format in [
            layer_ui::ExportFormat::Png,
            layer_ui::ExportFormat::Jpeg,
            layer_ui::ExportFormat::Tiff,
        ] {
            let mut task = begin(&mut host, CommandId::ExportDocument);
            ready(&mut task, Action::Describe);
            assert_eq!(task.details["suggested_name"], "Untitled");
            if let Some(previous) = remembered.replace(format) {
                assert_eq!(task.details["recipe"]["format"], serde_json::to_value(previous).unwrap());
                assert_eq!(task.details["presets"]["index"], 0);
            }
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
            let identity = (task.id, task.serial, task.stage);
            let Payload::Export { task:export, .. } = &task.payload else { panic!("export task missing") };
            let preview = export.previews().as_ptr();
            let retained_recipe = task.details["recipe"].clone();
            let retained_profiles = task.details["form"]["profiles"].clone();
            let retained_preset_recipe = task.preset_view["recipe"].clone();
            for language in layer_ui::UiLanguage::ALL {
                let localization = layer_ui::Localizer::shared(language);
                task.set_localization(localization.clone()).unwrap();
                assert_eq!((task.id,task.serial,task.stage),identity);
                let Payload::Export { task:export, .. } = &task.payload else { panic!("export task missing") };
                assert_eq!(export.previews().as_ptr(),preview);
                assert_eq!(task.details["recipe"],retained_recipe);
                assert_eq!(task.details["form"]["profiles"],retained_profiles);
                assert_eq!(task.preset_view["recipe"],retained_preset_recipe);
                assert_eq!(task.details["form"]["copy"],json!(layer_ui::color_feature_copy::ExportCopy::new(&localization)));
                let captions: Vec<layer_ui::ExportProfileCaption> = serde_json::from_value(task.details["form"]["profile_captions"].clone()).unwrap();
                assert_eq!(task.details["form"]["profile_names"],json!(captions.iter().map(|caption|caption.message(&localization)).collect::<Vec<_>>()));
            }
            task.set_localization(host.session.localization().clone()).unwrap();
            let path = directory.join(format!("delivery.{}", format.extension()));
            ready(
                &mut task,
                Action::ExportWrite {
                    path: path.to_str().unwrap().into(),
                },
            );
            let reason = layer_ui::DocumentHostErrorCopy::ExportPreferences(layer_ui::ColorFeatureError::PresetNameInvalid);
            let Payload::Export { notice, .. } = &mut task.payload else { panic!("export task missing") };
            *notice = Some(layer_ui::ColorFeatureError::PresetNameInvalid);
            task.complete(&mut host, true).unwrap();
            host.session.set_host_error_copy(task.notice());
            let id = task.id;
            drop(task);
            let checkpoint = host.session.engine().checkpoint();
            let document = host.session.engine().document().clone();
            let requests = json!(host.session.state().requests);
            assert!(!host.session.state().requests.iter().any(|request|request.id==id));
            for language in layer_ui::UiLanguage::ALL {
                let localization = layer_ui::Localizer::shared(language);
                host.set_localization(localization.clone());
                assert_eq!(host.session.state().host_error,Some(reason.message(&localization)));
                assert!(host.error.is_none());
                assert_eq!(host.session.engine().checkpoint(),checkpoint);
                assert_eq!(host.session.engine().document(),&document);
                assert_eq!(json!(host.session.state().requests),requests);
            }
            host.session.set_host_error(None);
            host.set_localization(layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
            let imported = layer_ui::read_import(
                std::fs::File::open(&path).unwrap(),
                layer_ui::ImportIntent::Open,
                Default::default(),
                layer_ui::photo_document_names("Delivery", &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)),
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
        let target = host.session.engine().document().active_layer.0;
        assert!(host.layer_thumbnails([(90, target)]).unwrap().0.is_empty(), "unrendered imports cannot publish a thumbnail");
        settle(&mut host);
        // Simulate unrelated shader warmup after the document frame settled.
        // This used to starve native thumbnails until every warmup job finished.
        host.dirty = true;
        host.startup.complete = false;
        let (accepted, mut thumbnails) = host.layer_thumbnails([(91, target)]).unwrap();
        assert_eq!(accepted, vec![91]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while thumbnails.is_empty() {
            assert!(Instant::now() < deadline, "thumbnail map did not complete");
            thumbnails = host.layer_thumbnails([]).unwrap().1;
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!((thumbnails[0].request_id, thumbnails[0].bytes.len()), (91, 4096));
        host.startup.complete = true;
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
        crate::color_storage::import(&profile_path, &cancel, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let inventory = crate::color_storage::list(&cancel).unwrap();
        let id = layer_ui::profile_library::profile_identity(&profile);
        assert!(inventory.iter().any(|p| p.id == id && p.issue.is_none()));
        crate::color_storage::remove(&id, &cancel, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();

        let mut unnamed = profile.clone();
        let tags = u32::from_be_bytes(unnamed[128..132].try_into().unwrap()) as usize;
        for tag in unnamed[132..132 + tags * 12].chunks_exact_mut(12) {
            if &tag[..4] == b"desc" { tag[..4].copy_from_slice(b"zzzz"); }
        }
        assert_eq!(layer_color::profile_description_optional(&ColorProfile::Icc(unnamed.clone().into())).unwrap(), None);
        std::fs::write(&profile_path, &unnamed).unwrap();
        crate::color_storage::import(&profile_path, &cancel, host.session.localization()).unwrap();
        let unnamed_id = layer_ui::profile_library::profile_identity(&unnamed);
        for language in layer_ui::UiLanguage::ALL {
            let localization = layer_ui::Localizer::shared(language);
            let candidate = crate::color_storage::export_profile(&unnamed_id, &cancel, &localization).unwrap();
            assert!(candidate.name.is_empty());
            assert_eq!(candidate.profile, ColorProfile::Icc(unnamed.clone().into()));
            assert_eq!(candidate.display_name(&localization), localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).as_ref());
        }
        crate::color_storage::remove(&unnamed_id, &cancel, host.session.localization()).unwrap();

        // Proof uses the real Windows transport, shared transaction and D3D12
        // presenter. Viewing never enters the project/export or edit history.
        let before_proof = host.session.engine().checkpoint();
        let source_pixels = pixels(host.session.renderer_mut().0.as_mut().unwrap());
        let mut setup = begin(&mut host, CommandId::SoftProofSetup);
        let embedded = layer_core::color::ProofRecipe::new("Embedded P3".into(), ColorProfile::Icc(profile.clone().into()));
        ready(&mut setup, Action::ProofOptions {
            settings: layer_ui::proof_panel::PrintProofSettings::from_recipe(&embedded).unwrap(),
            profile_id: None,
        });
        assert!(setup.prepare_owner(&host).unwrap());
        ready(&mut setup, Action::ProofPreserve);
        setup.commit(&mut host).unwrap();
        let mut view = layer_ui::proof_workflow::ProofView::default();
        setup.retain_proof(&mut view).unwrap();
        assert!(!view.observe(&host.session).needed);
        assert_eq!(host.session.engine().document().proof, Some(embedded.clone()));
        let proof_checkpoint = host.session.engine().checkpoint();
        let gpu = host.session.renderer_mut().0.as_mut().unwrap();
        let mut presenter = layer_render_wgpu::ViewportPresenter::for_surface(gpu, wgpu::TextureFormat::Rgba8Unorm, layer_render_wgpu::SdrSurfaceColor::Srgb).unwrap();
        for command in [CommandId::GamutWarning, CommandId::SoftProof, CommandId::GamutWarning, CommandId::SoftProof] {
            host.dispatch(UiAction::Invoke { command }).unwrap();
            let lut = view.lut(&host.session);
            let (enabled, warning) = (host.session.state().soft_proof, host.session.state().gamut_warning);
            let gpu = host.session.renderer_mut().0.as_mut().unwrap();
            presenter.set_proof(gpu, lut, enabled, warning).unwrap();
            assert_eq!(pixels(gpu), source_pixels);
            assert_eq!(host.session.engine().checkpoint(), proof_checkpoint);
        }
        let portable = directory.join("proof-portable.capy");
        host.session.capture_project_recovery().unwrap().write(std::fs::File::create(&portable).unwrap()).unwrap();
        let environment = crate::documents::recovery_environment(&host.session).unwrap();
        let restored = crate::documents::prepare_recovery(environment, portable, &Default::default()).unwrap();
        assert_eq!(restored.engine().document().proof, Some(embedded.clone()));
        assert!(!restored.state().soft_proof && !restored.state().gamut_warning);
        let mut replacement = begin(&mut host, CommandId::SoftProofSetup);
        let replacement_recipe = layer_core::color::ProofRecipe::new("sRGB".into(), ColorProfile::Builtin(RgbSpace::Srgb));
        ready(&mut replacement, Action::ProofOptions {
            settings: layer_ui::proof_panel::PrintProofSettings::from_recipe(&replacement_recipe).unwrap(), profile_id: None,
        });
        assert!(replacement.commit(&mut host).is_err());
        assert!(replacement.prepare_owner(&host).unwrap());
        ready(&mut replacement, Action::ProofPreserve);
        assert!(crate::color_storage::list(&cancel).unwrap().iter().any(|p| p.id == id));
        replacement.commit(&mut host).unwrap();
        replacement.retain_proof(&mut view).unwrap();
        assert_eq!(host.session.engine().document().proof, Some(replacement_recipe.clone()));
        host.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        assert_eq!(host.session.engine().document().proof, Some(embedded));
        host.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        assert!(host.session.engine().document().proof.is_none());
        assert_eq!(host.session.engine().checkpoint(), before_proof);
        host.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
        host.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
        assert_eq!(host.session.engine().document().proof, Some(replacement_recipe));
        assert_eq!(pixels(host.session.renderer_mut().0.as_mut().unwrap()), source_pixels);
    }
    #[cfg(target_os = "windows")]
    include!("document_hdr_tests.rs");

}
