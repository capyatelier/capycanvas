use super::*;
use crate::bubbles::{Desk, Shape, bead_scroll_limit, bead_travel};
use layer_core::BrushSnapshot;

const TAP_SLOP: f32 = 8.;
const EDGE_DISMISS_DISTANCE: f32 = 8.;

#[derive(Clone, Copy, Debug)]
struct BubbleDrag {
    id: u32,
    offset: [f32; 2],
    source: Option<Bounds>,
    pulled: bool,
    position: [f32; 2],
    viewport: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
struct BeadContact {
    id: u32,
    strip: Option<BeadStrip>,
    start: [f32; 2],
    last: [f32; 2],
    moved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Chooser {
    id: u32,
    medium: Option<ToolGroup>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ColorBubbleGeometry {
    layout: ColorBubbleLayout,
    options: BubbleOptions,
    pair: PairBeads,
    front: Bead,
    counts: [usize; 2],
    scroll: [f32; 2],
}

#[derive(Debug)]
struct ColorBubbleParts {
    beads: Vec<BeadPlacement>,
    hints: Vec<([f32; 2], f32)>,
    collisions: Vec<Shape>,
}
type BoundsKey = (u64, [f32; 2], bool);

#[derive(Debug, Default)]
pub(crate) struct Bubbles {
    desk: Desk,
    bounds: Option<(BoundsKey, Bounds)>,
    drag: Option<BubbleDrag>,
    pending_drag: Option<BubbleGrab>,
    drag_desk: Option<Desk>,
    halo: Option<(u32, u32)>,
    ring: Option<RingContact>,
    wheel: Option<WheelContact>,
    beads: Option<BeadContact>,
    scroll: Vec<(u32, [f32; 2])>,
    pill: Option<u32>,
    chooser: Option<Chooser>,
    card: Option<u32>,
    card_metrics: Option<(u32, bool, PanelScrollMeasurement, f32)>,
    properties: bool,
    stroke: bool,
    dirty: bool,
    color_geometry: std::cell::RefCell<Option<(ColorBubbleGeometry, ColorBubbleParts)>>,
}

impl Bubbles {
    pub(super) fn moving(&self)->bool {self.drag.is_some()||self.pending_drag.is_some()}
}

#[derive(Clone, Copy, Debug)]
struct RingContact {
    id: u32,
    setting: &'static str,
    before: f32,
    at: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
struct WheelContact {
    id: u32,
    context: ColorEditContext,
    part: Option<ColorWheelPart>,
    intensity: Option<f32>,
}

fn control_face(state: &UiState, control: ToolbarControl) -> (String, &'static str, bool, bool) {
    if let Some((choice, enabled, _, _)) = state.resolve_group(control) {
        return (choice.label, choice.icon, choice.selected, enabled);
    }
    let choice = customization::tool_choice_localized(control, &state.localization);
    let (enabled, selected) = tool_state(state, control);
    (choice.label, tool_icon(state, control), selected, enabled)
}

fn near(bounds: Bounds, point: [f32; 2], distance: f32) -> bool {
    let dx = (bounds.x - point[0]).max(point[0] - bounds.x - bounds.width).max(0.);
    let dy = (bounds.y - point[1]).max(point[1] - bounds.y - bounds.height).max(0.);
    dx.hypot(dy) <= distance
}

fn inset(bounds: Bounds, by: f32) -> Bounds {
    Bounds { x: bounds.x + by, y: bounds.y + by, width: (bounds.width - by * 2.).max(1.), height: (bounds.height - by * 2.).max(1.) }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn bubbles(&self) -> Vec<BubbleView> {
        self.state.workspace.layout.bubbles.bubbles.iter().map(|bubble| self.bubble_view(bubble)).collect()
    }

    pub fn bubble_presentations(&self) -> Vec<BubblePresentation> {
        let colors = self.state.display_colors();
        let map = |color| colors.mapped_swatch(color, self.effective_sdr_rendition());
        self.bubbles().into_iter().map(|view| BubblePresentation {
            color: view.color.and_then(|layout| self.bubble_color_presentation(view.id, layout, colors)),
            reach: view.reach(),
            readout: view.readout_center(),
            pill: view.palette_bounds(),
            history: [history_center(0), history_center(1)],
            dots: view.dot_centers(),
            bead_colors: view.beads.iter().map(|bead| bead.color.map(map)).collect(),
            stamp_color: view.stamp.map(|stamp| map(stamp.color)),
            rings: view.rings.iter().map(|ring| BubbleRingGeometry {
                start: ring.arc.point(0.), end: ring.arc.point(1.),
                thumb: ring.arc.point(ring.fill), icon: ring.arc.icon_point(),
            }).collect(),
            view,
        }).collect()
    }

    pub fn bubble_intensity_range(&self, id: u32, colors: &ColorState) -> [f32; 2] {
        let stops = self.bubble_state.wheel
            .filter(|wheel| wheel.id == id && wheel.part.is_none() && wheel.context == self.color_wheel_context())
            .and_then(|wheel| wheel.intensity).unwrap_or_else(|| colors.hdr_intensity());
        HdrIntensityArc::range(stops, colors.hdr_depth())
    }

    fn bubble_color_presentation(&self, id: u32, layout: ColorBubbleLayout, colors: &ColorState) -> Option<BubbleColorPresentation> {
        layout.intensity.map(|geometry| {
            let stops = colors.hdr_intensity();
            let range = self.bubble_intensity_range(id, colors);
            let end = geometry.point(1.);
            let mut marker = colors.mapped_swatch(colors.definition(), self.effective_sdr_rendition());
            marker[3] = 1.;
            BubbleColorPresentation {
                bounds: layout.bounds(), outline: layout.outline(0.),
                glass: [layout.diameter * 0.5, geometry.radius, geometry.width * 0.5 + layout.margin(), end[1].atan2(end[0])],
                intensity: BubbleIntensityPresentation {
                    geometry, path: (0..=64).map(|i| geometry.point(i as f32 / 64.)).collect(),
                    point: geometry.point((stops - range[0]) / (range[1] - range[0])), zero: geometry.zero_tick(range), range, stops,
                    ramp: colors.intensity_ramp(self.effective_sdr_rendition(), range), marker,
                    label: self.localization().text(MessageId::NATIVE_COLOR_INTENSITY).to_string(),
                    text: format!("{stops:+.2} EV"), enabled: colors.slot != ColorSlot::Transparent,
                },
            }
        })
    }

    pub fn bubble_color_previews(&self, colors: &ColorState) -> Vec<(u32, BubbleColorPresentation)> {
        let picker = &self.state.color_picker;
        if picker.preview.is_none() || picker.editor || picker.calibrating { return Vec::new(); }
        self.state.workspace.layout.bubbles.bubbles.iter().filter(|bubble| bubble.source.kind() == BubbleKind::Color)
            .filter_map(|bubble| self.bubble_color_presentation(bubble.id, self.color_bubble(bubble), colors).map(|color| (bubble.id, color))).collect()
    }

    pub fn bubble_placements(&mut self) -> Vec<BubblePlacement> {
        self.sync_bubbles();
        self.bubble_state.desk.placements()
    }

    pub fn step_bubbles(&mut self, now_ns: u64) -> bool {
        self.sync_bubbles();
        self.bubble_state.desk.advance(now_ns)
    }

    pub fn reduce_bubble_motion(&mut self, reduce: bool) {
        self.bubble_state.desk.still(reduce);
    }

    pub fn bubble_fade(&self) -> (bool, Vec<u32>) {
        (self.bubble_state.stroke, self.bubble_state.desk.faded().to_vec())
    }

    pub(crate) fn reset_bubbles(&mut self) -> Result<UiChange, String> {
        let mut change = if let Some(ring) = self.bubble_state.ring {
            self.bubble_ring(ring.id, ContactPhase::Cancel, [0.; 2])?
        } else { UiChange::default() };
        if let Some(wheel) = self.bubble_state.wheel {
            let restored = self.bubble_color(wheel.id, ContactPhase::Cancel, wheel.part, [0.; 2])?;
            change.regions |= restored.regions;
            change.canvas_wake |= restored.canvas_wake;
            change.revision = change.revision.max(restored.revision);
        }
        self.bubble_state = Bubbles { stroke: self.bubble_state.stroke, ..Bubbles::default() };
        Ok(change)
    }

    pub fn bubble_grab(&self, grab: BubbleGrab, held: bool) -> Option<BubbleGrab> {
        let anchor = match grab {
            BubbleGrab::Header { id } => DrawerAnchor::Header { id },
            BubbleGrab::Tile { panel, tile } => DrawerAnchor::Tile { panel, tile },
            BubbleGrab::Variant { variant } => return BubbleSource::for_variant(variant).map(|_| grab),
            BubbleGrab::Bubble { id } | BubbleGrab::Rim { id } => return self.state.workspace.layout.bubbles.get(id).map(|_| grab),
        };
        self.state.workspace.layout.anchor_control(anchor)
            .filter(|control| if held { BubbleSource::for_control(*control).is_some() } else { BubbleSource::tears_without_hold(*control) })
            .map(|_| grab)
    }

    pub fn can_take_bubble(&mut self, grab: BubbleGrab, position: [f32; 2], viewport: [f32; 2]) -> bool {
        valid_viewport(viewport).is_ok() && position.into_iter().all(f32::is_finite)
            && self.bubble_canvas(viewport).contains(position[0], position[1])
            && self.grab_source(grab, viewport).is_ok_and(|(_, bounds)| !self.bubble_homing(viewport, position, bounds))
    }

    fn bubble_viewport(&self) -> [f32; 2] {
        self.logical_viewport.unwrap_or([1200., 900.])
    }

    fn narrow(&self) -> bool {
        self.bubble_viewport()[0] < 600.
    }

    pub fn bubble_card_metrics(&self) -> Option<(u32, bool, PanelScrollMeasurement, f32)> {
        self.bubble_state.card_metrics.filter(|(id, full, ..)| self.bubble_state.card == Some(*id)
            && self.state.workspace.layout.bubbles.get(*id).is_some_and(|bubble| bubble.options.full == *full))
    }

    fn card_size(&self) -> [f32; 2] {
        let viewport = self.bubble_viewport();
        let measured = self.bubble_card_metrics().map(|(_, _, measurement, properties_height)|
            LayerDrawerSizing::pages(measurement, self.state.layers.len(), viewport[1], properties_height));
        layers_card(viewport, self.state.layers.len(), measured, self.bubble_state.properties)
    }

    fn hdr_color_bubbles(&self) -> bool {
        self.selection_masks.target().is_none()
            && self.engine.document().composition().color.depth.is_float()
    }

    fn color_bubble(&self, bubble: &ToolBubble) -> ColorBubbleLayout {
        ColorBubbleLayout::new(bubble.diameter(self.narrow(), self.state.workspace.layout.bubbles.show_sliders), self.hdr_color_bubbles())
    }

    fn pair_beads(&self) -> PairBeads {
        PairBeads::for_window(self.bubble_viewport()[0])
    }

    fn bubble_bounds(&mut self) -> Bounds {
        let viewport = self.bubble_viewport();
        let key = (self.workspace_model_revision, viewport, self.interaction.hidden);
        if let Some((cached, bounds)) = self.bubble_state.bounds
            && cached == key
        {
            return bounds;
        }
        let area = self.bubble_canvas(viewport);
        let bounds = inset(area, 8.);
        self.bubble_state.bounds = Some((key, bounds));
        bounds
    }

    fn bubble_canvas(&mut self, viewport: [f32; 2]) -> Bounds {
        if self.interaction.hidden { self.state.workspace.layout.free_area(viewport) } else { self.layout(viewport).work_area }
    }

    fn bubble_center(&self, bubble: &ToolBubble) -> [f32; 2] {
        self.bubble_state.desk.center(bubble.id).unwrap_or_else(|| bubble.center(self.bubble_viewport()))
    }

    fn sync_bubbles(&mut self) {
        let bounds = self.bubble_bounds();
        let viewport = self.bubble_viewport();
        let narrow = self.narrow();
        let pair = self.pair_beads();
        let hdr = self.hdr_color_bubbles();
        let layout = &self.state.workspace.layout.bubbles;
        let state = &mut self.bubble_state;
        for open in [&mut state.pill, &mut state.card] {
            if open.is_some_and(|id| layout.get(id).is_none()) {
                *open = None;
            }
        }
        if state.halo.is_some_and(|(id, _)| !layout.show_sliders || layout.get(id).is_none()) {
            state.halo = None;
        }
        if state.ring.is_some_and(|ring| !layout.show_sliders || layout.get(ring.id).is_none()) {
            state.ring = None;
        }
        if state.chooser.is_some_and(|c| layout.get(c.id).is_none()) {
            state.chooser = None;
        }
        let dropped_wheel = state.wheel.is_some_and(|c| layout.get(c.id).is_none());
        if dropped_wheel {
            state.wheel = None;
        }
        state.scroll.retain(|(id, _)| layout.get(*id).is_some());
        let items: Vec<_> = layout.bubbles.iter().map(|b| (b.id, b.center(viewport), if hdr && b.source.kind() == BubbleKind::Color { Shape::circle(b.diameter(narrow, layout.show_sliders) * 0.5) } else { b.shape(narrow, pair, layout.show_sliders) })).collect();
        if !state.desk.matches(&items, bounds) {
            state.desk.sync(&items, bounds);
        }
        let grown = self.bubble_growth();
        self.bubble_state.desk.grow(grown);
        if dropped_wheel {
            self.end_color_contact();
        }
    }

    fn bubble_growth(&self) -> Vec<(u32, Vec<Shape>)> {
        let mut grown = Vec::new();
        if let Some(id) = self.halo() {
            grown.push((id, vec![Shape::circle(halo_disc(self.halo_settings().len()) * 0.5)]));
        }
        if let Some(id) = self.bubble_state.card.filter(|id| self.state.workspace.layout.bubbles.get(*id).is_some()) {
            grown.push((id, vec![Shape::rect(self.card_size())]));
        }
        for bubble in self.state.workspace.layout.bubbles.bubbles.iter().filter(|b| b.source.kind() == BubbleKind::Color) {
            let color = self.color_bubble(bubble);
            let pill = self.bubble_state.pill == Some(bubble.id) && bubble.options.palette;
            if color.intensity.is_none() && !pill { continue; }
            let mut parts = if color.intensity.is_some() {
                self.color_bubble_parts(bubble).collisions.clone()
            } else { vec![bubble.shape(self.narrow(), self.pair_beads(), self.state.workspace.layout.bubbles.show_sliders)] };
            if pill {
                let b = color.palette_bounds();
                parts.push(Shape::rect([b.width, b.height]).at([b.x + b.width * 0.5, b.y + b.height * 0.5]));
            }
            grown.push((bubble.id, parts));
        }
        if let Some(chooser) = self.bubble_state.chooser
            && let Some(bubble) = self.state.workspace.layout.bubbles.get(chooser.id)
        {
            let items = self.chooser_items(bubble, chooser).0.into_iter().flat_map(|item| {
                let part = if item.kind == ChooserKind::Brush { Shape::rect(BRUSH_PILL) } else { Shape::circle(CHOOSER_ITEM * 0.5) };
                let tag = (item.shown && item.kind != ChooserKind::Brush).then(|| {
                    Shape::rect([item.tag.width, item.tag.height]).at([item.tag.x + item.tag.width * 0.5, item.tag.y + item.tag.height * 0.5])
                });
                std::iter::once(part.at(item.center)).chain(tag)
            });
            grown.push((chooser.id, std::iter::once(Shape::circle(TOOL_BUBBLE * 0.5)).chain(items).collect()));
        }
        grown
    }

    fn bubble_view(&self, bubble: &ToolBubble) -> BubbleView {
        let state = &self.state;
        let kind = bubble.source.kind();
        let narrow = self.narrow();
        let buttons: Vec<_> = match kind {
            BubbleKind::History => HISTORY_COMMANDS
                .map(|command| {
                    let (label, icon, _, enabled) = control_face(state, ToolbarControl::Command { command });
                    BubbleButton { label, icon, enabled }
                })
                .into(),
            _ => Vec::new(),
        };
        let (label, icon, selected, enabled) = match bubble.source {
            _ if !buttons.is_empty() => {
                (buttons.iter().map(|b| b.label.as_str()).collect::<Vec<_>>().join(" · "), buttons[0].icon, false, true)
            }
            BubbleSource::Medium { group } => (
                group.localized_label(self.localization()).to_string(),
                group.icon(),
                state.layer_tools.tool == LayerCanvasTool::Paint && tools::group(state.brush.preset) == group,
                true,
            ),
            BubbleSource::Control { control } => control_face(state, control),
        };
        let halo = self.halo() == Some(bubble.id);
        let rings: Vec<_> = match kind {
            BubbleKind::Controls if state.workspace.layout.bubbles.show_sliders => halo_arcs(CONTROL_RINGS.len())
                .into_iter()
                .zip(if bubble.options.swap { CONTROL_RINGS } else { [CONTROL_RINGS[1], CONTROL_RINGS[0]] })
                .map(|(arc, binding)| match binding.field(state) {
                    Some(field) => self.ring_view(bubble.id, arc, &field, binding.numeric()),
                    None => BubbleRing {
                        arc,
                        setting: binding.setting(),
                        label: self.localization().text(binding.label()),
                        icon: tool_setting_icon(binding.setting()),
                        fill: 0.,
                        text: String::new(),
                        active: false,
                        enabled: false,
                    },
                })
                .collect(),
            _ if halo => {
                let settings = self.halo_settings();
                halo_arcs(settings.len())
                    .into_iter()
                    .zip(settings)
                    .map(|(arc, setting)| self.ring_view(bubble.id, arc, setting, setting.numeric.clone()))
                    .collect()
            }
            _ => Vec::new(),
        };
        let readout = rings.iter().find(|r| r.active).map(|r| format!("{} · {}", r.label, r.text));
        let stamp = self.bubble_state.ring.filter(|r| state.workspace.layout.bubbles.show_sliders && r.id == bubble.id && tool_settings::shapes_dab(r.setting)).and_then(|r| {
            let size = ToolbarNumericBinding::BrushSize.field(state)?.value;
            let scale = self.logical_viewport.map_or(1., |v| (state.camera.viewport[0] as f32 / v[0]).max(f32::EPSILON));
            Some(BubbleStamp { center: r.at, diameter: size * state.camera.zoom / scale, color: state.colors.definition() })
        });
        let chooser = self.bubble_state.chooser.filter(|c| c.id == bubble.id).map(|c| self.chooser_items(bubble, c).0).unwrap_or_default();
        let card = (self.bubble_state.card == Some(bubble.id)).then(|| BubbleCard {
            size: self.card_size(),
            metrics: self.bubble_card_metrics().map(|(_, _, measurement, properties_height)| (measurement, properties_height)),
            full: bubble.options.full,
            properties: self.bubble_state.properties,
            disclosure: match self.bubble_state.properties {
                true => state.layer_properties.disclosure.return_label.clone(),
                false => self.localization().text(MessageId::WORKSPACE_PANEL_PROPERTIES).to_string(),
            },
        });
        BubbleView {
            id: bubble.id,
            source: bubble.source,
            kind,
            diameter: bubble.diameter(narrow, state.workspace.layout.bubbles.show_sliders),
            face: bubble_face(kind, bubble.diameter(narrow, state.workspace.layout.bubbles.show_sliders)),
            pair: self.pair_beads(),
            color: (kind == BubbleKind::Color).then(|| self.color_bubble(bubble)),
            label,
            icon,
            preview: match bubble.source {
                BubbleSource::Control { control: ToolbarControl::Brush { id } } => Some(id),
                _ => None,
            },
            variants: kind == BubbleKind::Dial,
            dots: match bubble.source {
                BubbleSource::Medium { group } => tools::group_presets(group).map(|id| id == state.brush.preset).collect(),
                _ => Vec::new(),
            },
            selected,
            enabled,
            open: halo || !chooser.is_empty() || card.is_some(),
            halo: halo.then(|| halo_disc(rings.len())),
            rings,
            readout,
            stamp,
            buttons,
            hints: if kind == BubbleKind::Color { self.bubble_hints(bubble) } else { Vec::new() },
            beads: if kind == BubbleKind::Color { self.bubble_beads(bubble) } else { Vec::new() },
            pill: (kind == BubbleKind::Color && self.bubble_state.pill == Some(bubble.id) && bubble.options.palette)
                .then(|| state.color_library.active_palette().name.clone()),
            chooser,
            card,
            layer: state.layers.iter().find(|l| l.selected).map(|l| l.id),
            layers: state.layers.len(),
            options: bubble.options,
        }
    }

    fn ring_view(&self, id: u32, arc: RingArc, setting: &ToolSetting, numeric: NumericControl) -> BubbleRing {
        let resolved = numeric.resolve(setting.value as f64, NumericOperation::Format).ok();
        BubbleRing {
            arc,
            setting: setting.id,
            label: setting.label.clone(),
            icon: tool_setting_icon(setting.id),
            fill: resolved.as_ref().map_or(0., |r| r.fill as f32),
            text: resolved.map(|r| r.text).unwrap_or_default(),
            active: self.bubble_state.ring.is_some_and(|r| r.id == id && r.setting == setting.id),
            enabled: true,
        }
    }

    fn ring_ids(&self, brush: u32, snapshot: &BrushSnapshot, controls: &[ToolSetting]) -> Vec<&'static str> {
        match self.state.workspace.layout.bubbles.ring(brush) {
            Some(saved) => saved.iter().filter_map(|id| controls.iter().find(|c| c.id == id)).map(|c| c.id).collect(),
            None => tool_settings::ring_defaults(brush, snapshot, controls),
        }
    }

    fn halo_settings(&self) -> Vec<&ToolSetting> {
        if self.state.layer_tools.tool != LayerCanvasTool::Paint {
            return self.state.tool_settings.iter().filter(|c| c.id == "grain_depth").collect();
        }
        let controls = &self.state.tool_settings;
        let ids = self.ring_ids(self.state.brush.preset, self.engine.configured_brush(), controls);
        ids.into_iter().filter_map(|id| controls.iter().find(|c| c.id == id)).collect()
    }

    fn bubble_has_sliders(&self) -> bool {
        self.state.workspace.layout.bubbles.show_sliders && !self.halo_settings().is_empty()
    }

    fn bead_scroll(&self, id: u32) -> [f32; 2] {
        self.bubble_state.scroll.iter().find(|(bubble, _)| *bubble == id).map_or([0.; 2], |(_, scroll)| *scroll)
    }

    fn color_bubble_parts(&self, bubble: &ToolBubble) -> std::cell::Ref<'_, ColorBubbleParts> {
        let library = &self.state.color_library;
        let key = ColorBubbleGeometry { layout: self.color_bubble(bubble), options: bubble.options, pair: self.pair_beads(),
            front: if self.state.display_colors().slot == ColorSlot::Background { Bead::Background } else { Bead::Foreground },
            counts: [library.history.len(), library.active_palette().swatches.len()], scroll: self.bead_scroll(bubble.id) };
        let mut cache = self.bubble_state.color_geometry.borrow_mut();
        if cache.as_ref().is_none_or(|(old, _)| *old != key) {
            let beads = color_beads(key.layout, key.options, key.pair, key.counts[0], key.counts[1], key.scroll, key.front);
            let hints = bead_hints(key.layout, key.options, key.counts[0], key.counts[1], key.scroll);
            let mut collisions: Vec<_> = key.layout.collision_circles().into_iter().map(|[x, y, r]| Shape::circle(r).at([x, y])).collect();
            collisions.extend(beads.iter().map(|b| Shape::circle(b.size * 0.5).at(b.center)));
            *cache = Some((key, ColorBubbleParts { beads, hints, collisions }));
        }
        drop(cache);
        std::cell::Ref::map(self.bubble_state.color_geometry.borrow(), |cache| &cache.as_ref().unwrap().1)
    }

    fn bubble_hints(&self, bubble: &ToolBubble) -> Vec<([f32; 2], f32)> {
        self.color_bubble_parts(bubble).hints.clone()
    }

    fn bubble_beads(&self, bubble: &ToolBubble) -> Vec<BubbleBead> {
        let state = &self.state;
        let library = &state.color_library;
        let swatches = &library.active_palette().swatches;
        let text = |id: MessageId| self.localization().text(id).to_string();
        let colors = state.display_colors();
        self.color_bubble_parts(bubble).beads.iter().copied()
            .map(|placement| {
                let (color, label, selected) = match placement.bead {
                    Bead::Recent { index } => (library.history.get(index).copied(), text(MessageId::WORKSPACE_BUBBLE_RECENT_COLORS), false),
                    Bead::Swatch { index } => {
                        let swatch = swatches.get(index);
                        (swatch.map(|s| s.color), swatch.map(|s| s.name.clone()).unwrap_or_default(), false)
                    }
                    Bead::Foreground => (Some(colors.foreground), text(MessageId::COMMANDS_FOREGROUND_COLOR), colors.slot == ColorSlot::Foreground),
                    Bead::Background => (Some(colors.background), text(MessageId::COMMANDS_BACKGROUND_COLOR), colors.slot == ColorSlot::Background),
                    Bead::Transparent => (
                        Some(layer_core::color::RgbColor { linear_rgb: None, space: colors.rgb_space(), rgba: [0.; 4] }),
                        text(MessageId::COMMANDS_TRANSPARENT_PAINT),
                        colors.slot == ColorSlot::Transparent,
                    ),
                    Bead::Palettes => (None, text(MessageId::WORKSPACE_BUBBLE_PALETTE), self.bubble_state.pill == Some(bubble.id)),
                };
                BubbleBead { placement, color, label, selected }
            })
            .collect()
    }

    fn chooser_items(&self, bubble: &ToolBubble, chooser: Chooser) -> (Vec<ChooserItem>, Vec<Option<ToolVariant>>) {
        let state = &self.state;
        let center = bubble.center(self.bubble_viewport());
        let bounds = self.bubble_state.bounds.map_or_else(|| self.state.workspace.layout.free_area(self.bubble_viewport()), |(_, b)| b);
        let brush_item = |id: u32, at: [f32; 2]| ChooserItem {
            kind: ChooserKind::Brush,
            center: at,
            label: tools::brush_label_localized(id, self.localization()).map(|l| l.to_string()).unwrap_or_default(),
            icon: tools::icon(id),
            preview: Some(id),
            selected: state.layer_tools.tool == LayerCanvasTool::Paint && state.brush.preset == id,
            shown: false,
            enabled: true,
            tag: tag_bounds(at, 0.),
        };
        let column = |group: ToolGroup, medium: Option<([f32; 2], f32, f32)>| {
            let ids: Vec<_> = tools::group_presets(group).collect();
            chooser_column(ids.len(), center, medium, bounds)
                .into_iter()
                .zip(ids)
                .map(|(at, id)| (brush_item(id, at), Some(ToolVariant::BrushPreset { id })))
                .collect::<Vec<_>>()
        };
        let pairs: Vec<(ChooserItem, Option<ToolVariant>)> = match bubble.source {
            BubbleSource::Medium { group } => column(group, None),
            BubbleSource::Control { control } => {
                let Some(group) = control.tool_group() else { return Default::default() };
                let choices = self.group_choices(group, None);
                let shown = chooser.medium.or_else(|| {
                    choices.iter().find_map(|(variant, item)| match variant {
                        ToolVariant::BrushGroup { group } if item.selected => Some(*group),
                        _ => None,
                    })
                });
                let ring = chooser_ring(choices.len(), center, bounds);
                let mut pairs: Vec<_> = choices
                    .into_iter()
                    .zip(ring)
                    .map(|((variant, item), at)| {
                        let medium = match variant {
                            ToolVariant::BrushGroup { group } => Some(group),
                            _ => None,
                        };
                        (
                            ChooserItem {
                                kind: if medium.is_some() { ChooserKind::Medium } else { ChooserKind::Tool },
                                center: at,
                                label: item.label.to_string(),
                                icon: item.icon,
                                preview: None,
                                selected: item.selected,
                                shown: medium.is_some() && medium == shown,
                                enabled: item.enabled,
                                tag: tag_bounds(at, tag_width(&item.label)),
                            },
                            Some(variant),
                        )
                    })
                    .collect();
                let anchor = pairs.iter().find(|(_, variant)| shown.is_some_and(|group| *variant == Some(ToolVariant::BrushGroup { group })));
                if let (Some(group), Some((item, _))) = (shown, anchor) {
                    let reach = pairs.iter().map(|(item, _)| item.center[0].hypot(item.center[1])).fold(0., f32::max);
                    let at = item.center;
                    pairs.extend(column(group, Some((at, reach, item.tag.width))));
                }
                pairs
            }
        };
        pairs.into_iter().unzip()
    }

    fn halo(&self) -> Option<u32> {
        let tools = &self.state;
        self.bubble_state
            .halo
            .filter(|(_, brush)| tools.workspace.layout.bubbles.show_sliders && (tools.layer_tools.tool == LayerCanvasTool::Paint || tools.tool_settings.iter().any(|setting| setting.id == "grain_depth")) && tools.brush.preset == *brush)
            .map(|(id, _)| id)
    }

    pub(crate) fn close_halo(&mut self) -> bool {
        self.bubble_state.ring = None;
        self.bubble_state.halo.take().is_some()
    }

    fn set_halo(&mut self, id: Option<u32>) {
        self.bubble_state.ring = None;
        self.bubble_state.halo = id.map(|id| (id, self.state.brush.preset));
    }

    fn close_bubble_parts(&mut self, keep: Option<u32>) {
        let state = &mut self.bubble_state;
        if state.chooser.is_some_and(|c| Some(c.id) != keep) {
            state.chooser = None;
        }
        if state.pill != keep {
            state.pill = None;
        }
    }

    fn fold_cards(&mut self) {
        let folds = self.bubble_state.card.and_then(|id| self.state.workspace.layout.bubbles.get(id)).is_some_and(|b| b.options.fold);
        if folds {
            self.bubble_state.card = None;
            self.bubble_state.properties = false;
        }
    }

    fn contains_open_part(&self, id: u32, point: [f32; 2]) -> bool {
        let Some(bubble) = self.state.workspace.layout.bubbles.get(id) else { return false };
        let view = self.bubble_view(bubble);
        let center = self.bubble_center(bubble);
        view.hit([point[0] - center[0], point[1] - center[1]]).is_some()
    }

    pub(crate) fn dismiss_bubble_parts(&mut self, position: [f32; 2]) -> bool {
        let state = &self.bubble_state;
        let open: Vec<_> = [state.chooser.map(|c| c.id), state.pill, state.card].into_iter().flatten().collect();
        let mut changed = false;
        for id in open {
            if self.contains_open_part(id, position) {
                continue;
            }
            let before = (self.bubble_state.chooser, self.bubble_state.pill, self.bubble_state.card);
            if self.bubble_state.chooser.is_some_and(|c| c.id == id) {
                self.bubble_state.chooser = None;
            }
            if self.bubble_state.pill == Some(id) {
                self.bubble_state.pill = None;
            }
            if self.bubble_state.card == Some(id) {
                self.fold_cards();
            }
            changed |= before != (self.bubble_state.chooser, self.bubble_state.pill, self.bubble_state.card);
        }
        changed
    }

    pub(crate) fn bubble_stroke(&mut self, phase: PenPhase, surface: [f32; 2]) {
        if self.state.workspace.layout.bubbles.bubbles.is_empty() {
            return;
        }
        let scale = self.logical_viewport.map_or(1., |v| (self.state.camera.viewport[0] as f32 / v[0]).max(f32::EPSILON));
        let point = surface.map(|v| v / scale);
        match phase {
            PenPhase::Down => {
                let before = (self.bubble_state.chooser, self.bubble_state.pill, self.bubble_state.card);
                self.bubble_state.chooser = None;
                self.bubble_state.pill = None;
                self.fold_cards();
                self.bubble_state.stroke = true;
                let cleared = self.bubble_state.desk.fade(None);
                let faded = self.bubble_state.desk.fade(Some(point));
                self.bubble_state.dirty |= cleared || faded || before != (self.bubble_state.chooser, self.bubble_state.pill, self.bubble_state.card);
            }
            PenPhase::Move => {
                self.bubble_state.dirty |= self.bubble_state.desk.fade(Some(point));
            }
            PenPhase::Up | PenPhase::Cancel => {
                self.bubble_state.dirty |= std::mem::replace(&mut self.bubble_state.stroke, false) && !self.bubble_state.desk.faded().is_empty();
            }
            PenPhase::Hover => {}
        }
    }

    pub(crate) fn take_bubble_changes(&mut self) -> u32 {
        if std::mem::take(&mut self.bubble_state.dirty) { regions::BUBBLES } else { 0 }
    }

    pub(crate) fn bubble_action(&mut self, action: BubbleAction) -> Result<UiChange, String> {
        match action {
            BubbleAction::Drag { grab, phase, position, viewport } => self.drag_bubble(grab, phase, position, viewport),
            BubbleAction::Tap { id, position } => self.tap_bubble(id, position),
            BubbleAction::Ring { id, phase, position } => self.bubble_ring(id, phase, position),
            BubbleAction::Wheel { id, phase, part, position } => self.bubble_color(id, phase, Some(part), position),
            BubbleAction::Intensity { id, phase, position } => self.bubble_color(id, phase, None, position),
            BubbleAction::SetIntensity { id, stops } => {
                if self.bubble(id)?.source.kind() != BubbleKind::Color || self.color_bubble(self.bubble(id)?).intensity.is_none() {
                    return Err("This bubble has no intensity control".into());
                }
                if self.state.display_colors().slot == ColorSlot::Transparent { return Ok(UiChange::default()); }
                self.dispatch(UiAction::Color { action: ColorAction::HdrIntensity { stops } })
            },
            BubbleAction::ResetRing { id, position } => {
                let view = self.bubble_view(&self.bubble(id)?.clone());
                let ring = view.rings.iter().find(|r| r.enabled && r.arc.covers(position)).ok_or("No setting under the contact")?;
                let mut change = self.dispatch(UiAction::ResetToolSetting { id: ring.setting.into() })?;
                change.regions |= regions::BUBBLES;
                Ok(change)
            }
            BubbleAction::Beads { id, phase, position } => self.bead_contact(id, phase, position),
            BubbleAction::Choose { id, phase, position } => self.choose(id, phase, position),
            BubbleAction::Palette { id, step } => self.cycle_palette(id, step),
            BubbleAction::MeasureCard { id, measurement, properties_height } => {
                if self.bubble_state.card != Some(id) || ![measurement.fixed_height,measurement.unit_height,properties_height]
                    .into_iter().all(|value|value.is_finite()&&value>=0.) || measurement.unit_height==0. {
                    return Err("Invalid card measurement".into());
                }
                let full=self.bubble(id)?.options.full;
                let metrics=(id,full,measurement,properties_height);
                if self.bubble_state.card_metrics.replace(metrics) == Some(metrics) {
                    return Ok(UiChange::default());
                }
                self.sync_bubbles();
                Ok(self.changed(regions::BUBBLES, false))
            }
            BubbleAction::Properties { id } => {
                if self.bubble_state.card != Some(id) {
                    return Err("Open the Layers card first".into());
                }
                self.bubble_state.properties = !self.bubble_state.properties;
                Ok(self.changed(regions::BUBBLES, false))
            }
            BubbleAction::Close { id } => {
                if self.bubble_state.card != Some(id) {
                    return Ok(UiChange::default());
                }
                self.bubble_state.card = None;
                self.bubble_state.properties = false;
                self.sync_bubbles();
                Ok(self.changed(regions::BUBBLES, false))
            }
            BubbleAction::PutBack { id } => {
                let source = self.bubble(id)?.source;
                self.edit_bubbles(LayoutChange::Bubble { action: LayoutPanelAction::Deleted, source }, |layout| {
                    layout.bubbles.put_back(|b| b.id == id);
                    Ok(())
                })
            }
            BubbleAction::PutBackAll => {
                let source = self.state.workspace.layout.bubbles.bubbles.first().ok_or("No bubbles are out")?.source;
                self.edit_bubbles(LayoutChange::Bubble { action: LayoutPanelAction::Deleted, source }, |layout| {
                    layout.bubbles.put_back(|_| true);
                    Ok(())
                })
            }
            BubbleAction::ToggleSliders { id } => {
                let source = self.bubble(id)?.source;
                let change = self.edit_bubbles(LayoutChange::Bubble { action: LayoutPanelAction::Customized, source }, |layout| {
                    layout.bubbles.show_sliders = !layout.bubbles.show_sliders;
                    Ok(())
                })?;
                if self.bubble_has_sliders() {
                    let target = std::iter::once(self.bubble(id)?).chain(self.state.workspace.layout.bubbles.bubbles.iter())
                        .find(|bubble| matches!(bubble.source.kind(), BubbleKind::Dial | BubbleKind::Brush | BubbleKind::Medium) && self.bubble_view(bubble).selected)
                        .map(|bubble| bubble.id);
                    if let Some(id) = target {
                        self.close_bubble_parts(None);
                        self.set_halo(Some(id));
                        self.sync_bubbles();
                    }
                }
                Ok(change)
            }
            BubbleAction::Option { id, option } => {
                let options = option.apply(self.bubble(id)?.options);
                self.bubble_action(BubbleAction::SetOptions { id, options })
            }
            BubbleAction::SetOptions { id, options } => {
                let source = self.bubble(id)?.source;
                self.edit_bubbles(LayoutChange::Bubble { action: LayoutPanelAction::Customized, source }, |layout| {
                    layout.bubbles.get_mut(id)?.options = options;
                    Ok(())
                })
            }
            BubbleAction::Resize { id, scale } => {
                let Some(bubble) = self.state.workspace.layout.bubbles.get(id) else { return Ok(UiChange::default()) };
                if bubble.source.kind() != BubbleKind::Color || !scale.is_finite() || scale <= 0. {
                    return Err("Only the color ring changes size".into());
                }
                let (source, options) = (bubble.source, BubbleOptions { size: pinched_size(bubble.options.size, scale), ..bubble.options });
                self.edit_bubbles(LayoutChange::Bubble { action: LayoutPanelAction::Resized, source }, |layout| {
                    layout.bubbles.get_mut(id)?.options = options;
                    Ok(())
                })
            }
            BubbleAction::ToggleRingSetting { brush, setting } => {
                let preset = tools::preset(brush)?;
                let snapshot = self.tools.brush(preset);
                let controls = tool_settings::controls(brush, &snapshot, self.localization());
                let mut shown = self.state.workspace.layout.bubbles.ring(brush).map_or_else(
                    || self.ring_ids(brush, &snapshot, &controls).into_iter().map(str::to_owned).collect(),
                    <[String]>::to_vec);
                let setting = controls.iter().find(|c| c.id == setting).ok_or("This setting is not used by the brush")?.id;
                if let Some(index) = shown.iter().position(|id| *id == setting) {
                    shown.remove(index);
                } else if shown.len() < MAX_RING_SETTINGS {
                    shown.push(setting.into());
                } else {
                    return Err(self.localization().text(MessageId::WORKSPACE_BUBBLE_RING_FULL).to_string());
                }
                let source = BubbleSource::Control { control: ToolbarControl::Brush { id: brush } };
                self.edit_bubbles(LayoutChange::Bubble { action: LayoutPanelAction::Customized, source }, |layout| {
                    layout.bubbles.set_ring(brush, shown);
                    Ok(())
                })
            }
        }
    }

    fn bubble(&self, id: u32) -> Result<&ToolBubble, String> {
        self.state.workspace.layout.bubbles.get(id).ok_or_else(|| "The bubble no longer exists".into())
    }

    fn edit_bubbles(&mut self, description: LayoutChange, edit: impl FnOnce(&mut DockLayout) -> Result<(), String>) -> Result<UiChange, String> {
        if self.bubble_state.moving() {
            return Err("Finish moving the bubble first".into());
        }
        let mut before = self.state.workspace.clone();
        self.state.customization.committed_header(&mut before.layout);
        let mut layout = self.state.workspace.layout.clone();
        edit(&mut layout)?;
        layout.validate()?;
        self.state.workspace.layout = layout;
        self.workspace_history.record_named(before, &self.state.workspace, description);
        self.sync_bubbles();
        Ok(self.changed(regions::BUBBLES | regions::COMMANDS | regions::LAYOUT, false))
    }

    fn select_bubble(&mut self, bubble: &ToolBubble) -> Result<UiChange, String> {
        let action = match bubble.source {
            BubbleSource::Medium { group } => UiAction::SelectBrushSet { group },
            BubbleSource::Control { control: ToolbarControl::Brush { id } } => UiAction::SelectBrush { id },
            BubbleSource::Control { control } => {
                let group = control.tool_group().ok_or("This tool is unavailable")?;
                self.variant_action(self.group_variant(group))
            }
        };
        let change = self.dispatch(action)?;
        self.remember_tool_slots();
        Ok(change)
    }

    fn tap_bubble(&mut self, id: u32, position: [f32; 2]) -> Result<UiChange, String> {
        let bubble = self.bubble(id)?.clone();
        let view = self.bubble_view(&bubble);
        let opened = |s: &mut Self| {
            s.sync_bubbles();
            Ok(s.changed(regions::BUBBLES, false))
        };
        match view.kind {
            BubbleKind::Color => return Ok(UiChange::default()),
            BubbleKind::Controls | BubbleKind::History => {
                let action = match view.kind {
                    BubbleKind::Controls if position.into_iter().all(|v| v.abs() <= TOOL_BUBBLE * 0.5) => ToolbarControl::ColorPicker.action(),
                    BubbleKind::History => history_button(position)
                        .filter(|&i| view.buttons[i].enabled)
                        .map(|i| UiAction::Invoke { command: HISTORY_COMMANDS[i] }),
                    _ => None,
                };
                let Some(action) = action else { return Ok(UiChange::default()) };
                self.close_bubble_parts(None);
                let mut change = self.dispatch(action)?;
                change.regions |= regions::BUBBLES;
                return Ok(change);
            }
            BubbleKind::Layers if self.bubble_state.card == Some(id) => return Ok(UiChange::default()),
            BubbleKind::Layers => {
                self.close_bubble_parts(None);
                self.bubble_state.card = Some(id);
                self.bubble_state.properties = false;
                return opened(self);
            }
            BubbleKind::Dial | BubbleKind::Brush | BubbleKind::Medium => {}
        }
        if !view.enabled {
            return Err(self.localization().text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET).to_string());
        }
        let chooser = self.bubble_state.chooser.is_some_and(|c| c.id == id);
        let halo = self.halo() == Some(id);
        self.close_bubble_parts(Some(id));
        if !view.selected {
            self.close_bubble_parts(None);
            let mut change = self.select_bubble(&bubble)?;
            if self.bubble_has_sliders() {
                self.set_halo(Some(id));
            }
            self.sync_bubbles();
            change.regions |= regions::BUBBLES;
            return Ok(change);
        }
        if chooser {
            self.bubble_state.chooser = None;
        } else if halo {
            self.set_halo(None);
            if view.kind != BubbleKind::Brush {
                self.bubble_state.chooser = Some(Chooser { id, medium: None });
            }
        } else if self.bubble_has_sliders() {
            self.set_halo(Some(id));
        } else if view.kind != BubbleKind::Brush {
            self.bubble_state.chooser = Some(Chooser { id, medium: None });
        }
        opened(self)
    }

