//! Bound the native command buffers materialized by a large document replay.
//! wgpu can expand each render pass into multiple Metal command buffers at
//! finish. Keep chunks unfinalized until all uploads are closed, then finish
//! and submit each chunk in order before materializing the next one.
use std::ops::{Deref, DerefMut};

/// A cached GPU value is usable in queue order while its producing commands
/// are pending. Discarding those commands must invalidate the CPU cache key.
pub(crate) struct CacheWrite {
    valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tracked: std::sync::atomic::AtomicBool,
}
impl CacheWrite {
    pub fn new() -> Self {
        Self::shared(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)))
    }
    pub fn shared(valid: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        Self {
            valid,
            tracked: std::sync::atomic::AtomicBool::new(false),
        }
    }
    pub fn validity(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.valid.clone()
    }
    pub fn recorded(&self)->bool {self.tracked.load(std::sync::atomic::Ordering::Acquire)}
    fn guard(&self)->DiscardedCacheWrite {
        assert!(!self.tracked.swap(true,std::sync::atomic::Ordering::Relaxed));
        DiscardedCacheWrite(Some(self.valid.clone()))
    }
    pub fn track(&self, encoder: &CommandEncoder) {
        let guard=self.guard();
        encoder.on_submitted_work_done(move || guard.complete());
    }
    pub fn track_raw(&self,device:&crate::PipelineDevice,encoder:&wgpu::CommandEncoder) {
        let guard=self.guard();
        device.on_encoder_done(encoder,move||guard.complete());
    }
}
impl Drop for CacheWrite {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        if !self.tracked.load(Ordering::Relaxed) {
            self.valid.store(false, Ordering::Release);
        }
    }
}
struct DiscardedCacheWrite(Option<std::sync::Arc<std::sync::atomic::AtomicBool>>);
impl DiscardedCacheWrite {
    fn complete(mut self) {
        self.0 = None;
    }
}
impl Drop for DiscardedCacheWrite {
    fn drop(&mut self) {
        if let Some(valid) = &self.0 {
            valid.store(false, std::sync::atomic::Ordering::Release);
        }
    }
}

#[derive(Clone)]
pub(crate) struct Submission(std::sync::Arc<std::sync::atomic::AtomicU8>);
#[repr(u8)]
enum SubmissionState { Encoding, Submitted, Discarded, Complete }
impl Submission {
    pub fn same(&self, other: &Self) -> bool { std::sync::Arc::ptr_eq(&self.0, &other.0) }
    #[cfg(test)]
    pub(crate) fn observation_id(&self)->usize {std::sync::Arc::as_ptr(&self.0) as usize}
    pub fn submitted(&self)->bool {matches!(self.0.load(std::sync::atomic::Ordering::Acquire),value if value==SubmissionState::Submitted as u8 || value==SubmissionState::Complete as u8)}
    pub fn discarded(&self)->bool {self.0.load(std::sync::atomic::Ordering::Acquire)==SubmissionState::Discarded as u8}
    pub fn complete(&self)->bool {self.0.load(std::sync::atomic::Ordering::Acquire)==SubmissionState::Complete as u8}
    pub fn pending(&self)->bool {self.0.load(std::sync::atomic::Ordering::Acquire)<SubmissionState::Discarded as u8}
}
#[derive(Default)]
pub(crate) struct LatestSubmission {index:Option<wgpu::SubmissionIndex>,completed:std::sync::Arc<std::sync::OnceLock<web_time::Instant>>,observed:bool}
impl LatestSubmission {
    pub fn record(&mut self,index:&wgpu::SubmissionIndex,completed:std::sync::Arc<std::sync::OnceLock<web_time::Instant>>,observed:bool) {
        if self.index.as_ref().is_none_or(|latest|latest<=index) {*self=Self {index:Some(index.clone()),completed,observed};}
    }
    pub fn completed(&mut self)->(std::sync::Arc<std::sync::OnceLock<web_time::Instant>>,bool) {(self.completed.clone(),!std::mem::replace(&mut self.observed,true))}
}
struct PendingSubmission(Submission);
impl Drop for PendingSubmission {
    fn drop(&mut self) {
        let _=self.0.0.try_update(std::sync::atomic::Ordering::AcqRel,std::sync::atomic::Ordering::Acquire,|state|
            (state==SubmissionState::Encoding as u8 || state==SubmissionState::Submitted as u8).then_some(SubmissionState::Discarded as u8));
    }
}

