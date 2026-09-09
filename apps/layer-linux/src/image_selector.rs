//! A native, model-driven image choice grid. Selection is supplied by the core.
use adw::prelude::*;
use std::rc::Rc;

pub struct ImageSelector {
    pub widget: gtk::Grid,
    buttons: Vec<gtk::ToggleButton>,
}
impl ImageSelector {
    pub fn new(
        labels: &[String],
        icons: &[String],
        columns: u32,
        select: impl Fn(u32) + 'static,
    ) -> Self {
        assert_eq!(labels.len(), icons.len());
        assert!(columns > 0);
        let widget = gtk::Grid::builder()
            .column_spacing(6)
            .row_spacing(6)
            .halign(gtk::Align::Center)
            .build();
        widget.add_css_class("image-selector");
        let select = Rc::new(select);
        let mut buttons = Vec::<gtk::ToggleButton>::new();
        for (i, (label, icon)) in labels.iter().zip(icons).enumerate() {
            let image = gtk::Image::builder()
                .icon_name(format!("layer-{icon}-symbolic"))
                .pixel_size(48)
                .build();
            let button = gtk::ToggleButton::builder()
                .child(&image)
                .tooltip_text(label)
                .build();
            button.update_property(&[gtk::accessible::Property::Label(label)]);
            if let Some(first) = buttons.first() {
                button.set_group(Some(first));
            }
            // Click, not toggled: applying a core snapshot must not send edits.
            let select = select.clone();
            button.connect_clicked(move |_| select(i as u32));
            widget.attach(
                &button,
                i as i32 % columns as i32,
                i as i32 / columns as i32,
                1,
                1,
            );
            buttons.push(button);
        }
        Self { widget, buttons }
    }

    pub fn set_selected(&self, selected: u32) {
        if let Some(button) = self.buttons.get(selected as usize) {
            button.set_active(true);
        }
    }
}