    fn choose(&mut self, id: u32, phase: ContactPhase, position: [f32; 2]) -> Result<UiChange, String> {
        let Some(chooser) = self.bubble_state.chooser.filter(|c| c.id == id) else { return Ok(UiChange::default()) };
        let bubble = self.bubble(id)?.clone();
        let (items, variants) = self.chooser_items(&bubble, chooser);
        let hit = items.iter().position(|item| item.contains(position) && item.enabled);
        let medium = hit.and_then(|i| match variants[i] {
            Some(ToolVariant::BrushGroup { group }) => Some(group),
            _ => None,
        });
        match phase {
            ContactPhase::Down | ContactPhase::Move => {
                if let Some(group) = medium
                    && chooser.medium != Some(group)
                {
                    self.bubble_state.chooser = Some(Chooser { id, medium: Some(group) });
                    return Ok(self.changed(regions::BUBBLES, false));
                }
                Ok(UiChange::default())
            }
            ContactPhase::Up => match hit.and_then(|i| variants[i]) {
                Some(ToolVariant::BrushGroup { group }) => {
                    self.bubble_state.chooser = Some(Chooser { id, medium: Some(group) });
                    Ok(self.changed(regions::BUBBLES, false))
                }
                Some(variant) => {
                    self.bubble_state.chooser = None;
                    let mut change = self.dispatch(self.variant_action(variant))?;
                    self.remember_tool_slots();
                    if self.bubble_has_sliders() {
                        self.set_halo(Some(id));
                    }
                    self.sync_bubbles();
                    change.regions |= regions::BUBBLES;
                    Ok(change)
                }
                None => Ok(UiChange::default()),
            },
            ContactPhase::Cancel => Ok(UiChange::default()),
        }
    }

