//! Explicit source commitment with complete-stack comparison and cancellation.
use super::*;
use layer_render_wgpu::snapshot::CaptureControl;
use std::cell::RefCell;

pub(super) async fn run(w: &Rc<Workspace>, id: u32) -> Result<bool, String> {
    let (workflow, background, time) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        (SourceWorkflow::begin(session, id)?, session.engine().view().background_rgba_linear, session.engine().animation_time())
    };
    let color = workflow.project.document.color;
    let project = workflow.project.clone();
    let original_gpu = w.snapshot_gpu()?;
    let workflow = Rc::new(RefCell::new(workflow));
    let comparison = super::preview::Comparison::new(w.snapshot_gpu()?, project, w.view_color());
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
    let worker_workflow = workflow.borrow().clone();
    let task = glib::MainContext::default().spawn_local(glib::clone!(
        #[weak] w,
        #[weak] explanation,
        #[strong] comparison,
        #[strong] control,
        #[strong] workflow,
        #[strong] original_gpu,
        async move {
            let token = control.clone();
            let result = gio::spawn_blocking(move || worker_workflow.prepare(None, layer_color::photo::PhotoMemoryBudget::current().source_bytes, || token.is_cancelled()))
                .await.map_err(|_| "Rasterization worker failed".to_string()).and_then(|r| r);
            if control.is_cancelled() { return; }
            let result = result.and_then(|(image, clipped)| {
                let gpu = w.gpu.borrow();
                let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
                let preview = workflow.borrow_mut().preview(session, image, control.is_cancelled(), original_gpu.same_device(&session.engine().backend().snapshot_gpu()?))?;
                explanation.set_label(if clipped > 0 {
                    "Some source colors are outside the document color space and will be clipped. Compare the complete result before applying."
                } else { "Compare the complete result before applying. Rasterized images use the document’s editing precision." });
                comparison.request(preview, background, time);
                Ok(())
            });
            if let Err(error) = result { comparison.invalidate(&error); }
        }
    ));
    let response = crate::alert::choose(dialog, &w.window).await;
    control.cancel();
    let compared = comparison.ready.get();
    comparison.close();
    let _ = task.await;
    comparison.finish().await;
    if response != "apply" {
        return Ok(false);
    }
    if !compared { return Err("No completed source comparison".into()); }
    let mut gpu = w.gpu.borrow_mut();
    let session = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
    let current = original_gpu.same_device(&session.engine().backend().snapshot_gpu()?);
    workflow.borrow_mut().comparison_completed()?;
    workflow.borrow_mut().commit(session, false, current)?;
    drop(gpu);
    w.wake();
    Ok(true)
}
