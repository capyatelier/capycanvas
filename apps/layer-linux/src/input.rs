//! GTK collects native records; the shared session interprets them. No raster
//! work or UI snapshots on the pen hot path.

use crate::workspace::Workspace;
use adw::prelude::*;
use glib::translate::IntoGlib;
use gtk::{gdk, glib};
use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_ui::{ContactPhase, Modifiers, PenButton, PointerButton, PointerKind, TouchPolicy, UiInput};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap},
    rc::Rc,
};
#[path = "tablet_input.rs"]
mod tablet;

#[derive(Default)]
pub(crate) struct CompositionKeys {
    owned: RefCell<BTreeSet<u32>>,
    current: Cell<Option<u32>>,
}
impl CompositionKeys {
    pub(crate) fn capture(&self, keycode: u32, composing: bool) -> bool {
        self.current.set(Some(keycode));
        if composing { self.owned.borrow_mut().insert(keycode); }
        self.active()
    }
    pub(crate) fn active(&self) -> bool {
        self.current.get().is_some_and(|key| self.owned.borrow().contains(&key))
    }
    pub(crate) fn release(&self, keycode: u32) -> bool {
        let owned = self.owned.borrow_mut().remove(&keycode);
        if self.current.get() == Some(keycode) { self.current.set(None); }
        owned
    }
    pub(crate) fn clear(&self) {
        self.current.set(None);
        self.owned.borrow_mut().clear();
    }
}

#[derive(Default)]
pub(crate) struct EntryComposition {
    composing: Cell<bool>,
    keys: CompositionKeys,
}
impl EntryComposition {
    pub(crate) fn active(&self) -> bool { self.composing.get() || self.keys.active() }
    pub(crate) fn marked(&self) -> bool { self.composing.get() }
    pub(crate) fn capture(&self, keycode: u32) -> bool { self.keys.capture(keycode, self.composing.get()) }
    pub(crate) fn release(&self, keycode: u32) -> bool { self.keys.release(keycode) }
    pub(crate) fn clear(&self) { self.composing.set(false); self.keys.clear(); }
}
pub(crate) fn guard_entry_activation(entry: &gtk::Entry) -> Rc<EntryComposition> {
    guard_editable_activation(entry)
}