    fn bubble_color(&mut self, id: u32, phase: ContactPhase, part: Option<ColorWheelPart>, position: [f32; 2]) -> Result<UiChange, String> {
        if !position.into_iter().all(f32::is_finite) {
            return Err("Invalid wheel position".into());
        }
        if phase == ContactPhase::Down {
            if self.bubble_state.wheel.is_some() { return Err("A color wheel contact is already active".into()); }
            if self.bubble(id)?.source.kind() != BubbleKind::Color { return Err("This bubble has no color wheel".into()); }
            let colors = if self.selection_masks.target().is_some() { &self.selection_masks.colors } else { &self.state.colors };
            if part.is_none() && (self.color_bubble(self.bubble(id)?).intensity.is_none() || colors.slot == ColorSlot::Transparent) { return Ok(UiChange::default()); }
            let wheel = WheelContact { id, context: self.color_wheel_context(),
                intensity: colors.hdr_depth().is_float().then(|| colors.hdr_intensity()), part };
            self.dispatch(UiAction::ColorContact { phase })?;
            self.bubble_state.wheel = Some(wheel);
        }
        let Some(wheel) = self.bubble_state.wheel.filter(|wheel| wheel.id == id) else { return Ok(UiChange::default()); };
        let phase = if wheel.context == self.color_wheel_context() { phase } else { ContactPhase::Up };
        if matches!(phase, ContactPhase::Cancel | ContactPhase::Up) {
            self.bubble_state.wheel = None;
            let mut change = self.dispatch(UiAction::ColorContact { phase })?;
            change.regions |= regions::BUBBLES;
            return Ok(change);
        }
        let action = if let Some(part) = wheel.part {
            let size = self.color_bubble(self.bubble(id)?).wheel_size();
            ColorAction::PickWheel { part, point: position.map(|v| v + size / 2.), size }
        } else {
            let arc = self.color_bubble(self.bubble(id)?).intensity.ok_or("This bubble has no intensity control")?;
            let colors = self.state.display_colors();
            ColorAction::HdrIntensity { stops: arc.value(position, self.bubble_intensity_range(id, colors)) }
        };
        let mut change = self.dispatch(UiAction::Color { action })?;
        change.regions |= regions::BUBBLES;
        Ok(change)
    }

