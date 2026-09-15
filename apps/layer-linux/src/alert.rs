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
            self.dialog.force_close();
        }
    }
}

pub async fn choose(dialog: adw::AlertDialog, parent: &impl IsA<gtk::Widget>) -> glib::GString {
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
    wait.dialog.present(Some(parent));
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
