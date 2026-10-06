//! The Crop tool: an oriented rectangle over the canvas with ratio
//! constraints, handles, guides and a dimmed shield, applied as one canvas
//! geometry edit. Straighten turns the rectangle to a drawn line or a typed
//! angle. Hosts forward contacts and present the shared bar and overlay.
use crate::localization::MessageId;
use super::operation::{HANDLE_HALF_SIZE, HANDLES, inside_convex, local_handle, nearest_handle};
use super::*;
use layer_core::{Affine, Affine64, CanvasGeometry, CanvasRect, Interpolation, Point, Rect};
use layer_engine::{PenEvent, PenPhase};
use layer_render::{CropOverlay, CursorSegment};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// Opacity of the shield outside the crop.
const SHIELD: f32 = 0.8;
/// A straighten line shorter than this, in logical pixels, is a click.
const MIN_LINE: f32 = 8.;
const DELETE_HINT: &str = "Turn on Delete Cropped Pixels to drop hidden pixels.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CropRatio {
    #[default]
    Free,
    Original,
    Square,
    FourFive,
    TwoThree,
    FiveSeven,
    SixteenNine,
}
impl CropRatio {
    const ALL: [Self; 7] =
        [Self::Free, Self::Original, Self::Square, Self::FourFive, Self::TwoThree, Self::FiveSeven, Self::SixteenNine];
    pub fn command(self) -> CommandId {
        match self {
            Self::Free => CommandId::CropRatioFree,
            Self::Original => CommandId::CropRatioOriginal,
            Self::Square => CommandId::CropRatioSquare,
            Self::FourFive => CommandId::CropRatioFourFive,
            Self::TwoThree => CommandId::CropRatioTwoThree,
            Self::FiveSeven => CommandId::CropRatioFiveSeven,
            Self::SixteenNine => CommandId::CropRatioSixteenNine,
        }
    }
    pub fn of(command: CommandId) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.command() == command)
    }
    /// The two sides of the ratio, before orientation.
    fn sides(self, canvas: [u32; 2]) -> Option<[f32; 2]> {
        match self {
            Self::Free => None,
            Self::Original => Some(canvas.map(|v| v as f32)),
            Self::Square => Some([1., 1.]),
            Self::FourFive => Some([4., 5.]),
            Self::TwoThree => Some([2., 3.]),
            Self::FiveSeven => Some([5., 7.]),
            Self::SixteenNine => Some([16., 9.]),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CropGuides {
    #[default]
    Thirds,
    Grid,
    Diagonal,
    Golden,
}
impl CropGuides {
    const ALL: [Self; 4] = [Self::Thirds, Self::Grid, Self::Diagonal, Self::Golden];
    pub fn command(self) -> CommandId {
        match self {
            Self::Thirds => CommandId::CropOverlayThirds,
            Self::Grid => CommandId::CropOverlayGrid,
            Self::Diagonal => CommandId::CropOverlayDiagonal,
            Self::Golden => CommandId::CropOverlayGolden,
        }
    }
    pub fn of(command: CommandId) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.command() == command)
    }
    fn next(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|g| *g == self).unwrap() + 1) % Self::ALL.len()]
    }
    /// Guide lines across the unit square.
    fn lines(self) -> Vec<[[f32; 2]; 2]> {
        let across = |at: &[f32]| {
            at.iter().flat_map(|&t| [[[t, 0.], [t, 1.]], [[0., t], [1., t]]]).collect::<Vec<_>>()
        };
        match self {
            Self::Thirds => across(&[1. / 3., 2. / 3.]),
            Self::Grid => across(&[1. / 6., 2. / 6., 3. / 6., 4. / 6., 5. / 6.]),
            Self::Golden => across(&[0.381_966, 0.618_034]),
            Self::Diagonal => Vec::new(),
        }
    }
}

/// Crop settings kept between sessions.
#[derive(Default)]
pub(super) struct CropOptions {
    pub ratio: CropRatio,
    pub guides: CropGuides,
    /// Delete Cropped Pixels; off keeps hidden pixels (decision 2).
    pub delete: bool,
}