pub(crate) struct CommandEncoder {
    submission: Option<PendingSubmission>,
    compute: Option<wgpu::ComputePass<'static>>,
    device: wgpu::Device,
    prologue: Option<wgpu::CommandEncoder>,
    label: Option<String>,
    current: std::cell::OnceCell<wgpu::CommandEncoder>,
    earlier: Vec<wgpu::CommandEncoder>,
    passes: usize,
    total_passes: u64,
    resource_work: bool,
    completions: std::cell::RefCell<Vec<Box<dyn FnOnce()+Send>>>,
    latest: std::sync::Arc<std::sync::Mutex<LatestSubmission>>,
    #[cfg(not(target_arch="wasm32"))]
    poll: crate::device_poll::Binding,
}

impl CommandEncoder {
    /// wgpu records each pass and the barriers before it into about two
    /// command buffers, and Android destroys a command pool that held more
    /// than 128 once its work completes (`vendor/README.md`), so chunks this
    /// size keep reusing their pools.
    pub(crate) const PASSES_PER_SUBMISSION: usize = 56;

    pub fn new(device: &crate::PipelineDevice, descriptor: &wgpu::CommandEncoderDescriptor<'_>) -> Self {
        Self {
            submission: None,
            compute: None,
            device: (**device).clone(),
            #[cfg(not(target_arch="wasm32"))]
            poll: device.poll_binding(),
            prologue: None,
            label: descriptor.label.map(str::to_owned),
            current: Default::default(),
            earlier: Vec::new(),
            passes: 0,
            total_passes: 0,
            resource_work: false,
            completions: Default::default(),
            latest: device.latest_submission.clone(),
        }
    }
    pub fn submission(&mut self)->Submission {
        self.submission.get_or_insert_with(||PendingSubmission(Submission(std::sync::Arc::new(std::sync::atomic::AtomicU8::new(SubmissionState::Encoding as u8))))).0.clone()
    }
    fn current(&self)->&wgpu::CommandEncoder {
        self.current.get_or_init(|| {
            #[cfg(test)] crate::test_support::PassCounts {command_encoders:1,..Default::default()}.record();
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {label:self.label.as_deref()})
        })
    }
    fn current_mut(&mut self)->&mut wgpu::CommandEncoder {self.current();self.current.get_mut().unwrap()}
    fn begin_pass(&mut self) {
        self.reserve_passes(1);
    }
    /// Account for a bounded helper that records raw wgpu passes. Rotate before
    /// the helper so its complete batch fits the same command-buffer ceiling.
    pub fn reserve_passes(&mut self, count: usize) {
        assert!(count <= Self::PASSES_PER_SUBMISSION);
        #[cfg(test)]
        crate::test_support::PassCounts {total:count as u64,..Default::default()}.record();
        self.resource_work |= count != 0;
        self.compute = None;
        if self.passes + count > Self::PASSES_PER_SUBMISSION {
            let next = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("continued document submission"),
                });
            let full = std::mem::replace(self.current_mut(), next);
            self.earlier.push(full);
            self.passes = 0;
        }
        self.passes += count;
        self.total_passes += count as u64;
    }
    pub fn pass_count(&self) -> u64 { self.total_passes }
    pub fn has_work(&self)->bool {self.resource_work || self.total_passes!=0}
    pub fn retain_resources(&mut self) { self.resource_work = true; }
    pub fn can_fit_passes(&self, count: usize) -> bool {
        count <= Self::PASSES_PER_SUBMISSION - self.passes
    }
    pub fn begin_render_pass<'a>(
        &'a mut self,
        descriptor: &wgpu::RenderPassDescriptor<'_>,
    ) -> wgpu::RenderPass<'a> {
        self.begin_pass();
        self.current_mut().begin_render_pass(descriptor)
    }
    pub fn begin_compute_pass<'a>(
        &'a mut self,
        descriptor: &wgpu::ComputePassDescriptor<'_>,
    ) -> wgpu::ComputePass<'a> {
        self.begin_pass();
        self.current_mut().begin_compute_pass(descriptor)
    }
    /// A compute pass that consecutive dispatches share until any other pass,
    /// copy or command ends it.
    pub fn compute_pass(&mut self) -> &mut wgpu::ComputePass<'static> {
        if self.compute.is_none() {
            self.begin_pass();
            self.compute = Some(self.current_mut().begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("shared compute"),
                timestamp_writes: None,
            }).forget_lifetime());
        }
        self.compute.as_mut().unwrap()
    }
    pub fn end_compute_pass(&mut self) { self.compute = None; }
    /// RegionRecords that run before every other command of this encoder, such as
    /// uploads to storage that nothing earlier in it reads.
    pub fn prologue(&mut self) -> &mut wgpu::CommandEncoder {
        self.resource_work = true;
        let device = &self.device;
        self.prologue.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("submission prologue"),
        }))
    }
    /// Record bounded timestamp commands without an artwork dependency.
    pub fn diagnostics(&mut self) -> &mut wgpu::CommandEncoder {
        self.compute = None;
        self.current_mut()
    }
    /// Finish staging uploads before calling this. Their completion callbacks
    /// belong to the final encoder, after every earlier chunk on this queue.
    pub fn submit(mut self, queue: &wgpu::Queue) -> Option<wgpu::SubmissionIndex> {
        #[cfg(test)] crate::test_support::PassCounts {completion_callbacks:1,submissions:1,..Default::default()}.record();
        self.compute=None;
        let pending=self.submission.take();let receipt=pending.as_ref().map(|pending|pending.0.clone());
        let completions=self.completions.take();
        let completed=std::sync::Arc::new(std::sync::OnceLock::new());let stamp=completed.clone();
        let complete=move|| {let _=stamp.set(web_time::Instant::now());for completion in completions {completion();} if let Some(pending)=pending {pending.0.0.store(SubmissionState::Complete as u8,std::sync::atomic::Ordering::Release);}};
        #[cfg(not(target_arch="wasm32"))]
        let complete=self.poll.callback(&self.device,move|()|complete());
        #[cfg(target_arch="wasm32")]
        let complete=crate::pipeline_device::waking_completion(move|()|complete());
        self.current().on_submitted_work_done(move||complete(()));
        #[cfg(not(target_arch="wasm32"))]
        let poll=self.poll.clone();
        #[cfg(not(target_arch="wasm32"))]
        let device=self.device.clone();
        let work=|| {
            let mut chunks=self.earlier.into_iter().chain(self.current.into_inner());
            let mut index=submit_traced(queue,self.prologue.take().into_iter().chain(chunks.next()),!self.resource_work);
            for encoder in chunks {index=submit_traced(queue,[encoder],!self.resource_work);}
            self.latest.lock().unwrap().record(&index,completed,true);
            index
        };
        #[cfg(not(target_arch="wasm32"))]
        let index=poll.submit(work);
        #[cfg(target_arch="wasm32")]
        let index=Some(work());
        if index.is_some() && let Some(receipt)=receipt {let _=receipt.0.compare_exchange(SubmissionState::Encoding as u8,SubmissionState::Submitted as u8,std::sync::atomic::Ordering::AcqRel,std::sync::atomic::Ordering::Acquire);}
        #[cfg(not(target_arch="wasm32"))]
        poll.request(&device);
        index
    }
    pub async fn submit_and_wait(self,device:&crate::PipelineDevice,queue:&wgpu::Queue)->Result<(),String> {
        #[cfg(not(target_arch="wasm32"))]
        let (done,completed)=std::sync::mpsc::channel();
        #[cfg(target_arch="wasm32")]
        let (done,completed)=futures_channel::oneshot::channel();
        self.on_submitted_work_done(move|| {let _=done.send(Ok(()));});
        self.submit(queue);
        #[cfg(not(target_arch="wasm32"))]
        {crate::raster::wait_mapping(device,&completed)}
        #[cfg(target_arch="wasm32")]
        {let _=device;completed.await.map_err(|error|error.to_string())?}
    }
    pub fn on_submitted_work_done(&self,callback:impl FnOnce()+Send+'static) {self.completions.borrow_mut().push(Box::new(callback));}
    pub fn hold(&self,value:impl Send+'static) {self.on_submitted_work_done(move||drop(value));}
    pub fn append_encoder(&mut self,mut other:Self) {
        assert!(other.submission.is_none());
        self.compute=None;other.compute=None;
        self.earlier.extend(self.current.take());
        self.earlier.extend(other.prologue.take());
        self.earlier.append(&mut other.earlier);
        self.earlier.extend(other.current.take());
        self.completions.get_mut().append(other.completions.get_mut());
        self.passes=0;self.total_passes+=other.total_passes;self.resource_work|=other.resource_work;
    }
    pub fn retire(&self,value:impl Send+'static) {
        #[cfg(not(target_arch="wasm32"))]
        {let value=self.poll.handle(&self.device).retirement(value);self.on_submitted_work_done(move||value.complete());}
        #[cfg(target_arch="wasm32")]
        self.hold(value);
    }

}

// Copies, upload callbacks and query commands do not start render/compute
// passes. Forward them to the current chunk without changing their ordering;
// recording one ends the shared compute pass.
impl Deref for CommandEncoder {
    type Target = wgpu::CommandEncoder;
    fn deref(&self) -> &Self::Target {
        self.current()
    }
}
impl DerefMut for CommandEncoder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.resource_work = true;
        self.compute = None;
        self.current_mut()
    }
}

