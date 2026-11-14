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
    sender: Option<mpsc::SyncSender<(NativeCapture,[crate::residency::Charge;2])>>,
    pending: Arc<crate::residency::Tier>,
    pub(super) staging: Arc<crate::residency::Tier>,
    pub(super) error: Arc<std::sync::Mutex<Option<String>>>,
    wake: Rc<RefCell<Option<Waker>>>,
}
impl CaptureWorker {
    pub(super) fn new(
        encoder: BrowserRasterEncoder,
        completion:Arc<PreparationWake>,
    ) -> Result<Self, GpuRasterError> {
        let (sender, receiver) = mpsc::sync_channel::<(NativeCapture,[crate::residency::Charge;2])>(16);
        let error = Arc::new(std::sync::Mutex::new(None));
        let wake: Rc<RefCell<Option<Waker>>> = Rc::default();
        let (failure, signal) = (error.clone(), wake.clone());
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
                let Some((captures, charges)) = captures else { break };
                if failure.lock().unwrap().is_some() {
                    drop(captures);
                } else if let Err(message) = captures.finish_browser(&encoder).await {
                    *failure.lock().unwrap() = Some(message);
                }
                drop(charges);
                std::task::Wake::wake_by_ref(&completion);
            }
        });
        Ok(Self {
            sender: Some(sender),
            pending: Arc::default(),
            staging: Arc::default(),
            error,
            wake,
        })
    }
    pub(super) fn ready(&self) -> bool {
        self.pending.used() < 16
            && self.staging.used() <= MAX_CAPTURE_BYTES - CAPTURE_CHUNK
    }
    pub(super) fn submit_batch(&self, captures: NativeCapture) -> Result<(), GpuRasterError> {
        let charges = [self.staging.charge(captures.storage_bytes()), self.pending.charge(1)];
        if self.sender.as_ref().unwrap().try_send((captures, charges)).is_err() {
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
    #[cfg(test)]
    pub(super) async fn finish_browser(self,encoder:&BrowserRasterEncoder)->Result<(),String> {
        let pool=self.pool.clone();self.finish_browser_recycling(encoder,move|buffer|pool.put(buffer)).await
    }
    pub(super) async fn finish_browser_recycling(mut self,encoder:&BrowserRasterEncoder,recycle:impl Fn(wgpu::Buffer))->Result<(),String> {
        let result: Result<(), String> = async {
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
                    let mut bytes=Vec::with_capacity(size as usize);
                    let mut offset=0;
                    for entry in &chunk.entries {
                        if let Some((at,comparison))=&entry.comparison {comparison.resolve(&mapped[*at as usize..*at as usize+crate::native_tiles::comparison::RESULT_BYTES as usize])?;}
                        bytes.extend_from_slice(&mapped[offset..offset+entry.size as usize]);
                        offset+=entry.size as usize;
                    }
                    bytes
                };
                chunk.buffer.unmap();
                recycle(chunk.buffer.clone());
                let _scratch = self.pool.working.charge(size);
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
