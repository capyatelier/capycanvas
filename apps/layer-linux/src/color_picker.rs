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

type Choices = (Vec<std::sync::Arc<str>>, Vec<std::sync::Arc<str>>);
pub struct Settings {
    pub root: gtk::Box,
    source: gtk::DropDown,
    size: gtk::DropDown,
    source_row: gtk::Box,
    labels: [gtk::Label; 2],
    copy: RefCell<layer_ui::native_copy::SamplerCopy>,
    choices: RefCell<Choices>,
    widths: Rc<RefCell<Vec<u32>>>,
    owner: Rc<RefCell<Weak<Workspace>>>,
    updating: Rc<Cell<bool>>,
}
impl Settings {
    pub fn new(localization: &layer_ui::Localizer) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, crate::panel_controls::CONTROL_SPACING);
        root.set_widget_name("color-picker-settings");
        let source = crate::panel_controls::dropdown(&[]);
        source.set_widget_name("color-picker-source");
        let size = crate::panel_controls::dropdown(&[]);
        size.set_widget_name("color-picker-size");
        let group = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        let [source_row, size_row] = [&source, &size].map(|widget| {
            let row = crate::panel_controls::row("", widget, crate::panel_controls::CONTROL_SPACING, Some(&group));
            root.append(&row);
            row
        });
        let labels = [&source_row, &size_row].map(|row| row.first_child().and_downcast::<gtk::Label>().unwrap());
        let owner: Rc<RefCell<Weak<Workspace>>> = Rc::default();
        let updating = Rc::new(Cell::new(false));
        let widths: Rc<RefCell<Vec<u32>>> = Rc::default();
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
            #[strong]
            widths,
            move |d| {
                let workspace = owner.borrow().upgrade();
                let width = widths.borrow().get(d.selected() as usize).copied();
                if !updating.get()
                    && let Some(w) = workspace
                    && let Some(width) = width
                {
                    w.dispatch(UiAction::SetColorSampleSize { width });
                }
            }
        ));
        let settings = Self {
            root,
            source,
            size,
            source_row,
            labels,
            copy: RefCell::new(layer_ui::NativeCopy::new(localization).sampler),
            choices: RefCell::default(),
            widths,
            owner,
            updating,
        };
        settings.set_localization(localization);
        settings
    }
    pub fn set_localization(&self, localization: &layer_ui::Localizer) {
        let copy = layer_ui::NativeCopy::new(localization).sampler;
        for ((label, widget), text) in self.labels.iter().zip([&self.source, &self.size]).zip([&copy.source, &copy.sample_size]) {
            label.set_text(text);
            widget.update_property(&[gtk::accessible::Property::Label(text)]);
        }
        *self.copy.borrow_mut() = copy;
    }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        *self.owner.borrow_mut() = Rc::downgrade(w);
        self.updating.set(true);
        let picker = &state.color_picker;
        let copy = self.copy.borrow();
        let sources: Vec<_> = std::iter::once(copy.visible_color.clone())
            .chain(picker.can_sample_layer.then(|| copy.selected_layer.clone()))
            .collect();
        let sizes: Vec<_> = picker.sample_sizes.iter()
            .filter_map(|width| copy.sizes.iter().find(|(value, _)| value == width).map(|(_, label)| label.clone()))
            .collect();
        let (source_choices, size_choices) = &mut *self.choices.borrow_mut();
        for (dropdown, current, next) in [(&self.source, source_choices, sources), (&self.size, size_choices, sizes)] {
            if *current != next {
                dropdown.set_model(Some(&gtk::StringList::new(&next.iter().map(AsRef::as_ref).collect::<Vec<_>>())));
                *current = next;
            }
        }
        *self.widths.borrow_mut() = picker.sample_sizes.to_vec();
        self.source_row.set_visible(!picker.calibrating);
        self.source.set_selected(u32::from(picker.layer && picker.can_sample_layer));
        self.size.set_selected(
            picker.sample_sizes.iter().position(|&n| n == picker.sample_width).map_or(gtk::INVALID_LIST_POSITION, |i| i as u32),
        );
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
    let label = w.gpu.borrow().as_ref().map(|g| g.session.color_picker_button_label(control));
    if let Some(label) = label {
        button.set_tooltip_text(Some(&label));
    }
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
pub(crate) fn finger_offset(w: &Workspace) -> f32 {
    let scale = w.area.scale_factor() as f32;
    let monitor = w
        .area
        .native()
        .and_then(|n| n.surface())
        .and_then(|s| s.display().monitor_at_surface(&s));
    layer_ui::finger_loupe_offset(monitor.map(|m| [m.geometry().height() as f32, m.height_mm() as f32]), scale)
}