/// An oriented rectangle: its size about `center`, turned by `angle`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CropFrame {
    pub center: Point,
    pub size: [f32; 2],
    pub angle: f32,
}
impl CropFrame {
    fn canvas(canvas: [u32; 2]) -> Self {
        let size = canvas.map(|v| v as f32);
        Self { center: Point { x: size[0] / 2., y: size[1] / 2. }, size, angle: 0. }
    }
    fn to_document(self) -> Affine {
        Affine::around(Point::default(), [1., 1.], self.angle, self.center)
    }
    fn local(self) -> Rect {
        let [w, h] = self.size.map(|v| v / 2.);
        Rect { min: Point { x: -w, y: -h }, max: Point { x: w, y: h } }
    }
    /// Corners in document pixels, top-left clockwise.
    pub fn corners(self) -> [Point; 4] {
        let map = self.to_document();
        self.local().corners().map(|p| map.map(p))
    }
    /// Unit square to document pixels.
    fn unit_to_document(self) -> Affine {
        let [w, h] = self.size;
        Affine([w, 0., 0., h, -w / 2., -h / 2.]).then(self.to_document())
    }
    fn handles(self) -> impl Iterator<Item = ([f32; 2], Point)> {
        let (map, local) = (self.to_document(), self.local());
        HANDLES.into_iter().map(move |side| (side, map.map(local_handle(local, side))))
    }
    /// The largest frame of `aspect` (width over height) turned by `angle`
    /// that fits in the canvas, centred on it.
    fn largest(aspect: f32, angle: f32, canvas: [u32; 2]) -> Self {
        let [width, height] = canvas.map(|v| v as f32);
        let (s, c) = angle.sin_cos();
        let (s, c) = (s.abs(), c.abs());
        let h = (width / (aspect * c + s)).min(height / (aspect * s + c)).max(1.);
        Self { size: [aspect * h, h], angle, ..Self::canvas(canvas) }
    }
    /// The canvas this frame cuts, turned upright about its centre.
    pub fn geometry(self, delete_outside: bool) -> CanvasGeometry {
        let [w, h] = self.size;
        let low = [self.center.x - w / 2., self.center.y - h / 2.];
        let high = [self.center.x + w / 2., self.center.y + h / 2.];
        let origin = low.map(|v| v.round() as i32);
        let size = std::array::from_fn(|i| (high[i].round() as i32 - origin[i]).max(1) as u32);
        let upright = self.angle.abs() < 1e-6;
        CanvasGeometry {
            rect: CanvasRect { origin, size },
            linear: if upright { Affine64::default() } else { CanvasGeometry::rotation([self.center.x, self.center.y].map(f64::from), -f64::from(self.angle)) },
            interpolation: Interpolation::Bicubic,
            delete_outside,
        }
    }
}

#[derive(Clone, Copy)]
enum CropDrag {
    Handle { side: [f32; 2], start: CropFrame, at: Point },
    Move { press: Point, start: CropFrame, at: Point },
    Line { from: Point, to: Point },
}

pub(super) struct CropSession {
    frame: CropFrame,
    portrait: bool,
    straighten: bool,
    drag: Option<CropDrag>,
    canvas: [u32; 2],
    previous: LayerCanvasTool,
}
impl CropSession {
    pub fn set_frame(&mut self, frame: CropFrame) {
        self.frame = frame;
        self.portrait = frame.size[1] > frame.size[0];
    }
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    /// The tool the crop returns to, which a saved workspace keeps.
    pub fn previous(&self) -> LayerCanvasTool {
        self.previous
    }
    #[cfg(test)]
    pub fn frame(&self) -> CropFrame {
        self.frame
    }
}

