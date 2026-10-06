use crate::{GpuRasterError, WgpuRasterizer};
use layer_core::AssetId;
use layer_render::{CanvasRenderer, FramePacket, HostImage, ReadbackImage, TipOutline};

/// Not a CPU fallback: until a GPU device is attached, only UI/viewport
/// bookkeeping is available. Pixel operations fail explicitly. Keep the large
/// GPU owner on the heap so native render-thread stacks can replace it safely.
#[derive(Default)]
pub struct AttachedRenderer(pub Option<Box<WgpuRasterizer>>);
impl AttachedRenderer {
    fn gpu(&mut self) -> Result<&mut WgpuRasterizer, GpuRasterError> {
        self.0.as_deref_mut().ok_or(GpuRasterError::AdapterUnavailable)
    }
}
impl CanvasRenderer for AttachedRenderer {
    fn preflight_image_object_affine(&self, scene: layer_core::SceneView<'_>, object: layer_core::authored::ImageObjectHandle,
        affine: layer_core::authored::Affine64, view: layer_render::ViewState,
    ) -> Result<(), Self::Error> {
        self.0.as_deref().ok_or(GpuRasterError::AdapterUnavailable)?.preflight_image_object_affine(scene, object, affine, view)
    }
    fn shader_input(&mut self) { if let Some(gpu) = &self.0 { gpu.shader_input(); } }
    fn shader_idle(&mut self, idle: bool) { if let Some(gpu) = &self.0 { gpu.shader_idle(idle); } }
    fn shaders_need_update(&self, document: &layer_core::Document, brush: &layer_core::BrushSnapshot, transform: bool) -> bool {
        self.0.as_ref().is_some_and(|gpu| gpu.startup_needs_update(document, brush, transform))
    }
    fn evaluation_context(&self) -> layer_core::authored::EvaluationContext {
        self.0.as_deref().map(CanvasRenderer::evaluation_context).unwrap_or_default()
    }
    fn seed_evaluation_context(&mut self, context: layer_core::authored::EvaluationContext) {
        if let Some(gpu) = self.0.as_deref_mut() { gpu.seed_evaluation_context(context); }
    }
    fn document_color(&self) -> layer_core::color::DocumentColor {
        self.0.as_deref().map(CanvasRenderer::document_color).unwrap_or_default()
    }
    fn adopt_prepared_color(&mut self, color: layer_core::color::DocumentColor) -> Result<bool, Self::Error> {
        self.gpu()?.adopt_prepared_color(color)
    }
    fn supports_tiled_sources(&self) -> bool {
        self.0.as_deref().is_some_and(CanvasRenderer::supports_tiled_sources)
    }
    fn supports_raster_damage(&self) -> bool {
        self.0.as_deref().is_some_and(CanvasRenderer::supports_raster_damage)
    }
    fn max_document_dimension(&self) -> u32 {
        self.0.as_deref().map_or(u32::MAX, CanvasRenderer::max_document_dimension)
    }
    fn raster_dependencies_ready(&mut self, packet: FramePacket<'_>) -> bool {
        self.0
            .as_mut()
            .is_none_or(|gpu| gpu.raster_dependencies_ready(packet))
    }
    fn can_capture_raster(&self) -> bool {
        self.0.as_ref().is_none_or(|gpu| gpu.can_capture_raster())
    }
    fn poll_pending(&mut self, view: layer_render::ViewState) -> Result<(), Self::Error> {
        if let Some(gpu) = self.0.as_mut() { gpu.poll_pending(view)?; }
        Ok(())
    }
    fn has_pending_submission(&self) -> bool { self.0.as_ref().is_some_and(|gpu| gpu.has_pending_submission()) }
    fn can_submit(&self) -> bool {
        self.0.as_ref().is_none_or(|gpu| gpu.can_submit())
    }
    fn has_pending_work(&self) -> bool {
        self.0.as_ref().is_some_and(|gpu| gpu.has_pending_work())
    }
    fn prepare_moving_layer(&mut self, layer: Option<layer_core::authored::OccurrenceHandle>) {
        if let Some(gpu) = self.0.as_mut() {
            gpu.prepare_moving_layer(layer);
        }
    }
    fn prepare_moving_pixels(&mut self, pixels: Option<(layer_core::authored::SourceTarget, layer_core::Selection)>) {
        if let Some(gpu) = self.0.as_mut() {
            gpu.prepare_moving_pixels(pixels);
        }
    }
    fn prepare_retouch(&mut self, retouch: Option<&layer_render::RetouchPreparation>) {
        if let Some(gpu) = self.0.as_mut() {
            gpu.prepare_retouch(retouch);
        }
    }
    fn take_retouch_miss(&mut self) -> Option<layer_core::StrokeId> {
        self.0.as_mut()?.take_retouch_miss()
    }
    fn retouch_waiting(&self) -> Option<layer_core::StrokeId> {
        self.0.as_ref()?.retouch_waiting()
    }
    fn retire_stroke_sources(&mut self) {
        if let Some(gpu) = self.0.as_mut() {
            gpu.retire_stroke_sources();
        }
    }