    fn bubble_ring(&mut self, id: u32, phase: ContactPhase, position: [f32; 2]) -> Result<UiChange, String> {
        if !position.into_iter().all(f32::is_finite) {
            return Err("Invalid ring position".into());
        }
        if phase == ContactPhase::Cancel {
            let Some(ring) = self.bubble_state.ring.take() else {
                return Ok(self.changed(regions::BUBBLES, false));
            };
            let binding = self.ring_binding(&self.bubble(ring.id)?.clone(), ring.setting);
            let mut change = self.set_ring(binding, ring.setting, ring.before)?;
            change.regions |= regions::BUBBLES;
            return Ok(change);
        }
        let bubble = self.bubble(id)?.clone();
        let view = self.bubble_view(&bubble);
        let setting = match phase {
            ContactPhase::Down => {
                let rings: Vec<_> = view.rings.iter().filter(|r| r.enabled).collect();
                let arcs: Vec<_> = rings.iter().map(|r| r.arc).collect();
                rings[ring_at(&arcs, position).ok_or("No setting under the contact")?].setting
            }
            _ => match self.bubble_state.ring {
                Some(ring) if ring.id == id => ring.setting,
                _ => return Ok(UiChange::default()),
            },
        };
        let ring = view.rings.iter().find(|r| r.setting == setting).ok_or("This setting is not used by the selected tool")?;
        let fraction = ring.arc.fraction(position) as f64;
        let binding = self.ring_binding(&bubble, setting);
        let field = match binding {
            Some(binding) => binding.field(&self.state).ok_or("This slider is unavailable")?,
            None => self.state.tool_settings.iter().find(|s| s.id == setting).ok_or("This setting is not used by the selected tool")?.clone(),
        };
        let before = match self.bubble_state.ring {
            Some(ring) if phase != ContactPhase::Down => ring.before,
            _ => field.value,
        };
        self.bubble_state.ring = (phase != ContactPhase::Up).then_some(RingContact { id, setting, before, at: position });
        let value = field
            .numeric
            .resolve(field.value as f64, NumericOperation::Position { position: fraction })
            .map_err(|reason| reason.message(&self.state.localization))?
            .value as f32;
        let mut change = self.set_ring(binding, setting, value)?;
        change.regions |= regions::BUBBLES;
        Ok(change)
    }

