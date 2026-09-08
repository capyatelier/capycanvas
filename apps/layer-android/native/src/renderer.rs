use layer_core::AssetId;
use layer_render::{CanvasRenderer, FramePacket, HostImage, ReadbackImage, TipOutline};
use layer_render_wgpu::{GpuRasterError, WgpuRasterizer};

/// Not a CPU fallback: until a Vulkan device is attached, only UI/viewport
/// bookkeeping is available. Pixel operations fail explicitly.
#[derive(Default)]
pub(crate) struct Renderer(pub Option<WgpuRasterizer>);
impl Renderer {
    fn gpu(&mut self) -> Result<&mut WgpuRasterizer, GpuRasterError> {
        self.0.as_mut().ok_or(GpuRasterError::AdapterUnavailable)
    }
}
impl CanvasRenderer for Renderer {
    type Error = GpuRasterError;
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
