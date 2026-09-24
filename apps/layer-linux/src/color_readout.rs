//! Retained text for live color previews. Glyphs share GTK's atlas instead of
//! uploading a transparent Cairo image each time a color component changes.
use crate::display_color::ViewColor;
use gtk::{gdk, graphene, pango, prelude::*};
use layer_ui::{ColorPanelLayout, ColorReadout, ColorState};
use std::collections::HashMap;

pub(crate) fn layout(widget: &gtk::Widget, font: f64, bold: bool, text: &str) -> pango::Layout {
    let mut description = pango::FontDescription::from_string("Adwaita Sans");
    description.set_absolute_size(font * pango::SCALE as f64);
    if bold {
        description.set_weight(pango::Weight::Bold);
    }
    let layout = widget.create_pango_layout(Some(text));
    layout.set_font_description(Some(&description));
    layout
}
pub(crate) fn advance(layout: &pango::Layout) -> f64 {
    layout.size().0 as f64 / pango::SCALE as f64
}
pub(crate) fn append(
    snapshot: &gtk::Snapshot,
    layout: &pango::Layout,
    position: [f64; 2],
    rotation: f64,
    centered: bool,
    ink: gdk::RGBA,
) {
    snapshot.save();
    snapshot.translate(&graphene::Point::new(
        position[0] as f32,
        position[1] as f32,
    ));
    snapshot.rotate(rotation as f32);
    snapshot.translate(&graphene::Point::new(
        if centered {
            (-advance(layout) * 0.5) as f32
        } else {
            0.
        },
        -(layout.baseline() as f32 / pango::SCALE as f32),
    ));
    snapshot.append_layout(layout, &ink);
    snapshot.restore();
}

pub(crate) fn draw(
    widget: &gtk::Widget,
    snapshot: &gtk::Snapshot,
    state: &ColorState,
    view: ViewColor,
) {
    let half = widget.width() as f64;
    let mut font = (half * 2. * 0.044).clamp(9., 12.);
    let Some(geometry) = ColorPanelLayout::new((half * 2.) as f32) else {
        return;
    };
    let radius = geometry.readout_radius as f64;
    let mut ink = widget.color();
    ink.set_alpha(0.9);
    let label = layout(widget, font, true, state.readout_label());
    let label_width = advance(&label);
    append(snapshot, &label, [2., font + 1.], 0., false, ink);
    let gamut = |space| {
        if matches!(view, ViewColor::Mapped { .. }) {
            state.definition().in_hdr_gamut(space)
        } else {
            state.definition().in_gamut(space)
        }
    };
    if !gamut(view.space()).unwrap() || !gamut(state.rgb_space()).unwrap() {
        append(
            snapshot,
            &layout(widget, font, true, "!"),
            [label_width + 6., font + 1.],
            0.,
            false,
            ink,
        );
    }
    if widget.has_visible_focus() {
        let focus = gtk::gsk::RoundedRect::from_rect(
            graphene::Rect::new(0.25, 0.25, (label_width + 6.5) as f32, (font + 5.5) as f32),
            5.75,
        );
        snapshot.append_border(&focus, &[1.5; 4], &[ink; 4]);
    }
    ink.set_alpha(0.8);
    let rgb = state.readout == ColorReadout::Rgb;
    let texts = state.readout_layout_text();
    let available = radius * std::f64::consts::FRAC_PI_2 - 4.;
    let (glyphs, widths, total, digit_advance) = loop {
        let mut glyphs = HashMap::new();
        for c in ('0'..='9').chain(texts.iter().flat_map(|text| text.chars())) {
            glyphs
                .entry(c)
                .or_insert_with(|| layout(widget, font, false, &c.to_string()));
        }
        let digit_advance = ('0'..='9').map(|c| advance(&glyphs[&c])).fold(0., f64::max);
        let widths = texts.each_ref().map(|text| {
            text.chars()
                .map(|c| {
                    if c.is_ascii_digit() || c == ' ' {
                        digit_advance
                    } else {
                        advance(&glyphs[&c])
                    }
                })
                .sum::<f64>()
                + if rgb { font * 0.8 + 2. } else { 0. }
        });
        let total: f64 = widths.iter().sum();
        if total + 6. <= available || font <= 8. {
            break (glyphs, widths, total, digit_advance);
        }
        font -= 0.25;
    };
    let chip = font * 0.8;
    let gap = ((available - total) * 0.5).clamp(3., radius * 0.24);
    let mut cursor = -(total + gap * 2.) * 0.5;
    for (index, text) in texts.iter().enumerate() {
        let width = widths[index];
        let mid = (-135_f64).to_radians() + (cursor + width * 0.5) / radius;
        cursor += width + gap;
        let mut along = -width * 0.5;
        if rgb {
            let angle = mid + (along + chip * 0.5) / radius;
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                (half + radius * angle.cos()) as f32,
                (half + radius * angle.sin()) as f32,
            ));
            snapshot.rotate((angle.to_degrees() + 90.) as f32);
            let bounds = graphene::Rect::new(
                (-chip * 0.5) as f32,
                (-font * 0.76) as f32,
                chip as f32,
                chip as f32,
            );
            snapshot.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(bounds, 2.));
            let [r, g, b] = [[0.93, 0.31, 0.36], [0.25, 0.73, 0.43], [0.29, 0.56, 0.98]][index];
            snapshot.append_color(&gdk::RGBA::new(r, g, b, 1.), &bounds);
            snapshot.pop();
            snapshot.restore();
            along += chip + 2.;
        }
        for c in text.chars() {
            let glyph = &glyphs[&c];
            let advance = if c.is_ascii_digit() || c == ' ' {
                digit_advance
            } else {
                advance(glyph)
            };
            let angle = mid + (along + advance * 0.5) / radius;
            append(
                snapshot,
                glyph,
                [half + radius * angle.cos(), half + radius * angle.sin()],
                angle.to_degrees() + 90.,
                true,
                ink,
            );
            along += advance;
        }
    }
}
