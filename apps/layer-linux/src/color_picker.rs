//! GTK picker controls, native hold recognition and double-press timing.
use crate::workspace::Workspace;
use gtk::{glib, prelude::*};
use layer_ui::{
    ColorPickerAction, ContactPhase, DrawerAnchor, ToolbarControl,
    UiAction, UiInput, UiState,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::Duration,
};

pub struct Settings {
    pub root: gtk::Box,
    source: gtk::DropDown,
    size: gtk::DropDown,
    owner: Rc<RefCell<Weak<Workspace>>>,
    updating: Rc<Cell<bool>>,
}
impl Settings {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_widget_name("color-picker-settings");
        root.set_margin_start(8);
        root.set_margin_end(8);
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        let source = gtk::DropDown::from_strings(&["Visible color", "Selected layer"]);
        source.set_widget_name("color-picker-source");
        let sizes = layer_ui::COLOR_SAMPLE_WIDTHS.map(|width| {
            if width == 1 { "Single pixel".to_string() } else { format!("{width} px circle") }
        });
        let size = gtk::DropDown::from_strings(&sizes.each_ref().map(String::as_str));
        size.set_widget_name("color-picker-size");
        for (name, widget) in [("Source", &source), ("Sample size", &size)] {
            let row = gtk::Box::new(gtk::Orientation::Vertical, 5);
            let label = gtk::Label::new(Some(name));
            label.set_xalign(0.);
            label.add_css_class("dim-label");
            widget.set_hexpand(true);
            widget.update_property(&[gtk::accessible::Property::Label(name)]);
            row.append(&label);
            row.append(widget);
            root.append(&row);
        }
        let owner: Rc<RefCell<Weak<Workspace>>> = Rc::default();
        let updating = Rc::new(Cell::new(false));
        source.connect_selected_notify(glib::clone!(
            #[strong]
            owner,
            #[strong]
            updating,
            move |d| {
                let workspace = owner.borrow().upgrade();
                if !updating.get()
                    && let Some(w) = workspace
                {
                    w.dispatch(UiAction::ColorPicker {
                        action: ColorPickerAction::Source {
                            layer: d.selected() == 1,
                        },
                    });
                }
            }
        ));
        size.connect_selected_notify(glib::clone!(
            #[strong]
            owner,
            #[strong]
            updating,
            move |d| {
                let workspace = owner.borrow().upgrade();
                if !updating.get()
                    && let Some(w) = workspace
                    && let Some(&width) = layer_ui::COLOR_SAMPLE_WIDTHS.get(d.selected() as usize)
                {
                    w.dispatch(UiAction::SetColorSampleSize { width });
                }
            }
        ));
        Self {
            root,
            source,
            size,
            owner,
            updating,
        }
    }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        *self.owner.borrow_mut() = Rc::downgrade(w);
        self.updating.set(true);
        let model = if state.color_picker.can_sample_layer {
            &["Visible color", "Selected layer"][..]
        } else {
            &["Visible color"][..]
        };
        if self
            .source
            .model()
            .is_none_or(|m| m.n_items() != model.len() as u32)
        {
            self.source.set_model(Some(&gtk::StringList::new(model)));
        }
        self.source.set_selected(u32::from(
            state.color_picker.layer && state.color_picker.can_sample_layer,
        ));
        self.source
            .set_tooltip_text(Some(if state.color_picker.can_sample_layer {
                "Visible artwork, or raw paint from the selected layer"
            } else {
                "Select an editable paint layer to enable layer sampling"
            }));
        self.size.set_selected(
            layer_ui::COLOR_SAMPLE_WIDTHS
                .iter()
                .position(|&n| n == state.color_picker.sample_width)
                .unwrap_or(0) as u32,
        );
        self.size
            .set_tooltip_text(Some("Document pixels · perceptual Oklab average"));
        self.updating.set(false);
    }
}

