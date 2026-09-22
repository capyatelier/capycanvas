//! Correct retained source interpretation without changing its original samples.
use super::profile::{ProfileChooser, ProfilePurpose};
use super::*;
use std::cell::RefCell;

pub(super) async fn repair(w: &Rc<Workspace>, id: u32) -> Result<bool, String> {
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
    let description = gio::spawn_blocking(move || layer_color::profile_description(&current))
        .await
        .map_err(|_| "Profile reader failed")??;
    let group = adw::PreferencesGroup::new();
    let current = adw::ActionRow::builder()
        .title("Current source profile")
        .subtitle(&format!(
            "{description}{}",
            if original.interpretation.profile_assumed {
                " (assumed)"
            } else {
                ""
            }
        ))
        .subtitle_selectable(true)
        .build();
    current.set_use_markup(false);
    group.add(&current);
    let chooser = ProfileChooser::new(w, "Correct source profile", "source-profile-space", working,
        ProfilePurpose::Source(original.interpretation.clone()));
    let space = chooser.row.clone();
    group.add(&space);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&group);
    content.append(&chooser.error);
    let hint = gtk::Label::builder().wrap(true).xalign(0.).build();
    hint.set_widget_name("source-profile-hint");
    content.append(&hint);
    let comparison = super::preview::Comparison::new(w.snapshot_gpu()?, project, w.view_color());
    content.append(&comparison.widget);
    let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(570)
        .child(&content)
        .build());
    let dialog = adw::AlertDialog::builder().heading("Repair Source Profile")
        .body(if baked {
            "This layer has pixel edits. Add a corrected original as a new layer at the same position. The existing layer keeps its edits, masks and adjustments."
        } else {
            "Change how the original image’s color numbers are interpreted. Original samples and depth stay intact. You can undo the change."
        }).prefer_wide_layout(true).content_width(520).extra_child(&scroll).build();
    dialog.set_widget_name("source-profile-dialog");
    dialog.add_responses(&[
        ("cancel", "Cancel"),
        (
            "apply",
            if baked {
                "Add Corrected Source"
            } else {
                "Apply Profile"
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
    let selected = chooser.selected.clone();
    space.connect_subtitle_notify(glib::clone!(
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
        move |_| {
            let result = selected().and_then(|profile| {
                let (corrected, _) = workflow.borrow().prepare(Some(profile.profile), 512 * 1024 * 1024, || false)?;
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
                    comparison.invalidate("Choose a valid profile to preview the complete canvas.");
                }
            }
        }
    ));
    let embedded = original.interpretation.profile.clone();
    let profile = gio::spawn_blocking(move || super::profile::describe(embedded)).await
        .map_err(|_| "Profile reader failed")??;
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
