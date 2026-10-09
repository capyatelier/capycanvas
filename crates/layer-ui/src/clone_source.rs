//! The Clone Stamp's and Healing Brush's source: Set Source, the source disc
//! on the canvas, and the options its bar and Tool Options share. The document's engine keeps
//! where strokes copy from, since strokes move an aligned source; the session
//! keeps which layers they copy and how the disc is being handled.
use super::*;
use crate::interaction::Restore;
use layer_core::{CloneSource, Point, RetouchSource};
use layer_render::CursorSegment;

/// The source disc's radius, in logical pixels.
pub(super) const DISC_RADIUS: f32 = 14.;

#[derive(Default)]
pub(super) struct RetouchState {
    /// What retouching strokes copy, kept apart from the region tools' source.
    pub source: RetouchSource,
    /// Set Source is held or armed: the next pen or mouse contact of a
    /// retouching tool sets its source instead of painting.
    pub armed: bool,
    /// The disc was tapped: its bar shows until it is tapped again or the
    /// tool changes.
    pub bar: bool,
    pub drag: Option<SourceDrag>,
}

/// A contact moving the source disc, or setting the source where it lands.
#[derive(Clone, Copy)]
pub(super) struct SourceDrag {
    id: u64,
    /// Surface position of the press.
    start: [f32; 2],
    /// The source as the contact found it, restored on cancel.
    before: CloneSource,
    /// From the pointer to the disc's center, in document units.
    grab: [f32; 2],
    moved: bool,
    /// The contact sets the source where it lands.
    sets: bool,
}

impl<R: CanvasRenderer> UiSession<R> {
    /// A retouching tool paints with the pen.
    pub(super) fn retouching(&self) -> bool {
        Self::tool_category(self.layer_interaction.tool, self.state.brush.tool) == ToolCategory::Retouching
    }

    /// A retouching tool that copies from the source disc: the Clone Stamp or
    /// the Healing Brush.
    fn cloning(&self) -> bool {
        self.retouching()
            && matches!(self.state.brush.tool, Tool::Clone | Tool::Heal)
            && self.selection_masks.target().is_none()
    }

    fn surface_to_document(&self, position: [f32; 2]) -> Point {
        self.state.camera.input_transform().map(Point { x: position[0], y: position[1] })
    }

    /// Where the source disc sits: the source point, or while a stroke is
    /// copying, the point under the brush it copies from.
    pub(super) fn clone_disc(&self) -> Option<Point> {
        if !self.cloning() {
            return None;
        }
        let source = self.engine.clone_source();
        let live = self.engine.clone_stroke_offset().zip(self.cursor.event).map(|(offset, event)| {
            let p = self.surface_to_document([event.surface_position.x, event.surface_position.y]);
            CloneSource { offset: Some(offset), ..source }.source_of(p)
        });
        live.flatten().or(source.point)
    }

    /// The disc's radius in document units.
    fn disc_reach(&self) -> f32 {
        let dpi = self.logical_viewport.map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        (DISC_RADIUS * dpi / self.state.camera.zoom).max(self.ruler_reach())
    }

    /// Whether a contact at surface `position` lands on the source disc.
    pub(super) fn clone_disc_hit(&self, position: [f32; 2]) -> bool {
        self.clone_disc().is_some_and(|center| {
            let p = self.surface_to_document(position);
            (p.x - center.x).hypot(p.y - center.y) <= self.disc_reach()
        })
    }

    /// Whether a primary contact of `kind` at `position` handles the source
    /// instead of painting: it lands on the disc, or Set Source is armed and
    /// it is a pen or mouse. A finger never sets the source.
    pub(super) fn clone_source_contact(&self, kind: PointerKind, position: [f32; 2]) -> bool {
        self.cloning()
            && self.interaction.navigation.is_none()
            && (self.clone_disc_hit(position) || (self.retouch.armed && kind != PointerKind::Touch))
    }

