use super::{Workspace, allocate_at};
use crate::layers::LayerPanel;
use adw::prelude::*;
use gtk::{gdk, glib, graphene, gsk, subclass::prelude::*};
use layer_ui::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const HOLD_SLOP: f32 = 8.;
const FADE_RETURN_MS: u64 = 180;
const FADE_US: i64 = 140_000;
const UNFOLD_US: i64 = 200_000;
const ITEM_IN_US: i64 = 180_000;
const ITEM_OUT_US: i64 = 120_000;
const HALO_IN_US: i64 = 160_000;
const HALO_OUT_US: i64 = 120_000;
const BEAD_TWEEN_US: i64 = 220_000;
const FOLD_US: i64 = 150_000;
const FACE_IN_US: i64 = 180_000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Contact {
    Pending([f32; 2]),
    Moving,
    Ring,
    Wheel(ColorWheelPart),
    Intensity,
    Beads(bool),
    Choose,
    Pair,
}


struct Item {
    root: gtk::Box,
    icon: gtk::Image,
    preview: RefCell<Option<gdk::Paintable>>,
    name: gtk::Label,
    tag: gtk::Label,
}

struct Tween {
    from: Vec<BubbleBead>,
    to: Vec<BubbleBead>,
    start: i64,
}

struct ColorPlate {
    layout: ColorBubbleLayout,
    outline: gsk::Path,
    shadows: [gsk::RenderNode; 2],
}

