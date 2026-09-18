//! Native controls shared by compact panels and export previews.
use adw::prelude::*;

pub const SPACING: i32 = layer_ui::WORKSPACE_SPACING as i32;

pub fn column() -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, SPACING)
}

/// Native action/choice row using the same compact height as panel numbers.
pub fn action_row() -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, SPACING);
    row.add_css_class("panel-control-row");
    row
}

/// Native menu choice, sized like the adjacent dropdowns. Long profile names
/// ellipsize instead of widening or clipping the panel; the tooltip keeps them.
pub fn menu_choice(button: &gtk::MenuButton, text: &str) {
    button.set_label(text);
    button.set_always_show_arrow(true);
    button.set_can_shrink(true);
    button.add_css_class("panel-choice");
}

pub fn check(label: &str) -> gtk::CheckButton {
    let check = gtk::CheckButton::with_label(label);
    if let Some(label) = check.child().and_downcast::<gtk::Label>() {
        label.set_wrap(true);
        label.set_xalign(0.);
        label.set_max_width_chars(1);
    }
    check
}

/// Selected text can shrink in compact rows; the popup retains full labels.
pub fn dropdown(choices: &[&str]) -> gtk::DropDown {
    let dropdown = gtk::DropDown::from_strings(choices);
    dropdown.set_factory(Some(&choice_factory(true)));
    dropdown.set_list_factory(Some(&choice_factory(false)));
    dropdown
}

fn choice_factory(shrink: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::new(None);
        label.set_xalign(0.);
        if shrink {
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_width_chars(1);
        }
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let value = item.item().and_downcast::<gtk::StringObject>().unwrap();
        let label = item.child().and_downcast::<gtk::Label>().unwrap();
        label.set_label(&value.string());
        label.set_tooltip_text(Some(&value.string()));
    });
    factory
}

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
    let row = action_row();
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.);
    label.set_width_chars(9);
    label.set_mnemonic_widget(Some(control));
    row.append(&label);
    control.set_hexpand(true);
    row.append(control);
    row
}
