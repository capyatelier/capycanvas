//! Await a native alert without transferring an owned dialog reference to C.
//! libadwaita-rs 0.9.2's choose wrapper uses into_glib_ptr for a borrowed C
//! parameter, retaining the dialog after dismissal. Signal ownership here is
//! explicit, and dropping an unfinished future closes its dialog.
use adw::prelude::*;
use gtk::glib;
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Poll, Waker},
};

#[derive(Default)]
struct Response {
    value: Option<glib::GString>,
    waker: Option<Waker>,
}
struct Wait {
    dialog: adw::AlertDialog,
    handler: Option<glib::SignalHandlerId>,
    completed: bool,
}
impl Drop for Wait {
    fn drop(&mut self) {
        if let Some(handler) = self.handler.take() {
            self.dialog.disconnect(handler);
        }
        if !self.completed {
            crate::page_dialog::force_close(&self.dialog);
        }
    }
}

pub async fn choose(dialog: adw::AlertDialog, parent: &impl IsA<gtk::Widget>) -> glib::GString {
    choose_role(dialog, parent, layer_ui::DialogRole::Alert, crate::page_dialog::Metadata::new()).await
}

pub async fn choose_editor(dialog: adw::AlertDialog, parent: &impl IsA<gtk::Widget>, primary: Option<&str>, secondary: &[&str]) -> glib::GString {
    let metadata = crate::page_dialog::Metadata::responses(primary, &dialog.close_response(), secondary);
    let metadata = if primary.is_none() { metadata.done() } else { metadata };
    choose_role(dialog, parent, layer_ui::DialogRole::Editor, metadata).await
}

pub async fn choose_intent(dialog: adw::AlertDialog, parent: &impl IsA<gtk::Widget>, intent: layer_ui::DialogIntent, primary: Option<&str>, secondary: &[&str]) -> glib::GString {
    let metadata = crate::page_dialog::Metadata::responses(primary, &dialog.close_response(), secondary).intent(intent);
    choose_role(dialog, parent, intent.role, metadata).await
}

async fn choose_role(dialog: adw::AlertDialog, parent: &impl IsA<gtk::Widget>, role: layer_ui::DialogRole, metadata: crate::page_dialog::Metadata) -> glib::GString {
    let state = Rc::new(RefCell::new(Response::default()));
    let handler = dialog.connect_response(None, {
        let state = state.clone();
        move |_, response| {
            let waker = {
                let mut state = state.borrow_mut();
                if state.value.is_none() {
                    state.value = Some(response.into());
                }
                state.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    });
    let mut wait = Wait {
        dialog,
        handler: Some(handler),
        completed: false,
    };
    crate::page_dialog::present_with(&wait.dialog, parent, role, metadata);
    let response = std::future::poll_fn(|cx| {
        let mut state = state.borrow_mut();
        if let Some(value) = state.value.take() {
            Poll::Ready(value)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    })
    .await;
    wait.completed = true;
    response
}
