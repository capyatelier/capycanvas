use super::*;

#[test]
#[ignore = "private GTK display: --native-test=native_dialog_content_prepares_constrained_measurement"]
fn native_dialog_content_prepares_constrained_measurement() {
    let app = crate::workspace::tests::native_test_app("art.capycanvas.DialogContentMeasure");
    let content = new(gtk::Orientation::Horizontal, 12);
    let first = gtk::DrawingArea::builder().width_request(160).height_request(40).build();
    let second = gtk::DrawingArea::builder().width_request(160).height_request(60).build();
    content.append(&first);
    content.append(&second);
    on_resize(&content, glib::clone!(#[weak] content, #[weak] first, #[weak] second, move |width| {
        let minimum = [first.measure(gtk::Orientation::Horizontal, -1).0 as f32,
            second.measure(gtk::Orientation::Horizontal, -1).0 as f32];
        content.set_orientation(if layer_ui::dialog_panes_fit(width as f32, minimum, 12.) {
            gtk::Orientation::Horizontal
        } else { gtk::Orientation::Vertical });
    }));
    assert_eq!(content.width(), 0);
    assert!(first.measure(gtk::Orientation::Horizontal, -1).0 + second.measure(gtk::Orientation::Horizontal, -1).0 + 12 > 280);
    let stacked = first.measure(gtk::Orientation::Vertical, 280).0 + second.measure(gtk::Orientation::Vertical, 280).0 + 12;
    let measured = content.measure(gtk::Orientation::Vertical, 280).0;
    assert_eq!(content.orientation(), gtk::Orientation::Vertical, "the constrained measure prepares the fitting layout before allocation");
    assert_eq!(measured, stacked, "the first requested height includes both stacked children and their spacing");
    assert!(content.first_child().unwrap().measure(gtk::Orientation::Horizontal, -1).0 <= 280);
    assert_eq!(content.width(), 0);

    content.set_size_request(280, -1);
    let window = gtk::ApplicationWindow::builder().application(&*app).decorated(false)
        .default_width(280).default_height(stacked).child(&content).build();
    window.present();
    crate::workspace::tests::until(|| content.is_mapped() && content.width() == 280 && second.height() > 0,
        "the first measured layout completes its native allocation");
    first.set_width_request(80);
    second.set_width_request(80);
    assert!(first.measure(gtk::Orientation::Horizontal, -1).0 + second.measure(gtk::Orientation::Horizontal, -1).0 + 12 <= 280);
    let horizontal = first.measure(gtk::Orientation::Vertical, 280).0.max(second.measure(gtk::Orientation::Vertical, 280).0);
    let measured = content.measure(gtk::Orientation::Vertical, 280).0;
    assert_eq!(content.orientation(), gtk::Orientation::Horizontal, "changed child minima are reconsidered at the same constrained width");
    assert_eq!(measured, horizontal, "the second requested height uses the current fitting row");
    assert!(measured < stacked);
    assert_eq!(content.width(), 280);
    window.close();
}