    fn ring_binding(&self, bubble: &ToolBubble, setting: &str) -> Option<ToolbarNumericBinding> {
        (bubble.source.kind() == BubbleKind::Controls)
            .then(|| CONTROL_RINGS.into_iter().find(|b| b.field(&self.state).is_some_and(|f| f.id == setting)))
            .flatten()
    }

    fn set_ring(&mut self, binding: Option<ToolbarNumericBinding>, setting: &'static str, value: f32) -> Result<UiChange, String> {
        self.dispatch(match binding {
            Some(binding) => binding.action(value),
            None => UiAction::SetToolSetting { id: setting.into(), value },
        })
    }

    fn bead_contact(&mut self, id: u32, phase: ContactPhase, position: [f32; 2]) -> Result<UiChange, String> {
        let bubble = self.bubble(id)?.clone();
        let size = bubble.diameter(self.narrow(), self.state.workspace.layout.bubbles.show_sliders);
        let library = &self.state.color_library;
        let (recent, swatches) = (library.history.len(), library.active_palette().swatches.len());
        match phase {
            ContactPhase::Down => {
                let beads: Vec<_> = self.bubble_beads(&bubble).into_iter().map(|b| b.placement).collect();
                let strip = bead_at(&beads, position).and_then(Bead::strip);
                self.bubble_state.beads = Some(BeadContact { id, strip, start: position, last: position, moved: false });
                Ok(UiChange::default())
            }
            ContactPhase::Move => {
                let Some(mut contact) = self.bubble_state.beads.filter(|c| c.id == id) else { return Ok(UiChange::default()) };
                contact.moved |= (position[0] - contact.start[0]).hypot(position[1] - contact.start[1]) > TAP_SLOP;
                let mut regions = 0;
                if let (true, Some(strip)) = (contact.moved, contact.strip) {
                    let limit = bead_scroll_limit(strip, size, if strip == BeadStrip::Recent { recent } else { swatches });
                    let mut scroll = self.bead_scroll(id);
                    let axis = usize::from(strip == BeadStrip::Palette);
                    scroll[axis] = (scroll[axis] + bead_travel(strip, size, contact.last, position)).clamp(0., limit);
                    self.bubble_state.scroll.retain(|(bubble, _)| *bubble != id);
                    self.bubble_state.scroll.push((id, scroll));
                    regions = regions::BUBBLES;
                }
                contact.last = position;
                self.bubble_state.beads = Some(contact);
                Ok(self.changed(regions, false))
            }
            ContactPhase::Up | ContactPhase::Cancel => {
                let Some(contact) = self.bubble_state.beads.take().filter(|c| c.id == id) else { return Ok(UiChange::default()) };
                if contact.moved || phase == ContactPhase::Cancel {
                    if let Some((_, scroll)) = self.bubble_state.scroll.iter_mut().find(|(bubble, _)| *bubble == id) {
                        *scroll = scroll.map(f32::round);
                    }
                    return Ok(self.changed(regions::BUBBLES, false));
                }
                let beads: Vec<_> = self.bubble_beads(&bubble).into_iter().map(|b| b.placement).collect();
                match bead_at(&beads, position) {
                    Some(bead) => self.bead_action(id, bead),
                    None => Ok(UiChange::default()),
                }
            }
        }
    }

