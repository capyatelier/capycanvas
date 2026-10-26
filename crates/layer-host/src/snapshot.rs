//! One snapshot schema for the streaming native transports.
use super::{NativeHost, SnapshotKey};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::json;

/// Value's serializer widens f32 to f64. Preserve those exact JSON numbers when
/// bypassing Value so geometry, color channels and settings retain their values.
struct SnapshotFormatter;
impl serde_json::ser::Formatter for SnapshotFormatter {
    fn write_f32<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        value: f32,
    ) -> std::io::Result<()> {
        serde_json::ser::Formatter::write_f64(
            &mut serde_json::ser::CompactFormatter,
            writer,
            f64::from(value),
        )
    }
}

/// Append the fields of a serialized host extension object to a published
/// snapshot object without parsing either one.
pub fn extend_update(mut bytes: Vec<u8>, extension: &[u8]) -> Vec<u8> {
    if extension.len() <= 2 {
        return bytes;
    }
    let closing = bytes.pop();
    debug_assert_eq!(closing, Some(b'}'));
    bytes.push(b',');
    bytes.extend_from_slice(&extension[1..]);
    bytes
}

pub(crate) type RawFields<'a> = std::collections::BTreeMap<&'a str, &'a serde_json::value::RawValue>;

pub struct SnapshotFields<'a> {
    bytes: &'a [u8],
    fields: RawFields<'a>,
}
impl<'a> SnapshotFields<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, serde_json::Error> {
        Ok(Self { bytes, fields: serde_json::from_slice(bytes)? })
    }
    pub fn contains(&self, key: &str) -> bool { self.fields.contains_key(key) }
    pub fn range(&self, key: &str) -> Option<std::ops::Range<usize>> {
        self.fields.get(key).map(|value| {
            let start = value.get().as_ptr() as usize - self.bytes.as_ptr() as usize;
            start..start + value.get().len()
        })
    }
    pub fn nonempty_object_range(&self, key: &str) -> Result<Option<std::ops::Range<usize>>, serde_json::Error> {
        let Some(value) = self.fields.get(key) else { return Ok(None) };
        if !value.get().starts_with('{') { return Ok(None) }
        let object: RawFields<'_> = serde_json::from_str(value.get())?;
        Ok((!object.is_empty()).then(|| self.range(key).unwrap()))
    }
}

impl NativeHost {
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>,serde_json::Error> {
        let mut bytes=Vec::new();
        let mut serializer=serde_json::Serializer::with_formatter(&mut bytes,SnapshotFormatter);
        self.serialize_snapshot(&mut serializer,&self.session.workspace_update(),false)?;
        Ok(bytes)
    }

