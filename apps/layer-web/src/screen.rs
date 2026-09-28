use super::*;
use layer_color::screen::{Chromaticities, ScreenReport};
use layer_core::color::RgbSpace;

const SCREEN_CHECK_DELAY_MS: f64 = 250.;

#[wasm_bindgen]
impl WebApp {
    pub fn set_screen(&mut self, gamut: &str, hdr: bool) -> u32 {
        let gamut = match gamut {
            "rec2020" => Chromaticities::BT2020,
            "p3" => Chromaticities::of(RgbSpace::DisplayP3),
            _ => Chromaticities::of(RgbSpace::Srgb),
        };
        let report = ScreenReport::managed(None, gamut, hdr, hdr.then_some(true));
        if self.session.set_screen_report(report) { layer_ui::regions::HOST } else { 0 }
    }

    pub fn screen_tick(&mut self, now_ms: f64) -> u32 {
        let (Some(gpu), Some(surface)) = (self.session.engine().backend().0.as_deref(), self.surface.as_mut()) else {
            return 0;
        };
        if let Some(result) = surface.presenter.screen_check_result() {
            let clipped = result.ok();
            return if self.session.set_screen_clipped(clipped) { layer_ui::regions::HOST } else { 0 };
        }
        if now_ms - self.screen_presented_ms >= SCREEN_CHECK_DELAY_MS {
            surface.presenter.check_screen(gpu);
        }
        0
    }
}
