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
        .model(&gtk::StringList::new(values))
        .build();
    row.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));
    row.set_use_subtitle(true);
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
    let dialog = adw::AlertDialog::builder().heading("Soft Proof Setup")
        .body("Preview the printer and paper using its ICC profile. Choose the lab’s delivery profile separately when exporting. No printer connection is needed.")
        .prefer_wide_layout(true).content_width(560)
        .build();
    dialog.set_widget_name("soft-proof-setup");
    dialog.add_responses(&[
        ("cancel", "Cancel"),
        ("remove", "Remove Setup"),
        ("apply", "Prepare and Apply"),
    ]);
    dialog.set_response_enabled("remove", previous.is_some());
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("apply"));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_width_request(480);
    let group = adw::PreferencesGroup::new();
    let target = choice(
        "Proof profile",
        "proof-profile",
        &[
            "sRGB",
            "Display P3",
            "Adobe RGB (1998)",
            "ProPhoto RGB",
            "ICC profile…",
        ],
    );
    group.add(&target);
    let chooser = super::profile::ProfileChooser::new(
        &w.window,
        &target,
        working,
        super::profile::ProfilePurpose::Proof,
    );
    group.add(&chooser.row);
    let name = adw::EntryRow::builder()
        .title("Target name (optional)")
        .build();
    name.set_widget_name("proof-name");
    group.add(&name);
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
        .subtitle("Map the artwork’s black to the print target’s black")
        .active(true)
        .build();
    bpc.set_widget_name("proof-bpc");
    group.add(&bpc);
    let ink = adw::SwitchRow::builder()
        .title("Simulate black ink")
        .active(true)
        .build();
    ink.set_widget_name("proof-black-ink");
    group.add(&ink);
    let paper = adw::SwitchRow::builder()
        .title("Simulate paper color")
        .subtitle("Includes black-ink simulation")
        .build();
    paper.set_widget_name("proof-paper");
    group.add(&paper);
    let manage = gtk::Button::with_label("Manage ICC Profiles…");
    manage.set_widget_name("proof-manage-profiles");
    manage.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| {
            glib::MainContext::default().spawn_local(glib::clone!(
                #[weak]
                w,
                async move {
                    if let Err(error) = super::profile::manage(&w).await {
                        w.changed(Err(error));
                    }
                }
            ));
        }
    ));
    content.append(&group);
    content.append(&chooser.error);
    content.append(&manage);
    let detail = gtk::Label::builder().label("Applies to the canvas and Navigator only. Use Save As for a print variant. Colors beyond the SDR proof domain are clamped for preview and marked by Gamut Warning.")
        .wrap(true).xalign(0.).build();
    content.append(&detail);
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
    paper.connect_active_notify(glib::clone!(
        #[weak]
        ink,
        move |paper| {
            if paper.is_active() {
                ink.set_active(true);
            }
            ink.set_sensitive(!paper.is_active());
        }
    ));
    if let Some(recipe) = &previous {
        let embedded = recipe.profile.clone();
        let channels = gio::spawn_blocking(move || layer_color::profile_channels(&embedded))
            .await
            .map_err(|_| "Profile reader failed")?
            .unwrap_or(layer_core::color::ProfileChannels::Rgb);
        let profile = ExportProfile {
            profile: recipe.profile.clone(),
            name: recipe.name.clone(),
            channels,
        };
        (chooser.restore)(profile);
        name.set_text(&recipe.name);
        intent.set_selected(
            intents
                .iter()
                .position(|i| *i == recipe.conversion.intent)
                .unwrap() as u32,
        );
        bpc.set_active(recipe.conversion.black_point_compensation);
        ink.set_active(recipe.simulate_black_ink);
        paper.set_active(recipe.simulate_paper);
    } else {
        target.set_selected(4);
    }
    loop {
        let response = crate::alert::choose(dialog.clone(), &w.window).await;
        if response != "apply" && response != "remove" {
            return Ok(());
        }
        let result = (|| {
            let gpu = w.gpu.borrow();
            let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
            if session.state().document_file.epoch != epoch
                || session.engine().document().color.space != working
            {
                return Err("The drawing changed; reopen Soft Proof Setup".to_string());
            }
            if response == "remove" {
                return Ok(None);
            }
            let profile = (chooser.selected)(target.selected())?;
            let label = if name.text().trim().is_empty() {
                profile.name
            } else {
                name.text().trim().to_owned()
            };
            let mut recipe = ProofRecipe::new(label, profile.profile);
            recipe.conversion.intent = intents[intent.selected() as usize];
            recipe.conversion.black_point_compensation = bpc.is_active();
            recipe.simulate_paper = paper.is_active();
            recipe.simulate_black_ink = ink.is_active();
            recipe.validate()?;
            Ok(Some(recipe))
        })();
        let result = match result {
            Ok(None) => {
                let result = w
                    .gpu
                    .borrow_mut()
                    .as_mut()
                    .ok_or("Canvas unavailable")?
                    .session
                    .set_proof_recipe(None);
                w.changed(result);
                return Ok(());
            }
            Ok(Some(recipe)) => {
                w.proof.pause().await;
                let result = prepare(w, working, recipe.clone()).await.and_then(|lut| {
                    let Some(lut) = lut else {
                        return Ok(false);
                    };
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
                });
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
        .heading("Preparing soft proof")
        .body("Checking the profile and preparing its viewing transform…")
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