    fn bead_action(&mut self, id: u32, bead: Bead) -> Result<UiChange, String> {
        let library = &self.state.color_library;
        let action = match bead {
            Bead::Recent { index } => ColorAction::Definition { color: *library.history.get(index).ok_or("The color is no longer in the history")? },
            Bead::Swatch { index } => ColorAction::Library {
                action: ColorLibraryAction::Use { id: library.active_palette().swatches.get(index).ok_or("The color is no longer in the palette")?.id },
            },
            Bead::Foreground => ColorAction::Select { slot: ColorSlot::Foreground },
            Bead::Background => ColorAction::Select { slot: ColorSlot::Background },
            Bead::Transparent => ColorAction::Select { slot: ColorSlot::Transparent },
            Bead::Palettes => {
                self.bubble_state.pill = (self.bubble_state.pill != Some(id)).then_some(id);
                return Ok(self.changed(regions::BUBBLES, false));
            }
        };
        self.bubble_state.pill = None;
        let mut change = self.dispatch(UiAction::Color { action })?;
        change.regions |= regions::BUBBLES;
        Ok(change)
    }

    fn cycle_palette(&mut self, id: u32, step: i32) -> Result<UiChange, String> {
        self.bubble(id)?;
        let library = &self.state.color_library;
        let count = library.palettes.len() as i32;
        let current = library.palettes.iter().position(|p| p.id == library.active).unwrap_or(0) as i32;
        let next = library.palettes[(current + step).rem_euclid(count.max(1)) as usize].id;
        if let Some((_, scroll)) = self.bubble_state.scroll.iter_mut().find(|(bubble, _)| *bubble == id) {
            scroll[1] = 0.;
        }
        let mut change = self.dispatch(UiAction::Color { action: ColorAction::Library { action: ColorLibraryAction::SelectPalette { id: next } } })?;
        change.regions |= regions::BUBBLES;
        Ok(change)
    }