impl ColorPlate {
    fn new(layout: ColorBubbleLayout) -> Self {
        let outline = color_outline(layout, 0.);
        let shadows = [0., 1.].map(|lift| {
            let snapshot = gtk::Snapshot::new();
            if layout.intensity.is_none() {
                let radius = layout.diameter * 0.5;
                soft_shadow(&snapshot, &graphene::Rect::new(-radius, -radius, layout.diameter, layout.diameter), radius, lift);
                return snapshot.to_node().unwrap();
            }
            let clip = gsk::PathBuilder::new();
            clip.add_rect(&graphene::Rect::new(-layout.diameter, -layout.diameter, layout.diameter * 2., layout.diameter * 2.));
            clip.add_path(&outline);
            snapshot.push_fill(&clip.to_path(), gsk::FillRule::EvenOdd);
            snapshot.push_shadow(&[gsk::Shadow::new(gdk::RGBA::new(0., 0., 0., 0.08 * (1. + lift)),
                0., 2. + lift * 2.5, 8. * (1. + lift * 0.5))]);
            snapshot.append_fill(&outline, gsk::FillRule::Winding, &gdk::RGBA::BLACK);
            snapshot.pop();
            snapshot.pop();
            snapshot.to_node().unwrap()
        });
        Self { layout, outline, shadows }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pair {
    start: f32,
    origin: [f32; 2],
    mid: [f32; 2],
    lifted: bool,
    ended: bool,
}

struct Press {
    sequence: Option<usize>,
    hit: Option<BubbleHit>,
    start: [f32; 2],
    moving: bool,
    held: bool,
    hold: Option<glib::SourceId>,
}

impl Press {
    fn forget(&mut self) {
        if let Some(source) = self.hold.take() {
            source.remove();
        }
    }
}

struct Card {
    root: gtk::Box,
    full: bool,
    inspector: super::drawers::LayerInspector,
    add: gtk::Button,
    disclosure: gtk::Button,
    disclosure_label: gtk::Label,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Layer {
        pub(super) placements: RefCell<Vec<BubblePlacement>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Layer {
        const NAME: &'static str = "CapyBubbleLayer";
        type Type = super::Layer;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Layer {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for Layer {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn size_allocate(&self, _: i32, _: i32, _: i32) {
            let placements = self.placements.borrow();
            let mut child = self.obj().first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                let bubble = widget.downcast_ref::<super::Bubble>().unwrap();
                let placement = placements.iter().find(|p| p.id == bubble.id()).copied();
                widget.set_child_visible(placement.is_some());
                if let Some(p) = placement {
                    bubble.set_placement(p);
                    let extent = bubble.extent();
                    allocate_at(&widget, Bounds {
                        x: p.center[0] - extent * 0.5,
                        y: p.center[1] - extent * 0.5,
                        width: extent,
                        height: extent,
                    });
                }
            }
        }
        fn contains(&self, _: f64, _: f64) -> bool {
            false
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let bubbles = obj.bubbles();
            for bubble in &bubbles {
                bubble.snapshot_stamp(obj.upcast_ref(), snapshot);
            }
            for bubble in &bubbles {
                obj.snapshot_child(bubble, snapshot);
            }
        }
    }

    #[derive(Default)]
    pub struct Bubble {
        pub(super) view: RefCell<Option<BubbleView>>,
        pub(super) face: RefCell<Option<gtk::Box>>,
        pub(super) icon: RefCell<Option<gtk::Image>>,
        pub(super) preview: RefCell<Option<gdk::Paintable>>,
        pub(super) buttons: RefCell<Vec<gtk::Image>>,
        pub(super) halves: RefCell<Vec<gtk::Box>>,
        pub(super) hovered: Cell<Option<BubbleHit>>,
        pub(super) last_ring: Cell<Option<(u32, [f32; 2])>>,
        pub(super) pressed: Cell<Option<BubbleHit>>,
        pub(super) stamp: RefCell<Option<(u64, f32, gdk::Texture)>>,
        pub(super) stamp_patch: RefCell<Option<crate::display_color::ColorPatch>>,
        pub(super) stamp_tick: RefCell<Option<gtk::TickCallbackId>>,
        pub(super) wheel: RefCell<Option<crate::tool_panels::ColorWheel>>,
        pub(super) plate: RefCell<Option<ColorPlate>>,
        pub(super) thumbnail: RefCell<Option<gtk::Picture>>,
        pub(super) count: RefCell<Option<gtk::Label>>,
        pub(super) icons: RefCell<Vec<gtk::Image>>,
        pub(super) readout: RefCell<Option<gtk::Label>>,
        pub(super) beads: RefCell<Vec<(crate::tool_panels::WheelButton, crate::display_color::ColorPatch)>>,
        pub(super) palettes: RefCell<Option<gtk::Box>>,
        pub(super) pill: RefCell<Option<(gtk::Box, gtk::Label)>>,
        pub(super) items: RefCell<Vec<super::Item>>,
        pub(super) card: RefCell<Option<super::Card>>,
        pub(super) fold: Cell<Option<(i64, bool)>>,
        pub(super) fade: Cell<Option<(Option<i64>, f64, f64)>>,
        pub(super) card_size: Cell<[f32; 2]>,
        pub(super) display: Cell<Option<(crate::display_color::ViewColor, f32)>>,
        pub(super) placement: Cell<Option<BubblePlacement>>,
        pub(super) colors: Cell<Option<[gdk::RGBA; 3]>>,
        pub(super) contact: Cell<Option<Contact>>,
        pub(super) hot: Cell<Option<usize>>,
        pub(super) pinch: Cell<Option<f32>>,
        pub(super) rings_shown: RefCell<Vec<BubbleRing>>,
        pub(super) halo_fade: Cell<Option<(i64, bool)>>,
        pub(super) born: RefCell<Vec<i64>>,
        pub(super) leaving: RefCell<Vec<(super::Item, ChooserItem, i64)>>,
        pub(super) tween: RefCell<Option<super::Tween>>,
        pub(super) ticking: Cell<bool>,
        pub(super) fingers: RefCell<Vec<(usize, [f32; 2])>>,
        pub(super) color_touches: crate::color_touch::ColorTouches,
        pub(super) pair: Cell<Option<super::Pair>>,
        pub(super) press: RefCell<Option<super::Press>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Bubble {
        const NAME: &'static str = "CapyToolBubble";
        type Type = super::Bubble;
        type ParentType = gtk::Widget;
        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Button);
        }
    }
    impl ObjectImpl for Bubble {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for Bubble {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let view = self.view.borrow();
            let Some(view) = view.as_ref() else { return };
            let obj = self.obj();
            let c = [width as f32 * 0.5, height as f32 * 0.5];
            let place = |widget: &gtk::Widget, at: [f32; 2], size: [f32; 2]| {
                allocate_at(widget, Bounds { x: c[0] + at[0] - size[0] * 0.5, y: c[1] + at[1] - size[1] * 0.5, width: size[0], height: size[1] });
            };
            if let Some(face) = self.face.borrow().as_ref() {
                place(face.upcast_ref(), [0.; 2], view.face);
            }
            if let (Some(wheel), Some(layout)) = (self.wheel.borrow().as_ref(), view.color) {
                allocate_at(wheel.upcast_ref(), Bounds { x: c[0] - layout.stage * 0.5, y: c[1] - layout.stage * 0.5,
                    width: layout.stage, height: layout.panel().height() });
            }
            for (i, half) in self.halves.borrow().iter().enumerate() {
                place(half.upcast_ref(), history_center(i), [HISTORY_BUBBLE[1]; 2]);
            }
            if let Some(count) = self.count.borrow().as_ref() {
                let (_, natural, _, _) = count.measure(gtk::Orientation::Horizontal, -1);
                let (_, tall, _, _) = count.measure(gtk::Orientation::Vertical, natural);
                let (at, size) = count_badge([natural as f32, tall as f32]);
                place(count.upcast_ref(), at, size);
            }
            for (icon, ring) in self.icons.borrow().iter().zip(self.rings_shown.borrow().iter()) {
                place(icon.upcast_ref(), ring.arc.icon_point(), [16.; 2]);
            }
            if let (Some(patch), Some(stamp), Some((_, extent, _))) = (self.stamp_patch.borrow().as_ref(), view.stamp, self.stamp.borrow().as_ref()) {
                place(patch.upcast_ref(), stamp.center, [stamp.diameter * extent; 2]);
            }
            if let Some(readout) = self.readout.borrow().as_ref().filter(|r| r.is_visible()) {
                let (_, natural, _, _) = readout.measure(gtk::Orientation::Horizontal, -1);
                let (_, tall, _, _) = readout.measure(gtk::Orientation::Vertical, natural);
                place(readout.upcast_ref(), view.readout_center(), [natural as f32, tall as f32]);
            }
            let shown = obj.shown_beads();
            let colors = shown.iter().filter(|b| b.placement.bead != Bead::Palettes);
            for ((swatch, _), bead) in self.beads.borrow().iter().zip(colors) {
                place(swatch.upcast_ref(), bead.placement.center, [bead.placement.size; 2]);
            }
            if let (Some(palettes), Some(bead)) = (self.palettes.borrow().as_ref(), shown.iter().find(|b| b.placement.bead == Bead::Palettes)) {
                place(palettes.upcast_ref(), bead.placement.center, [bead.placement.size; 2]);
            }
            if let Some((pill, _)) = self.pill.borrow().as_ref() {
                let b = view.palette_bounds();
                place(pill.upcast_ref(), [0., b.y + b.height * 0.5], [b.width, b.height]);
            }
            for (item, chooser) in self.items.borrow().iter().zip(&view.chooser) {
                place(item.root.upcast_ref(), chooser.center, chooser.size());
                if item.tag.is_visible() {
                    let (_, natural, _, _) = item.tag.measure(gtk::Orientation::Horizontal, -1);
                    let width = (natural as f32).min(CHOOSER_TAG[0]);
                    let (_, tall, _, _) = item.tag.measure(gtk::Orientation::Vertical, width as i32);
                    place(item.tag.upcast_ref(), chooser.tag_center(width), [width, tall as f32]);
                }
            }
            for (item, chooser, _) in self.leaving.borrow().iter() {
                place(item.root.upcast_ref(), chooser.center, chooser.size());
            }
            if let Some(card) = self.card.borrow().as_ref().filter(|c| c.root.is_child_visible()) {
                place(card.root.upcast_ref(), [0.; 2], self.card_size.get());
            }
        }
        fn contains(&self, x: f64, y: f64) -> bool {
            let view = self.view.borrow();
            let Some(view) = view.as_ref() else { return false };
            let obj = self.obj();
            let p = [x as f32 - obj.width() as f32 * 0.5, y as f32 - obj.height() as f32 * 0.5];
            if self.contact.get().is_some() || !self.fingers.borrow().is_empty() {
                let reach = view.reach() + SECOND_FINGER_REACH;
                return p[0].abs() <= reach && p[1].abs() <= reach;
            }
            view.hit(p).is_some()
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let view = self.view.borrow();
            let Some(view) = view.as_ref() else { return };
            let now = obj.now();
            let c = [obj.width() as f32 * 0.5, obj.height() as f32 * 0.5];
            let placement = self.placement.get();
            let homing = placement.is_some_and(|p| p.homing);
            let scale = self.pinch.get().unwrap_or(1.) * if homing { 0.82 } else if placement.is_some_and(|p| p.lifted) { 1.04 } else { 1. };
            snapshot.save();
            snapshot.translate(&graphene::Point::new(c[0], c[1]));
            snapshot.scale(scale, scale);
            snapshot.translate(&graphene::Point::new(-c[0], -c[1]));
            if homing {
                snapshot.push_opacity(0.55);
            }
            let lift = if placement.is_some_and(|p| p.lifted) { 1. } else { 0. };
            let round = |widget: &gtk::Widget| {
                if let Some(bounds) = widget.compute_bounds(&*obj) {
                    super::soft_shadow(snapshot, &bounds, f32::MAX, lift);
                }
                let part = gtk::Snapshot::new();
                obj.snapshot_child(widget, &part);
                crate::squircle::append_round(snapshot, part);
            };
            let grow = |at: [f32; 2], scale: f32, alpha: f32| {
                snapshot.save();
                snapshot.translate(&graphene::Point::new(at[0], at[1]));
                snapshot.scale(scale, scale);
                snapshot.translate(&graphene::Point::new(-at[0], -at[1]));
                snapshot.push_opacity(alpha as f64);
            };
            let done = || {
                snapshot.pop();
                snapshot.restore();
            };
            let colors = self.colors.get();
            let (card_shown, face_shown) = obj.fold_progress(view.card.is_some());
            if let (Some(card), Some(progress)) = (self.card.borrow().as_ref().filter(|c| c.root.is_child_visible()), card_shown) {
                grow(c, 0.6 + 0.4 * progress, progress);
                if let Some(bounds) = card.root.compute_bounds(&*obj) {
                    super::soft_shadow(snapshot, &bounds, layer_ui::SURFACE_RADIUS, lift);
                }
                obj.snapshot_child(&card.root, snapshot);
                done();
            }
            if let Some(progress) = face_shown {
                grow(c, 0.6 + 0.4 * progress, progress);
                if let Some(face) = self.face.borrow().as_ref() {
                    if view.color.is_some() {
                        if let (Some(plate), Some([chip, _, _])) = (self.plate.borrow().as_ref(), colors) {
                            super::color_plate(snapshot, c, plate, chip, lift);
                        }
                        if let Some(wheel) = self.wheel.borrow().as_ref() { obj.snapshot_child(wheel, snapshot); }
                    } else if view.kind == BubbleKind::Layers {
                        obj.snapshot_child(face, snapshot);
                    } else {
                        round(face.upcast_ref());
                    }
                }
                for half in self.halves.borrow().iter() {
                    obj.snapshot_child(half, snapshot);
                }
                if let Some(preview) = self.preview.borrow().as_ref() {
                    snapshot.push_opacity(0.78);
                    crate::previews::append(snapshot, preview, &graphene::Rect::new(c[0] - 16., c[1] + 7., 32., 8.));
                    snapshot.pop();
                }
                if let Some(count) = self.count.borrow().as_ref() {
                    round(count.upcast_ref());
                }
                done();
            }
            if let Some(colors) = colors {
                if view.kind == BubbleKind::Medium {
                    super::dots(snapshot, c, view, colors[1]);
                }
                let alpha = obj.halo_alpha();
                let rings = self.rings_shown.borrow();
                if alpha > 0. && !rings.is_empty() {
                    grow(c, 0.85 + 0.15 * alpha, alpha);
                    for ring in rings.iter() {
                        snapshot.push_opacity(if ring.enabled { 1. } else { 0.4 });
                        super::ring(snapshot, c, ring, colors);
                        snapshot.pop();
                    }
                    for icon in self.icons.borrow().iter().take(rings.len()) {
                        obj.snapshot_child(icon, snapshot);
                    }
                    done();
                }
            }
            let shown = obj.shown_beads();
            if let Some([_, ink, _]) = colors {
                for ([x, y], alpha) in &view.hints {
                    snapshot.append_fill(&circle([c[0] + x, c[1] + y], HINT_DOT * 0.5), gsk::FillRule::Winding, &rgba(ink, 0.35 * alpha));
                }
            }
            let colored = shown.iter().filter(|b| b.placement.bead != Bead::Palettes);
            for ((swatch, _), bead) in self.beads.borrow().iter().zip(colored) {
                super::bead_shadow(snapshot, c, bead);
                let part = gtk::Snapshot::new();
                part.push_opacity(bead.placement.alpha as f64);
                obj.snapshot_child(swatch, &part);
                part.pop();
                crate::squircle::append_round(snapshot, part);
            }
            if let Some(palettes) = self.palettes.borrow().as_ref().filter(|p| p.is_visible()) {
                round(palettes.upcast_ref());
            }
            if let Some((pill, _)) = self.pill.borrow().as_ref().filter(|(p, _)| p.is_visible()) {
                round(pill.upcast_ref());
            }
            let hot = self.hot.get();
            let born = self.born.borrow();
            let item = |item: &super::Item, chooser: &ChooserItem, scale: f32, alpha: f32| {
                let at = [c[0] + chooser.center[0], c[1] + chooser.center[1]];
                grow(at, scale, alpha);
                round(item.root.upcast_ref());
                if let Some(preview) = item.preview.borrow().as_ref().filter(|_| chooser.kind == ChooserKind::Brush) {
                    let [w, h] = BRUSH_PILL;
                    crate::previews::append(snapshot, preview, &graphene::Rect::new(at[0] - w * 0.34, at[1] - h * 0.5 + 6., w * 0.68, 14.));
                }
                done();
            };
            for (i, (widget, chooser)) in self.items.borrow().iter().zip(&view.chooser).enumerate() {
                let alpha = ease(now.saturating_sub(born.get(i).copied().unwrap_or(0)), ITEM_IN_US);
                let hot = if hot != Some(i) { 1. } else if chooser.kind == ChooserKind::Brush { 1.06 } else { 1.12 };
                if alpha > 0. {
                    item(widget, chooser, (0.6 + 0.4 * alpha) * hot, alpha);
                }
                if widget.tag.is_visible() && alpha >= 1. {
                    round(widget.tag.upcast_ref());
                }
            }
            for (widget, chooser, start) in self.leaving.borrow().iter() {
                let alpha = 1. - ease(now - start, ITEM_OUT_US);
                if alpha > 0. {
                    item(widget, chooser, 0.6 + 0.4 * alpha, alpha);
                }
            }
            if let Some(readout) = self.readout.borrow().as_ref().filter(|r| r.is_visible()) {
                round(readout.upcast_ref());
            }
            if homing {
                snapshot.pop();
            }
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    pub struct Layer(ObjectSubclass<imp::Layer>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
glib::wrapper! {
    pub struct Bubble(ObjectSubclass<imp::Bubble>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

fn ease(elapsed: i64, span: i64) -> f32 {
    1. - (1. - (elapsed as f32 / span as f32).clamp(0., 1.)).powi(3)
}

fn smooth(elapsed: i64, span: i64) -> f32 {
    let t = (elapsed as f32 / span as f32).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn motion() -> bool {
    gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations())
}

fn tweened(from: &[BubbleBead], to: &[BubbleBead], t: f32) -> Vec<BubbleBead> {
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let mut out: Vec<_> = to
        .iter()
        .map(|b| {
            let end = b.placement;
            let start = from.iter().find(|f| f.placement.bead == end.bead).map_or(BeadPlacement { alpha: 0., ..end }, |f| f.placement);
            let placement = BeadPlacement {
                center: [lerp(start.center[0], end.center[0]), lerp(start.center[1], end.center[1])],
                size: lerp(start.size, end.size),
                alpha: lerp(start.alpha, end.alpha),
                ..end
            };
            BubbleBead { placement, ..b.clone() }
        })
        .collect();
    out.extend(from.iter().filter(|f| !to.iter().any(|b| b.placement.bead == f.placement.bead)).map(|f| BubbleBead {
        placement: BeadPlacement { alpha: f.placement.alpha * (1. - t), ..f.placement },
        ..f.clone()
    }));
    out
}

fn rgba(color: gdk::RGBA, alpha: f32) -> gdk::RGBA {
    gdk::RGBA::new(color.red(), color.green(), color.blue(), color.alpha() * alpha)
}

fn circle(center: [f32; 2], radius: f32) -> gsk::Path {
    let path = gsk::PathBuilder::new();
    path.add_circle(&graphene::Point::new(center[0], center[1]), radius);
    path.to_path()
}

fn color_outline(layout: ColorBubbleLayout, outset: f32) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    for (i, arc) in layout.outline(outset).into_iter().enumerate() {
        let p = arc.point(0.);
        if i == 0 { builder.move_to(p[0], p[1]); } else { builder.line_to(p[0], p[1]); }
        let steps = (arc.sweep.abs() / std::f32::consts::FRAC_PI_2).ceil() as usize;
        for step in 1..=steps {
            let p = arc.point(step as f32 / steps as f32);
            builder.svg_arc_to(arc.radius, arc.radius, 0., false, true, p[0], p[1]);
        }
    }
    builder.close();
    builder.to_path()
}

fn color_plate(snapshot: &gtk::Snapshot, center: [f32; 2], plate: &ColorPlate, color: gdk::RGBA, lift: f32) {
    snapshot.save();
    snapshot.translate(&graphene::Point::new(center[0], center[1]));
    snapshot.append_node(&plate.shadows[usize::from(lift > 0.)]);
    if plate.layout.intensity.is_some() {
        let fill = gtk::Snapshot::new();
        fill.append_fill(&plate.outline, gsk::FillRule::Winding, &color);
        if let Some(node) = fill.to_node() { snapshot.append_node(crate::glass::color_bubble(node, plate.layout)); }
    } else {
        let radius = plate.layout.diameter * 0.5;
        let bounds = graphene::Rect::new(-radius, -radius, radius * 2., radius * 2.);
        let fill = gtk::Snapshot::new();
        fill.push_rounded_clip(&gsk::RoundedRect::from_rect(bounds, radius));
        fill.append_color(&color, &bounds);
        fill.pop();
        crate::squircle::append_round(snapshot, fill);
    }
    snapshot.restore();
}

fn arc_path(c: [f32; 2], arc: &RingArc, to: f32) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    let [x0, y0] = arc.point(0.);
    builder.move_to(c[0] + x0, c[1] + y0);
    let sweep = arc.sweep * to;
    let mut done = 0f32;
    while (sweep - done).abs() > 1e-3 {
        let step = (sweep - done).clamp(-179., 179.);
        done += step;
        let [x, y] = arc.point(done / arc.sweep);
        builder.svg_arc_to(arc.radius, arc.radius, 0., false, step > 0., c[0] + x, c[1] + y);
    }
    builder.to_path()
}

fn ring(snapshot: &gtk::Snapshot, c: [f32; 2], ring: &BubbleRing, [chip, ink, thumb_color]: [gdk::RGBA; 3]) {
    let stroke = |width: f32| {
        let stroke = gsk::Stroke::new(width);
        stroke.set_line_cap(gsk::LineCap::Round);
        stroke
    };
    let arc = ring.arc;
    let [x, y] = arc.icon_point();
    let start = y.atan2(x).to_degrees();
    let lead = (start - arc.start + 540.).rem_euclid(360.) - 180.;
    let banded = RingArc { start, sweep: arc.sweep - lead, ..arc };
    let band = gtk::Snapshot::new();
    band.append_stroke(&arc_path(c, &banded, 1.), &stroke(arc.band), &chip);
    if let Some(node) = band.to_node() {
        snapshot.append_node(crate::glass::arc(node, c, arc.radius, arc.band, [banded.start, banded.sweep]));
    }
    snapshot.append_stroke(&arc_path(c, &arc, 1.), &stroke(RING_TRACK), &rgba(ink, 0.18));
    if ring.fill > 0.004 {
        snapshot.append_stroke(&arc_path(c, &arc, ring.fill), &stroke(RING_TRACK), &rgba(ink, 0.5));
    }
    let thumb = circle({
        let [x, y] = arc.point(ring.fill);
        [c[0] + x, c[1] + y]
    }, if ring.active { 7.5 } else { 7. });
    snapshot.append_fill(&thumb, gsk::FillRule::Winding, &thumb_color);
    snapshot.append_stroke(&thumb, &gsk::Stroke::new(1.), &rgba(ink, 0.6));
}

fn soft_shadow(snapshot: &gtk::Snapshot, bounds: &graphene::Rect, radius: f32, lift: f32) {
    shadow(snapshot, bounds, radius, 8. * (1. + lift * 0.5), 2. + lift * 2.5, 0.08 * (1. + lift));
}

fn bead_shadow(snapshot: &gtk::Snapshot, c: [f32; 2], bead: &BubbleBead) {
    let [x, y] = bead.placement.center;
    let half = bead.placement.size * 0.5;
    let bounds = graphene::Rect::new(c[0] + x - half, c[1] + y - half, half * 2., half * 2.);
    shadow(snapshot, &bounds, f32::MAX, 3., 1., 0.13 * bead.placement.alpha);
}

fn shadow(snapshot: &gtk::Snapshot, bounds: &graphene::Rect, radius: f32, spread: f32, drop: f32, alpha: f32) {
    let stops = |edge: f32, end: f32| {
        [0., 0.25, 0.5, 0.75, 1.].map(|t| gsk::ColorStop::new(edge + (end - edge) * t, gdk::RGBA::new(0., 0., 0., alpha * (1. - t) * (1. - t)))).to_vec()
    };
    let (x, y, w, h) = (bounds.x(), bounds.y() + drop, bounds.width(), bounds.height());
    let radius = radius.min(w * 0.5).min(h * 0.5);
    let reach = radius + spread;
    let shape = |rect: graphene::Rect, r: f32| gsk::RoundedRect::from_rect(rect, r);
    let path = gsk::PathBuilder::new();
    path.add_rounded_rect(&shape(graphene::Rect::new(x - spread, y - spread, w + spread * 2., h + spread * 2.), reach));
    path.add_rounded_rect(&shape(*bounds, radius));
    snapshot.push_fill(&path.to_path(), gsk::FillRule::EvenOdd);
    let [left, right, top, bottom] = [x + radius, x + w - radius, y + radius, y + h - radius];
    for (cx, cy, clip) in [
        (left, top, graphene::Rect::new(left - reach, top - reach, reach, reach)),
        (right, top, graphene::Rect::new(right, top - reach, reach, reach)),
        (left, bottom, graphene::Rect::new(left - reach, bottom, reach, reach)),
        (right, bottom, graphene::Rect::new(right, bottom, reach, reach)),
    ] {
        snapshot.push_clip(&clip);
        let area = graphene::Rect::new(cx - reach, cy - reach, reach * 2., reach * 2.);
        snapshot.append_radial_gradient(&area, &graphene::Point::new(cx, cy), reach, reach, 0., 1., &stops(radius / reach, 1.));
        snapshot.pop();
    }
    let band = |length: f32| {
        let side = spread / (length + spread * 2.);
        let mut gradient = stops(side, 0.);
        gradient.reverse();
        gradient.extend(stops(1. - side, 1.));
        gradient
    };
    if right > left {
        let area = graphene::Rect::new(left, y - spread, right - left, h + spread * 2.);
        snapshot.append_linear_gradient(&area, &graphene::Point::new(left, y - spread), &graphene::Point::new(left, y + h + spread), &band(h));
    }
    if bottom > top {
        let area = graphene::Rect::new(x - spread, top, w + spread * 2., bottom - top);
        snapshot.append_linear_gradient(&area, &graphene::Point::new(x - spread, top), &graphene::Point::new(x + w + spread, top), &band(w));
    }
    snapshot.pop();
}

fn dots(snapshot: &gtk::Snapshot, c: [f32; 2], view: &BubbleView, ink: gdk::RGBA) {
    for (current, [x, y]) in view.dots.iter().zip(view.dot_centers()) {
        let at = [c[0] + x, c[1] + y];
        snapshot.append_fill(&circle(at, 1.75), gsk::FillRule::Winding, &rgba(ink, if *current { 0.9 } else { 0.3 }));
    }
}

fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

fn glass(class: &str) -> gtk::Box {
    let part = gtk::Box::new(gtk::Orientation::Vertical, 0);
    part.add_css_class(class);
    part.set_can_target(false);
    part
}

impl Item {
    fn new(bubble: &Bubble) -> Self {
        let root = glass("tool-bubble");
        let icon = crate::icons::image("layer-settings-symbolic");
        icon.set_pixel_size(18);
        icon.set_vexpand(true);
        let name = gtk::Label::new(None);
        name.add_css_class("bubble-brush-name");
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_max_width_chars(18);
        name.set_margin_start(8);
        name.set_margin_end(8);
        name.set_margin_bottom(5);
        name.set_valign(gtk::Align::End);
        name.set_vexpand(true);
        root.append(&icon);
        root.append(&name);
        root.set_parent(bubble);
        let tag = gtk::Label::new(None);
        tag.add_css_class("bubble-tag");
        tag.set_ellipsize(gtk::pango::EllipsizeMode::End);
        tag.set_can_target(false);
        tag.set_parent(bubble);
        Self { root, icon, preview: RefCell::new(None), name, tag }
    }
    fn set(&self, item: &ChooserItem, theme: Theme, hot: bool) {
        let brush = item.kind == ChooserKind::Brush;
        self.icon.set_visible(!brush || item.preview.is_none());
        crate::icons::set(&self.icon, Some(&format!("layer-{}-symbolic", item.icon)));
        let preview = item.preview.filter(|_| brush).map(|id| crate::previews::texture(id, theme));
        if let (Some(preview), Some(bubble)) = (&preview, self.root.parent()) { crate::previews::redraw_when_loaded(preview, &bubble); }
        *self.preview.borrow_mut() = preview;
        self.name.set_visible(brush);
        self.name.set_label(&item.label);
        self.tag.set_label(&item.label);
        self.tag.set_visible(!brush && (item.shown || hot));
        set_class(&self.root, "selected-tool", item.selected);
        set_class(&self.root, "bubble-brush", brush);
        self.root.set_opacity(if item.enabled { 1. } else { 0.45 });
        self.root.update_property(&[gtk::accessible::Property::Label(&item.label)]);
    }
    fn remove(self) {
        self.root.unparent();
        self.tag.unparent();
    }
}

fn drags(w: &gtk::Widget) -> bool {
    w.is::<gtk::Range>() || w.is::<gtk::Editable>() || w.is::<gtk::DrawingArea>() || w.is::<crate::number_control::NumberControl>()
}

impl Card {
    fn new(w: &Rc<Workspace>, bubble: &Bubble, id: u32, full: bool) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("dock-panel");
        root.add_css_class("content-drawer");
        root.set_widget_name("bubble-card");
        root.set_overflow(gtk::Overflow::Hidden);
        let inspector = super::drawers::LayerInspector::new(w, UiAction::Bubble { action: BubbleAction::Properties { id } });
        inspector.root.add_css_class("panel-body");
        inspector.root.set_vexpand(true);
        inspector.layers.set_simple(!full);
        inspector.layers.footer.set_visible(full);
        inspector.footer.set_visible(full);
        root.append(&inspector.root);
        let footer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        footer.add_css_class("layer-properties-footer");
        footer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        footer.set_visible(!full);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, layer_ui::PANEL_GROUP_SPACING as i32);
        row.set_height_request(layer_ui::PropertyDisclosure::HEIGHT as i32);
        let add = gtk::Button::new();
        add.set_child(Some(&super::drawers::icon_slot(&crate::icons::image("layer-add-layer-symbolic"), "bubble-new-layer-slot")));
        add.add_css_class("flat");
        add.add_css_class("layer-properties-disclosure");
        add.add_css_class("customizable-target");
        add.set_widget_name("bubble-new-layer");
        let action = UiAction::Layer { action: LayerAction::New { group: false, clipped: false } };
        w.bind_action_tooltip(&add, action.clone());
        add.connect_clicked(glib::clone!(#[weak] w, move |_| w.dispatch(action.clone())));
        row.append(&add);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, layer_ui::PANEL_GROUP_SPACING as i32);
        content.set_halign(gtk::Align::End);
        let disclosure_label = gtk::Label::builder().xalign(1.).ellipsize(gtk::pango::EllipsizeMode::End).width_chars(1).build();
        disclosure_label.set_hexpand(true);
        let chevron = crate::icons::image("layer-chevron-down-symbolic");
        chevron.add_css_class("layer-properties-chevron");
        content.append(&disclosure_label);
        content.append(&super::drawers::icon_slot(&chevron, "bubble-layer-properties-chevron-slot"));
        let disclosure = gtk::Button::new();
        disclosure.add_css_class("flat");
        disclosure.add_css_class("layer-properties-disclosure");
        disclosure.add_css_class("customizable-target");
        disclosure.set_hexpand(true);
        disclosure.set_child(Some(&content));
        disclosure.set_widget_name("bubble-layer-properties");
        disclosure.connect_clicked(glib::clone!(#[weak] w, move |_| w.dispatch(UiAction::Bubble { action: BubbleAction::Properties { id } })));
        row.append(&disclosure);
        footer.append(&row);
        root.append(&footer);
        root.set_parent(bubble);
        Self { root, full, inspector, add, disclosure, disclosure_label }
    }
    fn refresh(&self, w: &Rc<Workspace>, state: &UiState, card: &BubbleCard) {
        self.inspector.refresh(w, state, regions::ALL, card.properties);
        self.disclosure_label.set_label(&card.disclosure);
        set_class(&self.disclosure, "open", card.properties);
        self.disclosure.set_sensitive(state.layer_properties.drawer_toggle_enabled(card.properties));
        self.add.set_opacity(if card.properties { 0. } else { 1. });
        self.add.set_can_target(!card.properties);
    }
    fn within(&self, picked: Option<&gtk::Widget>, matches: impl Fn(&gtk::Widget) -> bool) -> bool {
        std::iter::successors(picked.cloned(), |w| w.parent()).take_while(|w| w != self.root.upcast_ref::<gtk::Widget>()).any(|w| matches(&w))
    }
    fn input(&self, picked: Option<&gtk::Widget>) -> bool {
        self.within(picked, |w| drags(w) || w.is::<gtk::Button>() || w.is::<gtk::DropDown>() || w.is::<gtk::Switch>() || w.is::<gtk::CheckButton>())
    }
    fn handles(&self, picked: Option<&gtk::Widget>) -> bool {
        !self.within(picked, |w| w.has_css_class("layer-row") || w.is::<gtk::Scrollbar>() || drags(w))
    }
}

impl Bubble {
    pub fn id(&self) -> u32 {
        self.imp().view.borrow().as_ref().map_or(0, |v| v.id)
    }
    fn extent(&self) -> f32 {
        self.imp().view.borrow().as_ref().map_or(0., |view| (view.reach() * 2. + 24.).ceil())
    }
    fn set_placement(&self, placement: BubblePlacement) {
        let previous = self.imp().placement.replace(Some(placement));
        if previous.is_none_or(|p| (p.lifted, p.homing) != (placement.lifted, placement.homing)) {
            self.queue_draw();
        }
    }
    fn center(&self) -> [f32; 2] {
        [self.width() as f32 * 0.5, self.height() as f32 * 0.5]
    }
    fn fold_progress(&self, open: bool) -> (Option<f32>, Option<f32>) {
        let ease = |elapsed: i64, span: i64| 1. - (1. - (elapsed as f32 / span as f32).clamp(0., 1.)).powi(3);
        let Some((start, opening)) = self.imp().fold.get() else {
            return if open { (Some(1.), None) } else { (None, Some(1.)) };
        };
        let elapsed = self.frame_clock().map_or(i64::MAX / 2, |clock| clock.frame_time()) - start;
        match opening {
            true => (Some(ease(elapsed, UNFOLD_US)), None),
            false if elapsed < FOLD_US => (Some(1. - ease(elapsed, FOLD_US)), None),
            false => (None, Some(ease(elapsed - FOLD_US, FACE_IN_US))),
        }
    }
    fn animate_fold(&self, opening: bool) {
        let Some(clock) = self.frame_clock().filter(|_| gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations())) else {
            self.imp().fold.set(None);
            self.finish_fold(opening);
            return;
        };
        self.imp().fold.set(Some((clock.frame_time(), opening)));
        self.add_tick_callback(move |bubble, clock| {
            bubble.queue_draw();
            let Some((start, current)) = bubble.imp().fold.get().filter(|(_, current)| *current == opening) else {
                return glib::ControlFlow::Break;
            };
            if clock.frame_time() - start < if opening { UNFOLD_US } else { FOLD_US + FACE_IN_US } {
                return glib::ControlFlow::Continue;
            }
            bubble.imp().fold.set(None);
            bubble.finish_fold(current);
            glib::ControlFlow::Break
        });
    }
    fn finish_fold(&self, opening: bool) {
        if let Some(card) = self.imp().card.borrow().as_ref().filter(|_| !opening) {
            card.root.set_child_visible(false);
        }
        self.queue_draw();
    }
    fn hit(&self, view: &BubbleView, p: [f32; 2], picked: Option<&gtk::Widget>) -> Option<BubbleHit> {
        view.hit(p).filter(|hit| *hit != BubbleHit::Card || self.imp().card.borrow().as_ref().is_none_or(|c| c.handles(picked)))
    }

    fn new(w: &Rc<Workspace>, view: &BubbleView, theme: Theme) -> Self {
        let bubble: Self = glib::Object::new();
        bubble.set_widget_name(&format!("bubble-{}", view.id));
        let imp = bubble.imp();
        let face = glass("tool-bubble");
        face.set_parent(&bubble);
        match view.kind {
            BubbleKind::Color => {
                face.add_css_class("bubble-plate");
                let wheel = crate::tool_panels::ColorWheel::bare();
                wheel.set_bubble_layout(view.color.unwrap());
                wheel.set_parent(&bubble);
                if let Some(intensity) = wheel.intensity() {
                    intensity.set_can_target(false);
                    intensity.connect_value_changed(glib::clone!(#[weak] w, #[weak] bubble, move |scale| {
                        if scale.updating() { return; }
                        w.dispatch(UiAction::Bubble { action: BubbleAction::SetIntensity { id: bubble.id(), stops: scale.value() as f32 } });
                    }));
                }
                *imp.wheel.borrow_mut() = Some(wheel);
            }
            BubbleKind::Layers => {
                face.add_css_class("squircle-bubble");
                let thumbnail = gtk::Picture::new();
                thumbnail.add_css_class("bubble-thumbnail");
                thumbnail.set_content_fit(gtk::ContentFit::Cover);
                thumbnail.set_can_shrink(true);
                thumbnail.set_overflow(gtk::Overflow::Hidden);
                thumbnail.set_size_request(46, 46);
                thumbnail.set_halign(gtk::Align::Center);
                thumbnail.set_valign(gtk::Align::Center);
                thumbnail.set_vexpand(true);
                face.append(&thumbnail);
                *imp.thumbnail.borrow_mut() = Some(thumbnail);
                let count = gtk::Label::new(None);
                count.add_css_class("bubble-count");
                count.set_can_target(false);
                count.set_parent(&bubble);
                *imp.count.borrow_mut() = Some(count);
            }
            _ => {
                let overlay = gtk::Overlay::new();
                overlay.set_vexpand(true);
                let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
                column.set_valign(gtk::Align::Center);
                column.set_halign(gtk::Align::Center);
                let icon = crate::icons::image(&format!("layer-{}-symbolic", view.icon));
                icon.set_pixel_size(20);
                column.append(&icon);
                match view.kind {
                    BubbleKind::Medium => icon.set_margin_bottom(8),
                    BubbleKind::History => {
                        column.remove(&icon);
                        for button in &view.buttons {
                            let half = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                            half.add_css_class("bubble-half");
                            half.set_can_target(false);
                            let image = crate::icons::image(&format!("layer-{}-symbolic", button.icon));
                            image.set_pixel_size(20);
                            image.set_hexpand(true);
                            half.append(&image);
                            half.set_parent(&bubble);
                            imp.buttons.borrow_mut().push(image);
                            imp.halves.borrow_mut().push(half);
                        }
                    }
                    _ => {}
                }
                if let Some(id) = view.preview {
                    icon.set_margin_bottom(12);
                    let preview = crate::previews::texture(id, theme);
                    crate::previews::redraw_when_loaded(&preview, &bubble);
                    *imp.preview.borrow_mut() = Some(preview);
                }
                *imp.icon.borrow_mut() = Some(icon);
                overlay.set_child(Some(&column));
                if view.variants {
                    let marker = crate::icons::image("layer-tool-group-symbolic");
                    marker.set_halign(gtk::Align::End);
                    marker.set_valign(gtk::Align::End);
                    marker.set_margin_end(9);
                    marker.set_margin_bottom(9);
                    overlay.add_overlay(&marker);
                }
                face.append(&overlay);
            }
        }
        *imp.face.borrow_mut() = Some(face);
        let readout = gtk::Label::new(None);
        readout.add_css_class("bubble-readout");
        readout.set_can_target(false);
        readout.set_visible(false);
        readout.set_parent(&bubble);
        *imp.readout.borrow_mut() = Some(readout);
        bubble.install_input(w, view.id);
        w.install_menu(&bubble, ContextTarget::Bubble { id: view.id });
        bubble
    }

    fn set_beads(&self, w: &Rc<Workspace>, view: &BubbleView) {
        let imp = self.imp();
        if view.kind != BubbleKind::Color {
            return;
        }
        let turning = imp.contact.get() == Some(Contact::Beads(true));
        let changed = imp.view.borrow().as_ref().is_some_and(|old| old.diameter == view.diameter
            && !old.beads.iter().map(|b| b.placement).eq(view.beads.iter().map(|b| b.placement)));
        let tween = (changed && !turning && motion()).then(|| Tween { from: self.shown_beads(), to: view.beads.clone(), start: self.now() });
        let shown = tween.as_ref().map_or_else(|| view.beads.clone(), |t| tweened(&t.from, &t.to, 0.));
        *imp.tween.borrow_mut() = tween;
        self.sync_patches(&shown);
        self.ensure_ticking();
        let palettes = view.beads.iter().find(|b| b.placement.bead == Bead::Palettes);
        if palettes.is_some() && imp.palettes.borrow().is_none() {
            let bead = glass("bubble-glass");
            let icon = crate::icons::image(&format!("layer-{}-symbolic", Panel::Palettes.icon()));
            icon.set_pixel_size(13);
            icon.set_vexpand(true);
            bead.append(&icon);
            bead.set_parent(self);
            *imp.palettes.borrow_mut() = Some(bead);
        }
        if let Some(bead) = imp.palettes.borrow().as_ref() {
            bead.set_visible(palettes.is_some());
            set_class(bead, "selected-tool", palettes.is_some_and(|b| b.selected));
        }
        if view.pill.is_some() && imp.pill.borrow().is_none() {
            let id = view.id;
            let pill = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            pill.add_css_class("bubble-pill");
            let step = |icon: &str, step: i32| {
                let button = crate::icons::button(icon);
                button.add_css_class("flat");
                button.add_css_class("customizable-target");
                button.connect_clicked(glib::clone!(#[weak] w, move |_| {
                    w.dispatch(UiAction::Bubble { action: BubbleAction::Palette { id, step } });
                }));
                button
            };
            let name = gtk::Label::new(None);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            name.set_width_chars(1);
            let choose = gtk::Button::new();
            choose.add_css_class("flat");
            choose.add_css_class("customizable-target");
            choose.set_hexpand(true);
            choose.set_child(Some(&name));
            choose.connect_clicked(glib::clone!(#[weak] w, move |button| {
                w.show_context(button.upcast_ref(), ContextTarget::BubblePalettes { id }, button.width() as f64 * 0.5, button.height() as f64);
            }));
            pill.append(&step("layer-go-previous-symbolic", -1));
            pill.append(&choose);
            pill.append(&step("layer-go-next-symbolic", 1));
            pill.set_parent(self);
            *imp.pill.borrow_mut() = Some((pill, name));
        }
        if let Some((pill, name)) = imp.pill.borrow().as_ref() {
            pill.set_visible(view.pill.is_some());
            name.set_label(view.pill.as_deref().unwrap_or_default());
        }
    }

    fn sync_patches(&self, shown: &[BubbleBead]) {
        let imp = self.imp();
        let colors: Vec<_> = shown.iter().filter(|b| b.placement.bead != Bead::Palettes).collect();
        let mut beads = imp.beads.borrow_mut();
        while beads.len() < colors.len() {
            let (swatch, patch) = crate::tool_panels::WheelButton::swatch();
            swatch.add_css_class("bubble-bead");
            swatch.set_can_target(false);
            swatch.set_parent(self);
            beads.push((swatch, patch));
        }
        while beads.len() > colors.len() {
            beads.pop().unwrap().0.unparent();
        }
        let (display, headroom) = imp.display.get().unwrap_or_default();
        for ((swatch, patch), bead) in beads.iter().zip(&colors) {
            swatch.set_selected(bead.selected);
            set_class(swatch, "foreground", bead.placement.bead == Bead::Foreground);
            if let Some(color) = bead.color {
                patch.set_display_color(color, display, headroom);
            }
        }
    }

    fn shown_beads(&self) -> Vec<BubbleBead> {
        let imp = self.imp();
        match imp.tween.borrow().as_ref() {
            Some(t) => tweened(&t.from, &t.to, ease(self.now() - t.start, BEAD_TWEEN_US)),
            None => imp.view.borrow().as_ref().map(|v| v.beads.clone()).unwrap_or_default(),
        }
    }

    fn now(&self) -> i64 {
        self.frame_clock().map_or_else(glib::monotonic_time, |clock| clock.frame_time())
    }

    fn halo_alpha(&self) -> f32 {
        match self.imp().halo_fade.get() {
            None => 1.,
            Some((start, true)) => ease(self.now() - start, HALO_IN_US),
            Some((start, false)) => 1. - ease(self.now() - start, HALO_OUT_US),
        }
    }

    fn ensure_ticking(&self) {
        if self.imp().ticking.replace(true) {
            return;
        }
        self.add_tick_callback(|bubble, _| {
            bubble.queue_allocate();
            bubble.queue_draw();
            if bubble.settle() {
                glib::ControlFlow::Continue
            } else {
                bubble.imp().ticking.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    fn settle(&self) -> bool {
        let imp = self.imp();
        let now = self.now();
        if let Some((start, from, to)) = imp.fade.get() {
            let start = start.unwrap_or(now);
            imp.fade.set(Some((Some(start), from, to)));
            let t = smooth(now - start, FADE_US) as f64;
            self.set_opacity(from + (to - from) * t);
            if t >= 1. {
                imp.fade.set(None);
            }
        }
        if let Some((start, showing)) = imp.halo_fade.get()
            && now - start >= if showing { HALO_IN_US } else { HALO_OUT_US }
        {
            imp.halo_fade.set(None);
            if !showing {
                imp.rings_shown.borrow_mut().clear();
                self.sync_ring_icons();
            }
        }
        let (gone, kept): (Vec<_>, Vec<_>) = imp.leaving.take().into_iter().partition(|(_, _, start)| now - start >= ITEM_OUT_US);
        *imp.leaving.borrow_mut() = kept;
        for (item, _, _) in gone {
            item.remove();
        }
        if imp.tween.borrow().as_ref().is_some_and(|t| now - t.start >= BEAD_TWEEN_US) {
            imp.tween.take();
            let beads = imp.view.borrow().as_ref().map(|v| v.beads.clone()).unwrap_or_default();
            self.sync_patches(&beads);
        }
        imp.halo_fade.get().is_some()
            || !imp.leaving.borrow().is_empty()
            || imp.tween.borrow().is_some()
            || imp.born.borrow().iter().any(|born| now - born < ITEM_IN_US)
            || imp.fade.get().is_some()
    }

    fn fade_to(&self, opacity: f64) {
        let imp = self.imp();
        if imp.fade.get().map_or(self.opacity(), |(_, _, to)| to) == opacity {
            return;
        }
        if !motion() {
            imp.fade.set(None);
            self.set_opacity(opacity);
            return;
        }
        imp.fade.set(Some((None, self.opacity(), opacity)));
        self.ensure_ticking();
    }

    fn sync_ring_icons(&self) {
        let imp = self.imp();
        let rings = imp.rings_shown.borrow();
        let count = rings.len();
        let mut icons = imp.icons.borrow_mut();
        while icons.len() < count {
            let icon = crate::icons::image("layer-settings-symbolic");
            icon.set_pixel_size(16);
            icon.set_can_target(false);
            icon.set_parent(self);
            icons.push(icon);
        }
        for (i, icon) in icons.iter().enumerate() {
            icon.set_child_visible(i < count);
            if let Some(ring) = rings.get(i) {
                crate::icons::set(icon, Some(&format!("layer-{}-symbolic", ring.icon)));
                icon.set_tooltip_text(Some(&ring.label));
                icon.set_opacity(if ring.enabled { 1. } else { 0.4 });
            }
        }
    }

    fn set_rings(&self, view: &BubbleView) {
        let imp = self.imp();
        let showing = !imp.rings_shown.borrow().is_empty() && !matches!(imp.halo_fade.get(), Some((_, false)));
        if !view.rings.is_empty() {
            *imp.rings_shown.borrow_mut() = view.rings.clone();
            if !showing {
                imp.halo_fade.set(motion().then(|| (self.now(), true)));
            }
        } else if showing && motion() {
            imp.halo_fade.set(Some((self.now(), false)));
        } else if !matches!(imp.halo_fade.get(), Some((_, false))) {
            imp.rings_shown.borrow_mut().clear();
        }
        self.ensure_ticking();
    }

    fn set_items(&self, view: &BubbleView, theme: Theme) {
        let imp = self.imp();
        let previous: Vec<ChooserItem> = imp.view.borrow().as_ref().map(|v| v.chooser.clone()).unwrap_or_default();
        let now = self.now();
        let mut items = imp.items.borrow_mut();
        if view.chooser.is_empty() && !items.is_empty() {
            let closing: Vec<_> = items.drain(..).zip(previous).collect();
            imp.born.borrow_mut().clear();
            drop(items);
            for (item, chooser) in closing {
                item.tag.set_visible(false);
                if motion() {
                    imp.leaving.borrow_mut().push((item, chooser, now));
                } else {
                    item.remove();
                }
            }
            self.ensure_ticking();
            return;
        }
        while items.len() < view.chooser.len() {
            items.push(Item::new(self));
        }
        while items.len() > view.chooser.len() {
            items.pop().unwrap().remove();
        }
        let mut born = imp.born.borrow_mut();
        born.resize(view.chooser.len(), 0);
        let mut fresh = 0;
        for (i, chooser) in view.chooser.iter().enumerate() {
            if !previous.get(i).is_some_and(|p| p.kind == chooser.kind && p.label == chooser.label) {
                let stagger = if chooser.kind == ChooserKind::Brush { 16_000 } else { 14_000 };
                born[i] = if motion() { now + fresh * stagger } else { 0 };
                fresh += 1;
            }
        }
        drop(born);
        let hot = imp.hot.get();
        for (i, (item, chooser)) in items.iter().zip(&view.chooser).enumerate() {
            item.set(chooser, theme, hot == Some(i));
        }
        drop(items);
        self.ensure_ticking();
    }

    fn set_card(&self, w: &Rc<Workspace>, state: &UiState, view: &BubbleView) {
        let imp = self.imp();
        let shown = imp.card.borrow().as_ref().is_some_and(|c| c.root.is_child_visible());
        let folding = imp.fold.get().is_some_and(|(_, opening)| !opening);
        let Some(card) = view.card.as_ref() else {
            if shown && !folding {
                self.animate_fold(false);
            }
            return;
        };
        if imp.card.borrow().as_ref().is_some_and(|c| c.full != card.full) {
            imp.card.take().unwrap().root.unparent();
        }
        if imp.card.borrow().is_none() {
            *imp.card.borrow_mut() = Some(Card::new(w, self, view.id, card.full));
        }
        let opening = !imp.card.borrow().as_ref().is_some_and(|c| c.root.is_child_visible()) || folding;
        if let Some(body) = imp.card.borrow().as_ref() {
            body.root.set_child_visible(true);
            body.refresh(w, state, card);
        }
        if opening {
            self.animate_fold(true);
        }
    }

    fn set_view(&self, view: BubbleView) {
        let imp = self.imp();
        if let Some(card) = &view.card { imp.card_size.set(card.size); }
        if let Some(icon) = imp.icon.borrow().as_ref() {
            crate::icons::set(icon, Some(&format!("layer-{}-symbolic", view.icon)));
        }
        if let Some(face) = imp.face.borrow().as_ref() {
            set_class(face, "selected-tool", view.selected && matches!(view.kind, BubbleKind::Dial | BubbleKind::Brush | BubbleKind::Medium | BubbleKind::Controls));
            face.set_opacity(if view.enabled { 1. } else { 0.45 });
            face.set_child_visible(view.card.is_none());
        }
        if let Some(count) = imp.count.borrow().as_ref() {
            count.set_label(&view.layers.to_string());
            count.set_child_visible(view.card.is_none());
        }
        for (image, button) in imp.buttons.borrow().iter().zip(&view.buttons) {
            image.set_opacity(if button.enabled { 1. } else { 0.35 });
            image.set_tooltip_text(Some(&button.label));
        }
        if let Some(layout) = view.color {
            if imp.plate.borrow().as_ref().is_none_or(|old| old.layout != layout) {
                *imp.plate.borrow_mut() = Some(ColorPlate::new(layout));
            }
            if let Some(wheel) = imp.wheel.borrow().as_ref() { wheel.set_bubble_layout(layout); }
        }
        self.set_rings(&view);
        if let Some(readout) = imp.readout.borrow().as_ref() {
            readout.set_visible(view.readout.is_some());
            readout.set_label(view.readout.as_deref().unwrap_or_default());
        }
        self.update_property(&[gtk::accessible::Property::Label(&view.label)]);
        *imp.view.borrow_mut() = Some(view);
        self.sync_ring_icons();
        self.queue_allocate();
        self.queue_draw();
    }

    fn set_stamp(&self, w: &Rc<Workspace>, view: &BubbleView) {
        let imp = self.imp();
        let Some(stamp) = view.stamp else {
            if let Some(patch) = imp.stamp_patch.take() {
                patch.unparent();
                imp.stamp.take();
                self.set_cursor(None::<&gdk::Cursor>);
                self.layer_redraw();
            }
            return;
        };
        if self.cursor().is_none_or(|c| c.name().as_deref() != Some("none")) {
            self.set_cursor_from_name(Some("none"));
        }
        let (display, headroom) = imp.display.get().unwrap_or_default();
        imp.stamp_patch
            .borrow_mut()
            .get_or_insert_with(|| {
                let patch = crate::display_color::ColorPatch::new(false);
                patch.set_parent(self);
                patch
            })
            .set_display_color(stamp.color, display, headroom);
        if imp.stamp_tick.borrow().is_none() {
            let tick = self.add_tick_callback(glib::clone!(#[weak] w, #[upgrade_or] glib::ControlFlow::Break, move |bubble, _| {
                if bubble.poll_stamp(&w) {
                    return glib::ControlFlow::Continue;
                }
                bubble.imp().stamp_tick.take();
                glib::ControlFlow::Break
            }));
            *imp.stamp_tick.borrow_mut() = Some(tick);
        }
    }

    fn snapshot_stamp(&self, layer: &gtk::Widget, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        let (Some(patch), Some((_, _, texture))) = (imp.stamp_patch.borrow().clone(), imp.stamp.borrow().clone()) else { return };
        let (Some(origin), Some(bounds)) = (self.compute_point(layer, &graphene::Point::zero()), patch.compute_bounds(self)) else { return };
        snapshot.save();
        snapshot.translate(&origin);
        snapshot.push_mask(gsk::MaskMode::Alpha);
        snapshot.append_texture(&texture, &bounds);
        snapshot.pop();
        self.snapshot_child(&patch, snapshot);
        snapshot.pop();
        snapshot.restore();
    }

    fn layer_redraw(&self) {
        if let Some(layer) = self.parent() {
            layer.queue_draw();
        }
    }

    fn poll_stamp(&self, w: &Rc<Workspace>) -> bool {
        let imp = self.imp();
        if imp.stamp_patch.borrow().is_none() {
            return false;
        }
        let known = imp.stamp.borrow().as_ref().map(|(id, _, _)| *id);
        let stamp = w.gpu.borrow_mut().as_mut().and_then(|g| {
            let context = g.session.state().toolbar_context();
            g.session.toolbar_stamp(context, known).ok().flatten()
        });
        if let Some(stamp) = stamp.filter(|s| !s.alpha.is_empty()) {
            let size = stamp.size as i32;
            let texture = gdk::MemoryTexture::new(size, size, gdk::MemoryFormat::A8, &glib::Bytes::from_owned(stamp.alpha), stamp.size as usize);
            imp.stamp.replace(Some((stamp.request_id, stamp.extent, texture.upcast())));
            self.queue_allocate();
            self.layer_redraw();
        }
        true
    }

    fn hot_item(&self, p: [f32; 2]) {
        let view = self.imp().view.borrow();
        let hot = view.as_ref().and_then(|v| v.chooser.iter().position(|i| i.contains(p)));
        if self.imp().hot.replace(hot) != hot {
            for (i, (item, chooser)) in self.imp().items.borrow().iter().zip(view.iter().flat_map(|v| &v.chooser)).enumerate() {
                item.tag.set_visible(chooser.kind != ChooserKind::Brush && (chooser.shown || hot == Some(i)));
            }
            self.queue_allocate();
            self.queue_draw();
        }
    }

    fn install_input(&self, w: &Rc<Workspace>, id: u32) {
        let drag = gtk::GestureDrag::new();
        drag.set_button(gdk::BUTTON_PRIMARY);
        let local = |bubble: &Bubble, x: f64, y: f64| {
            let [cx, cy] = bubble.center();
            [x as f32 - cx, y as f32 - cy]
        };
        let surface_point = |w: &Workspace, bubble: &Bubble, x: f64, y: f64| {
            bubble.compute_point(&w.surface, &graphene::Point::new(x as f32, y as f32)).map(|p| [p.x(), p.y()])
        };
        let send = |w: &Rc<Workspace>, action: BubbleAction| w.dispatch(UiAction::Bubble { action });
        drag.connect_drag_begin(glib::clone!(#[weak] w, move |gesture, x, y| {
            let Some(bubble) = gesture.widget().and_downcast::<Bubble>() else { return };
            if gesture.current_event().and_then(|event| event.surface()).is_some_and(|surface| Some(surface) != bubble.native().and_then(|native| native.surface())) {
                return;
            }
            let position = local(&bubble, x, y);
            let Some(view) = bubble.imp().view.borrow().clone() else { return };
            let picked = bubble.pick(x, y, gtk::PickFlags::DEFAULT);
            let Some(at) = surface_point(&w, &bubble, x, y) else { return };
            let hit = bubble.hit(&view, position, picked.as_ref());
            if matches!(hit, Some(BubbleHit::Ring | BubbleHit::Intensity)) && bubble.repeated(gesture.current_event_time(), position) {
                send(&w, if hit == Some(BubbleHit::Intensity) { BubbleAction::SetIntensity { id, stops: 0. } } else { BubbleAction::ResetRing { id, position } });
                gesture.set_state(gtk::EventSequenceState::Claimed);
                return;
            }
            let state = match bubble.begin_contact(&w, id, hit, position, at) {
                None => gtk::EventSequenceState::Denied,
                Some(Contact::Ring | Contact::Choose | Contact::Wheel(_) | Contact::Intensity) => gtk::EventSequenceState::Claimed,
                Some(_) => return,
            };
            gesture.set_state(state);
        }));
        drag.connect_drag_update(glib::clone!(#[weak] w, move |gesture, dx, dy| {
            let Some(bubble) = gesture.widget().and_downcast::<Bubble>() else { return };
            let Some((x0, y0)) = gesture.start_point() else { return };
            let (x, y) = (x0 + dx, y0 + dy);
            let position = local(&bubble, x, y);
            match bubble.imp().contact.get() {
                Some(Contact::Wheel(part)) => {
                    if let Some(position) = bubble.wheel_position(position) {
                        send(&w, BubbleAction::Wheel { id, phase: ContactPhase::Move, part, position });
                    }
                }
                Some(Contact::Intensity) => send(&w, BubbleAction::Intensity { id, phase: ContactPhase::Move, position }),
                Some(Contact::Ring) => send(&w, BubbleAction::Ring { id, phase: ContactPhase::Move, position }),
                Some(Contact::Choose) => {
                    bubble.hot_item(position);
                    send(&w, BubbleAction::Choose { id, phase: ContactPhase::Move, position });
                }
                Some(Contact::Beads(claimed)) => {
                    if !claimed && w.surface.drag_check_threshold(0, 0, dx as i32, dy as i32) {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        bubble.imp().contact.set(Some(Contact::Beads(true)));
                    }
                    send(&w, BubbleAction::Beads { id, phase: ContactPhase::Move, position });
                }
                Some(Contact::Pending(origin)) => {
                    if !w.surface.drag_check_threshold(0, 0, dx as i32, dy as i32) {
                        return;
                    }
                    let Some(at) = surface_point(&w, &bubble, x, y) else { return };
                    let grab = BubbleGrab::Bubble { id };
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    if w.bubbles.drag(&w, grab, ContactPhase::Down, origin) {
                        bubble.imp().contact.set(Some(Contact::Moving));
                        w.bubbles.drag(&w, grab, ContactPhase::Move, at);
                    } else {
                        bubble.imp().contact.set(None);
                    }
                }
                Some(Contact::Moving) => {
                    if let Some(at) = surface_point(&w, &bubble, x, y) {
                        w.bubbles.drag(&w, BubbleGrab::Bubble { id }, ContactPhase::Move, at);
                    }
                }
                Some(Contact::Pair) | None => {}
            }
        }));
        drag.connect_drag_end(glib::clone!(#[weak] w, move |gesture, dx, dy| {
            let Some(bubble) = gesture.widget().and_downcast::<Bubble>() else { return };
            let Some((x0, y0)) = gesture.start_point() else { return };
            if bubble.imp().contact.get() == Some(Contact::Pair) {
                return;
            }
            let (x, y) = (x0 + dx, y0 + dy);
            let position = local(&bubble, x, y);
            match bubble.imp().contact.take() {
                Some(Contact::Wheel(part)) => send(&w, BubbleAction::Wheel { id, phase: ContactPhase::Up, part, position: [0.; 2] }),
                Some(Contact::Intensity) => send(&w, BubbleAction::Intensity { id, phase: ContactPhase::Up, position }),
                Some(Contact::Ring) => send(&w, BubbleAction::Ring { id, phase: ContactPhase::Up, position }),
                Some(Contact::Choose) => {
                    bubble.imp().hot.set(None);
                    send(&w, BubbleAction::Choose { id, phase: ContactPhase::Up, position });
                }
                Some(Contact::Beads(_)) => send(&w, BubbleAction::Beads { id, phase: ContactPhase::Up, position }),
                Some(Contact::Moving) => {
                    if let Some(at) = surface_point(&w, &bubble, x, y) {
                        w.bubbles.drag(&w, BubbleGrab::Bubble { id }, ContactPhase::Up, at);
                    }
                }
                Some(Contact::Pending(_)) => send(&w, BubbleAction::Tap { id, position }),
                Some(Contact::Pair) | None => {}
            }
        }));
        drag.connect_cancel(glib::clone!(#[weak] w, move |gesture, _| {
            let Some(bubble) = gesture.widget().and_downcast::<Bubble>() else { return };
            if bubble.imp().contact.get() != Some(Contact::Pair) {
                bubble.cancel_contact(&w, id, [0.; 2]);
            }
        }));
        self.add_controller(drag);

        let fingers = gtk::EventControllerLegacy::new();
        fingers.set_propagation_phase(gtk::PropagationPhase::Capture);
        fingers.connect_event(glib::clone!(#[weak] w, #[upgrade_or] glib::Propagation::Proceed, move |controller, event| {
            let Some(bubble) = controller.widget().and_downcast::<Bubble>() else { return glib::Propagation::Proceed };
            if matches!(event.event_type(), gdk::EventType::EnterNotify | gdk::EventType::MotionNotify | gdk::EventType::ButtonPress | gdk::EventType::ProximityIn) {
                let hide = event.device_tool().is_some()
                    || event.device().is_some_and(|d| d.source() == gdk::InputSource::Pen)
                    || bubble.imp().stamp_patch.borrow().is_some();
                if bubble.cursor().is_some() != hide {
                    bubble.set_cursor_from_name(hide.then_some("none"));
                }
            }
            let point = w.event_point(controller);
            let color = bubble.imp().view.borrow().as_ref().filter(|v| v.kind == BubbleKind::Color)
                .map(|v| point.is_some_and(|at| v.hit(bubble.local(at)).is_some()));
            if let Some(inside) = color {
                bubble.imp().color_touches.forward(&w, bubble.upcast_ref(), event, point, inside);
            }
            let Some(at) = point else { return glib::Propagation::Proceed };
            bubble.mark_parts(event, at);
            let sequence = || event.event_sequence().as_ptr() as usize;
            let stop = match event.event_type() {
                gdk::EventType::TouchBegin => bubble.finger_down(&w, id, sequence(), at),
                gdk::EventType::TouchUpdate => bubble.finger_moved(&w, id, sequence(), at),
                gdk::EventType::TouchEnd => bubble.finger_up(&w, id, sequence(), at, false),
                gdk::EventType::TouchCancel => bubble.finger_up(&w, id, sequence(), at, true),
                gdk::EventType::ButtonPress => {
                    if event.downcast_ref::<gdk::ButtonEvent>().is_some_and(|b| b.button() == gdk::BUTTON_PRIMARY) {
                        bubble.press_down(&w, id, None, at);
                    }
                    false
                }
                gdk::EventType::MotionNotify => bubble.press_moved(&w, id, None, at),
                gdk::EventType::ButtonRelease => {
                    bubble.press_up(&w, id, None, at, false);
                    false
                }
                _ => false,
            };
            if stop { glib::Propagation::Stop } else { glib::Propagation::Proceed }
        }));
        self.add_controller(fingers);

        let hover = gtk::EventControllerMotion::new();
        let hovered = |bubble: &Bubble, x: f64, y: f64| {
            let [cx, cy] = bubble.center();
            bubble.mark("hovered", false, bubble.part_at([x as f32 - cx, y as f32 - cy]));
        };
        hover.connect_enter(move |controller, x, y| {
            if let Some(bubble) = controller.widget().and_downcast::<Bubble>() {
                hovered(&bubble, x, y);
            }
        });
        hover.connect_motion(move |controller, x, y| {
            if let Some(bubble) = controller.widget().and_downcast::<Bubble>() {
                hovered(&bubble, x, y);
            }
        });
        hover.connect_leave(|controller| {
            if let Some(bubble) = controller.widget().and_downcast::<Bubble>() {
                bubble.mark("hovered", false, None);
            }
        });
        self.add_controller(hover);
    }

    fn part_at(&self, p: [f32; 2]) -> Option<BubbleHit> {
        let view = self.imp().view.borrow();
        let view = view.as_ref()?;
        view.hit(p).filter(|hit| match hit {
            BubbleHit::Item(_) | BubbleHit::Half(_) => true,
            BubbleHit::Face => !matches!(view.kind, BubbleKind::Color | BubbleKind::History),
            _ => false,
        })
    }

    fn part_widget(&self, part: BubbleHit) -> Option<gtk::Widget> {
        let imp = self.imp();
        match part {
            BubbleHit::Face => imp.face.borrow().as_ref().map(|f| f.clone().upcast()),
            BubbleHit::Half(i) => imp.halves.borrow().get(i).map(|h| h.clone().upcast()),
            BubbleHit::Item(i) => imp.items.borrow().get(i).map(|item| item.root.clone().upcast()),
            _ => None,
        }
    }

    fn mark(&self, class: &str, pressed: bool, part: Option<BubbleHit>) {
        let cell = if pressed { &self.imp().pressed } else { &self.imp().hovered };
        let old = cell.replace(part);
        if old == part {
            return;
        }
        if let Some(widget) = old.and_then(|p| self.part_widget(p)) {
            widget.remove_css_class(class);
        }
        if let Some(widget) = part.and_then(|p| self.part_widget(p)) {
            widget.add_css_class(class);
        }
    }

    fn mark_parts(&self, event: &gdk::Event, at: [f32; 2]) {
        let p = self.local(at);
        let first = self.imp().fingers.borrow().is_empty();
        match event.event_type() {
            gdk::EventType::ButtonPress | gdk::EventType::TouchBegin if first => self.mark("pressed", true, self.part_at(p)),
            gdk::EventType::ButtonRelease | gdk::EventType::TouchEnd | gdk::EventType::TouchCancel => self.mark("pressed", true, None),
            gdk::EventType::MotionNotify | gdk::EventType::TouchUpdate
                if self.imp().pressed.get().is_some()
                    && self.imp().press.borrow().as_ref().is_none_or(|press| (at[0] - press.start[0]).hypot(at[1] - press.start[1]) > HOLD_SLOP) =>
            {
                self.mark("pressed", true, None)
            }
            _ => {}
        }
    }

    fn repeated(&self, time: u32, at: [f32; 2]) -> bool {
        let (interval, distance) = gtk::Settings::default().map_or((400, 5), |s| (s.gtk_double_click_time(), s.gtk_double_click_distance()));
        let last = self.imp().last_ring.replace(Some((time, at)));
        let repeated = last.is_some_and(|(t, p)| time.wrapping_sub(t) <= interval as u32 && (at[0] - p[0]).hypot(at[1] - p[1]) <= (distance as f32).max(HOLD_SLOP));
        if repeated {
            self.imp().last_ring.set(None);
        }
        repeated
    }

    fn local(&self, at: [f32; 2]) -> [f32; 2] {
        let center = self.imp().placement.get().map_or([0.; 2], |p| p.center);
        [at[0] - center[0], at[1] - center[1]]
    }

    fn wheel_position(&self, position: [f32; 2]) -> Option<[f32; 2]> {
        let wheel = self.imp().wheel.borrow().as_ref()?.clone();
        let point = self.compute_point(&wheel, &graphene::Point::new(position[0] + self.width() as f32 * 0.5, position[1] + self.height() as f32 * 0.5))?;
        let (size, origin) = wheel.drawing_bounds();
        let diameter = self.imp().view.borrow().as_ref()?.diameter - RING_RIM * 2.;
        (size > 0.).then(|| [(point.x() - origin[0]) / size * diameter - diameter * 0.5, (point.y() - origin[1]) / size * diameter - diameter * 0.5])
    }

    fn wheel_contact(&self, w: &Rc<Workspace>, position: [f32; 2]) -> Option<(ColorWheelPart, [f32; 2])> {
        let wheel = self.imp().wheel.borrow().as_ref()?.clone();
        let point = self.compute_point(&wheel, &graphene::Point::new(position[0] + self.width() as f32 * 0.5, position[1] + self.height() as f32 * 0.5))?;
        if wheel.pick(point.x() as f64, point.y() as f64, gtk::PickFlags::DEFAULT).is_some_and(|picked| picked != wheel.clone().upcast::<gtk::Widget>()) {
            return None;
        }
        let position = self.wheel_position(position)?;
        let size = self.imp().view.borrow().as_ref()?.diameter - RING_RIM * 2.;
        let shape = w.gpu.borrow().as_ref()?.session.state().preview_colors().shape;
        ColorWheelGeometry::new(size)?.hit_shape(position.map(|v| v + size / 2.), shape).map(|part| (part, position))
    }

    fn begin_contact(&self, w: &Rc<Workspace>, id: u32, hit: Option<BubbleHit>, position: [f32; 2], at: [f32; 2]) -> Option<Contact> {
        let send = |action: BubbleAction| w.dispatch(UiAction::Bubble { action });
        let phase = ContactPhase::Down;
        let contact = match hit? {
            BubbleHit::Pill => return None,
            BubbleHit::Wheel => {
                let (part, position) = self.wheel_contact(w, position)?;
                send(BubbleAction::Wheel { id, phase, part, position });
                Contact::Wheel(part)
            }
            BubbleHit::Intensity => {
                send(BubbleAction::Intensity { id, phase, position });
                if let Some(scale) = self.imp().wheel.borrow().as_ref().and_then(|wheel| wheel.intensity()) { scale.grab_focus(); }
                Contact::Intensity
            }
            BubbleHit::Card | BubbleHit::Face | BubbleHit::Half(_) => Contact::Pending(at),
            BubbleHit::Ring => {
                send(BubbleAction::Ring { id, phase, position });
                Contact::Ring
            }
            BubbleHit::Item(_) => {
                self.hot_item(position);
                send(BubbleAction::Choose { id, phase, position });
                Contact::Choose
            }
            BubbleHit::Bead => {
                send(BubbleAction::Beads { id, phase, position });
                Contact::Beads(false)
            }
        };
        self.imp().contact.set(Some(contact));
        Some(contact)
    }

    fn cancel_contact(&self, w: &Rc<Workspace>, id: u32, at: [f32; 2]) {
        let send = |action: BubbleAction| w.dispatch(UiAction::Bubble { action });
        let (phase, position) = (ContactPhase::Cancel, [0.; 2]);
        match self.imp().contact.take() {
            Some(Contact::Wheel(part)) => send(BubbleAction::Wheel { id, phase, part, position }),
            Some(Contact::Intensity) => send(BubbleAction::Intensity { id, phase, position }),
            Some(Contact::Ring) => send(BubbleAction::Ring { id, phase, position }),
            Some(Contact::Choose) => {
                self.imp().hot.set(None);
                send(BubbleAction::Choose { id, phase, position });
            }
            Some(Contact::Beads(_)) => send(BubbleAction::Beads { id, phase, position }),
            Some(Contact::Moving) => {
                w.bubbles.drag(w, BubbleGrab::Bubble { id }, phase, at);
            }
            _ => {}
        }
    }

    fn press_down(&self, w: &Rc<Workspace>, id: u32, sequence: Option<usize>, at: [f32; 2]) {
        if let Some(mut previous) = self.imp().press.take() {
            previous.forget();
        }
        let Some(view) = self.imp().view.borrow().clone() else { return };
        let Some(point) = w.surface.compute_point(self, &graphene::Point::new(at[0], at[1])) else { return };
        let (x, y) = (point.x() as f64, point.y() as f64);
        let p = self.local(at);
        let picked = self.pick(x, y, gtk::PickFlags::DEFAULT);
        let shown = view.hit(p);
        let wheel = matches!(shown, Some(BubbleHit::Wheel | BubbleHit::Intensity));
        let input = shown == Some(BubbleHit::Card) && self.imp().card.borrow().as_ref().is_some_and(|c| c.input(picked.as_ref()));
        if shown.is_none() || input || !wheel && !super::customization::owns_context(self.upcast_ref(), x, y) {
            return;
        }
        let hit = self.hit(&view, p, picked.as_ref());
        let long = gtk::Settings::default().map_or(500, |s| s.gtk_long_press_time()) as u64;
        let delay = if shown == Some(BubbleHit::Intensity) || wheel && self.wheel_contact(w, p).is_some() { long * 2 } else { long };
        let hold = glib::timeout_add_local_once(
            std::time::Duration::from_millis(delay),
            glib::clone!(#[weak(rename_to = bubble)] self, #[weak] w, move || bubble.press_held(&w, id)),
        );
        *self.imp().press.borrow_mut() = Some(Press { sequence, hit, start: at, moving: false, held: false, hold: Some(hold) });
    }

    fn press_held(&self, w: &Rc<Workspace>, id: u32) {
        let start = {
            let mut press = self.imp().press.borrow_mut();
            let Some(press) = press.as_mut() else { return };
            press.hold = None;
            if press.moving {
                return;
            }
            press.held = true;
            press.start
        };
        self.cancel_contact(w, id, start);
        let [x, y] = self.local(start);
        let [cx, cy] = self.center();
        w.show_held_context(self.upcast_ref(), ContextTarget::Bubble { id }, (cx + x) as f64, (cy + y) as f64);
    }

    fn press_moved(&self, w: &Rc<Workspace>, id: u32, sequence: Option<usize>, at: [f32; 2]) -> bool {
        let imp = self.imp();
        let (held, moving, hit) = match imp.press.borrow_mut().as_mut().filter(|c| c.sequence == sequence) {
            Some(press) => {
                let far = (at[0] - press.start[0]).hypot(at[1] - press.start[1]) > HOLD_SLOP;
                if press.held && !far {
                    return true;
                }
                if far {
                    press.forget();
                }
                let held = std::mem::take(&mut press.held);
                (held, press.moving, press.hit)
            }
            None => return false,
        };
        if held {
            w.dismiss_context();
            self.begin_contact(w, id, hit, self.local(at), at);
            return false;
        }
        if moving {
            w.bubbles.drag(w, BubbleGrab::Rim { id }, ContactPhase::Move, at);
            return true;
        }
        let size = imp.view.borrow().as_ref().map_or(0., |v| v.diameter);
        if hit != Some(BubbleHit::Wheel) || !color_drag_out(size, self.local(at)) { return false; }
        self.cancel_contact(w, id, at);
        let lifted = w.bubbles.drag(w, BubbleGrab::Rim { id }, ContactPhase::Down, at);
        match imp.press.borrow_mut().as_mut() {
            Some(press) if lifted => press.moving = true,
            _ => return false,
        }
        true
    }

    fn press_up(&self, w: &Rc<Workspace>, id: u32, sequence: Option<usize>, at: [f32; 2], cancel: bool) {
        let Some(mut press) = self.imp().press.borrow_mut().take_if(|c| c.sequence == sequence) else { return };
        press.forget();
        if press.held && cancel {
            w.dismiss_context();
        }
        if press.moving {
            w.bubbles.drag(w, BubbleGrab::Rim { id }, if cancel { ContactPhase::Cancel } else { ContactPhase::Up }, at);
        } else if cancel {
            self.cancel_contact(w, id, at);
        }
    }

    fn pair_points(&self) -> Option<([f32; 2], f32)> {
        let fingers = self.imp().fingers.borrow();
        let [(_, a), (_, b)] = [*fingers.first()?, *fingers.get(1)?];
        Some(([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5], (a[0] - b[0]).hypot(a[1] - b[1])))
    }

    fn finger_down(&self, w: &Rc<Workspace>, id: u32, sequence: usize, at: [f32; 2]) -> bool {
        let count = {
            let mut fingers = self.imp().fingers.borrow_mut();
            fingers.push((sequence, at));
            fingers.len()
        };
        match count {
            1 => {
                self.press_down(w, id, Some(sequence), at);
                false
            }
            2 => {
                self.begin_pair(w, id);
                true
            }
            _ => true,
        }
    }

    fn finger_moved(&self, w: &Rc<Workspace>, id: u32, sequence: usize, at: [f32; 2]) -> bool {
        if let Some(finger) = self.imp().fingers.borrow_mut().iter_mut().find(|f| f.0 == sequence) {
            finger.1 = at;
        }
        match self.imp().pair.get() {
            Some(pair) => {
                if !pair.ended {
                    self.update_pair(w, id);
                }
                true
            }
            None => self.press_moved(w, id, Some(sequence), at),
        }
    }

    fn finger_up(&self, w: &Rc<Workspace>, id: u32, sequence: usize, at: [f32; 2], cancel: bool) -> bool {
        let imp = self.imp();
        imp.fingers.borrow_mut().retain(|f| f.0 != sequence);
        match imp.pair.get() {
            Some(pair) => {
                if !pair.ended {
                    self.end_pair(w, id, cancel);
                }
                if imp.fingers.borrow().is_empty() {
                    imp.pair.set(None);
                }
            }
            None => self.press_up(w, id, Some(sequence), at, cancel),
        }
        false
    }

    fn begin_pair(&self, w: &Rc<Workspace>, id: u32) {
        let imp = self.imp();
        let Some((mid, distance)) = self.pair_points() else { return };
        self.cancel_contact(w, id, mid);
        imp.contact.set(Some(Contact::Pair));
        if let Some(mut press) = imp.press.take() {
            press.forget();
            if press.held {
                w.dismiss_context();
            }
            if press.moving {
                w.bubbles.drag(w, BubbleGrab::Rim { id }, ContactPhase::Cancel, mid);
            }
        }
        imp.hot.set(None);
        imp.pair.set(Some(Pair { start: distance.max(1.), origin: mid, mid, lifted: false, ended: false }));
    }

    fn update_pair(&self, w: &Rc<Workspace>, id: u32) {
        let imp = self.imp();
        let (Some((mid, distance)), Some(mut pair)) = (self.pair_points(), imp.pair.get()) else { return };
        if !pair.lifted {
            let moved = w.surface.drag_check_threshold(0, 0, (mid[0] - pair.origin[0]) as i32, (mid[1] - pair.origin[1]) as i32)
                || w.surface.drag_check_threshold(0, 0, (distance - pair.start) as i32, 0);
            if !moved {
                return;
            }
            pair.lifted = w.bubbles.drag(w, BubbleGrab::Bubble { id }, ContactPhase::Down, pair.origin);
            if !pair.lifted {
                imp.contact.set(None);
                imp.pair.set(Some(Pair { ended: true, ..pair }));
                return;
            }
        }
        imp.pair.set(Some(Pair { mid, ..pair }));
        let color = imp.view.borrow().as_ref().filter(|v| v.kind == BubbleKind::Color).map(|v| v.diameter);
        if let Some(size) = color {
            imp.pinch.set(Some(pinched_size(size, distance / pair.start) / size));
            self.queue_draw();
        }
        w.bubbles.drag(w, BubbleGrab::Bubble { id }, ContactPhase::Move, mid);
    }

    fn end_pair(&self, w: &Rc<Workspace>, id: u32, cancel: bool) {
        let imp = self.imp();
        let Some(pair) = imp.pair.get() else { return };
        imp.pair.set(Some(Pair { ended: true, ..pair }));
        imp.contact.set(None);
        if !pair.lifted {
            return;
        }
        w.bubbles.drag(w, BubbleGrab::Bubble { id }, if cancel { ContactPhase::Cancel } else { ContactPhase::Up }, pair.mid);
        if let Some(pinch) = imp.pinch.take() {
            self.queue_draw();
            if !cancel && (pinch - 1.).abs() > 0.01 {
                w.dispatch(UiAction::Bubble { action: BubbleAction::Resize { id, scale: pinch } });
            }
        }
    }
}


impl Layer {
    fn bubbles(&self) -> Vec<Bubble> {
        let mut out = Vec::new();
        let mut child = self.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            out.push(widget.downcast::<Bubble>().unwrap());
        }
        out
    }
}

pub(crate) struct Bubbles {
    pub(super) layer: Layer,
    active_card: RefCell<Option<Bubble>>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
    restore: Rc<RefCell<Option<glib::SourceId>>>,
    theme: Cell<Option<Theme>>,
}

impl Bubbles {
    pub(super) fn new() -> Self {
        let layer: Layer = glib::Object::new();
        layer.set_widget_name("tool-bubbles");
        Self { layer, active_card: RefCell::new(None), tick: RefCell::new(None), restore: Rc::default(), theme: Cell::new(None) }
    }

    pub(super) fn measure_cards(&self, w: &Rc<Workspace>) {
        let Some(bubble) = self.active_card.borrow().clone() else { return };
        let metrics = {
            let view = bubble.imp().view.borrow();
            let Some(card) = view.as_ref().and_then(|view| view.card.as_ref()) else { return };
            let body = bubble.imp().card.borrow();
            let Some(body) = body.as_ref() else { return };
            let (measurement, properties_height) = body.inspector.measure(card.size[0].round() as i32, w.surface.height() as f32);
            (bubble.id(), card.full, measurement, properties_height)
        };
        if w.gpu.borrow().as_ref().is_none_or(|gpu| gpu.session.bubble_card_metrics() == Some(metrics)) { return; }
        let (id, _, measurement, properties_height) = metrics;
        w.measure(UiAction::Bubble { action: BubbleAction::MeasureCard { id, measurement, properties_height } });
        let view = w.gpu.borrow().as_ref().and_then(|gpu|
            gpu.session.bubbles().into_iter().find(|view| view.id == id));
        if let Some(view) = view { bubble.set_view(view); }
    }

    pub(super) fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        self.active_card.borrow_mut().take();
        let animations = gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations());
        let Some((views, (stroke, faded))) = w.gpu.borrow_mut().as_mut().map(|g| {
            g.session.reduce_bubble_motion(!animations);
            (g.session.bubbles(), g.session.bubble_fade())
        }) else {
            return;
        };
        w.surface.imp().layout.borrow_mut().bubbles = state.workspace.layout.bubbles.clone();
        let colors = w.palette.get().map(|palette| {
            let [r, g, b, a] = palette.glass.panel.0;
            let rgb = |c: HexColor| gdk::RGBA::new(c.0[0] as f32 / 255., c.0[1] as f32 / 255., c.0[2] as f32 / 255., 1.);
            [gdk::RGBA::new(r, g, b, a), rgb(palette.text), rgb(palette.thumb)]
        });
        let retheme = self.theme.replace(Some(state.theme)) != Some(state.theme);
        for bubble in self.layer.bubbles() {
            let keep = views.iter().find(|v| v.id == bubble.id()).is_some_and(|v| {
                bubble.imp().view.borrow().as_ref().is_some_and(|old| old.source == v.source)
            });
            if !keep {
                bubble.unparent();
            }
        }
        for view in views {
            let bubble = self.layer.bubbles().into_iter().find(|b| b.id() == view.id).unwrap_or_else(|| {
                let bubble = Bubble::new(w, &view, state.theme);
                bubble.set_parent(&self.layer);
                bubble
            });
            if bubble.imp().colors.replace(colors) != colors {
                bubble.queue_draw();
            }
            if retheme && let Some(id) = view.preview.filter(|_| bubble.imp().preview.borrow().is_some()) {
                let preview = crate::previews::texture(id, state.theme);
                crate::previews::redraw_when_loaded(&preview, &bubble);
                *bubble.imp().preview.borrow_mut() = Some(preview);
            }
            bubble.imp().display.set(Some((w.paint_view_color(), w.picker_headroom())));
            if let (Some(thumbnail), Some(layer)) = (bubble.imp().thumbnail.borrow().as_ref(), view.layer) {
                thumbnail.set_paintable(w.layer_panel.preview(layer).as_ref());
            }
            bubble.set_card(w, state, &view);
            if view.card.is_some() { *self.active_card.borrow_mut() = Some(bubble.clone()); }
            bubble.set_stamp(w, &view);
            if bubble.imp().view.borrow().as_ref() != Some(&view) || retheme {
                bubble.set_beads(w, &view);
                bubble.set_items(&view, state.theme);
                bubble.set_view(view);
            }
        }
        self.fade(stroke, &faded);
        self.refresh_wheels(w, &state.preview_colors(), w.paint_view_color(), w.picker_headroom(), state.color_picker.preview.is_some());
        self.animate(w);
    }