/// The picker has a single-press cancel action, so native double-press explicitly
/// opens its drawer instead of invoking the button a second time.
pub fn bind_button(
    w: &Rc<Workspace>,
    button: &gtk::Button,
    anchor: DrawerAnchor,
    control: ToolbarControl,
) {
    button.set_tooltip_text(Some(&format!(
        "{} (I) · Double-press for options",
        layer_ui::tool_choice(control).label
    )));
    let pressed = Rc::new(Cell::new(None));
    let last = Rc::new(Cell::new(None::<(u32, Option<gtk::gdk::InputSource>)>));
    let click = gtk::GestureClick::new();
    click.set_button(1);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    click.connect_pressed(glib::clone!(
        #[strong]
        pressed,
        move |g, _, _, _| {
            pressed.set(
                g.current_event()
                    .map(|e| (e.time(), e.device().map(|d| d.source()))),
            );
        }
    ));
    button.add_controller(click);
    button.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |_| {
            if matches!(anchor, DrawerAnchor::Header { .. })
                && w.gpu
                    .borrow()
                    .as_ref()
                    .is_some_and(|g| g.session.state().customization.header_editing)
            {
                pressed.take();
                last.set(None);
                return;
            }
            let now = pressed.take();
            let previous = last.replace(now);
            let settings = gtk::Settings::for_display(&w.area.display());
            let double = now
                .zip(previous)
                .is_some_and(|((time, device), (old, old_device))| {
                    device == old_device
                        && time.wrapping_sub(old) <= settings.gtk_double_click_time().max(0) as u32
                });
            if !double {
                w.dispatch(control.action().unwrap());
                return;
            }
            last.set(None);
            w.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Settings { anchor } });
        }
    ));
}

#[derive(Default)]
pub struct Hold {
    pending: RefCell<Option<(u64, [f32; 2], glib::SourceId)>>,
}
impl Hold {
    pub fn cancel(&self) {
        if let Some((_, _, timer)) = self.pending.borrow_mut().take() {
            timer.remove();
        }
    }
    pub fn input(
        self: &Rc<Self>,
        w: &Rc<Workspace>,
        id: u64,
        phase: ContactPhase,
        position: [f32; 2],
        contacts: usize,
    ) {
        // Shared Rust checks ownership when the timer fires; this collector
        // only recognizes native timing, slop, and the contact's lifetime.
        if contacts > 1 || matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
            self.cancel();
            return;
        }
        if phase == ContactPhase::Move {
            let drifted = self
                .pending
                .borrow()
                .as_ref()
                .is_some_and(|(contact, p, _)| {
                    *contact == id
                        && w.area.drag_check_threshold(
                            p[0] as i32,
                            p[1] as i32,
                            position[0] as i32,
                            position[1] as i32,
                        )
                });
            if drifted {
                self.cancel();
            }
            return;
        }
        if phase != ContactPhase::Down {
            return;
        }
        self.cancel();
        let delay = gtk::Settings::for_display(&w.area.display()).gtk_long_press_time();
        let timer = glib::timeout_add_local_once(
            Duration::from_millis(delay.into()),
            glib::clone!(
                #[weak]
                w,
                #[weak(rename_to = hold)]
                self,
                move || {
                    hold.pending.borrow_mut().take();
                    let scale = w.area.scale_factor() as f32;
                    let offset = finger_offset(&w);
                    w.interact(UiInput::ColorPickerHold {
                        id,
                        position: position.map(|v| v * scale),
                        offset,
                    });
                }
            ),
        );
        *self.pending.borrow_mut() = Some((id, position, timer));
    }
}
fn finger_offset(w: &Workspace) -> f32 {
    let scale = w.area.scale_factor() as f32;
    let monitor = w
        .area
        .native()
        .and_then(|n| n.surface())
        .and_then(|s| s.display().monitor_at_surface(&s));
    // Ten millimetres where the monitor reports physical dimensions; logical
    // DPI is a fallback for virtual displays. Bound implausible EDID values.
    monitor.filter(|m| m.height_mm() > 0).map_or(44., |m| {
        (m.geometry().height() as f32 * 10. / m.height_mm() as f32).clamp(36., 64.)
    }) * scale
}