/// Keep a tilt within a quarter turn of level, so a steep line levels to
/// vertical instead.
fn tilt(angle: f32) -> f32 {
    angle - (angle / FRAC_PI_2).round() * FRAC_PI_2
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn cropping(&self) -> bool {
        self.operation.crop.is_some()
    }

    /// Why a command that edits the document waits on the open canvas operation.
    pub(super) fn operation_refusal(&self) -> std::sync::Arc<str> {
        let l = self.localization();
        if self.cropping() { l.text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_CROP_FIRST) } else { l.text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_TRANSFORM_FIRST) }
    }

    /// Width over height of the chosen ratio in the frame's orientation.
    fn crop_aspect(&self) -> Option<f32> {
        let session = self.operation.crop.as_ref()?;
        let [a, b] = self.operation.crop_options.ratio.sides(session.canvas)?;
        let (long, short) = (a.max(b), a.min(b));
        Some(if session.portrait { short / long } else { long / short })
    }

    pub(super) fn begin_crop(&mut self, angle: Option<f32>) -> Result<(), String> {
        self.require_idle()?;
        if self.operation.transforming() {
            return Err("Apply or cancel the transform first".into());
        }
        if self.cropping() {
            if let Some(angle) = angle {
                self.set_crop_angle(angle)?;
            }
            return Ok(());
        }
        refused(self.canvas_geometry_refusal())?;
        self.cancel_layer_gesture()?;
        let doc = self.engine.document();
        let canvas = doc.composition().size;
        let previous = match self.layer_interaction.tool {
            LayerCanvasTool::Crop | LayerCanvasTool::Transform => LayerCanvasTool::Move,
            tool => tool,
        };
        self.operation.crop = Some(CropSession {
            frame: CropFrame::canvas(canvas),
            portrait: canvas[1] > canvas[0],
            straighten: false,
            drag: None,
            canvas,
            previous,
        });
        self.layer_interaction.tool = LayerCanvasTool::Crop;
        self.state.layer_tools.tool = LayerCanvasTool::Crop;
        self.layer_interaction.changed = true;
        if let Some(aspect) = self.crop_aspect() {
            self.operation.crop.as_mut().unwrap().frame = CropFrame::largest(aspect, 0., canvas);
        }
        if let Some(angle) = angle {
            self.set_crop_angle(angle)?;
        }
        self.refresh_tools();
        Ok(())
    }

    /// Close the crop, applying it as one undo step when `apply`.
    pub(super) fn finish_crop(&mut self, apply: bool) -> Result<(), String> {
        let Some(session) = self.operation.crop.as_ref() else {
            return Ok(());
        };
        if apply {
            self.require_idle()?;
            let geometry = session.frame.geometry(self.operation.crop_options.delete);
            let unchanged = geometry.rect == CanvasRect { origin: [0; 2], size: session.canvas }
                && geometry.linear == Affine64::default()
                && !geometry.delete_outside;
            if !unchanged {
                match self.apply_canvas_geometry(&geometry, Vec::new()) {
                    Ok(()) | Err(layer_core::CanvasGeometryError::Unchanged) => (),
                    Err(e) if e.exceeds_raster_limits() && !geometry.delete_outside => {
                        return Err(format!("{e}. {DELETE_HINT}"));
                    }
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        let previous = self.operation.crop.take().map_or(LayerCanvasTool::Move, |s| s.previous);
        self.layer_interaction.path.clear();
        self.layer_interaction.tool = previous;
        self.state.layer_tools.tool = previous;
        self.layer_interaction.changed = true;
        self.operation.changed = true;
        self.sync_crop_overlay();
        self.refresh_tools();
        Ok(())
    }

    /// A canvas change from elsewhere, such as Undo, leaves the frame stale.
    pub(super) fn reconcile_crop(&mut self) {
        let doc = self.engine.document();
        if self.operation.crop.as_ref().is_some_and(|s| s.canvas != doc.composition().size) {
            let _ = self.finish_crop(false);
        }
    }

    pub(super) fn crop_command(&mut self, command: CommandId) -> Result<(), String> {
        self.require_idle()?;
        if command == CommandId::Crop {
            return self.begin_crop(None);
        }
        let session = self.operation.crop.as_mut().ok_or("Choose the Crop tool first")?;
        let (canvas, angle, [w, h]) = (session.canvas, session.frame.angle, session.frame.size);
        let options = &mut self.operation.crop_options;
        let refit = if let Some(ratio) = CropRatio::of(command) {
            options.ratio = ratio;
            session.portrait = h > w;
            true
        } else if let Some(guides) = CropGuides::of(command) {
            options.guides = guides;
            false
        } else {
            match command {
                CommandId::CropCycleOverlay => options.guides = options.guides.next(),
                CommandId::CropDeleteCroppedPixels => options.delete = !options.delete,
                CommandId::CropStraighten => session.straighten = !session.straighten,
                CommandId::CropSwapOrientation => session.portrait = !session.portrait,
                _ => return Err("Not a crop command".into()),
            }
            command == CommandId::CropSwapOrientation
        };
        if refit {
            let swapped = (command == CommandId::CropSwapOrientation).then_some(h / w);
            if let Some(aspect) = self.crop_aspect().or(swapped) {
                self.operation.crop.as_mut().unwrap().frame = CropFrame::largest(aspect, angle, canvas);
            }
        }
        self.layer_interaction.changed = true;
        self.refresh_tools();
        Ok(())
    }

    pub(super) fn reset_crop(&mut self) -> Result<(), String> {
        self.require_idle()?;
        let session = self.operation.crop.as_mut().ok_or("Choose the Crop tool first")?;
        session.straighten = false;
        session.frame = CropFrame::canvas(session.canvas);
        session.portrait = session.canvas[1] > session.canvas[0];
        let canvas = session.canvas;
        if let Some(aspect) = self.crop_aspect() {
            self.operation.crop.as_mut().unwrap().frame = CropFrame::largest(aspect, 0., canvas);
        }
        self.layer_interaction.changed = true;
        self.refresh_tools();
        Ok(())
    }

    /// Turn the frame, keeping its shape and fitting it in the canvas.
    pub(super) fn set_crop_angle(&mut self, angle: f32) -> Result<(), String> {
        if !angle.is_finite() || angle.abs() > FRAC_PI_4 + 1e-4 {
            return Err("Straighten turns the image by at most 45°".into());
        }
        let session = self.operation.crop.as_mut().ok_or("Choose the Crop tool first")?;
        let [w, h] = session.frame.size;
        let canvas = session.canvas;
        let aspect = self.crop_aspect().unwrap_or(w / h);
        self.operation.crop.as_mut().unwrap().frame = CropFrame::largest(aspect, angle, canvas);
        self.layer_interaction.changed = true;
        self.refresh_tools();
        Ok(())
    }

    pub(super) fn crop_controls(&self) -> Vec<tool_settings::ToolSetting> {
        let localizer = self.localization();
        let Some(session) = &self.operation.crop else {
            return Vec::new();
        };
        let limit = f64::from(self.engine.geometry_limits().canvas_dimension());
        let pixels = |label| tool_settings::ToolSetting {
            id: "",
            label: localizer.text(label),
            label_id: label,
            group: localizer.text(MessageId::TOOL_CONTROL_GROUP_SIZE),
            numeric: NumericControl::number(1., limit, 1., 0).unit("px"),
            value: 0.,
        };
        let degrees = NumericControl {
            scale: 180. / std::f64::consts::PI,
            step: std::f64::consts::PI / 180.,
            resolution: 0.00001,
            digits: 1,
            ..NumericControl::number(-std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_4, 0.01, 3).unit("°")
        };
        let [w, h] = session.frame.size;
        vec![
            tool_settings::ToolSetting { id: "crop_width", value: w, ..pixels(MessageId::TOOL_CONTROL_CROP_WIDTH) },
            tool_settings::ToolSetting { id: "crop_height", value: h, ..pixels(MessageId::TOOL_CONTROL_CROP_HEIGHT) },
            tool_settings::ToolSetting { id: "crop_angle", label: localizer.text(MessageId::TOOL_CONTROL_CROP_STRAIGHTEN), label_id: MessageId::TOOL_CONTROL_CROP_STRAIGHTEN, group: std::sync::Arc::from(""), numeric: degrees, value: session.frame.angle },
        ]
    }

    pub(super) fn set_crop_control(&mut self, id: &str, value: f32) -> Result<(), String> {
        self.require_idle()?;
        let control = self.crop_controls().into_iter().find(|c| c.id == id).ok_or("No crop setting")?;
        control.numeric.validate(value, control.label.as_ref()).map_err(|reason| reason.message(self.localization()))?;
        if id == "crop_angle" {
            return self.set_crop_angle(value);
        }
        let aspect = self.crop_aspect();
        let session = self.operation.crop.as_mut().ok_or("Choose the Crop tool first")?;
        let axis = usize::from(id == "crop_height");
        session.frame.size[axis] = value;
        if let Some(aspect) = aspect {
            session.frame.size[1 - axis] = if axis == 0 { value / aspect } else { value * aspect };
        }
        self.layer_interaction.changed = true;
        self.refresh_tools();
        Ok(())
    }

    pub(super) fn crop_pen(&mut self, event: PenEvent, p: Point) -> Result<(), String> {
        let reach = self.ruler_reach();
        let modifiers = self.interaction.modifiers;
        let Some(session) = &mut self.operation.crop else {
            return Ok(());
        };
        match event.phase {
            PenPhase::Down => {
                let frame = session.frame;
                session.drag = if session.straighten {
                    Some(CropDrag::Line { from: p, to: p })
                } else if let Some(side) = nearest_handle(frame.handles(), p, reach, Affine::IDENTITY) {
                    Some(CropDrag::Handle { side, start: frame, at: p })
                } else {
                    inside_convex(&frame.corners(), p).then_some(CropDrag::Move { press: p, start: frame, at: p })
                };
                if session.drag.is_some() {
                    self.layer_interaction.path = vec![p];
                }
            }
            PenPhase::Move | PenPhase::Up => {
                let Some(drag) = session.drag.as_mut() else {
                    return Ok(());
                };
                match drag {
                    CropDrag::Handle { at, .. } | CropDrag::Move { at, .. } => *at = p,
                    CropDrag::Line { to, .. } => *to = p,
                }
                self.apply_crop_drag(modifiers);
                if event.phase == PenPhase::Up {
                    self.end_crop_drag()?;
                }
            }
            PenPhase::Cancel => {
                self.cancel_crop_drag();
            }
            PenPhase::Hover => (),
        }
        Ok(())
    }

    /// Re-run the drag in progress, as when a modifier key changes.
    pub(super) fn update_crop_drag(&mut self) -> bool {
        if !self.operation.crop.as_ref().is_some_and(CropSession::dragging) {
            return false;
        }
        self.apply_crop_drag(self.interaction.modifiers);
        true
    }

    fn apply_crop_drag(&mut self, modifiers: Modifiers) {
        let aspect = self.crop_aspect();
        let limit = self.engine.geometry_limits().canvas_dimension() as f32;
        let snap = modifiers.shift;
        let Some(session) = &mut self.operation.crop else { return };
        let Some(drag) = session.drag else { return };
        match drag {
            CropDrag::Move { press, start, at } => {
                session.frame.center = Point { x: start.center.x + at.x - press.x, y: start.center.y + at.y - press.y };
            }
            CropDrag::Line { from, to } => {
                if snap {
                    let (dx, dy) = (to.x - from.x, to.y - from.y);
                    let step = PI / 12.;
                    let angle = (dy.atan2(dx) / step).round() * step;
                    let length = dx.hypot(dy);
                    session.drag = Some(CropDrag::Line {
                        from,
                        to: Point { x: from.x + angle.cos() * length, y: from.y + angle.sin() * length },
                    });
                }
            }
            CropDrag::Handle { side, start, at } => {
                let aspect = aspect.or((snap).then(|| start.size[0] / start.size[1]));
                let p = start.to_document().inverse().map_or(at, |inverse| inverse.map(at));
                let local = start.local();
                let centered = modifiers.alt;
                let fixed = if centered { Point::default() } else { local_handle(local, side.map(|v| -v)) };
                let rect = if side[0] != 0. && side[1] != 0. {
                    let constraint = if aspect.is_some() { SelectionConstraint::Ratio } else { SelectionConstraint::Free };
                    let (a, b) = selection_tools::constrained_corners(
                        constraint,
                        [aspect.unwrap_or(1.), 1.],
                        [1.; 2],
                        centered,
                        fixed,
                        p,
                        false,
                    );
                    Rect::around([a, b])
                } else {
                    let axis = usize::from(side[0] == 0.);
                    let along = |q: Point| if axis == 0 { q.x } else { q.y };
                    let reach = along(p);
                    let (low, high) = if centered {
                        (-reach.abs(), reach.abs())
                    } else {
                        (along(fixed).min(reach), along(fixed).max(reach))
                    };
                    let span = (high - low).max(1.);
                    let other = aspect.map_or(start.size[1 - axis], |a| if axis == 0 { span / a } else { span * a });
                    let [x0, x1, y0, y1] = if axis == 0 {
                        [low, low + span, -other / 2., other / 2.]
                    } else {
                        [-other / 2., other / 2., low, low + span]
                    };
                    Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } }
                };
                let size = [rect.max.x - rect.min.x, rect.max.y - rect.min.y].map(|v| v.clamp(1., limit));
                let middle = Point { x: (rect.min.x + rect.max.x) / 2., y: (rect.min.y + rect.max.y) / 2. };
                session.frame = CropFrame { center: start.to_document().map(middle), size, angle: start.angle };
            }
        }
    }

    fn end_crop_drag(&mut self) -> Result<(), String> {
        let zoom = self.state.camera.zoom;
        let Some(session) = &mut self.operation.crop else { return Ok(()) };
        let drag = session.drag.take();
        self.layer_interaction.path.clear();
        self.layer_interaction.changed = true;
        if let Some(CropDrag::Line { from, to }) = drag {
            let (dx, dy) = (to.x - from.x, to.y - from.y);
            session.straighten = false;
            if dx.hypot(dy) * zoom >= MIN_LINE {
                return self.set_crop_angle(tilt(dy.atan2(dx)));
            }
        }
        self.refresh_tools();
        Ok(())
    }

    /// Roll back the drag in progress; the crop stays open.
    pub(super) fn cancel_crop_drag(&mut self) -> bool {
        let Some(session) = &mut self.operation.crop else { return false };
        let Some(drag) = session.drag.take() else { return false };
        if let CropDrag::Handle { start, .. } | CropDrag::Move { start, .. } = drag {
            session.frame = start;
        }
        self.layer_interaction.path.clear();
        self.layer_interaction.changed = true;
        true
    }

    /// Whether a finger at `position` lands on a crop handle. A finger inside
    /// the frame pans and zooms, so the canvas stays navigable.
    pub(super) fn crop_touch_hit(&self, position: [f32; 2]) -> bool {
        let Some(session) = &self.operation.crop else { return false };
        let p = self.state.camera.input_transform().map(Point { x: position[0], y: position[1] });
        session.straighten || nearest_handle(session.frame.handles(), p, self.ruler_reach(), Affine::IDENTITY).is_some()
    }

    fn crop_overlay(&self) -> Option<CropOverlay> {
        let session = self.operation.crop.as_ref()?;
        Some(CropOverlay { to_crop: session.frame.unit_to_document().inverse()?, dim: SHIELD })
    }

    pub(super) fn sync_crop_overlay(&mut self) {
        let overlay = self.crop_overlay();
        self.engine.backend_mut().set_crop_overlay(overlay);
    }

    pub(super) fn append_crop_overlay(&self, segments: &mut Vec<CursorSegment>) {
        let Some(session) = &self.operation.crop else { return };
        let map = self.document_to_logical();
        let unit = session.frame.unit_to_document();
        let mut line = |a: Point, b: Point, solid: bool| {
            segments.push(CursorSegment { from: map(a), to: map(b), distance: 0., marker: f32::from(solid), scale: 1. })
        };
        let corners = session.frame.corners();
        for i in 0..4 {
            line(corners[i], corners[(i + 1) % 4], true);
        }
        let guides = self.operation.crop_options.guides;
        let point = |[x, y]: [f32; 2]| unit.map(Point { x, y });
        for [a, b] in guides.lines() {
            line(point(a), point(b), false);
        }
        if guides == CropGuides::Diagonal {
            let [w, h] = session.frame.size;
            let side = w.min(h);
            let [u, v] = [side / w, side / h];
            for ([x, y], [dx, dy]) in [([0., 0.], [u, v]), ([1., 0.], [-u, v]), ([0., 1.], [u, -v]), ([1., 1.], [-u, -v])] {
                line(point([x, y]), point([x + dx, y + dy]), false);
            }
        }
        if let Some(CropDrag::Line { from, to }) = session.drag {
            line(from, to, true);
        }
        if session.drag.is_none() || matches!(session.drag, Some(CropDrag::Handle { .. } | CropDrag::Move { .. })) {
            for (_, p) in session.frame.handles() {
                let [x, y] = map(p);
                segments.push(CursorSegment {
                    from: [x - HANDLE_HALF_SIZE, y - HANDLE_HALF_SIZE],
                    to: [x + HANDLE_HALF_SIZE, y + HANDLE_HALF_SIZE],
                    distance: 0.,
                    marker: 2.,
                    scale: 1.,
                });
            }
        }
    }

    /// The crop commands, in bar and Tool Options order.
    pub(super) fn crop_actions(&self) -> Vec<tool_settings::ToolSettingAction> {
        CropRatio::ALL
            .map(CropRatio::command)
            .into_iter()
            .chain([CommandId::CropSwapOrientation, CommandId::CropFitContent])
            .chain(CropGuides::ALL.map(CropGuides::command))
            .chain([
                CommandId::CropStraighten,
                CommandId::CropDeleteCroppedPixels,
                CommandId::ResetTransform,
                CommandId::ApplyTransform,
                CommandId::CancelTransform,
            ])
            .map(|command| tool_settings::ToolSettingAction { command, checkable: command.is_toggle() })
            .collect()
    }

    /// Whether a crop command is on: the chosen ratio and guides, an armed
    /// Straighten and Delete Cropped Pixels.
    pub(super) fn crop_selected(&self, command: CommandId) -> bool {
        let options = &self.operation.crop_options;
        let session = self.operation.crop.as_ref();
        (command == CommandId::Crop && session.is_some())
            || CropRatio::of(command) == Some(options.ratio)
            || CropGuides::of(command) == Some(options.guides)
            || (command == CommandId::CropDeleteCroppedPixels && options.delete)
            || (command == CommandId::CropStraighten && session.is_some_and(|s| s.straighten))
    }

    /// Straighten Image to Guide: crop with the selected straight guide level.
    pub(super) fn straighten_to_guide(&mut self) -> Result<(), String> {
        let ruler = self.selected_ruler().ok_or("Select a straight guide first")?;
        let layer_core::RulerGeometry::Straight { start, end } = ruler.geometry else {
            return Err("Select a straight guide first".into());
        };
        let angle = tilt((end.y - start.y).atan2(end.x - start.x));
        self.begin_crop(Some(angle))
    }

    pub(super) fn straighten_to_guide_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        match self.selected_ruler().map(|r| r.geometry) {
            Some(layer_core::RulerGeometry::Straight { .. }) => {
                if self.operation.transforming() { Some(l.text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_TRANSFORM_FIRST)) } else { self.canvas_geometry_refusal().filter(|_| !self.cropping()) }
            }
            _ => Some(l.text(MessageId::COMMANDS_REFUSAL_CROP_SELECT_A_STRAIGHT_GUIDE_FIRST)),
        }
    }
}
