//! Negotiated scRGB or BT.2020 PQ presentation. Unknown capability is mapped SDR.
//! Feedback is asynchronous, with one in-flight preferred-description request.
use super::{color::*, *};
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_surface_feedback_v1 as feedback, wp_image_description_info_v1 as info,
};

#[derive(Default)]
pub(super) struct HdrState {
    ready: Option<Result<(), String>>,
    feedback: Option<feedback::WpColorManagementSurfaceFeedbackV1>,
    image: Option<description::WpImageDescriptionV1>,
    dirty: bool,
    generation: u64,
    peak: u32,
    reference: u32,
    headroom: f32,
}
impl Drop for HdrState {
    fn drop(&mut self) {
        if let Some(image) = self.image.take() {
            image.destroy();
        }
        if let Some(feedback) = self.feedback.take() {
            feedback.destroy();
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum HdrDescription {
    Surface,
    Preferred(u64),
}

impl Child {
    /// Called on the GPU owner before configuring a floating-point swapchain.
    pub fn describe_hdr(&mut self) -> Result<Option<layer_render_wgpu::SdrSurfaceColor>, String> {
        if self.color.is_none() {
            return Ok(None);
        }
        self.events.roundtrip(&mut self.state).map_err(error)?;
        let state = &self.state.color;
        if !state.intents.contains(&(manager::RenderIntent::Perceptual as u32)) {
            return Ok(None);
        }
        let encoding = if state.features.contains(&(manager::Feature::WindowsScrgb as u32)) {
            layer_render_wgpu::SdrSurfaceColor::WindowsScrgb
        } else if state.features.contains(&(manager::Feature::Parametric as u32))
            && state.primaries.contains(&(manager::Primaries::Bt2020 as u32))
            && state.transfers.contains(&(manager::TransferFunction::St2084Pq as u32)) {
            layer_render_wgpu::SdrSurfaceColor::Bt2100Pq
        } else {
            eprintln!("Wayland HDR unavailable: neither scRGB nor BT.2020 PQ is advertised");
            return Ok(None);
        };
        let qh = self.events.handle();
        let color = self.color.as_mut().unwrap();
        self.state.hdr.ready = None;
        let image = if encoding == layer_render_wgpu::SdrSurfaceColor::WindowsScrgb {
            color.manager.create_windows_scrgb(&qh, HdrDescription::Surface)
        } else {
            let creator = color.manager.create_parametric_creator(&qh, ());
            creator.set_primaries_named(manager::Primaries::Bt2020);
            creator.set_tf_named(manager::TransferFunction::St2084Pq);
            // PQ defaults specify 203 cd/m² reference white and 10,000 cd/m²
            // signal peak. No unsupported extended-volume request is needed.
            creator.create(&qh, HdrDescription::Surface)
        };
        // These descriptions require no profile IO. A roundtrip
        // delivers its immediate ready/failed event, never an unbounded loop.
        self.events.roundtrip(&mut self.state).map_err(error)?;
        match self.state.hdr.ready.take() {
            Some(Ok(())) => {
                let surface = color
                    .surface
                    .get_or_insert_with(|| color.manager.get_surface(&self.surface, &qh, ()));
                surface.set_image_description(&image, manager::RenderIntent::Perceptual);
                if let Some(old) = color.image.replace(image) {
                    old.destroy();
                }
                let feedback = color.manager.get_surface_feedback(&self.surface, &qh, ());
                self.state.hdr.feedback = Some(feedback);
                self.state.hdr.dirty = true;
                self.poll_hdr_feedback();
                self.connection.flush().map_err(error)?;
                eprintln!(
                    "Wayland HDR description: {encoding:?}; artwork white = 203 cd/m²"
                );
                Ok(Some(encoding))
            }
            _ => {
                image.destroy();
                Ok(None)
            }
        }
    }
    pub fn hdr_headroom(&self) -> f32 {
        self.state.hdr.headroom.max(1.)
    }
    pub(super) fn poll_hdr_feedback(&mut self) {
        let hdr = &mut self.state.hdr;
        if hdr.dirty
            && hdr.image.is_none()
            && let Some(feedback) = &hdr.feedback
        {
            hdr.dirty = false;
            hdr.peak = 0;
            hdr.reference = 0;
            hdr.generation += 1;
            hdr.image = Some(feedback.get_preferred(
                &self.events.handle(),
                HdrDescription::Preferred(hdr.generation),
            ));
        }
    }
}
impl Dispatch<feedback::WpColorManagementSurfaceFeedbackV1, ()> for Events {
    fn event(
        state: &mut Self,
        _: &feedback::WpColorManagementSurfaceFeedbackV1,
        event: feedback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(
            event,
            feedback::Event::PreferredChanged { .. } | feedback::Event::PreferredChanged2 { .. }
        ) {
            state.hdr.dirty = true;
            state.hdr.headroom = 1.;
        }
    }
}
impl Dispatch<description::WpImageDescriptionV1, HdrDescription> for Events {
    fn event(
        state: &mut Self,
        image: &description::WpImageDescriptionV1,
        event: description::Event,
        data: &HdrDescription,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let result = match event {
            description::Event::Ready { .. } | description::Event::Ready2 { .. } => Ok(()),
            description::Event::Failed { msg, .. } => Err(msg),
            _ => return,
        };
        match *data {
            HdrDescription::Surface => state.hdr.ready = Some(result),
            HdrDescription::Preferred(generation) if generation == state.hdr.generation => {
                if result.is_ok() {
                    image.get_information(qh, generation);
                } else {
                    state.hdr.headroom = 1.;
                    if let Some(image) = state.hdr.image.take() {
                        image.destroy();
                    }
                }
            }
            _ => (),
        }
    }
}
impl Dispatch<info::WpImageDescriptionInfoV1, u64> for Events {
    fn event(
        state: &mut Self,
        _: &info::WpImageDescriptionInfoV1,
        event: info::Event,
        generation: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if *generation != state.hdr.generation {
            return;
        }
        match event {
            info::Event::Luminances { reference_lum, .. } => state.hdr.reference = reference_lum,
            info::Event::TargetLuminance { max_lum, .. } => state.hdr.peak = max_lum,
            info::Event::Done => {
                if !state.hdr.dirty {
                    // Target luminance is a compositor hint, not a measurement.
                    state.hdr.headroom = display_headroom(state.hdr.peak, state.hdr.reference);
                    eprintln!(
                        "Wayland display hint: {} cd/m² peak, {} cd/m² reference, {:.3}× HDR headroom",
                        state.hdr.peak, state.hdr.reference, state.hdr.headroom
                    );
                }
                if let Some(image) = state.hdr.image.take() {
                    image.destroy();
                }
            }
            _ => (),
        }
    }
}

// Compositors anchor surface reference white to their preferred reference white.
// Dividing by the document's 203 nits misreads user-adjusted SDR brightness.
fn display_headroom(peak: u32, reference: u32) -> f32 {
    if reference == 0 { return 1.; }
    (peak as f32 / reference as f32).clamp(1., 10000. / 203.)
}

#[cfg(test)]
mod tests {
    #[test]
    fn headroom_uses_compositor_reference_and_rejects_unknown_capability() {
        assert_eq!(super::display_headroom(1000, 100), 10.);
        assert_eq!(super::display_headroom(80, 80), 1.);
        assert_eq!(super::display_headroom(0, 203), 1.);
        assert_eq!(super::display_headroom(1000, 0), 1.);
    }
}
