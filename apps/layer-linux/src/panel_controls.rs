//! Native controls shared by compact panels and export previews.
use adw::prelude::*;

/// A mutually exclusive choice with native keyboard, pointer and accessibility
/// behavior. Names are stable model identifiers; labels are presentation only.
pub fn segmented(name: &str, choices: &[(&str, &str)]) -> adw::ToggleGroup {
    let group = adw::ToggleGroup::builder()
        .homogeneous(true)
        .can_shrink(true)
        .hexpand(true)
        .build();
    group.set_widget_name(name);
    group.add_css_class("flat");
    for &(id, label) in choices {
        group.add(adw::Toggle::builder().name(id).label(label).build());
    }
    group
}

/// Standard single-line label/control arrangement, also used for compact
/// numeric controls. No panel-specific drawing, input handling or CSS.
pub fn row(title: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.);
    label.set_width_chars(9);
    label.set_mnemonic_widget(Some(control));
    row.append(&label);
    control.set_hexpand(true);
    row.append(control);
    row
}