    fn fade(&self, stroke: bool, faded: &[u32]) {
        let show = |bubble: &Bubble, faded: bool| {
            bubble.fade_to(if faded { FADED_BUBBLE as f64 } else { 1. });
            bubble.set_can_target(!faded);
        };
        if stroke {
            if let Some(source) = self.restore.take() {
                source.remove();
            }
            for bubble in self.layer.bubbles() {
                show(&bubble, faded.contains(&bubble.id()));
            }
            return;
        }
        if self.restore.borrow().is_some() || self.layer.bubbles().iter().all(|b| b.can_target()) {
            return;
        }
        let layer = self.layer.downgrade();
        let restore = self.restore.clone();
        let source = glib::timeout_add_local_once(std::time::Duration::from_millis(FADE_RETURN_MS), move || {
            restore.take();
            for bubble in layer.upgrade().iter().flat_map(Layer::bubbles) {
                show(&bubble, false);
            }
        });
        *self.restore.borrow_mut() = Some(source);
    }

    pub(crate) fn layer_panels(&self) -> Vec<Rc<LayerPanel>> {
        self.layer.bubbles().iter().filter_map(|b| b.imp().card.borrow().as_ref().filter(|c| c.root.is_child_visible()).map(|c| c.inspector.layers.clone())).collect()
    }

