//! Entry point into the nonmodal Proof panel's SDR page.
use crate::workspace::Workspace;
use std::rc::Rc;
pub(crate) fn open(w: &Rc<Workspace>) -> Result<(), String> {
    w.proof_panel.open(w, crate::files::proof::Page::Sdr)
}
