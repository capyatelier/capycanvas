use super::*;
use layer_render::FrameDemand;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Input { FrameSlot, Decode, Compile, Gpu, Name, Residency }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outstanding { Runnable, Blocked(Input) }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ArrivalToken(pub(super) u64);

impl WgpuRasterizer {
    #[cfg(test)]
    pub(crate) fn outstanding_terms(&self) -> Vec<(&'static str, Outstanding)> {
        self.outstanding().into_iter().filter_map(|(term, state)| state.map(|state| (term, state))).collect()
    }
    fn outstanding(&self) -> [(&'static str, Option<Outstanding>); 26] {
        let restoring = self.restore_preparation().is_some();
        self.terms().map(|(term, state)| (term, state.filter(|_| !restoring || matches!(term, "restore preparation" | "display selection" | "settling"))))
    }
    fn terms(&self) -> [(&'static str, Option<Outstanding>); 26] {
        use Outstanding::{Blocked, Runnable};
        let decode = self.image_decode_waiting();
        let gated = |pending: bool| pending.then_some(if decode { Blocked(Input::Decode) } else { Runnable });
        let objects = self.scene.as_deref().and_then(|scene| scene.objects_outstanding(self));
        let selection_compiled = self.selection_clip.pipelines().into_iter().take(2).all(|pipeline| pipeline.ready());
        [
            ("display selection", self.pending_display_selection.is_some()
                .then_some(if selection_compiled { Runnable } else { Blocked(Input::Compile) })),
            ("selection previews", self.selection_previews.pending.then_some(Blocked(Input::Compile))),
            ("paper decode results", self.papers.images_ready().then_some(Runnable)),
            ("paper decodes", self.papers.image_waiting().then_some(Blocked(Input::Decode))),
            ("fill preview", self.fill_preview_outstanding()),
            ("held decode results", self.held_result_ready().then_some(Runnable)),
            ("fill edges", gated(self.fill_edges_preflight.as_ref().is_some_and(|state| state.pending()))),
            ("bake preflight", self.bake_preflight.as_ref().filter(|state| state.pending())
                .map(|state| if state.waiting() { Blocked(Input::Decode) } else { Runnable })),
            ("restore preparation", self.restore_preparation()),
            ("effect analysis", self.analysis_dirty.then_some(Runnable)),
            ("transform damage", (!self.transform_damage.is_empty()).then_some(Runnable)),
            ("document damage", (!self.document_damage.is_empty()).then_some(Runnable)),
            ("settling", self.settling.is_some().then_some(Runnable)),
            ("retouch sources", gated(self.retouch.as_ref().is_some_and(|retouch| retouch.pending()))),
            ("navigator", self.navigator.outstanding(self).map(|state| match objects { Some(Blocked(input)) => Blocked(input), _ => state })),
            ("canonical names", (self.navigator.waiting_for_name()
                || self.scale_display.as_ref().is_some_and(|cache| cache.waiting_for_name(self))).then_some(Blocked(Input::Name))),
            ("transform sources", gated(self.transform_sources_pending && self.transform_preview.is_some())),
            ("transform standby", (self.transform_sources_pending && self.moving_pixels.is_some() && self.transform_preview.is_none())
                .then_some(Blocked(Input::Compile))),
            ("moving images", self.moving_images_outstanding()),
            ("decodes", gated(self.moving_decode.as_ref().is_some_and(|(_, queue)| queue.pending()))),
            ("objects", objects.filter(|_| self.moving_layer.is_none())),
            ("display", self.scale_display.as_ref().and_then(|cache| cache.outstanding(self))),
            ("display retention", self.retention_above_grant().then_some(Runnable)),
            ("paper shade", self.paper_shade_work_pending().then_some(Runnable)),
            ("startup", self.startup_outstanding()),
            ("region preparation", self.region_prewarm_runnable().then_some(Runnable)),
        ]
    }
    pub(crate) fn moving_images_outstanding(&self) -> Option<Outstanding> {
        if self.window_samples_ready() { return Some(Outstanding::Runnable); }
        (self.moving_images_waiting || self.moving_images.pending())
            .then(|| if self.image_decode_waiting() { Outstanding::Blocked(Input::Decode) } else { Outstanding::Runnable })
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
        if self.fill_preview_ready().is_some() { return None; }
        Some(if self.fill_preview_shown == Some(key) && self.fill_preview_done.is_some() { Outstanding::Blocked(Input::Gpu) } else { Outstanding::Runnable })
    }
    pub(crate) fn fill_preview_in_flight(&self) -> bool {
        self.fill_preview_done.as_ref().is_some_and(|done| !done.load(std::sync::atomic::Ordering::Acquire))
    }
    pub(crate) fn wakes(&self, input: Input) -> bool {
        self.preparation_can_wake() && (input != Input::Gpu || self.gpu_results_pending())
    }
    pub(crate) fn frame_slot_blocked(&self) -> bool { !self.can_submit() && self.wakes(Input::FrameSlot) }
    fn unwoken(&self) -> impl Iterator<Item = (&'static str, Outstanding)> + '_ {
        let slot = self.frame_slot_blocked();
        self.outstanding().into_iter().filter_map(move |(term, state)| match state? {
            Outstanding::Runnable => (!slot).then_some((term, Outstanding::Runnable)),
            Outstanding::Blocked(input) => (!self.wakes(input)).then_some((term, Outstanding::Blocked(input))),
        })
    }
    pub(crate) fn frame_work(&self) -> bool { self.unwoken().next().is_some() }
    fn completion_edge(&self) -> bool {
        let Some(wake) = self.preparation_wake.as_ref().filter(|_| self.gpu_results_pending()) else { return false; };
        let completions = self.device.on_next_completion(wake);
        self.observed_completions.swap(completions, std::sync::atomic::Ordering::AcqRel) != completions && self.frame_results_pending()
    }
    pub(crate) fn frame_demand(&self) -> FrameDemand {
        if self.region_step_runnable() || self.completion_edge() || self.frame_work() { return FrameDemand::Work; }
        if self.scene_tiers.display.settle() && self.frame_work() { return FrameDemand::Work; }
        #[cfg(debug_assertions)]
        self.assert_blocked_inputs_outstanding();
        FrameDemand::Wait
    }
    #[cfg(debug_assertions)]
    fn assert_blocked_inputs_outstanding(&self) {
        let decodes = self.moving_decode.as_ref().is_some_and(|(_, queue)| queue.pending()) || self.papers.image_waiting()
            || self.bake_preflight.as_ref().is_some_and(|state| state.waiting());
        let lost: Vec<_> = self.outstanding().into_iter().filter(|(_, state)| match state {
            Some(Outstanding::Blocked(Input::Decode)) => !decodes,
            Some(Outstanding::Blocked(Input::Gpu)) => self.fill_preview_done.is_none(),
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
        let units=crate::test_support::PassCounts::snapshot().delta(self.counts.take().expect("an audited frame begins")).destination_units;
        let demanded=self.terms.iter().any(|(term,state)|matches!(*term,"display"|"navigator") && *state==Outstanding::Runnable);
        self.idle=if demanded && units==0 {self.idle+1}else {0};
        assert!(self.idle<2,"destinations reported Runnable but attempted no unit in two consecutive frames: {context}, terms={:?}, submissions={}",
            self.terms,r.metrics.submissions-self.submissions);
    }
}