fn submit_traced(queue: &wgpu::Queue, encoders: impl IntoIterator<Item = wgpu::CommandEncoder>, presentation: bool) -> wgpu::SubmissionIndex {
    let mut trace = crate::performance_trace::Span::new(c"capy.command_finish");
    let commands = encoders.into_iter().map(wgpu::CommandEncoder::finish).collect::<Vec<_>>();
    trace.next(c"capy.queue_submit");
    if presentation { queue.submit_for_present(commands) } else { queue.submit(commands) }
}

pub(crate) trait ColorPass {
    fn color_pass(&mut self, label: &str, view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) -> wgpu::RenderPass<'_>;
}
impl ColorPass for wgpu::CommandEncoder {
    fn color_pass(&mut self, label: &str, view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) -> wgpu::RenderPass<'_> {
        self.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        })
    }
}
impl ColorPass for CommandEncoder {
    fn color_pass(&mut self, label: &str, view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) -> wgpu::RenderPass<'_> {
        self.begin_pass();
        self.current_mut().color_pass(label, view, load)
    }
}

#[cfg(test)]
#[path = "resident_tests.rs"]
mod resident_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use crate::test_support::{dab_batch, packet};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    #[test]
    fn a_submission_recorded_after_a_newer_one_does_not_replace_it() {
        let r=renderer();
        let (older,newer)=(r.queue.submit([]),r.queue.submit([]));
        let (early,late)=(Arc::new(std::sync::OnceLock::new()),Arc::new(std::sync::OnceLock::new()));
        let mut latest=LatestSubmission::default();
        latest.record(&newer,late.clone(),true);latest.record(&older,early,false);
        let (completed,observe)=latest.completed();
        assert!(Arc::ptr_eq(&completed,&late) && !observe,"the latest submission went back to an older one recorded after it");
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_submitted_receipt_is_discarded_when_its_unfired_completion_is_retired() {
        let r=renderer();
        let receipt=Submission(Arc::new(std::sync::atomic::AtomicU8::new(SubmissionState::Submitted as u8)));
        let pending=PendingSubmission(receipt.clone());
        let callback=r.device.callback(move|()| {
            let completed=pending;
            completed.0.0.store(SubmissionState::Complete as u8,Ordering::Release);
        });
        assert!(receipt.submitted() && receipt.pending());
        r.renderer_factory().retire_device_services().unwrap();
        crate::finish_shader_compiler_shutdown();
        assert!(receipt.discarded(),"retirement must abandon an unfired submitted completion");
        assert!(!receipt.pending());
        drop(callback);
    }
    #[test]
    fn submission_acknowledges_only_its_own_encoder() {
        let r=renderer();let mut held=encoder(&r);let owner=held.submission();
        assert!(!owner.submitted());assert!(!owner.discarded());
        encoder(&r).submit(&r.queue);
        assert!(!owner.submitted());assert!(!owner.discarded());
        held.submit(&r.queue);
        assert!(owner.submitted());assert!(!owner.discarded());
    }
    #[test]
    fn dropped_encoder_discards_its_submission() {
        let r=renderer();let mut held=encoder(&r);let owner=held.submission();drop(held);
        assert!(owner.discarded());assert!(!owner.submitted());
    }
    #[test]
    fn submission_completes_while_the_host_waits() {
        let mut r=renderer();
        let mut commands=encoder(&r);
        let receipt=commands.submission();
        let (send,completed)=std::sync::mpsc::channel();
        commands.on_submitted_work_done(move|| {let _=send.send(());});
        r.last_submission=commands.submit(&r.queue);
        let completion=completed.recv_timeout(std::time::Duration::from_secs(10));
        r.wait_idle().unwrap();
        assert!(completion.is_ok(),"GPU completion required another host poll: {completion:?}");
        assert!(!receipt.pending());
        assert!(receipt.submitted());
    }
    #[test]
    fn a_stroke_at_fit_over_a_photo_its_settle_and_a_capture_register_at_most_one_completion_per_submission() {
        fn counted(run:impl FnOnce())->crate::test_support::PassCounts {let before=crate::test_support::PassCounts::snapshot();run();crate::test_support::PassCounts::snapshot().delta(before)}
        let extent=[4096,2048];
        let photo=layer_core::color::source::rgba8_source(extent,|x,y|[(x/7) as u8,(y/5) as u8,((x^y)%199) as u8,255]);
        let mut doc=layer_color::photo_project(Arc::unwrap_or_clone(photo),Default::default(),"Photo".into(),layer_core::color::SampleDepth::U8).unwrap();
        let (ink,target)=crate::test_support::add_paint(&mut doc.artwork,"Ink",extent);
        let stack=doc.artwork.compositions.get(doc.artwork.root).unwrap().result;
        let entries=&mut doc.artwork.stacks.get_mut(stack).unwrap().entries;entries.retain(|entry|*entry!=ink);entries.insert(0,ink);
        let mut doc=layer_core::Document::from_artwork(doc.artwork).unwrap();
        doc.working.occurrence=Some(ink);doc.working.target=Some(target);
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        layer_render::CanvasRenderer::set_working_layer(&mut r,Some(ink));
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
        frame.view=crate::test_support::view([800,400]);frame.view.document_to_surface=[0.19,0.,0.,0.19,11.,6.];
        crate::test_support::submit_settled(&mut r,frame);
        crate::test_support::evict_sources(&mut r);
        let style=crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
        let stroke=counted(|| for step in 0..8 {
            let mut dab=crate::tests::test_dab([600.+400.*step as f32,1000.],[0.8,0.2,0.1,1.],1.);dab.radii=[120.;2];
            let batch=layer_render::DabBatch {stroke_start:step==0,stroke_end:step==7,..dab_batch(target,style.clone(),dab.bounds())};
            crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame}).unwrap();
        });
        let settle=counted(||crate::test_support::submit_settled(&mut r,frame));
        let mut snapshot=None;
        let capture=counted(|| {
            let mut capture=r.snapshot_gpu().capture_scene(doc.snapshot(),layer_core::SceneScope::All,Default::default()).unwrap();
            capture.read_region([0,0,1024,1024]).unwrap();snapshot=Some(capture);
        });
        let phases=[("stroke",stroke),("settle",settle),("capture",capture)].map(|(phase,counts)|(phase,counts.completion_callbacks,counts.submissions));
        assert!(phases.iter().all(|(_,callbacks,submissions)|*submissions!=0 && callbacks<=submissions),
            "each phase registers at most one GPU completion callback per submission (phase, callbacks, submissions): {phases:?}");
        drop(snapshot);
    }
    #[test]
    fn bare_submit_and_destroy_do_not_wait_on_the_poll_owner() {
        let r=renderer();
        let (outstanding,duration,_raw)=crate::test_support::bounded_outstanding_gpu_work(&r);
        let victim=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:4,
            usage:wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let upload_buffer=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:4,
            usage:wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let mut upload=r.device.create_command_encoder(&Default::default());
        upload.clear_buffer(&upload_buffer,0,None);
        outstanding.submit(&r.queue);
        let start=std::time::Instant::now();
        victim.destroy();
        let destroy=start.elapsed();
        let start=std::time::Instant::now();
        let _submitted=r.queue.submit([upload.finish()]);
        let submit=start.elapsed();
        r.device.request_poll();
        let actual=duration();
        assert!((40. ..=75.).contains(&actual),"actual bounded GPU duration {actual} ms");
        assert!(destroy<=std::time::Duration::from_millis(1),"destroy waited: {destroy:?}");
        assert!(submit<=std::time::Duration::from_millis(1),"bare submit waited: {submit:?}");
    }
    #[test]
    fn a_memory_pass_without_work_creates_no_command_encoder() {
        let mut r=renderer();
        let before=crate::test_support::PassCounts::snapshot();
        drop(CommandEncoder::new(&r.device,&Default::default()));
        r.memory.invalidate_limits();r.refresh_memory_limits();
        let counts=crate::test_support::PassCounts::snapshot().delta(before);
        assert_eq!((counts.grant_encoders,counts.command_encoders),(1,0),"an encoder that records nothing created a GPU command encoder");
    }
    fn renderer() -> WgpuRasterizer {
        let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(r.adapter.get_info().device_type, wgpu::DeviceType::Cpu);
        r
    }
    fn encoder(r: &WgpuRasterizer) -> CommandEncoder {
        CommandEncoder::new(&r.device, &Default::default())
    }
    fn document(size: [u32; 2]) -> layer_core::Document {
        layer_core::Document::new(layer_core::authored::PortableId::random(), size[0], size[1],
            layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() })
    }
    fn bytes(r: &WgpuRasterizer, buffer: &wgpu::Buffer) -> Vec<u8> {
        pollster::block_on(crate::local_tone::read_buffer_async(&r.device, &r.queue, buffer)).unwrap()
    }
    fn finish(r: &mut WgpuRasterizer, e: CommandEncoder) {
        r.uploads.finish(&e);
        e.submit(&r.queue);
        crate::test_support::complete(r);
    }
    fn copyable_styles(r: &mut WgpuRasterizer) {
        r.style_buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("style upload readback fixture"), size: r.style_buffer.size(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        r.style_upload_valid.store(false, Ordering::Release);
    }

    #[test]
    fn zero_pass_copies_clears_and_raw_access_remain_resource_work() {
        let mut r = renderer();
        let source = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: None, size: 16, usage: wgpu::BufferUsages::COPY_SRC, mapped_at_creation: true,
        });
        source.get_mapped_range_mut(..).unwrap().copy_from_slice(&[7; 16]);
        source.unmap();
        let output = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: None, size: 16, usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut e = encoder(&r);
        e.copy_buffer_to_buffer(&source, 0, &output, 0, 16);
        assert_eq!(e.pass_count(), 0);
        assert!(e.resource_work);
        finish(&mut r, e);
        assert_eq!(bytes(&r, &output), vec![7; 16]);
        let mut e = encoder(&r);
        e.clear_buffer(&output, 0, None);
        assert_eq!(e.pass_count(), 0);
        assert!(e.resource_work);
        finish(&mut r, e);
        assert_eq!(bytes(&r, &output), vec![0; 16]);
        let mut e = encoder(&r);
        let _: &mut wgpu::CommandEncoder = &mut e;
        assert!(e.resource_work);
        let mut e = encoder(&r);
        e.prologue().insert_debug_marker("raw prologue");
        assert!(e.resource_work);
    }

    #[test]
    fn diagnostics_and_callbacks_submit_without_resource_work() {
        let r = renderer();
        assert!(r.device.features().contains(wgpu::Features::TIMESTAMP_QUERY));
        let mut telemetry = crate::telemetry::Telemetry::new(&r.device, &r.queue);
        telemetry.enabled = true;
        let complete = Arc::new(AtomicBool::new(false));
        let observed = complete.clone();
        let mut e = encoder(&r);
        e.reserve_passes(0);
        telemetry.begin(&r.device, &r.queue, e.diagnostics(), 1);
        telemetry.end(e.diagnostics());
        e.on_submitted_work_done(move || observed.store(true, Ordering::Release));
        assert_eq!(e.pass_count(), 0);
        assert!(!e.resource_work);
        e.submit(&r.queue);
        telemetry.submitted(&r.queue);
        crate::test_support::complete(&r);
        assert!(complete.load(Ordering::Acquire));
        assert_eq!(telemetry.completed_snapshot(&r.device, &r.queue).gpu.count, 1);
    }

    #[test]
    fn style_upload_cache_preserves_gpu_bytes_and_invalidates_discarded_or_resized_storage() {
        let mut r = renderer();
        copyable_styles(&mut r);
        let doc = document([64; 2]);
        let first = packet(doc.scene(), [64; 2]);
        let mut e = encoder(&r);
        r.prepare_uploads(first, &mut [], &mut e).unwrap();
        assert!(e.resource_work);
        assert!(r.style_upload_valid.load(Ordering::Acquire));
        drop(e);
        assert!(!r.style_upload_valid.load(Ordering::Acquire));
        let mut e = encoder(&r);
        r.prepare_uploads(first, &mut [], &mut e).unwrap();
        assert!(e.resource_work);
        finish(&mut r, e);
        let expected = r.style_upload.clone();
        assert_eq!(&bytes(&r, &r.style_buffer)[..expected.len()], expected);
        let mut e = encoder(&r);
        r.prepare_uploads(first, &mut [], &mut e).unwrap();
        assert!(!e.resource_work, "identical style records uploaded again");
        finish(&mut r, e);
        let changed = FramePacket { document_extent: [96, 64], ..first };
        let mut e = encoder(&r);
        r.prepare_uploads(changed, &mut [], &mut e).unwrap();
        assert!(e.resource_work);
        drop(e);
        assert!(!r.style_upload_valid.load(Ordering::Acquire));
        let mut e = encoder(&r);
        r.prepare_uploads(changed, &mut [], &mut e).unwrap();
        assert!(e.resource_work, "discarded optimistic style bytes were reused");
        finish(&mut r, e);
        let expected = r.style_upload.clone();
        assert_eq!(&bytes(&r, &r.style_buffer)[..expected.len()], expected);
        let old = r.style_buffer.clone();
        r.ensure_upload_capacity(0, r.style_capacity + 1).unwrap();
        assert_ne!(old, r.style_buffer);
        assert!(!r.style_upload_valid.load(Ordering::Acquire));
        let mut e = encoder(&r);
        r.prepare_uploads(changed, &mut [], &mut e).unwrap();
        assert!(e.resource_work, "new GPU storage reused the old table's CPU key");
        finish(&mut r, e);
        let mut e = encoder(&r);
        r.prepare_uploads(changed, &mut [], &mut e).unwrap();
        assert!(!e.resource_work);
    }

    #[test]
    fn cached_pan_and_rotation_reuse_style_uploads_and_changed_cells_until_brush_input() {
        let mut r = renderer();
        let mut doc = document([1024; 2]);
        let paint_owner = doc.scene().order()[0];
        let target = doc.scene().source_target(paint_owner).unwrap();
        let SourceTarget::Paint(paint) = target else { unreachable!() };
        let source = crate::test_support::depth_source([1024; 2], layer_core::color::SampleDepth::U8,
            layer_core::color::RgbSpace::Srgb, 4 * 1024 * 1024, |_, _| [0.2, 0.3, 0.4, 1.]);
        doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(source.into()));
        let filter = crate::tests::native_effects::insert_effect(&mut doc,
            layer_core::EffectInstance::new(crate::tests::fixture("brightness_to_opacity").program()));
        doc.artwork.occurrences.get_mut(filter).unwrap().attachment = layer_core::authored::Attachment::Effect;
        doc.artwork.stacks.get_mut(doc.composition().result).unwrap().entries = vec![filter, paint_owner];
        crate::tests::native_effects::refresh(&mut doc);
        let mut frame = packet(doc.scene(), [1024; 2]);
        frame.composite_all = false;
        frame.view = crate::test_support::view([256; 2]);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        crate::test_support::submit_settled(&mut r, frame);
        let mut dab = crate::tests::test_dab([128., 128.], [0.8, 0.1, 0.2, 1.], 1.);
        dab.radii = [32.; 2];
        let batch = dab_batch(target, crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen), dab.bounds());
        crate::test_support::submit_settled(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame });
        let flags = r.changed_cells.as_ref().unwrap().buffer(target, [0, 0]).expect("paint flags must be resident").clone();
        let sentinel = vec![0x55; flags.size() as usize - 16];
        r.queue.write_buffer(&flags, 16, &sentinel);
        r.queue.submit([]);
        r.wait_idle().unwrap();
        let passes = r.metrics.command_passes;
        let composite = r.composite_revision;
        let navigator = r.navigator.revision;
        let styles = r.style_upload.clone();
        for angle in [0.0f32, 0.03, -0.04, 0.0] {
            frame.time_seconds += 0.01;
            let (s, c) = angle.sin_cos();
            frame.view.document_to_surface = [0.25 * c, 0.25 * s, -0.25 * s, 0.25 * c, 4., -3.];
            r.submit(frame).unwrap();
            r.wait_idle().unwrap();
            assert_eq!(r.metrics.command_passes, passes);
            assert_eq!(r.composite_revision, composite);
            assert_eq!(r.navigator.revision, navigator);
            assert_eq!(r.style_upload, styles);
            assert_eq!(&bytes(&r, &flags)[16..], sentinel);
            let mut e = encoder(&r);
            r.prepare_uploads(frame, &mut [], &mut e).unwrap();
            let mut cells = r.changed_cells.take().unwrap();
            cells.prepare(&r, frame, &[], true, &mut e);
            r.changed_cells = Some(cells);
            assert!(!e.resource_work, "cached camera update recorded resource commands");
        }
        let mut e = encoder(&r);
        let mut cells = r.changed_cells.take().unwrap();
        cells.prepare(&r, FramePacket { dab_batches: std::slice::from_ref(&batch), dabs: std::slice::from_ref(&dab), ..frame },
            &[vec![]], true, &mut e);
        r.changed_cells = Some(cells);
        assert!(e.resource_work, "new brush input did not reset changed-cell masks");
        finish(&mut r, e);
        assert!(bytes(&r, &flags)[16..].iter().all(|v| *v == 0));
    }

    #[test]
    fn a_frame_dropped_before_submission_publishes_no_composite_damage_and_its_retry_is_presented() {
        use crate::layer_tests::placement::{paint_document,set_source,occurrence_id};
        let extent=[128;2];
        let mut doc=paint_document(extent,"Dropped publication");
        set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|_,_|[255,0,0,255]));
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        crate::test_support::submit_settled(&mut r,FramePacket {composite_all:false,..packet(doc.scene(),extent)});
        let (view,format)=(crate::test_support::view(extent),wgpu::TextureFormat::Rgba8Unorm);
        let (shown,surface)=crate::create_target(r.device(),extent,format,"retained publication");
        let mut retained=ViewportPresenter::for_renderer(&r,format);
        retained.set_target_retention(true);
        pollster::block_on(retained.prepare(&r)).unwrap();
        retained.present(&r,&surface,view,[0.;4]).unwrap();
        let before=crate::layer_tests::page_bytes(&r,&shown);
        let handle=occurrence_id(&doc);
        let mut hidden=doc.scene().occurrence(handle).unwrap().clone();
        hidden.visible=false;
        doc.apply(layer_core::Edit::Occurrence(layer_core::authored::RecordChange::replace(&doc.artwork.occurrences,handle,Some(hidden)).unwrap())).unwrap();
        let edit=FramePacket {composite_all:false,time_seconds:0.01,..packet(doc.scene(),extent)};
        let revision=r.composite_revision;
        r.test.discard_frame=true;
        crate::test_support::submit_ready(&mut r,edit).unwrap();
        assert!(!r.test.discard_frame,"the edit frame's encoder is dropped");
        assert_eq!((r.composite_revision,r.composite_damage_since(revision)),(revision,Some(crate::pixel_rect::PixelRect::EMPTY)),
            "a frame that was never submitted published composite damage");
        retained.set_cursor(r.device(),&[layer_render::CursorSegment {from:[4.,4.],to:[12.,12.],distance:0.,marker:2.,scale:1.}],1.);
        retained.present(&r,&surface,view,[0.;4]).unwrap();
        retained.set_cursor(r.device(),&[],1.);
        crate::test_support::submit_settled(&mut r,FramePacket {time_seconds:0.02,..edit});
        retained.present(&r,&surface,view,[0.;4]).unwrap();
        let (fresh,fresh_surface)=crate::create_target(r.device(),extent,format,"fresh publication");
        let mut reference=ViewportPresenter::for_renderer(&r,format);
        pollster::block_on(reference.prepare(&r)).unwrap();
        reference.present(&r,&fresh_surface,view,[0.;4]).unwrap();
        let expected=crate::layer_tests::page_bytes(&r,&fresh);
        assert_ne!(expected,before,"the fixture's edit must change the picture");
        assert_eq!(crate::layer_tests::page_bytes(&r,&shown),expected,"the retained target lost the retried edit");
    }

    #[test]
    fn discarded_backdrop_uniform_upload_is_reencoded_before_rendering() {
        let mut r = renderer();
        let doc = document([64, 128]);
        let view = crate::test_support::view([128; 2]);
        crate::test_support::submit_settled(&mut r, FramePacket { view, ..packet(doc.scene(), [64, 128]) });
        let surface = crate::create_target(r.device(), [128; 2], wgpu::TextureFormat::Rgba8Unorm, "discarded backdrop fixture").0;
        let target = surface.create_view(&Default::default());
        let mut retained = ViewportPresenter::for_renderer(&r, wgpu::TextureFormat::Rgba8Unorm);
        let mut reference = ViewportPresenter::for_renderer(&r, wgpu::TextureFormat::Rgba8Unorm);
        let region = BackdropRegion { bounds: [32., 16., 64., 96.], radii: [12.; 4], shape: BackdropRegion::SQUIRCLE };
        let surround = [0., 0., 0., 1.];
        for presenter in [&mut retained, &mut reference] {
            presenter.set_backdrop(&r, &[region], Default::default(), false);
            pollster::block_on(presenter.prepare(&r)).unwrap();
            presenter.present(&r, &target, view, surround).unwrap();
        }
        let before = crate::layer_tests::page_bytes(&r, &surface);
        retained.set_backdrop(&r, &[region.with_strength(0.25)], Default::default(), false);
        let mut discarded = r.device().create_command_encoder(&Default::default());
        retained.encode(&r, &mut discarded, &target, view, surround).unwrap();
        drop(discarded);
        retained.present(&r, &target, view, surround).unwrap();
        let recovered = crate::layer_tests::page_bytes(&r, &surface);
        reference.set_backdrop(&r, &[region.with_strength(0.25)], Default::default(), false);
        reference.present(&r, &target, view, surround).unwrap();
        let expected = crate::layer_tests::page_bytes(&r, &surface);
        assert_ne!(expected, before, "fixture must change visible glass strength");
        assert_eq!(recovered, expected, "discarded uniform upload poisoned the backdrop cache");
    }
}
