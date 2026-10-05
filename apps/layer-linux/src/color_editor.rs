use crate::display_color::{ColorPatch, ViewColor};
use crate::tool_panels::ColorWheel;
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gdk, glib};
use layer_core::color::RgbColor;
use layer_ui::{
    ColorAction, ColorEditor, ColorEditorAction, ColorEditorError, ColorEditorTarget, ColorEditorView, ColorPickerAction,
    ColorScrubSpeed, ColorShape, ColorSlot, ColorStripCorner, ColorStripPlacement, ColorStripView, SwatchSheetView, UiAction,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};

const SHAPES: [(ColorShape, &str, &str, &str); 3] = [
    (ColorShape::Circle, "circle", "layer-color-circle-symbolic", "OKLCH"),
    (ColorShape::Square, "square", "layer-color-square-symbolic", "HSB"),
    (ColorShape::Triangle, "triangle", "layer-color-triangle-symbolic", "HLS"),
];

const RECENT_TILE: i32 = 28;
const SHEET_TILE: i32 = 34;
const TILE_GAP: i32 = 4;

type Accepted = Box<dyn FnOnce(&Rc<Workspace>, RgbColor, Option<f32>)>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target {
    Dialog,
    Hex,
    Number(ColorEditorTarget),
}

pub(crate) struct Field {
    target: Target,
    pub(crate) stack: gtk::Stack,
    pub(crate) show: gtk::Button,
    pub(crate) label: gtk::Label,
    pub(crate) entry: gtk::Entry,
    edit: RefCell<String>,
    composition: Rc<crate::input::EntryComposition>,
}
impl Field {
    fn new(target: Target, name: &str, chars: i32) -> Rc<Self> {
        let label = gtk::Label::builder().xalign(if target == Target::Hex { 0. } else { 1. }).width_chars(chars).build();
        label.add_css_class("edit-color-number");
        let show = gtk::Button::builder().child(&label).build();
        show.add_css_class("flat");
        show.add_css_class("number-value");
        show.set_widget_name(name);
        let entry = gtk::Entry::builder().has_frame(false).xalign(label.xalign()).width_chars(chars).max_width_chars(chars).build();
        entry.add_css_class("number-entry");
        entry.set_widget_name(&format!("{name}-entry"));
        let composition = crate::input::guard_entry_activation(&entry);
        let stack = gtk::Stack::builder().hhomogeneous(true).vhomogeneous(true).build();
        stack.add_named(&show, Some("show"));
        stack.add_named(&entry, Some("edit"));
        if target == Target::Hex {
            for widget in [show.upcast_ref::<gtk::Widget>(), entry.upcast_ref()] { widget.add_css_class("edit-color-hex"); }
        } else {
            show.set_cursor_from_name(Some("ns-resize"));
        }
        Rc::new(Self { target, stack, show, label, entry, edit: RefCell::default(), composition })
    }
    pub(crate) fn editing(&self) -> bool {
        self.stack.visible_child_name().as_deref() == Some("edit")
    }
    fn set(&self, text: &str, edit: &str, name: &str) {
        self.label.set_text(text);
        *self.edit.borrow_mut() = edit.into();
        self.show.update_property(&[gtk::accessible::Property::Label(&format!("{name} {text}"))]);
        self.entry.update_property(&[gtk::accessible::Property::Label(name)]);
    }
}

pub(crate) struct Row {
    pub(crate) menu: gtk::MenuButton,
    names: gtk::Stack,
    pub(crate) choices: Vec<gtk::CheckButton>,
    space: gtk::Label,
    pub(crate) fields: [Rc<Field>; 3],
    pub(crate) copy: gtk::Button,
    text: RefCell<String>,
}

pub(crate) struct Editor {
    this: Weak<Editor>,
    workspace: Weak<Workspace>,
    localization: RefCell<Arc<layer_ui::Localizer>>,
    pub(crate) draft: RefCell<ColorEditor>,
    view: Cell<ViewColor>,
    headroom: Cell<f32>,
    epoch: u64,
    accepted: RefCell<Option<Accepted>>,
    pub(crate) error: RefCell<Option<(Target, ColorEditorError)>>,
    pub(crate) picking: Cell<bool>,
    finished: Cell<bool>,
    updating: Cell<bool>,
    pub(crate) dialog: adw::Dialog,
    pub(crate) title: gtk::Label,
    pub(crate) wheel: ColorWheel,
    pub(crate) shape_toggles: Vec<gtk::ToggleButton>,
    pub(crate) current: gtk::Button,
    pub(crate) current_patch: ColorPatch,
    pub(crate) new_patch: ColorPatch,
    pub(crate) current_caption: gtk::Label,
    new_caption: gtk::Label,
    pub(crate) pick: gtk::Button,
    pub(crate) hex_note: gtk::Label,
    pub(crate) hex: Rc<Field>,
    pub(crate) hex_copy: gtk::Button,
    pub(crate) rows: [Row; 3],
    intensity_label: gtk::Label,
    pub(crate) intensity: Rc<Field>,
    pub(crate) validation: gtk::Label,
    pub(crate) body: gtk::Box,
    right: gtk::Box,
    pub(crate) left: gtk::Box,
    pub(crate) header: gtk::Grid,
    values: gtk::Box,
    recent: gtk::Box,
    bottom_stack: gtk::Stack,
    mini_current: ColorPatch,
    mini_new: ColorPatch,
    mini_hex: gtk::Label,
    pub(crate) swatches: gtk::Button,
    pub(crate) cancel: gtk::Button,
    pub(crate) apply_button: gtk::Button,
    pub(crate) sheet: gtk::Box,
    pub(crate) sheet_scroll: gtk::ScrolledWindow,
    pub(crate) sheet_open: Cell<bool>,
    sheet_progress: Rc<Cell<f64>>,
    sheet_slide: adw::TimedAnimation,
    pub(crate) search: gtk::SearchEntry,
    pub(crate) sheet_body: gtk::Box,
    pub(crate) sheet_close: gtk::Button,
}

fn copy_button(name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    crate::icons::set_button(&button, "layer-copy-symbolic");
    button.add_css_class("flat");
    button.add_css_class("edit-color-copy");
    button.set_valign(gtk::Align::Center);
    button.set_widget_name(name);
    button
}

fn pair(width: i32, height: i32) -> (gtk::Box, ColorPatch, ColorPatch) {
    let current = ColorPatch::device_aligned(false);
    let new = ColorPatch::device_aligned(false);
    let pair = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pair.set_homogeneous(true);
    pair.add_css_class("edit-color-pair");
    pair.set_overflow(gtk::Overflow::Hidden);
    for patch in [&current, &new] {
        patch.set_size_request(width, height);
        pair.append(patch);
    }
    (pair, current, new)
}

