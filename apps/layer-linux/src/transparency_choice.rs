use adw::prelude::*;
use gtk::cairo;
use layer_ui::Transparency;
use std::{cell::Cell, f64::consts::TAU, rc::Rc};

const CHECKS: i32 = 4;

pub struct TransparencyChoice {
    pub widget: gtk::Box,
    buttons: Vec<(gtk::ToggleButton, gtk::DrawingArea, Rc<Cell<bool>>)>,
}

impl TransparencyChoice {
    pub fn new(name: &str, options: &[String], select: impl Fn(u32) + 'static) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        widget.add_css_class("swatch-selector");
        widget.add_css_class("inline");
        widget.add_css_class("transparency-choice");
        widget.set_valign(gtk::Align::Center);
        let select = Rc::new(select);
        let mut buttons: Vec<(gtk::ToggleButton, gtk::DrawingArea, Rc<Cell<bool>>)> = Vec::new();
        for (i, label) in options.iter().enumerate() {
            let level = Transparency::CHOICES[i].0;
            let checked = Rc::new(Cell::new(false));
            let preview = gtk::DrawingArea::builder().content_width(28).content_height(28).build();
            preview.add_css_class("transparency-preview");
            let state = checked.clone();
            preview.set_draw_func(move |area, cr, width, height| {
                draw(area, cr, width, height, level, state.get())
            });
            let button = gtk::ToggleButton::builder()
                .child(&preview)
                .tooltip_text(label)
                .valign(gtk::Align::Center)
                .build();
            button.set_widget_name(&format!("setting-{name}-circle-{i}"));
            button.update_property(&[gtk::accessible::Property::Label(label)]);
            if let Some((first, _, _)) = buttons.first() {
                button.set_group(Some(first));
            }
            let select = select.clone();
            button.connect_clicked(move |_| select(i as u32));
            widget.append(&button);
            buttons.push((button, preview, checked));
        }
        Self { widget, buttons }
    }

    pub fn set_selected(&self, selected: u32) {
        for (i, (button, preview, checked)) in self.buttons.iter().enumerate() {
            let active = i == selected as usize;
            if button.is_active() != active {
                button.set_active(active);
            }
            if checked.replace(active) != active {
                preview.queue_draw();
            }
        }
    }
}

fn draw(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: i32,
    height: i32,
    level: Transparency,
    checked: bool,
) {
    let [w, h] = [f64::from(width), f64::from(height)];
    let size = w.min(h);
    cr.translate((w - size) / 2., (h - size) / 2.);
    cr.scale(size, size);
    cr.arc(0.5, 0.5, 0.5, 0., TAU);
    cr.clip();
    let panel = area.color();
    let dark = 0.2126 * panel.red() + 0.7152 * panel.green() + 0.0722 * panel.blue() < 0.5;
    let tint = if dark { 0.55 } else { 0.8 };
    let alpha = f64::from(level.surface_alpha(dark));
    if level.enabled() {
        let cell = 1. / f64::from(CHECKS);
        let (light, shade) = (0.94, 0.28);
        for row in 0..CHECKS {
            for column in 0..CHECKS {
                let tone = if (row + column) % 2 == 0 { light } else { shade };
                cr.set_source_rgb(tone, tone, tone);
                cr.rectangle(f64::from(column) * cell, f64::from(row) * cell, cell, cell);
                let _ = cr.fill();
            }
        }
    }
    cr.set_source_rgba(tint, tint, tint, alpha);
    let _ = cr.paint();
    if level.enabled() {
        let clear = 1. - alpha;
        let sheen = cairo::RadialGradient::new(0.32, 0.26, 0.02, 0.32, 0.26, 0.5);
        sheen.add_color_stop_rgba(0., 1., 1., 1., 0.25 + 1.2 * clear);
        sheen.add_color_stop_rgba(1., 1., 1., 1., 0.);
        let _ = cr.set_source(&sheen);
        let _ = cr.paint();
    }
    cr.reset_clip();
    cr.arc(0.5, 0.5, 0.5 - 0.5 / size, 0., TAU);
    cr.set_line_width(1. / size);
    cr.set_source_rgba(0.5, 0.5, 0.5, 0.45);
    let _ = cr.stroke();
    if checked {
        cr.move_to(0.3, 0.52);
        cr.line_to(0.44, 0.66);
        cr.line_to(0.71, 0.36);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        let (ink, halo) = if dark { (1., 0.) } else { (0.18, 1.) };
        cr.set_line_width(4. / size);
        cr.set_source_rgba(halo, halo, halo, 0.45);
        let _ = cr.stroke_preserve();
        cr.set_line_width(2. / size);
        cr.set_source_rgb(ink, ink, ink);
        let _ = cr.stroke();
    }
}