    fn set_transform_preview(
        &mut self,
        preview: Option<&layer_render::TransformPreview>,
    ) -> Result<(), Self::Error> {
        self.gpu()?.set_transform_preview(preview)
    }
    fn request_effect_analysis(&mut self, query: layer_core::ArtworkQuery) -> Result<bool, Self::Error> { self.gpu()?.request_effect_analysis(query) }
    fn take_effect_analysis(&mut self) -> Option<Result<(), Self::Error>> { self.0.as_mut()?.take_effect_analysis() }
    fn accept_effect_analysis(&mut self) -> Result<(), Self::Error> { self.gpu()?.accept_effect_analysis() }
    fn cancel_effect_analysis(&mut self) { if let Some(gpu) = self.0.as_mut() { gpu.cancel_effect_analysis(); } }
    fn retain_effect_analyses(&mut self, layers: &[layer_core::authored::OccurrenceHandle]) -> Result<(), Self::Error> {
        self.0.as_mut().map_or(Ok(()), |gpu| gpu.retain_effect_analyses(layers))
    }
    fn request_snapshot(&mut self, request: layer_render::SnapshotRequest) -> Result<bool, Self::Error> {
        self.gpu()?.request_snapshot(request)
    }
    fn take_snapshot(&mut self) -> Option<Result<layer_render::SnapshotResult, Self::Error>> {
        self.0.as_mut()?.take_snapshot()
    }
    fn cancel_snapshot(&mut self) {
        if let Some(gpu) = self.0.as_mut() { gpu.cancel_snapshot(); }
    }
    fn request_color_sample(
        &mut self,
        request: layer_render::ColorSampleRequest,
    ) -> Result<bool, Self::Error> {
        self.gpu()?.request_color_sample(request)
    }
    fn take_color_sample(&mut self) -> Option<Result<layer_render::ColorSample, Self::Error>> {
        self.0.as_mut()?.take_color_sample()
    }
    fn request_region(
        &mut self,
        request: layer_render::RegionRequest,
    ) -> Result<bool, Self::Error> {
        self.gpu()?.request_region(request)
    }
    fn take_region(&mut self) -> Option<Result<layer_render::RegionResult, Self::Error>> {
        self.0.as_mut()?.take_region()
    }
    fn cancel_region(&mut self) {
        if let Some(gpu) = self.0.as_mut() {
            gpu.cancel_region();
        }
    }
    fn paint_selection(&mut self, update: &layer_render::SelectionPaint) -> Result<bool,Self::Error> {
        self.gpu()?.paint_selection(update)
    }
    fn take_selection_paint(&mut self) -> Option<Result<layer_render::SelectionPaintResult,Self::Error>> {
        (self.0.as_mut()?).take_selection_paint()
    }
    fn cancel_selection_paint(&mut self) {
        if let Some(gpu) = self.0.as_mut() { gpu.cancel_selection_paint(); }
    }
    fn set_quick_mask_thumbnail(&mut self, selection: Option<&layer_core::Selection>) {
        if let Some(gpu) = self.0.as_mut() { gpu.set_quick_mask_thumbnail(selection); }
    }
    fn set_selection_overlay(&mut self, overlay: Option<layer_render::SelectionOverlay>) {
        if let Some(gpu) = self.0.as_mut() { gpu.set_selection_overlay(overlay); }
    }
    fn set_clipping_preview(&mut self, shadows: bool, highlights: bool) {
        if let Some(gpu) = self.0.as_mut() { gpu.set_clipping_preview(shadows, highlights); }
    }
    fn set_crop_overlay(&mut self, overlay: Option<layer_render::CropOverlay>) {
        if let Some(gpu) = self.0.as_mut() { gpu.set_crop_overlay(overlay); }
    }
    fn set_selection_outline(
        &mut self,
        selection: Option<&layer_core::Selection>,
    ) -> Result<(), Self::Error> {
        self.gpu()?.set_selection_outline(selection)
    }
    fn request_effect_validation(
        &mut self,
        request: layer_render::EffectValidationRequest,
    ) -> Result<bool, Self::Error> {
        self.gpu()?.request_effect_validation(request)
    }
    fn take_effect_validation(&mut self) -> Option<layer_render::EffectValidationResult> {
        self.0.as_mut()?.take_effect_validation()
    }
    fn set_telemetry_enabled(&mut self, enabled: bool) {
        if let Some(gpu) = &mut self.0 {
            gpu.set_telemetry_enabled(enabled);
        }
    }
    fn telemetry(&self) -> layer_render::RendererTelemetry {
        self.0.as_ref().map(|g| g.telemetry()).unwrap_or_default()
    }
    type Error = GpuRasterError;
    fn request_filter_previews(
        &mut self,
        request: layer_render::FilterPreviewRequest,
    ) -> Result<bool, Self::Error> {
        self.gpu()?.request_filter_previews(request)
    }
    fn take_filter_previews(
        &mut self,
    ) -> Option<Result<layer_render::FilterPreviewImage, Self::Error>> {
        self.0.as_mut()?.take_filter_previews()
    }
    fn cancel_filter_previews(&mut self) {
        if let Some(gpu) = &mut self.0 { gpu.cancel_filter_previews(); }
    }
    fn request_thumbnail(
        &mut self,
        id: u64,
        target: layer_render::ThumbnailTarget,
    ) -> Result<(), Self::Error> {
        self.gpu()?.request_thumbnail(id, target)
    }
    fn take_thumbnail(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.0.as_mut()?.take_thumbnail()
    }
    fn tip_outline(&self, id: &AssetId) -> Option<&TipOutline> {
        self.0.as_ref()?.tip_outline(id)
    }
    fn tip_mask(&self, id: &AssetId) -> Option<HostImage<'_>> {
        self.0.as_ref()?.tip_mask(id)
    }
    fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), Self::Error> {
        if let Some(gpu) = &mut self.0 {
            gpu.resize_surface(width, height)?;
        }
        Ok(())
    }
    fn prepare_asset(&mut self, id: &AssetId, image: HostImage<'_>) -> Result<(), Self::Error> {
        self.gpu()?.prepare_asset(id, image)
    }
    fn release_asset(&mut self, id: &AssetId) {
        if let Some(gpu) = &mut self.0 {
            gpu.release_asset(id);
        }
    }
    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
        self.gpu()?.submit(packet)
    }
}

#[test]
fn renderer_replacement_keeps_native_stack_usage_bounded() {
    assert_eq!(std::mem::size_of::<AttachedRenderer>(), std::mem::size_of::<usize>());
}

#[test]
fn analysis_retention_before_device_attachment_needs_no_gpu() {
    let mut renderer=AttachedRenderer::default();
    renderer.retain_effect_analyses(&[layer_core::OccurrenceHandle::from_index(3),layer_core::OccurrenceHandle::from_index(7)]).unwrap();
    renderer.retain_effect_analyses(&[]).unwrap();
    assert!(renderer.0.is_none());
}
