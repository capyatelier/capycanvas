use super::*;
use layer_render::FrameDemand;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Input { FrameSlot, Decode, Compile, Gpu, Name, Residency }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outstanding { Runnable, Blocked(Input) }
impl Outstanding {
    pub(crate) fn either(first: Option<Self>, second: Option<Self>) -> Option<Self> {
        if second == Some(Self::Runnable) { second } else { first.or(second) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Allowance { Fresh, Spent }

#[derive(Clone)]
pub(crate) enum Producer { Compile(Arc<compile::Needs>), Texture(AssetId), Name(layer_core::raster::ContentVersion), Declaration(u64), Gpu(submission::Submission), Class(Input) }
impl Producer {
    fn input(&self) -> Input {
        match self { Self::Compile(_) | Self::Texture(_) => Input::Compile, Self::Name(_) | Self::Declaration(_) => Input::Name, Self::Gpu(_) => Input::Gpu, Self::Class(input) => *input }
    }
    fn pending(&self, r: &WgpuRasterizer) -> bool {
        match self {
            Self::Compile(needs) => !needs.ready() && r.compilation.check().is_ok(),
            Self::Texture(id) => r.texture_pending(id),
            Self::Name(name) => name.resolved().is_none(),
            Self::Declaration(published) => r.raster_declarations() == *published,
            Self::Gpu(submission) => {
                if let Some(wake) = &r.preparation_wake { r.device.on_next_completion(wake); }
                submission.pending()
            },
            Self::Class(input) => r.waits_on(*input),
        }
    }
}
impl PartialEq for Producer {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Compile(first), Self::Compile(second)) => Arc::ptr_eq(first, second),
            (Self::Texture(first), Self::Texture(second)) => first == second,
            (Self::Name(first), Self::Name(second)) => first.identity() == second.identity(),
            (Self::Declaration(first), Self::Declaration(second)) => first == second,
            (Self::Gpu(first), Self::Gpu(second)) => first.same(second),
            (Self::Class(first), Self::Class(second)) => first == second,
            _ => false,
        }
    }
}
impl Eq for Producer {}
impl std::fmt::Debug for Producer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_tuple("Producer").field(&self.input()).finish() }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UnitWait { Ready(Allowance), Reclaim(crate::residency::Blocked, Allowance), Input(Producer, Allowance), Parked(crate::residency::Blocked), End(Option<GpuRasterError>) }
impl Default for UnitWait {
    fn default() -> Self { Self::Ready(Allowance::Fresh) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting(crate::residency::Blocked);
impl Waiting {
    pub(crate) fn error(need: crate::residency::Blocked) -> GpuRasterError { GpuRasterError::Waiting(Self(need)) }
    pub(crate) fn need(&self) -> &crate::residency::Blocked { &self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Turn { Enter(Allowance), Sleep(Input), Dormant, Deliver }

impl UnitWait {
    pub(crate) fn refused(need: crate::residency::Blocked, allowance: Allowance) -> Self {
        match Self::parked(need, allowance) {
            Self::Parked(need) if need.class().required() => Self::End(None),
            wait => wait,
        }
    }
    pub(crate) fn parked(need: crate::residency::Blocked, allowance: Allowance) -> Self {
        match need.waiting() {
            Some(_) if need.external() => Self::Reclaim(need, allowance),
            Some(_) if allowance == Allowance::Fresh => Self::Reclaim(need, Allowance::Spent),
            _ => Self::Parked(need),
        }
    }
    pub(crate) fn input(producer: Producer) -> Self { Self::Input(producer, Allowance::Fresh) }
    pub(crate) fn pending(error: &GpuRasterError, producer: Producer) -> Self {
        match error { GpuRasterError::Waiting(waiting) => Self::parked(waiting.need().clone(), Allowance::Fresh), _ => Self::input(producer) }
    }
    pub(crate) fn naming(&self) -> bool { matches!(self, Self::Input(Producer::Name(_) | Producer::Declaration(_), _)) }
    pub(crate) fn refusing(&self) -> bool { matches!(self, Self::Reclaim(..) | Self::Parked(_) | Self::End(_)) }
    pub(crate) fn need(&self) -> Option<&crate::residency::Blocked> { match self { Self::Reclaim(need, _) | Self::Parked(need) => Some(need), _ => None } }
    pub(crate) fn ended(&self) -> Option<GpuRasterError> {
        match self { Self::End(error) => Some(error.clone().unwrap_or(GpuRasterError::StrokeMemory)), _ => None }
    }
    pub(crate) fn observe(&self, r: &WgpuRasterizer) -> Turn { self.observe_with(r, r.preparation_wake.as_ref()) }
    pub(crate) fn observe_with(&self, r: &WgpuRasterizer, wake: Option<&crate::residency::Wake>) -> Turn {
        match self {
            Self::Ready(allowance) => Turn::Enter(*allowance),
            Self::Reclaim(need, _) if need.waiting().is_some_and(|batch| batch.pending_with(wake)) => Turn::Sleep(Input::Residency),
            Self::Reclaim(_, allowance) => Turn::Enter(*allowance),
            Self::Input(producer, _) if producer.pending(r) => Turn::Sleep(producer.input()),
            Self::Input(_, allowance) => Turn::Enter(*allowance),
            Self::Parked(need) => {
                let releasable = wake.is_none_or(|wake| r.device.on_release(need, wake));
                if releasable && need.gained() && need.fits() { Turn::Enter(Allowance::Spent) } else { Turn::Dormant }
            },
            Self::End(_) => Turn::Deliver,
        }
    }
    pub(crate) fn demand(&self, r: &WgpuRasterizer) -> Option<Outstanding> {
        match self.observe(r) {
            Turn::Enter(_) | Turn::Deliver => Some(Outstanding::Runnable),
            Turn::Sleep(input) => Some(Outstanding::Blocked(input)),
            Turn::Dormant => None,
        }
    }
    pub(crate) fn eligible(&self, r: &WgpuRasterizer) -> Option<Allowance> {
        match self.observe(r) { Turn::Enter(allowance) => Some(allowance), _ => None }
    }
}

pub(crate) struct Delivery<T> { rx: std::sync::mpsc::Receiver<Result<T, GpuRasterError>>, held: std::cell::RefCell<Option<Result<T, GpuRasterError>>>, loss: Arc<crate::pipeline_device::DeviceLoss> }
impl<T> Delivery<T> {
    pub(crate) fn channel(loss: &Arc<crate::pipeline_device::DeviceLoss>) -> (std::sync::mpsc::Sender<Result<T, GpuRasterError>>, Self) {
        let (tx, rx) = std::sync::mpsc::channel();
        (tx, Self { rx, held: std::cell::RefCell::new(None), loss: loss.clone() })
    }
    pub(crate) fn arrived(&self) -> bool {
        let mut held = self.held.borrow_mut();
        if held.is_none() {
            *held = match self.rx.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => self.loss.error().cloned().map(Err),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(self.loss.error().cloned().unwrap_or_else(|| GpuRasterError::WaitFailed("The GPU reply was abandoned".into())))),
            };
        }
        held.is_some()
    }
    pub(crate) fn take(&self) -> Option<Result<T, GpuRasterError>> {
        if !self.arrived() { return None; }
        self.held.borrow_mut().take()
    }
}

impl WgpuRasterizer {
    #[cfg(test)]
    pub(crate) fn outstanding_terms(&self) -> Vec<(&'static str, Outstanding)> {
        self.outstanding().into_iter().filter_map(|(term, state)| state.map(|state| (term, state))).collect()
    }
    fn outstanding(&self) -> [(&'static str, Option<Outstanding>); 27] {
        let held = self.held_frame.is_some();
        let restoring = self.restore_preparation().is_some();
        self.terms().map(|(term, state)| (term, state.filter(|state| if held {
            *state != Outstanding::Runnable || matches!(term, "held frame" | "display selection" | "settling" | "display retention" | "startup" | "regions")
        } else {
            !restoring || matches!(term, "restore preparation" | "display selection" | "settling" | "display retention")
        })))
    }
    fn terms(&self) -> [(&'static str, Option<Outstanding>); 27] {
        use Outstanding::{Blocked, Runnable};
        let decode = self.image_decode_waiting();
        let gated = |pending: bool| pending.then_some(if decode { Blocked(Input::Decode) } else { Runnable });
        let objects = self.scene.as_deref().and_then(|scene| scene.objects_outstanding(self));
        [
            ("held frame", self.held_frame.as_ref().and_then(|wait| wait.demand(self))),
            ("display selection", self.pending_display_selection.as_ref().and_then(|(_, wait)| wait.demand(self))),
            ("selection previews", self.selection_previews_outstanding()),
            ("fill preview", self.fill_preview_outstanding()),
            ("held decode results", self.held_result_ready().then_some(Runnable)),
            ("fill edges", gated(self.fill_edges_preflight.as_ref().is_some_and(|state| state.pending()))),
            ("bake preflight", self.bake_preflight.as_ref().filter(|state| state.pending())
                .map(|state| if state.waiting() { Blocked(Input::Decode) } else { Runnable })),
            ("restore preparation", self.restore_preparation()),
            ("effect analysis", self.analysis_dirty.then_some(Runnable)),
            ("transform damage", (!self.transform_damage.is_empty()).then_some(Runnable)),
            ("document damage", (!self.document_damage.is_empty()).then_some(Runnable)),
            ("settling", self.settling.is_some().then_some(if self.settling_ready() { Runnable } else { Blocked(Input::Gpu) })),
            ("retouch sources", self.retouch.as_ref().and_then(|retouch| Outstanding::either(retouch.refused_room(self), gated(retouch.pending())))),
            ("navigator", self.navigator.outstanding(self).map(|state| match objects { Some(Blocked(input)) => Blocked(input), _ => state })),
            ("floor", crate::scene::scale::Floor::demand(self)),
            ("canonical names", (self.navigator.waiting_for_name()
                || self.scale_display.as_ref().is_some_and(|cache| cache.waiting_for_name(self))).then_some(Blocked(Input::Name))),
            ("transform sources", gated(self.transform_sources_pending && self.transform_preview.is_some())),
            ("transform standby", (self.transform_sources_pending && self.moving_pixels.is_some() && self.transform_preview.is_none())
                .then_some(Blocked(Input::Compile))),
            ("moving images", self.moving_images_outstanding()),
            ("decodes", gated(self.moving_decode.as_ref().is_some_and(|(_, queue)| queue.pending()))),
            ("objects", objects.filter(|_| self.moving_layer.is_none())),
            ("display", self.scale_display.as_ref().and_then(|cache| cache.outstanding(self))),
            ("display retention", if self.retention_above_grant() || self.reclaim_due() || self.victims_marked() { Some(Runnable) } else { self.memory.reclaim_unanswered().then_some(Blocked(Input::Residency)) }),
            ("paper shade", self.paper_shade_work_pending().then_some(Runnable)),
            ("startup", self.startup_outstanding()),
            ("regions", self.regions_outstanding()),
            ("pen-down tiles", self.pen_down_warm_due().then_some(Runnable)),
        ]
    }
    pub(crate) fn moving_images_outstanding(&self) -> Option<Outstanding> {
        let images = &self.moving_images;
        if images.published() || images.buildable(|source, coordinate, format| self.mip_source_resident(source, coordinate, format)) {
            return Some(Outstanding::Runnable);
        }
        if !(self.moving_images_waiting || images.pending()) { return None; }
        if self.image_decode_waiting() { return Some(Outstanding::Blocked(Input::Decode)); }
        Some(Outstanding::Runnable)
    }
    pub(crate) fn waits_on(&self, input: Input) -> bool {
        match input {
            Input::Decode => self.image_decode_waiting(),
            Input::Compile => self.scene.as_deref().is_some_and(scene::Scene::compiling),
            Input::Name | Input::FrameSlot | Input::Gpu | Input::Residency => true,
        }
    }
    fn fill_preview_outstanding(&self) -> Option<Outstanding> {
        let key = self.fill_preview.as_ref()?.key();
        if self.fill_preview_drawn() != Some(key) { return Some(Outstanding::Runnable); }
        let (_, receipt) = self.fill_preview_shown.as_ref()?;
        Producer::Gpu(receipt.clone()).pending(self).then_some(Outstanding::Blocked(Input::Gpu))
    }
    pub(crate) fn fill_preview_in_flight(&self) -> bool {
        self.fill_preview_shown.as_ref().is_some_and(|(_, receipt)| receipt.pending())
    }
    pub(crate) fn wakes(&self, input: Input) -> bool {
        self.preparation_can_wake() && (input != Input::Gpu || self.gpu_awaited()) && (input != Input::Compile || self.compilation.check().is_ok())
    }
    fn gpu_awaited(&self) -> bool {
        self.gpu_results_pending() || self.restore_awaits_gpu()
    }
    pub(crate) fn frame_slot_blocked(&self) -> bool { !self.can_submit() && self.wakes(Input::FrameSlot) }
    fn unwoken(&self) -> impl Iterator<Item = (&'static str, Outstanding)> + '_ {
        let slot = self.frame_slot_blocked();
        self.outstanding().into_iter().filter_map(move |(term, state)| match state? {
            Outstanding::Runnable => (!slot || term == "settling").then_some((term, Outstanding::Runnable)),
            Outstanding::Blocked(input) => (!self.wakes(input)).then_some((term, Outstanding::Blocked(input))),
        })
    }
    pub(crate) fn frame_work(&self) -> bool { self.unwoken().next().is_some() }
    fn completion_edge(&self) -> bool {
        let Some(wake) = self.preparation_wake.as_ref().filter(|_| self.gpu_results_pending()) else { return false; };
        self.device.on_next_completion(wake) != self.observed_completions.load(std::sync::atomic::Ordering::Acquire) && self.frame_results_ready()
    }
    fn frame_results_ready(&self) -> bool {
        self.color_sample_ready() || self.selection_paint_ready() || self.region_results_awaited()
            || self.filter_previews_ready() || self.effect_validation_pending() || self.brush_stamp_ready()
    }
    pub(crate) fn frame_demand(&self) -> FrameDemand {
        if self.region_step_runnable() || self.completion_edge() || self.frame_work() { return FrameDemand::Work; }
        if self.scene_tiers.display.settle() && self.frame_work() { return FrameDemand::Work; }
        #[cfg(debug_assertions)]
        self.assert_blocked_inputs_outstanding();
        self.presentation_due().map_or(FrameDemand::Wait, FrameDemand::WaitUntil)
    }
    #[cfg(debug_assertions)]
    fn assert_blocked_inputs_outstanding(&self) {
        let decodes = self.moving_decode.as_ref().is_some_and(|(_, queue)| queue.pending())
            || self.bake_preflight.as_ref().is_some_and(|state| state.waiting()) || !self.source_results.is_empty();
        let lost: Vec<_> = self.outstanding().into_iter().filter(|(_, state)| match state {
            Some(Outstanding::Blocked(Input::Decode)) => !decodes,
            Some(Outstanding::Blocked(Input::Gpu)) => !self.gpu_awaited(),
            _ => false,
        }).collect();
        debug_assert!(lost.is_empty(), "an idle renderer waits on inputs that are not outstanding: {lost:?}");
    }
    pub fn frame_preparation_pending(&self) -> bool {
        self.region_step_runnable() || self.outstanding().iter().any(|(_, state)| state.is_some())
    }
    pub fn settled(&self) -> bool { self.can_submit() && !self.frame_preparation_pending() }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct DemandAudit {terms:Vec<(&'static str,Outstanding)>,counts:Option<crate::test_support::PassCounts>,submissions:u64,idle:usize}
#[cfg(test)]
impl DemandAudit {
    pub(crate) fn begin(&mut self,r:&WgpuRasterizer) {
        self.terms=r.unwoken().collect();
        self.counts=Some(crate::test_support::PassCounts::snapshot());self.submissions=r.metrics.submissions;
    }
    pub(crate) fn end(&mut self,r:&WgpuRasterizer,context:std::fmt::Arguments<'_>) {
        let counts=crate::test_support::PassCounts::snapshot().delta(self.counts.take().expect("an audited frame begins"));
        let attempted=counts.destination_units!=0 || counts.destination_admissions!=0 || counts.destination_wait_transitions!=0;
        let demanded=self.terms.iter().any(|(term,state)|matches!(*term,"display"|"navigator") && *state==Outstanding::Runnable);
        self.idle=if demanded && !attempted {self.idle+1}else {0};
        assert!(self.idle<2,"destinations reported Runnable but attempted no unit in two consecutive frames: {context}, terms={:?}, submissions={}, units={}, admissions={}, wait transitions={}",
            self.terms,r.metrics.submissions-self.submissions,counts.destination_units,counts.destination_admissions,counts.destination_wait_transitions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_parked_unit_whose_release_service_has_retired_sleeps_instead_of_entering() {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.set_preparation_wake(Arc::new(|| {}));
        let need = r.scene_tiers.display.wanting(crate::residency::Class::Refinement, 1);
        r.device.poll_binding().retire_lost(|| {});
        assert_eq!(UnitWait::Parked(need).observe(&r), Turn::Dormant, "a retired release service granted a parked unit an attempt no released room backs");
    }
    #[test]
    fn a_reply_whose_producer_leaves_without_sending_has_arrived_as_a_disconnect() {
        let (tx, delivery) = Delivery::<u32>::channel(&Default::default());
        assert!(!delivery.arrived(), "a live producer is still awaited");
        drop(tx);
        assert!(delivery.arrived(), "a producer that leaves ends the wait instead of looking like a later event");
    }
    #[test]
    fn an_abandoned_reply_is_taken_once_as_its_failure_and_a_sent_one_as_its_result() {
        let (tx, delivery) = Delivery::<u32>::channel(&Default::default());
        assert_eq!(delivery.take(), None);
        tx.send(Ok(7)).unwrap();
        drop(tx);
        assert!(delivery.arrived());
        assert_eq!(delivery.take(), Some(Ok(7)), "a result sent before the producer left is delivered first");
        let (tx, delivery) = Delivery::<u32>::channel(&Default::default());
        drop(tx);
        assert!(matches!(delivery.take(), Some(Err(GpuRasterError::WaitFailed(_)))), "the owner receives the abandoned reply as its failure");
    }
    #[test]
    fn a_reply_still_awaited_when_its_device_is_lost_is_taken_once_as_the_loss_and_a_late_result_is_inert() {
        let loss = Arc::new(crate::pipeline_device::DeviceLoss::default());
        let (tx, delivery) = Delivery::<u32>::channel(&loss);
        let (landed, arrived) = Delivery::<u32>::channel(&loss);
        landed.send(Ok(3)).unwrap();
        assert!(arrived.arrived(), "a result that landed before the loss is held");
        assert!(!delivery.arrived(), "a live producer is still awaited");
        loss.report("Canvas GPU stopped (Unknown): test".into());
        let lost = Some(Err(GpuRasterError::WaitFailed("Canvas GPU stopped (Unknown): test".into())));
        assert!(delivery.arrived(), "a lost device ends the wait without its producer");
        tx.send(Ok(7)).unwrap();
        assert_eq!(delivery.take(), lost, "the reply fails with the loss, and the result that lands after it is inert");
        assert_eq!(arrived.take(), Some(Ok(3)), "a result that landed before the loss is still delivered");
        let (tx, abandoned) = Delivery::<u32>::channel(&loss);
        drop(tx);
        assert_eq!(abandoned.take(), lost, "a producer dropped by the loss reports the loss");
    }
}