impl Editor {
    fn build(workspace: &Rc<Workspace>, draft: ColorEditor, epoch: u64, accepted: Accepted) -> Rc<Self> {
        let localization = workspace.localization();
        let dialog = adw::Dialog::builder().content_width(720).width_request(320).height_request(300).build();
        dialog.set_widget_name("edit-color-dialog");
        dialog.add_css_class("edit-color-dialog");
        let title = gtk::Label::new(None);
        title.add_css_class("title-4");
        title.set_halign(gtk::Align::Center);
        let wheel = ColorWheel::bare();
        wheel.set_widget_name("edit-color-wheel");
        wheel.set_halign(gtk::Align::Center);
        let shapes = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        shapes.set_halign(gtk::Align::Center);
        shapes.add_css_class("choice-well");
        shapes.set_widget_name("edit-color-shapes");
        let shape_toggles: Vec<gtk::ToggleButton> = SHAPES.iter().map(|(_, name, icon, label)| {
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            content.set_halign(gtk::Align::Center);
            content.append(&crate::icons::image(icon));
            content.append(&gtk::Label::new(Some(label)));
            let toggle = gtk::ToggleButton::builder().child(&content).build();
            toggle.set_widget_name(&format!("edit-color-shape-{name}"));
            shapes.append(&toggle);
            toggle
        }).collect();
        for toggle in &shape_toggles[1..] { toggle.set_group(Some(&shape_toggles[0])); }
        let left = gtk::Box::new(gtk::Orientation::Vertical, 6);
        left.set_valign(gtk::Align::Start);
        left.append(&wheel);
        left.append(&shapes);
        let current_patch = ColorPatch::device_aligned(false);
        let new_patch = ColorPatch::device_aligned(false);
        for patch in [&current_patch, &new_patch] { patch.set_size_request(54, 46); }
        let current = gtk::Button::builder().child(&current_patch).build();
        current.add_css_class("flat");
        current.add_css_class("edit-color-revert");
        current.set_widget_name("edit-color-current");
        new_patch.set_widget_name("edit-color-new");
        let pair_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        pair_box.set_homogeneous(true);
        pair_box.add_css_class("edit-color-pair");
        pair_box.set_overflow(gtk::Overflow::Hidden);
        pair_box.set_valign(gtk::Align::Center);
        pair_box.append(&current);
        pair_box.append(&new_patch);
        let current_caption = gtk::Label::new(None);
        let new_caption = gtk::Label::new(None);
        let captions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        captions.set_homogeneous(true);
        for caption in [&current_caption, &new_caption] {
            caption.add_css_class("caption");
            caption.add_css_class("dim-label");
            captions.append(caption);
        }
        let pick_icon = crate::icons::image("layer-eyedropper-symbolic");
        pick_icon.set_pixel_size(24);
        let pick = gtk::Button::builder().child(&pick_icon).build();
        pick.add_css_class("edit-color-pick");
        pick.set_valign(gtk::Align::Center);
        pick.set_widget_name("edit-color-pick");
        let hex_note = gtk::Label::builder().valign(gtk::Align::Center).build();
        hex_note.add_css_class("caption");
        hex_note.add_css_class("edit-color-badge");
        hex_note.set_widget_name("edit-color-hex-note");
        let hex = Field::new(Target::Hex, "edit-color-hex", 7);
        let hex_copy = copy_button("edit-color-hex-copy");
        let hex_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        hex_row.set_halign(gtk::Align::End);
        hex_row.set_valign(gtk::Align::Center);
        hex_row.set_hexpand(true);
        hex_row.append(&hex_note);
        hex_row.append(&hex.stack);
        hex_row.append(&hex_copy);
        let header = gtk::Grid::builder().column_spacing(12).row_spacing(2).build();
        header.attach(&pair_box, 0, 0, 1, 1);
        header.attach(&pick, 1, 0, 1, 1);
        header.attach(&hex_row, 2, 0, 1, 1);
        header.attach(&captions, 0, 1, 1, 1);
        let grid = gtk::Grid::builder().column_spacing(4).row_spacing(2).build();
        grid.set_widget_name("edit-color-rows");
        grid.add_css_class("edit-color-rows");
        let rows = std::array::from_fn(|row| {
            let menu = gtk::MenuButton::new();
            menu.add_css_class("flat");
            menu.add_css_class("edit-color-format");
            menu.set_widget_name(&format!("edit-color-form-{row}"));
            let choices_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let names = gtk::Stack::builder().hhomogeneous(true).build();
            let choices: Vec<gtk::CheckButton> = layer_ui::COLOR_FORM_FAMILIES[row].iter().enumerate().map(|(index, _)| {
                let choice = gtk::CheckButton::new();
                choice.set_widget_name(&format!("edit-color-form-{row}-{index}"));
                choices_box.append(&choice);
                let name = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                name.append(&gtk::Label::builder().hexpand(true).xalign(0.).build());
                name.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
                names.add_named(&name, Some(&index.to_string()));
                choice
            }).collect();
            menu.set_child(Some(&names));
            for choice in &choices[1..] { choice.set_group(Some(&choices[0])); }
            let popover = gtk::Popover::builder().child(&choices_box).build();
            menu.set_popover(Some(&popover));
            let space = gtk::Label::new(None);
            space.add_css_class("caption");
            space.add_css_class("edit-color-badge");
            space.set_valign(gtk::Align::Center);
            space.set_halign(gtk::Align::Start);
            space.set_hexpand(true);
            grid.attach(&menu, 0, row as i32, 1, 1);
            grid.attach(&space, 1, row as i32, 1, 1);
            let fields = std::array::from_fn(|index| {
                let field = Field::new(Target::Number(ColorEditorTarget::Value { row, index }), &format!("edit-color-value-{row}-{index}"), 6);
                grid.attach(&field.stack, 2 + index as i32, row as i32, 1, 1);
                field
            });
            let copy = copy_button(&format!("edit-color-copy-{row}"));
            grid.attach(&copy, 5, row as i32, 1, 1);
            Row { menu, names, choices, space, fields, copy, text: RefCell::default() }
        });
        let intensity_label = gtk::Label::builder().xalign(0.).build();
        intensity_label.add_css_class("edit-color-intensity-label");
        let intensity = Field::new(Target::Number(ColorEditorTarget::Intensity), "edit-color-ev", 9);
        grid.attach(&intensity_label, 0, 3, 2, 1);
        grid.attach(&intensity.stack, 2, 3, 3, 1);
        intensity.stack.set_halign(gtk::Align::End);
        let validation = gtk::Label::builder().wrap(true).xalign(0.).visible(false).build();
        validation.add_css_class("error");
        validation.set_widget_name("edit-color-validation");
        let values = gtk::Box::new(gtk::Orientation::Vertical, 6);
        values.append(&grid);
        values.append(&validation);
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        let page = gtk::Box::new(gtk::Orientation::Vertical, 16);
        page.add_css_class("edit-color-body");
        page.append(&title);
        page.append(&body);
        let right = gtk::Box::new(gtk::Orientation::Vertical, 14);
        right.set_hexpand(true);
        let recent = gtk::Box::new(gtk::Orientation::Horizontal, TILE_GAP);
        recent.set_widget_name("edit-color-recent");
        let recent_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .child(&recent)
            .build();
        let (mini_pair, mini_current, mini_new) = pair(18, 22);
        let mini_hex = gtk::Label::new(None);
        mini_hex.add_css_class("edit-color-number");
        let mini = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        mini.append(&mini_pair);
        mini.append(&mini_hex);
        let bottom_stack = gtk::Stack::builder().hexpand(true).transition_type(gtk::StackTransitionType::Crossfade).build();
        bottom_stack.add_named(&recent_scroll, Some("recent"));
        bottom_stack.add_named(&mini, Some("mini"));
        let swatches = gtk::Button::new();
        crate::icons::set_button(&swatches, "layer-chevron-down-symbolic");
        swatches.add_css_class("edit-color-more");
        swatches.set_valign(gtk::Align::Center);
        swatches.set_widget_name("edit-color-swatches");
        let cancel = gtk::Button::new();
        cancel.set_widget_name("edit-color-cancel");
        let apply_button = gtk::Button::new();
        apply_button.add_css_class("suggested-action");
        apply_button.set_widget_name("edit-color-apply");
        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bottom.add_css_class("edit-color-bottom");
        bottom.append(&bottom_stack);
        bottom.append(&swatches);
        bottom.append(&cancel);
        bottom.append(&apply_button);
        let search = gtk::SearchEntry::builder().hexpand(true).build();
        search.set_widget_name("edit-color-search");
        let sheet_close = gtk::Button::new();
        crate::icons::set_button(&sheet_close, "layer-chevron-down-symbolic");
        sheet_close.add_css_class("flat");
        sheet_close.set_widget_name("edit-color-sheet-close");
        let sheet_head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        sheet_head.append(&search);
        sheet_head.append(&sheet_close);
        let sheet_body = gtk::Box::new(gtk::Orientation::Vertical, 14);
        sheet_body.set_widget_name("edit-color-sheet-body");
        sheet_body.add_css_class("edit-color-sheet-body");
        let sheet_scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&crate::squircle::Squircles::new(&sheet_body))
            .build());
        let sheet = gtk::Box::new(gtk::Orientation::Vertical, 10);
        sheet.add_css_class("edit-color-sheet");
        sheet.set_widget_name("edit-color-sheet");
        sheet.set_visible(false);
        sheet.append(&crate::squircle::Squircles::new(&sheet_head));
        sheet.append(&sheet_scroll);
        let body_view = gtk::Viewport::builder().vscroll_policy(gtk::ScrollablePolicy::Natural).child(&crate::squircle::Squircles::new(&page)).build();
        let body_scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&body_view)
            .build());
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&body_scroll));
        overlay.add_overlay(&sheet);
        overlay.set_clip_overlay(&sheet, true);
        let sheet_progress = Rc::new(Cell::new(0.));
        overlay.connect_get_child_position(glib::clone!(#[strong] sheet_progress, move |overlay, _| {
            let offset = ((1. - sheet_progress.get()) * f64::from(overlay.height())).round() as i32;
            Some(gdk::Rectangle::new(0, offset, overlay.width(), overlay.height()))
        }));
        let sheet_slide = adw::TimedAnimation::new(&overlay, 0., 1., 250, adw::CallbackAnimationTarget::new(glib::clone!(#[strong] sheet_progress, #[weak] overlay, move |value| {
            sheet_progress.set(value);
            overlay.queue_allocate();
        })));
        sheet_slide.set_easing(adw::Easing::EaseOutCubic);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("edit-color-content");
        content.append(&overlay);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&crate::squircle::Squircles::new(&bottom));
        dialog.set_child(Some(&content));
        dialog.set_default_widget(Some(&apply_button));
        let editor = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            workspace: Rc::downgrade(workspace),
            localization: RefCell::new(localization),
            draft: RefCell::new(draft),
            view: Cell::new(workspace.paint_view_color()),
            headroom: Cell::new(workspace.picker_headroom()),
            epoch,
            accepted: RefCell::new(Some(accepted)),
            error: RefCell::new(None),
            picking: Cell::new(false),
            finished: Cell::new(false),
            updating: Cell::new(false),
            dialog,
            title,
            wheel,
            shape_toggles,
            current,
            current_patch,
            new_patch,
            current_caption,
            new_caption,
            pick,
            hex_note,
            hex,
            hex_copy,
            rows,
            intensity_label,
            intensity,
            validation,
            body,
            right,
            left,
            header,
            values,
            recent,
            bottom_stack,
            mini_current,
            mini_new,
            mini_hex,
            swatches,
            cancel,
            apply_button,
            sheet,
            sheet_scroll,
            sheet_open: Cell::new(false),
            sheet_progress,
            sheet_slide,
            search,
            sheet_body,
            sheet_close,
        });
        editor.arrange(false);
        editor.relabel();
        editor.fill_recent();
        editor.refresh();
        editor
    }

    fn arrange(&self, narrow: bool) {
        for widget in [self.left.upcast_ref::<gtk::Widget>(), self.header.upcast_ref(), self.values.upcast_ref(), self.right.upcast_ref()] {
            if let Some(parent) = widget.parent().and_downcast::<gtk::Box>() { parent.remove(widget); }
        }
        self.left.set_halign(if narrow { gtk::Align::Center } else { gtk::Align::Fill });
        if narrow {
            self.body.set_orientation(gtk::Orientation::Vertical);
            self.body.set_spacing(14);
            for widget in [self.header.upcast_ref::<gtk::Widget>(), self.left.upcast_ref(), self.values.upcast_ref()] { self.body.append(widget); }
        } else {
            self.body.set_orientation(gtk::Orientation::Horizontal);
            self.body.set_spacing(24);
            self.right.append(&self.header);
            self.right.append(&self.values);
            self.body.append(&self.left);
            self.body.append(&self.right);
        }
    }

    fn relabel(&self) {
        let localization = self.localization.borrow().clone();
        let copy = layer_ui::NativeCopy::new(&localization);
        let common = layer_ui::CommonCopy::new(&localization);
        let color = &copy.color;
        self.dialog.set_title(&color.edit);
        self.title.set_text(&color.edit);
        self.current_caption.set_text(&color.current);
        self.new_caption.set_text(&color.new);
        self.current.set_tooltip_text(Some(&color.current));
        self.current.update_property(&[gtk::accessible::Property::Label(&color.current)]);
        self.new_patch.update_property(&[gtk::accessible::Property::Label(&color.new)]);
        self.pick.set_tooltip_text(Some(&color.pick_canvas));
        self.pick.update_property(&[gtk::accessible::Property::Label(&color.pick_canvas)]);
        for button in std::iter::once(&self.hex_copy).chain(self.rows.iter().map(|row| &row.copy)) {
            button.set_tooltip_text(Some(&color.copy));
            button.update_property(&[gtk::accessible::Property::Label(&color.copy)]);
        }
        for row in &self.rows {
            row.menu.set_tooltip_text(Some(&color.format));
        }
        self.intensity_label.set_text(&color.intensity_ev);
        self.cancel.set_label(&common.cancel);
        self.apply_button.set_label(&color.use_color);
        self.search.set_placeholder_text(Some(&color.swatch_search));
        self.search.update_property(&[gtk::accessible::Property::Label(&color.swatch_search)]);
        self.sheet_close.set_tooltip_text(Some(&color.close_swatches));
        self.sheet_close.update_property(&[gtk::accessible::Property::Label(&color.close_swatches)]);
        let swatches = if self.sheet_open.get() { &color.close_swatches } else { &color.all_swatches };
        self.swatches.set_tooltip_text(Some(swatches));
        self.swatches.update_property(&[gtk::accessible::Property::Label(swatches)]);
        for (toggle, name) in self.shape_toggles.iter().zip([&color.circle, &color.square, &color.triangle]) {
            toggle.set_tooltip_text(Some(name));
            toggle.update_property(&[gtk::accessible::Property::Description(name)]);
        }
    }

    fn view(&self) -> Option<ColorEditorView> {
        let localization = self.localization.borrow().clone();
        self.draft.borrow().view(self.view.get().space(), None, &localization).map_err(|error| eprintln!("{error}")).ok()
    }

    pub(crate) fn refresh(&self) {
        let Some(view) = self.view() else { return; };
        self.updating.set(true);
        let (display, headroom) = (self.view.get(), self.headroom.get());
        self.wheel.set_state(self.draft.borrow().picker(), display, headroom, false);
        if let Some(choice) = view.shapes.iter().find(|choice| choice.selected) {
            for (toggle, (shape, ..)) in self.shape_toggles.iter().zip(SHAPES) { toggle.set_active(shape == choice.shape); }
        }
        for (current, new) in [(&self.current_patch, &self.new_patch), (&self.mini_current, &self.mini_new)] {
            current.set_display_color(view.current_value, display, headroom);
            new.set_display_color(view.value, display, headroom);
        }
        let localization = self.localization.borrow().clone();
        let copy = layer_ui::NativeCopy::new(&localization).color;
        self.hex.set(&view.hex, &view.hex, &copy.hex);
        self.mini_hex.set_text(&view.hex);
        self.hex_note.set_visible(view.hex_note.is_some());
        if let Some(note) = &view.hex_note {
            self.hex_note.set_text(&note.text);
            self.hex_note.set_tooltip_text(note.tip.as_deref());
        }
        for (row, shown) in self.rows.iter().zip(&view.rows) {
            row.menu.update_property(&[gtk::accessible::Property::Label(&shown.label)]);
            for (index, (choice, form)) in row.choices.iter().zip(&shown.forms).enumerate() {
                choice.set_label(Some(&form.label));
                choice.set_active(form.form == shown.form);
                let name = index.to_string();
                if let Some(label) = row.names.child_by_name(&name).and_then(|name| name.first_child()).and_downcast::<gtk::Label>() { label.set_text(&form.label); }
                if form.form == shown.form { row.names.set_visible_child_name(&name); }
            }
            row.space.set_visible(shown.space.is_some());
            row.space.set_text(shown.space.unwrap_or_default());
            for (field, value) in row.fields.iter().zip(&shown.values) {
                field.set(&value.text, &value.edit, &value.name);
            }
            *row.text.borrow_mut() = shown.copy.clone();
        }
        self.intensity_label.set_visible(view.intensity.is_some());
        self.intensity.stack.set_visible(view.intensity.is_some());
        if let Some(value) = &view.intensity {
            self.intensity.set(&value.text, &value.edit, &value.name);
        }
        self.updating.set(false);
        self.refresh_error();
    }

    fn fields(&self) -> impl Iterator<Item = &Rc<Field>> {
        std::iter::once(&self.hex).chain(self.rows.iter().flat_map(|row| row.fields.iter())).chain(std::iter::once(&self.intensity))
    }

    fn refresh_error(&self) {
        let error = self.error.borrow();
        let message = error.as_ref().map(|(_, error)| error.message(&self.localization.borrow()));
        self.validation.set_visible(message.is_some());
        self.validation.set_text(message.as_deref().unwrap_or_default());
        let mut blocked = false;
        for field in self.fields() {
            if error.as_ref().is_some_and(|(target, _)| *target == field.target) {
                field.entry.add_css_class("error");
                blocked |= field.editing();
            } else {
                field.entry.remove_css_class("error");
            }
            blocked |= field.composition.active();
        }
        self.apply_button.set_sensitive(!blocked);
    }

    fn apply(&self, action: ColorEditorAction) -> Result<(), ColorEditorError> {
        let result = self.draft.borrow_mut().apply(action);
        self.refresh();
        result
    }

    fn report(&self, target: Target, result: Result<(), ColorEditorError>) {
        let changed = match result {
            Ok(()) => self.error.borrow_mut().take_if(|(failed, _)| *failed == target || *failed == Target::Dialog).is_some(),
            Err(error) => { *self.error.borrow_mut() = Some((target, error)); true }
        };
        if changed { self.refresh_error(); }
    }

    fn begin_edit(&self, field: &Field) {
        field.entry.set_text(&field.edit.borrow());
        field.stack.set_visible_child_name("edit");
        field.entry.grab_focus();
        field.entry.select_region(0, -1);
    }

    fn end_edit(&self, field: &Field) {
        field.stack.set_visible_child_name("show");
        field.show.grab_focus();
    }

    pub(crate) fn commit(&self, field: &Field) {
        if !field.editing() || field.composition.active() { return; }
        let text = field.entry.text().to_string();
        if text == *field.edit.borrow() {
            self.cancel_edit(field);
            return;
        }
        let action = match field.target {
            Target::Dialog => return,
            Target::Hex => ColorEditorAction::Text { text, row: None },
            Target::Number(ColorEditorTarget::Value { row, index }) => ColorEditorAction::Value { row, index, text },
            Target::Number(ColorEditorTarget::Intensity) => ColorEditorAction::Intensity { text },
        };
        let result = self.apply(action);
        let accepted = result.is_ok();
        self.report(field.target, result);
        if accepted { self.end_edit(field); }
    }

    pub(crate) fn cancel_edit(&self, field: &Field) {
        self.report(field.target, Ok(()));
        self.end_edit(field);
    }

    pub(crate) fn step(&self, target: ColorEditorTarget, steps: f32, speed: ColorScrubSpeed) {
        let result = self.apply(ColorEditorAction::Scrub { target, pixels: steps * 2., speed })
            .and_then(|()| self.apply(ColorEditorAction::EndScrub { cancel: false }));
        self.report(Target::Number(target), result);
    }

    pub(crate) fn paste(&self, text: &str) {
        let result = self.apply(ColorEditorAction::Text { text: text.into(), row: None });
        self.report(Target::Dialog, result);
    }

    fn copy(&self, button: &gtk::Button, text: &str) {
        button.clipboard().set_text(text);
        crate::icons::set_button(button, "layer-check-symbolic");
        button.set_tooltip_text(Some(&layer_ui::NativeCopy::new(&self.localization.borrow()).color.copied));
        let (button, localization) = (button.downgrade(), self.localization.borrow().clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
            if let Some(button) = button.upgrade() {
                crate::icons::set_button(&button, "layer-copy-symbolic");
                button.set_tooltip_text(Some(&layer_ui::NativeCopy::new(&localization).color.copy));
            }
        });
    }

    fn fill_recent(&self) {
        while let Some(child) = self.recent.first_child() { self.recent.remove(&child); }
        let Some(workspace) = self.workspace.upgrade() else { return; };
        let history = workspace.gpu.borrow().as_ref().map(|g| g.session.state().color_library.history.clone()).unwrap_or_default();
        for color in history {
            self.recent.append(&self.tile(color, &layer_ui::ColorLibrary::tile_detail("", color), "edit-color-recent-tile", RECENT_TILE));
        }
    }

    fn tile(&self, color: RgbColor, detail: &str, name: &str, size: i32) -> gtk::Button {
        let patch = ColorPatch::new(false);
        patch.set_size_request(size, size);
        patch.set_overflow(gtk::Overflow::Hidden);
        patch.set_display_color(color, self.view.get(), self.headroom.get());
        let tile = gtk::Button::builder().child(&patch).tooltip_text(detail).build();
        tile.add_css_class("palette-tile");
        tile.set_widget_name(name);
        tile.update_property(&[gtk::accessible::Property::Label(detail)]);
        let editor = self.this.clone();
        tile.connect_clicked(move |_| {
            if let Some(editor) = editor.upgrade() {
                let result = editor.apply(ColorEditorAction::Color { color });
                editor.report(Target::Dialog, result);
            }
        });
        tile
    }

    pub(crate) fn fill_sheet(&self) {
        while let Some(child) = self.sheet_body.first_child() { self.sheet_body.remove(&child); }
        let Some(workspace) = self.workspace.upgrade() else { return; };
        let localization = self.localization.borrow().clone();
        let query = self.draft.borrow().memory().search.clone();
        let current = self.draft.borrow().value();
        let Some(sheet) = workspace.gpu.borrow().as_ref().map(|g| {
            SwatchSheetView::new(&g.session.state().color_library, &query, current, |color| color.rgba, &localization)
        }) else { return; };
        if let Some(empty) = sheet.empty {
            let label = gtk::Label::builder().label(&empty).wrap(true).xalign(0.).build();
            label.add_css_class("dim-label");
            label.set_widget_name("edit-color-sheet-empty");
            self.sheet_body.append(&label);
        }
        let add = layer_ui::NativeCopy::new(&localization).palettes.add_current;
        for section in sheet.sections {
            let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let title = gtk::Label::builder().label(&section.title).xalign(0.).ellipsize(gtk::pango::EllipsizeMode::End).build();
            let count = gtk::Label::new(Some(&section.count));
            count.add_css_class("dim-label");
            head.append(&title);
            head.append(&count);
            let tiles = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .homogeneous(true)
                .min_children_per_line(4)
                .max_children_per_line(64)
                .column_spacing(TILE_GAP as u32)
                .row_spacing(TILE_GAP as u32)
                .build();
            for tile in &section.tiles {
                let button = self.tile(tile.color, &tile.detail, "edit-color-sheet-tile", SHEET_TILE);
                if tile.current { button.add_css_class("selected"); }
                tiles.append(&button);
            }
            if let Some(palette) = section.palette.filter(|_| section.can_add) {
                let button = gtk::Button::new();
                crate::icons::set_button(&button, "layer-plus-symbolic");
                button.add_css_class("palette-add");
                button.set_size_request(SHEET_TILE, SHEET_TILE);
                button.set_halign(gtk::Align::Center);
                button.set_valign(gtk::Align::Center);
                button.set_widget_name(&format!("edit-color-add-{palette}"));
                button.set_tooltip_text(Some(&add));
                button.update_property(&[gtk::accessible::Property::Label(&add)]);
                let editor = self.this.clone();
                button.connect_clicked(move |_| {
                    let Some(editor) = editor.upgrade() else { return; };
                    let Some(workspace) = editor.workspace.upgrade() else { return; };
                    let color = editor.draft.borrow().value();
                    workspace.dispatch(UiAction::Color { action: ColorAction::Library {
                        action: layer_ui::ColorLibraryAction::Store { palette, name: String::new(), color },
                    } });
                    editor.fill_sheet();
                });
                tiles.append(&button);
            }
            let block = gtk::Box::new(gtk::Orientation::Vertical, 6);
            block.append(&head);
            block.append(&tiles);
            self.sheet_body.append(&block);
        }
    }

    pub(crate) fn show_sheet(&self, open: bool) {
        if self.sheet_open.replace(open) == open { return; }
        if open {
            self.updating.set(true);
            self.search.set_text(&self.draft.borrow().memory().search);
            self.updating.set(false);
            self.fill_sheet();
            self.sheet_scroll.vadjustment().set_value(0.);
            self.sheet.set_visible(true);
        }
        self.sheet_slide.set_value_from(self.sheet_progress.get());
        self.sheet_slide.set_value_to(if open { 1. } else { 0. });
        self.sheet_slide.play();
        self.body.set_sensitive(!open);
        self.bottom_stack.set_visible_child_name(if open { "mini" } else { "recent" });
        if open { self.swatches.add_css_class("open"); } else { self.swatches.remove_css_class("open"); }
        self.relabel();
        if open { self.search.grab_focus(); } else { self.swatches.grab_focus(); }
    }

    fn start_pick(&self) {
        let Some(workspace) = self.workspace.upgrade() else { return; };
        let original = self.draft.borrow().value();
        self.picking.set(true);
        self.dialog.close();
        let touch_offset = crate::color_picker::finger_offset(&workspace);
        workspace.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Editor { original, touch_offset } });
        refresh_display(&workspace);
    }

    fn refresh_picking(&self, workspace: &Workspace) {
        if !self.picking.get() { return; }
        let Some((active, preview, picked)) = workspace.gpu.borrow().as_ref().map(|g| {
            let picker = &g.session.state().color_picker;
            (picker.editor, picker.preview, picker.picked)
        }) else { return; };
        if active {
            let original = self.draft.borrow().value();
            let sample = preview.unwrap_or(original);
            if let Ok(view) = self.draft.borrow().strip(sample) {
                let tip = layer_ui::NativeCopy::new(&self.localization.borrow()).color.picking_strip;
                workspace.color_strip.show(workspace, &view, &tip, [original, sample], self.view.get(), self.headroom.get());
            }
            return;
        }
        self.picking.set(false);
        workspace.color_strip.hide();
        if let Some(color) = picked {
            let result = self.apply(ColorEditorAction::Color { color });
            self.report(Target::Dialog, result);
        }
        self.dialog.present(Some(&workspace.window));
        self.pick.grab_focus();
    }

    fn finish(&self, accept: bool) {
        if self.finished.replace(true) { return; }
        let Some(workspace) = self.workspace.upgrade() else { return; };
        let draft = self.draft.borrow().clone();
        if draft.memory_changed() {
            workspace.dispatch(UiAction::Color { action: ColorAction::EditorMemory { memory: draft.memory().clone() } });
        }
        if !accept { return; }
        let current = workspace.gpu.borrow().as_ref().map(|g| g.session.state().document_file.epoch);
        if current != Some(self.epoch) {
            if let Some(gpu) = workspace.gpu.borrow_mut().as_mut() {
                gpu.session.raise_message_notice(layer_ui::MessageId::COMMON_ACTION_FAILED);
            }
            workspace.changed(Ok(layer_ui::UiChange { regions: layer_ui::regions::HOST, ..Default::default() }));
            return;
        }
        if let Some(accepted) = self.accepted.take() { accepted(&workspace, draft.value(), draft.intensity()); }
    }

    fn bind(&self, workspace: &Rc<Workspace>, can_pick: bool) {
        let weak = self.this.clone();
        let narrow = adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 700sp").unwrap());
        narrow.connect_apply(glib::clone!(#[strong] weak, move |_| if let Some(editor) = weak.upgrade() { editor.arrange(true); }));
        narrow.connect_unapply(glib::clone!(#[strong] weak, move |_| if let Some(editor) = weak.upgrade() { editor.arrange(false); }));
        self.dialog.add_breakpoint(narrow);
        self.wheel.connect_pick(glib::clone!(#[strong] weak, move |action| {
            if let Some(editor) = weak.upgrade() {
                let result = editor.apply(ColorEditorAction::Wheel { action });
                editor.report(Target::Dialog, result);
            }
        }));
        if let Some(intensity) = self.wheel.intensity() {
            intensity.connect_value_changed(glib::clone!(#[strong] weak, move |scale| {
                let Some(editor) = weak.upgrade() else { return; };
                if scale.updating() || editor.updating.get() { return; }
                let result = editor.apply(ColorEditorAction::Wheel { action: ColorAction::HdrIntensity { stops: scale.value() as f32 } });
                editor.report(Target::Number(ColorEditorTarget::Intensity), result);
            }));
        }
        for (toggle, (shape, ..)) in self.shape_toggles.iter().zip(SHAPES) {
            toggle.connect_toggled(glib::clone!(#[strong] weak, move |toggle| {
                let Some(editor) = weak.upgrade() else { return; };
                if editor.updating.get() || !toggle.is_active() { return; }
                let result = editor.apply(ColorEditorAction::Wheel { action: ColorAction::Shape { shape } });
                editor.report(Target::Dialog, result);
            }));
        }
        self.current.connect_clicked(glib::clone!(#[strong] weak, move |_| {
            if let Some(editor) = weak.upgrade() {
                let result = editor.apply(ColorEditorAction::Revert);
                editor.report(Target::Dialog, result);
            }
        }));
        self.pick.set_visible(can_pick);
        self.pick.connect_clicked(glib::clone!(#[strong] weak, move |_| if let Some(editor) = weak.upgrade() { editor.start_pick(); }));
        self.hex_copy.connect_clicked(glib::clone!(#[strong] weak, move |button| {
            if let Some(editor) = weak.upgrade() { let hex = editor.hex.label.text(); editor.copy(button, &hex); }
        }));
        for (index, row) in self.rows.iter().enumerate() {
            row.copy.connect_clicked(glib::clone!(#[strong] weak, move |button| {
                if let Some(editor) = weak.upgrade() { let text = editor.rows[index].text.borrow().clone(); editor.copy(button, &text); }
            }));
            for (choice, form) in row.choices.iter().zip(layer_ui::COLOR_FORM_FAMILIES[index]) {
                let form = *form;
                choice.connect_toggled(glib::clone!(#[strong] weak, move |choice| {
                    let Some(editor) = weak.upgrade() else { return; };
                    if editor.updating.get() || !choice.is_active() { return; }
                    if let Some(popover) = editor.rows[index].menu.popover() { popover.popdown(); }
                    let result = editor.apply(ColorEditorAction::Form { row: index, form });
                    editor.report(Target::Dialog, result);
                }));
            }
        }
        for field in self.fields() {
            let target = field.target;
            let find = glib::clone!(#[strong] weak, move || weak.upgrade().and_then(|editor| {
                let field = editor.fields().find(|field| field.target == target).cloned();
                field.map(|field| (editor, field))
            }));
            field.show.connect_clicked(glib::clone!(#[strong] find, move |_| {
                if let Some((editor, field)) = find() { editor.begin_edit(&field); }
            }));
            field.entry.connect_activate(glib::clone!(#[strong] find, move |_| {
                if let Some((editor, field)) = find() { editor.commit(&field); }
            }));
            let keys = gtk::EventControllerKey::new();
            keys.connect_key_pressed(glib::clone!(#[strong] find, move |_, key, _, _| {
                if key != gdk::Key::Escape { return glib::Propagation::Proceed; }
                if let Some((editor, field)) = find() { editor.cancel_edit(&field); }
                glib::Propagation::Stop
            }));
            field.entry.add_controller(keys);
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(glib::clone!(#[strong] find, move |_| {
                if let Some((editor, field)) = find() && !editor.picking.get() { editor.commit(&field); }
            }));
            field.entry.add_controller(focus);
            if let Some(text) = field.entry.delegate().and_downcast::<gtk::Text>() {
                text.connect_preedit_changed(glib::clone!(#[strong] weak, move |_, _| {
                    if let Some(editor) = weak.upgrade() { editor.refresh_error(); }
                }));
            }
            let Target::Number(target) = target else { continue; };
            let steps = gtk::EventControllerKey::new();
            steps.connect_key_pressed(glib::clone!(#[strong] weak, move |_, key, _, modifiers| {
                let Some(editor) = weak.upgrade() else { return glib::Propagation::Proceed; };
                let direction = match key { gdk::Key::Up | gdk::Key::KP_Up => 1., gdk::Key::Down | gdk::Key::KP_Down => -1., _ => return glib::Propagation::Proceed };
                editor.step(target, direction, speed(modifiers));
                glib::Propagation::Stop
            }));
            field.show.add_controller(steps);
            let scrubbing = Rc::new(Cell::new(false));
            let drag = gtk::GestureDrag::new();
            drag.set_button(1);
            drag.set_propagation_phase(gtk::PropagationPhase::Capture);
            drag.connect_drag_begin(glib::clone!(#[strong] scrubbing, move |_, _, _| scrubbing.set(false)));
            drag.connect_drag_update(glib::clone!(#[strong] weak, #[strong] scrubbing, move |gesture, _, dy| {
                let Some(editor) = weak.upgrade() else { return; };
                if !scrubbing.get() {
                    if dy.abs() < 4. { return; }
                    scrubbing.set(true);
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                }
                let result = editor.apply(ColorEditorAction::Scrub { target, pixels: -dy as f32, speed: speed(gesture.current_event_state()) });
                editor.report(Target::Number(target), result);
            }));
            drag.connect_drag_end(glib::clone!(#[strong] weak, #[strong] scrubbing, move |_, _, _| {
                if scrubbing.replace(false) && let Some(editor) = weak.upgrade() {
                    let result = editor.apply(ColorEditorAction::EndScrub { cancel: false });
                    editor.report(Target::Number(target), result);
                }
            }));
            drag.connect_cancel(glib::clone!(#[strong] weak, move |_, _| {
                if scrubbing.replace(false) && let Some(editor) = weak.upgrade() {
                    let result = editor.apply(ColorEditorAction::EndScrub { cancel: true });
                    editor.report(Target::Number(target), result);
                }
            }));
            field.show.add_controller(drag);
        }
        self.swatches.connect_clicked(glib::clone!(#[strong] weak, move |_| {
            if let Some(editor) = weak.upgrade() { editor.show_sheet(!editor.sheet_open.get()); }
        }));
        self.sheet_close.connect_clicked(glib::clone!(#[strong] weak, move |_| if let Some(editor) = weak.upgrade() { editor.show_sheet(false); }));
        self.sheet_slide.connect_done(glib::clone!(#[strong] weak, move |_| {
            if let Some(editor) = weak.upgrade().filter(|editor| !editor.sheet_open.get()) { editor.sheet.set_visible(false); }
        }));
        self.search.connect_search_changed(glib::clone!(#[strong] weak, move |search| {
            let Some(editor) = weak.upgrade() else { return; };
            if editor.updating.get() { return; }
            let result = editor.draft.borrow_mut().apply(ColorEditorAction::Search { text: search.text().into() });
            if result.is_ok() { editor.fill_sheet(); }
        }));
        self.search.connect_stop_search(glib::clone!(#[strong] weak, move |search| {
            let Some(editor) = weak.upgrade() else { return; };
            if search.text().is_empty() { editor.show_sheet(false); } else { search.set_text(""); }
        }));
        let sheet_keys = gtk::EventControllerKey::new();
        sheet_keys.connect_key_pressed(glib::clone!(#[strong] weak, move |_, key, _, _| {
            if key != gdk::Key::Escape { return glib::Propagation::Proceed; }
            if let Some(editor) = weak.upgrade() { editor.show_sheet(false); }
            glib::Propagation::Stop
        }));
        self.sheet.add_controller(sheet_keys);
        let clipboard_keys = gtk::EventControllerKey::new();
        clipboard_keys.connect_key_pressed(glib::clone!(#[strong] weak, move |_, key, _, modifiers| {
            let Some(editor) = weak.upgrade() else { return glib::Propagation::Proceed; };
            if !modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                || editor.dialog.root().and_then(|root| root.focus()).is_some_and(|focus| focus.is::<gtk::Text>()) {
                return glib::Propagation::Proceed;
            }
            match key.to_lower() {
                gdk::Key::c => {
                    let hex = editor.hex.label.text();
                    editor.dialog.clipboard().set_text(&hex);
                }
                gdk::Key::v => {
                    let clipboard = editor.dialog.clipboard();
                    let weak = editor.this.clone();
                    glib::spawn_future_local(async move {
                        if let Ok(Some(text)) = clipboard.read_text_future().await && let Some(editor) = weak.upgrade() { editor.paste(&text); }
                    });
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        }));
        self.dialog.add_controller(clipboard_keys);
        self.cancel.connect_clicked(glib::clone!(#[strong] weak, move |_| if let Some(editor) = weak.upgrade() { editor.dialog.close(); }));
        self.apply_button.connect_clicked(glib::clone!(#[strong] weak, move |_| {
            let Some(editor) = weak.upgrade() else { return; };
            if editor.error.borrow().is_some() { return; }
            editor.finish(true);
            editor.dialog.close();
        }));
        self.dialog.connect_closed(glib::clone!(#[strong] weak, move |_| {
            let Some(editor) = weak.upgrade() else { return; };
            if editor.picking.get() { return; }
            editor.finish(false);
            if let Some(workspace) = editor.workspace.upgrade() {
                workspace.color_editors.borrow_mut().retain(|open| !Rc::ptr_eq(open, &editor));
            }
        }));
        workspace.on_localization(glib::clone!(#[strong] weak, move |localization| {
            let Some(editor) = weak.upgrade() else { return false; };
            *editor.localization.borrow_mut() = localization.clone();
            editor.relabel();
            editor.refresh();
            if editor.sheet_open.get() { editor.fill_sheet(); }
            true
        }));
        if let Some(editor) = self.this.upgrade() { workspace.color_editors.borrow_mut().push(editor); }
    }
}

