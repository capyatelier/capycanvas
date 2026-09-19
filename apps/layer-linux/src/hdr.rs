//! HDR display details and entry points into the nonmodal Proof panel.
use crate::workspace::Workspace;
use adw::prelude::*;
use std::rc::Rc;
pub(crate) fn open(w: &Rc<Workspace>) -> Result<(), String> {
    w.proof_panel.open(w, crate::files::proof::Page::Sdr)
}

pub(crate) fn display_details(w: &Rc<Workspace>) {
    let text = {
        let gpu = w.gpu.borrow();
        let Some(gpu) = gpu.as_ref() else {
            return;
        };
        let headroom = gpu.session.engine().backend().display_headroom;
        let encoding = gpu.session.engine().backend().display_encoding;
        let route = match encoding {
            Some(layer_render_wgpu::SdrSurfaceColor::Bt2100Pq) => "BT.2020 PQ · floating-point surface. Colors are limited to the display signal’s range for viewing.",
            Some(_) => "Linear scRGB · floating-point surface.",
            None => "The compositor did not offer a supported HDR surface.",
        };
        let state = if headroom > 1. {
            format!("HDR presentation · {headroom:.1}× compositor headroom")
        } else {
            "Showing the saved SDR appearance. The compositor has not reported HDR headroom for this window.".into()
        };
        format!(
            "{state}\n\n{route}\n\nArtwork reference white: 203 cd/m². Display limits come from the compositor, not a brightness measurement. The HDR master is preserved."
        )
    };
    let dialog = adw::AlertDialog::builder()
        .heading("Display Details")
        .body(&text)
        .build();
    dialog.add_response("close", "Close");
    dialog.set_close_response("close");
    dialog.present(Some(&w.window));
}