pub(crate) fn guard_editable_activation(entry: &(impl IsA<gtk::Widget> + IsA<gtk::Editable>)) -> Rc<EntryComposition> {
    if let Some(ownership) = unsafe { entry.data::<Rc<EntryComposition>>("capy-entry-composition") } {
        return unsafe { ownership.as_ref() }.clone();
    }
    let mut editable = entry.upcast_ref::<gtk::Editable>().clone();
    while let Some(delegate) = editable.delegate() { editable = delegate; }
    if let Some(ownership) = unsafe { editable.data::<Rc<EntryComposition>>("capy-entry-composition") } {
        let ownership = unsafe { ownership.as_ref() }.clone();
        unsafe { entry.set_data("capy-entry-composition", ownership.clone()); }
        return ownership;
    }
    let ownership = Rc::new(EntryComposition::default());
    if let Some(text) = editable.downcast_ref::<gtk::Text>() {
        text.connect_preedit_changed(glib::clone!(#[strong] ownership, move |_, preedit| ownership.composing.set(!preedit.is_empty())));
    }
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(glib::clone!(#[strong] ownership, move |_, _, keycode, _| {
        ownership.capture(keycode);
        glib::Propagation::Proceed
    }));
    keys.connect_key_released(glib::clone!(#[strong] ownership, move |_, _, keycode, _| { ownership.release(keycode); }));
    editable.add_controller(keys);
    let bubble = gtk::EventControllerKey::new();
    bubble.set_propagation_phase(gtk::PropagationPhase::Bubble);
    bubble.connect_key_pressed(glib::clone!(#[strong] ownership, move |_, _, _, _| {
        if ownership.active() { glib::Propagation::Stop } else { glib::Propagation::Proceed }
    }));
    editable.add_controller(bubble);
    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave(glib::clone!(#[strong] ownership, move |_| ownership.clear()));
    editable.connect_unmap(glib::clone!(#[strong] ownership, move |_| ownership.clear()));
    editable.add_controller(focus);
    unsafe { entry.set_data("capy-entry-composition", ownership.clone()); }
    unsafe { editable.set_data("capy-entry-composition", ownership.clone()); }
    ownership
}

pub(crate) fn stop_search(entry: &gtk::SearchEntry, handler: impl Fn(&gtk::SearchEntry) + 'static) {
    entry.connect_stop_search(move |search| {
        if !guard_editable_activation(search).active() { handler(search); }
    });
}

pub(crate) fn localization_input_busy(window: &impl IsA<gtk::Window>) -> bool {
    let mut busy = false;
    for window in crate::text_language::owned_windows(window) {
    crate::text_language::visit(window.upcast_ref(), &mut |widget| {
        busy |= widget.is_mapped() && (widget.is::<gtk::PopoverMenu>()
            || widget.is::<gtk::Popover>() && (widget.ancestor(gtk::DropDown::static_type()).is_some()
                || widget.ancestor(adw::ComboRow::static_type()).is_some()));
        if let Some(control) = widget.downcast_ref::<crate::number_control::NumberControl>() {
            busy |= control.composing();
        }
        if let Some(ownership) = unsafe { widget.data::<Rc<EntryComposition>>("capy-entry-composition") } {
            busy |= unsafe { ownership.as_ref() }.active();
        }
    });
    }
    busy
}

/// Only direct touch/stylus contacts open menus on a primary-button hold.
pub(crate) fn touch_or_pen(gesture: &impl IsA<gtk::Gesture>) -> bool {
    gesture_pointer_kind(gesture) != PointerKind::Mouse
}

pub(crate) fn gesture_pointer_kind(gesture: &impl IsA<gtk::Gesture>) -> PointerKind {
    gesture.last_event(gesture.last_updated_sequence().as_ref()).map_or(PointerKind::Mouse, |event| pointer_kind(&event))
}

pub(crate) fn pointer_kind(event: &gdk::Event) -> PointerKind {
    if event.device_tool().is_some() || event.device().is_some_and(|d| d.source() == gdk::InputSource::Pen) {
        PointerKind::Pen
    } else if event.device().is_some_and(|d| d.source() == gdk::InputSource::Touchscreen) {
        PointerKind::Touch
    } else {
        PointerKind::Mouse
    }
}

fn scrollbar_input(scroll: &gtk::ScrolledWindow, mouse: bool) {
    for bar in [scroll.hscrollbar(), scroll.vscrollbar()] {
        bar.set_opacity(if mouse { 1. } else { 0. });
        bar.set_can_target(mouse);
    }
}

unsafe extern "C" fn scrollbar_input_before_dispatch(
    _: *mut glib::gobject_ffi::GSignalInvocationHint,
    count: u32,
    values: *const glib::gobject_ffi::GValue,
    _: glib::ffi::gpointer,
) -> glib::ffi::gboolean {
    use glib::translate::from_glib_borrow;
    if count < 2 { return 1; }
    let surface: glib::translate::Borrowed<gdk::Surface> = unsafe {
        from_glib_borrow(glib::gobject_ffi::g_value_get_object(values) as *mut gdk::ffi::GdkSurface)
    };
    let event: glib::translate::Borrowed<gdk::Event> = unsafe {
        from_glib_borrow(glib::gobject_ffi::g_value_get_pointer(values.add(1)) as *mut gdk::ffi::GdkEvent)
    };
    if !matches!(event.event_type(), gdk::EventType::MotionNotify | gdk::EventType::ButtonPress
        | gdk::EventType::TouchBegin | gdk::EventType::ProximityIn | gdk::EventType::Scroll) { return 1; }
    let Some(root) = gtk::Native::for_surface(&*surface).and_then(|native| native.upcast::<gtk::Widget>().root()) else { return 1; };
    let root = root.upcast::<gtk::Widget>();
    let mouse = pointer_kind(&event) == PointerKind::Mouse;
    if root.has_css_class("mouse-scrollbars") == mouse { return 1; }
    if mouse { root.add_css_class("mouse-scrollbars"); } else { root.remove_css_class("mouse-scrollbars"); }
    crate::text_language::visit(&root, &mut |widget| {
        if widget.has_css_class("capy-scroller") && let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>() {
            scrollbar_input(scroll, mouse);
        }
    });
    1
}

pub(crate) fn swipe_control(widget: &gtk::Widget) -> bool {
    let controllers = widget.observe_controllers();
    widget.has_css_class("workspace-reorder-handle")
        || widget.has_css_class("document-tab")
        || widget.has_css_class("panel-grip")
        || widget.is::<gtk::Range>() || widget.is::<gtk::Editable>()
        || widget.is::<crate::number_control::NumberControl>()
        || !widget.is::<gtk::ScrolledWindow>() && !widget.is::<crate::swipe_row::SwipeRow>()
            && (0..controllers.n_items()).filter_map(|i| controllers.item(i)).any(|c| {
                c.is::<gtk::GestureDrag>() || c.is::<gtk::GestureSwipe>() || c.is::<gtk::GestureStylus>()
            })
}

/// GTK's built-in kinetic scrolling only accepts touchscreen sequences. Give
/// tablet contacts the same pre-hold scrolling path, using native slop and
/// gesture arbitration so held rows and explicit handles keep their capture.
pub(crate) fn pen_scroller(scroll: gtk::ScrolledWindow) -> gtk::ScrolledWindow {
    static SCROLLBAR_INPUT: std::sync::Once = std::sync::Once::new();
    SCROLLBAR_INPUT.call_once(|| unsafe {
        let class = glib::gobject_ffi::g_type_class_ref(gdk::Surface::static_type().into_glib());
        let signal = glib::subclass::SignalId::lookup("event", gdk::Surface::static_type()).unwrap();
        glib::gobject_ffi::g_signal_add_emission_hook(signal.into_glib(), 0,
            Some(scrollbar_input_before_dispatch), std::ptr::null_mut(), None);
        glib::gobject_ffi::g_type_class_unref(class);
    });
    scroll.add_css_class("capy-scroller");
    scrollbar_input(&scroll, false);
    scroll.connect_map(|scroll| {
        let mouse = scroll.root().is_some_and(|root| root.upcast::<gtk::Widget>().has_css_class("mouse-scrollbars"));
        scrollbar_input(scroll, mouse);
    });
    let drag = gtk::GestureDrag::new();
    drag.set_button(1);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let origin = Rc::new(Cell::new([0.; 2]));
    drag.connect_drag_begin(glib::clone!(#[weak] scroll, #[strong] origin, move |g, x, y| {
        let pen = g.current_event().is_some_and(|e| e.device_tool().is_some()
            || e.device().is_some_and(|d| d.source() == gdk::InputSource::Pen));
        let mut target = scroll.pick(x, y, gtk::PickFlags::DEFAULT);
        let mut direct = false;
        while let Some(widget) = target {
            if widget == scroll { break; }
            direct |= widget.has_css_class("drag-immediate")
                || widget.has_css_class("workspace-reorder-handle")
                || widget.has_css_class("document-tab")
                || widget.is::<gtk::Range>() || widget.is::<gtk::Editable>()
                || widget.is::<gtk::DrawingArea>() || widget.is::<crate::number_control::NumberControl>();
            if let Some(inner) = widget.downcast_ref::<gtk::ScrolledWindow>() {
                direct |= [inner.hadjustment(), inner.vadjustment()].iter()
                    .any(|a| a.upper() - a.lower() > a.page_size());
            }
            target = widget.parent();
        }
        if !pen || direct { g.set_state(gtk::EventSequenceState::Denied); return; }
        origin.set([scroll.hadjustment().value(), scroll.vadjustment().value()]);
    }));
    drag.connect_drag_update(glib::clone!(#[weak] scroll, #[strong] origin, move |g, dx, dy| {
        if !scroll.drag_check_threshold(0, 0, dx as i32, dy as i32) { return; }
        let (axis, delta, adjustment) = if dx.abs() > dy.abs() {
            (0, dx, scroll.hadjustment())
        } else { (1, dy, scroll.vadjustment()) };
        if adjustment.upper() - adjustment.lower() <= adjustment.page_size() {
            g.set_state(gtk::EventSequenceState::Denied);
            return;
        }
        g.set_state(gtk::EventSequenceState::Claimed);
        adjustment.set_value((origin.get()[axis] - delta)
            .clamp(adjustment.lower(), (adjustment.upper() - adjustment.page_size()).max(adjustment.lower())));
    }));
    scroll.add_controller(drag);
    scroll
}

const TOUCH_DEVICES: u64 = 1 << 40;

pub fn observe_calibration(workspace:&Rc<Workspace>,widget:&impl IsA<gtk::Widget>) {
    let observer=gtk::EventControllerLegacy::new();observer.set_propagation_phase(gtk::PropagationPhase::Capture);
    observer.connect_event(glib::clone!(#[weak] workspace,#[upgrade_or] glib::Propagation::Proceed,move |_,event| {
        if event.device_tool().is_none() {return glib::Propagation::Proceed;}
        let phase=match event.event_type() {
            gdk::EventType::ProximityIn=>PenPhase::Hover,
            gdk::EventType::ProximityOut=>PenPhase::Cancel,
            gdk::EventType::MotionNotify=>if event.modifier_state().contains(gdk::ModifierType::BUTTON1_MASK) {PenPhase::Move}else{PenPhase::Hover},
            gdk::EventType::ButtonPress|gdk::EventType::ButtonRelease
                if event.downcast_ref::<gdk::ButtonEvent>().is_some_and(|e|e.button()==1)=>{
                    if event.event_type()==gdk::EventType::ButtonPress {PenPhase::Down}else{PenPhase::Up}
                },
            _=>return glib::Propagation::Proceed,
        };
        let (x,y)=event.position().unwrap_or_default();
        if let Some(sample)=workspace.input.sample(&workspace,Some(event),phase,x,y) {workspace.calibration_input(sample);}
        glib::Propagation::Proceed
    }));
    widget.add_controller(observer);
}

#[derive(Default)]
pub struct Input {
    sequence: Cell<u64>,
    last: Cell<Option<PenEvent>>,
    touches: RefCell<HashMap<gdk::EventSequence, u64>>,
    touch_points: RefCell<HashMap<u64, [f32; 2]>>,
    next_touch: Cell<u64>,
    touchpad: Cell<Option<([f32; 2], f32)>>,
    clock: Cell<Option<(u32, u64)>>,
    tablets: tablet::TabletDevices,
    picker_hold: Rc<crate::color_picker::Hold>,
}

pub fn install(workspace: &Rc<Workspace>) {
    let input = workspace.input.clone();
    // GestureSingle resets an active sequence on a different button, and
    // GestureStylus emits down/up even for that rejected event. Stop tablet
    // side buttons before either painting or mouse-pan gestures can see them.
    let pen_buttons = gtk::EventControllerLegacy::new();
    pen_buttons.set_propagation_phase(gtk::PropagationPhase::Capture);
    pen_buttons.connect_event(glib::clone!(
        #[weak]
        workspace,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, event| {
            let Some(button) = event.downcast_ref::<gdk::ButtonEvent>().map(|e| e.button()).filter(|b| *b != 1)
            else {
                return glib::Propagation::Proceed;
            };
            if event.device_tool().is_none()
                && !event.device().is_some_and(|d| d.source() == gdk::InputSource::Pen)
            {
                return glib::Propagation::Proceed;
            }
            let pen_button = match button {
                2 => Some(PenButton::Primary),
                3 => Some(PenButton::Secondary),
                8 => Some(PenButton::Tertiary),
                _ => None,
            };
            if let Some(button) = pen_button {
                workspace.interact(UiInput::PenButton {
                    button,
                    pressed: event.event_type() == gdk::EventType::ButtonPress,
                });
            }
            glib::Propagation::Stop
        }
    ));
    workspace.area.add_controller(pen_buttons);
    let stylus = gtk::GestureStylus::new();
    stylus.set_stylus_only(false); // GTK provides one path for pen and mouse.
    stylus.set_button(1);
    stylus.connect_proximity(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        move |g, x, y| input.stylus(&workspace, g, PenPhase::Hover, x, y)
    ));
    stylus.connect_down(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        move |g, x, y| {
            if workspace.chrome_contact_at(x as f32, y as f32) {
                g.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            workspace.area.grab_focus();
            input.stylus(&workspace, g, PenPhase::Down, x, y);
            g.set_state(gtk::EventSequenceState::Claimed);
        }
    ));
    stylus.connect_motion(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        move |g, x, y| {
            input.stylus(&workspace, g, PenPhase::Move, x, y);
        }
    ));
    stylus.connect_up(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        move |g, x, y| {
            input.stylus(&workspace, g, PenPhase::Up, x, y);
        }
    ));
    stylus.connect_cancel(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        move |_, _| input.cancel(&workspace)
    ));
    workspace.area.add_controller(stylus);
    let hover = gtk::EventControllerMotion::new();
    hover.connect_motion(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        move |controller, x, y| {
            if controller
                .current_event()
                .is_some_and(|e| e.device_tool().is_some())
            {
                return;
            }
            let dpi = workspace.area.scale_factor() as f32;
            let event = PenEvent {
                device_id: 1,
                sequence: 0,
                timestamp_ns: input.timestamp(controller.current_event_time()),
                view_revision: 0,
                surface_position: Point {
                    x: x as f32 * dpi,
                    y: y as f32 * dpi,
                },
                pressure: 1.0,
                tilt_radians: [0.0; 2],
                twist_radians: 0.0,
                distance: 0.0,
                phase: PenPhase::Hover,
                tool: ToolKind::Mouse,
                flags: SampleFlags::PRIMARY,
            };
            workspace.cursor_input(Some(event));
        }
    ));
    hover.connect_leave(glib::clone!(
        #[weak]
        workspace,
        move |_| workspace.cursor_input(None)
    ));
    workspace.area.add_controller(hover);

    // Touch is consumed before mouse emulation; stable native sequences feed
    // the shared touch logic, which decides whether a finger drives the canvas.
    let touch = gtk::EventControllerLegacy::new();
    touch.set_propagation_phase(gtk::PropagationPhase::Capture);
    touch.connect_event(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, event| {
            if event.event_type() == gdk::EventType::TouchpadPinch
                && let Some(pinch) = event.downcast_ref::<gdk::TouchpadEvent>()
            {
                let dpi = workspace.area.scale_factor() as f32;
                match pinch.gesture_phase() {
                    gdk::TouchpadGesturePhase::Begin => {
                        let point = event.position().and_then(|(x, y)| widget_point(&workspace.area, x, y));
                        input.touchpad.set(point.map(|p| ([p.x() * dpi, p.y() * dpi], 1.)));
                        if let Some(g) = workspace.gpu.borrow_mut().as_mut() { g.session.begin_view_gesture(); }
                    }
                    gdk::TouchpadGesturePhase::Update => if let Some((from, previous)) = input.touchpad.get() {
                        let (dx, dy) = pinch.deltas();
                        let to = [from[0] + dx as f32 * dpi, from[1] + dy as f32 * dpi];
                        let scale = pinch.pinch_scale() as f32;
                        if scale.is_finite() && scale > 0. {
                            let result = workspace.gpu.borrow_mut().as_mut().map(|g| g.session.multi_touch_gesture(from, to, scale / previous, pinch.pinch_angle_delta() as f32));
                            input.touchpad.set(Some((to, scale)));
                            if let Some(result) = result { workspace.changed(result); }
                        }
                    },
                    _ => input.touchpad.set(None),
                }
                return glib::Propagation::Stop;
            }
            let phase = match event.event_type() {
                gdk::EventType::TouchBegin => PenPhase::Down,
                gdk::EventType::TouchUpdate => PenPhase::Move,
                gdk::EventType::TouchEnd => PenPhase::Up,
                gdk::EventType::TouchCancel => PenPhase::Cancel,
                _ => return glib::Propagation::Proceed,
            };
            let sequence = event.event_sequence();
            let known = input.touches.borrow().get(&sequence).copied();
            let id = match (phase, known) {
                (_, Some(id)) => id,
                (PenPhase::Down, None) => {
                    let id = input.next_touch.get() + 1;
                    input.next_touch.set(id);
                    input.touches.borrow_mut().insert(sequence.clone(), id);
                    id
                }
                // A reset/cancelled sequence cannot acquire a new identity
                // from a trailing update or release.
                _ => return glib::Propagation::Stop,
            };
            let located = event
                .position()
                .and_then(|(x, y)| widget_point(&workspace.area, x, y))
                .map(|p| [p.x(), p.y()]);
            let position =
                located.or_else(|| matches!(phase, PenPhase::Up | PenPhase::Cancel).then_some([0.0; 2]));
            if let Some(position) = position {
                if phase == PenPhase::Down && workspace.chrome_contact_at(position[0], position[1]) {
                    return glib::Propagation::Stop;
                }
                input.picker_hold.input(&workspace, id, contact_phase(phase), position, input.touches.borrow().len());
                let scale = workspace.area.scale_factor() as f32;
                if phase == PenPhase::Down {
                    let settings = gtk::Settings::for_display(&workspace.area.display());
                    workspace.set_touch_policy(TouchPolicy {
                        tap_ms: settings.gtk_long_press_time().max(1),
                        slop: settings.gtk_dnd_drag_threshold().max(1) as f32 * scale,
                    });
                }
                let time_ns = input.timestamp(event.time());
                let reply = workspace.interact(UiInput::Pointer {
                    id,
                    phase: contact_phase(phase),
                    kind: PointerKind::Touch,
                    button: PointerButton::Primary,
                    position: position.map(|v| v * scale),
                    time_ns,
                });
                if reply.paint {
                    input.touch_pen(&workspace, id, phase, located, time_ns);
                } else {
                    input.touch_points.borrow_mut().remove(&id);
                }
            }
            if matches!(phase, PenPhase::Up | PenPhase::Cancel) {
                input.touches.borrow_mut().remove(&sequence);
            }
            glib::Propagation::Stop
        }
    ));
    workspace.area.add_controller(touch);

    for button in [2, 3] {
        let pan = gtk::GestureDrag::new();
        pan.set_button(button);
        pan.connect_drag_begin(glib::clone!(
            #[weak]
            workspace,
            move |g, x, y| {
                if workspace.chrome_contact_at(x as f32, y as f32) {
                    g.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                workspace.area.grab_focus();
                pan_event(&workspace, button, ContactPhase::Down, [x as f32, y as f32]);
                g.set_state(gtk::EventSequenceState::Claimed);
            }
        ));
        pan.connect_drag_update(glib::clone!(
            #[weak]
            workspace,
            move |g, x, y| {
                let Some((sx, sy)) = g.start_point() else {
                    return;
                };
                let to = [(sx + x) as f32, (sy + y) as f32];
                pan_event(&workspace, button, ContactPhase::Move, to);
            }
        ));
        pan.connect_drag_end(glib::clone!(
            #[weak]
            workspace,
            move |g, x, y| {
                if let Some((sx, sy)) = g.start_point() {
                    pan_event(
                        &workspace,
                        button,
                        ContactPhase::Up,
                        [(sx + x) as f32, (sy + y) as f32],
                    );
                }
            }
        ));
        pan.connect_cancel(glib::clone!(
            #[weak]
            workspace,
            move |_, _| pan_event(&workspace, button, ContactPhase::Cancel, [0.0; 2])
        ));
        workspace.area.add_controller(pan);
    }
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    scroll.connect_scroll(glib::clone!(
        #[weak]
        workspace,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |controller, dx, dy| {
            let pointer = controller
                .current_event()
                .and_then(|e| e.position())
                .map(|(x, y)| (x, y, controller.current_event_state()))
                .or_else(|| {
                    let native = workspace.area.native()?;
                    let device = native.display().default_seat()?.pointer()?;
                    native.surface()?.device_position(&device)
                });
            if let Some((x, y, modifiers)) = pointer
                && let Some(point) = widget_point(&workspace.area, x, y)
            {
                let dpi = workspace.area.scale_factor() as f32;
                let unit = if controller.unit() == gdk::ScrollUnit::Wheel {
                    40.0
                } else {
                    1.0
                };
                let result = workspace.gpu.borrow_mut().as_mut().map(|g| {
                    g.session.scroll(
                        [point.x() * dpi, point.y() * dpi],
                        [dx as f32 * unit, dy as f32 * unit],
                        dpi,
                        modifiers.contains(gdk::ModifierType::CONTROL_MASK),
                        modifiers.contains(gdk::ModifierType::SHIFT_MASK),
                    )
                });
                if let Some(result) = result {
                    workspace.changed(result);
                }
            }
            glib::Propagation::Stop
        }
    ));
    workspace.area.add_controller(scroll);
    // Native popups can deactivate the toplevel while focus remains in its
    // widget tree. Only leaving both the window and its descendants is blur.
    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        move |focus| input.window_focus_changed(&workspace, focus)
    ));
    workspace.window.add_controller(focus.clone());
    workspace.window.connect_is_active_notify(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        #[strong]
        focus,
        move |window| {
            if !window.is_active() {
                input.window_focus_changed(&workspace, &focus);
            }
        }
    ));
    workspace.area.connect_unmap(glib::clone!(
        #[weak]
        workspace,
        move |_| input.cancel(&workspace)
    ));
}

