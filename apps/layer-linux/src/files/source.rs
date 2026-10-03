//! Correct retained source interpretation without changing its original samples.
use super::profile::{ProfileChooser, ProfilePurpose};
use super::*;
use std::cell::RefCell;

pub(super) async fn repair(w: &Rc<Workspace>, id: u32) -> Result<bool, String> {
    let copy = std::rc::Rc::new(layer_ui::color_feature_copy::DocumentColorCopy::new(&w.localization()));
    let localization = w.localization().clone();
    let (workflow, background, time) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        (SourceWorkflow::begin(session, id)?, session.engine().view().background_rgba_linear, session.engine().animation_time())
    };
    let original = workflow.original.clone();
    let project = workflow.project.clone();
    let working = project.document.color.space;
    let baked = workflow.adds_layer();
    let workflow = Rc::new(RefCell::new(workflow));
    let original_gpu = w.snapshot_gpu()?;
    let current = original.interpretation.profile.clone();
    let description = gio::spawn_blocking(move || layer_color::profile_description_optional(&current))
        .await
        .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile reader failed".into()).profile_message(&w.localization()))?.map_err(|reason| layer_ui::ColorFeatureError::Diagnostic(reason).profile_message(&w.localization()))?;
    let profile_description = description.clone();
    let description = description.unwrap_or_else(|| localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).to_string());
    let mut args = layer_ui::FluentArgs::new(); args.set("name", description.as_str());
    args.set("assumed", if original.interpretation.profile_assumed {"yes"} else {"no"});
    let description = localization.format(layer_ui::MessageId::COLOR_FEATURES_COLOR_SOURCE_NAME, &args);
    let group = adw::PreferencesGroup::new();
    let current = adw::ActionRow::builder()
        .title(copy.current_source.as_ref())
        .subtitle(&description)
        .subtitle_selectable(true)
        .build();
    current.set_use_markup(false);
    group.add(&current);
    let chooser = ProfileChooser::new(w, copy.correct_profile.as_ref(), "source-profile-space", working,
        ProfilePurpose::Source(original.interpretation.clone()));
    let space = chooser.row.clone();
    group.add(&space);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&group);
    content.append(&chooser.error);
    let hint = gtk::Label::builder().wrap(true).xalign(0.).build();
    hint.set_widget_name("source-profile-hint");
    content.append(&hint);
    let comparison = super::preview::Comparison::new(w.snapshot_gpu()?, project, w.view_color(), &w.localization());
    comparison.bind_localization(w);
    content.append(&comparison.widget);
    let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(570)
        .child(&content)
        .build());
    let dialog = adw::AlertDialog::builder().heading(copy.repair_title.as_ref())
        .body(if baked {
            copy.source_baked_help.as_ref()
        } else {
            copy.source_native_help.as_ref()
        }).prefer_wide_layout(true).content_width(520).extra_child(&scroll).build();
    dialog.set_widget_name("source-profile-dialog");
    dialog.add_responses(&[
        ("cancel", copy.common.cancel.as_ref()),
        (
            "apply",
            if baked {
                copy.add_source.as_ref()
            } else {
                copy.apply_profile.as_ref()
            },
        ),
    ]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("cancel"));
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    *comparison.changed.borrow_mut() = Some(Box::new(glib::clone!(
        #[weak]
        dialog,
        move |ready| {
            dialog.set_response_enabled("apply", ready);
        }
    )));
    let weak = dialog.downgrade(); let current = current.downgrade(); let space_weak = space.downgrade();
    let assumed = original.interpretation.profile_assumed;
    w.on_localization(move |localization| {
        let Some(dialog) = weak.upgrade() else { return false };
        let copy = layer_ui::color_feature_copy::DocumentColorCopy::new(localization);
        dialog.set_heading(Some(&copy.repair_title));
        dialog.set_body(if baked { &copy.source_baked_help } else { &copy.source_native_help });
        dialog.set_response_label("cancel", &copy.common.cancel);
        dialog.set_response_label("apply", if baked { &copy.add_source } else { &copy.apply_profile });
        if let Some(space) = space_weak.upgrade() { space.set_title(&copy.correct_profile); }
        if let Some(current) = current.upgrade() {
            current.set_title(&copy.current_source);
            let name = profile_description.clone().unwrap_or_else(|| localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).to_string());
            let mut args = layer_ui::FluentArgs::new(); args.set("name", name.as_str()); args.set("assumed", if assumed { "yes" } else { "no" });
            current.set_subtitle(&localization.format(layer_ui::MessageId::COLOR_FEATURES_COLOR_SOURCE_NAME, &args));
        }
        true
    });
    let selected = chooser.selected.clone();
    chooser.connect_changed(glib::clone!(
        #[weak]
        w,
        #[weak]
        comparison,
        #[weak]
        hint,
        #[strong]
        workflow,
        #[strong]
        original_gpu,
        move || {
            let result = selected().and_then(|profile| {
                let (corrected, _) = workflow.borrow().prepare(Some(profile.profile), layer_color::photo::PhotoMemoryBudget::current().encode_bytes, || false)?;
                let gpu = w.gpu.borrow();
                let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
                workflow.borrow_mut().preview(session, corrected, false, original_gpu.same_device(&session.engine().backend().snapshot_gpu()?))
            });
            match result {
                Ok(project) => {
                    hint.set_visible(false);
                    comparison.request(project, background, time);
                }
                Err(message) => {
                    hint.set_label(&message);
                    hint.set_visible(true);
                    comparison.invalidate(layer_ui::color_feature_copy::DocumentColorCopy::new(&w.localization()).choose_valid_profile.as_ref());
                }
            }
        }
    ));
    let embedded = original.interpretation.profile.clone();
    let profile = gio::spawn_blocking(move || super::profile::describe(embedded)).await
        .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile reader failed".into()).profile_message(&w.localization()))?.map_err(|reason| reason.profile_message(&w.localization()))?;
    (chooser.restore)(profile);
    let response = crate::alert::choose(dialog, &w.window).await;
    let compared = comparison.ready.get();
    comparison.close();
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
