//! Optional GPU queue spans for native presenters. Three nonblocking slots;
//! Queue spans bracket submitted work; encoded spans stay in the drawing submission.
//! This is neither CPU work time nor drawable presentation/input latency.
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct GpuFrameSample {
    pub frame: u64,
    pub elapsed_ns: u64,
    /// 1: valid, 2: map/read failure, 3: invalid timestamp order/range.
    pub status: u64,
    /// Raw device-clock endpoints; native hosts can calibrate them to CPU time.
    pub start_tick: u64,
    pub end_tick: u64,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct GpuFrameTimingStats {
    /// 0: not initialized, 1: supported, 2: unavailable on this device.
    pub support: u64,
    pub requested: u64,
    pub skipped: u64,
    pub invalid: u64,
    pub pending: u64,
}

// Metal can omit timestamp writes for empty passes. Keep a tiny storage write
// in each marker pass. Created only when telemetry is enabled; its GPU overhead
// is part of the measured span, and must be calibrated by the caller.
pub(crate) enum TimestampMarker {
    Encoder,
    Pass { pipeline: crate::Deferred<wgpu::ComputePipeline>, bind_group: wgpu::BindGroup, _scope: crate::compile::Scope },
}
impl TimestampMarker {
    pub fn new(device: &crate::PipelineDevice, _invalid: Arc<AtomicU64>) -> Self {
        if device.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
            return Self::Encoder;
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("timestamp marker"),
            source: wgpu::ShaderSource::Wgsl("@group(0) @binding(0) var<storage, read_write> marker: u32; @compute @workgroup_size(1) fn main() { marker = marker + 1u; }".into()),
        });
        let layout = crate::bindings::layout(device, "timestamp marker", &[crate::bindings::buffer(
            0, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Storage { read_only: false }, false,
            std::num::NonZeroU64::new(4),
        )]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("timestamp marker"), bind_group_layouts: &[Some(&layout)], immediate_size: 0,
        });
        let pipeline = crate::Deferred::compute_module(device,
            "timestamp marker", &pipeline_layout, &module, "main");
        let scope=device.compilation().expect("Timer compilation coordinator");
        scope.require([&pipeline],crate::compile::Class::Validation);
        #[cfg(target_arch="wasm32")]
        {
            let (pipeline,scope)=(pipeline.clone(),scope.clone());
            wasm_bindgen_futures::spawn_local(async move {
                if scope.prepare_all([&pipeline]).await.is_err() { _invalid.fetch_add(1,Ordering::Relaxed); }
            });
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp marker"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let bind_group = crate::bindings::group(device, "timestamp marker", &layout, [
            buffer.as_entire_binding(),
        ]);
        Self::Pass { pipeline, bind_group, _scope: scope }
    }
    #[cfg(test)]
    pub async fn prepare(&self)->Result<(),String> {
        if let Self::Pass {pipeline,_scope:scope,..}=self {scope.prepare_all([pipeline]).await?;}
        Ok(())
    }
    fn ready(&self) -> bool {
        match self { Self::Encoder => true, Self::Pass { pipeline, .. } => pipeline.ready() }
    }
    fn query_count(&self) -> u32 {
        match self { Self::Encoder => 2, Self::Pass { .. } => 4 }
    }
    pub fn write(&self, encoder: &mut wgpu::CommandEncoder, query: &wgpu::QuerySet, index: u32) {
        let Self::Pass { pipeline, bind_group, .. } = self else {
            encoder.write_timestamp(query, index);
            return;
        };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("timestamp marker"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: query,
                beginning_of_pass_write_index: Some(index * 2),
                end_of_pass_write_index: Some(index * 2 + 1),
            }),
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
}

struct Slot {
    query: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    busy: Arc<AtomicBool>,
    gpu_done: Arc<AtomicBool>,
    awaiting_resolve: Option<(u64, u32)>,
}

