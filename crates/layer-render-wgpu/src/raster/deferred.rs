//! Per-publication native outputs let readback follow canvas presentation.
//! Encoding writes immutable native samples directly, without a snapshot copy.
use super::*;
use std::sync::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Condvar;

#[derive(Default)]
pub(super) struct PresentationPriority {
    active: Mutex<usize>,
    #[cfg(not(target_arch = "wasm32"))]
    wake: Condvar,
    #[cfg(target_arch = "wasm32")]
    wake: Mutex<Option<std::task::Waker>>,
}
impl PresentationPriority {
    fn enter(self: &Arc<Self>) -> RasterPresentation {
        *self.active.lock().unwrap() += 1;
        RasterPresentation(self.clone())
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn wait(&self) -> Result<(), String> {
        let active = self.active.lock().unwrap();
        let (active, _) = self
            .wake
            .wait_timeout_while(active, READBACK_TIMEOUT, |active| *active != 0)
            .unwrap();
        if *active != 0 {
            return Err("Canvas presentation did not release raster backing".into());
        }
        Ok(())
    }
}
/// Hold from canvas encoding through presentation submission. Only a previously
/// submitted bounded transfer may precede the frame; acquiring never waits for
/// GPU completion or compression.
#[must_use]
pub struct RasterPresentation(Arc<PresentationPriority>);
impl Drop for RasterPresentation {
    fn drop(&mut self) {
        let mut active = self.0.active.lock().unwrap();
        *active -= 1;
        if *active == 0 {
            #[cfg(not(target_arch = "wasm32"))]
            self.0.wake.notify_all();
            #[cfg(target_arch = "wasm32")]
            if let Some(wake) = self.0.wake.lock().unwrap().take() { wake.wake(); }
        }
    }
}
pub(super) enum CaptureBatch {
    Mapped(Vec<RasterCapture>),
    Native(NativeCapture),
}
impl CaptureBatch {
    pub fn storage_bytes(&self) -> u64 {
        match self {
            Self::Mapped(captures) => captures.iter().map(|c| c.staging_bytes).sum(),
            Self::Native(capture) => capture.storage_bytes(),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn finish(self) -> Result<(), String> {
        match self {
            Self::Mapped(captures) => {
                for capture in captures {
                    capture.finish()?;
                }
                Ok(())
            }
            Self::Native(capture) => capture.finish(),
        }
    }
}
pub(super) struct NativeOutput {
    pub resource: pool::Resource,
    pub tile: RasterTile,
}
impl NativeOutput {
    pub fn capture(&self) -> TileCapture<'_> {
        TileCapture {
            source: match &self.resource {
                pool::Resource::Buffer(buffer) => CaptureSource::Packed(buffer),
                pool::Resource::Texture(texture) => CaptureSource::Texture(texture),
            },
            tile: self.tile.clone(),
        }
    }
}
pub(super) struct NativeCapture {
    pub outputs: Vec<NativeOutput>,
    pub status: NativeEncodeStatus,
    pub pool: Arc<BufferPool>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}
impl NativeCapture {
    fn storage_bytes(&self) -> u64 {
        STATUS_BYTES + self.outputs.iter().map(|o| o.resource.bytes()).sum::<u64>()
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn finish(mut self) -> Result<(), String> {
        let mut validated = false;
        while !self.outputs.is_empty() {
            self.pool.priority.wait()?;
            let mut count = 0;
            let mut size = 0;
            for output in &self.outputs {
                if size + output.resource.bytes() > CAPTURE_CHUNK {
                    break;
                }
                count += 1;
                size += output.resource.bytes();
            }
            let copies: Vec<_> = self.outputs[..count]
                .iter()
                .map(NativeOutput::capture)
                .collect();
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("background native raster readback"),
                });
            let capture = prepare_capture(
                &self.device,
                &self.pool,
                &mut encoder,
                &copies,
                (!validated).then_some(&self.status),
            )
            .map_err(|e| e.to_string())?;
            struct Transfer(Arc<BufferPool>, u64);
            impl Drop for Transfer {
                fn drop(&mut self) {
                    self.0.transfer.fetch_sub(self.1, Ordering::Relaxed);
                }
            }
            self.pool
                .transfer
                .fetch_add(capture.staging_bytes, Ordering::Relaxed);
            let _transfer = Transfer(self.pool.clone(), capture.staging_bytes);
            let commands = encoder.finish();
            // Recheck after preparation. A frame can race this check, but only
            // this one bounded transfer can then precede it on the GPU queue.
            self.pool.priority.wait()?;
            let submission = self.queue.submit([commands]);
            capture
                .submitted_device(&self.device, submission)
                .finish()?;
            validated = true;
            // Never recycle a writable output before its GPU readback completes.
            for output in self.outputs.drain(..count) {
                self.pool.put_resource(output.resource);
            }
        }
        Ok(())
    }
}
impl Drop for NativeCapture {
    fn drop(&mut self) {
        for output in &self.outputs {
            if output.tile.try_backing().is_none() {
                let _ = output
                    .tile
                    .publish(Err("Native raster backing was abandoned".into()));
            }
        }
    }
}
impl WgpuRasterizer {
    pub fn prioritize_raster_presentation(&self) -> RasterPresentation {
        self.raster_buffers.priority.enter()
    }
}

#[cfg(target_arch = "wasm32")]
impl PresentationPriority {
    async fn wait_browser(&self) {
        std::future::poll_fn(|cx| {
            if *self.active.lock().unwrap() == 0 {
                std::task::Poll::Ready(())
            } else {
                *self.wake.lock().unwrap() = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }).await
    }
}
#[cfg(target_arch = "wasm32")]
impl CaptureBatch {
    pub(super) async fn finish_browser(self, encoder: &browser::BrowserRasterEncoder) -> Result<(), String> {
        match self {
            Self::Mapped(captures) => {
                for capture in captures { capture.finish_browser(encoder).await?; }
                Ok(())
            }
            Self::Native(capture) => capture.finish_browser(encoder).await,
        }
    }
}
#[cfg(target_arch = "wasm32")]
impl NativeCapture {
    async fn finish_browser(mut self, worker: &browser::BrowserRasterEncoder) -> Result<(), String> {
        let mut validated = false;
        while !self.outputs.is_empty() {
            self.pool.priority.wait_browser().await;
            let mut count = 0;
            let mut size = 0;
            for output in &self.outputs {
                if size + output.resource.bytes() > CAPTURE_CHUNK { break; }
                count += 1;
                size += output.resource.bytes();
            }
            let copies: Vec<_> = self.outputs[..count].iter().map(NativeOutput::capture).collect();
            let mut commands = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("browser native raster readback"),
            });
            let capture = prepare_capture(&self.device, &self.pool, &mut commands, &copies,
                (!validated).then_some(&self.status)).map_err(|e| e.to_string())?;
            struct Transfer(Arc<BufferPool>, u64);
            impl Drop for Transfer {
                fn drop(&mut self) { self.0.transfer.fetch_sub(self.1, Ordering::Relaxed); }
            }
            self.pool.transfer.fetch_add(capture.staging_bytes, Ordering::Relaxed);
            let _transfer = Transfer(self.pool.clone(), capture.staging_bytes);
            self.pool.priority.wait_browser().await;
            let submission = self.queue.submit([commands.finish()]);
            capture.submitted_device(&self.device, submission).finish_browser(worker).await?;
            validated = true;
            for output in self.outputs.drain(..count) { self.pool.put_resource(output.resource); }
        }
        Ok(())
    }
}