    pub(crate) fn thumbnails(&self) -> Vec<(u64, gtk::Picture)> {
        self.layer
            .bubbles()
            .iter()
            .filter_map(|b| Some((b.imp().view.borrow().as_ref()?.layer?, b.imp().thumbnail.borrow().clone()?)))
            .collect()
    }

    pub(super) fn refresh_wheels(&self, w: &Workspace, colors: &ColorState, view: crate::display_color::ViewColor, headroom: f32, preview: bool) {
        for bubble in self.layer.bubbles() {
            if let Some(wheel) = bubble.imp().wheel.borrow().as_ref() {
                let range = w.gpu.borrow().as_ref().map(|g| g.session.bubble_intensity_range(bubble.id(), colors));
                wheel.set_state(colors, view, headroom, preview, range);
            }
        }
    }

    pub(super) fn drag(&self, w: &Rc<Workspace>, grab: BubbleGrab, phase: ContactPhase, position: [f32; 2]) -> bool {
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let action = UiAction::Bubble { action: BubbleAction::Drag { grab, phase, position, viewport } };
        let result = w.gpu.borrow_mut().as_mut().map(|g| g.session.dispatch(action));
        let accepted = matches!(result, Some(Ok(_)));
        match result {
            Some(Ok(change)) if change.regions == 0 && !change.canvas_wake => {}
            Some(Ok(change)) => w.changed(Ok(change)),
            _ => {}
        }
        self.animate(w);
        accepted
    }

    pub(super) fn animate(&self, w: &Rc<Workspace>) {
        if self.tick.borrow().is_some() {
            return;
        }
        let tick = self.layer.add_tick_callback(glib::clone!(
            #[weak] w,
            #[upgrade_or] glib::ControlFlow::Break,
            move |layer, clock| {
                let now = clock.frame_time().max(0) as u64 * 1000;
                let frame = w.gpu.borrow_mut().as_mut().map(|g| {
                    let moving = g.session.step_bubbles(now);
                    (moving, g.session.bubble_placements())
                });
                let Some((moving, placements)) = frame else {
                    w.bubbles.tick.take();
                    return glib::ControlFlow::Break;
                };
                *layer.imp().placements.borrow_mut() = placements;
                layer.queue_allocate();
                if moving {
                    glib::ControlFlow::Continue
                } else {
                    w.bubbles.tick.take();
                    glib::ControlFlow::Break
                }
            }
        ));
        *self.tick.borrow_mut() = Some(tick);
    }
}