fn pan_event(workspace: &Rc<Workspace>, button: u32, phase: ContactPhase, position: [f32; 2]) {
    let dpi = workspace.area.scale_factor() as f32;
    workspace.interact(UiInput::Pointer {
        id: u64::MAX - button as u64,
        phase,
        kind: PointerKind::Mouse,
        button: PointerButton::Pan,
        position: position.map(|v| v * dpi),
        time_ns: 0,
    });
}

pub fn key_input(
    key: gdk::Key,
    keycode: u32,
    pressed: bool,
    modifiers: gdk::ModifierType,
    editing: bool,
    divider: Option<u32>,
) -> UiInput {
    let latin = gdk::Display::default().and_then(|display| display.map_keycode(keycode))
        .and_then(|entries| latin_key(key, &entries).to_unicode())
        .filter(char::is_ascii_graphic).map(|c| c.to_string());
    let key = match key {
        gdk::Key::Tab | gdk::Key::ISO_Left_Tab => "Tab".into(),
        gdk::Key::Escape => "Escape".into(),
        gdk::Key::Left => "ArrowLeft".into(),
        gdk::Key::Right => "ArrowRight".into(),
        gdk::Key::Up => "ArrowUp".into(),
        gdk::Key::Down => "ArrowDown".into(),
        gdk::Key::BackSpace => "Backspace".into(),
        gdk::Key::Return | gdk::Key::KP_Enter => "Enter".into(),
        gdk::Key::Delete => "Delete".into(),
        gdk::Key::Page_Up => "PageUp".into(),
        gdk::Key::Page_Down => "PageDown".into(),
        _ => key
            .to_unicode()
            .filter(|c| !c.is_control())
            .map(|c| c.to_string())
            .or_else(|| key.name().map(|n| n.to_string()))
            .unwrap_or_default(),
    };
    UiInput::Key {
        latin: latin.filter(|latin| *latin != key),
        key,
        pressed,
        repeat: false,
        editing,
        divider,
        modifiers: Modifiers {
            command: modifiers
                .intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::META_MASK),
            shift: modifiers.contains(gdk::ModifierType::SHIFT_MASK),
            alt: modifiers.contains(gdk::ModifierType::ALT_MASK),
        },
    }
}