    /// Shared incremental publication: a full snapshot establishes model_revision;
    /// later workspace_update messages replace only transient geometry. Consumers
    /// apply matching revisions at placement/draw time and retain panel content.
    /// Full snapshots and motion use the same acknowledgement/serialization path.
    pub fn take_update_bytes(&mut self) -> Result<Option<Vec<u8>>, serde_json::Error> {
        self.model_transport = false;
        self.last_model_snapshot = None;
        let mut bytes = Vec::new();
        let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, SnapshotFormatter);
        Ok(self
            .take_snapshot_with(&mut serializer, false)?
            .map(|()| bytes))
    }

    /// Opt-in live reflow: retain controls at content_revision while applying a
    /// complete resolved layout and current camera/measurements, including viewport changes.
    /// Consumers of take_update_bytes refresh all models for dimension changes.
    pub fn take_layout_update_bytes(&mut self) -> Result<Option<Vec<u8>>, serde_json::Error> {
        self.model_transport = false;
        self.last_model_snapshot = None;
        self.layout_update_bytes()
    }

    pub(super) fn layout_update_bytes(&mut self) -> Result<Option<Vec<u8>>, serde_json::Error> {
        let mut bytes = Vec::new();
        let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, SnapshotFormatter);
        Ok(self
            .take_snapshot_with(&mut serializer, true)?
            .map(|()| bytes))
    }

    /// Painting is reported ready only once a contact would paint: the brush
    /// is prepared and the workspace accepts canvas input, as a loading one
    /// does not.
    fn shader_progress(&self) -> layer_render_wgpu::StartupProgress {
        let brush_ready = self.session.engine().backend().0.is_some() && self.session.paint_ready() && !self.session.workspace_read_only();
        layer_render_wgpu::StartupProgress {
            brush_ready,
            complete: self.startup.complete && brush_ready,
            ..self.startup
        }
    }

    pub(super) fn take_snapshot_with<S: Serializer>(
        &mut self,
        serializer: S,
        layout: bool,
    ) -> Result<Option<S::Ok>, S::Error> {
        let key = SnapshotKey {
            revision: self.session.state().revision,
            localization_generation: self.localization_generation(),
            command_search_revision: self.session.command_search_revision(),
            logical: self.logical,
            chrome_hidden: self.chrome_hidden,
            keep_zen_button: self.keep_zen_button,
            navigation_cursor: self.session.navigation_mode(),
            gpu_ready: self.session.engine().backend().0.is_some(),
            startup: self.shader_progress(),
            error: self.error.clone(),
        };
        let camera = &self.session.state().camera;
        if self.last_snapshot.as_ref() == Some(&key)
            && self.last_workspace_model_revision == Some(self.session.workspace_model_revision())
        {
            if self.last_camera_revision != Some(camera.revision) {
                let snapshot =
                    json!({"camera": camera, "revision": key.revision}).serialize(serializer)?;
                self.last_camera_revision = Some(camera.revision);
                return Ok(Some(snapshot));
            }
            return Ok(None);
        }
        // Search has independent state: retaining the workspace model must not
        // mistake a query/open/close for an ordinary geometry-only publication.
        if self.last_workspace_model_revision == Some(self.session.workspace_model_revision())
            && self.last_camera_revision == Some(camera.revision)
            && self.last_snapshot.as_ref().is_some_and(|previous| {
                previous.command_search_revision != key.command_search_revision
                    && SnapshotKey {
                        revision: key.revision,
                        command_search_revision: key.command_search_revision,
                        ..previous.clone()
                    } == key
            })
        {
            #[derive(Serialize)]
            struct Search<'a> {
                command_search: &'a Option<layer_ui::CommandSearchView>,
                revision: u64,
            }
            let snapshot = Search {
                command_search: &self.session.state().command_search,
                revision: key.revision,
            }.serialize(serializer)?;
            self.last_snapshot = Some(key);
            return Ok(Some(snapshot));
        }
        let update = self.session.workspace_update();
        if self.last_workspace_model_revision == Some(update.model_revision)
            && self.last_snapshot.as_ref().is_some_and(|previous| {
                SnapshotKey {
                    revision: key.revision,
                    ..previous.clone()
                } == key
            })
        {
            #[derive(Serialize)]
            struct Motion<'a> {
                workspace_update: &'a layer_ui::WorkspaceUpdate,
                #[serde(skip_serializing_if = "Option::is_none")]
                color_preview: Option<layer_ui::PickerPreview<'a>>,
                #[serde(skip_serializing_if = "Option::is_none")]
                camera: Option<&'a layer_ui::Camera>,
            }
            let snapshot = Motion {
                workspace_update: &update,
                color_preview: self.session.state().layer_tools.tool.picks_color().then(|| self.color_preview()),
                camera: (self.last_camera_revision != Some(camera.revision)).then_some(camera),
            }
            .serialize(serializer)?;
            self.last_camera_revision = Some(camera.revision);
            self.last_snapshot = Some(key);
            return Ok(Some(snapshot));
        }
        if layout
            && self.last_workspace_content_revision == Some(update.content_revision)
            && self.last_snapshot.as_ref().is_some_and(|previous| {
                SnapshotKey {
                    revision: key.revision,
                    logical: key.logical,
                    ..previous.clone()
                } == key
            })
        {
            let snapshot = self
                .session
                .workspace_layout_update(self.logical)
                .serialize(serializer)?;
            self.last_snapshot = Some(key);
            self.last_workspace_model_revision = Some(update.model_revision);
            self.last_camera_revision = Some(camera.revision);
            return Ok(Some(snapshot));
        }
        let snapshot = self.serialize_snapshot(serializer, &update,self.model_transport)?;
        // A serializer failure cannot acknowledge an update that was not sent.
        self.last_snapshot = Some(key);
        self.last_workspace_content_revision = Some(update.content_revision);
        self.last_workspace_model_revision = Some(update.model_revision);
        self.last_camera_revision = Some(self.session.state().camera.revision);
        Ok(Some(snapshot))
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self) -> serde_json::Value {
        self.serialize_snapshot(serde_json::value::Serializer, &self.session.workspace_update(),self.model_transport)
            .expect("Native snapshot contains JSON-compatible fields")
    }

    #[cfg(test)]
    pub(super) fn take_value(&mut self) -> Option<serde_json::Value> {
        self.take_update_bytes()
            .expect("Native snapshot contains JSON-compatible fields")
            .map(|bytes| serde_json::from_slice(&bytes).unwrap())
    }

    fn color_view(&self, colors: &layer_ui::ColorState) -> layer_ui::ColorPanelView {
        match self.ui_color {
            crate::UiColor::Mapped => colors.view_mapped(self.session.effective_sdr_rendition(), self.session.localization()),
            crate::UiColor::Tagged(space) => colors.view_in_localized(space, self.session.localization()),
        }
    }
    pub(crate) fn swatch_preview(&self, colors: &layer_ui::ColorState, color: layer_core::color::RgbColor) -> [f32; 4] {
        match self.ui_color {
            crate::UiColor::Mapped => colors.mapped_swatch(color, self.session.effective_sdr_rendition()),
            crate::UiColor::Tagged(space) => colors.preview_in(color, space),
        }
    }
    fn color_preview(&self) -> layer_ui::PickerPreview<'_> {
        let state = self.session.state();
        let colors = state.preview_colors();
        let view = self.color_view(&colors);
        let bubble_colors = self.session.bubble_color_previews(&colors);
        layer_ui::PickerPreview { picker: &state.color_picker, colors, view, bubble_colors }
    }
    fn serialize_snapshot<S: Serializer>(
        &self,
        serializer: S,
        update: &layer_ui::WorkspaceUpdate,
        model_transport: bool,
    ) -> Result<S::Ok, S::Error> {
        let layout = self.session.layout(self.logical);
        let state = self.session.state();
        let bubbles = self.session.bubble_presentations();
        let mut panel_ids: Vec<_> = layout
            .groups
            .iter()
            .flat_map(|g| &g.panels)
            .copied()
            .collect();
        for panel in layout
            .collapsed
            .iter()
            .flat_map(|c| &c.groups)
            .flat_map(|g| &g.icons)
            .map(|i| i.panel)
            .chain(
                state
                    .customization
                    .drawer
                    .iter()
                    .chain(&state.customization.column_drawers)
                    .flat_map(|d| d.panels()),
            )
            .chain(bubbles.iter().filter(|bubble| bubble.view.card.is_some()).flat_map(|_| [layer_ui::Panel::Layers, layer_ui::Panel::Properties]))
        {
            if !panel_ids.contains(&panel) {
                panel_ids.push(panel);
            }
        }
        let panels: Vec<_> = panel_ids
            .into_iter()
            .filter_map(|p| self.session.panel_view(p).ok())
            .collect();
        #[derive(Serialize)]
        struct Menu {
            id: layer_ui::ApplicationMenu,
            label: std::sync::Arc<str>,
            model: layer_ui::ContextMenu,
        }
        let menus = layer_ui::ApplicationMenu::ALL.map(|id| Menu {
            id,
            label: id.localized_label(self.session.localization()),
            model: self.session.application_menu(id),
        });
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("workspace_update", update)?;
        map.serialize_entry("state", state)?;
        map.serialize_entry("layout", &layout)?;
        map.serialize_entry("panels", &panels)?;
        map.serialize_entry("bubbles", &bubbles)?;
        map.serialize_entry(
            "filter_preview_revision",
            &self.session.filter_preview_revision(),
        )?;
        map.serialize_entry("application_menus", &menus)?;
        map.serialize_entry("header", &self.session.header_view_with(!model_transport))?;
        map.serialize_entry("proof_panel", &layer_ui::color_management::proof_view(&self.session))?;
        map.serialize_entry("color_preview", &self.color_preview())?;
        let colors = state.display_colors();
        map.serialize_entry("color_panel", &self.color_view(colors))?;
        map.serialize_entry("color_edit_context", &self.session.color_edit_context())?;
        map.serialize_entry("color_wheel_context", &self.session.color_wheel_context())?;
        let (display, rendition) = match self.ui_color {
            crate::UiColor::Mapped => (layer_core::color::RgbSpace::Srgb, Some(self.session.effective_sdr_rendition())),
            crate::UiColor::Tagged(space) => (space, None),
        };
        map.serialize_entry("paint_pair", &colors.paint_pair(display, rendition))?;
        map.serialize_entry(
            "palette_panel",
            &layer_ui::PalettePanelView::new(colors, &state.color_library, |color| {
                self.swatch_preview(colors, color)
            }, self.session.localization()),
        )?;
        let document_spec = layer_ui::new_document_spec(self.session.localization());
        map.serialize_entry("document_options", &json!({
            "unsaved_description": document_spec.unsaved_description, "discard_label": document_spec.discard,
            "cancel_label": document_spec.cancel,
            "save_label": layer_ui::DocumentRequest::ConfirmClose { title: String::new() }.accept_label(self.session.localization()),
            "open_label": layer_ui::DocumentRequest::Open.accept_label(self.session.localization()),
            "filter_label": layer_ui::DocumentRequest::Open.filter(self.session.localization()).0,
            "extension": layer_ui::DocumentRequest::Open.filter(self.session.localization()).1,
            "photo_extensions": layer_color::photo::extensions().collect::<Vec<_>>()}))?;
        map.serialize_entry("preferences", &self.session.preferences())?;
        map.serialize_entry("picker", &self.session.tool_picker())?;
        map.serialize_entry("workspace_menu", &self.session.workspace_menu())?;
        map.serialize_entry("toolbar_prompt", &self.session.toolbar_prompt())?;
        map.serialize_entry("toolbar_manager", &self.session.toolbar_manager())?;
        map.serialize_entry("panel_measurements", &state.workspace.layout.measurements)?;
        map.serialize_entry("titlebar_insets", &state.workspace.layout.titlebar_insets)?;
        map.serialize_entry("header_presentation", &state.workspace.layout.header_presentation)?;
        map.serialize_entry(
            "header_presentation_height",
            &state.workspace.layout.header_presentation.height,
        )?;
        map.serialize_entry(
            "workspace_bottom_inset",
            &state.workspace.layout.bottom_inset,
        )?;
        map.serialize_entry("chrome_hidden", &self.chrome_hidden)?;
        let gpu_ready = self.session.engine().backend().0.is_some();
        map.serialize_entry("gpu_ready", &gpu_ready)?;
        map.serialize_entry("keep_zen_button", &self.keep_zen_button)?;
        map.serialize_entry("pan_cursor", &(self.session.navigation_mode() == Some(layer_ui::NavigationMode::Pan)))?;
        map.serialize_entry("navigation_cursor", &self.session.navigation_mode())?;
        let progress = self.shader_progress();
        map.serialize_entry("canvas_ready", &(gpu_ready && progress.canvas_ready))?;
        map.serialize_entry("brush_ready", &(gpu_ready && progress.brush_ready))?;
        map.serialize_entry("shaders_ready", &(gpu_ready && progress.complete))?;
        map.serialize_entry("error", &self.error)?;
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_ui::{CommandId, Platform, UiAction, WorkspaceState};
    use serde_json::Value;
    struct FailedWriter;
    impl std::io::Write for FailedWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic write failure"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn failed_take(host: &mut NativeHost, layout: bool) {
        let mut serializer = serde_json::Serializer::with_formatter(FailedWriter, SnapshotFormatter);
        assert!(host.take_snapshot_with(&mut serializer, layout).is_err());
    }

    fn assert_undo_redo(host: &mut NativeHost, read: fn(&mut NativeHost) -> Value, saved: &Value, committed: &Value) {
        host.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace }).unwrap();
        assert_eq!(&read(host)["state"]["workspace"], saved);
        host.dispatch(UiAction::Invoke { command: CommandId::RedoWorkspace }).unwrap();
        assert_eq!(read(host)["state"]["workspace"], committed["state"]["workspace"]);
    }

    #[test]
    fn command_search_publications_retain_workspace_and_include_close() {
        use layer_ui::CommandSearchAction;
        let mut host = host(Platform::Android);
        host.dispatch(UiAction::Invoke { command: CommandId::Hand }).unwrap();
        let initial = decoded(host.take_model_update_bytes().unwrap()).unwrap();
        assert!(initial.get("state").is_some());
        let model_revision = host.session.workspace_model_revision();
        host.dispatch(UiAction::Invoke { command: CommandId::SearchCommands }).unwrap();
        let opened = decoded(host.take_model_update_bytes().unwrap()).unwrap();
        assert!(opened["command_search"]["results"].is_array(), "{opened}");
        assert!(opened.get("state").is_none());
        host.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Query { text: "pencil".into() } }).unwrap();
        let queried = decoded(host.take_model_update_bytes().unwrap()).unwrap();
        assert_eq!(queried["command_search"]["query"], "pencil");
        assert!(queried.get("workspace_update").is_none());
        host.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Close }).unwrap();
        let closed = decoded(host.take_model_update_bytes().unwrap()).unwrap();
        assert!(closed.get("command_search").unwrap().is_null());
        assert_eq!(host.session.workspace_model_revision(), model_revision);
        assert!(host.take_model_update_bytes().unwrap().is_none());
        host.dispatch(UiAction::Invoke { command: CommandId::Eraser }).unwrap();
        let changed = decoded(host.take_model_update_bytes().unwrap()).unwrap();
        assert!(changed.get("model_update").is_some(), "Search packets keep the model baseline: {changed}");
        assert!(!changed.to_string().contains("\"command_search\""), "The closed search matches the baseline: {changed}");
    }

    fn host(platform: Platform) -> NativeHost {
        let mut host = NativeHost::new(platform).unwrap();
        host.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState::for_platform(platform)),
        })
        .unwrap();
        host.resize(2410, 1810, 2.).unwrap();
        host
    }
    #[test]
    fn workspace_replacement_serializes_metadata_for_every_presented_panel() {
        use layer_ui::{HostRequestKind, ManagedWorkspace, PreparedWorkspace, WorkspaceCapture, WorkspaceCommand, WorkspacePreset};
        fn complete(host: &NativeHost, stage: &str) {
            let snapshot = host.snapshot();
            let available = snapshot["panels"].as_array().unwrap();
            let layout = host.session.layout(host.logical);
            let mut referenced: Vec<_> = layout.groups.iter().flat_map(|group| &group.panels).copied().collect();
            referenced.extend(layout.collapsed.iter().flat_map(|column| &column.groups)
                .flat_map(|group| &group.icons).map(|icon| icon.panel));
            referenced.extend(host.session.state().customization.drawer.iter()
                .chain(&host.session.state().customization.column_drawers).flat_map(|drawer| drawer.panels()));
            assert!(!referenced.is_empty(), "{stage}");
            for panel in referenced {
                let id = serde_json::to_value(panel).unwrap();
                let metadata = available.iter().find(|value| value["id"] == id)
                    .unwrap_or_else(|| panic!("{stage}: missing presented panel {id}; available: {}", snapshot["panels"]));
                assert!(!metadata["icon"].as_str().unwrap().is_empty(), "{stage}: {id}");
                assert!(!metadata["title"].as_str().unwrap().is_empty(), "{stage}: {id}");
            }
        }
        for platform in [Platform::Mac, Platform::Ios] {
            for theme in ["light", "dark"] {
                let mut host = host(platform);
                host.resize(2400, 1740, 2.).unwrap();
                let previous = host.session.state().revision;
                let change = host.session.configure_workspace_manager(ManagedWorkspace {
                    id: "initial".into(), name: "Initial".into(),
                    baseline: host.session.state().workspace.layout.clone(), choices: Vec::new(),
                }).unwrap();
                host.apply_change(previous, change);
                complete(&host, "initial");
                host.dispatch(serde_json::from_value(json!({"type":"set_theme", "theme":theme})).unwrap()).unwrap();
                complete(&host, "theme");
                host.dispatch(UiAction::WorkspaceManager {command:WorkspaceCommand::Switch {
                    id:"builtin:workspace:painter".into(),
                }}).unwrap();
                assert!(host.session.state().requests.iter().any(|request| matches!(&request.kind,
                    HostRequestKind::Workspace {command:WorkspaceCommand::Switch {id}} if id == "builtin:workspace:painter")));
                complete(&host, "switch request");
                let prepared = PreparedWorkspace::new(WorkspaceCapture::from_template(&WorkspacePreset::Painter.layout(platform)).unwrap()).unwrap();
                host.session.begin_workspace_transition().unwrap();
                let previous = host.session.state().revision;
                let change = host.session.adopt_workspace(prepared).unwrap();
                host.session.end_workspace_transition();
                host.apply_change(previous, change);
                complete(&host, "adopted Painter");
                assert!(host.session.layout(host.logical).collapsed.is_empty());
            }
        }
    }
    #[test]
    fn layers_bubble_publishes_both_retained_panels_until_its_card_folds() {
        use layer_ui::{BubbleAction, BubbleGrab, ContactPhase, CustomizationAction, HeaderItem, Panel, ToolbarControl, WorkspacePreset};
        for platform in [Platform::Gtk,Platform::Android,Platform::Mac,Platform::Ios,Platform::Windows] {
            let mut host=host(platform);
            host.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {
                layout:WorkspacePreset::Painter.layout(platform), ..Default::default()
            })}).unwrap();
            let count=|snapshot:&Value,id:&str| snapshot["panels"].as_array().unwrap().iter().filter(|panel|panel["id"]==id).count();
            let initial=host.take_value().unwrap();
            assert_eq!(count(&initial,"properties"),0,"{platform:?}: Painter has no docked Properties");
            let header=host.session.state().workspace.layout.header.entries().find(|entry|entry.item==HeaderItem::Tool {control:ToolbarControl::Panel {panel:Panel::Layers}}).unwrap().id;
            for phase in [ContactPhase::Down,ContactPhase::Move,ContactPhase::Up] {
                host.dispatch(UiAction::Bubble {action:BubbleAction::Drag {
                    grab:BubbleGrab::Header {id:header},phase,position:[640.,520.],viewport:[1200.,900.]
                }}).unwrap();
            }
            let id=host.session.state().workspace.layout.bubbles.bubbles.last().unwrap().id;
            for properties in [false,true] {
                if properties {host.dispatch(UiAction::Bubble {action:BubbleAction::Properties {id}}).unwrap();}
                let snapshot=host.take_value().unwrap();
                assert!(snapshot["bubbles"][0]["view"]["card"].is_object(),"{platform:?}");
                for panel in ["layers","properties"] {
                    assert_eq!(count(&snapshot,panel),1,"{platform:?}: properties={properties} retains {panel} exactly once");
                }
            }
            host.dispatch(UiAction::Bubble {action:BubbleAction::Close {id}}).unwrap();
            let folded=host.take_value().unwrap();
            assert!(folded["bubbles"][0]["view"]["card"].is_null());
            for panel in ["layers","properties"] {assert_eq!(count(&folded,panel),count(&initial,panel),"{platform:?}: folded card retires {panel}");}
            host.dispatch(UiAction::Customize {action:CustomizationAction::SetPanelVisible {panel:Panel::Properties,visible:true}}).unwrap();
            assert_eq!(count(&host.take_value().unwrap(),"properties"),1,"{platform:?}: independently configured Properties remains available");
        }
    }

    #[test]
    fn combined_layer_drawer_publishes_both_retained_panels() {
        use layer_ui::{Bounds, CustomizationAction, HeaderItem, HeaderItemBounds, Panel, ToolbarControl, WorkspacePreset};
        let mut host = host(Platform::Windows);
        host.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {
            layout:WorkspacePreset::Painter.layout(Platform::Windows), ..Default::default()
        })}).unwrap();
        let has_properties = |snapshot:&Value| snapshot["panels"].as_array().unwrap().iter().any(|panel| panel["id"] == "properties");
        assert!(!has_properties(&host.take_value().unwrap()));
        let id = host.session.state().workspace.layout.header.entries().find(|entry| entry.item == HeaderItem::Tool {control:ToolbarControl::Panel {panel:Panel::Layers}}).unwrap().id;
        host.dispatch(UiAction::MeasureHeader {height:60., items:vec![HeaderItemBounds {id,bounds:Bounds {x:800.,y:0.,width:40.,height:60.}}]}).unwrap();
        host.dispatch(UiAction::Customize {action:CustomizationAction::ToggleHeaderDrawer {id}}).unwrap();
        assert!(has_properties(&host.take_value().unwrap()), "Layers retains the hidden Properties body");
        host.dispatch(UiAction::Customize {action:CustomizationAction::ToggleLayerDrawerProperties}).unwrap();
        assert!(has_properties(&host.take_value().unwrap()), "Properties keeps the same panel projection");
        host.dispatch(UiAction::Customize {action:CustomizationAction::CloseExpanded}).unwrap();
        assert!(!has_properties(&host.take_value().unwrap()), "Closing retires the undocked panel projection");
    }
    #[test]
    fn picker_previews_use_the_color_panel_projection() {
        for platform in [Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
            let mut host = host(platform);
            if matches!(platform, Platform::Mac | Platform::Ios) {
                host.ui_color = crate::UiColor::Tagged(layer_core::color::RgbSpace::DisplayP3);
            }
            let snapshot = host.take_value().unwrap();
            assert_eq!(snapshot["color_preview"]["view"], snapshot["color_panel"], "{platform:?}");
        }
    }
    #[test]
    fn paint_pair_publishes_selection_only_and_mask_changes() {
        use layer_ui::{ColorAction, ColorSlot};
        for platform in [Platform::Android, Platform::Windows] {
            let mut host = host(platform);
            let initial = host.take_value().unwrap()["paint_pair"].clone();
            host.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Background } }).unwrap();
            let selected = host.take_value().unwrap()["paint_pair"].clone();
            assert_eq!(selected["front_swatch"], "background");
            assert_eq!(selected["swatches"], initial["swatches"]);
            host.dispatch(UiAction::Color { action: ColorAction::ToggleTransparent }).unwrap();
            assert_eq!(host.take_value().unwrap()["paint_pair"], selected);
            host.dispatch(UiAction::Color { action: ColorAction::QuickColor { white: false } }).unwrap();
            let temporary = host.take_value().unwrap()["paint_pair"].clone();
            assert_eq!(temporary["front_swatch"], "foreground");
            assert_eq!(temporary["rgba"], serde_json::json!([0., 0., 0., 1.]));
            assert_eq!(temporary["swatches"], initial["swatches"]);
            host.dispatch(UiAction::Invoke { command: CommandId::QuickMask }).unwrap();
            let mask = host.take_value().unwrap();
            assert_eq!(mask["paint_pair"]["definition"], mask["color_panel"]["definition"]);
            host.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Background } }).unwrap();
            let mask = host.take_value().unwrap();
            assert_eq!(mask["paint_pair"]["front_swatch"], "background");
            assert_eq!(mask["paint_pair"]["definition"], mask["color_panel"]["definition"]);
            host.dispatch(UiAction::Invoke { command: CommandId::QuickMask }).unwrap();
            assert_eq!(host.take_value().unwrap()["paint_pair"], temporary);
        }
    }
    fn decoded(bytes: Option<Vec<u8>>) -> Option<Value> {
        bytes.map(|bytes| serde_json::from_slice(&bytes).unwrap())
    }
    fn update(host: &mut NativeHost) -> Value {
        decoded(host.take_update_bytes().unwrap()).unwrap()
    }
    fn workspace_fixture(host: &mut NativeHost) {
        let mut workspace = serde_json::to_value(host.session.state().workspace.clone()).unwrap();
        let layout = &mut workspace["layout"];
        layout["bands"] = json!([
            {"id":40,"edge":"left","extent":252,"root":{"kind":"tabs","id":41,"panels":["brushes","sizes","tool_settings"],"active":"brushes","tab_style":"icon"}},
            {"id":42,"edge":"right","extent":252,"root":{"kind":"tabs","id":43,"panels":["layers","properties"],"active":"layers","tab_style":"icon"}}
        ]);
        for field in ["floating", "collapsed", "column_scroll", "fit_tab_groups"] {
            layout[field] = json!([]);
        }
        layout["next_id"] = json!(100);
        workspace["zen_mode"] = json!(false);
        host.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(serde_json::from_value(workspace).unwrap()),
        })
        .unwrap();
    }
    fn drag(host: &mut NativeHost, phase: layer_ui::ContactPhase, position: [f32; 2]) {
        host.dispatch(UiAction::DragWorkspace {
            item: layer_ui::DockItem::Panel {
                panel: layer_ui::Panel::Brushes,
            },
            phase,
            position,
            viewport: host.logical,
            tabs: vec![],
        })
        .unwrap();
    }

    fn resize_divider(host: &mut NativeHost, phase: layer_ui::ContactPhase, x: f32) {
        host.dispatch(UiAction::DragDivider {
            id: 40,
            phase,
            position: [x, 300.],
            viewport: host.logical,
        })
        .unwrap();
    }
    fn layout_update(host: &mut NativeHost) -> Value {
        decoded(host.take_layout_update_bytes().unwrap()).unwrap()
    }

    #[test]
    fn resize_layout_updates_retain_content_and_match_full_models_and_history() {
        use layer_ui::ContactPhase::*;
        for platform in [
            Platform::Android,
            Platform::Ios,
            Platform::Mac,
            Platform::Windows,
        ] {
            let mut host = host(platform);
            workspace_fixture(&mut host);
            let initial = layout_update(&mut host);
            let saved = initial["state"]["workspace"].clone();
            let retained = host.snapshot();
            let content_revision = initial["workspace_update"]["content_revision"].clone();
            resize_divider(&mut host, Down, 255.);
            for x in [320., 420., 350.] {
                resize_divider(&mut host, Move, x);
                let packet = layout_update(&mut host);
                let full = host.snapshot();
                assert!(packet.get("state").is_none(), "{platform:?}: {packet}");
                assert_eq!(
                    packet["workspace_update"]["content_revision"],
                    content_revision
                );
                assert_ne!(
                    packet["workspace_update"]["model_revision"],
                    initial["workspace_update"]["model_revision"]
                );
                assert_eq!(packet["layout"], full["layout"]);
                assert_eq!(packet["camera"], full["state"]["camera"]);
                assert_eq!(packet["panel_measurements"], full["panel_measurements"]);
                assert!(packet.to_string().len() < full.to_string().len() / 4);
                for field in ["bands", "floating", "collapsed", "fit_tab_groups", "fit_height_groups"] {
                    assert_eq!(
                        packet["workspace_layout"][field],
                        full["state"]["workspace"]["layout"][field]
                    );
                }
                for field in ["panels", "application_menus", "color_panel", "preferences"] {
                    assert_eq!(retained[field], full[field], "Retained {field}");
                }
                assert!(host.take_layout_update_bytes().unwrap().is_none());
            }
            resize_divider(&mut host, Cancel, 350.);
            assert_eq!(layout_update(&mut host)["state"]["workspace"], saved);
            resize_divider(&mut host, Down, 255.);
            resize_divider(&mut host, Move, 370.);
            layout_update(&mut host);
            resize_divider(&mut host, Up, 370.);
            let committed = layout_update(&mut host);
            assert_ne!(committed["state"]["workspace"], saved);
            assert_undo_redo(&mut host, layout_update, &saved, &committed);
        }
    }

    #[test]
    fn open_column_resize_publishes_layout_without_rebuilding_content() {
        use layer_ui::{ContactPhase::*, CustomizationAction, Panel};
        let mut host = host(Platform::Gtk);
        workspace_fixture(&mut host);
        for action in [
            CustomizationAction::SetColumnCollapsed { group: 41, collapsed: true },
            CustomizationAction::SetColumnDrawers { column: 41, drawers: false },
            CustomizationAction::ToggleColumnDrawer { group: 41, panel: Panel::Brushes },
        ] {
            host.dispatch(UiAction::Customize { action }).unwrap();
        }
        let initial = layout_update(&mut host);
        let resolved = host.session.layout(host.logical);
        let divider = resolved.dividers.iter().find(|d| resolved.open_column_at_divider(d.id) == Some(41)).unwrap();
        let id = divider.id;
        let start = [divider.bounds.x + divider.bounds.width * 0.5, divider.bounds.y + 100.];
        let resize = |host: &mut NativeHost, phase, position| {
            host.dispatch(UiAction::DragDivider { id, phase, position, viewport: host.logical }).unwrap();
        };
        resize(&mut host, Down, start);
        let begun = layout_update(&mut host);
        for delta in [35., 60., 90.] {
            resize(&mut host, Move, [start[0] + delta, start[1]]);
            let packet = layout_update(&mut host);
            let full = host.snapshot();
            assert!(packet.get("state").is_none());
            assert_eq!(
                packet["workspace_update"]["content_revision"],
                begun["workspace_update"]["content_revision"]
            );
            assert_eq!(packet["layout"], full["layout"]);
            assert_eq!(packet["workspace_layout"]["column_stacks"], full["state"]["workspace"]["layout"]["column_stacks"]);
        }
        resize(&mut host, Cancel, start);
        let cancelled = layout_update(&mut host);
        assert_eq!(
            cancelled["state"]["workspace"],
            initial["state"]["workspace"]
        );
        assert_eq!(cancelled["layout"], initial["layout"]);
    }

    #[test]
    fn resize_content_changes_collapse_and_legacy_consumers_require_full_models() {
        use layer_ui::ContactPhase::*;
        let mut host = host(Platform::Android);
        workspace_fixture(&mut host);
        layout_update(&mut host);
        resize_divider(&mut host, Down, 255.);
        resize_divider(&mut host, Move, 320.);
        // Existing incremental consumers still get their original full schema.
        assert!(update(&mut host).get("state").is_some());
        host.dispatch(UiAction::SetBrushSize { value: 37. })
            .unwrap();
        resize_divider(&mut host, Move, 340.);
        assert_eq!(layout_update(&mut host)["state"]["brush"]["diameter"], 37.);
        host.chrome_hidden = true;
        resize_divider(&mut host, Move, 350.);
        assert_eq!(layout_update(&mut host)["chrome_hidden"], true);
        host.chrome_hidden = false;
        layout_update(&mut host);
        resize_divider(&mut host, Move, 20.);
        let collapsed = layout_update(&mut host);
        assert!(collapsed.get("state").is_some());
        assert!(
            !collapsed["layout"]["collapsed"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        resize_divider(&mut host, Move, 400.);
        let expanded = layout_update(&mut host);
        assert!(expanded.get("state").is_some());
        assert!(
            expanded["layout"]["collapsed"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        resize_divider(&mut host, Cancel, 400.);
        assert!(layout_update(&mut host).get("state").is_some());
    }

    #[test]
    fn viewport_reflow_retains_content_and_matches_full_models() {
        for platform in [Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
            let mut host = host(platform);
            workspace_fixture(&mut host);
            let initial = layout_update(&mut host);
            for (width, height, density) in [(900, 700, 1.), (1800, 1400, 1.5), (720, 1080, 2.)] {
                host.resize(width, height, density).unwrap();
                let packet = layout_update(&mut host);
                let full = host.snapshot();
                assert!(packet.get("state").is_none());
                assert_eq!(packet["workspace_update"]["content_revision"], initial["workspace_update"]["content_revision"]);
                assert_eq!(packet["layout"], full["layout"]);
                assert_eq!(packet["camera"], full["state"]["camera"]);
                assert_eq!(packet["panel_measurements"], full["panel_measurements"]);
                for field in ["panels", "application_menus", "preferences"] {
                    assert_eq!(initial[field], full[field], "Retained {field}");
                }
                assert!(host.take_layout_update_bytes().unwrap().is_none());
            }
            host.resize(1000, 800, 1.).unwrap();
            assert!(update(&mut host).get("state").is_some());
            host.resize(1100, 850, 1.).unwrap();
            host.dispatch(UiAction::SetBrushSize { value: 37. }).unwrap();
            assert_eq!(layout_update(&mut host)["state"]["brush"]["diameter"], 37.);
        }
    }

    #[test]
    fn viewport_header_bounds_reflow_without_republishing_content() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
            let mut host = host(platform);
            workspace_fixture(&mut host);
            let id = host.session.state().workspace.layout.header.entries().next().unwrap().id;
            let measurement = |x, height| UiAction::MeasureHeader { height, items: vec![layer_ui::HeaderItemBounds {
                id, bounds: layer_ui::Bounds { x, y: 0., width: 80., height },
            }] };
            host.dispatch(measurement(100., 48.)).unwrap();
            let initial = layout_update(&mut host);
            for x in [120., 140., 160.] {
                host.resize(1000 + x as u32, 800, 1.).unwrap();
                host.dispatch(measurement(x, 48.)).unwrap();
                let packet = layout_update(&mut host);
                let full = host.snapshot();
                assert!(packet.get("state").is_none());
                assert_eq!(packet["workspace_update"]["content_revision"], initial["workspace_update"]["content_revision"]);
                assert_eq!(packet["layout"], full["layout"]);
                assert_eq!(packet["camera"], full["state"]["camera"]);
                assert_eq!(initial["header"], full["header"]);
                let mut retained_layout = initial["state"]["workspace"]["layout"].clone();
                for (field, value) in packet["workspace_layout"].as_object().unwrap() {
                    retained_layout.as_object_mut().unwrap().insert(field.clone(), value.clone());
                }
                assert_eq!(retained_layout, full["state"]["workspace"]["layout"]);
                assert!(retained_layout.get("header_presentation").is_none());
                assert_eq!(packet["header_presentation"], full["header_presentation"]);
                assert_eq!(packet["header_presentation"]["items"][0]["bounds"]["x"], x);
                assert_eq!(packet["header_presentation"], serde_json::to_value(&host.session.state().workspace.layout.header_presentation).unwrap());
            }
            host.dispatch(measurement(160., 60.)).unwrap();
            let taller = layout_update(&mut host);
            assert!(taller.get("state").is_some());
            assert_eq!(taller["header_presentation_height"], 60.);
        }
    }

    #[test]
    fn reflow_measurements_and_failed_layout_serialization_preserve_revisions() {
        use layer_ui::ContactPhase::*;
        let mut host = host(Platform::Android);
        workspace_fixture(&mut host);
        let initial = layout_update(&mut host);
        host.dispatch(UiAction::MeasurePanels {
            measurements: vec![layer_ui::PanelMeasurement {
                panel: layer_ui::Panel::Brushes,
                tab_width: 80.,
                content_height: 500.,
                scroll: None,
            }],
        })
        .unwrap();
        let measured = layout_update(&mut host);
        assert_eq!(
            measured["workspace_update"]["content_revision"],
            initial["workspace_update"]["content_revision"]
        );
        assert_eq!(
            measured["panel_measurements"],
            host.snapshot()["panel_measurements"]
        );
        resize_divider(&mut host, Down, 255.);
        resize_divider(&mut host, Move, 360.);
        failed_take(&mut host, true);
        let packet = layout_update(&mut host);
        assert!(packet.get("state").is_none());
        assert_eq!(packet["layout"], host.snapshot()["layout"]);
        assert_eq!(packet["camera"], host.snapshot()["state"]["camera"]);
        host.resize(2200, 1800, 2.).unwrap();
        let resized = layout_update(&mut host);
        assert!(resized.get("state").is_none());
        assert_eq!(resized["layout"], host.snapshot()["layout"]);
        assert_eq!(resized["camera"], host.snapshot()["state"]["camera"]);
    }

    #[test]
    fn workspace_updates_retain_models_and_match_authoritative_layout_and_history() {
        use layer_ui::ContactPhase::*;
        for platform in [
            Platform::Android,
            Platform::Ios,
            Platform::Mac,
            Platform::Windows,
        ] {
            let mut host = host(platform);
            workspace_fixture(&mut host);
            let initial = update(&mut host);
            let saved = initial["state"]["workspace"].clone();
            let bounds = host
                .session
                .layout(host.logical)
                .groups
                .into_iter()
                .find(|g| g.id == 41)
                .unwrap()
                .bounds;
            let start = [bounds.x + 10., bounds.y + 10.];
            drag(&mut host, Down, start);
            let down = update(&mut host);
            assert!(down.get("state").is_some());
            let model_revision = down["workspace_update"]["model_revision"].clone();
            host.dispatch(UiAction::BeginTabDrag {
                tabs: (0..3)
                    .map(|index| layer_ui::TabHit {
                        group: 41,
                        index,
                        bounds: layer_ui::Bounds {
                            x: bounds.x + index as f32 * 36.,
                            y: bounds.y,
                            width: 36.,
                            height: 36.,
                        },
                    })
                    .collect(),
                clip: layer_ui::Bounds {
                    width: bounds.width - 20.,
                    height: 36.,
                    ..bounds
                },
            })
            .unwrap();
            drag(&mut host, Move, [start[0] + 3., start[1]]);
            let motion = update(&mut host);
            assert!(motion.get("state").is_none());
            assert_eq!(motion["workspace_update"]["model_revision"], model_revision);
            assert_eq!(motion["workspace_update"]["drag"]["tab"]["group"], 41);
            assert_eq!(
                motion["workspace_update"]["drag"]["tab"]["preview"]["bounds"]["x"],
                json!(bounds.x + 3.)
            );
            // Tear-off requires new models even if several moves arrive before publication.
            drag(&mut host, Move, [500., 400.]);
            drag(&mut host, Move, [510., 410.]);
            let detached = update(&mut host);
            assert!(detached.get("state").is_some());
            for x in [520., 535., 510.] {
                drag(&mut host, Move, [x, 410.]);
                let next = update(&mut host);
                assert!(next.get("state").is_none());
                let group = &next["workspace_update"]["drag"]["group"];
                let expected = layer_ui::Bounds { x: x - 10., y: 400., ..bounds };
                assert_eq!(group["bounds"], serde_json::to_value(expected).unwrap());
            }
            drag(&mut host, Cancel, [510., 410.]);
            let canceled = update(&mut host);
            assert_eq!(canceled["state"]["workspace"], saved);
            assert!(canceled["workspace_update"]["drag"].is_null());
            drag(&mut host, Down, start);
            drag(&mut host, Move, [500., 400.]);
            update(&mut host);
            drag(&mut host, Up, [510., 410.]);
            let committed = update(&mut host);
            assert_undo_redo(&mut host, update, &saved, &committed);
        }
    }

    #[test]
    fn workspace_motion_cannot_acknowledge_unpublished_content_or_host_changes() {
        use layer_ui::ContactPhase::*;
        let mut host = host(Platform::Android);
        workspace_fixture(&mut host);
        update(&mut host);
        let b = host
            .session
            .layout(host.logical)
            .groups
            .into_iter()
            .find(|g| g.id == 41)
            .unwrap()
            .bounds;
        drag(&mut host, Down, [b.x + 10., b.y + 10.]);
        drag(&mut host, Move, [500., 400.]);
        update(&mut host);
        host.dispatch(UiAction::SetBrushSize { value: 37. })
            .unwrap();
        drag(&mut host, Move, [510., 400.]);
        assert_eq!(update(&mut host)["state"]["brush"]["diameter"], 37.);
        host.chrome_hidden = true;
        drag(&mut host, Move, [520., 400.]);
        assert_eq!(update(&mut host)["chrome_hidden"], true);
        // A camera change may share the same packet as unpresented drag motion.
        host.dispatch(UiAction::Invoke {
            command: CommandId::ZoomIn,
        })
        .unwrap();
        update(&mut host);
        drag(&mut host, Move, [550., 400.]);
        host.dispatch(UiAction::Invoke {
            command: CommandId::ZoomIn,
        })
        .unwrap();
        let packet = update(&mut host);
        assert!(packet.get("state").is_none());
        assert!(packet.get("camera").is_some());
        assert!(packet["workspace_update"]["drag"]["group"].is_object());
    }

    #[test]
    fn raw_snapshot_fields_preserve_presence_and_utf8_ranges() {
        let bytes = br#"{"state":null,"workspace_update":null,"command_search":null,"brush_ready":null,"camera":{"text":"\u03bb \\\" } {"},"localization":{"text":"\u65e5\n"}}"#;
        let fields = SnapshotFields::parse(bytes).unwrap();
        for key in ["state", "workspace_update", "command_search", "brush_ready"] {
            assert!(fields.contains(key));
            assert_eq!(&bytes[fields.range(key).unwrap()], b"null");
        }
        assert!(!fields.contains("missing"));
        assert!(fields.range("missing").is_none());
        let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        for key in ["camera", "localization"] {
            let range = fields.range(key).unwrap();
            assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes[range]).unwrap(), value[key]);
        }
        for empty in [b"{}".as_slice(), b"null", b"[]", b"false", b"\"text\""] {
            let bytes = [b"{\"localization\":".as_slice(), empty, b"}"].concat();
            assert!(SnapshotFields::parse(&bytes).unwrap().nonempty_object_range("localization").unwrap().is_none());
        }
        let bytes = serde_json::to_vec(&json!({"localization":{"text":"Japanese 日本語 λ\\\"\n"},"camera":{"zoom":0.123456789}})).unwrap();
        let fields = SnapshotFields::parse(&bytes).unwrap();
        assert_eq!(fields.nonempty_object_range("localization").unwrap(), fields.range("localization"));
        let camera = bytes[fields.range("camera").unwrap()].to_vec();
        let owned = std::ffi::CString::new(bytes).unwrap();
        assert_eq!(&owned.as_bytes()[SnapshotFields::parse(owned.as_bytes()).unwrap().range("camera").unwrap()], camera);
        assert!(SnapshotFields::parse(b"{").is_err());
    }

    #[test]
    fn raw_snapshot_fields_follow_camera_and_structural_publications() {
        let mut host = host(Platform::Windows);
        let full = host.take_update_bytes().unwrap().unwrap();
        assert!(SnapshotFields::parse(&full).unwrap().contains("state"));
        assert!(host.take_update_bytes().unwrap().is_none());
        host.scroll([1200., 900.], [30., -20.], 2., false, false).unwrap();
        let camera = host.take_update_bytes().unwrap().unwrap();
        let fields = SnapshotFields::parse(&camera).unwrap();
        assert!(!fields.contains("state"));
        let range = fields.range("camera").unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&camera[range]).unwrap(), json!(host.session.state().camera));
        host.dispatch(UiAction::SetBrushSize { value: 37. }).unwrap();
        let full = host.take_update_bytes().unwrap().unwrap();
        assert!(SnapshotFields::parse(&full).unwrap().contains("state"));
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&full).unwrap()["state"]["brush"]["diameter"], 37.);
    }

    #[test]
    fn extend_update_appends_fields_without_rewriting_shared_bytes() {
        let mut host = host(Platform::Windows);
        let shared = host.take_update_bytes().unwrap().unwrap();
        let text = "Synthetic \"quoted\" λ\n";
        let extension = serde_json::to_vec(&json!({"host_status": text})).unwrap();
        let extended = extend_update(shared.clone(), &extension);
        assert_eq!(extended[..shared.len() - 1], shared[..shared.len() - 1]);
        let value: Value = serde_json::from_slice(&extended).unwrap();
        assert_eq!(value["host_status"], text);
        assert!(value.get("state").is_some());
        assert_eq!(extend_update(shared.clone(), b"{}"), shared);
    }

    #[test]
    fn failed_serialization_keeps_full_camera_and_workspace_updates_pending() {
        let mut host = host(Platform::Mac);
        failed_take(&mut host, false);
        assert!(update(&mut host).get("state").is_some());
        host.dispatch(UiAction::Invoke {
            command: CommandId::ZoomIn,
        })
        .unwrap();
        // The first edit can also synchronize initial document controls. Consume
        // that state before testing a subsequent camera-only publication.
        update(&mut host);
        host.dispatch(UiAction::Invoke {
            command: CommandId::ZoomIn,
        })
        .unwrap();
        failed_take(&mut host, false);
        assert!(update(&mut host).get("camera").is_some());
        host.dispatch(UiAction::Invoke {
            command: CommandId::ZenMode,
        })
        .unwrap();
        failed_take(&mut host, false);
        assert!(update(&mut host).get("state").is_some());
        assert!(host.take_update_bytes().unwrap().is_none());
    }
}
