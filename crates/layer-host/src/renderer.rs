use layer_core::AssetId;
use layer_render::{CanvasRenderer, FramePacket, HostImage, ReadbackImage, TipOutline};
use layer_render_wgpu::{GpuRasterError, WgpuRasterizer};

/// Not a CPU fallback: until a GPU device is attached, only UI/viewport
/// bookkeeping is available. Pixel operations fail explicitly.
#[derive(Default)]
pub struct Renderer(pub Option<WgpuRasterizer>);
impl Renderer {
    fn gpu(&mut self) -> Result<&mut WgpuRasterizer, GpuRasterError> {
        self.0.as_mut().ok_or(GpuRasterError::AdapterUnavailable)
    }
}
impl CanvasRenderer for Renderer {
    fn set_transform_preview(
        &mut self,
        preview: Option<&layer_render::TransformPreview>,
    ) -> Result<(), Self::Error> {
        self.gpu()?.set_transform_preview(preview)
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
    fn request_thumbnail(
        &mut self,
        id: u64,
        target: layer_core::LayerId,
    ) -> Result<(), Self::Error> {
        self.gpu()?.request_thumbnail(id, target)
    }
    fn take_thumbnail(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.0.as_mut()?.take_thumbnail()
    }
    fn tip_outline(&self, id: &AssetId) -> Option<&TipOutline> {
        self.0.as_ref()?.tip_outline(id)
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
    fn source_asset(&self, id: &AssetId) -> Option<layer_core::ProjectAsset> {
        self.0.as_ref()?.source_asset(id)
    }
    fn release_asset(&mut self, id: &AssetId) {
        if let Some(gpu) = &mut self.0 {
            gpu.release_asset(id);
        }
    }
    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
        self.gpu()?.submit(packet)
    }
    fn request_readback(&mut self, id: u64) -> Result<(), Self::Error> {
        self.gpu()?.request_readback(id)
    }
    fn take_readback(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.0.as_mut()?.take_readback()
    }
}