    pub(crate) fn bubble_palette_menu(&self, id: u32) -> Result<ContextMenu, String> {
        self.bubble(id)?;
        let library = &self.state.color_library;
        let mut rows: Vec<_> = library
            .palettes
            .iter()
            .map(|palette| {
                let mut item = ContextMenuItem::command(
                    palette.name.clone(),
                    UiAction::Color { action: ColorAction::Library { action: ColorLibraryAction::SelectPalette { id: palette.id } } },
                );
                item.selected = Some(palette.id == library.active);
                item
            })
            .collect();
        rows.push(ContextMenuItem::edit(
            self.localization().text(MessageId::WORKSPACE_BUBBLE_EDIT_PALETTES).to_string(),
            CustomizationAction::SetPanelVisible { panel: Panel::Palettes, visible: true },
        ));
        Ok(ContextMenu { title: self.localization().text(MessageId::WORKSPACE_BUBBLE_PALETTE).to_string(), sections: vec![rows] })
    }

    fn bubble_home(&mut self, viewport: [f32; 2], point: [f32; 2]) -> bool {
        let hidden = self.interaction.hidden;
        let header = self.state.workspace.layout.header_presentation.height.max(HEADER_HEIGHT);
        (!hidden && point[1] < header)
            || self.interaction.facts.content_drawer.is_some_and(|b|b.contains(point[0],point[1]))
            || self.interaction.facts.drawer_connection.is_some_and(|b|b.contains(point[0],point[1]))
            || self.interaction.facts.expanded_panel.is_some_and(|p|p.bounds.contains(point[0],point[1]))
            || self.state.customization.column_drawer_bounds.iter().any(|m|m.bounds.contains(point[0],point[1]))
            || self.layout(viewport).groups.iter().any(|g| {
                (!hidden || g.floating || self.state.workspace.layout.shows_in_zen(g.id)) && g.bounds.contains(point[0], point[1])
            })
    }

    fn bubble_homing(&mut self, viewport: [f32; 2], point: [f32; 2], source: Option<Bounds>) -> bool {
        self.bubble_home(viewport, point) || source.is_some_and(|s| near(s, point, crate::layout::WORKSPACE_PROXIMITY))
    }

    fn grab_source(&self, grab: BubbleGrab, viewport: [f32; 2]) -> Result<(BubbleSource, Option<Bounds>), String> {
        let layout = &self.state.workspace.layout;
        let unavailable = || self.localization().text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET).to_string();
        let control = |anchor| layout.anchor_control(anchor).and_then(BubbleSource::for_control).ok_or_else(unavailable);
        Ok(match grab {
            BubbleGrab::Bubble { id } | BubbleGrab::Rim { id } => (self.bubble(id)?.source, None),
            BubbleGrab::Tile { panel, tile } => (
                control(DrawerAnchor::Tile { panel, tile })?,
                self.layout(viewport).groups.iter().find(|g| g.panels.contains(&panel)).map(|g| g.bounds),
            ),
            BubbleGrab::Header { id } => (
                control(DrawerAnchor::Header { id })?,
                Some(Bounds { x: 0., y: 0., width: viewport[0], height: layout.header_presentation.height.max(HEADER_HEIGHT) }),
            ),
            BubbleGrab::Variant { variant } => (BubbleSource::for_variant(variant).ok_or_else(unavailable)?, None),
        })
    }

    fn drag_bubble(&mut self, grab: BubbleGrab, phase: ContactPhase, position: [f32; 2], viewport: [f32; 2]) -> Result<UiChange, String> {
        valid_viewport(viewport)?;
        if !position.into_iter().all(f32::is_finite) {
            return Err("Invalid drag position".into());
        }
        if phase == ContactPhase::Down {
            if self.bubble_state.moving() || self.workspace_drag.is_some() {
                return Err("Another item is already moving".into());
            }
            if !matches!(grab, BubbleGrab::Bubble { .. } | BubbleGrab::Rim { .. }) && !self.can_take_bubble(grab, position, viewport) {
                self.grab_source(grab, viewport)?;
                self.bubble_state.pending_drag=Some(grab);
                return Ok(UiChange::default());
            }
            return self.lift_bubble(grab, position, viewport);
        }
        let mut change = UiChange::default();
        if let Some(pending)=self.bubble_state.pending_drag {
            if pending!=grab{return Ok(UiChange::default());}
            if matches!(phase,ContactPhase::Move|ContactPhase::Up) && self.can_take_bubble(pending,position,viewport) {
                self.bubble_state.pending_drag=None;
                change = self.lift_bubble(pending,position,viewport)?;
            } else {
                if matches!(phase,ContactPhase::Up|ContactPhase::Cancel){self.bubble_state.pending_drag=None;}
                return Ok(UiChange::default());
            }
        }
        let Some(drag) = self.bubble_state.drag else { return Ok(UiChange::default()) };
        let exiting = matches!(phase, ContactPhase::Move | ContactPhase::Up) && viewport == drag.viewport
            && (0..2).any(|axis| position[axis] <= EDGE_DISMISS_DISTANCE && position[axis] < drag.position[axis]
                || position[axis] >= viewport[axis] - EDGE_DISMISS_DISTANCE && position[axis] > drag.position[axis]);
        if let Some(drag) = self.bubble_state.drag.as_mut() {
            drag.position = position;
            drag.viewport = viewport;
        }
        let phase = if exiting { ContactPhase::Up } else { phase };
        let center = [position[0] + drag.offset[0], position[1] + drag.offset[1]];
        let present = self.state.workspace.layout.bubbles.get(drag.id).is_some();
        match phase {
            ContactPhase::Move if present => {
                self.bubble_state.desk.drag(center);
                let homing = self.bubble_homing(viewport, position, drag.source);
                self.bubble_state.desk.home(homing);
                return Ok(change);
            }
            ContactPhase::Up if present => {
                self.bubble_state.drag = None;
                self.bubble_state.drag_desk = None;
                self.bubble_state.desk.home(false);
                if exiting || self.bubble_homing(viewport, position, drag.source)
                    || drag.pulled && !self.bubble_canvas(viewport).contains(position[0],position[1]) {
                    let source = self.bubble(drag.id)?.source;
                    let before = self.workspace_history.gesture_start().cloned();
                    self.workspace_history.cancel(&mut self.state.workspace);
                    self.bubble_state.desk.release();
                    if let Some(before) = before {
                        self.state.workspace.layout.bubbles.put_back(|b| b.id == drag.id);
                        self.workspace_history.record_named(before, &self.state.workspace, LayoutChange::Bubble { action: LayoutPanelAction::Deleted, source });
                    }
                } else {
                    self.bubble_state.desk.drag(center);
                    let viewport = self.bubble_viewport();
                    for (id, rest) in self.bubble_state.desk.settled() {
                        if let Ok(bubble) = self.state.workspace.layout.bubbles.get_mut(id) {
                            bubble.place(rest, viewport);
                        }
                    }
                    self.bubble_state.desk.release();
                    self.workspace_history.finish(&self.state.workspace);
                    if drag.pulled {
                        change = merge_change(change, self.open_pulled(drag.id)?);
                    }
                }
            }
            _ => {
                self.bubble_state.drag = None;
                if let Some(before) = self.bubble_state.drag_desk.take() { self.bubble_state.desk.cancel(before); }
                self.workspace_history.cancel(&mut self.state.workspace);
            }
        }
        self.sync_bubbles();
        Ok(merge_change(change, self.changed(regions::BUBBLES | regions::COMMANDS, false)))
    }