fn speed(modifiers: gdk::ModifierType) -> ColorScrubSpeed {
    if modifiers.contains(gdk::ModifierType::SHIFT_MASK) { ColorScrubSpeed::Fast }
    else if modifiers.intersects(gdk::ModifierType::ALT_MASK | gdk::ModifierType::CONTROL_MASK) { ColorScrubSpeed::Fine }
    else { ColorScrubSpeed::Normal }
}

pub(crate) struct Strip {
    pub(crate) root: gtk::Button,
    original: ColorPatch,
    sample: ColorPatch,
    hex: gtk::Label,
    intensity: gtk::Label,
    space: gtk::Label,
    values: gtk::Label,
    pub(crate) corner: Cell<ColorStripCorner>,
    hover: Cell<Option<[f32; 2]>>,
}
impl Strip {
    pub(crate) fn new() -> Self {
        let (pair, original, sample) = pair(20, 40);
        let hex = gtk::Label::builder().xalign(0.).hexpand(true).build();
        hex.add_css_class("edit-color-strip-hex");
        let intensity = gtk::Label::builder().xalign(1.).build();
        intensity.add_css_class("edit-color-number");
        let space = gtk::Label::builder().xalign(0.).hexpand(true).build();
        space.add_css_class("dim-label");
        let values = gtk::Label::builder().xalign(1.).build();
        values.add_css_class("edit-color-number");
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        top.append(&hex);
        top.append(&intensity);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.append(&space);
        line.append(&values);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.append(&top);
        text.append(&line);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        content.append(&pair);
        content.append(&text);
        let root = gtk::Button::builder().child(&content).halign(gtk::Align::Start).valign(gtk::Align::Start).visible(false).build();
        root.add_css_class("edit-color-strip");
        root.set_widget_name("edit-color-strip");
        root.set_size_request(248, -1);
        Self { root, original, sample, hex, intensity, space, values, corner: Cell::default(), hover: Cell::default() }
    }
    pub(crate) fn bind(&self, workspace: &Rc<Workspace>) {
        let workspace = Rc::downgrade(workspace);
        self.root.connect_clicked(glib::clone!(#[strong] workspace, move |_| {
            let Some(workspace) = workspace.upgrade() else { return; };
            let picking = workspace.gpu.borrow().as_ref().is_some_and(|g| g.session.state().color_picker.editor);
            if picking { workspace.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Toggle }); }
        }));
        let motion = gtk::EventControllerMotion::new();
        let hover = move |point: Option<[f64; 2]>| if let Some(workspace) = workspace.upgrade() { workspace.color_strip.hover(&workspace, point); };
        let enter = hover.clone();
        motion.connect_enter(move |_, x, y| enter(Some([x, y])));
        let moved = hover.clone();
        motion.connect_motion(move |_, x, y| moved(Some([x, y])));
        motion.connect_leave(move |_| hover(None));
        self.root.add_controller(motion);
    }
    fn show(&self, workspace: &Workspace, view: &ColorStripView, tip: &str, [original, sample]: [RgbColor; 2], display: ViewColor, headroom: f32) {
        self.original.set_display_color(original, display, headroom);
        self.sample.set_display_color(sample, display, headroom);
        self.hex.set_text(&view.hex);
        self.intensity.set_visible(view.intensity.is_some());
        self.intensity.set_text(view.intensity.as_deref().unwrap_or_default());
        self.space.set_text(view.label);
        self.values.set_text(&view.values.join("  "));
        self.place(workspace);
        self.root.set_tooltip_text(Some(tip));
        self.root.update_property(&[gtk::accessible::Property::Label(tip)]);
        self.root.set_visible(true);
    }
    fn hide(&self) {
        self.root.set_visible(false);
    }
    pub(crate) fn hover(&self, workspace: &Workspace, point: Option<[f64; 2]>) {
        let scale = workspace.area.scale_factor() as f32;
        let point = point.and_then(|[x, y]| self.root.compute_point(&workspace.area, &gtk::graphene::Point::new(x as f32, y as f32)));
        self.hover.set(point.map(|p| [p.x() * scale, p.y() * scale]));
        self.place(workspace);
    }
    fn place(&self, workspace: &Workspace) {
        let Some(parent) = self.root.parent() else { return; };
        let Some((area, sample)) = workspace.gpu.borrow().as_ref().map(|g| (g.session.state().camera.work_area, g.session.state().color_picker.sample_point)) else { return; };
        let scale = workspace.area.scale_factor() as f32;
        let origin = workspace.area.compute_point(&parent, &gtk::graphene::Point::zero()).unwrap_or_else(gtk::graphene::Point::zero);
        let height = self.root.child().map_or(0, |content| content.measure(gtk::Orientation::Vertical, -1).1) + 16;
        let avoid: Vec<[f32; 2]> = sample.into_iter().chain(self.hover.get()).collect();
        let placement = ColorStripPlacement::new(area, [self.root.width_request(), height].map(|v| v as f32 * scale), scale, &avoid, self.corner.get());
        self.corner.set(placement.corner);
        self.root.set_margin_start((origin.x() + placement.origin[0] / scale).max(0.).round() as i32);
        self.root.set_margin_top((origin.y() + placement.origin[1] / scale).max(0.).round() as i32);
    }
}