    pub(super) fn begin_clone_source_contact(&mut self, id: u64, kind: PointerKind, position: [f32; 2]) {
        let before = self.engine.clone_source();
        let p = self.surface_to_document(position);
        let sets = self.retouch.armed && kind != PointerKind::Touch;
        let grab = match before.point.filter(|_| !sets) {
            Some(center) => [center.x - p.x, center.y - p.y],
            None => {
                let mut source = before;
                source.set(p);
                self.engine.set_clone_source(source);
                self.sync_retouch_points();
                [0.; 2]
            }
        };
        self.retouch.drag = Some(SourceDrag { id, start: position, before, grab, moved: sets, sets });
    }

    /// Follow or finish the source contact `id`. A disc contact that never
    /// moved past the touch slop is a tap, which shows or hides the disc's
    /// bar; one that set the source disarms a Set Source that isn't held.
    pub(super) fn clone_source_input(&mut self, id: u64, phase: ContactPhase, position: [f32; 2]) -> Option<UiChange> {
        let mut drag = self.retouch.drag.filter(|d| d.id == id)?;
        let slop = self.interaction.touch_policy.slop;
        match phase {
            ContactPhase::Down => {}
            ContactPhase::Move | ContactPhase::Up => {
                let travel = (position[0] - drag.start[0]).hypot(position[1] - drag.start[1]);
                if drag.moved || travel > slop {
                    drag.moved = true;
                    let p = self.surface_to_document(position);
                    let mut source = self.engine.clone_source();
                    source.move_to(Point { x: p.x + drag.grab[0], y: p.y + drag.grab[1] });
                    self.engine.set_clone_source(source);
                    self.sync_retouch_points();
                }
            }
            ContactPhase::Cancel => {
                self.engine.set_clone_source(drag.before);
                self.sync_retouch_points();
            }
        }
        self.retouch.drag = Some(drag);
        if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
            self.retouch.drag = None;
            self.interaction.pointer = None;
            let held = self.interaction.momentary.iter().any(|(_, restore)| matches!(restore, Restore::Toggle(CommandId::CloneSourceArm, _)));
            if drag.sets && !held {
                self.retouch.armed = false;
            } else if phase == ContactPhase::Up && !drag.moved {
                self.retouch.bar = !self.retouch.bar;
            }
            self.refresh_commands();
            return Some(self.changed(regions::COMMANDS, true));
        }
        Some(self.changed(0, true))
    }

    /// Tell the engine what the selected retouching tool copies, and put a
    /// Clone source that was never set in the middle of the view.
    pub(super) fn sync_retouch(&mut self) {
        let retouching = self.retouching();
        if !retouching {
            self.retouch.bar = false;
            self.retouch.armed = false;
        }
        self.engine.set_retouch(retouching.then_some(self.retouch.source));
        if self.cloning() && self.engine.clone_source().point.is_none() {
            let doc = self.engine.document();
            let [x, y, width, height] = self.state.camera.work_area;
            let center = if width > 0. && height > 0. {
                self.surface_to_document([x + width / 2., y + height / 2.])
            } else {
                Point { x: doc.composition().size[0] as f32 / 2., y: doc.composition().size[1] as f32 / 2. }
            };
            let mut source = self.engine.clone_source();
            source.set(Point { x: center.x.clamp(0., doc.composition().size[0] as f32), y: center.y.clamp(0., doc.composition().size[1] as f32) });
            self.engine.set_clone_source(source);
        }
        self.sync_retouch_points();
    }

    /// Reference pages worth capturing before the next stroke: around the
    /// source, and where the hovering brush would copy from.
    pub(super) fn sync_retouch_points(&mut self) {
        if !self.retouching() {
            return;
        }
        let source = self.engine.clone_source();
        let hover = self.cursor.event.map(|e| self.surface_to_document([e.surface_position.x, e.surface_position.y]));
        let points: Vec<Point> = source.point.into_iter().chain(hover.and_then(|p| source.source_of(p))).collect();
        self.engine.set_retouch_points(&points);
    }

    /// Retouching options, in bar and Tool Options order. Spot Healing finds
    /// its own source, so it offers only what it samples.
    pub(super) fn clone_actions(&self) -> Vec<tool_settings::ToolSettingAction> {
        let commands: &[CommandId] = if self.state.brush.tool == Tool::SpotHeal {
            &[CommandId::SelectionReference, CommandId::SelectionEditing]
        } else {
            &[
                CommandId::CloneAligned,
                CommandId::SelectionReference,
                CommandId::SelectionEditing,
                CommandId::CloneFlipHorizontal,
                CommandId::CloneFlipVertical,
                CommandId::CloneResetOffset,
                CommandId::CloneSourceArm,
            ]
        };
        commands.iter().map(|&command| tool_settings::ToolSettingAction { command, checkable: command.is_toggle() }).collect()
    }

    pub(super) fn clone_command_enabled(&self, command: CommandId) -> bool {
        self.require_idle().is_ok()
            && self.cloning()
            && (command != CommandId::CloneResetOffset || self.engine.clone_source().offset.is_some())
    }

    pub(super) fn clone_command_selected(&self, command: CommandId) -> bool {
        let on = match command {
            CommandId::CloneAligned => self.engine.clone_source().aligned,
            CommandId::CloneFlipHorizontal => self.engine.clone_source().flip[0],
            CommandId::CloneFlipVertical => self.engine.clone_source().flip[1],
            CommandId::CloneSourceArm => self.retouch.armed,
            CommandId::SelectionReference => self.retouch.source == RetouchSource::References,
            CommandId::SelectionEditing => self.retouch.source == RetouchSource::Editing,
            _ => return false,
        };
        on && self.retouching()
    }

    pub(super) fn clone_command(&mut self, command: CommandId) -> Result<(), String> {
        if !self.retouching() {
            return Err("Choose a retouching tool first".into());
        }
        if !self.cloning() && !matches!(command, CommandId::SelectionReference | CommandId::SelectionEditing) {
            return Err("Spot Healing finds its own source".into());
        }
        let mut source = self.engine.clone_source();
        match command {
            CommandId::CloneAligned => source.set_aligned(!source.aligned),
            CommandId::CloneFlipHorizontal => source.toggle_flip(0),
            CommandId::CloneFlipVertical => source.toggle_flip(1),
            CommandId::CloneResetOffset => source.reset_offset(),
            CommandId::CloneSourceArm => self.retouch.armed = !self.retouch.armed,
            CommandId::SelectionReference => self.retouch.source = RetouchSource::References,
            CommandId::SelectionEditing => self.retouch.source = RetouchSource::Editing,
            _ => return Err("Unknown clone option".into()),
        }
        self.engine.set_clone_source(source);
        self.refresh_tools();
        Ok(())
    }

    /// The disc as cursor segments: a ring with a sight at its center.
    pub(super) fn append_clone_overlay(&self, segments: &mut Vec<CursorSegment>) {
        let Some(center) = self.clone_disc() else { return };
        let [x, y] = self.document_to_logical()(center);
        let steps = 40;
        let point = |i: usize| {
            let (sin, cos) = (i as f32 * std::f32::consts::TAU / steps as f32).sin_cos();
            [x + DISC_RADIUS * cos, y + DISC_RADIUS * sin]
        };
        let mut distance = 0.;
        for i in 0..steps {
            let (from, to) = (point(i), point(i + 1));
            segments.push(CursorSegment { from, to, distance, marker: 1., scale: 1. });
            distance += (to[0] - from[0]).hypot(to[1] - from[1]);
        }
        let sight = DISC_RADIUS / 2.;
        segments.push(CursorSegment { from: [x - sight, y - sight], to: [x + sight, y + sight], distance: 0., marker: 5., scale: 1. });
    }

    /// The disc's bounds in document space, `[min_x, min_y, max_x, max_y]`.
    pub(super) fn clone_disc_bounds(&self) -> Option<[f32; 4]> {
        let center = self.clone_disc()?;
        let dpi = self.logical_viewport.map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        let r = DISC_RADIUS * dpi / self.state.camera.zoom;
        Some([center.x - r, center.y - r, center.x + r, center.y + r])
    }
}
