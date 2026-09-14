//! GPU objects stay on their browser event loop; only packed bytes cross to the
//! host's compression worker. One consumer bounds cached scratch per renderer.
use super::*;
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, task::Waker};

pub type BrowserRasterEncoder = Rc<
    dyn Fn(
        Vec<u8>,
        Vec<layer_core::color::PixelDescriptor>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<TileBlob>, String>>>>,
>;

pub(super) struct CaptureWorker {
    sender: Option<mpsc::SyncSender<Vec<RasterCapture>>>,
    pending: Arc<AtomicUsize>,
    pub(super) staging: Arc<AtomicU64>,
    pub(super) error: Arc<std::sync::Mutex<Option<String>>>,
    wake: Rc<RefCell<Option<Waker>>>,
}
impl CaptureWorker {
    pub(super) fn new(
        _device: wgpu::Device,
        _pool: Arc<BufferPool>,
        encoder: BrowserRasterEncoder,
    ) -> Result<Self, GpuRasterError> {
        let (sender, receiver) = mpsc::sync_channel::<Vec<RasterCapture>>(16);
        let pending = Arc::new(AtomicUsize::new(0));
        let staging = Arc::new(AtomicU64::new(0));
        let error = Arc::new(std::sync::Mutex::new(None));
        let wake: Rc<RefCell<Option<Waker>>> = Rc::default();
        let (count, bytes, failure, signal) = (
            pending.clone(),
            staging.clone(),
            error.clone(),
            wake.clone(),
        );
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let captures = std::future::poll_fn(|cx| match receiver.try_recv() {
                    Ok(job) => std::task::Poll::Ready(Some(job)),
                    Err(mpsc::TryRecvError::Disconnected) => std::task::Poll::Ready(None),
                    Err(mpsc::TryRecvError::Empty) => {
                        *signal.borrow_mut() = Some(cx.waker().clone());
                        std::task::Poll::Pending
                    }
                })
                .await;
                let Some(captures) = captures else { break };
                let size: u64 = captures.iter().map(|c| c.staging_bytes).sum();
                for capture in captures {
                    if failure.lock().unwrap().is_some() {
                        drop(capture);
                    } else if let Err(message) = capture.finish_browser(&encoder).await {
                        *failure.lock().unwrap() = Some(message);
                    }
                }
                bytes.fetch_sub(size, Ordering::Release);
                count.fetch_sub(1, Ordering::Release);
            }
        });
        Ok(Self {
            sender: Some(sender),
            pending,
            staging,
            error,
            wake,
        })
    }
    pub(super) fn ready(&self) -> bool {
        self.pending.load(Ordering::Acquire) < 16
            && self.staging.load(Ordering::Acquire) <= MAX_CAPTURE_BYTES
    }
    pub(super) fn submit(&self, captures: Vec<RasterCapture>) -> Result<(), GpuRasterError> {
        let size = captures.iter().map(|c| c.staging_bytes).sum();
        self.pending.fetch_add(1, Ordering::Release);
        self.staging.fetch_add(size, Ordering::Release);
        if self.sender.as_ref().unwrap().try_send(captures).is_err() {
            self.pending.fetch_sub(1, Ordering::Release);
            self.staging.fetch_sub(size, Ordering::Release);
            return Err(GpuRasterError::Effect(
                "Browser raster queue is full or stopped".into(),
            ));
        }
        if let Some(waker) = self.wake.borrow_mut().take() {
            waker.wake();
        }
        Ok(())
    }
}
impl Drop for CaptureWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(waker) = self.wake.borrow_mut().take() {
            waker.wake();
        }
    }
}

impl RasterCapture {
    async fn finish_browser(mut self, encoder: &BrowserRasterEncoder) -> Result<(), String> {
        let result: Result<(), String> = async {
            if let Some(validation) = &mut self.validation {
                (&mut validation.ready)
                    .await
                    .map_err(|_| "Raster validation mapping was abandoned")??;
                self.accept_validation()?;
            }
            for chunk in &mut self.chunks {
                (&mut chunk.ready)
                    .await
                    .map_err(|_| "Raster mapping was abandoned")??;
                let size = chunk.entries.iter().map(|e| e.size).sum::<u64>();
                let bytes = {
                    let mapped = chunk
                        .buffer
                        .get_mapped_range(..)
                        .map_err(|e| e.to_string())?;
                    mapped[..size as usize].to_vec()
                };
                chunk.buffer.unmap();
                self.pool.put(chunk.buffer.clone());
                struct Scratch(Arc<BufferPool>, u64);
                impl Drop for Scratch {
                    fn drop(&mut self) {
                        self.0.working.fetch_sub(self.1, Ordering::Release);
                    }
                }
                self.pool.working.fetch_add(size, Ordering::Release);
                let _scratch = Scratch(self.pool.clone(), size);
                let descriptors = chunk.entries.iter().map(|e| e.descriptor).collect();
                let blobs = encoder(bytes, descriptors).await?;
                if blobs.len() != chunk.entries.len() {
                    return Err("Raster worker returned the wrong tile count".into());
                }
                for (entry, blob) in chunk.entries.iter().zip(blobs) {
                    if blob.descriptor != entry.descriptor {
                        return Err("Raster worker changed the pixel representation".into());
                    }
                    entry.tile.publish(Ok(blob))?;
                }
            }
            Ok(())
        }
        .await;
        if let Err(message) = &result {
            self.fail(message);
        }
        self.chunks.clear();
        result
    }
}