fn latin_key(key: gdk::Key, entries: &[(gdk::KeymapKey, gdk::Key)]) -> gdk::Key {
    let level = entries.iter().find(|(_, k)| *k == key).map_or(0, |(entry, _)| entry.level());
    entries
        .iter()
        .filter(|(entry, k)| entry.level() == level && k.to_unicode().is_some_and(|c| c.is_ascii_graphic()))
        .min_by_key(|(entry, _)| entry.group())
        .map_or(key, |&(_, k)| k)
}

pub(crate) fn control_takes_key(input: &UiInput, keyboard_focus: bool) -> bool {
    matches!(input, UiInput::Key { key, modifiers, .. } if layer_ui::control_takes_key(key, *modifiers, keyboard_focus))
}

/// Whether the focused widget was reached with the keyboard rather than a pointer.
#[derive(Default)]
pub(crate) struct KeyboardFocus {
    modality: Cell<bool>,
    focused: Cell<bool>,
}
impl KeyboardFocus {
    pub(crate) fn focused(&self) -> bool {
        self.focused.get()
    }
    pub(crate) fn key_pressed(&self, controller: &gtk::EventControllerKey) {
        if controller.current_event().and_then(|event| event.downcast::<gdk::KeyEvent>().ok()).is_some_and(|key| !key.is_modifier()) {
            self.modality.set(true);
        }
    }
}
pub(crate) fn keyboard_focus(window: &adw::ApplicationWindow) -> Rc<KeyboardFocus> {
    let focus = Rc::new(KeyboardFocus::default());
    let input = gtk::EventControllerLegacy::new();
    input.set_propagation_phase(gtk::PropagationPhase::Capture);
    input.connect_event(glib::clone!(
        #[strong]
        focus,
        move |_, event| {
            if matches!(event.event_type(), gdk::EventType::ButtonPress | gdk::EventType::TouchBegin) {
                focus.modality.set(false);
                focus.focused.set(false);
            }
            glib::Propagation::Proceed
        }
    ));
    window.add_controller(input);
    window.connect_focus_widget_notify(glib::clone!(
        #[strong]
        focus,
        move |_| focus.focused.set(focus.modality.get())
    ));
    focus
}

