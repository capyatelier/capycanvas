//! An explicit export owns its staging buffer, never the live renderer. Waiting
//! and packing rows belong to the file worker, after GPU submission by the owner.
use super::*;

pub struct ExportReadback {
    device: wgpu::Device,
    buffer: wgpu::Buffer,
    submission: wgpu::SubmissionIndex,
    receiver: mpsc::Receiver<Result<(), String>>,
    request_id: u64,
    extent: [u32; 2],
    padded_row_bytes: u32,
}
impl ExportReadback {
    pub(super) fn new(
        device: wgpu::Device,
        buffer: wgpu::Buffer,
        submission: wgpu::SubmissionIndex,
        receiver: mpsc::Receiver<Result<(), String>>,
        request_id: u64,
        extent: [u32; 2],
        padded_row_bytes: u32,
    ) -> Self {
        Self {
            device,
            buffer,
            submission,
            receiver,
            request_id,
            extent,
            padded_row_bytes,
        }
    }
    /// Worker only. The document can change or close after the ticket is issued;
    /// the copied texture and its queue position preserve the captured pixels.
    pub fn finish(self) -> Result<ReadbackImage, GpuRasterError> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(self.submission),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|e| GpuRasterError::WaitFailed(e.to_string()))?;
        self.receiver
            .recv_timeout(READBACK_TIMEOUT)
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .map_err(GpuRasterError::MapFailed)?;
        let mapped = self
            .buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
        let [width, height] = self.extent;
        let stride = width * 4;
        let mut bytes = vec![0; stride as usize * height as usize];
        for (source, destination) in mapped
            .chunks_exact(self.padded_row_bytes as usize)
            .zip(bytes.chunks_exact_mut(stride as usize))
        {
            destination.copy_from_slice(&source[..stride as usize]);
        }
        drop(mapped);
        self.buffer.unmap();
        Ok(ReadbackImage {
            request_id: self.request_id,
            width,
            height,
            stride,
            bytes,
        })
    }
}
