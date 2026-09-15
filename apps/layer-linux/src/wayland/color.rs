//! The app describes its pass-through swapchain; WSI must not also own this
//! protocol object. Piecewise sRGB is explicit (v2 TF 14), never legacy TF 9.
use super::*;
use crate::display_color::ViewColor;
use std::{
    collections::HashSet,
    fs::File,
    io::{Seek, Write},
    os::fd::{AsFd, FromRawFd},
};
use wayland_client::{WEnum, globals::GlobalList};
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_surface_v1 as surface, wp_color_manager_v1 as manager,
    wp_image_description_creator_icc_v1 as icc, wp_image_description_creator_params_v1 as params,
    wp_image_description_v1 as description,
};

#[derive(Default)]
pub(super) struct State {
    features: HashSet<u32>,
    primaries: HashSet<u32>,
    transfers: HashSet<u32>,
    intents: HashSet<u32>,
    generation: u64,
    ready: Option<Result<(), String>>,
}
pub(super) struct ColorSurface {
    manager: manager::WpColorManagerV1,
    surface: Option<surface::WpColorManagementSurfaceV1>,
    image: Option<description::WpImageDescriptionV1>,
}
impl ColorSurface {
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<Events>) -> Option<Self> {
        #[cfg(test)]
        if std::env::var_os("LAYER_TEST_VIEW_UNMANAGED").is_some() {
            return None;
        }
        let max_version = 2;
        #[cfg(test)]
        let max_version = if std::env::var_os("LAYER_TEST_VIEW_ICC").is_some() {
            1
        } else {
            max_version
        };
        Some(Self {
            manager: globals.bind(qh, 1..=max_version, ()).ok()?,
            surface: None,
            image: None,
        })
    }
}
impl Drop for ColorSurface {
    fn drop(&mut self) {
        if let Some(surface) = self.surface.take() {
            surface.destroy();
        }
        if let Some(image) = self.image.take() {
            image.destroy();
        }
        self.manager.destroy();
    }
}
impl Child {
    /// Runs during worker initialization, before configuring any swapchain.
    /// A compositor without color management uses the explicit sRGB fallback.
    pub fn describe_sdr(&mut self) -> Result<ViewColor, String> {
        let Some(color) = self.color.as_mut() else {
            return Ok(ViewColor::Srgb);
        };
        self.events.roundtrip(&mut self.state).map_err(error)?;
        let qh = self.events.handle();
        if !self
            .state
            .color
            .intents
            .contains(&(manager::RenderIntent::Perceptual as u32))
        {
            return Err("The compositor does not support SDR perceptual presentation".into());
        }
        let choices = [ViewColor::DisplayP3, ViewColor::Srgb];
        #[cfg(test)]
        let choices = if std::env::var_os("LAYER_TEST_VIEW_SRGB").is_some() {
            [ViewColor::Srgb; 2]
        } else {
            choices
        };
        let mut last_error = "The compositor cannot describe SDR colors accurately".to_string();
        for view in choices {
            // ICC also handles v1 compositors and profiles not available as named
            // primaries. Both routes describe the exact piecewise sRGB curve.
            for use_icc in [false, true] {
                let state = &mut self.state.color;
                let primaries = match view {
                    ViewColor::Srgb => manager::Primaries::Srgb,
                    ViewColor::DisplayP3 => manager::Primaries::DisplayP3,
                };
                if use_icc {
                    if !state.features.contains(&(manager::Feature::IccV2V4 as u32)) {
                        continue;
                    }
                } else if color.manager.version() < 2
                    || !state
                        .features
                        .contains(&(manager::Feature::Parametric as u32))
                    || !state.primaries.contains(&(primaries as u32))
                    || !state
                        .transfers
                        .contains(&(manager::TransferFunction::CompoundPower24 as u32))
                {
                    continue;
                }
                state.generation += 1;
                state.ready = None;
                let image = if use_icc {
                    let profile = layer_color::profile_bytes(
                        &layer_core::color::ColorProfile::Builtin(view.space()),
                    )?;
                    // An anonymous, bounded file survives through the protocol's
                    // transferred fd; it never modifies the user's ICC library.
                    let fd = unsafe {
                        libc::memfd_create(c"capy-sdr-profile".as_ptr(), libc::MFD_CLOEXEC)
                    };
                    if fd < 0 {
                        return Err(std::io::Error::last_os_error().to_string());
                    }
                    let mut file = unsafe { File::from_raw_fd(fd) };
                    file.write_all(&profile).map_err(error)?;
                    // Mutter 50.4 reads relative to the received descriptor's
                    // current offset. Start at zero as well as declaring offset 0.
                    file.rewind().map_err(error)?;
                    let creator = color.manager.create_icc_creator(&qh, ());
                    creator.set_icc_file(file.as_fd(), 0, profile.len() as u32);
                    let image = creator.create(&qh, state.generation);
                    self.connection.flush().map_err(error)?;
                    image
                } else {
                    let creator = color.manager.create_parametric_creator(&qh, ());
                    creator.set_primaries_named(primaries);
                    creator.set_tf_named(manager::TransferFunction::CompoundPower24);
                    creator.create(&qh, state.generation)
                };
                // The server may complete ICC parsing asynchronously. No GTK
                // input or document work runs on this initialization worker.
                while self.state.color.ready.is_none() {
                    self.events
                        .blocking_dispatch(&mut self.state)
                        .map_err(error)?;
                }
                match self.state.color.ready.take().unwrap() {
                    Ok(()) => {
                        let surface = color.manager.get_surface(&self.surface, &qh, ());
                        surface.set_image_description(&image, manager::RenderIntent::Perceptual);
                        color.surface = Some(surface);
                        color.image = Some(image);
                        self.connection.flush().map_err(error)?;
                        eprintln!(
                            "Wayland SDR description: {view:?}, {}",
                            if use_icc {
                                "ICC"
                            } else {
                                "piecewise sRGB (TF 14)"
                            }
                        );
                        return Ok(view);
                    }
                    Err(error) => {
                        last_error = error;
                        image.destroy();
                    }
                }
            }
        }
        Err(last_error)
    }
}
impl Dispatch<manager::WpColorManagerV1, ()> for Events {
    fn event(
        state: &mut Self,
        _: &manager::WpColorManagerV1,
        event: manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            manager::Event::SupportedFeature {
                feature: WEnum::Value(value),
            } => {
                state.color.features.insert(value as u32);
            }
            manager::Event::SupportedPrimariesNamed {
                primaries: WEnum::Value(value),
            } => {
                state.color.primaries.insert(value as u32);
            }
            manager::Event::SupportedTfNamed {
                tf: WEnum::Value(value),
            } => {
                state.color.transfers.insert(value as u32);
            }
            manager::Event::SupportedIntent {
                render_intent: WEnum::Value(value),
            } => {
                state.color.intents.insert(value as u32);
            }
            _ => (),
        }
    }
}
impl Dispatch<description::WpImageDescriptionV1, u64> for Events {
    fn event(
        state: &mut Self,
        _: &description::WpImageDescriptionV1,
        event: description::Event,
        generation: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if *generation != state.color.generation {
            return;
        }
        match event {
            description::Event::Ready { .. } | description::Event::Ready2 { .. } => {
                state.color.ready = Some(Ok(()));
            }
            description::Event::Failed { msg, .. } => {
                state.color.ready = Some(Err(format!("SDR image description: {msg}")));
            }
            _ => (),
        }
    }
}
wayland_client::delegate_noop!(Events: ignore surface::WpColorManagementSurfaceV1);
wayland_client::delegate_noop!(Events: ignore icc::WpImageDescriptionCreatorIccV1);
wayland_client::delegate_noop!(Events: ignore params::WpImageDescriptionCreatorParamsV1);
