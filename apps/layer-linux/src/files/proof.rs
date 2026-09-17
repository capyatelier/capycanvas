//! Printer/paper simulation is independent of delivery recipes and file export.
use super::*;
use layer_core::color::{ProofRecipe, RenderingIntent, RgbSpace};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn choice(title: &str, name: &str, values: &[&str]) -> adw::ComboRow {
    let row = adw::ComboRow::builder()
        .title(title)
        .use_subtitle(true)
        .build();
    row.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));
    row.set_model(Some(&gtk::StringList::new(values)));
    row.set_selected(0);
    row.set_widget_name(name);
    row
}

pub(super) async fn run(w: &Rc<Workspace>) -> Result<(), String> {
    let (epoch, working, previous) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        (
            session.state().document_file.epoch,
            session.engine().document().color.space,
            session.engine().document().proof.clone(),
        )
    };
    let dialog = adw::AlertDialog::builder()
        .heading("Proof Setup")
        .body("Preview how colors will look in print.")
        .prefer_wide_layout(true)
        .content_width(480)
        .build();
    dialog.set_widget_name("soft-proof-setup");
    dialog.add_responses(&[("cancel", "Cancel"), ("apply", "Apply")]);
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("apply"));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let group = adw::PreferencesGroup::new();
    let chooser = super::profile::ProfileChooser::new(
        w,
        "Proof profile",
        "proof-profile",
        working,
        super::profile::ProfilePurpose::Proof,
    );
    group.add(&chooser.row);
    let intents = [
        RenderingIntent::RelativeColorimetric,
        RenderingIntent::Perceptual,
        RenderingIntent::Saturation,
        RenderingIntent::AbsoluteColorimetric,
    ];
    let intent = choice(
        "Rendering intent",
        "proof-intent",
        &[
            "Relative colorimetric",
            "Perceptual",
            "Saturation",
            "Absolute colorimetric",
        ],
    );
    group.add(&intent);
    let bpc = adw::SwitchRow::builder()
        .title("Black point compensation")
        .active(true)
        .build();
    bpc.set_tooltip_text(Some(
        "Preserve shadow detail when mapping colors to the print profile.",
    ));
    bpc.set_widget_name("proof-bpc");
    group.add(&bpc);
    let simulation = choice(
        "Print simulation",
        "proof-simulation",
        &["Colors only", "Black ink", "Paper and ink"],
    );
    simulation.set_selected(1);
    group.add(&simulation);
    intent.set_tooltip_text(Some("How colors outside the printer’s range are mapped."));
    content.append(&group);
    content.append(&chooser.error);
    let issue = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .visible(false)
        .build();
    issue.add_css_class("error");
    issue.set_widget_name("proof-setup-error");
    content.append(&issue);
    dialog.set_extra_child(Some(&content));
    intent.connect_selected_notify(glib::clone!(
        #[weak]
        bpc,
        move |intent| {
            let absolute = intent.selected() == 3;
            if absolute {
                bpc.set_active(false);
            }
            bpc.set_sensitive(!absolute);
        }
    ));
    if let Some(recipe) = &previous {
        let embedded = recipe.profile.clone();
        match gio::spawn_blocking(move || super::profile::describe(embedded))
            .await
            .map_err(|_| "Profile reader failed".to_string())
            .and_then(|r| r)
        {
            Ok(mut profile) => {
                if profile.name == super::profile::UNNAMED_PROFILE {
                    profile.name = recipe.name.clone();
                }
                chooser.restore_document(profile);
            }
            Err(error) => {
                chooser.error.set_label(&error);
                chooser.error.set_visible(true);
            }
        }
        intent.set_selected(
            intents
                .iter()
                .position(|i| *i == recipe.conversion.intent)
                .unwrap() as u32,
        );
        bpc.set_active(recipe.conversion.black_point_compensation);
        simulation.set_selected(if recipe.simulate_paper {
            2
        } else {
            u32::from(recipe.simulate_black_ink)
        });
    }
    dialog.set_response_enabled("apply", (chooser.selected)().is_ok());
    chooser.row.connect_subtitle_notify(glib::clone!(
        #[weak]
        dialog,
        #[strong(rename_to = selected)]
        chooser.selected,
        move |_| dialog.set_response_enabled("apply", selected().is_ok())
    ));
    loop {
        let response = crate::alert::choose(dialog.clone(), &w.window).await;
        if response != "apply" {
            return Ok(());
        }
        let result = (|| {
            let gpu = w.gpu.borrow();
            let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
            if session.state().document_file.epoch != epoch
                || session.engine().document().color.space != working
            {
                return Err("The drawing changed; reopen Proof Setup".to_string());
            }
            let profile = (chooser.selected)()?;
            let mut recipe = ProofRecipe::new(profile.name, profile.profile);
            recipe.conversion.intent = intents[intent.selected() as usize];
            recipe.conversion.black_point_compensation = bpc.is_active();
            recipe.simulate_paper = simulation.selected() == 2;
            recipe.simulate_black_ink = simulation.selected() != 0;
            recipe.validate()?;
            Ok(recipe)
        })();
        let result = match result {
            Ok(recipe) => {
                w.proof.pause().await;
                let result = async {
                    let Some(lut) = prepare(w, working, recipe.clone()).await? else {
                        return Ok(false);
                    };
                    super::profile::preserve_replaced_proof(previous.as_ref(), &recipe).await?;
                    let result = {
                        let mut gpu = w.gpu.borrow_mut();
                        let session = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
                        if session.state().document_file.epoch != epoch
                            || session.engine().document().color.space != working
                        {
                            return Err("The drawing changed while preparing the proof".into());
                        }
                        session.set_proof_recipe(Some(recipe.clone()))?
                    };
                    w.proof.retain(working, recipe, lut);
                    w.changed(Ok(result));
                    Ok(true)
                }
                .await;
                w.proof.resume(w);
                result
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(true) => return Ok(()),
            Ok(false) => (),
            Err(error) => {
                issue.set_text(&error);
                issue.set_visible(true);
            }
        }
    }
}

async fn prepare(
    w: &Workspace,
    space: RgbSpace,
    recipe: ProofRecipe,
) -> Result<Option<Arc<layer_color::ProofLut>>, String> {
    let progress = adw::AlertDialog::builder()
        .heading("Preparing preview…")
        .build();
    progress.set_widget_name("proof-progress");
    progress.add_response("cancel", "Cancel");
    progress.set_close_response("cancel");
    let cancelled = Arc::new(AtomicBool::new(false));
    progress.connect_response(None, {
        let cancelled = cancelled.clone();
        move |_, _| cancelled.store(true, Ordering::Release)
    });
    progress.present(Some(&w.window));
    let worker_cancelled = cancelled.clone();
    let result = gio::spawn_blocking(move || {
        layer_color::ProofLut::build(space, &recipe, || worker_cancelled.load(Ordering::Acquire))
    })
    .await
    .map_err(|_| "Proof preparation worker failed".to_string())
    .and_then(|r| r);
    let was_cancelled = cancelled.load(Ordering::Acquire);
    progress.force_close();
    if was_cancelled {
        Ok(None)
    } else {
        result.map(|lut| Some(Arc::new(lut)))
    }
}
