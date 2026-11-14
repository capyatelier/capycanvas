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
pub(super) struct NativeOutput {
    resource: crate::resident::Resident<pool::Resource>,
    pub tile: RasterTile,
    pub charge:crate::residency::Charge,
    pub comparison:Option<crate::native_tiles::comparison::CanonicalReadback>,
}
pub(super) struct Readback {buffer:wgpu::Buffer,charge:crate::residency::Charge}
impl NativeOutput {
    pub fn new(resource: pool::Resource, tile: RasterTile, charge: crate::residency::Charge) -> Self {
        Self { resource: crate::resident::Resident::unwritten(resource), tile, charge, comparison:None }
    }
    pub fn bytes(&self) -> u64 { self.charge.bytes() }
    pub fn initialize<'a>(&'a mut self, submission: &'a submission::Submission) -> Option<crate::resident::RasterWrite<'a, pool::Resource>> {
        self.resource.initialize(submission)
    }
    pub fn capture(&self) -> Result<TileCapture<'_>, String> {
        let pixels = self.resource.read(self.tile.capture_name(), None)
            .ok_or("Native raster output does not match its submitted capture")?;
        Ok(TileCapture {
            source: match pixels.value() {
                pool::Resource::Buffer(buffer) => CaptureSource::Packed(buffer),
                pool::Resource::Texture(texture) => CaptureSource::Texture(texture),
            },
            tile: self.tile.clone(),
        })
    }
}
pub(crate) struct NativeCapture {
    pub(super) outputs: Vec<NativeOutput>,
    pub pool: Arc<BufferPool>,
    pub device: PipelineDevice,
    pub queue: wgpu::Queue,
    pub(super) readback:Option<Readback>,
    pub(super) allocation:crate::residency::Reservation,
    pub(super) owner:layer_core::MemoryOwner,
}
impl NativeCapture {
    pub(super) fn prepare_readback(&mut self,bytes:u64) {
        if bytes==0 {return;}
        let buffer=self.pool.take(&self.device,bytes);
        self.readback=Some(Readback {buffer,charge:self.pool.charge(bytes)});
    }
    fn recycle(&mut self,count:usize) {
        for output in self.outputs.drain(..count) {self.pool.put_charged(output.resource.retire(),output.charge);}
    }
    fn recycle_readback(&mut self) {
        if let Some(readback)=self.readback.take() {self.pool.put_charged(pool::Resource::Buffer(readback.buffer),readback.charge);}
    }
    pub(super) fn take_window(&mut self)->Self {
        Self {outputs:std::mem::take(&mut self.outputs),pool:self.pool.clone(),device:self.device.clone(),queue:self.queue.clone(),
            readback:self.readback.take(),allocation:self.allocation.split(0).unwrap(),owner:self.owner.clone()}
    }
    pub fn append(&mut self,mut other:Self) {
        if self.readback.as_ref().map_or(0,|readback|readback.buffer.size())<other.readback.as_ref().map_or(0,|readback|readback.buffer.size()) {
            std::mem::swap(&mut self.readback,&mut other.readback);
        }
        self.outputs.append(&mut other.outputs);
    }
    pub fn storage_bytes(&self) -> u64 {
        self.outputs.iter().map(NativeOutput::bytes).sum::<u64>()
    }
    fn next_transfer(&self, label: &str) -> Result<(usize, PreparedCapture, wgpu::CommandBuffer), String> {
        let readback=&self.readback.as_ref().ok_or("Native readback was not admitted")?.buffer;
        let mut count = 0;
        let mut size = 0;
        for output in &self.outputs {
            let bytes=output.bytes()+crate::native_tiles::comparison::RESULT_BYTES*u64::from(output.comparison.is_some());
            if size + bytes > CAPTURE_CHUNK.min(readback.size()) {
                break;
            }
            count += 1;
            size += bytes;
        }
        let copies = self.outputs[..count].iter().map(NativeOutput::capture).collect::<Result<Vec<_>, _>>()?;
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
        let buffer=|bytes| {assert!(bytes<=readback.size());readback.clone()};
        let capture=if self.outputs[..count].iter().all(|output|output.comparison.is_none()) {
            prepare_capture(&self.pool,&mut encoder,&copies,buffer)
        }else {
            let comparisons=self.outputs[..count].iter().map(|output|output.comparison.clone()).collect::<Vec<_>>();
            prepare_capture_compared(&self.pool,&mut encoder,&copies,&comparisons,buffer)
        }.map_err(|e|e.to_string())?;
        Ok((count, capture, encoder.finish()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn finish(mut self) -> Result<(), String> {
        while !self.outputs.is_empty() {
            self.pool.priority.wait()?;
            let (count, capture, commands) = self.next_transfer("background native raster readback")?;
            // Recheck after preparation. A frame can race this check, but only
            // this one bounded transfer can then precede it on the GPU queue.
            self.pool.priority.wait()?;
            self.device.submit(&self.queue,[commands]);
            capture
                .submitted_device(&self.device)
                .finish_recycling(drop)?;
            self.recycle(count);
        }
        self.recycle_readback();Ok(())
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
        let outputs=std::mem::take(&mut self.outputs);let readback=self.readback.take();
        if outputs.is_empty() && readback.is_none() {return;}
        let allocation=(self.allocation.split(self.allocation.bytes()),self.owner.clone());
        #[cfg(not(target_arch="wasm32"))]
        {let retired=self.device.retirement((outputs,readback,allocation));
        self.device.on_queue_done(&self.queue,move||retired.complete());}
        #[cfg(target_arch="wasm32")]
        {
            let charges=outputs.into_iter().map(|output|output.charge).chain(readback.map(|readback|readback.charge)).collect::<Vec<_>>();
            self.device.on_queue_done(&self.queue,move||drop((charges,allocation)));
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
impl NativeCapture {
    pub(crate) async fn finish_browser(mut self, worker: &browser::BrowserRasterEncoder) -> Result<(), String> {
        while !self.outputs.is_empty() {
            self.pool.priority.wait_browser().await;
            let (count, capture, commands) = self.next_transfer("browser native raster readback")?;
            self.pool.priority.wait_browser().await;
            self.device.submit(&self.queue,[commands]);
            capture.submitted_device(&self.device).finish_browser_recycling(worker,drop).await?;
            self.recycle(count);
        }
        self.recycle_readback();Ok(())
    }
}


#[cfg(all(test,not(target_arch="wasm32")))]
mod tests {
    use super::*;

    fn fixture_input_name(coordinate:[u32;2])->layer_core::raster::ContentVersion {
        use layer_core::raster::ContentVersion;
        let fill=[0.25f32,0.125,0.5,1.].into_iter().enumerate()
            .fold(0u128,|bits,(i,value)|bits|u128::from(value.to_bits())<<(32*i));
        ContentVersion::derive(u128::from(coordinate[0])|u128::from(coordinate[1])<<32,
            &[ContentVersion::derive(fill,&[])])
    }
    fn capture_fixture_inputs(r:&mut WgpuRasterizer,expected:&[layer_core::raster::ContentVersion],commands:&mut submission::CommandEncoder)->NativeCapture {
        let descriptor=r.document_color().paint_descriptor();
        pollster::block_on(r.compilation.prepare_all(r.native_edit.as_ref().unwrap()
            .required_pipelines(r.document_color().depth,false))).unwrap();
        let receipt=commands.submission();let mut encodings=Vec::new();
        assert_eq!(r.paint_layers[0].pages.len(),expected.len());
        let inputs=r.paint_layers[0].pages.iter_mut().zip(expected).map(|(page,expected)| {
            let tile=RasterTile::pending(descriptor);
            let write=page.primary.revise(expected,&receipt).unwrap();
            let texture=write.value().texture.clone();
            encodings.push(write.encoded(tile.content_version()));
            (texture,tile,layer_core::color::LayerColorMode::FullColor)
        }).collect();
        let capture=r.encode_private_tiles(inputs,commands).unwrap();
        let actual=commands.submission();
        assert!(actual.same(&receipt),"the fixture's bounded capture must remain in one submission");
        for encoding in encodings {encoding.publish(&actual);}
        capture
    }

    fn capture_fixture(pages:u32)->(WgpuRasterizer,NativeCapture) {
        let extent=[11*PAGE_SIZE,3*PAGE_SIZE];
        let doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let color=doc.composition().color;
        let mut r=crate::memory::memory_tests::renderer(color,layer_core::DeviceMemoryFacts {
            budget_bytes:Some(2<<30),..Default::default()}).unwrap();
        r.ensure_document_metadata(extent,doc.scene()).unwrap();
        let mut commands=submission::CommandEncoder::new(&r.device,&Default::default());
        let receipt=commands.submission();let mut names=Vec::new();
        for n in 0..pages {
            let coordinate=[n%11,n/11];let name=fixture_input_name(coordinate);
            let mut page=r.create_page(coordinate,"native ownership fixture");
            let write=page.primary.initialize(&receipt).unwrap();
            {let _pass=commands.color_pass("native ownership fixture",&write.value().view,
                wgpu::LoadOp::Clear(wgpu::Color {r:0.25,g:0.125,b:0.5,a:1.}));}
            write.publish(&name);page.primary_needs_clear=false;names.push(name);
            r.paint_layers[0].pages.push(page);
        }
        commands.submit(&r.queue);crate::test_support::complete(&r);
        r.refresh_storage_metrics();r.publish_memory();r.factory.memory_budget().flush_reports();r.memory.invalidate_limits();r.refresh_memory_limits();
        let baseline=r.paint_memory_bytes()+r.raster_buffers.retained.used();
        let initial_grant=r.memory.raster_grant();
        assert!(baseline<=initial_grant);
        assert!(initial_grant<baseline+u64::from(pages)*256*1024,"fixture must consume Raster's current grant");
        let mut commands=submission::CommandEncoder::new(&r.device,&Default::default());
        let capture=capture_fixture_inputs(&mut r,&names,&mut commands);
        r.uploads.finish(&commands);commands.submit(&r.queue);
        (r,capture)
    }

    #[test]
    fn native_readback_rejects_a_destination_with_another_captures_name() {
        let (_r,mut capture)=capture_fixture(17);
        let original=capture.outputs[0].tile.clone();
        let (equivalent,comparison)=RasterTile::pending_from_live(original.descriptor(),original.content_version());
        comparison.resolve(false).unwrap();
        for wrong in [RasterTile::pending(original.descriptor()),equivalent] {
            assert!(!wrong.same_capture(&original));
            capture.outputs[0].tile=wrong;
            let transfer=capture.next_transfer("mismatched native capture");
            capture.outputs[0].tile=original.clone();
            assert!(transfer.is_err(),"pixel equivalence never authorizes another byte capture's name");
        }
    }

    #[test]
    fn native_capture_completion_wakes_after_publication_and_releases_its_counters() {
        let (mut r,capture)=capture_fixture(17);
        let tiles:Vec<_>=capture.outputs.iter().map(|output|output.tile.clone()).collect();
        let (before,unused)=mpsc::channel();
        r.set_raster_preparation_wake(Arc::new(move|| {let _=before.send(());}));
        r.prepare_source_backing().unwrap();
        let runtime=r.raster.as_ref().unwrap();
        let slot=Arc::downgrade(&runtime.preparation_wake);
        let pending=runtime.worker.as_ref().unwrap().pending.clone();
        let staging=runtime.worker.as_ref().unwrap().staging.clone();
        let (completed,ready)=mpsc::channel();
        r.set_raster_preparation_wake(Arc::new(move|| {
            let registered=slot.upgrade().is_some_and(|slot|slot.callback.lock().unwrap().is_some());
            let published=tiles.iter().all(|tile|matches!(tile.try_backing(),Some(Ok(_))));
            let _=completed.send((registered,published,pending.used(),staging.used()));
        }));
        r.raster.as_ref().unwrap().worker.as_ref().unwrap().submit_batch(capture).unwrap();
        assert_eq!(ready.recv_timeout(std::time::Duration::from_secs(10)).unwrap(),(true,true,0,0));
        assert!(!r.raster_publication_pending());
        assert!(unused.try_recv().is_err(),"the callback installed after worker creation replaces the earlier callback");
    }

    #[test]
    fn renderer_replacement_routes_old_capture_completion_to_the_current_preparation_wake() {
        use layer_render::CanvasRenderer;
        let (mut previous,capture)=capture_fixture(17);
        let tiles:Vec<_>=capture.outputs.iter().map(|output|output.tile.clone()).collect();
        let (retired,unused)=mpsc::channel();
        previous.set_raster_preparation_wake(Arc::new(move|| {let _=retired.send(());}));
        previous.prepare_source_backing().unwrap();
        let presentation=previous.prioritize_raster_presentation();
        previous.raster.as_ref().unwrap().worker.as_ref().unwrap().submit_batch(capture).unwrap();
        assert!(previous.raster_publication_pending());
        assert!(tiles.iter().all(|tile|tile.try_backing().is_none()));
        let color=previous.document_color();let factory=previous.factory.clone();
        let mut current=WgpuRasterizer::from_factory_inner(factory.clone(),factory.pipeline_device(color),Initialization::Headless).unwrap();
        current.initialize_native(color).unwrap();
        current.adopt_committed_pages(&mut previous);
        assert!(Arc::ptr_eq(&previous.raster.as_ref().unwrap().preparation_wake,
            &current.raster.as_ref().unwrap().preparation_wake));
        let pending=previous.raster.as_ref().unwrap().worker.as_ref().unwrap().pending.clone();
        let staging=previous.raster.as_ref().unwrap().worker.as_ref().unwrap().staging.clone();
        let (completed,ready)=mpsc::channel();
        current.set_raster_preparation_wake(Arc::new(move|| {
            let published=tiles.iter().all(|tile|matches!(tile.try_backing(),Some(Ok(_))));
            let _=completed.send((published,pending.used(),staging.used()));
        }));
        assert!(ready.try_recv().is_err());
        drop(presentation);
        assert_eq!(ready.recv_timeout(std::time::Duration::from_secs(10)).unwrap(),(true,0,0));
        assert!(unused.try_recv().is_err(),"a retired generation must not receive the completion wake");
    }

    #[test]
    fn native_capture_owns_outputs_and_rounded_readback_until_completion() {
        let mut failures=Vec::new();
        for pages in [17u32,33] {
            let (r,mut capture)=capture_fixture(pages);
            let output_bytes=capture.outputs.iter().map(|output|output.bytes()).sum::<u64>();
            assert_eq!(output_bytes,u64::from(pages)*256*1024);
            let observe=|phase:&str,r:&WgpuRasterizer,capture:&NativeCapture,readback:u64,failures:&mut Vec<String>| {
                let readback=readback.max(capture.readback.as_ref().map_or(0,|readback|readback.buffer.size()));
                let physical=r.paint_memory_bytes()+r.staged_restore_bytes()+r.raster_buffers.retained.used()
                    +capture.outputs.iter().map(|output|output.bytes()).sum::<u64>()+readback;
                let pinned=r.pinned_paint_bytes()+capture.storage_bytes()+readback;
                let report=r.raster_memory_report();let grant=r.memory.raster_grant();
                if report.gpu_bytes<physical || report.pinned_gpu_bytes<pinned || grant<physical {
                    failures.push(format!("seed=139a pages={pages} {phase}: physical={physical} report={} pins={} grant={grant} native={} readback={readback}",
                        report.gpu_bytes,report.pinned_gpu_bytes,capture.storage_bytes()));
                }
            };
            observe("native outputs accepted",&r,&capture,0,&mut failures);
            let (count,readback,commands)=capture.next_transfer("native ownership readback").unwrap();
            assert_eq!(count,pages as usize);
            let readback_bytes=readback.chunks.iter().map(|(buffer,_)|buffer.size()).sum::<u64>();
            assert_eq!(readback_bytes,output_bytes.next_power_of_two());
            observe("rounded readback accepted",&r,&capture,readback_bytes,&mut failures);
            r.queue.submit([commands]);
            readback.submitted_device(&r.device).finish_recycling(drop).unwrap();
            assert!(capture.outputs.iter().all(|output|matches!(output.tile.try_backing(),Some(Ok(_)))));
            observe("readback complete, native outputs held",&r,&capture,0,&mut failures);
            capture.recycle(count);capture.recycle_readback();
            observe("outputs returned",&r,&capture,0,&mut failures);
        }
        assert!(failures.is_empty(),"native capture ownership from creation through completion:\n{}",failures.join("\n"));
    }

    #[test]
    fn pending_native_capture_keeps_its_raster_owner_after_renderer_departure() {
        for finish in [true,false] {
            let (r,capture)=capture_fixture(17);
            let budget=r.factory.memory_budget().clone();
            let device=(*r.device).clone();let queue=r.queue.clone();
            let held=capture.storage_bytes()+capture.readback.as_ref().unwrap().buffer.size();
            let raster=layer_core::MemoryOwnerKind::RasterPool as usize;
            assert_eq!(budget.snapshot().rows[raster].iter().map(|row|row.owners).sum::<usize>(),1);
            drop(r);
            let submission=queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
            device.poll(wgpu::PollType::Wait {submission_index:Some(submission),timeout:Some(READBACK_TIMEOUT)}).unwrap();
            let snapshot=budget.snapshot();let rows=&snapshot.rows[raster];
            assert_eq!(rows.iter().map(|row|row.owners).sum::<usize>(),1,"capture lost its Raster owner after renderer retirement");
            assert!(rows.iter().map(|row|row.gpu_bytes).sum::<u64>()>=held);
            assert!(rows.iter().map(|row|row.pinned_gpu_bytes).sum::<u64>()>=held);
            if finish {capture.finish().unwrap();}else {drop(capture);}
            let submission=queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
            device.poll(wgpu::PollType::Wait {submission_index:Some(submission),timeout:Some(READBACK_TIMEOUT)}).unwrap();
            assert_eq!(budget.snapshot().rows[raster].iter().map(|row|row.owners).sum::<usize>(),0,"completed or retired capture retained its Raster owner");
        }
    }

    #[test]
    fn appended_native_captures_reuse_the_largest_admitted_readback() {
        let (mut r,mut capture)=capture_fixture(16);
        let mut tickets=capture.outputs.iter().map(|output|output.tile.clone()).collect::<Vec<_>>();
        let expected=capture.outputs.iter().map(|output|output.tile.content_version()).collect::<Vec<_>>();
        let mut commands=submission::CommandEncoder::new(&r.device,&Default::default());
        let other=capture_fixture_inputs(&mut r,&expected,&mut commands);
        r.uploads.finish(&commands);commands.submit(&r.queue);
        tickets.extend(other.outputs.iter().map(|output|output.tile.clone()));
        assert_eq!(capture.storage_bytes(),4*1024*1024);
        assert_eq!(other.storage_bytes(),4*1024*1024);
        capture.append(other);
        assert_eq!(capture.storage_bytes(),8*1024*1024);
        assert_eq!(capture.readback.as_ref().unwrap().buffer.size(),4*1024*1024);
        capture.finish().unwrap();
        assert_eq!(tickets.len(),32);
        assert!(tickets.iter().all(|tile|matches!(tile.try_backing(),Some(Ok(_)))));
    }

    fn assert_canonical_names(scalar:bool) {
        use crate::native_tiles::{comparison::CanonicalComparisons,NativeTileEncoder,NativeTileRequest};
        use crate::native_tiles::scalar::{NativeScalarEncoder,NativeScalarRequest};
        use layer_core::color::{DocumentColor,RgbSpace,SampleDepth,LayerColorMode};
        use layer_core::raster::ContentName;
            for in_place in [false,true] {
                for changed in [false,true] {
                    let color=DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::U8};
                    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();
                    let working=crate::test_support::page_texture(&r,if scalar {wgpu::TextureFormat::R32Float}else {wgpu::TextureFormat::Rgba32Float});
                    let canonical=if in_place {working.clone()}else {crate::test_support::page_texture(&r,working.format())};
                    let value=if changed {0.25f32}else {0.0};
                    let bytes=if scalar {vec![value;65536].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>()}
                        else {(0..65536).flat_map(|_|[value,0.,0.,1.]).flat_map(f32::to_le_bytes).collect()};
                    crate::test_support::upload_page(&r,&working,&bytes);
                    let descriptor=if scalar {RasterPlane::Mask.descriptor(color)}else {color.paint_descriptor()};
                    let before=ContentName::fresh();
                    let (tile,name)=RasterTile::pending_from_live(descriptor,before.into());
                    assert_eq!(tile.content_version().resolved(),None);
                    let mut allocation=r.admit_raster_working(CanonicalComparisons::bytes(1).unwrap()).unwrap();
                    let comparison=CanonicalComparisons::reserved(&mut r,1,&mut allocation).unwrap();
                    let result=comparison.result(0,name);
                    let transfer=r.prepare_native_transfer(color.space).unwrap();
                    let mut commands=r.device.create_command_encoder(&Default::default());
                    let packed=scalar.then(||r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("canonical scalar fixture"),
                        size:descriptor.byte_len([256;2]).unwrap() as u64,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false}));
                    let encoded=(!scalar).then(||crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba8Uint));
                    if scalar {
                        let encoder=if in_place {NativeScalarEncoder::in_place(&r.device)}else {NativeScalarEncoder::with_device(&r.device)};
                        pollster::block_on(r.compilation.prepare_all(&encoder.pipelines)).unwrap();
                        let batch=encoder.prepare_compared(&r.device,&[NativeScalarRequest {working:&working,canonical:&canonical,
                            encoded:packed.as_ref().unwrap(),depth:SampleDepth::U8,region:[0,0,256,256]}],&mut Default::default(),Some(&comparison)).unwrap();
                        let mut pass=commands.begin_compute_pass(&Default::default());encoder.encode(&mut pass,&batch);
                    }else {
                        let encoder=NativeTileEncoder::new(&r.device,in_place);
                        pollster::block_on(r.compilation.prepare_all(&encoder.pipelines)).unwrap();
                        let batch=encoder.prepare_compared(&r.device,&[NativeTileRequest {working:&working,canonical:&canonical,
                            encoded:encoded.as_ref().unwrap().into(),depth:SampleDepth::U8,region:[0,0,256,256],mode:LayerColorMode::FullColor,
                            space:color.space,transfer:&transfer,alpha:descriptor.alpha}],&mut Default::default(),&[],Some(&comparison)).unwrap();
                        let mut pass=commands.begin_compute_pass(&Default::default());encoder.encode(&mut pass,&batch);
                    }
                    let source=match (&packed,&encoded) {(Some(buffer),_)=>CaptureSource::Packed(buffer),(_,Some(texture))=>CaptureSource::Texture(texture),_=>unreachable!()};
                    let capture=prepare_capture_compared(&r.raster_buffers,&mut commands,&[TileCapture {source,tile:tile.clone()}],&[Some(result)],
                        |bytes|r.raster_buffers.take(&r.device,bytes)).unwrap();
                    assert_eq!(tile.content_version().resolved(),None,"encoding must not guess GPU comparison outcome");
                    r.queue.submit([commands.finish()]);
                    capture.submitted_device(&r.device).finish().unwrap();
                    assert_eq!(tile.content_version().matches(&before.into()),Some(!changed),"scalar={scalar} in_place={in_place} changed={changed}");
                    let pixels=tile.wait_backing().unwrap().decode_on_worker().unwrap();
                    assert_eq!(pixels.len(),descriptor.byte_len([256;2]).unwrap());
                    let canonical_bytes=crate::layer_tests::page_bytes(&r,&canonical);
                    assert_eq!(canonical_bytes==bytes,!changed,"comparison must describe pre-packing working pixels exactly");
                }
            }
    }

    #[test]
    fn color_native_canonical_names_resolve_from_capture_owned_comparisons() {assert_canonical_names(false);}
    #[test]
    fn scalar_native_canonical_names_resolve_from_capture_owned_comparisons() {assert_canonical_names(true);}

}
