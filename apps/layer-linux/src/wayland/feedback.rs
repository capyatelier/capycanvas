use super::{color::*, *};
use layer_color::screen::{Chromaticities, CompositorDescription, ScreenColor, Transfer};
use wayland_client::WEnum;
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_surface_feedback_v1 as feedback, wp_image_description_info_v1 as info,
};

#[derive(Default)]
pub(super) struct DisplayFeedback {
    feedback: Option<feedback::WpColorManagementSurfaceFeedbackV1>,
    image: Option<description::WpImageDescriptionV1>,
    dirty: bool,
    generation: u64,
    info: Info,
    color: ScreenColor,
    revision: u64,
}

#[derive(Default)]
struct Info {
    primaries: Option<Chromaticities>,
    target: Option<Chromaticities>,
    transfer: Option<Transfer>,
    reference_white: Option<u32>,
    target_peak: Option<u32>,
}

impl Info {
    fn description(&self) -> ScreenColor {
        match (self.primaries, self.target, self.transfer, self.reference_white, self.target_peak) {
            (Some(primaries), Some(target), Some(transfer), Some(white), Some(peak)) => {
                ScreenColor::Described(CompositorDescription {
                    primaries,
                    target,
                    transfer,
                    reference_white: white as f32,
                    target_peak: Some(peak as f32),
                })
            }
            _ => ScreenColor::Unreported,
        }
    }
}

pub(super) struct Preferred(u64);

impl Drop for DisplayFeedback {
    fn drop(&mut self) {
        if let Some(image) = self.image.take() {
            image.destroy();
        }
        if let Some(feedback) = self.feedback.take() {
            feedback.destroy();
        }
    }
}

impl DisplayFeedback {
    fn publish(&mut self, color: ScreenColor) {
        if color != self.color {
            eprintln!("Wayland preferred description: {color:?}");
        }
        self.color = color;
        self.revision += 1;
        if let Some(image) = self.image.take() {
            image.destroy();
        }
    }
}

impl Child {
    pub(super) fn watch_display(&mut self) {
        let Some(color) = &self.color else { return };
        if self.state.display.feedback.is_none() {
            self.state.display.feedback = Some(color.manager.get_surface_feedback(&self.surface, &self.events.handle(), ()));
            self.state.display.dirty = true;
            self.poll_display_feedback();
        }
    }

    pub fn watching_display(&self) -> bool {
        self.state.display.feedback.is_some()
    }

    pub fn screen_color(&self) -> (u64, ScreenColor) {
        if self.color.is_none() {
            return (0, ScreenColor::Unmanaged);
        }
        (self.state.display.revision, self.state.display.color)
    }

    pub(super) fn poll_display_feedback(&mut self) {
        let display = &mut self.state.display;
        if display.dirty
            && display.image.is_none()
            && let Some(feedback) = &display.feedback
        {
            display.dirty = false;
            display.generation += 1;
            display.info = Info::default();
            display.image = Some(feedback.get_preferred(&self.events.handle(), Preferred(display.generation)));
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
        if matches!(event, feedback::Event::PreferredChanged { .. } | feedback::Event::PreferredChanged2 { .. }) {
            state.display.dirty = true;
        }
    }
}

impl Dispatch<description::WpImageDescriptionV1, Preferred> for Events {
    fn event(
        state: &mut Self,
        image: &description::WpImageDescriptionV1,
        event: description::Event,
        &Preferred(generation): &Preferred,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if generation != state.display.generation {
            return;
        }
        match event {
            description::Event::Ready { .. } | description::Event::Ready2 { .. } => {
                image.get_information(qh, generation);
            }
            description::Event::Failed { .. } => state.display.publish(ScreenColor::Unreported),
            _ => (),
        }
    }
}

fn chromaticities(values: [i32; 8]) -> Chromaticities {
    let xy = |i: usize| [f64::from(values[i]) / 1e6, f64::from(values[i + 1]) / 1e6];
    Chromaticities { primaries: [xy(0), xy(2), xy(4)], white: xy(6) }
}

fn transfer(tf: manager::TransferFunction) -> Transfer {
    use manager::TransferFunction as T;
    match tf {
        T::Srgb | T::ExtSrgb | T::CompoundPower24 => Transfer::Srgb,
        T::Gamma22 => Transfer::Gamma(2.2),
        T::Gamma28 => Transfer::Gamma(2.8),
        T::Bt1886 => Transfer::Bt1886,
        T::St2084Pq => Transfer::Pq,
        T::Hlg => Transfer::Hlg,
        T::ExtLinear => Transfer::Linear,
        _ => Transfer::Other,
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
        if *generation != state.display.generation {
            return;
        }
        let info = &mut state.display.info;
        match event {
            info::Event::Primaries { r_x, r_y, g_x, g_y, b_x, b_y, w_x, w_y } => {
                info.primaries = Some(chromaticities([r_x, r_y, g_x, g_y, b_x, b_y, w_x, w_y]));
            }
            info::Event::TargetPrimaries { r_x, r_y, g_x, g_y, b_x, b_y, w_x, w_y } => {
                info.target = Some(chromaticities([r_x, r_y, g_x, g_y, b_x, b_y, w_x, w_y]));
            }
            info::Event::TfNamed { tf: WEnum::Value(tf) } => info.transfer = Some(transfer(tf)),
            info::Event::TfNamed { .. } => info.transfer = Some(Transfer::Other),
            info::Event::TfPower { eexp } => info.transfer = Some(Transfer::Gamma(eexp as f32 / 10_000.)),
            info::Event::Luminances { reference_lum, .. } => info.reference_white = Some(reference_lum),
            info::Event::TargetLuminance { max_lum, .. } => info.target_peak = Some(max_lum),
            info::Event::Done => {
                let color = state.display.info.description();
                state.display.publish(color);
            }
            _ => (),
        }
    }
}
