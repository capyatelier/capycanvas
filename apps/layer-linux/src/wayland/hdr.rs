//! Negotiated scRGB or BT.2020 PQ presentation. Unknown capability is mapped SDR.
use super::{color::*, *};

#[derive(Default)]
pub(super) struct HdrState {
    ready: Option<Result<(), String>>,
}
pub(super) struct HdrSurface;

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
            color.manager.create_windows_scrgb(&qh, HdrSurface)
        } else {
            let creator = color.manager.create_parametric_creator(&qh, ());
            creator.set_primaries_named(manager::Primaries::Bt2020);
            creator.set_tf_named(manager::TransferFunction::St2084Pq);
            // PQ defaults specify 203 cd/m² reference white and 10,000 cd/m²
            // signal peak. No unsupported extended-volume request is needed.
            creator.create(&qh, HdrSurface)
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
                self.watch_display();
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
}
impl Dispatch<description::WpImageDescriptionV1, HdrSurface> for Events {
    fn event(
        state: &mut Self,
        _: &description::WpImageDescriptionV1,
        event: description::Event,
        _: &HdrSurface,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.hdr.ready = match event {
            description::Event::Ready { .. } | description::Event::Ready2 { .. } => Some(Ok(())),
            description::Event::Failed { msg, .. } => Some(Err(msg)),
            _ => return,
        };
    }
}
