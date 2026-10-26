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
    pub fn track(&self, encoder: &wgpu::CommandEncoder) {
        use std::sync::atomic::Ordering;
        assert!(!self.tracked.swap(true, Ordering::Relaxed));
        let guard = DiscardedCacheWrite(Some(self.valid.clone()));
        encoder.on_submitted_work_done(move || guard.complete());
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

pub(crate) struct CommandEncoder {
    compute: Option<wgpu::ComputePass<'static>>,
    device: wgpu::Device,
    prologue: Option<wgpu::CommandEncoder>,
    current: wgpu::CommandEncoder,
    earlier: Vec<wgpu::CommandEncoder>,
    passes: usize,
    total_passes: u64,
    resource_work: bool,
}

impl CommandEncoder {
    /// wgpu records each pass and the barriers before it into about two
    /// command buffers, and Android destroys a command pool that held more
    /// than 128 once its work completes (`vendor/README.md`), so chunks this
    /// size keep reusing their pools.
    pub(crate) const PASSES_PER_SUBMISSION: usize = 56;

    pub fn new(device: &wgpu::Device, descriptor: &wgpu::CommandEncoderDescriptor<'_>) -> Self {
        Self {
            compute: None,
            device: device.clone(),
            prologue: None,
            current: device.create_command_encoder(descriptor),
            earlier: Vec::new(),
            passes: 0,
            total_passes: 0,
            resource_work: false,
        }
    }
    fn begin_pass(&mut self) {
        self.reserve_passes(1);
    }
    /// Account for a bounded helper that records raw wgpu passes. Rotate before
    /// the helper so its complete batch fits the same command-buffer ceiling.
    pub fn reserve_passes(&mut self, count: usize) {
        assert!(count <= Self::PASSES_PER_SUBMISSION);
        self.resource_work |= count != 0;
        self.compute = None;
        if self.passes + count > Self::PASSES_PER_SUBMISSION {
            let next = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("continued document submission"),
                });
            self.earlier
                .push(std::mem::replace(&mut self.current, next));
            self.passes = 0;
        }
        self.passes += count;
        self.total_passes += count as u64;
    }
    pub fn pass_count(&self) -> u64 { self.total_passes }
    pub fn has_work(&self)->bool {self.resource_work || self.total_passes!=0}
    pub fn can_fit_passes(&self, count: usize) -> bool {
        count <= Self::PASSES_PER_SUBMISSION - self.passes
    }
    pub fn begin_render_pass<'a>(
        &'a mut self,
        descriptor: &wgpu::RenderPassDescriptor<'_>,
    ) -> wgpu::RenderPass<'a> {
        self.begin_pass();
        self.current.begin_render_pass(descriptor)
    }
    pub fn begin_compute_pass<'a>(
        &'a mut self,
        descriptor: &wgpu::ComputePassDescriptor<'_>,
    ) -> wgpu::ComputePass<'a> {
        self.begin_pass();
        self.current.begin_compute_pass(descriptor)
    }
    /// A compute pass that consecutive dispatches share until any other pass,
    /// copy or command ends it.
    pub fn compute_pass(&mut self) -> &mut wgpu::ComputePass<'static> {
        if self.compute.is_none() {
            self.begin_pass();
            self.compute = Some(self.current.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("shared compute"),
                timestamp_writes: None,
            }).forget_lifetime());
        }
        self.compute.as_mut().unwrap()
    }
    /// Commands that run before every other command of this encoder, such as
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
        &mut self.current
    }
    /// Finish staging uploads before calling this. Their completion callbacks
    /// belong to the final encoder, after every earlier chunk on this queue.
    pub fn submit(mut self, queue: &wgpu::Queue) -> wgpu::SubmissionIndex {
        self.compute = None;
        let mut chunks = self.earlier.into_iter().chain([self.current]);
        let mut index = submit_traced(queue, self.prologue.take().into_iter().chain(chunks.next()), !self.resource_work);
        for encoder in chunks {
            index = submit_traced(queue, [encoder], !self.resource_work);
        }
        index
    }
}

// Copies, upload callbacks and query commands do not start render/compute
// passes. Forward them to the current chunk without changing their ordering;
// recording one ends the shared compute pass.
impl Deref for CommandEncoder {
    type Target = wgpu::CommandEncoder;
    fn deref(&self) -> &Self::Target {
        &self.current
    }
}
impl DerefMut for CommandEncoder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.resource_work = true;
        self.compute = None;
        &mut self.current
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
        self.current.color_pass(label, view, load)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use crate::test_support::{dab_batch, packet};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

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
    fn discarded_backdrop_uniform_upload_is_reencoded_before_rendering() {
        let mut r = renderer();
        let doc = document([64, 128]);
        let view = crate::test_support::view([128; 2]);
        r.submit(FramePacket { view, ..packet(doc.scene(), [64, 128]) }).unwrap();
        let surface = crate::create_target(r.device(), [128; 2], wgpu::TextureFormat::Rgba8Unorm, "discarded backdrop fixture").0;
        let target = surface.create_view(&Default::default());
        let mut retained = ViewportPresenter::for_renderer(&r, wgpu::TextureFormat::Rgba8Unorm);
        let mut reference = ViewportPresenter::for_renderer(&r, wgpu::TextureFormat::Rgba8Unorm);
        let region = BackdropRegion { bounds: [32., 16., 64., 96.], radii: [12.; 4], shape: BackdropRegion::SQUIRCLE };
        let surround = [0., 0., 0., 1.];
        for presenter in [&mut retained, &mut reference] {
            presenter.set_backdrop(&r, &[region], Default::default(), false);
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