pub struct GpuFrameTimer {
    device:crate::PipelineDevice,
    slots: Vec<Slot>,
    active: Option<(usize, u64, u32)>,
    query_count: u32,
    period: f64,
    marker: Option<TimestampMarker>,
    ready: Arc<Mutex<VecDeque<GpuFrameSample>>>,
    invalid: Arc<AtomicU64>,
    omitted: Arc<AtomicU64>,
    requested: u64,
    skipped: u64,
}
impl GpuFrameTimer {
    #[cfg(test)]
    pub(crate) async fn prepare(&self)->Result<(),String> {
        if let Some(marker)=&self.marker {marker.prepare().await?;}
        Ok(())
    }
    pub fn new(renderer: &crate::WgpuRasterizer) -> Self {
        Self::with_device(&renderer.device,&renderer.queue)
    }
    pub(crate) fn with_device(device:&crate::PipelineDevice,queue:&wgpu::Queue)->Self {
        let period = if device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            f64::from(queue.get_timestamp_period())
        } else {
            0.
        };
        let invalid = Arc::default();
        let marker = (period > 0.).then(|| TimestampMarker::new(device, Arc::clone(&invalid)));
        let query_count = marker.as_ref().map_or(0, TimestampMarker::query_count);
        let slots = if period > 0. {
            (0..3)
                .map(|_| Slot {
                    query: device.create_query_set(&wgpu::QuerySetDescriptor {
                        label: Some("frame GPU timestamps"),
                        ty: wgpu::QueryType::Timestamp,
                        count: query_count,
                    }),
                    resolve: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("frame timestamp resolve"),
                        size: 256,
                        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    }),
                    read: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("frame timestamp readback"),
                        size: u64::from(query_count) * 8,
                        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    busy: Arc::new(AtomicBool::new(false)),
                    gpu_done: Arc::new(AtomicBool::new(false)),
                    awaiting_resolve: None,
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            device:device.for_resource(),
            slots,
            active: None,
            query_count,
            period,
            marker,
            ready: Arc::new(Mutex::new(VecDeque::with_capacity(256))),
            invalid,
            omitted: Arc::default(),
            requested: 0,
            skipped: 0,
        }
    }
    /// Encode markers in an existing drawing submission. CPU encoding time is
    /// outside this span, and timing adds no start/end queue submissions.
    /// Pair a successful begin with end_encoded and submitted.
    pub(crate) fn begin_encoded(&mut self, encoder: &mut wgpu::CommandEncoder, frame: u64) -> bool {
        if !self.reserve(frame, self.query_count.saturating_sub(1)) {
            return false;
        }
        let (index, _, _) = self.active.unwrap();
        crate::submission::CacheWrite::shared(self.slots[index].busy.clone()).track_raw(&self.device,encoder);
        self.marker.as_ref().unwrap().write(encoder, &self.slots[index].query, 0);
        true
    }

    /// Optional pass-only profiling, without extra marker passes. The caller
    /// must notify `submitted` after submitting the containing command buffer.
    pub(crate) fn begin_render_pass(&mut self, frame: u64) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if !self.reserve(frame, 1) {
            return None;
        }
        let (index, _, _) = self.active.unwrap();
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.slots[index].query,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        })
    }

    fn reserve(&mut self, frame: u64, end_index: u32) -> bool {
        self.requested += 1;
        if self.active.is_some_and(|(index,_,_)|!self.slots[index].busy.load(Ordering::Acquire)) {self.active=None;}
        let slot = if self.active.is_none() && self.marker.as_ref().is_some_and(TimestampMarker::ready) {
            self.slots
                .iter()
                .enumerate()
                .find(|(_, slot)| !slot.busy.load(Ordering::Acquire))
        } else {
            None
        };
        let Some((index, slot)) = slot else {
            self.skipped += 1;
            return false;
        };
        slot.busy.store(true, Ordering::Release);
        slot.gpu_done.store(false, Ordering::Release);
        self.active = Some((index, frame, end_index));
        true
    }
    pub(crate) fn end_encoded(&self, encoder: &mut wgpu::CommandEncoder) {
        if let Some((index, _, _)) = self.active {
            self.marker
                .as_ref()
                .unwrap()
                .write(encoder, &self.slots[index].query, 1);
        }
    }
    pub(crate) fn submitted(&mut self, queue: &wgpu::Queue) {
        let Some((index, frame, end_index)) = self.active.take() else {
            return;
        };
        let slot = &mut self.slots[index];
        let done = slot.gpu_done.clone();
        self.device.on_queue_done(queue,move || done.store(true, Ordering::Release));
        slot.awaiting_resolve = Some((frame, end_index));
    }
    /// Optional host queue spans also include gaps between drawing/presentation
    /// submissions. Saturation drops the observation rather than waiting.
    pub fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: u64) -> bool {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame GPU start"),
        });
        if !self.begin_encoded(&mut encoder, frame) {
            return false;
        }
        if self.device.submit_for_present(queue,[encoder.finish()]).is_none() {self.active=None;return false;}
        true
    }
    pub fn end(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.active.is_none() {
            return;
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame GPU end"),
        });
        self.end_encoded(&mut encoder);
        if self.device.submit_for_present(queue,[encoder.finish()]).is_none() {self.active=None;return;}
        self.submitted(queue);
    }
    /// Resolve only after the marker submission has completed. On Metal,
    /// resolving counters in the marker's command buffer returned stale values.
    /// This state machine never waits and retains the slot until mapped/drained.
    pub fn poll(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if !self.slots.iter().any(|slot|slot.awaiting_resolve.is_some() && slot.gpu_done.load(Ordering::Acquire)) {return;}
        let mut encoder=device.create_command_encoder(&wgpu::CommandEncoderDescriptor {label:Some("completed frame timestamps")});
        self.resolve_completed(&mut encoder);self.device.submit_for_present(queue,[encoder.finish()]);
    }
    pub(crate) fn resolve_completed(&mut self,encoder:&mut wgpu::CommandEncoder) {
        for slot in &mut self.slots {
            if slot.awaiting_resolve.is_none() || !slot.gpu_done.load(Ordering::Acquire) {continue;}
            let (frame,end_index)=slot.awaiting_resolve.take().unwrap();
            slot.gpu_done.store(false,Ordering::Release);
            encoder.resolve_query_set(&slot.query,0..end_index+1,&slot.resolve,0);
            encoder.copy_buffer_to_buffer(&slot.resolve,0,&slot.read,0,u64::from(end_index+1)*8);
            crate::submission::CacheWrite::shared(slot.busy.clone()).track_raw(&self.device,encoder);
            let busy=slot.busy.clone();let ready=self.ready.clone();let invalid=self.invalid.clone();let omitted=self.omitted.clone();let period=self.period;
            crate::raster::map_then(&self.device,Some(encoder),&slot.read,u64::from(end_index+1)*8,move |bytes| {
                let start=u64::from_le_bytes(bytes[..8].try_into().unwrap());let offset=end_index as usize*8;
                let end=u64::from_le_bytes(bytes[offset..offset+8].try_into().unwrap());let elapsed=end.saturating_sub(start) as f64*period;
                let mut sample=GpuFrameSample {frame,status:3,..Default::default()};
                if start>0 && end>start && elapsed.is_finite() && elapsed<u64::MAX as f64 {
                    sample.elapsed_ns=elapsed.round() as u64;sample.start_tick=start;sample.end_tick=end;sample.status=1;
                }
                Ok(sample)
            },move |result| {
                if matches!(result,Err(crate::GpuRasterError::WaitFailed(_))) {busy.store(false,Ordering::Release);return;}
                let sample=result.unwrap_or(GpuFrameSample {frame,status:2,..Default::default()});
                if sample.status!=1 {invalid.fetch_add(1,Ordering::Relaxed);}
                if let Ok(mut ready)=ready.try_lock() {
                    if ready.len()<256 {ready.push_back(sample);}else{omitted.fetch_add(1,Ordering::Relaxed);}
                }else{omitted.fetch_add(1,Ordering::Relaxed);}
                busy.store(false,Ordering::Release);
            });
        }
    }
    #[cfg(test)]
    pub(crate) fn completed(&self)->usize {
        self.slots.iter().filter(|slot|slot.awaiting_resolve.is_some() && slot.gpu_done.load(Ordering::Acquire)).count()
    }
    pub fn take_into(&self, output: &mut [GpuFrameSample]) -> usize {
        let Ok(mut ready) = self.ready.try_lock() else {
            return 0;
        };
        let count = output.len().min(ready.len());
        for sample in &mut output[..count] {
            *sample = ready.pop_front().unwrap();
        }
        count
    }
    pub fn stats(&self) -> GpuFrameTimingStats {
        GpuFrameTimingStats {
            support: if self.period > 0. { 1 } else { 2 },
            requested: self.requested,
            skipped: self.skipped + self.omitted.load(Ordering::Relaxed),
            invalid: self.invalid.load(Ordering::Relaxed),
            pending: self
                .slots
                .iter()
                .filter(|s| s.busy.load(Ordering::Acquire))
                .count() as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_markers_and_render_passes_reuse_the_same_slot() {
        let instance = crate::WgpuRasterizer::headless_instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        })).unwrap();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("timestamp regression triangle"),
            source: wgpu::ShaderSource::Wgsl("@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32> { let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.)); return vec4(p[i],0.,1.); } @fragment fn fs()->@location(0) vec4<f32> { return vec4(1.,0.,0.,1.); }".into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("timestamp regression triangle"), layout: None,
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader, entry_point: Some("fs"), compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
            multiview_mask: None, cache: None,
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("timestamp regression target"),
            size: wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let device=crate::PipelineDevice::from(device);device.bind_queue(&queue);
        #[cfg(not(target_arch="wasm32"))]
        device.initialize_poll_owner().unwrap();
        let owner=device.compilation().unwrap();
        owner.idle(false,false);
        let mut timer = GpuFrameTimer::with_device(&device, &queue);
        let Some(TimestampMarker::Pass {_scope:scope,..})=&timer.marker else {panic!("pass timestamps without encoder timestamps")};
        assert!(!scope.is_idle(),"the timer shares the active drawing's coordinator");
        owner.idle(true,true);
        assert!(scope.is_idle(),"a passive timer does not hold optional compilation");
        owner.idle(false,false);
        assert!(!scope.is_idle());
        pollster::block_on(timer.prepare()).unwrap();
        let mut previous_end = 0;
        for frame in 1..=12 {
            let mut encoder = device.create_command_encoder(&Default::default());
            let encoded = frame % 2 == 1;
            if encoded { assert!(timer.begin_encoded(&mut encoder, frame)); }
            {
                let timestamps = if encoded { None } else { Some(timer.begin_render_pass(frame).unwrap()) };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("timestamp regression draw"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view, depth_slice: None, resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None, timestamp_writes: timestamps,
                    occlusion_query_set: None, multiview_mask: None,
                });
                pass.set_pipeline(&pipeline);
                pass.draw(0..3, 0..1);
            }
            if encoded { timer.end_encoded(&mut encoder); }
            queue.submit_for_present([encoder.finish()]);
            timer.submitted(&queue);
            device.wait_queue().unwrap();
            timer.poll(&device, &queue);
            device.wait_queue().unwrap();
            let mut samples = [GpuFrameSample::default(); 2];
            assert_eq!(timer.take_into(&mut samples), 1);
            let sample = samples[0];
            assert_eq!(sample.frame, frame);
            assert_eq!(sample.status, 1, "{sample:?}");
            assert!(sample.elapsed_ns > 0 && sample.end_tick > sample.start_tick);
            assert!(sample.start_tick >= previous_end && sample.start_tick > 0, "Stale timestamps after slot reuse: {sample:?}");
            previous_end = sample.end_tick;
            assert_eq!(timer.stats().pending, 0);
            assert_eq!(timer.stats().invalid, 0);
        }
        assert_eq!(timer.stats().requested, 12);
        assert_eq!(timer.stats().skipped, 0);
    }

    #[test]
    fn encoded_span_excludes_cpu_delay_before_the_drawing_is_submitted() {
        let instance = crate::WgpuRasterizer::headless_instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        for extra in [wgpu::Features::empty(), adapter.features() & wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS] {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY | extra,
            ..Default::default()
        }))
        .unwrap();
        let device=crate::PipelineDevice::from(device);device.bind_queue(&queue);
        #[cfg(not(target_arch="wasm32"))]
        device.initialize_poll_owner().unwrap();
        let mut timer = GpuFrameTimer::with_device(&device, &queue);
        pollster::block_on(timer.prepare()).unwrap();
        let mut encoder = device.create_command_encoder(&Default::default());
        assert!(timer.begin_encoded(&mut encoder, 42));
        // This deliberate CPU stall must not become a reported GPU spike.
        std::thread::sleep(std::time::Duration::from_millis(200));
        timer.end_encoded(&mut encoder);
        queue.submit_for_present([encoder.finish()]);
        timer.submitted(&queue);
        device.wait_queue().unwrap();
        timer.poll(&device, &queue);
        device.wait_queue().unwrap();
        let mut samples = [GpuFrameSample::default(); 1];
        assert_eq!(timer.take_into(&mut samples), 1);
        assert_eq!(samples[0].frame, 42);
        assert_eq!(samples[0].status, 1);
        assert!(
            samples[0].elapsed_ns > 0 && samples[0].elapsed_ns < 100_000_000,
            "CPU encoding delay entered the GPU span: {:?}",
            samples[0]
        );
        assert_eq!(timer.stats().pending, 0);
        }
    }

    #[test]
    fn hardware_spans_keep_identity_bound_pending_and_reuse_completed_slots() {
        let instance = crate::WgpuRasterizer::headless_instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("Hardware adapter required");
        assert!(
            adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY),
            "Timestamp-capable GPU required"
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .unwrap();
        let device=crate::PipelineDevice::from(device);device.bind_queue(&queue);
        #[cfg(not(target_arch="wasm32"))]
        device.initialize_poll_owner().unwrap();
        let mut timer = GpuFrameTimer::with_device(&device, &queue);
        pollster::block_on(timer.prepare()).unwrap();
        let mut expected = Vec::new();
        let mut samples = [GpuFrameSample::default(); 16];
        let mut observed = Vec::new();
        for round in 0..3 {
            let before = expected.len();
            for frame in (round * 8 + 1)..=(round * 8 + 8) {
                if timer.begin(&device, &queue, frame) {
                    // A rejected overlapping begin must preserve the original ID.
                    assert!(!timer.begin(&device, &queue, 10_000 + frame));
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size: 4096,
                        usage: wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let mut encoder = device.create_command_encoder(&Default::default());
                    encoder.clear_buffer(&buffer, 0, None);
                    queue.submit_for_present([encoder.finish()]);
                    timer.end(&device, &queue);
                    expected.push(frame);
                }
                assert!(timer.stats().pending <= 3);
            }
            device.wait_queue().unwrap();
            assert_eq!(
                expected.len() - before,
                3,
                "A fourth pending observation must be skipped"
            );
            timer.poll(&device, &queue);
            device.wait_queue().unwrap();
            let count = timer.take_into(&mut samples);
            for sample in &samples[..count] {
                assert_eq!(sample.status, 1, "Invalid hardware timestamps: {sample:?}");
                assert!(
                    sample.elapsed_ns > 0,
                    "Empty marker passes must not yield false zero timings"
                );
                assert!(sample.start_tick > 0 && sample.end_tick > sample.start_tick);
                observed.push(sample.frame);
            }
            assert_eq!(timer.stats().pending, 0);
        }
        expected.sort_unstable();
        observed.sort_unstable();
        assert_eq!(observed, expected);
        assert!(
            observed.len() >= 3,
            "Completed slots must be reusable across rounds"
        );
        assert_eq!(timer.stats().invalid, 0);
        assert_eq!(
            timer.stats().requested,
            observed.len() as u64 + timer.stats().skipped
        );
        assert_eq!(
            timer.take_into(&mut samples),
            0,
            "Each result is delivered once"
        );
    }
    #[test]
    fn encoded_marker_and_resolve_recover_from_dropped_owners() {
        let instance=crate::WgpuRasterizer::headless_instance();
        let adapter=pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device,queue)=pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {required_features:wgpu::Features::TIMESTAMP_QUERY,..Default::default()})).unwrap();
        let device=crate::PipelineDevice::from(device);device.bind_queue(&queue);
        #[cfg(not(target_arch="wasm32"))]
        device.initialize_poll_owner().unwrap();
        let wait=||device.wait_queue().unwrap();
        let mut timer=GpuFrameTimer::with_device(&device,&queue);pollster::block_on(timer.prepare()).unwrap();let mut samples=[GpuFrameSample::default();3];
        let mut held=device.create_command_encoder(&Default::default());
        assert!(timer.begin_encoded(&mut held,101));timer.end_encoded(&mut held);let held=held.finish();
        queue.submit([]);wait();timer.poll(&device,&queue);
        assert_eq!(timer.take_into(&mut samples),0);assert_eq!(timer.stats().pending,1);
        drop(held);assert_eq!(timer.stats().pending,0);
        let mut marker=device.create_command_encoder(&Default::default());
        assert!(timer.begin_encoded(&mut marker,102));timer.end_encoded(&mut marker);
        queue.submit([marker.finish()]);timer.submitted(&queue);wait();
        let mut held=device.create_command_encoder(&Default::default());timer.resolve_completed(&mut held);
        queue.submit([]);wait();assert_eq!(timer.take_into(&mut samples),0);assert_eq!(timer.stats().pending,1);
        drop(held);assert_eq!(timer.stats().pending,0);assert_eq!(timer.take_into(&mut samples),0);
        let mut marker=device.create_command_encoder(&Default::default());
        assert!(timer.begin_encoded(&mut marker,103));timer.end_encoded(&mut marker);
        queue.submit([marker.finish()]);timer.submitted(&queue);wait();
        let mut owner=device.create_command_encoder(&Default::default());timer.resolve_completed(&mut owner);
        queue.submit([]);wait();assert_eq!(timer.take_into(&mut samples),0);
        queue.submit([owner.finish()]);wait();
        assert_eq!(timer.take_into(&mut samples),1);assert_eq!(samples[0].frame,103);assert_eq!(samples[0].status,1);
        assert!(samples[0].end_tick>samples[0].start_tick);assert_eq!(timer.stats().pending,0);
    }
    #[test]
    fn an_abandoned_completion_ticket_neither_resolves_nor_produces_a_sample() {
        let instance=crate::WgpuRasterizer::headless_instance();
        let adapter=pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device,queue)=pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {required_features:wgpu::Features::TIMESTAMP_QUERY,..Default::default()})).unwrap();
        let device=crate::PipelineDevice::from(device);device.bind_queue(&queue);
        device.initialize_poll_owner().unwrap();
        let wait=||device.poll(wgpu::PollType::Wait {submission_index:None,timeout:Some(std::time::Duration::from_secs(5))}).unwrap();
        let mut timer=GpuFrameTimer::with_device(&device,&queue);pollster::block_on(timer.prepare()).unwrap();let mut samples=[GpuFrameSample::default();3];
        let mut marker=device.create_command_encoder(&Default::default());
        assert!(timer.begin_encoded(&mut marker,104));timer.end_encoded(&mut marker);
        queue.submit([marker.finish()]);wait();
        device.poll_binding().retire_lost(||{});
        timer.submitted(&queue);
        let completed=timer.slots.iter().any(|slot|slot.gpu_done.load(Ordering::Acquire));
        let mut resolve=device.create_command_encoder(&Default::default());timer.resolve_completed(&mut resolve);
        let resolved=timer.slots.iter().all(|slot|slot.awaiting_resolve.is_none());
        queue.submit([resolve.finish()]);wait();
        assert!(!completed,"an abandoned ticket read as GPU completion");
        assert!(!resolved,"an abandoned ticket resolved a query set that may never have been written");
        assert_eq!(timer.take_into(&mut samples),0,"an abandoned ticket produced a sample");
    }

}
