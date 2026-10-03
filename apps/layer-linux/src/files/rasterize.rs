//! Explicit source commitment with complete-stack comparison and cancellation.
use super::*;
use layer_render_wgpu::snapshot::CaptureControl;
use std::cell::RefCell;

pub(super) async fn run(w: &Rc<Workspace>, id: u32) -> Result<bool, String> {
    let copy = std::rc::Rc::new(layer_ui::color_feature_copy::DocumentColorCopy::new(&w.localization()));
    let localization = w.localization().clone();
    let (workflow, background, time) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or_else(|| layer_ui::NewDocumentError::CanvasUnavailable.message(&w.localization()))?.session;
        (SourceWorkflow::begin(session, id)?, session.engine().view().background_rgba_linear, session.engine().animation_time())
    };
    let color = workflow.project.document.color;
    let project = workflow.project.clone();
    let original_gpu = w.snapshot_gpu()?;
    let workflow = Rc::new(RefCell::new(workflow));
    let comparison = super::preview::Comparison::new(w.snapshot_gpu()?, project, w.view_color(), &w.localization());
    comparison.bind_localization(w);
    comparison.invalidate(copy.rasterizing.as_ref());
    let explanation = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .label(copy.rasterize_comparison.as_ref())
        .build();
    explanation.set_widget_name("rasterize-reduction");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&explanation);
    content.append(&comparison.widget);
    let mut args = layer_ui::FluentArgs::new(); args.set("space", color.space.name());
    let bits = color.depth.bits().to_string(); args.set("bits", bits.as_str());
    let details = localization.format(layer_ui::MessageId::COLOR_FEATURES_COLOR_RASTERIZE_DETAILS, &args);
    let dialog = adw::AlertDialog::builder().heading(copy.rasterize_title.as_ref())
        .body(&details)
        .prefer_wide_layout(true).content_width(520).extra_child(&content).build();
    dialog.set_widget_name("rasterize-source-dialog");
    dialog.add_responses(&[("cancel", copy.common.cancel.as_ref()), ("apply", copy.rasterize.as_ref())]);
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
    let memory_budget = layer_color::photo::PhotoMemoryBudget::current().source_bytes;
    let clipped_state = Rc::new(std::cell::Cell::new(None::<bool>));
    let weak = dialog.downgrade(); let explanation_weak = explanation.downgrade(); let clipped = clipped_state.clone();
    w.on_localization(move |localization| {
        let Some(dialog) = weak.upgrade() else { return false };
        let copy = layer_ui::color_feature_copy::DocumentColorCopy::new(localization);
        dialog.set_heading(Some(&copy.rasterize_title));
        let mut args = layer_ui::FluentArgs::new(); args.set("space", color.space.name());
        let bits = color.depth.bits().to_string(); args.set("bits", bits.as_str());
        dialog.set_body(&localization.format(layer_ui::MessageId::COLOR_FEATURES_COLOR_RASTERIZE_DETAILS, &args));
        dialog.set_response_label("cancel", &copy.common.cancel); dialog.set_response_label("apply", &copy.rasterize);
        if let Some(explanation) = explanation_weak.upgrade() { explanation.set_label(match clipped.get() { Some(true) => &copy.source_clipped, Some(false) => &copy.rasterize_compare, None => &copy.rasterize_comparison }); }
        true
    });
    let task = glib::MainContext::default().spawn_local(glib::clone!( #[strong] clipped_state,
        #[weak] w,
        #[weak] explanation,
        #[strong] comparison,
        #[strong] control,
        #[strong] workflow,
        #[strong] original_gpu,
        async move {
            let token = control.clone();
            let result = gio::spawn_blocking(move || worker_workflow.prepare(None, memory_budget, || token.is_cancelled()))
                .await.map_err(|_| "Rasterization worker failed".to_string()).and_then(|r| r);
            if control.is_cancelled() { return; }
            let result = result.and_then(|(image, clipped)| {
                let gpu = w.gpu.borrow();
                let session = &gpu.as_ref().ok_or_else(|| layer_ui::NewDocumentError::CanvasUnavailable.message(&w.localization()))?.session;
                let preview = workflow.borrow_mut().preview(session, image, control.is_cancelled(), original_gpu.same_device(&session.engine().backend().snapshot_gpu()?))?;
                clipped_state.set(Some(clipped > 0));
                let copy = layer_ui::color_feature_copy::DocumentColorCopy::new(&w.localization());
                explanation.set_label(if clipped > 0 {
                    copy.source_clipped.as_ref()
                } else { copy.rasterize_compare.as_ref() });
                comparison.request(preview, background, time);
                Ok(())
            });
            if let Err(error) = result {
                let message = layer_ui::ColorFeatureError::Diagnostic(error).profile_message(&w.localization());
                comparison.invalidate(&message);
            }
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
    if !compared { return Err(w.localization().text(layer_ui::MessageId::DOCUMENTS_ERROR_SOURCE_NOT_PREPARED).to_string()); }
    let mut gpu = w.gpu.borrow_mut();
    let session = &mut gpu.as_mut().ok_or_else(|| layer_ui::NewDocumentError::CanvasUnavailable.message(&w.localization()))?.session;
    let current = original_gpu.same_device(&session.engine().backend().snapshot_gpu()?);
    workflow.borrow_mut().comparison_completed()?;
    workflow.borrow_mut().commit(session, false, current)?;
    drop(gpu);
    w.wake();
    Ok(true)
}