    fn open_pulled(&mut self, id: u32) -> Result<UiChange, String> {
        let bubble = self.bubble(id)?.clone();
        match bubble.source.kind() {
            BubbleKind::Dial | BubbleKind::Brush | BubbleKind::Medium => {
                self.close_bubble_parts(None);
                let change = if self.bubble_view(&bubble).selected { UiChange::default() } else { self.select_bubble(&bubble)? };
                if self.bubble_has_sliders() && self.bubble_view(&bubble).selected {
                    self.set_halo(Some(id));
                }
                Ok(change)
            }
            _ => Ok(UiChange::default()),
        }
    }

    fn lift_bubble(&mut self, grab: BubbleGrab, position: [f32; 2], viewport: [f32; 2]) -> Result<UiChange, String> {
        if self.bubble_state.drag.is_some() || self.workspace_drag.is_some() {
            return Err("Another item is already moving".into());
        }
        let (source, bounds) = self.grab_source(grab, viewport)?;
        let desk = self.bubble_state.desk.clone();
        let existing = match grab {
            BubbleGrab::Bubble { id } | BubbleGrab::Rim { id } => Some(id),
            _ => self.state.workspace.layout.bubbles.find(source).map(|b| b.id),
        };
        self.workspace_history.begin_named(
            &self.state.workspace,
            LayoutChange::Bubble { action: if existing.is_some() { LayoutPanelAction::Moved } else { LayoutPanelAction::Added }, source },
        );
        let id = match existing {
            Some(id) => id,
            None => {
                let id = self.state.workspace.layout.allocate()?;
                let viewport = self.bubble_viewport();
                let mut bubble = ToolBubble::new(id, source, position, viewport);
                bubble.options = self.state.workspace.layout.bubbles.options_for(source);
                self.state.workspace.layout.bubbles.bubbles.push(bubble);
                id
            }
        };
        let center = match grab {
            BubbleGrab::Bubble { .. } => self.bubble_state.desk.center(id).unwrap_or(position),
            BubbleGrab::Rim { .. } => {
                let center = self.bubble_state.desk.center(id).unwrap_or(position);
                let radius = self.bubble(id)?.diameter(self.narrow(), self.state.workspace.layout.bubbles.show_sliders) * 0.5;
                let away = [position[0] - center[0], position[1] - center[1]];
                let length = away[0].hypot(away[1]).max(1e-3);
                [position[0] - away[0] / length * radius, position[1] - away[1] / length * radius]
            }
            _ => position,
        };
        let pulled = !matches!(grab, BubbleGrab::Bubble { .. } | BubbleGrab::Rim { .. });
        let mut closes = pulled && self.state.customization.drawer.take().is_some();
        if let BubbleGrab::Tile { panel, .. } = grab {
            self.state.customization.column_drawers.retain(|drawer| {
                let source = drawer.panels().any(|p| p == panel);
                closes |= source;
                !source
            });
        }
        self.close_bubble_parts(None);
        if pulled && source.kind() == BubbleKind::Layers {
            self.bubble_state.card = Some(id);
            self.bubble_state.properties = false;
        }
        self.bubble_state.drag_desk = Some(desk);
        self.bubble_state.drag = Some(BubbleDrag { id, offset: [center[0] - position[0], center[1] - position[1]], source: bounds, pulled, position, viewport });
        self.sync_bubbles();
        self.bubble_state.desk.grab(id);
        if matches!(grab, BubbleGrab::Rim { .. }) {
            self.bubble_state.desk.catch_up();
        }
        self.bubble_state.desk.drag(center);
        let homing = self.bubble_homing(viewport, position, bounds);
        self.bubble_state.desk.home(homing);
        Ok(self.changed(regions::BUBBLES | if closes { regions::CUSTOMIZATION } else { 0 }, false))
    }

    pub(crate) fn bubble_menu(&self, id: u32) -> Result<ContextMenu, String> {
        let bubble = self.bubble(id)?;
        let localization = self.localization();
        let text = |id: MessageId| localization.text(id).to_string();
        let action = |action| UiAction::Bubble { action };
        let options = |label: MessageId, checked: bool, option: BubbleOption| {
            let mut item = ContextMenuItem::command(text(label), action(BubbleAction::Option { id, option }));
            item.selected = Some(checked);
            item
        };
        let o = bubble.options;
        let mut sections = Vec::new();
        let current = (self.state.layer_tools.tool == LayerCanvasTool::Paint).then_some(self.state.brush.preset);
        let view = self.bubble_view(bubble);
        match bubble.source {
            BubbleSource::Control { control: ToolbarControl::Brush { id: brush } } => sections.extend(self.ring_menu(brush)),
            BubbleSource::Control { control: ToolbarControl::Color } => {
                sections.push(self.paint_history_items());
                sections.push(vec![
                    options(MessageId::WORKSPACE_BUBBLE_RECENT_COLORS, o.recent, BubbleOption::Recent),
                    options(MessageId::WORKSPACE_BUBBLE_PALETTE, o.palette, BubbleOption::Palette),
                    options(MessageId::WORKSPACE_BUBBLE_COLOR_PAIR, o.pair, BubbleOption::Pair),
                ]);
                sections.push(
                    [MessageId::WORKSPACE_SIZE_SMALL, MessageId::WORKSPACE_SIZE_MEDIUM, MessageId::WORKSPACE_SIZE_LARGE]
                        .into_iter()
                        .zip(COLOR_RING_SIZES)
                        .map(|(label, size)| options(label, o.size == size, BubbleOption::Size { size }))
                        .collect(),
                );
            }
            BubbleSource::Control { control: ToolbarControl::ColorPicker } => {
                sections.push(vec![options(MessageId::WORKSPACE_BUBBLE_SWAP_SLIDERS, o.swap, BubbleOption::Swap)]);
            }
            BubbleSource::Control { control: ToolbarControl::Panel { .. } } => {
                sections.push(vec![
                    options(MessageId::WORKSPACE_BUBBLE_SIMPLE, !o.full, BubbleOption::Full { full: false }),
                    options(MessageId::WORKSPACE_BUBBLE_FULL, o.full, BubbleOption::Full { full: true }),
                ]);
                sections.push(vec![options(MessageId::WORKSPACE_BUBBLE_FOLD, o.fold, BubbleOption::Fold)]);
                if self.bubble_state.card == Some(id) {
                    sections.push(vec![ContextMenuItem::command(text(MessageId::COMMON_CLOSE), action(BubbleAction::Close { id }))]);
                }
            }
            BubbleSource::Control { .. } | BubbleSource::Medium { .. }
                if matches!(view.kind, BubbleKind::Dial | BubbleKind::Brush | BubbleKind::Medium) && view.selected =>
            {
                sections.extend(current.and_then(|brush| self.ring_menu(brush)));
            }
            _ => {}
        }
        if matches!(view.kind, BubbleKind::Dial | BubbleKind::Brush | BubbleKind::Medium | BubbleKind::Controls) {
            let mut item = ContextMenuItem::command(text(MessageId::WORKSPACE_SHOW_SLIDERS_MENU), action(BubbleAction::ToggleSliders { id }));
            item.selected = Some(self.state.workspace.layout.bubbles.show_sliders);
            sections.push(vec![item]);
        }
        sections.push(vec![ContextMenuItem::command(text(MessageId::WORKSPACE_BUBBLE_PUT_BACK), action(BubbleAction::PutBack { id }))]);
        let title = self.tool_name(bubble, &view).unwrap_or(view.label);
        sections[0].insert(0, ContextMenuItem::heading(&title));
        Ok(ContextMenu { title, sections })
    }

    fn tool_name(&self, bubble: &ToolBubble, view: &BubbleView) -> Option<String> {
        let brush = |id: u32| tools::brush_label_localized(id, self.localization()).map(|label| label.to_string());
        match (view.kind, bubble.source) {
            (BubbleKind::Medium, BubbleSource::Medium { group }) => brush(self.tools.group(group)),
            (BubbleKind::Dial, BubbleSource::Control { control }) => match self.group_variant(control.tool_group()?) {
                ToolVariant::BrushGroup { group } => brush(self.tools.group(group)),
                ToolVariant::BrushPreset { id } => brush(id),
                variant => Some(variant.label(self.localization())),
            },
            _ => None,
        }
    }

    fn ring_menu(&self, brush: u32) -> Option<Vec<ContextMenuItem>> {
        let snapshot = self.tools.brush(tools::preset(brush).ok()?);
        let controls = tool_settings::controls(brush, &snapshot, self.localization());
        let shown = self.ring_ids(brush, &snapshot, &controls);
        let count = self.state.workspace.layout.bubbles.ring(brush).map_or(shown.len(), <[String]>::len);
        let rows = controls
            .iter()
            .map(|c| {
                let on = shown.contains(&c.id);
                let mut item = ContextMenuItem::command(
                    c.label.to_string(),
                    UiAction::Bubble { action: BubbleAction::ToggleRingSetting { brush, setting: c.id.into() } },
                );
                item.icon = Some(tool_setting_icon(c.id).into());
                item.selected = Some(on);
                item.enabled = on || count < MAX_RING_SETTINGS;
                item
            })
            .collect();
        Some(rows)
    }
}