/// Tablet pad buttons the compositor leaves to the app arrive as keys.
pub fn pad_buttons(workspace: &Rc<Workspace>) -> gtk::EventControllerLegacy {
    let pads = gtk::EventControllerLegacy::new();
    pads.set_propagation_phase(gtk::PropagationPhase::Capture);
    pads.connect_event(glib::clone!(
        #[weak]
        workspace,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, event| {
            let pressed = match event.event_type() {
                gdk::EventType::PadButtonPress => true,
                gdk::EventType::PadButtonRelease => false,
                _ => return glib::Propagation::Proceed,
            };
            let Some(pad) = event.downcast_ref::<gdk::PadEvent>() else {
                return glib::Propagation::Proceed;
            };
            let UiInput::Key { modifiers, .. } = key_input(gdk::Key::VoidSymbol, 0, pressed, event.modifier_state(), false, None) else {
                return glib::Propagation::Proceed;
            };
            workspace.interact(UiInput::Key {
                latin: None,
                key: format!("pad_button_{}", pad.button() + 1),
                pressed,
                repeat: false,
                editing: false,
                divider: None,
                modifiers,
            });
            glib::Propagation::Stop
        }
    ));
    pads
}

fn contact_phase(phase: PenPhase) -> ContactPhase {
    match phase {
        PenPhase::Down => ContactPhase::Down,
        PenPhase::Up => ContactPhase::Up,
        PenPhase::Cancel => ContactPhase::Cancel,
        _ => ContactPhase::Move,
    }
}

