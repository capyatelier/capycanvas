//! Renderer replacement keeps CPU document/session ownership in place.
use super::*;

impl<R: CanvasRenderer> UiSession<R> {
    pub fn rendering_suspended(&self) -> bool {
        self.rendering_suspended
    }

    pub(super) fn command_without_renderer(command: CommandId) -> bool {
        matches!(
            command,
            CommandId::SaveDocument
                | CommandId::SaveDocumentAs
                | CommandId::CloseDocument
                | CommandId::Settings
                | CommandId::KeyboardShortcuts
                | CommandId::ToggleTheme
                | CommandId::Fullscreen
                | CommandId::NewWindow
                | CommandId::About
                | CommandId::Website
                | CommandId::SourceCode
        )
    }

    pub(super) fn action_without_renderer(action: &UiAction) -> bool {
        match action {
            UiAction::Invoke { command } => Self::command_without_renderer(*command),
            UiAction::CompleteRequest { .. }
            | UiAction::CloseSettings
            | UiAction::OpenSettings { .. }
            | UiAction::EditSettings { .. }
            | UiAction::RestoreSettings { .. }
            | UiAction::Preferences { .. }
            | UiAction::SetTheme { .. }
            | UiAction::SystemThemeChanged { .. }
            | UiAction::WindowFullscreen { .. }
            | UiAction::MeasurePanels { .. }
            | UiAction::MeasureTitlebar { .. }
            | UiAction::MeasureWorkspaceBottom { .. }
            | UiAction::MeasureColumnDrawers { .. }
            | UiAction::MeasureDrawerTiles { .. }
            | UiAction::MeasureColumnScroll { .. } => true,
            _ => false,
        }
    }

