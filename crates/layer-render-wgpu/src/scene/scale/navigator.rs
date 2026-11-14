use super::*;

#[cfg(test)]
std::thread_local! {
    static RENDER_GRAPHS:std::cell::Cell<u64>=const {std::cell::Cell::new(0)};
}

#[derive(Default)]
pub(crate) struct Navigator {
    cache: Option<Cache>,
    refreshed: f32,
    pub revision: u64,
    contact: Option<u64>,
    stale: bool,
}

impl Navigator {
    pub fn pending(&self) -> bool {
        self.stale || self.cache.as_ref().is_some_and(|cache| cache.presented_matches_expected().is_some_and(|matches|
            !matches || cache.submission_valid.as_ref().is_some_and(|valid|!valid.load(std::sync::atomic::Ordering::Acquire))))
    }
    fn held(&self,r:&WgpuRasterizer)->bool {self.view().is_some() && r.vector_motion_split.is_some()}
    pub(crate) fn outstanding(&self,r:&WgpuRasterizer)->Option<crate::frame_demand::Outstanding> {
        if self.held(r) || !self.pending() {return None;}
        Some(self.cache.as_ref()?.cell_demand(r).unwrap_or(crate::frame_demand::Outstanding::Runnable))
    }
    pub fn waiting_for_name(&self)->bool {
        self.cache.as_ref().is_some_and(|cache|cache.presented_matches_expected().is_none())
    }
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.cache.as_ref().filter(|c| c.submission_valid.as_ref()
            .is_none_or(|v| v.load(std::sync::atomic::Ordering::Acquire)))
            .and_then(|c| c.pixels.committed_root()).map(pages::CommittedImage::view)
    }
    pub fn scale(&self) -> f32 { self.cache.as_ref().map_or(1., |c| (1 << c.plan.level) as f32) }
    pub fn storage_bytes(&self) -> u64 { self.cache.as_ref().map_or(0, Cache::window_bytes) }

    pub(in crate::scene) fn prepare(&mut self,r:&WgpuRasterizer,packet:FramePacket<'_>,scene:&Scene,pages:Pages,reset:bool)->Result<(),GpuRasterError> {
        #[cfg(test)] crate::test_support::PassCounts {navigator_checks:1,..Default::default()}.record();
        self.contact=None;self.stale=false;
        let plan=display_mips::Plan::new(packet.document_extent)?;
        if reset || self.cache.as_ref().is_none_or(|cache|cache.plan!=plan) {
            let window=self.cache.as_ref().map(|cache|cache.window);
            self.cache=Some(Cache::new(r,Request {plan,evaluation:Evaluation::Display},packet.scene.order().len()));
            if let Some(window)=window {self.cache.as_mut().unwrap().window=window;}
        }
        let cache=self.cache.as_mut().unwrap();
        cache.pages=pages;
        cache.prepare_graph(r,packet,scene)
    }
    pub(in crate::scene) fn follows(&self,display:&Cache)->bool {self.contact.is_some() && self.contact==display.contact_id()}
    pub(super) fn append_wanted_work(&self,r:&WgpuRasterizer,packet:FramePacket<'_>,scene:&Scene,windows:&mut Vec<pages::Interest>) {
        if let Some(cache)=&self.cache {cache.append_wanted_work(r,packet,scene,windows);}
    }

    pub fn refresh(&mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, changed: bool,
        encoder: &mut crate::submission::CommandEncoder, budget: &mut WorkBudget,
    ) -> Result<(), GpuRasterError> {
        crate::performance_trace::counter(c"Capy Navigator age us", ((packet.time_seconds - self.refreshed).max(0.) * 1e6) as u64);
        crate::performance_trace::counter(c"Capy Navigator pending", u64::from(self.pending()));
        crate::performance_trace::counter(c"Capy Navigator bytes", self.storage_bytes());
        let Some(display) = r.scale_display.as_ref() else { return Ok(()); };
        let followed=self.cache.as_ref().zip(display.contact_id()).and_then(|(cache,contact)|display.contact_window(contact,cache).map(|source|(source,contact)));
        let follows=followed.is_some();
        let deferred=self.held(r) || changed && self.view().is_some() && packet.time_seconds>=self.refreshed && packet.time_seconds-self.refreshed<0.05;
        let source=if let Some((source,contact))=followed {
            if deferred {
                self.stale=true;self.contact=Some(contact);return Ok(());
            }
            Some((source,Some(contact)))
        } else {
            let pages=display.pages.clone();
            self.prepare(r,packet,r.scene.as_ref().unwrap(),pages,false)?;
            let cache=self.cache.as_mut().unwrap();
            let Some(matches)=cache.presented_matches_expected() else {return Ok(());};
            let valid=cache.submission_valid.as_ref().is_none_or(|valid|valid.load(std::sync::atomic::Ordering::Acquire));
            if matches && valid || deferred {return Ok(());}
            let cache=self.cache.as_ref().unwrap();
            r.scale_display.as_ref().into_iter().flat_map(|display|std::iter::once(display).chain(display.overview.as_deref()))
                .filter_map(|window|window.current_window(cache).map(|source|(source,window.contact_id()))).max_by_key(|(source,_)|source.level())
        };
        let contact=source.as_ref().and_then(|(source,contact)|contact.filter(|_|source.current()));
        let cache = self.cache.as_mut().unwrap();
        let write = crate::submission::CacheWrite::new();
        let mut scene = r.scene.take().unwrap();
        let scene_write = scene.begin_write(r,encoder)?;
        let mut commands = scene.scale_commands.take().unwrap_or_else(|| RegionRecords::charged(r, &scene.tiers.gpu));
        commands.begin();
        let _trace = crate::performance_trace::Span::new(c"capy.navigator_refresh");
        r.telemetry.phase_begin(13, &r.device, &r.queue, encoder);
        let result=(|| {
            if let Some((source,_))=source {
                cache.inherit(&scene,r,encoder,&mut commands,source)?;
            }else {
                #[cfg(test)]
                RENDER_GRAPHS.with(|count|count.set(count.get()+1));
                cache.render_graph(&mut scene,r,packet,cache.plan.bounds,
                    &mut Encoding {encoder,commands:&mut commands, retained:0, budget},Destination::Navigator,None)?;
            }
            Ok(())
        })();
        r.telemetry.phase_end(13, encoder);
        scene.scale_commands = Some(commands);
        r.scene = Some(scene);
        match result {
            Err(GpuRasterError::NotReady|GpuRasterError::SourceWorkingSetExceeded)=> {
                scene_write.track(encoder);
                return Ok(());
            },
            result=>result?,
        }
        if !follows && cache.presented_matches_expected()!=Some(true) {
            scene_write.track(encoder);
            return Ok(());
        }
        cache.submission_valid = Some(write.validity());
        self.contact=contact;
        write.track(encoder);
        scene_write.track(encoder);
        self.refreshed = packet.time_seconds;self.stale=false;
        self.revision = self.revision.wrapping_add(1);
        crate::performance_trace::counter(c"Capy Navigator refreshes", self.revision);
        crate::performance_trace::counter(c"Capy Navigator pixels", cache.plan.size.map(u64::from).into_iter().product());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{float_pixels, packet, submit_settled};
    use layer_core::{Affine, Document, ImageTransform, Point};

    fn document() -> Document {
        let mut doc = Document::new(layer_core::authored::PortableId::random(), 1025, 513, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let SourceTarget::Paint(paint) = doc.working.target.unwrap() else { unreachable!() };
        doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new((layer_core::color::source::rgba8_source([1025, 513], |x, y|
            [(x / 5) as u8, (y / 3) as u8, 40, 255])).into()));
        doc
    }
    fn pixels(r: &WgpuRasterizer) -> Vec<[f32; 4]> {
        float_pixels(r, r.navigator.cache.as_ref().unwrap().texture())
    }
    fn assert_same_picture(r: &WgpuRasterizer, actual: &[[f32; 4]], expected: &[[f32; 4]]) {
        assert_eq!(actual.len(),expected.len());
        let width=r.navigator.cache.as_ref().unwrap().plan.size[0];
        let mut worst=(0u32,[0u32;2],0usize,0f32,0f32);
        for (index,(actual,expected)) in actual.iter().zip(expected).enumerate() {
            for (channel,(&actual,&expected)) in actual.iter().zip(expected).enumerate() {
                assert!(actual.is_finite() && expected.is_finite(),"nonfinite Navigator comparison: pixel={index}, channel={channel}, actual={actual}, expected={expected}");
                let ordered=|value:f32| {let bits=value.to_bits();if bits&0x8000_0000!=0 {!bits}else {bits|0x8000_0000}};
                let distance=if actual==expected {0}else {ordered(actual).abs_diff(ordered(expected))};
                if distance>worst.0 {worst=(distance,[index as u32%width,index as u32/width],channel,actual,expected);}
            }
        }
        assert!(worst.0<=1,"repeated Navigator picture exceeds one stored-format ULP: worst(ulp,xy,channel,actual,expected)={worst:?}");
    }
    fn pose(r: &mut WgpuRasterizer, doc: &Document, x: f32) {
        r.set_transform_preview(Some(&layer_render::TransformPreview {
            transaction: 1, target: doc.working.target.unwrap(), moving: true, selection: None,
            transform: ImageTransform::affine(Affine::translation(Point { x, y: 0. })),
        })).unwrap();
    }

    #[test]
    fn navigator_coalesces_poses_and_flushes_without_further_input() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        submit_settled(&mut r, frame);
        let original = pixels(&r);
        let revision = r.navigator.revision;
        for step in 1..=4 {
            pose(&mut r, &doc, step as f32 * 20.);
            frame.time_seconds = step as f32 * 0.01;
            crate::test_support::submit_ready(&mut r, frame).unwrap();
            assert_eq!(r.navigator.revision, revision);
            assert!(r.navigator.pending() && r.next_frame()==layer_render::FrameDemand::Work);
        }
        assert_eq!(pixels(&r), original);
        frame.time_seconds = 0.06;
        pose(&mut r, &doc, 100.);
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert_eq!(r.navigator.revision, revision + 1);
        assert_ne!(pixels(&r), original);
        assert!(!r.navigator.pending());
        frame.time_seconds = 0.07;
        pose(&mut r, &doc, 120.);
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert!(r.next_frame()==layer_render::FrameDemand::Work);
        frame.time_seconds = 0.071;
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert_eq!(r.navigator.revision, revision + 2);
        assert!(!r.navigator.pending());
        let settled = pixels(&r);
        for step in 1..=3 {
            frame.time_seconds += 0.1;
            frame.view.document_to_surface[4] = step as f32 * -80.;
            crate::test_support::submit_ready(&mut r, frame).unwrap();
            assert_eq!(r.navigator.revision, revision + 2);
            assert_eq!(pixels(&r), settled);
        }
        for step in 1..=20 {
            frame.time_seconds += 0.06;
            pose(&mut r, &doc, 120. + step as f32);
            crate::test_support::submit_ready(&mut r, frame).unwrap();
            assert_eq!(r.navigator.revision, revision + 2 + step);
        }
        r.prepare_moving_pixels(None);
        r.set_transform_preview(None).unwrap();
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        submit_settled(&mut r, frame);
        assert_same_picture(&r, &pixels(&r), &original);
    }

    #[test]
    fn navigator_history_and_blending_refresh_before_native_refinement() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        submit_settled(&mut r, frame);
        let original = pixels(&r);
        let original_format=r.navigator.cache.as_ref().unwrap().texture().format();
        let mut edited = doc.clone();
        edited.artwork.occurrences.get_mut(doc.working.occurrence.unwrap()).unwrap().opacity = 0.3;
        let mut alternate = None;
        for (step, document) in [&edited, &doc, &edited, &doc].into_iter().enumerate() {
            frame.time_seconds += 0.01;
            let edit = FramePacket { scene: document.scene(), ..frame };
            let previous = pixels(&r);
            r.submit(edit).unwrap();
            assert!(r.navigator.pending());
            r.background_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
            r.submit(edit).unwrap();
            assert!(pixels(&r) != previous, "logical edits must reach Navigator while native refinement is held");
            r.background_ready.store(true, std::sync::atomic::Ordering::Release);
            submit_settled(&mut r, edit);
            assert!(!r.navigator.pending());
            let actual = pixels(&r);
            if step == 0 { assert_ne!(actual, original); alternate = Some((actual,r.navigator.cache.as_ref().unwrap().texture().format())); }
            else {
                let (expected,expected_format)=if step%2==0 {alternate.clone().unwrap()}else {(original.clone(),original_format)};
                let format=r.navigator.cache.as_ref().unwrap().texture().format();
                assert_eq!((format,expected_format),(wgpu::TextureFormat::Rgba32Float,wgpu::TextureFormat::Rgba32Float));
                assert_same_picture(&r,&actual,&expected);
            }
        }
        frame.blend_space = layer_core::BlendSpace::Perceptual;
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        submit_settled(&mut r, frame);
        assert!(!r.navigator.pending());
        assert_ne!(pixels(&r), original);
    }

    #[test]
    fn navigator_tracks_temporary_brush_tails_and_their_removal() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        submit_settled(&mut r, frame);
        let original = pixels(&r);
        let mut dab = crate::tests::test_dab([100., 100.], [0.9, 0.02, 0.1, 1.], 1.);
        dab.radii = [45.; 2];
        for (time, x) in [(0.01, 100.), (0.03, 150.), (0.06, 200.)] {
            dab.center.x = x;
            let batch = DabBatch { kind: DabBatchKind::Preview, ..crate::test_support::dab_batch(
                doc.working.target.unwrap(), crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen), dab.bounds()) };
            frame.time_seconds = time;
            crate::test_support::submit_ready(&mut r, FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame }).unwrap();
        }
        assert_ne!(pixels(&r), original);
        frame.time_seconds = 0.07;
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert!(r.navigator.pending());
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        submit_settled(&mut r, frame);
        assert!(!r.navigator.pending());
        assert_same_picture(&r, &pixels(&r), &original);
    }

    #[test]
    fn a_contact_stroke_at_sixty_hertz_republishes_the_navigator_at_most_every_fifty_milliseconds() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        submit_settled(&mut r, frame);
        let revision = r.navigator.revision;
        let mut dab = crate::tests::test_dab([100., 100.], [0.9, 0.02, 0.1, 1.], 1.);
        dab.radii = [20.; 2];
        for step in 1..=30u8 {
            dab.center.x = 100. + f32::from(step) * 20.;
            let batch = DabBatch { stroke_start: step == 1, stroke_end: false, ..crate::test_support::dab_batch(
                doc.working.target.unwrap(), crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen), dab.bounds()) };
            frame.time_seconds = f32::from(step) / 60.;
            crate::test_support::submit_ready(&mut r, FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame }).unwrap();
        }
        let published = r.navigator.revision - revision;
        assert!((1..=10).contains(&published), "half a second of painting republished the Navigator {published} times");
    }

    #[test]
    fn an_animated_effect_at_sixty_hertz_republishes_the_navigator_at_most_every_fifty_milliseconds() {
        let mut doc = document();
        let mut grain = layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("film_grain").unwrap().program());
        grain.set("animate", layer_core::EffectValue::Toggle(true)).unwrap();
        let grain = super::super::tests::effect_occurrence(&mut doc, grain, "Grain");
        super::super::tests::insert_occurrence(&mut doc, grain, 0);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        r.wait_idle().unwrap();
        let revision = r.navigator.revision;
        for step in 1..=30u8 {
            frame.time_seconds = f32::from(step) / 60.;
            crate::test_support::submit_ready(&mut r, frame).unwrap();
        }
        let published = r.navigator.revision - revision;
        assert!((1..=10).contains(&published), "half a second of animation republished the Navigator {published} times");
    }

    #[test]
    fn navigator_refreshes_current_contact_pages_without_replanning() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        submit_settled(&mut r, frame);
        let original = pixels(&r);
        let mut dab = crate::tests::test_dab([100., 100.], [0.9, 0.02, 0.1, 1.], 1.);
        dab.radii = [45.; 2];
        for (time, x) in [(0.01, 100.), (0.03, 150.), (0.06, 200.)] {
            dab.center.x = x;
            let batch = DabBatch { kind: DabBatchKind::Preview, ..crate::test_support::dab_batch(
                doc.working.target.unwrap(), crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen), dab.bounds()) };
            frame.time_seconds = time;
            let renders=RENDER_GRAPHS.with(std::cell::Cell::get);
            crate::test_support::submit_ready(&mut r, FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame }).unwrap();
            assert_eq!(RENDER_GRAPHS.with(std::cell::Cell::get),renders,
                "dab-carrying frame at {time}s must not render the Navigator graph");
        }
        assert_ne!(pixels(&r), original,"Navigator must refresh during the contact by 0.06s");
        assert!(!r.navigator.pending(),"the timer publication must be current during contact");
    }

    #[test]
    fn a_navigator_coarser_than_the_view_shows_the_stroke_during_contact() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.5, 0., 0., 0.5, 0., 0.];
        submit_settled(&mut r, frame);
        assert!(r.navigator.cache.as_ref().unwrap().plan.level>r.scale_display.as_ref().unwrap().plan.level,"the Navigator is coarser than the view");
        let original = pixels(&r);
        let mut dab = crate::tests::test_dab([100., 100.], [0.9, 0.02, 0.1, 1.], 1.);
        dab.radii = [45.; 2];
        for (time, x) in [(0.01, 100.), (0.03, 150.), (0.06, 200.), (0.12, 250.)] {
            dab.center.x = x;
            let batch = DabBatch { kind: DabBatchKind::Preview, ..crate::test_support::dab_batch(
                doc.working.target.unwrap(), crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen), dab.bounds()) };
            frame.time_seconds = time;
            crate::test_support::submit_ready(&mut r, FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame }).unwrap();
        }
        assert_ne!(pixels(&r), original,"the Navigator shows the stroke before pen-up");
        assert!(!r.navigator.pending(),"the contact publication is current");
    }

    #[test]
    fn navigator_keeps_its_picture_when_contact_projection_is_not_current() {
        let doc=document();let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame=packet(doc.scene(),doc.composition().size);frame.composite_all=false;
        frame.view.document_to_surface=[0.5,0.,0.,0.5,0.,0.];submit_settled(&mut r,frame);
        let original=pixels(&r);let revision=r.navigator.revision;
        let mut dab=crate::tests::test_dab([100.,100.],[0.9,0.02,0.1,1.],1.);dab.radii=[45.;2];
        let batch=DabBatch {kind:DabBatchKind::Preview,..crate::test_support::dab_batch(doc.working.target.unwrap(),
            crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds())};
        frame.time_seconds=0.06;let renders=RENDER_GRAPHS.with(std::cell::Cell::get);
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:&[dab],dab_batches:&[batch],..frame}).unwrap();
        assert_eq!(RENDER_GRAPHS.with(std::cell::Cell::get),renders,"a missing matching-level projection must not evaluate the graph during contact");
        assert!(r.navigator.view().is_some(),"the previous committed Navigator must remain presentable");
        if r.navigator.revision==revision {assert_eq!(pixels(&r),original,"an unpublishable projection must keep the previous picture");}
        else {assert!(!r.navigator.pending(),"a new publication must carry current matching-level witnesses");}
    }

    #[test]
    fn navigator_replacement_and_discarded_submission_never_publish_old_pixels() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let validation=r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        submit_settled(&mut r, frame);
        let original = pixels(&r);
        let mut discarded_doc=doc.clone();
        discarded_doc.artwork.occurrences.get_mut(doc.working.occurrence.unwrap()).unwrap().opacity=0.3;
        let discarded_frame=FramePacket {time_seconds:0.1,scene:discarded_doc.scene(),..frame};
        crate::scene::scale::tests::prepare_fixture_frame(&r,discarded_frame,r.scene.as_ref().unwrap());
        let mut navigator = std::mem::take(&mut r.navigator);
        navigator.prepare(&r,discarded_frame,r.scene.as_ref().unwrap(),r.scale_display.as_ref().unwrap().pages.clone(),false).unwrap();
        let revision=navigator.revision;
        let receipt;
        {
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            let before=crate::test_support::PassCounts::snapshot();
            crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder| {
                navigator.refresh(r,discarded_frame,false,encoder,&mut Scene::begin_work(false))?;
                if navigator.revision==revision {Err(GpuRasterError::NotReady)}else {Ok(())}
            });
            receipt=encoder.submission();
            assert!(crate::test_support::PassCounts::snapshot().delta(before).total>0,"the discarded publication must encode real GPU work");
            assert!(navigator.revision>revision,"the authored change must create a new publication");
            let cache=navigator.cache.as_ref().unwrap();
            assert_eq!(crate::test_support::published_coverage(crate::test_support::coverage_hash(&cache.geometry)).unwrap().receipt,receipt.observation_id());
            assert!(!receipt.discarded());
        }
        assert!(receipt.discarded(),"the actual publication receipt must remain discarded");
        assert!(navigator.pending());
        assert!(navigator.view().is_none());
        r.navigator = navigator;
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        submit_settled(&mut r, frame);
        assert!(!r.navigator.pending());
        assert_same_picture(&r, &pixels(&r), &original);
        let mut replacement_doc = doc.clone();
        let SourceTarget::Paint(paint) = replacement_doc.working.target.unwrap() else { unreachable!() };
        replacement_doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new((layer_core::color::source::rgba8_source([1025, 513], |_, _| [0, 0, 255, 255])).into()));
        let replacement = FramePacket { reset_layers: true, scene: replacement_doc.scene(), ..frame };
        submit_settled(&mut r, replacement);
        let changed = pixels(&r);
        assert!(changed.iter().all(|c| c[0] == 0. && c[1] == 0. && c[2] > 0.99 && c[3] == 1.));
        let mut recovered = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        submit_settled(&mut recovered, replacement);
        assert_eq!(pixels(&recovered), changed);
        assert!(pollster::block_on(validation.pop()).is_none());
    }

    #[test]
    fn navigator_checks_selected_signatures_without_an_invalidation_hint() {
        let mut doc=freshness_document();let extent=doc.composition().size;
        let owner=doc.working.occurrence.unwrap();let SourceTarget::Paint(paint)=doc.working.target.unwrap() else {unreachable!()};
        doc.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase::new(
            layer_core::color::source::rgba8_source(extent,|_,_|[180,70,30,255]).into()));
        doc.artwork.occurrences.get_mut(owner).unwrap().opacity=0.5;
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let frame=FramePacket {blend_space:doc.composition().blend,composite_all:false,..packet(doc.scene(),extent)};
        submit_settled(&mut r,frame);let previous=pixels(&r);let revision=r.navigator.revision;
        let mut changed=doc.clone();changed.artwork.occurrences.get_mut(owner).unwrap().opacity=0.25;
        let edit=FramePacket {time_seconds:0.1,scene:changed.scene(),..frame};
        let scene=r.scene.take().unwrap();scene.prepare_signatures(&r,edit).unwrap();r.scene=Some(scene);
        let mut navigator=std::mem::take(&mut r.navigator);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder| {
            navigator.refresh(r,edit,false,encoder,&mut Scene::begin_work(false))?;
            if navigator.pending() {Err(GpuRasterError::NotReady)}else {Ok(())}
        });
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        r.navigator=navigator;
        assert!(r.navigator.revision>revision,"a changed selected signature must publish without an invalidation hint");
        assert_ne!(pixels(&r),previous,"the changed opacity must reach actual Navigator pixels");
        let (error,first)=freshness_errors(&r,&changed);
        assert!(error[2]<2e-5,"Navigator must match the selected contents without an invalidation hint: {error:?}, first={first:?}");
    }

    #[test]
    fn a_navigator_frame_after_a_stroke_replans_only_the_cells_the_stroke_changed() {
        use super::super::tests::cell_keys::{assert_cached_keys_are_exact,begin_frame,cached_cells,expected,replanned,touched};
        use crate::test_support::dab_batch;
        let doc=super::super::tests::document_at([2048,1024]);let extent=doc.composition().size;let target=doc.working.target.unwrap();
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
        frame.view.width_px=2048;frame.view.height_px=1024;frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
        submit_settled(&mut r,frame);begin_frame(&mut r,frame);
        let content=r.navigator.cache.as_ref().unwrap().graph.content.clone().unwrap();
        let settled=cached_cells(&r,r.scale_display.as_ref().unwrap());let before=expected(&r);let revision=r.navigator.revision;
        let dab=crate::tests::test_dab([300.,300.],[0.9,0.1,0.3,1.],0.8);
        let batch=dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:1.,..frame}).unwrap();
        submit_settled(&mut r,FramePacket {time_seconds:2.,..frame});
        assert!(r.navigator.revision>revision,"the Navigator refreshes after the stroke");
        assert!(Arc::ptr_eq(r.navigator.cache.as_ref().unwrap().graph.content.as_ref().unwrap(),&content),"the Navigator keeps its expression identity across stroke frames");
        let cache=r.scale_display.as_ref().unwrap();let damaged=touched(&batch,&settled);
        assert!(!damaged.is_empty() && damaged.len()<settled.len());
        assert_eq!(replanned(cache,&settled,&before,&expected(&r)),damaged,"the Navigator frame re-plans only the {} cells the stroke changed",damaged.len());
        let refreshed=cached_cells(&r,cache);
        for (region,version) in &refreshed {assert_eq!(damaged.contains(region),settled[region].identity()!=version.identity(),"only the damaged cells change their key: {region:?}");}
        assert_cached_keys_are_exact(cache,&expected(&r),&refreshed);
        let cached=super::super::signatures::tests::cached_len(&expected(&r));
        begin_frame(&mut r,FramePacket {time_seconds:3.,..frame});
        assert_eq!(super::super::signatures::tests::cached_len(&expected(&r)),cached,"an unchanged Navigator frame reuses every cached key");
    }

    fn freshness_document()->Document {
        let extent=[1536,768];
        let mut doc=Document::new(layer_core::authored::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Current ink".into(),paper:"Paper".into()});
        let SourceTarget::Paint(paint)=doc.working.target.unwrap() else {unreachable!()};
        doc.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase::new(
            layer_core::color::source::rgba8_source(extent,|x,y|[(x/5) as u8,(y/3) as u8,40,255]).into()));
        let root=doc.artwork.root;
        let composition=doc.artwork.compositions.get_mut(root).unwrap();
        composition.color.depth=layer_core::color::SampleDepth::F32;
        composition.blend=layer_core::BlendSpace::Linear;doc
    }

    fn freshness_reference(doc:&Document)->Vec<[f32;4]> {
        let reference=freshness_reference_renderer(doc);
        float_pixels(&reference,crate::test_support::document_texture(&reference))
    }

    fn freshness_reference_renderer(doc:&Document)->WgpuRasterizer {
        let mut reference=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        reference.test.reference=true;
        submit_settled(&mut reference,FramePacket {blend_space:doc.composition().blend,
            ..packet(doc.scene(),doc.composition().size)});
        crate::test_support::complete(&reference);reference
    }

    fn freshness_difference(actual:&[[f32;4]],expected:&[[f32;4]],plan:display_mips::Plan,format:wgpu::TextureFormat)
        ->Option<([u32;2],[f32;4],[f32;4])> {
        freshness_mismatches(actual,expected,plan,format).0
    }

    fn freshness_mismatches(actual:&[[f32;4]],expected:&[[f32;4]],plan:display_mips::Plan,format:wgpu::TextureFormat)
        ->(Option<([u32;2],[f32;4],[f32;4])>,[usize;2]) {
        let side=1<<plan.level;
        let mut first=None;let mut halves=[0;2];
        for y in 0..plan.size[1] {for x in 0..plan.size[0] {
            let mut sum=[0.;4];let mut count=0.;
            for yy in plan.bounds.min_y()+y*side..(plan.bounds.min_y()+(y+1)*side).min(plan.bounds.max_y()) {
                for xx in plan.bounds.min_x()+x*side..(plan.bounds.min_x()+(x+1)*side).min(plan.bounds.max_x()) {
                    for c in 0..4 {sum[c]+=expected[(yy*plan.extent[0]+xx) as usize][c];}count+=1.;
                }
            }
            let expected=sum.map(|value|value/count);
            let expected=match format {wgpu::TextureFormat::Rgba16Float=>expected.map(|value|layer_core::color::f16::from_f32(value).to_f32()),
                wgpu::TextureFormat::Rgba32Float=>expected,format=>panic!("unexpected actual format: {format:?}")};
            let actual=actual[(y*plan.size[0]+x) as usize];
            if actual.iter().zip(expected).any(|(a,b)|(*a-b).abs()>=2e-5) {
                first.get_or_insert(([x,y],actual,expected));
                halves[usize::from(plan.bounds.min_x()+x*side>=4096)]+=1;
            }
        }}
        (first,halves)
    }

    fn freshness_errors(r:&WgpuRasterizer,doc:&Document)->([f32;3],Option<([u32;2],[f32;4],[f32;4])>) {
        let expected=freshness_reference(doc);
        let cache=r.navigator.cache.as_ref().expect("Navigator must publish an image");
        let actual=pixels(r);let format=cache.texture().format();
        (crate::scene::scale::tests::quality(&actual,&expected,cache.plan,format),freshness_difference(&actual,&expected,cache.plan,format))
    }

    fn settle_freshness_engine(engine:&mut layer_engine::CanvasEngine<WgpuRasterizer>) {
        let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
        loop {
            engine.render_frame().unwrap();crate::test_support::complete(engine.backend());
            if !engine.has_pending_input() && !engine.has_pending_document_edits() && engine.backend().next_frame()!=layer_render::FrameDemand::Work {break;}
            assert!(std::time::Instant::now()<deadline,"Navigator/history work must settle under its existing deadline");
            std::thread::yield_now();
        }
    }

    #[test]
    fn navigator_freshness_matches_selected_stroke_history_and_layer_moves() {
        use layer_engine::{CanvasEngine,PenPhase,SampleFlags,ViewTransform,input_queue};
        let doc=freshness_document();let extent=doc.composition().size;
        let r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let (mut input,consumer)=input_queue(64);
        let view=layer_render::ViewState {width_px:512,height_px:256,document_to_surface:[0.5,0.,0.,0.5,0.,0.]};
        let mut engine=CanvasEngine::new(r,doc,consumer,view,ViewTransform {revision:1,surface_to_document:[2.,0.,0.,2.,0.,0.]}).unwrap();
        engine.set_brush(layer_core::BrushSnapshot {diameter:96.,mappings:Arc::from([]),
            ..layer_core::default_brush(layer_core::DefaultBrushPreset::GPen)}).unwrap();
        let mut failures=Vec::new();
        let check=|label:&str,engine:&CanvasEngine<WgpuRasterizer>,failures:&mut Vec<String>| {
            let (error,first)=freshness_errors(engine.backend(),engine.document());
            if error[2]>=2e-5 {failures.push(format!("{label}: Navigator/fresh-selected-native error={error:?}, first(xy,actual,expected)={first:?}"));}
        };
        settle_freshness_engine(&mut engine);check("open",&engine,&mut failures);
        for (index,phase) in [PenPhase::Down,PenPhase::Move,PenPhase::Up].into_iter().enumerate() {
            let mut event=crate::test_support::pen((index+1) as u64,phase,[72.+index as f32*28.,64.],SampleFlags::PRIMARY);
            event.view_revision=1;input.push(event).unwrap();
            engine.render_frame().unwrap();crate::test_support::complete(engine.backend());
        }
        settle_freshness_engine(&mut engine);check("stroke",&engine,&mut failures);
        assert!(engine.undo().unwrap());settle_freshness_engine(&mut engine);check("undo stroke",&engine,&mut failures);
        assert!(engine.redo().unwrap());settle_freshness_engine(&mut engine);check("redo stroke",&engine,&mut failures);
        let owner=engine.document().working.occurrence.unwrap();
        let edit=engine.document().move_layers_edit(&[owner],[128,64]).unwrap();
        engine.apply_edit(edit).unwrap();settle_freshness_engine(&mut engine);check("move layer",&engine,&mut failures);
        assert!(engine.undo().unwrap());settle_freshness_engine(&mut engine);check("undo layer move",&engine,&mut failures);
        assert_eq!(engine.document().composition().size,extent);
        assert!(failures.is_empty(),"{}",failures.join("\n"));
    }

    #[test]
    fn navigator_matches_a_fresh_renderer_after_each_stroke_and_undo_at_fit() {
        use layer_engine::{CanvasEngine,PenPhase,SampleFlags,ViewTransform,input_queue};
        let mut doc=freshness_document();
        let root=doc.artwork.root;doc.artwork.compositions.get_mut(root).unwrap().color.depth=layer_core::color::SampleDepth::U8;
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let (wake,woken)=std::sync::mpsc::channel();let wake=std::sync::Mutex::new(wake);
        r.set_preparation_wake(Arc::new(move||{let _=wake.lock().unwrap().send(());}));
        let (mut input,consumer)=input_queue(64);
        let view=layer_render::ViewState {width_px:384,height_px:192,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
        let mut engine=CanvasEngine::new(r,doc,consumer,view,ViewTransform {revision:1,surface_to_document:[4.,0.,0.,4.,0.,0.]}).unwrap();
        engine.set_brush(layer_core::BrushSnapshot {diameter:96.,mappings:Arc::from([]),
            ..layer_core::default_brush(layer_core::DefaultBrushPreset::Paintbrush)}).unwrap();
        let clock=std::cell::Cell::new(1_000_000_000u64);
        let frame=|engine:&mut CanvasEngine<WgpuRasterizer>| {
            clock.set(clock.get()+8_333_333);engine.render_frame_at(clock.get()).unwrap();crate::test_support::complete(engine.backend());
        };
        let settle=|engine:&mut CanvasEngine<WgpuRasterizer>| {
            let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
            loop {
                frame(engine);
                if (engine.has_pending_input() || engine.has_pending_document_edits() || engine.backend().next_frame()==layer_render::FrameDemand::Work)
                    || woken.recv_timeout(std::time::Duration::from_millis(500)).is_ok() {
                    while woken.try_recv().is_ok() {}
                    assert!(std::time::Instant::now()<deadline,"work must settle");continue;
                }
                break;
            }
        };
        let mut failures=Vec::new();
        let check=|label:&str,engine:&CanvasEngine<WgpuRasterizer>,failures:&mut Vec<String>| {
            assert!(engine.backend().scale_display.as_ref().unwrap().overview.is_none(),"the view at Fit has no overview");
            let (error,first)=freshness_errors(engine.backend(),engine.document());
            if error[2]>=2e-5 {failures.push(format!("{label}: Navigator/fresh error={error:?}, first(xy,actual,expected)={first:?}"));}
        };
        settle(&mut engine);check("open",&engine,&mut failures);
        let mut sequence=0;
        for stroke in 0..3u8 {
            let y=40.+f32::from(stroke)*48.;
            for (index,phase) in [PenPhase::Down,PenPhase::Move,PenPhase::Move,PenPhase::Up].into_iter().enumerate() {
                sequence+=1;
                let mut event=crate::test_support::pen(sequence,phase,[60.+index as f32*80.,y],SampleFlags::PRIMARY);
                event.view_revision=1;input.push(event).unwrap();
                frame(&mut engine);
            }
            settle(&mut engine);check(&format!("stroke {stroke}"),&engine,&mut failures);
        }
        assert!(engine.undo().unwrap());settle(&mut engine);check("undo",&engine,&mut failures);
        assert!(engine.redo().unwrap());settle(&mut engine);check("redo",&engine,&mut failures);
        assert!(engine.undo().unwrap());assert!(engine.undo().unwrap());settle(&mut engine);check("undo twice",&engine,&mut failures);
        assert!(failures.is_empty(),"{}",failures.join("\n"));
    }

    fn pictures(r:&WgpuRasterizer)->Vec<(&'static str,display_mips::Plan,Vec<[f32;4]>)> {
        let cache=r.scale_display.as_ref().unwrap();
        let mut out=vec![("view",cache.plan,crate::scene::scale::tests::display_pixels(r))];
        if let Some(overview)=cache.overview.as_deref() {
            out.push(("overview",overview.plan,crate::scene::scale::tests::window_pixels(r,overview.pixels.root().unwrap(),overview.plan)));
        }
        out.push(("navigator",r.navigator.cache.as_ref().unwrap().plan,pixels(r)));
        out
    }

    fn masked_stack()->(Document,OccurrenceHandle,OccurrenceHandle) {
        let mut base=document();let extent=base.composition().size;let working=base.working.occurrence.unwrap();
        let above=crate::scene::scale::tests::paint_occurrence(&mut base,"Above",Some(layer_core::color::source::rgba8_source(extent,|x,y|[90,(x/7) as u8,(y/2) as u8,120])));
        crate::scene::scale::tests::insert_occurrence(&mut base,above,1);
        crate::scene::scale::tests::coverage_mask(&mut base,working,[0,0],Some(layer_core::Selection::polygon(vec![
            Point {x:60.,y:40.},Point {x:420.,y:40.},Point {x:420.,y:300.},Point {x:60.,y:300.}]).unwrap()));
        base.artwork.occurrences.get_mut(working).unwrap().mask.as_mut().unwrap().enabled=false;
        (base,working,above)
    }
    fn contact_pictures(prepared:Option<&Document>,start:&Document,later:&Document)->Vec<Vec<(&'static str,display_mips::Plan,Vec<[f32;4]>)>> {
        let extent=start.composition().size;
        let mut r=WgpuRasterizer::new_native_headless(start.composition().color).unwrap();
        let mut frame=packet(start.scene(),extent);frame.composite_all=false;
        frame.view.document_to_surface=[0.5,0.,0.,0.5,0.,0.];
        if let Some(prepared)=prepared {submit_settled(&mut r,FramePacket {scene:prepared.scene(),..frame});}
        submit_settled(&mut r,frame);
        let mut dab=crate::tests::test_dab([100.,100.],[0.9,0.02,0.1,1.],1.);dab.radii=[45.;2];
        let mut frames=Vec::new();
        for (step,x) in [100.,150.,200.,250.,300.].into_iter().enumerate() {
            dab.center.x=x;
            let batch=DabBatch {stroke_start:step==0,stroke_end:false,..crate::test_support::dab_batch(
                start.working.target.unwrap(),crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds())};
            let document=if step==0 {start}else {later};
            crate::test_support::submit_ready(&mut r,FramePacket {scene:document.scene(),dabs:&[dab],dab_batches:&[batch],time_seconds:0.06*(step+1) as f32,..frame}).unwrap();
            frames.push(pictures(&r));
        }
        frames
    }
    fn picture_error(actual:&[[f32;4]],expected:&[[f32;4]])->f32 {
        actual.iter().zip(expected).flat_map(|(a,b)|a.iter().zip(b).map(|(a,b)|(a-b).abs())).fold(0f32,f32::max)
    }

    #[test]
    fn every_contact_frame_after_a_scene_change_matches_the_same_change_made_before_contact() {
        let (base,working,above)=masked_stack();
        let changes:[(&str,OccurrenceHandle,fn(&mut layer_core::authored::Occurrence));3]=[
            ("opacity of the stroked layer",working,|occurrence|occurrence.opacity=0.4),
            ("visibility of the layer above",above,|occurrence|occurrence.visible=false),
            ("mask of the stroked layer",working,|occurrence|occurrence.mask.as_mut().unwrap().enabled=true)];
        for (label,id,change) in changes {
            let mut changed=base.clone();change(changed.artwork.occurrences.get_mut(id).unwrap());
            let during=contact_pictures(Some(&changed),&base,&changed);let before=contact_pictures(None,&changed,&changed);
            for (step,(during,before)) in during.iter().zip(&before).enumerate().skip(1) {
                assert_eq!(during.len(),before.len(),"{label}: frame {} has the same pictures",step+1);
                for ((name,plan,actual),(_,expected_plan,expected)) in during.iter().zip(before) {
                    assert_eq!(plan,expected_plan);
                    let error=picture_error(actual,expected);
                    assert!(error<5e-6,"{label}: contact frame {} {name} differs from the change made before contact by {error}",step+1);
                }
            }
        }
    }

    #[test]
    fn a_contact_change_waiting_on_new_inputs_never_mixes_revisions_and_lands_before_the_stroke_ends() {
        let (base,working,_)=masked_stack();
        let mut changed=base.clone();changed.artwork.occurrences.get_mut(working).unwrap().mask.as_mut().unwrap().enabled=true;
        let during=contact_pictures(None,&base,&changed);let before=contact_pictures(None,&changed,&changed);
        let view=|frame:&Vec<(&'static str,display_mips::Plan,Vec<[f32;4]>)>|frame[0].2.clone();
        let mut landed=None;
        for (step,frame) in during.iter().enumerate().skip(1) {
            let [held,new]=[&during[step-1],&before[step]].map(|expected|picture_error(&view(frame),&view(expected)));
            assert!(held<5e-6 || new<5e-6,"contact frame {step} mixes revisions: {held} from the held picture, {new} from the changed one");
            if new<5e-6 {landed.get_or_insert(step);}else {assert!(landed.is_none(),"contact frame {step} returns to the held picture");}
        }
        assert!(landed.is_some(),"the change lands before the stroke ends");
    }

    #[test]
    fn view_overview_and_navigator_match_a_fresh_renderer_through_strokes_undo_redo_and_cancel() {
        use layer_engine::{CanvasEngine,PenPhase,SampleFlags,ViewTransform,input_queue};
        let signature_frame=|engine:&CanvasEngine<WgpuRasterizer>|engine.backend().scene.as_ref().and_then(|scene|scene.scale_pages.lock().unwrap().expected().cloned());
        for (scale,mask) in [(0.25f32,false),(0.5,false),(2.,false),(2.,true)] {
            let mut doc=freshness_document();
            if mask {
                let working=doc.working.occurrence.unwrap();let mask=crate::scene::scale::tests::coverage_mask(&mut doc,working,[0,0],None);
                doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.5;doc.working.target=Some(SourceTarget::Coverage(mask));
            }
            let root=doc.artwork.root;doc.artwork.compositions.get_mut(root).unwrap().color.depth=layer_core::color::SampleDepth::U8;
            let extent=doc.composition().size;
            let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            let (wake,woken)=std::sync::mpsc::channel();let wake=std::sync::Mutex::new(wake);
            r.set_preparation_wake(Arc::new(move||{let _=wake.lock().unwrap().send(());}));
            let (mut input,consumer)=input_queue(64);
            let view=layer_render::ViewState {width_px:384,height_px:192,document_to_surface:[scale,0.,0.,scale,0.,0.]};
            let mut engine=CanvasEngine::new(r,doc,consumer,view,ViewTransform {revision:1,surface_to_document:[1./scale,0.,0.,1./scale,0.,0.]}).unwrap();
            engine.set_brush(layer_core::BrushSnapshot {diameter:64.,mappings:Arc::from([]),
                ..layer_core::default_brush(layer_core::DefaultBrushPreset::Paintbrush)}).unwrap();
            let clock=std::cell::Cell::new(1_000_000_000u64);
            let frame=|engine:&mut CanvasEngine<WgpuRasterizer>| {
                clock.set(clock.get()+8_333_333);engine.render_frame_at(clock.get()).unwrap();crate::test_support::complete(engine.backend());
                if let Some(expected)=signature_frame(engine) {super::super::signatures::tests::assert_authored_versions_are_exact(&expected);}
            };
            let settle=|engine:&mut CanvasEngine<WgpuRasterizer>| {
                let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
                loop {
                    frame(engine);
                    if engine.has_pending_input() || engine.has_pending_document_edits() || engine.backend().next_frame()==layer_render::FrameDemand::Work
                        || woken.recv_timeout(std::time::Duration::from_millis(300)).is_ok() {
                        while woken.try_recv().is_ok() {}
                        assert!(std::time::Instant::now()<deadline,"work must settle");continue;
                    }
                    break;
                }
            };
            let mut failures=Vec::new();
            let mut compare=|label:&str,engine:&CanvasEngine<WgpuRasterizer>| {
                let mut fresh=WgpuRasterizer::new_native_headless(engine.document().composition().color).unwrap();
                submit_settled(&mut fresh,FramePacket {composite_all:false,view,blend_space:engine.document().composition().blend,..packet(engine.document().scene(),extent)});
                let [actual,expected]=[engine.backend(),&fresh].map(pictures);
                assert_eq!(actual.iter().map(|(name,plan,_)|(*name,*plan)).collect::<Vec<_>>(),expected.iter().map(|(name,plan,_)|(*name,*plan)).collect::<Vec<_>>(),"{label} at {scale} mask={mask}: the same windows");
                for ((name,_,actual),(_,_,expected)) in actual.iter().zip(&expected) {
                    let error=actual.iter().zip(expected).flat_map(|(a,b)|a.iter().zip(b).map(|(a,b)|(a-b).abs())).fold(0f32,f32::max);
                    if error>2e-3 {failures.push(format!("{label} at {scale} mask={mask}: {name} differs from a fresh renderer by {error}"));}
                }
            };
            settle(&mut engine);compare("open",&engine);
            let opened=pictures(engine.backend()).swap_remove(0).2;
            let mut sequence=0;
            let authored=|engine:&CanvasEngine<WgpuRasterizer>| {
                let frame=signature_frame(engine)?;let graph=&engine.backend().scale_display.as_ref()?.graph;
                let content=graph.content.as_ref().or(graph.root.as_ref())?;
                Some((SignatureFrame::from_snapshot(frame.snapshot()).authored_revision(content).ok()?,frame))
            };
            let mut stroke=|engine:&mut CanvasEngine<WgpuRasterizer>,y:f32,end:PenPhase| {
                for (index,phase) in [PenPhase::Down,PenPhase::Move,PenPhase::Move,end].into_iter().enumerate() {
                    sequence+=1;
                    let mut event=crate::test_support::pen(sequence,phase,[60.+index as f32*60.,y],SampleFlags::PRIMARY);
                    event.view_revision=1;input.push(event).unwrap();
                    let before=authored(engine);
                    frame(engine);
                    if let (Some((before,previous)),Some((after,next)))=(before,authored(engine)) {
                        assert!(Arc::ptr_eq(&previous,&next) || before!=after,"a stroke frame with a new signature frame changes the root's authored version");
                    }
                }
            };
            stroke(&mut engine,40.,PenPhase::Up);settle(&mut engine);compare("stroke",&engine);
            assert!(picture_error(&pictures(engine.backend())[0].2,&opened)>0.05,"the stroke at {scale} mask={mask} shows in the view");
            stroke(&mut engine,90.,PenPhase::Up);settle(&mut engine);compare("second stroke",&engine);
            assert!(engine.undo().unwrap());settle(&mut engine);compare("undo",&engine);
            stroke(&mut engine,140.,PenPhase::Cancel);settle(&mut engine);compare("cancel",&engine);
            engine.redo().unwrap();settle(&mut engine);compare("redo",&engine);
            assert!(failures.is_empty(),"{}",failures.join("\n"));
        }
    }

    #[test]
    fn a_zoom_in_mid_contact_leaves_no_live_contact_after_pen_up_and_the_overview_and_navigator_follow_later_edits() {
        use layer_engine::{CanvasEngine,PenEvent,PenPhase,SampleFlags,ViewTransform,input_queue};
        let extent=[2048,1024];let mut doc=crate::scene::scale::tests::document_at(extent);
        let other=crate::scene::scale::tests::paint_occurrence(&mut doc,"Other",Some(layer_core::color::source::rgba8_source(extent,|x,_|[20,(x/9) as u8,200,90])));
        crate::scene::scale::tests::insert_occurrence(&mut doc,other,0);
        let camera=|scale:f32,offset:[f32;2],size:[u32;2],revision:u64|(layer_render::ViewState {width_px:size[0],height_px:size[1],document_to_surface:[scale,0.,0.,scale,offset[0],offset[1]]},
            ViewTransform {revision,surface_to_document:[1./scale,0.,0.,1./scale,-offset[0]/scale,-offset[1]/scale]});
        let fit=camera(0.25,[0.;2],[512,256],1);let zoomed=camera(1.,[-200.;2],[512,256],2);
        let (mut input,consumer)=input_queue(64);
        let mut engine=CanvasEngine::new(WgpuRasterizer::new_native_headless(doc.composition().color).unwrap(),doc,consumer,fit.0,fit.1).unwrap();
        engine.set_brush(layer_core::BrushSnapshot {diameter:64.,..layer_core::default_brush(layer_core::DefaultBrushPreset::GPen)}).unwrap();
        let clock=std::cell::Cell::new(1_000_000_000u64);
        let frame=|engine:&mut CanvasEngine<WgpuRasterizer>| {clock.set(clock.get()+8_333_333);engine.render_frame_at(clock.get()).unwrap();engine.backend_mut().wait_idle().unwrap();};
        let settle=|engine:&mut CanvasEngine<WgpuRasterizer>| {
            let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
            loop {
                frame(engine);
                if !engine.has_pending_input() && !engine.has_pending_document_edits() && engine.backend().next_frame()!=layer_render::FrameDemand::Work {break;}
                assert!(std::time::Instant::now()<deadline,"work must settle");
            }
        };
        let mut pen=|engine:&mut CanvasEngine<WgpuRasterizer>,step:u64,phase:PenPhase,x:f32,(view,transform):(layer_render::ViewState,ViewTransform)| {
            let [a,_,_,d,tx,ty]=view.document_to_surface;
            input.push(PenEvent {timestamp_ns:clock.get()+8_333_333,view_revision:transform.revision,..crate::test_support::pen(step,phase,[a*x+tx,d*300.+ty],SampleFlags::PRIMARY)}).unwrap();
            frame(engine);
            while engine.has_pending_input() || engine.has_pending_document_edits() {frame(engine);}
        };
        settle(&mut engine);
        let fit_plan=engine.backend().scale_display.as_ref().unwrap().plan;
        for (step,x) in [300.,340.,380.].into_iter().enumerate() {pen(&mut engine,step as u64+1,if step==0 {PenPhase::Down}else {PenPhase::Move},x,fit);}
        assert!(engine.backend().scale_display.as_ref().unwrap().contact_id().is_some(),"the fit view is in contact");
        engine.set_view(zoomed.0,zoomed.1);
        for (step,x) in [420.,460.].into_iter().enumerate() {pen(&mut engine,step as u64+4,PenPhase::Move,x,zoomed);}
        assert_eq!(engine.backend().scale_display.as_ref().unwrap().overview.as_ref().unwrap().plan,fit_plan,"the zoomed-in view keeps the fit cache as its overview");
        pen(&mut engine,6,PenPhase::Up,500.,zoomed);
        settle(&mut engine);
        let display=engine.backend().scale_display.as_ref().unwrap();
        assert!(std::iter::once(display).chain(display.overview.as_deref()).all(|cache|cache.contact_id().is_none()),"no cache holds a live contact after pen-up");
        let mut failures=Vec::new();
        let mut compare=|label:&str,engine:&CanvasEngine<WgpuRasterizer>| {
            let mut fresh=WgpuRasterizer::new_native_headless(engine.document().composition().color).unwrap();
            submit_settled(&mut fresh,FramePacket {composite_all:false,view:engine.view(),blend_space:engine.document().composition().blend,..packet(engine.document().scene(),extent)});
            for ((name,_,actual),(_,_,expected)) in pictures(engine.backend()).iter().zip(&pictures(&fresh)) {
                let error=picture_error(actual,expected);
                if error>2e-3 {failures.push(format!("the {name} {label} differs from a fresh renderer by {error}"));}
            }
        };
        compare("after pen-up",&engine);
        engine.set_layer_opacity(other,0.4).unwrap();settle(&mut engine);
        let panned=camera(1.,[-900.,-200.],[512,256],3);engine.set_view(panned.0,panned.1);settle(&mut engine);
        compare("after an edit to another layer and a pan",&engine);
        assert!(engine.undo().unwrap());settle(&mut engine);
        engine.set_view(zoomed.0,ViewTransform {revision:4,..zoomed.1});settle(&mut engine);
        compare("after undo and a pan",&engine);
        assert!(failures.is_empty(),"{}",failures.join("\n"));
    }

    #[test]
    fn navigator_freshness_covers_the_far_half_of_an_8192_pixel_drawing() {
        let extent=[8192,768];let mut doc=freshness_document();
        let mut artwork=layer_core::authored::Artwork::new(extent).unwrap();
        let (owner,target)=crate::test_support::add_paint(&mut artwork,"Far-half ink",extent);
        let SourceTarget::Paint(paint)=target else {unreachable!()};
        artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase::new(
            layer_core::color::source::rgba8_source(extent,|x,y|if x>=4096 {[231,(y%127) as u8,19,255]}else {[11,31,97,255]}).into()));
        let root=artwork.root;artwork.compositions.get_mut(root).unwrap().color=doc.composition().color;
        artwork.compositions.get_mut(root).unwrap().blend=layer_core::BlendSpace::Linear;
        let stack=artwork.compositions.get(root).unwrap().result;artwork.stacks.get_mut(stack).unwrap().entries=vec![owner];
        doc=Document::from_artwork(artwork).unwrap();
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        submit_settled(&mut r,FramePacket {blend_space:doc.composition().blend,..packet(doc.scene(),extent)});
        let expected=freshness_reference(&doc);
        let display=r.scale_display.as_ref().unwrap();
        for (label,cache) in std::iter::once(("main",display)).chain(display.overview.as_deref().map(|overview|("overview",overview))) {
            let actual=float_pixels(&r,cache.texture());
            let error=crate::scene::scale::tests::quality(&actual,&expected,cache.plan,cache.texture().format());
            assert!(error[2]<2e-5,"8192-wide {label} must cover matching pixels at document x>=4096: {error:?}, first={:?}",freshness_difference(&actual,&expected,cache.plan,cache.texture().format()));
        }
        let mut presenter=crate::test_support::float_presenter(&r);
        let (texture,target)=create_color_target(&r.device,extent,"wide root publication oracle");
        presenter.present(&r,&target,crate::test_support::view(extent),[0.;4]).unwrap();
        let shown=float_pixels(&r,&texture);
        let reference=freshness_reference_renderer(&doc);
        let mut reference_presenter=crate::test_support::float_presenter(&reference);
        let (reference_texture,reference_target)=create_color_target(&reference.device,extent,"wide fresh presented reference");
        reference_presenter.present(&reference,&reference_target,crate::test_support::view(extent),[0.;4]).unwrap();
        let presented_expected=float_pixels(&reference,&reference_texture);
        assert!(crate::test_support::max_error(&shown,&presented_expected)<2e-5,"presented root must cover the far half of the8192-wide drawing, first={:?}",freshness_difference(&shown,&presented_expected,display_mips::Plan::at(extent,0),texture.format()));
        let cache=r.navigator.cache.as_ref().unwrap();assert_eq!(cache.plan.extent,extent);
        let actual=pixels(&r);let right=cache.plan.size[0]/2;
        assert!((0..cache.plan.size[1]).all(|y|(right..cache.plan.size[0]).all(|x|actual[(y*cache.plan.size[0]+x) as usize][0]>0.5)),
            "Navigator must cover document x>=4096, not leave the far half empty or stale");
        let error=crate::scene::scale::tests::quality(&actual,&expected,cache.plan,cache.texture().format());
        let (first,halves)=freshness_mismatches(&actual,&expected,cache.plan,cache.texture().format());
        let first_document=first.map(|(at,_,_)|[cache.plan.bounds.min_x()+(at[0]<<cache.plan.level),cache.plan.bounds.min_y()+(at[1]<<cache.plan.level)]);
        assert!(error[2]<2e-5,"8192-wide Navigator must match selected contents over its whole coverage: {error:?}, first(navigator_xy,actual,expected)={first:?}, first_document_xy={first_document:?}, mismatching_pixels(x<4096,x>=4096)={halves:?}");
    }

    fn replaced_base()->(Document,Document,Arc<layer_core::color::source::SourceImage>) {
        let extent=[512,256];
        let mut doc=Document::new(layer_core::authored::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Revision window".into(),paper:"Paper".into()});
        let target=doc.working.target.unwrap();let owner=doc.working.occurrence.unwrap();
        let SourceTarget::Paint(paint)=target else {unreachable!()};
        let root=doc.artwork.root;
        let composition=doc.artwork.compositions.get_mut(root).unwrap();
        composition.color.depth=layer_core::color::SampleDepth::F32;
        composition.blend=layer_core::BlendSpace::Linear;
        let stack=composition.result;doc.artwork.stacks.get_mut(stack).unwrap().entries=vec![owner];
        doc.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase::new(
            layer_core::color::source::rgba8_source(extent,|_,_|[11,31,197,255]).into()));
        let replacement=layer_core::color::source::rgba8_source(extent,|_,_|[231,19,47,255]);
        let mut changed=doc.clone();changed.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase::new(replacement.clone().into()));
        (doc,changed,replacement)
    }
    fn prepare_first_replacement_tile(r:&mut WgpuRasterizer,replacement:&Arc<layer_core::color::source::SourceImage>)->crate::test_support::GatedStore {
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let pixels=crate::prepare_image_tile(replacement,[0;2],r.device.working_space(),&r.device.source_samples,None).unwrap();
        let tile=Scene::decode_prepared_for(r,&mut encoder,replacement,[0;2],pixels,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
        Scene::promote_source(r,&mut encoder,tile).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(r);
        crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([replacement.as_ref()]),false)
    }

    #[test]
    fn an_authored_window_does_not_wait_for_a_margin_cell_outside_the_view() {
        let (doc,changed,replacement)=replaced_base();let extent=doc.composition().size;
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame=FramePacket {blend_space:doc.composition().blend,composite_all:false,..packet(doc.scene(),extent)};
        frame.view.width_px=256;frame.view.height_px=256;
        submit_settled(&mut r,frame);crate::test_support::complete(&r);
        assert!(r.scale_display.as_ref().unwrap().plan.bounds==PixelRect::full(extent),"the view's plan keeps the second cell as margin");
        let mut presenter=crate::test_support::float_presenter(&r);
        let (output,view)=create_color_target(&r.device,[256,256],"margin-cell authored publication");
        let store=prepare_first_replacement_tile(&mut r,&replacement);
        let next=FramePacket {scene:changed.scene(),..frame};
        crate::test_support::submit_ready(&mut r,next).unwrap();crate::test_support::complete(&r);store.gate.entered();
        crate::test_support::submit_ready(&mut r,next).unwrap();crate::test_support::complete(&r);
        presenter.present(&r,&view,next.view,[0.;4]).unwrap();
        let shown=float_pixels(&r,&output);
        store.gate.release();
        let fresh=freshness_reference_renderer(&changed);let mut fresh_presenter=crate::test_support::float_presenter(&fresh);
        let (fresh_output,fresh_view)=create_color_target(&fresh.device,[256,256],"margin-cell fresh publication");
        fresh_presenter.present(&fresh,&fresh_view,next.view,[0.;4]).unwrap();let reference=float_pixels(&fresh,&fresh_output);
        assert!(shown[0].iter().zip(reference[0]).all(|(a,b)|(a-b).abs()<2e-5),
            "the visible cell presents the new revision although the margin cell is gated: {:?} {:?}",shown[0],reference[0]);
    }

    #[test]
    fn a_held_decode_blocks_display_frames_until_its_completion_wakes_one() {
        use layer_render::{CanvasRenderer,FrameDemand};
        let (doc,changed,replacement)=replaced_base();let extent=doc.composition().size;
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let (wake,woken)=std::sync::mpsc::channel();let wake=std::sync::Mutex::new(wake);
        r.set_preparation_wake(Arc::new(move||{let _=wake.lock().unwrap().send(());}));
        let frame=FramePacket {blend_space:doc.composition().blend,composite_all:false,..packet(doc.scene(),extent)};
        submit_settled(&mut r,frame);crate::test_support::complete(&r);
        let store=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([replacement.as_ref()]),false);
        let next=FramePacket {scene:changed.scene(),time_seconds:frame.time_seconds+0.1,..frame};
        crate::test_support::submit_ready(&mut r,next).unwrap();crate::test_support::complete(&r);store.gate.entered();
        let start=std::time::Instant::now();let mut idle=0;
        while start.elapsed()<std::time::Duration::from_secs(2) {
            if r.next_frame()==FrameDemand::Work {idle+=1;crate::test_support::submit_ready(&mut r,next).unwrap();crate::test_support::complete(&r);}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(idle,0,"a display waiting on a held decode runs no frames");
        while woken.try_recv().is_ok() {}
        store.gate.release();
        woken.recv_timeout(std::time::Duration::from_secs(10)).expect("the decode's completion wakes the host");
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);let mut woke=0;let mut approximate=false;
        while r.next_frame()==FrameDemand::Work {
            woke+=1;crate::test_support::submit_ready(&mut r,next).unwrap();crate::test_support::complete(&r);
            if woke==1 {
                let cache=r.scale_display.as_ref().unwrap();approximate=!cache.output_complete();
                assert_eq!(cache.presented_matches_expected(),Some(true),"the decode's completion presents the new revision in its first frame");
            }
            assert!(std::time::Instant::now()<deadline,"the woken display settles");
        }
        assert_eq!(woke,1+usize::from(approximate),"the decode's completion wakes the frame that presents it, and one refinement frame if that frame is approximate");
    }

    #[test]
    fn a_root_window_keeps_its_previous_revision_until_every_visible_page_matches() {
        let (doc,changed,replacement)=replaced_base();let extent=doc.composition().size;
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let frame=FramePacket {blend_space:doc.composition().blend,composite_all:false,..packet(doc.scene(),extent)};
        submit_settled(&mut r,frame);crate::test_support::complete(&r);
        let mut presenter=crate::test_support::float_presenter(&r);
        let (output,view)=create_color_target(&r.device,extent,"whole-revision root publication");
        presenter.present(&r,&view,frame.view,[0.;4]).unwrap();let previous=float_pixels(&r,&output);
        let cache=r.scale_display.as_ref().unwrap();
        let receipt=crate::test_support::published_coverage(crate::test_support::coverage_hash(&cache.geometry)).unwrap().receipt;
        let store=prepare_first_replacement_tile(&mut r,&replacement);
        let next=FramePacket {scene:changed.scene(),..frame};
        crate::test_support::submit_ready(&mut r,next).unwrap();crate::test_support::complete(&r);store.gate.entered();
        for index in 0..=2 {
            if index!=0 {crate::test_support::submit_ready(&mut r,next).unwrap();crate::test_support::complete(&r);}
            let cache=r.scale_display.as_ref().unwrap();let coverage=cache.coverage_snapshot();
            assert!(coverage.ready(PixelRect::new(0,0,256,256),pages::Accuracy::Display),"the fixture must commit one real page of the new revision");
            assert!(!coverage.ready(PixelRect::new(256,0,512,256),pages::Accuracy::Display),"the gated page must remain owed");
            presenter.present(&r,&view,frame.view,[0.;4]).unwrap();
            assert!(crate::test_support::max_error(&float_pixels(&r,&output),&previous)<2e-5,
                "an authored root window must stay wholly previous while any visible page of the requested revision is missing, first={:?}",freshness_difference(&float_pixels(&r,&output),&previous,display_mips::Plan::at(extent,0),output.format()));
            let published=crate::test_support::published_coverage(crate::test_support::coverage_hash(&cache.geometry)).unwrap();
            assert_eq!(published.receipt,receipt,"a partial new revision cannot replace the root publication witness");
        }
        store.gate.release();submit_settled(&mut r,next);crate::test_support::complete(&r);
        let cache=r.scale_display.as_ref().unwrap();assert!(cache.coverage_snapshot().ready(PixelRect::full(extent),pages::Accuracy::Display));
        let published=crate::test_support::published_coverage(crate::test_support::coverage_hash(&cache.geometry)).unwrap();assert_ne!(published.receipt,receipt);
        presenter.present(&r,&view,frame.view,[0.;4]).unwrap();let actual=float_pixels(&r,&output);
        let reference=freshness_reference_renderer(&changed);
        let raw=float_pixels(&r,crate::test_support::document_texture(&r));
        let raw_expected=float_pixels(&reference,crate::test_support::document_texture(&reference));
        let raw_error=crate::test_support::max_error(&raw,&raw_expected);
        assert!(raw_error<2e-5,"complete raw root differs from fresh raw reference: max={raw_error}, first={:?}",
            freshness_difference(&raw,&raw_expected,display_mips::Plan::at(extent,0),crate::test_support::document_texture(&r).format()));
        let mut reference_presenter=crate::test_support::float_presenter(&reference);
        let (reference_texture,reference_target)=create_color_target(&reference.device,extent,"whole-revision fresh presented reference");
        reference_presenter.present(&reference,&reference_target,frame.view,[0.;4]).unwrap();
        let expected=float_pixels(&reference,&reference_texture);
        let error=crate::test_support::max_error(&actual,&expected);
        assert!(error<2e-5,"complete presented root differs from fresh presented reference: max={error}, first={:?}",
            freshness_difference(&actual,&expected,display_mips::Plan::at(extent,0),output.format()));
        assert!(crate::test_support::max_error(&actual,&previous)>0.5,"the complete new revision must really replace the previous picture");
    }
}

