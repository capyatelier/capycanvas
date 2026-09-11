//! GTK collects native records; the shared session interprets them. No raster
//! work or UI snapshots on the pen hot path.

use crate::workspace::Workspace;
use adw::prelude::*;
use glib::translate::IntoGlib;
use gtk::{gdk, glib};
use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_ui::{ContactPhase, Modifiers, PointerButton, PointerKind, UiInput};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
};

#[derive(Default)]
pub struct Input {
    sequence: Cell<u64>,
    last: Cell<Option<PenEvent>>,
    pending: RefCell<VecDeque<PenEvent>>,
    deferred_contacts: RefCell<std::collections::BTreeSet<u64>>,
    touches: RefCell<HashMap<gdk::EventSequence, u64>>,
    next_touch: Cell<u64>,
    clock: Cell<Option<(u32, u64)>>,
}

pub fn install(workspace: &Rc<Workspace>) {
    let input = workspace.input.clone();
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
            if workspace.reveal_chrome_at(x as f32, y as f32) {
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
    // the shared two-touch gesture logic rather than painting with fingers.
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
            let phase = match event.event_type() {
                gdk::EventType::TouchBegin => PenPhase::Down,
                gdk::EventType::TouchUpdate => PenPhase::Move,
                gdk::EventType::TouchEnd => PenPhase::Up,
                gdk::EventType::TouchCancel => PenPhase::Cancel,
                _ => return glib::Propagation::Proceed,
            };
            let sequence = event.event_sequence();
            let id = *input
                .touches
                .borrow_mut()
                .entry(sequence.clone())
                .or_insert_with(|| {
                    let id = input.next_touch.get() + 1;
                    input.next_touch.set(id);
                    id
                });
            let position = event
                .position()
                .and_then(|(x, y)| surface_point(&workspace, x, y))
                .map(|p| [p.x(), p.y()])
                .or_else(|| matches!(phase, PenPhase::Up | PenPhase::Cancel).then_some([0.0; 2]));
            if let Some(position) = position {
                if phase == PenPhase::Down && workspace.reveal_chrome_at(position[0], position[1]) {
                    return glib::Propagation::Stop;
                }
                let scale = workspace.area.scale_factor() as f32;
                workspace.interact(UiInput::Pointer {
                    id,
                    phase: contact_phase(phase),
                    kind: PointerKind::Touch,
                    button: PointerButton::Primary,
                    position: position.map(|v| v * scale),
                });
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
                if workspace.reveal_chrome_at(x as f32, y as f32) {
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
            let point = controller
                .current_event()
                .and_then(|e| e.position())
                .and_then(|(x, y)| surface_point(&workspace, x, y));
            if let Some(point) = point {
                let dpi = workspace.area.scale_factor() as f32;
                let unit = if controller.unit() == gdk::ScrollUnit::Wheel {
                    40.0
                } else {
                    1.0
                };
                let modifiers = controller.current_event_state();
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
    workspace.window.connect_is_active_notify(glib::clone!(
        #[weak]
        workspace,
        #[strong]
        input,
        move |window| {
            if !window.is_active() {
                input.cancel(&workspace);
                workspace.cursor_input(None);
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
    });
}

pub fn key_input(
    key: gdk::Key,
    pressed: bool,
    modifiers: gdk::ModifierType,
    editing: bool,
    divider: Option<u32>,
) -> UiInput {
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

fn contact_phase(phase: PenPhase) -> ContactPhase {
    match phase {
        PenPhase::Down => ContactPhase::Down,
        PenPhase::Up => ContactPhase::Up,
        PenPhase::Cancel => ContactPhase::Cancel,
        _ => ContactPhase::Move,
    }
}

fn surface_point(workspace: &Workspace, x: f64, y: f64) -> Option<gtk::graphene::Point> {
    let (dx, dy) = workspace.window.surface_transform();
    workspace.window.compute_point(
        &workspace.area,
        &gtk::graphene::Point::new((x + dx) as f32, (y + dy) as f32),
    )
}

impl Input {
    fn timestamp(&self, ms: u32) -> u64 {
        let (previous, previous_ns) = self
            .clock
            .get()
            .unwrap_or((ms, glib::monotonic_time().max(0) as u64 * 1000));
        let delta = ms.wrapping_sub(previous) as i32 as i64 * 1_000_000;
        let ns = previous_ns.saturating_add_signed(delta);
        if delta >= 0 {
            self.clock.set(Some((ms, ns)));
        }
        ns
    }
    fn stylus(
        &self,
        workspace: &Rc<Workspace>,
        gesture: &gtk::GestureStylus,
        phase: PenPhase,
        x: f64,
        y: f64,
    ) {
        let revision = workspace
            .gpu
            .borrow()
            .as_ref()
            .map(|g| g.session.state().camera.revision);
        let Some(view_revision) = revision else {
            return;
        };
        let tool = gesture.device_tool();
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
        let timestamp_ns = self.timestamp(gesture.current_event_time());
        let dpi = workspace.area.scale_factor() as f32;
        let axis = |a| gesture.axis(a).unwrap_or(0.0) as f32;
        let event = PenEvent {
            device_id: tool.as_ref().map_or(1, |t| t.serial().max(2)),
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
            flags: SampleFlags::PRIMARY,
        };
        workspace.cursor_input(Some(event));
        if phase == PenPhase::Hover {
            return;
        }
        let reply = workspace.interact(UiInput::Pointer {
            id: event.device_id,
            phase: contact_phase(phase),
            kind: if tool_kind == ToolKind::Mouse {
                PointerKind::Mouse
            } else {
                PointerKind::Pen
            },
            button: PointerButton::Primary,
            position: [event.surface_position.x, event.surface_position.y],
        });
        if !reply.paint {
            return;
        }
        if phase == PenPhase::Move {
            for point in gesture.backlog().unwrap_or_default() {
                let flags = point.flags();
                if !flags.contains(gdk::AxisFlags::X | gdk::AxisFlags::Y) {
                    continue;
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
                        event.pressure
                    },
                    tilt_radians: [
                        if flags.contains(gdk::AxisFlags::XTILT) {
                            get(gdk::AxisUse::Xtilt) * std::f32::consts::FRAC_PI_2
                        } else {
                            event.tilt_radians[0]
                        },
                        if flags.contains(gdk::AxisFlags::YTILT) {
                            get(gdk::AxisUse::Ytilt) * std::f32::consts::FRAC_PI_2
                        } else {
                            event.tilt_radians[1]
                        },
                    ],
                    ..event
                };
                if self
                    .last
                    .get()
                    .is_none_or(|last| history.timestamp_ns >= last.timestamp_ns)
                {
                    self.send(workspace, history);
                }
            }
        }
        self.send(workspace, event);
    }
    pub(crate) fn send(&self, workspace: &Rc<Workspace>, mut event: PenEvent) {
        // A contact begun during compilation must not start midway when its
        // shader becomes ready. Navigation and native controls remain active.
        let ready = workspace.gpu.borrow().as_ref().is_some_and(|g| {
            let engine = g.session.engine();
            engine
                .backend()
                .paint_ready(engine.document(), engine.brush())
        });
        let mut deferred = self.deferred_contacts.borrow_mut();
        if event.phase == PenPhase::Down && !ready {
            deferred.insert(event.device_id);
        }
        let blocked = deferred.contains(&event.device_id);
        if matches!(event.phase, PenPhase::Up | PenPhase::Cancel) {
            deferred.remove(&event.device_id);
        }
        drop(deferred);
        if blocked {
            return;
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
            if self.pending.borrow().is_empty() {
                if let Err(event) = gpu.session.pen(event) {
                    self.pending.borrow_mut().push_back(event);
                }
            } else {
                self.pending.borrow_mut().push_back(event);
            }
        }
        workspace.wake();
    }
    pub fn has_pending(&self) -> bool {
        !self.pending.borrow().is_empty()
    }
    pub fn flush(&self, workspace: &Rc<Workspace>) {
        let mut gpu = workspace.gpu.borrow_mut();
        if let Some(gpu) = gpu.as_mut() {
            let mut pending = self.pending.borrow_mut();
            while let Some(event) = pending.pop_front() {
                if let Err(event) = gpu.session.pen(event) {
                    pending.push_front(event);
                    break;
                }
            }
        }
    }
    fn cancel(&self, workspace: &Rc<Workspace>) {
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
    use super::*;
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