    /// CPU-only queue pressure relief on the sole input/render owner.
    pub fn flush_input(&mut self) -> Result<UiChange, String> {
        self.engine.flush_input().map_err(error)?;
        self.input_pending = false;
        self.refresh_document();
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS, true))
    }

    /// Preserve saveable source state after rendering cannot resume. The host
    /// has stopped new canvas input and delivered every admitted sample first.
    pub fn suspend_renderer(&mut self) -> Result<UiChange, String> {
        let retired_regions = self.input(UiInput::Blur)?.change.regions;
        self.engine.finish_input().map_err(error)?;
        self.input_pending = false;
        self.eyedropper.renderer_replaced();
        self.region_tools.renderer_replaced();
        self.navigator_preview = Default::default();
        if self.pending_filters.take().is_some() {
            self.state.filter_load.pending = false;
            self.state.filter_load.error =
                Some("Filter validation stopped because painting is unavailable".into());
        }
        self.rendering_suspended = true;
        self.refresh_document();
        self.poll_document_close();
        self.refresh_commands();
        Ok(self.changed(
            retired_regions
                | regions::DOCUMENT
                | regions::BRUSH
                | regions::COMMANDS
                | regions::HOST,
            false,
        ))
    }

    /// Prepare immutable source assets before publishing a replacement renderer.
    /// Retain document history, tool settings, camera and workspace. GPU-only
    /// readbacks are cancelled; pending filter validation resumes from retained
    /// source bytes before it can publish a catalog or edit.
    pub fn replace_renderer(&mut self, mut renderer: R) -> Result<(R, UiChange), String> {
        for (id, asset) in &self.files.assets {
            renderer.prepare_owned_asset(id, asset).map_err(error)?;
        }
        if let Some(pending) = &self.pending_filters
            && !renderer
                .request_effect_validation(pending.validation.clone())
                .map_err(error)?
        {
            return Err("The replacement GPU could not resume filter validation".into());
        }
        let previous = self.engine.replace_backend(renderer).map_err(error)?;
        self.rendering_suspended = false;
        self.eyedropper.renderer_replaced();
        self.region_tools.renderer_replaced();
        self.navigator_preview = Default::default();
        if let Some(pending) = &mut self.pending_filters {
            pending.validated = false;
        }
        self.refresh_commands();
        let change = self.changed(regions::DOCUMENT | regions::BRUSH | regions::COMMANDS, true);
        Ok((previous, change))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{AssetId, ProjectAsset};
    use layer_render::{BackendError, FramePacket, HostImage, ReadbackImage};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Backend {
        assets: BTreeMap<AssetId, ProjectAsset>,
        reject_assets: bool,
        previews: usize,
        samples: usize,
        validation: Option<layer_render::EffectValidationRequest>,
        validation_result: Option<layer_render::EffectValidationResult>,
    }
    impl CanvasRenderer for Backend {
        type Error = BackendError;
        fn resize_surface(&mut self, _: u32, _: u32) -> Result<(), Self::Error> {
            Ok(())
        }
        fn prepare_asset(&mut self, _: &AssetId, _: HostImage<'_>) -> Result<(), Self::Error> {
            unreachable!("the session retains owned asset bytes")
        }
        fn prepare_owned_asset(
            &mut self,
            id: &AssetId,
            asset: &ProjectAsset,
        ) -> Result<(), Self::Error> {
            if self.reject_assets {
                return Err(BackendError("asset preparation failed"));
            }
            self.assets.insert(id.clone(), asset.clone());
            Ok(())
        }
        fn release_asset(&mut self, _: &AssetId) {}
        fn submit(&mut self, _: FramePacket<'_>) -> Result<(), Self::Error> {
            Ok(())
        }
        fn request_readback(&mut self, _: u64) -> Result<(), Self::Error> {
            Ok(())
        }
        fn take_readback(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
            None
        }
        fn request_canvas_preview(&mut self, _: Option<u64>) -> Result<bool, Self::Error> {
            self.previews += 1;
            Ok(true)
        }
        fn request_color_sample(
            &mut self,
            _: layer_render::ColorSampleRequest,
        ) -> Result<bool, Self::Error> {
            self.samples += 1;
            Ok(true)
        }
        fn request_effect_validation(
            &mut self,
            request: layer_render::EffectValidationRequest,
        ) -> Result<bool, Self::Error> {
            self.validation = Some(request);
            Ok(true)
        }
        fn take_effect_validation(&mut self) -> Option<layer_render::EffectValidationResult> {
            self.validation_result.take()
        }
    }
    fn invoke(s: &mut UiSession<Backend>, command: CommandId) {
        s.dispatch(UiAction::Invoke { command }).unwrap();
    }

    #[test]
    fn replacement_retains_source_assets_undo_redo_workspace_and_pending_save() {
        let mut s = UiSession::new(
            Backend::default(),
            Document::new("recovery", 128, 128),
            [256, 256],
        )
        .unwrap();
        s.set_platform(Platform::Windows);
        let image = ProjectAsset {
            extent: [1, 1],
            format: layer_core::ProjectAssetFormat::Rgba8Srgb,
            bytes: std::sync::Arc::from([180, 20, 75, 128]),
        };
        s.import_layer_asset("Imported source", image).unwrap();
        s.frame(0, 0).unwrap();
        let imported = s.engine.document().clone();
        invoke(&mut s, CommandId::Undo);
        s.frame(1, 1).unwrap();
        let document = s.engine.document().clone();
        let workspace = s.capture_workspace().unwrap();
        let camera = s.state.camera.clone();
        let checkpoint = s.engine.checkpoint();
        s.suspend_renderer().unwrap();
        assert!(s.rendering_suspended());
        assert!(s.capture_project_recovery().is_ok());
        assert!(!s.command(CommandId::Redo).enabled);
        invoke(&mut s, CommandId::SaveDocument);
        let request = s.files.pending.as_ref().unwrap().0;
        let (previous, change) = s.replace_renderer(Backend::default()).unwrap();
        assert!(change.canvas_wake);
        assert!(!s.rendering_suspended());
        assert_eq!(s.engine.document(), &document);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.capture_workspace().unwrap(), workspace);
        assert_eq!(s.state.camera, camera);
        assert_eq!(s.files.pending.as_ref().unwrap().0, request);
        assert_eq!(s.engine.backend().assets, previous.assets);
        assert!(
            !s.engine.backend().assets.is_empty(),
            "undone imports still belong to redo"
        );
        s.complete_document_request(request, Ok(false)).unwrap();
        invoke(&mut s, CommandId::Redo);
        s.frame(2, 2).unwrap();
        assert_eq!(s.engine.document().layers, imported.layers);
    }

    #[test]
    fn replacement_clears_gpu_waits_and_resumes_filter_validation() {
        let mut s = UiSession::new(
            Backend::default(),
            Document::new("requests", 128, 128),
            [128, 128],
        )
        .unwrap();
        s.frame(0, 0).unwrap();
        s.poll_navigator_preview(0, true).unwrap();
        s.eyedropper
            .queue(layer_render::ColorSampleSource::Composite, [10, 10]);
        s.eyedropper.poll(s.engine.backend_mut()).unwrap();
        assert!(s.eyedropper.busy());
        let original = s.effect_catalog.clone();
        let mut package = layer_core::EffectPackage {
            format: 1,
            categories: original.categories().to_vec(),
            filters: original.filters().to_vec(),
        };
        std::sync::Arc::make_mut(&mut package.filters[0].program).label =
            "Recovered candidate".into();
        s.load_effect_library(
            &serde_json::to_string(&package).unwrap(),
            |_| panic!("resolved sources"),
            layer_core::EffectInstallMode::Merge,
        )
        .unwrap();
        assert!(s.state.filter_load.pending);
        let original_validation = s.engine.backend().validation.as_ref().unwrap().clone();
        assert_eq!(original_validation.programs.len(), 1);
        s.replace_renderer(Backend::default()).unwrap();
        assert!(s.state.filter_load.pending);
        let resumed = s.engine.backend().validation.as_ref().unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &resumed.programs[0],
            &original_validation.programs[0]
        ));
        let request = resumed.request_id;
        assert_eq!(request, s.state.filter_load.request_id);
        s.renderer_mut().validation_result = Some(layer_render::EffectValidationResult {
            request_id: request,
            result: Ok(()),
        });
        s.frame(1, 1).unwrap();
        assert!(
            s.state.filter_load.pending,
            "publication waits for reconstruction replay"
        );
        assert_eq!(s.effect_catalog.filters(), original.filters());
        s.frame(2, 2).unwrap();
        assert!(!s.state.filter_load.pending);
        assert!(s.state.filter_load.error.is_none());
        assert_eq!(
            s.effect_catalog
                .get(&original_validation.programs[0].id)
                .unwrap()
                .program
                .label
                .as_ref(),
            "Recovered candidate"
        );
        assert!(!s.eyedropper.busy());
        s.poll_navigator_preview(1, true).unwrap();
        assert_eq!(s.engine.backend().previews, 1);
        s.eyedropper
            .queue(layer_render::ColorSampleSource::Composite, [10, 10]);
        s.eyedropper.poll(s.engine.backend_mut()).unwrap();
        assert_eq!(s.engine.backend().samples, 1);
    }

    #[test]
    fn suspension_cancels_transform_and_filter_candidate_without_changing_sources() {
        let mut s = UiSession::new(
            Backend::default(),
            Document::new("retire", 128, 128),
            [128, 128],
        )
        .unwrap();
        s.set_platform(Platform::Windows);
        s.import_layer_asset(
            "Source",
            ProjectAsset {
                extent: [1, 1],
                format: layer_core::ProjectAssetFormat::Rgba8Srgb,
                bytes: std::sync::Arc::from([20, 30, 40, 255]),
            },
        )
        .unwrap();
        s.frame(0, 0).unwrap();
        let document = s.engine.document().clone();
        let assets = s.files.assets.clone();
        let catalog = s.effect_catalog.clone();
        let mut package = layer_core::EffectPackage {
            format: 1,
            categories: catalog.categories().to_vec(),
            filters: catalog.filters().to_vec(),
        };
        std::sync::Arc::make_mut(&mut package.filters[0].program).label =
            "Unpublished candidate".into();
        s.load_effect_library(
            &serde_json::to_string(&package).unwrap(),
            |_| panic!("owned sources"),
            layer_core::EffectInstallMode::Merge,
        )
        .unwrap();
        invoke(&mut s, CommandId::ScaleRotate);
        assert!(s.operation.active());
        assert!(s.state.filter_load.pending);
        s.suspend_renderer().unwrap();
        assert!(!s.operation.active());
        assert!(!s.state.filter_load.pending);
        assert!(s.state.filter_load.error.is_some());
        assert_eq!(s.effect_catalog.filters(), catalog.filters());
        assert_eq!(s.engine.document(), &document);
        let source = s.capture_project_recovery().unwrap();
        assert_eq!(source.assets, assets);
        assert!(s.command(CommandId::SaveDocumentAs).enabled);
        assert!(
            s.dispatch(UiAction::Color {
                action: ColorAction::Shape {
                    shape: ColorShape::Triangle
                }
            })
            .is_err()
        );
        assert_eq!(s.engine.document(), &document);
    }

    #[test]
    fn failed_asset_upload_leaves_current_document_and_gpu_queries_intact() {
        let mut s = UiSession::new(
            Backend::default(),
            Document::new("failed", 128, 128),
            [128, 128],
        )
        .unwrap();
        s.import_layer_asset(
            "Source",
            ProjectAsset {
                extent: [1, 1],
                format: layer_core::ProjectAssetFormat::Rgba8Srgb,
                bytes: std::sync::Arc::from([20, 30, 40, 255]),
            },
        )
        .unwrap();
        s.eyedropper
            .queue(layer_render::ColorSampleSource::Composite, [10, 10]);
        s.eyedropper.poll(s.engine.backend_mut()).unwrap();
        let document = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        assert!(
            s.replace_renderer(Backend {
                reject_assets: true,
                ..Default::default()
            })
            .is_err()
        );
        assert_eq!(s.engine.document(), &document);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert!(s.eyedropper.busy());
        assert!(!s.engine.backend().reject_assets);
        assert_eq!(s.engine.backend().samples, 1);
    }
}
