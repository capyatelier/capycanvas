use adw::prelude::*;
use layer_ui::Swatch;
use std::{
    cell::{Cell, RefCell},
    fmt::Write,
    rc::Rc,
};

pub struct SwatchSelector {
    pub widget: gtk::Box,
    pub entry: gtk::Entry,
    pub focus: gtk::EventControllerFocus,
    buttons: Vec<(gtk::ToggleButton, gtk::Image)>,
    editor: gtk::Revealer,
    editing: Cell<bool>,
    custom_index: usize,
    selected: Cell<usize>,
    value: RefCell<String>,
    custom: RefCell<String>,
    scope: String,
    name: String,
    colors: gtk::CssProvider,
    style: RefCell<String>,
}
impl Drop for SwatchSelector {
    fn drop(&mut self) {
        gtk::style_context_remove_provider_for_display(&self.widget.display(), &self.colors);
    }
}
impl SwatchSelector {
    pub fn new(
        scope: &str,
        name: &str,
        title: &str,
        swatches: &[Swatch],
        placeholder: &str,
        inline: bool,
        select: impl Fn(String) + 'static,
    ) -> Rc<Self> {
        let widget = if inline {
            gtk::Box::new(gtk::Orientation::Horizontal, 8)
        } else {
            gtk::Box::new(gtk::Orientation::Vertical, 0)
        };
        widget.add_css_class("swatch-selector");
        let entry = gtk::Entry::builder()
            .width_chars(9)
            .max_width_chars(9)
            .max_length(7)
            .placeholder_text(placeholder)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .margin_top(if inline { 0 } else { 12 })
            .build();
        entry.add_css_class("preference-entry");
        entry.set_widget_name(&format!("setting-text-{name}"));
        entry.update_property(&[gtk::accessible::Property::Label(&format!(
            "Custom {}",
            title.to_lowercase()
        ))]);
        let focus = gtk::EventControllerFocus::new();
        entry.add_controller(focus.clone());
        let editor = gtk::Revealer::builder()
            .child(&entry)
            .transition_type(if inline {
                gtk::RevealerTransitionType::SlideLeft
            } else {
                gtk::RevealerTransitionType::SlideDown
            })
            .build();
        let circles: gtk::Widget = if inline {
            widget.add_css_class("inline");
            widget.set_valign(gtk::Align::Center);
            widget.append(&editor);
            gtk::Box::new(gtk::Orientation::Horizontal, 10).upcast()
        } else {
            gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .column_spacing(10)
                .row_spacing(10)
                .max_children_per_line(swatches.len() as u32)
                .halign(gtk::Align::Center)
                .build()
                .upcast()
        };
        widget.append(&circles);
        if !inline {
            widget.append(&editor);
        }
        let colors = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &colors,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        let mut buttons = Vec::<(gtk::ToggleButton, gtk::Image)>::new();
        for (i, swatch) in swatches.iter().enumerate() {
            let image = gtk::Image::builder().pixel_size(16).build();
            let button = gtk::ToggleButton::builder()
                .child(&image)
                .tooltip_text(&swatch.label)
                .valign(gtk::Align::Center)
                .build();
            button.set_widget_name(&format!("setting-{name}-swatch-{i}"));
            button.update_property(&[gtk::accessible::Property::Label(&swatch.label)]);
            if let Some((first, _)) = buttons.first() {
                button.set_group(Some(first));
            }
            if let Some(flow) = circles.downcast_ref::<gtk::FlowBox>() {
                let child = gtk::FlowBoxChild::builder()
                    .child(&button)
                    .focusable(false)
                    .build();
                flow.append(&child);
            } else {
                circles.downcast_ref::<gtk::Box>().unwrap().append(&button);
            }
            buttons.push((button, image));
        }
        let this = Rc::new(Self {
            widget,
            entry,
            focus,
            buttons,
            editor,
            editing: Cell::new(false),
            custom_index: swatches.iter().position(|s| s.custom).unwrap(),
            selected: Cell::new(0),
            value: RefCell::default(),
            custom: RefCell::default(),
            scope: scope.into(),
            name: name.into(),
            colors,
            style: RefCell::default(),
        });
        let select = Rc::new(select);
        for (i, swatch) in swatches.iter().enumerate() {
            let weak = Rc::downgrade(&this);
            let select = select.clone();
            let (custom, value) = (swatch.custom, swatch.value.clone());
            this.buttons[i].0.connect_clicked(move |_| {
                let Some(this) = weak.upgrade() else { return };
                if custom {
                    this.edit_custom();
                } else {
                    this.editing.set(false);
                    select(value.clone());
                }
            });
        }
        this.update(swatches, 0, "", placeholder);
        this
    }

    fn edit_custom(&self) {
        self.editing.set(true);
        self.sync();
        self.entry.set_text(&self.custom.borrow());
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    pub fn custom(&self) -> String {
        self.custom.borrow().clone()
    }

    pub fn update(&self, swatches: &[Swatch], selected: u32, value: &str, custom: &str) {
        let mut style = String::new();
        for (i, swatch) in swatches.iter().enumerate() {
            if let Some(color) = swatch.color {
                write!(
                    style,
                    "window#{} #setting-{}-swatch-{i} {{ background-color: {color}; }}",
                    self.scope, self.name
                )
                .unwrap();
            }
        }
        if *self.style.borrow() != style {
            self.colors.load_from_string(&style);
            *self.style.borrow_mut() = style;
        }
        for (i, ((button, image), swatch)) in self.buttons.iter().zip(swatches).enumerate() {
            let icon = swatch
                .icon
                .as_deref()
                .or((i == selected as usize).then_some("check"))
                .map(|icon| format!("layer-{icon}-symbolic"));
            crate::icons::set_colored(image, icon.as_deref(), swatch.foreground);
            button.set_tooltip_text(Some(&swatch.label));
        }
        if self.value.replace(value.into()) != value {
            self.editing.set(false);
        }
        self.selected.set(selected as usize);
        *self.custom.borrow_mut() = custom.into();
        if !self.focus.contains_focus() && self.entry.text() != custom {
            self.entry.set_text(custom);
        }
        self.sync();
    }

    fn sync(&self) {
        let active = if self.editing.get() {
            self.custom_index
        } else {
            self.selected.get()
        };
        self.buttons[active].0.set_active(true);
        self.editor.set_reveal_child(active == self.custom_index);
    }
}
