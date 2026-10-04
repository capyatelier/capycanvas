//! Correct retained source interpretation without changing its original samples.
use super::profile::{ProfileChooser, ProfilePurpose};
use super::*;
use std::cell::RefCell;

enum SourceProfileHint {
    Reading,
    Profile(layer_ui::ColorFeatureError),
    Diagnostic(String),
}
impl SourceProfileHint {
    fn message(&self, localization: &layer_ui::Localizer) -> String {
        match self {
            Self::Reading => localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_READING).to_string(),
            Self::Profile(reason) => reason.profile_message(localization),
            Self::Diagnostic(reason) => reason.clone(),
        }
    }
}

pub(super) async fn repair(w: &Rc<Workspace>, id: u32) -> Result<bool, String> {
    let copy = std::rc::Rc::new(layer_ui::color_feature_copy::DocumentColorCopy::new(&w.localization()));
    let localization = w.localization().clone();
    let (mut workflow, context) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        (SourceWorkflow::begin(session, id)?, session.engine().backend().capture_context()?)
    };
    workflow.context = context.resolve().await?;
    let original = workflow.original.clone();
    let project = workflow.project.clone();
    let working = project.composition().color.space;
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
    let chooser = Rc::new(ProfileChooser::new(w, copy.correct_profile.as_ref(), "source-profile-space", working,
        ProfilePurpose::Source(original.interpretation.clone())));
    let space = chooser.row.clone();
    group.add(&space);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&group);
    content.append(&chooser.error);
    let hint = gtk::Label::builder().wrap(true).xalign(0.).build();
    hint.set_widget_name("source-profile-hint");
    let hint_state = Rc::new(RefCell::new(None::<SourceProfileHint>));
    content.append(&hint);
    let comparison = super::preview::Comparison::new(w.snapshot_gpu()?, project, workflow.borrow().context.clone(), w.view_color(), &w.localization());
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
    let hint_weak = hint.downgrade(); let localized_hint = hint_state.clone();
    w.on_localization(move |localization| {
        let Some(dialog) = weak.upgrade() else { return false };
        let copy = layer_ui::color_feature_copy::DocumentColorCopy::new(localization);
        dialog.set_heading(Some(&copy.repair_title));
        dialog.set_body(if baked { &copy.source_baked_help } else { &copy.source_native_help });
        dialog.set_response_label("cancel", &copy.common.cancel);
        dialog.set_response_label("apply", if baked { &copy.add_source } else { &copy.apply_profile });
        if let Some(space) = space_weak.upgrade() { space.set_title(&copy.correct_profile); }
        if let Some(hint) = hint_weak.upgrade() && let Some(reason) = localized_hint.borrow().as_ref() { hint.set_label(&reason.message(localization)); }
        if let Some(current) = current.upgrade() {
            current.set_title(&copy.current_source);
            let name = profile_description.clone().unwrap_or_else(|| localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).to_string());
            let mut args = layer_ui::FluentArgs::new(); args.set("name", name.as_str()); args.set("assumed", if assumed { "yes" } else { "no" });
            current.set_subtitle(&localization.format(layer_ui::MessageId::COLOR_FEATURES_COLOR_SOURCE_NAME, &args));
        }
        true
    });
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
        #[weak]
        chooser,
        #[strong]
        hint_state,
        move || {
            let result = if chooser.is_pending() { Err(SourceProfileHint::Reading) } else {
                (chooser.selected_typed)().map_err(SourceProfileHint::Profile)
            }.and_then(|profile| {
                let (corrected, _) = workflow.borrow().prepare(Some(profile.profile), layer_color::photo::PhotoMemoryBudget::current().encode_bytes, || false).map_err(SourceProfileHint::Diagnostic)?;
                let gpu = w.gpu.borrow();
                let session = &gpu.as_ref().ok_or_else(|| SourceProfileHint::Diagnostic("Canvas unavailable".into()))?.session;
                let current = original_gpu.same_device(&session.engine().backend().snapshot_gpu().map_err(SourceProfileHint::Diagnostic)?);
                workflow.borrow_mut().preview(session, corrected, false, current).map_err(SourceProfileHint::Diagnostic)
            });
            match result {
                Ok(project) => {
                    hint_state.borrow_mut().take();
                    hint.set_visible(false);
                    comparison.request(project);
                }
                Err(reason) => {
                    hint.set_label(&reason.message(&w.localization()));
                    *hint_state.borrow_mut() = Some(reason);
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
