//! Blending with real Mutter delivery: the New drawing dialog's field, Edit ›
//! Blending with its undo, and the Document Properties row, in the light and
//! dark themes.
use super::crop::fill_rect;
use super::new_photo::{capture_ui, combo, finish, response};
use super::photo_edit::{choose, document, labelled, shown, start};
use super::*;
use layer_core::BlendSpace;

fn blending_row(w: &Workspace) -> adw::ComboRow {
    find_named(w.window.upcast_ref(), "new-document-blending")
        .and_downcast::<adw::ComboRow>()
        .expect("the Blending row")
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_blending_new_drawing_edit_menu_and_properties() {
    let (_app, w, mut input) = start("art.capycanvas.Blending");
    let directory = std::path::Path::new("../../artifacts/photo-m4/blending").join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    assert_eq!(document(&w).composition().blend, BlendSpace::Perceptual, "a new 8-bit drawing blends perceptually");
    fill_rect(&w, [0.2, 0.2, 0.8, 0.8]);
    w.dispatch(UiAction::SetLayerOpacity { id: None, opacity: 0.5 });
    let center = [document(&w).composition().size[0] as f32 / 2., document(&w).composition().size[1] as f32 / 2.];
    pump(300);
    let perceptual = shown(&w, center);

    choose(&w, &mut input, "Edit", &["Blending", "Linear Light Blending"]);
    until(|| document(&w).composition().blend == BlendSpace::Linear, "Edit › Blending › Linear Light Blending");
    pump(300);
    let linear = shown(&w, center);
    assert!(linear[0] > perceptual[0] + 15, "half-opacity paint over white is lighter in linear light: {perceptual:?} {linear:?}");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).composition().blend == BlendSpace::Perceptual, "one undo step restores Perceptual");
    pump(300);
    assert_eq!(shown(&w, center), perceptual, "undo restores the composite");

    w.dispatch(UiAction::Invoke { command: CommandId::DocumentProperties });
    until(|| w.window.visible_dialog().is_some(), "Document Properties");
    let dialog = w.window.visible_dialog().unwrap();
    until(|| labelled(dialog.upcast_ref(), "Blending").is_some() && labelled(dialog.upcast_ref(), "Perceptual").is_some(), "the Blending row");
    response(&w, "done");
    finish(&w);

    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(200);
        let name = format!("{theme:?}").to_lowercase();
        w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
        until(|| w.window.visible_dialog().is_some(), "New drawing");
        find_named(w.window.upcast_ref(), "new-document-color")
            .and_downcast::<adw::ExpanderRow>()
            .unwrap()
            .set_expanded(true);
        assert_eq!(blending_row(&w).selected(), 0, "Perceptual by default");
        assert!(blending_row(&w).is_sensitive());
        assert_eq!(blending_row(&w).subtitle().as_deref(), Some("Like Photoshop and Clip Studio Paint"));
        pump(250);
        capture_ui(&w, &directory, &format!("new-drawing-{name}.png"));
        combo(&w, "new-document-depth").set_selected(2);
        assert!(!blending_row(&w).is_sensitive(), "float documents blend in linear light");
        assert_eq!(blending_row(&w).selected(), 1);
        assert_eq!(blending_row(&w).subtitle().as_deref(), Some("Float documents blend in linear light"));
        pump(250);
        capture_ui(&w, &directory, &format!("new-drawing-float-{name}.png"));
        combo(&w, "new-document-depth").set_selected(0);
        assert_eq!(blending_row(&w).selected(), 0, "the choice returns with an 8-bit depth");
        response(&w, "cancel");
        finish(&w);
    }

    let created = Rc::new(RefCell::new(None));
    let result = created.clone();
    *w.open_document.borrow_mut() = Some(Rc::new(move |project, _| {
        result.replace(Some(project));
    }));
    w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
    until(|| w.window.visible_dialog().is_some(), "New drawing");
    blending_row(&w).set_selected(1);
    assert_eq!(blending_row(&w).subtitle().as_deref(), Some("Physically based"));
    response(&w, "create");
    finish(&w);
    let project = created.borrow_mut().take().expect("a new drawing");
    assert_eq!(project.composition().blend, BlendSpace::Linear);
    input.finish();
    w.window.destroy();
    pump(100);
}