pub(crate) fn refresh_display(workspace: &Workspace) {
    let view = workspace.paint_view_color();
    let headroom = workspace.picker_headroom();
    let editors = workspace.color_editors.borrow().clone();
    for editor in editors {
        let previous = (editor.view.replace(view), editor.headroom.replace(headroom));
        if previous != (view, headroom) {
            editor.refresh();
            editor.fill_recent();
        }
        editor.refresh_picking(workspace);
    }
}

pub fn show(workspace: &Rc<Workspace>, slot: ColorSlot) {
    let Some(draft) = workspace.gpu.borrow().as_ref().map(|g| ColorEditor::for_slot(g.session.state().display_colors(), slot)) else {
        return;
    };
    open(workspace, draft, move |workspace, color, intensity| {
        workspace.dispatch(UiAction::Color {
            action: if let Some(stops) = intensity { ColorAction::SetSlotIntensity { slot, color, stops } }
                else { ColorAction::SetSlot { slot, color } },
        });
    });
}

pub fn choose(
    workspace: &Rc<Workspace>,
    definition: RgbColor,
    accepted: impl FnOnce(&Rc<Workspace>, RgbColor) + 'static,
) {
    choose_opaque(workspace, definition, false, accepted);
}

pub(crate) fn edit_fill(workspace: &Rc<Workspace>, layer: u64, fill: layer_ui::LayerFillColor) {
    choose_opaque(workspace, fill.color, fill.opaque, move |workspace, color| {
        workspace.dispatch(UiAction::Effect { action: layer_ui::EffectAction::Set { layer, key: fill.key, value: layer_core::EffectValue::Color(color) } });
    });
}