/// Convert a raw GDK surface position to widget coordinates. GTK's native
/// transform places the widget inside the surface (including CSD shadows),
/// so input must subtract it. Maximized/fullscreen windows hide a wrong sign
/// because their decoration offset is zero.
pub(crate) fn widget_point(
    widget: &impl IsA<gtk::Widget>,
    x: f64,
    y: f64,
) -> Option<gtk::graphene::Point> {
    let native = widget.native()?;
    let (dx, dy) = native.surface_transform();
    native.dynamic_cast::<gtk::Widget>().ok()?.compute_point(
        widget,
        &gtk::graphene::Point::new((x - dx) as f32, (y - dy) as f32),
    )
}

impl Input {
    fn timestamp(&self, ms: u32) -> u64 {
        self.timestamp_at(ms, glib::monotonic_time().max(0) as u64 * 1000)
    }
    fn timestamp_at(&self, ms: u32, received_ns: u64) -> u64 {
        let (previous, previous_ns) = self.clock.get().unwrap_or((ms, received_ns));
        let delta = ms.wrapping_sub(previous) as i32 as i64 * 1_000_000;
        let mapped = previous_ns.saturating_add_signed(delta);
        if delta >= 0 {
            // Wayland's timestamp origin is unspecified. Receipt is an upper
            // bound on sample time, so refine the offset with the minimum
            // observed delivery delay instead of freezing the first latency.
            // New receipts are monotonic: this can shorten a calibration-time
            // interval, but cannot put a new sample before the preceding one.
            let ns = mapped.min(received_ns);
            self.clock.set(Some((ms, ns)));
            ns
        } else {
            // Older history uses the current batch's mapping and does not train
            // clock alignment from its deliberately delayed delivery.
            mapped
        }
    }
    fn sample(
        &self,
        workspace: &Rc<Workspace>,
        native: Option<&gdk::Event>,
        phase: PenPhase,
        x: f64,
        y: f64,
    ) -> Option<PenEvent> {
        let revision = workspace
            .gpu
            .borrow()
            .as_ref()
            .map(|g| g.session.state().camera.revision);
        let view_revision=revision?;
        let tool = native.and_then(|event|event.device_tool());
        let tool_kind = if tool
            .as_ref()
            .is_some_and(|t| t.tool_type() == gdk::DeviceToolType::Eraser)
        {
            ToolKind::Eraser
        } else if tool.is_some() {
            ToolKind::Pen
        } else {
            ToolKind::Mouse
        };
        let timestamp_ns = self.timestamp(native.map_or(0,|event|event.time()));
        let tablet_flags = native.and_then(|event| event.device())
            .map_or(SampleFlags::NONE, |device| self.tablets.flags(&device, tool.as_ref()));
        let pen=native.and_then(|event|event.device()).zip(tool.as_ref())
            .map(|(device,tool)|self.tablets.pen(&device,tool));
        if let Some(pen)=&pen
            && let Some(gpu)=workspace.gpu.borrow_mut().as_mut(){gpu.session.register_pen_device(pen);}
        let dpi = workspace.area.scale_factor() as f32;
        let axis = |a| native.and_then(|event|event.axis(a)).unwrap_or(0.0) as f32;
        Some(PenEvent {
            device_id: pen.as_ref().map_or(1,|pen|pen.id),
            sequence: 0,
            timestamp_ns,
            view_revision,
            surface_position: Point {
                x: x as f32 * dpi,
                y: y as f32 * dpi,
            },
            pressure: if tool_kind == ToolKind::Mouse {
                1.0
            } else {
                axis(gdk::AxisUse::Pressure)
            },
            // GDK tilt is normalized [-1,1], rotation is in degrees.
            tilt_radians: [
                axis(gdk::AxisUse::Xtilt) * std::f32::consts::FRAC_PI_2,
                axis(gdk::AxisUse::Ytilt) * std::f32::consts::FRAC_PI_2,
            ],
            twist_radians: axis(gdk::AxisUse::Rotation).to_radians(),
            distance: axis(gdk::AxisUse::Distance),
            phase,
            tool: tool_kind,
            flags: SampleFlags(SampleFlags::PRIMARY.0 | tablet_flags.0),
        })
    }
    fn stylus(&self,workspace:&Rc<Workspace>,gesture:&gtk::GestureStylus,phase:PenPhase,x:f64,y:f64) {
        let native=gesture.current_event();
        let Some(event)=self.sample(workspace,native.as_ref(),phase,x,y) else{return;};
        let dpi=workspace.area.scale_factor() as f32;
        let defaults=if event.is_finite() {event} else {self.last.get().filter(|p|p.device_id==event.device_id).unwrap_or(event)};
        let history=if phase==PenPhase::Move {gesture.backlog().unwrap_or_default()} else {Vec::new()};
        let decode=|point:&gdk::TimeCoord| {
                let flags = point.flags();
                if !flags.contains(gdk::AxisFlags::X | gdk::AxisFlags::Y) {
                    return None;
                }
                let get = |a: gdk::AxisUse| point.axes()[a.into_glib() as usize] as f32;
                let history = PenEvent {
                    timestamp_ns: self.timestamp(point.time()),
                    surface_position: Point {
                        x: get(gdk::AxisUse::X) * dpi,
                        y: get(gdk::AxisUse::Y) * dpi,
                    },
                    pressure: if flags.contains(gdk::AxisFlags::PRESSURE) {
                        get(gdk::AxisUse::Pressure)
                    } else {
                        defaults.pressure
                    },
                    tilt_radians: [
                        if flags.contains(gdk::AxisFlags::XTILT) {
                            get(gdk::AxisUse::Xtilt) * std::f32::consts::FRAC_PI_2
                        } else {
                            defaults.tilt_radians[0]
                        },
                        if flags.contains(gdk::AxisFlags::YTILT) {
                            get(gdk::AxisUse::Ytilt) * std::f32::consts::FRAC_PI_2
                        } else {
                            defaults.tilt_radians[1]
                        },
                    ],
                    twist_radians:defaults.twist_radians,
                    distance:defaults.distance,
                    ..event
                };
                Some(history)
            };
        let routing=if phase==PenPhase::Move && !event.is_finite() {
            let Some(last)=history.iter().rev().filter_map(decode).find(|p|p.is_finite()
                && self.last.get().is_none_or(|last|p.timestamp_ns>=last.timestamp_ns)) else {return;};
            last
        } else {event};
        let routing={
            let mut gpu=workspace.gpu.borrow_mut();
            let Some(routing)=gpu.as_mut().and_then(|gpu|gpu.session.filter_pen(routing)) else {return;};
            routing
        };
        workspace.cursor_input(Some(routing));
        if routing.phase==PenPhase::Hover {return;}
        let reply=workspace.interact(UiInput::Pointer {
            id:routing.device_id,phase:contact_phase(routing.phase),
            kind:if routing.tool==ToolKind::Mouse {PointerKind::Mouse} else {PointerKind::Pen},
            button:PointerButton::Primary,position:[routing.surface_position.x,routing.surface_position.y],time_ns:routing.timestamp_ns,
        });
        #[cfg(test)]
        if let Some(gpu)=workspace.gpu.borrow().as_ref() {
            gpu.session.engine().backend().stats.lock().unwrap().pen_routes.push((event.timestamp_ns,
                format!("{:?}",routing.phase),if reply.paint {"paint"} else {"interaction"}));
        }
        if !reply.paint {return;}
        for history in history.iter().filter_map(decode) {
                if self
                    .last
                    .get()
                    .is_none_or(|last| history.timestamp_ns >= last.timestamp_ns)
                {
                    self.send(workspace, history);
                }
        }
        if event.is_finite() {self.send(workspace,event);}
        else if matches!(routing.phase,PenPhase::Up|PenPhase::Cancel) {self.send(workspace,routing);}
    }
    fn touch_pen(&self, workspace: &Rc<Workspace>, id: u64, phase: PenPhase, position: Option<[f32; 2]>, timestamp_ns: u64) {
        let mut points = self.touch_points.borrow_mut();
        let Some(position) = position.or_else(|| points.get(&id).copied()) else {
            return;
        };
        if matches!(phase, PenPhase::Up | PenPhase::Cancel) {
            points.remove(&id);
        } else {
            points.insert(id, position);
        }
        drop(points);
        let Some(view_revision) = workspace.gpu.borrow().as_ref().map(|g| g.session.state().camera.revision) else {
            return;
        };
        let dpi = workspace.area.scale_factor() as f32;
        self.send(workspace, PenEvent {
            device_id: TOUCH_DEVICES | id,
            sequence: 0,
            timestamp_ns,
            view_revision,
            surface_position: Point { x: position[0] * dpi, y: position[1] * dpi },
            pressure: 1.0,
            tilt_radians: [0.0; 2],
            twist_radians: 0.0,
            distance: 0.0,
            phase,
            tool: ToolKind::Finger,
            flags: SampleFlags::PRIMARY,
        });
    }
    pub(crate) fn send(&self, workspace: &Rc<Workspace>, event: PenEvent) {
        if (workspace.documents.changing.get() || !workspace.workspaces.accepts_input(workspace))
            && !matches!(event.phase, PenPhase::Up | PenPhase::Cancel)
        {
            return;
        }
        self.deliver(workspace, event);
    }
    fn deliver(&self, workspace: &Rc<Workspace>, mut event: PenEvent) {
        {
            let mut gpu=workspace.gpu.borrow_mut();
            let Some(filtered)=gpu.as_mut().and_then(|gpu|gpu.session.filter_pen(event)) else {return;};
            event=filtered;
        }
        #[cfg(test)]
        let delivered_ns = glib::monotonic_time() as u64 * 1000;
        #[cfg(test)]
        if let Some(gpu) = workspace.gpu.borrow().as_ref() {
            gpu.session.engine().backend().stats.lock().unwrap().pen_routes.push((
                event.timestamp_ns, format!("{:?}", event.phase), "send"));
        }
        event.sequence = self.sequence.get() + 1;
        self.sequence.set(event.sequence);
        self.last
            .set(if matches!(event.phase, PenPhase::Up | PenPhase::Cancel) {
                None
            } else {
                Some(event)
            });
        {
            let mut gpu = workspace.gpu.borrow_mut();
            let Some(gpu) = gpu.as_mut() else {
                return;
            };
            gpu.session.pen(event);
            #[cfg(test)]
            if event.flags.contains(SampleFlags::PRIMARY)
                && event.phase == PenPhase::Move
                && gpu.session.state().layer_tools.tool == layer_ui::LayerCanvasTool::Transform
            {
                let doc = gpu.session.engine().document();
                if let Some(handle) = doc.working.occurrence
                    && doc.scene().paint_source(handle).is_some_and(|source| source.base.is_some())
                    && let Some(occurrence) = doc.scene().occurrence(handle) {
                    gpu.session.engine().backend().stats.lock().unwrap().photo_inputs.push((
                        delivered_ns, layer_ui::occurrence_token(handle),
                        [event.surface_position.x, event.surface_position.y], occurrence.offset,
                    ));
                }
            }
        }
        if matches!(event.phase, PenPhase::Up | PenPhase::Cancel) {
            workspace.wake_stroke_end();
        } else {
            workspace.wake();
        }
    }
    pub fn discard(&self) {
        self.last.set(None);
    }
    fn window_focus_changed(
        self: &Rc<Self>,
        workspace: &Rc<Workspace>,
        focus: &gtk::EventControllerFocus,
    ) {
        // Let GTK finish moving focus between native surfaces before deciding.
        glib::idle_add_local_once(glib::clone!(
            #[weak]
            workspace,
            #[weak(rename_to = input)]
            self,
            #[weak]
            focus,
            move || {
                if !workspace.window.is_active() && !focus.contains_focus() {
                    input.cancel(&workspace);
                    workspace.cursor_input(None);
                }
            }
        ));
    }
    pub(crate) fn cancel_touch(&self, workspace: &Rc<Workspace>, sequence: &gdk::EventSequence, time: u32) {
        let Some(id) = self.touches.borrow_mut().remove(sequence) else { return };
        self.picker_hold.input(workspace, id, ContactPhase::Cancel, [0.; 2], self.touches.borrow().len());
        workspace.interact(UiInput::Pointer {
            id,
            phase: ContactPhase::Cancel,
            kind: PointerKind::Touch,
            button: PointerButton::Primary,
            position: [0.; 2],
            time_ns: self.timestamp(time),
        });
        self.touch_points.borrow_mut().remove(&id);
    }
    fn cancel(&self, workspace: &Rc<Workspace>) {
        self.picker_hold.cancel();
        let reply = workspace.interact(UiInput::Blur);
        if reply.cancel_paint
            && let Some(event) = self.last.get()
        {
            self.send(
                workspace,
                PenEvent {
                    phase: PenPhase::Cancel,
                    ..event
                },
            );
        }
        self.touches.borrow_mut().clear();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn composition_keys_retain_same_press_until_release_or_retirement() {
        let keys = super::CompositionKeys::default();
        assert!(keys.capture(36, true));
        assert!(keys.active());
        assert!(keys.capture(36, false));
        assert!(!keys.capture(9, false));
        assert!(!keys.release(9));
        assert!(keys.capture(36, false));
        assert!(keys.release(36));
        assert!(!keys.capture(36, false));
        assert!(!keys.active());
        assert!(keys.capture(9, true));
        keys.clear();
        assert!(!keys.active());
        assert!(!keys.capture(9, false));
    }

    use super::*;
    #[test]
    fn shortcuts_use_the_latin_key_at_the_same_position_and_level() {
        let z = |group, level, key| (gdk::KeymapKey::new(52, group, level), key);
        let russian_first = [z(0, 0, gdk::Key::Cyrillic_ya), z(0, 1, gdk::Key::Cyrillic_YA), z(1, 0, gdk::Key::z), z(1, 1, gdk::Key::Z)];
        assert_eq!(latin_key(gdk::Key::Cyrillic_ya, &russian_first), gdk::Key::z);
        assert_eq!(latin_key(gdk::Key::Cyrillic_YA, &russian_first), gdk::Key::Z);
        let english_first = [z(0, 0, gdk::Key::z), z(0, 1, gdk::Key::Z), z(1, 0, gdk::Key::Cyrillic_ya), z(1, 1, gdk::Key::Cyrillic_YA)];
        assert_eq!(latin_key(gdk::Key::Cyrillic_ya, &english_first), gdk::Key::z);
        let greek_only = [(gdk::KeymapKey::new(38, 0, 0), gdk::Key::Greek_alpha)];
        assert_eq!(latin_key(gdk::Key::Greek_alpha, &greek_only), gdk::Key::Greek_alpha);
    }

    #[test]
    fn native_clock_removes_initial_delivery_latency_without_reordering() {
        for hz in [60u64, 120, 240, 480, 1000] {
            for first_delay in [1_000_000u64, 8_625_000, 40_000_000] {
                let input = Input::default();
                let origin_ms = u32::MAX - 15;
                let origin_ns = 100_000_000_000;
                let mut last = 0;
                let mut last_receipt = origin_ns;
                for i in 0..100u64 {
                    let elapsed_ms = i * 1000 / hz;
                    let ms = origin_ms.wrapping_add(elapsed_ms as u32);
                    let ideal = origin_ns + elapsed_ms * 1_000_000;
                    // A slow first callback, subsequent batching, then a quiet
                    // low-latency stream; this includes the real +8.625 ms case.
                    let delay = if i == 0 {
                        first_delay
                    } else if i % 17 < 3 {
                        6_000_000
                    } else {
                        300_000
                    };
                    let received = (ideal + delay).max(last_receipt);
                    let mapped = input.timestamp_at(ms, received);
                    assert!(mapped <= received);
                    assert!(mapped >= last);
                    let history = input.timestamp_at(ms.wrapping_sub(1), received);
                    assert_eq!(history, mapped - 1_000_000);
                    assert_eq!(input.timestamp_at(ms, received + 2_000_000), mapped);
                    if i > 50 {
                        assert_eq!(mapped, ideal + 300_000);
                    }
                    last = mapped;
                    last_receipt = received;
                }
            }
        }
    }

    #[test]
    fn native_clock_preserves_history_and_u32_wrap() {
        let input = Input::default();
        input.clock.set(Some((u32::MAX - 2, 1_000_000_000)));
        assert_eq!(input.timestamp(1), 1_004_000_000);
        assert_eq!(input.timestamp(0), 1_003_000_000);
        assert_eq!(input.timestamp(1), 1_004_000_000);
        assert_eq!(input.timestamp(3), 1_006_000_000);
    }
}
