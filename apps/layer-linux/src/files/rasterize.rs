//! Explicit source commitment with complete-stack comparison and cancellation.
use super::*;
use layer_core::LayerId;
use layer_render_wgpu::snapshot::CaptureControl;
use std::{cell::RefCell, sync::Arc};

pub(super) async fn run(w: &Rc<Workspace>, id: u64) -> Result<bool, String> {
    let (epoch, revision, original, color, project, background, time) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        let document = session.engine().document();
        (
            session.state().document_file.epoch,
            document.revision,
            document
                .layer(LayerId(id))
                .and_then(|l| l.source.clone())
                .ok_or("No retained image")?,
            document.color,
            session.capture_project_recovery()?,
            session.state().camera.view().background_rgba_linear,
            session.engine().animation_time(),
        )
    };
    let comparison = super::preview::Comparison::new(project, w.view_color());
    comparison.invalidate("Converting the retained original…");
    let explanation = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .label("Preparing the comparison…")
        .build();
    explanation.set_widget_name("rasterize-reduction");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&explanation);
    content.append(&comparison.widget);
    let dialog = adw::AlertDialog::builder().heading("Rasterize Retained Source")
        .body(&format!("Convert the original image to {} · {}-bit pixels. Its full size and position, existing paint edits and masks stay intact. The original profile and precision remain available through Undo.", color.space.name(), color.depth.bits()))
        .prefer_wide_layout(true).content_width(520).extra_child(&content).build();
    dialog.set_widget_name("rasterize-source-dialog");
    dialog.add_responses(&[("cancel", "Cancel"), ("apply", "Rasterize")]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("cancel"));
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("apply", false);
    *comparison.changed.borrow_mut() = Some(Box::new(glib::clone!(
        #[weak]
        dialog,
        move |ready| {
            dialog.set_response_enabled("apply", ready);
        }
    )));
    let control = CaptureControl::default();
    let converted = Rc::new(RefCell::new(None));
    let worker_original = original.clone();
    let task = glib::MainContext::default().spawn_local(glib::clone!(
        #[weak] w,
        #[weak] explanation,
        #[strong] comparison,
        #[strong] control,
        #[strong] converted,
        async move {
            let token = control.clone();
            let result = gio::spawn_blocking(move || layer_color::rasterize_source(&worker_original, color, 512 * 1024 * 1024, || token.is_cancelled()))
                .await.map_err(|_| "Rasterization worker failed".to_string()).and_then(|r| r);
            if control.is_cancelled() { return; }
            let result = result.and_then(|(image, statistics)| {
                let gpu = w.gpu.borrow();
                let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
                if session.state().document_file.epoch != epoch || session.engine().document().revision != revision {
                    return Err("The document changed while rasterizing; try again".into());
                }
                let original = session.engine().document().layer(LayerId(id)).and_then(|l| l.source.as_ref()).ok_or("Source no longer exists")?;
                let image = Arc::new(image);
                let preview = session.preview_rasterized_source(LayerId(id), original, image.clone())?;
                explanation.set_label(if statistics.clipped_channels > 0 {
                    "Some source colors are outside the document color space and will be clipped. Compare the complete result before applying."
                } else { "Compare the complete result before applying. Rasterized images use the document’s editing precision." });
                *converted.borrow_mut() = Some(image);
                comparison.request(preview, background, time);
                Ok(())
            });
            if let Err(error) = result { comparison.invalidate(&error); }
        }
    ));
    let response = dialog.choose_future(Some(&w.window)).await;
    control.cancel();
    comparison.close();
    let _ = task.await;
    comparison.finish().await;
    if response != "apply" {
        return Ok(false);
    }
    let converted = converted
        .borrow_mut()
        .take()
        .ok_or("No rasterized image to apply")?;
    let mut gpu = w.gpu.borrow_mut();
    let session = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
    if session.state().document_file.epoch != epoch
        || session.engine().document().revision != revision
    {
        return Err("The document changed while rasterizing; try again".into());
    }
    session.apply_rasterized_source(LayerId(id), &original, converted)?;
    drop(gpu);
    w.wake();
    Ok(true)
}