fn choose_opaque(workspace: &Rc<Workspace>, definition: RgbColor, opaque: bool, accepted: impl FnOnce(&Rc<Workspace>, RgbColor) + 'static) {
    let Some(draft) = workspace.gpu.borrow().as_ref().map(|g| ColorEditor::for_color(g.session.state().display_colors(), definition, opaque)) else {
        return;
    };
    open(workspace, draft, move |workspace, color, _| accepted(workspace, color));
}

fn open(
    workspace: &Rc<Workspace>,
    draft: Result<ColorEditor, ColorEditorError>,
    accepted: impl FnOnce(&Rc<Workspace>, RgbColor, Option<f32>) + 'static,
) {
    let draft = match draft {
        Ok(draft) => draft,
        Err(error) => {
            workspace.changed(Err(error.message(&workspace.localization())));
            return;
        }
    };
    let Some(epoch) = workspace.gpu.borrow().as_ref().map(|g| g.session.state().document_file.epoch) else { return; };
    let can_pick = workspace.window.visible_dialog().is_none();
    let editor = Editor::build(workspace, draft, epoch, Box::new(accepted));
    editor.bind(workspace, can_pick);
    editor.dialog.present(Some(&workspace.window));
}

pub struct ColorButton {
    pub widget: gtk::Button,
    definition: Cell<RgbColor>,
    pub opaque: Cell<bool>,
    patch: ColorPatch,
}
impl ColorButton {
    pub fn new() -> Rc<Self> {
        let patch = ColorPatch::new(false);
        patch.set_size_request(32, 20);
        let widget = gtk::Button::builder()
            .child(&patch)
            .build();
        Rc::new(Self {
            widget,
            definition: Cell::new(RgbColor::BLACK),
            opaque: Cell::new(false),
            patch,
        })
    }
    pub fn color(&self) -> RgbColor {
        self.definition.get()
    }
    pub fn set_color(&self, color: RgbColor, view: ViewColor) {
        self.set_display_color(color, view, 1.);
    }
    pub fn set_display_color(&self, color: RgbColor, view: ViewColor, headroom: f32) {
        self.definition.set(color);
        self.patch.set_display_color(color, view, headroom);
    }
    pub fn bind(
        self: &Rc<Self>,
        workspace: &Rc<Workspace>,
        accepted: impl Fn(&Rc<Workspace>, RgbColor) + 'static,
    ) {
        self.bind_copy(workspace);
        let weak = Rc::downgrade(self);
        let workspace = Rc::downgrade(workspace);
        let accepted = Rc::new(accepted);
        self.widget.connect_clicked(move |_| {
            let (Some(button), Some(workspace)) = (weak.upgrade(), workspace.upgrade()) else {
                return;
            };
            let original = button.color();
            let weak = weak.clone();
            let accepted = accepted.clone();
            choose_opaque(&workspace, original, button.opaque.get(), move |workspace, color| {
                let Some(button) = weak.upgrade() else {
                    return;
                };
                if button.widget.root().is_some() && button.color() == original {
                    accepted(workspace, color);
                }
            });
        });
    }
    pub(crate) fn bind_copy(&self, workspace: &Workspace) {
        let widget = self.widget.downgrade();
        workspace.on_localization(move |localization| {
            let Some(widget) = widget.upgrade() else { return false; };
            let copy = layer_ui::NativeCopy::new(localization).color;
            widget.set_tooltip_text(Some(&copy.edit));
            widget.update_property(&[gtk::accessible::Property::Label(&copy.edit)]);
            true
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::workspace::tests::{NativeTestApp, new_drawing_at, new_photo::{ready, response}, pump, save_snapshot, until};
    use layer_core::color::SampleDepth;
    use layer_ui::{ColorForm, EffectAction, PreferenceAction, PreferenceId, PreferenceValue, Theme, UiLanguage, COLOR_FORM_FAMILIES};

    pub(crate) fn label(row: &Row) -> String {
        row.names.visible_child().and_then(|name| name.first_child()).and_downcast::<gtk::Label>().map(|label| label.text().to_string()).unwrap_or_default()
    }
    pub(crate) fn editor(w: &Workspace) -> Rc<Editor> {
        until(|| w.color_editors.borrow().iter().any(|editor| editor.dialog.is_mapped()), "mapped Edit Color dialog");
        w.color_editors.borrow().iter().rev().find(|editor| editor.dialog.is_mapped()).unwrap().clone()
    }
    pub(crate) fn type_into(w: &Workspace, name: &str, text: &str) {
        let editor = editor(w);
        let field = editor.fields().find(|field| field.show.widget_name() == name).unwrap_or_else(|| panic!("{name}")).clone();
        if !field.editing() { field.show.emit_clicked(); }
        assert!(field.editing() && field.entry.delegate().is_some_and(|text| text.is_focus()), "{name} opens for typing");
        field.entry.set_text(text);
        field.entry.emit_activate();
        pump(20);
    }
    pub(crate) fn value(w: &Workspace, row: usize, index: usize, text: &str) {
        type_into(w, &format!("edit-color-value-{row}-{index}"), text);
    }
    pub(crate) fn form(w: &Workspace, row: usize, form: ColorForm) {
        let editor = editor(w);
        let index = COLOR_FORM_FAMILIES[row].iter().position(|candidate| *candidate == form).unwrap();
        editor.rows[row].choices[index].set_active(true);
        pump(20);
        assert_eq!(editor.draft.borrow().memory().forms[row], form);
    }
    pub(crate) fn every_form(w: &Workspace) {
        for (row, family) in COLOR_FORM_FAMILIES.iter().enumerate() {
            for candidate in family.iter().chain(&family[..1]) { form(w, row, *candidate); }
        }
    }

    #[test]
    #[ignore = "private display and hardware GPU retained color drafts"]
    fn native_color_editor_live_language() {
        let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
        let (application, active) = crate::application("art.capycanvas.ColorDraftLanguages");
        let app = NativeTestApp(application);
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        for depth in [SampleDepth::U8, SampleDepth::F16] {
            let index = active.borrow().len();
            crate::open_workspace(&app, &active, Some((new_drawing_at(64, 64, depth), None)));
            until(|| active.borrow().len() > index, "prepared color draft window");
            let w = active.borrow().last().unwrap().clone();
            w.window.maximize(); w.window.present(); ready(&w);
            w.dispatch(UiAction::Effect { action:EffectAction::Insert { effect:"black_white".into() } });
            w.dispatch(UiAction::Color { action:ColorAction::Definition { color:RgbColor::WHITE } });
            ready(&w);
            let switch = |language| {
                let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32;
                w.dispatch(UiAction::Preferences { action:PreferenceAction::Edit { id:PreferenceId::Language, value:PreferenceValue::Choice(choice) } });
                until(|| w.localization().language() == language, "retained color draft language");
            };
            let checkpoint = w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint();
            let original = w.gpu.borrow().as_ref().unwrap().session.state().colors.clone();
            for theme in [Theme::Light, Theme::Dark] {
                w.dispatch(UiAction::SetTheme { theme:Some(theme) }); ready(&w);
                show(&w, ColorSlot::Foreground);
                let editor = editor(&w);
                assert_eq!(editor.intensity.stack.is_visible(), depth.is_float());
                assert!(editor.apply_button.is_sensitive());
                assert_eq!(editor.hex.label.text(), "#FFFFFF");
                if depth.is_float() { type_into(&w, "edit-color-ev", "2"); }
                let draft = editor.draft.borrow().clone();
                for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                    switch(language);
                    let copy = layer_ui::NativeCopy::new(&w.localization()).color;
                    assert_eq!(*editor.draft.borrow(), draft);
                    assert_eq!(editor.title.text(), copy.edit.as_ref());
                    assert_eq!(editor.apply_button.label().as_deref(), Some(copy.use_color.as_ref()));
                    assert_eq!(editor.current_caption.text(), copy.current.as_ref());
                    assert_eq!(editor.pick.tooltip_text().as_deref(), Some(copy.pick_canvas.as_ref()));
                    assert!(editor.apply_button.is_sensitive());
                    save_snapshot(&w, 60, || output.join(format!("color-valid-{depth:?}-{}-{theme:?}.png", language.tag())));
                    if matches!(language, UiLanguage::French | UiLanguage::German) {
                        let settings = gtk::Settings::default().unwrap();
                        let previous = settings.property::<String>("gtk-font-name");
                        for (font, width) in [("Sans 16", None), (previous.as_str(), Some(w.window.width().min(744)))] {
                            settings.set_property("gtk-font-name", font);
                            if let Some(width) = width { w.window.unmaximize(); w.window.set_default_size(width, 780); }
                            pump(300);
                            let sheet = editor.dialog.child().unwrap();
                            for widget in [editor.apply_button.upcast_ref::<gtk::Widget>(), editor.rows[2].copy.upcast_ref(), editor.hex_copy.upcast_ref(), editor.wheel.upcast_ref()] {
                                let bounds = widget.compute_bounds(&sheet).unwrap();
                                assert!(bounds.x() >= -1. && bounds.x() + bounds.width() <= sheet.width() as f32 + 1., "{} fits with {font} in {}: {bounds:?}", widget.widget_name(), language.tag());
                            }
                            for row in &editor.rows {
                                assert!(!label(row).is_empty());
                            }
                            save_snapshot(&w, 60, || output.join(format!("color-{}-{depth:?}-{}-{theme:?}.png", if width.is_some() { "narrow" } else { "large" }, language.tag())));
                        }
                        w.window.maximize(); pump(250);
                    }
                }
                let literal = "Tiếng Việt ไทย İı {draft} 🎨";
                value(&w, 0, 0, literal);
                let field = editor.rows[0].fields[0].clone();
                assert!(field.editing() && field.entry.has_css_class("error"));
                field.entry.select_region(1, 6);
                let selection = field.entry.selection_bounds();
                assert!(!editor.apply_button.is_sensitive());
                for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                    switch(language);
                    let (_, error) = editor.error.borrow().clone().unwrap();
                    assert!(matches!(error, ColorEditorError::Numeric { .. }));
                    assert_eq!(editor.validation.text(), error.message(&w.localization()));
                    assert_eq!(field.entry.text(), literal);
                    assert_eq!(field.entry.selection_bounds(), selection);
                    assert!(!editor.apply_button.is_sensitive());
                    assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint(), checkpoint);
                    save_snapshot(&w, 60, || output.join(format!("color-draft-{depth:?}-{}-{theme:?}.png", language.tag())));
                }
                switch(UiLanguage::English);
                field.entry.emit_by_name::<()>("activate", &[]);
                assert!(field.editing());
                editor.cancel_edit(&field);
                assert!(!field.editing() && editor.apply_button.is_sensitive() && !editor.validation.is_visible());
                response(&w, "cancel");
                pump(50);
                assert_eq!(w.gpu.borrow().as_ref().unwrap().session.state().colors.definition(), original.definition());
                assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint(), checkpoint);
                eprintln!("GTK color draft {depth:?}/{theme:?}: retained refusal, copy and layouts in all languages passed");
            }
            assert!(w.window.visible_dialog().is_none());assert!(!w.servicing.get());
            assert!(w.gpu.borrow().as_ref().unwrap().session.can_park_document());
            let before_epoch = w.gpu.borrow().as_ref().unwrap().session.state().document_file.epoch;
            let before_notice = w.gpu.borrow().as_ref().unwrap().session.state().notice.as_ref().map(|notice|notice.id);
            let mut opening = std::pin::pin!(w.documents.open(&w, (new_drawing_at(80, 80, depth), None)));
            let mut shown = false;
            glib::MainContext::default().block_on(std::future::poll_fn(|context| {
                use std::future::Future;
                match opening.as_mut().poll(context) {
                    std::task::Poll::Pending => {
                        if !shown && w.documents.changing.get() && w.gpu.borrow().is_some() {
                            show(&w, ColorSlot::Foreground);shown = true;
                        }
                        std::task::Poll::Pending
                    }
                    result => result,
                }
            })).unwrap();
            assert!(shown,"color dialog opens after document transport admission and before async replacement");
            until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "edit-color-dialog" && dialog.is_mapped()), "stale color dialog");
            ready(&w);
            {
                let gpu=w.gpu.borrow();let session=&gpu.as_ref().unwrap().session;
                assert_ne!(session.state().document_file.epoch,before_epoch);
                assert_eq!(session.engine().document().composition().size,[80,80]);
            }
            let replacement = w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint();
            value(&w, 0, 0, "12");
            response(&w, "apply");
            until(|| w.gpu.borrow().as_ref().unwrap().session.state().notice.is_some(), "stale color action refusal");
            let notice = w.gpu.borrow().as_ref().unwrap().session.state().notice.clone().unwrap();
            assert_ne!(Some(notice.id),before_notice);
            for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                switch(language);
                let current = w.gpu.borrow().as_ref().unwrap().session.state().notice.clone().unwrap();
                assert_eq!(current.id, notice.id);
                assert_eq!(current.text, w.localization().text(layer_ui::MessageId::COMMON_ACTION_FAILED).as_ref());
                assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint(), replacement);
            }
            w.window.destroy(); pump(100);
        }
    }
}
