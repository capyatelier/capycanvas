//! Retained tab buttons adapt to their actual allocation and native font metrics.
//! Shared Rust chooses which complete labels fit; native input targets survive.
use gtk::{glib, prelude::*, subclass::prelude::*};
use layer_ui::TabStyle;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use gtk::{gdk, graphene, gsk};
pub(crate) const TAB_RADIUS: f32 = layer_ui::SURFACE_RADIUS * crate::squircle::CORNER_FIT;
type CornerPaths = HashMap<[u32; 8], (gsk::Path, gsk::Path)>;
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Strip {
        pub buttons: RefCell<Vec<gtk::Button>>,
        pub style: Cell<TabStyle>,
        pub hover_bottom: RefCell<Vec<[i8; 2]>>,
        pub tints: RefCell<Option<[gtk::Box; 2]>>,
        pub gpu_background: Cell<bool>,
        pub(super) paths: RefCell<CornerPaths>,
        pub(super) hover_path: RefCell<Option<([u32; 6], gsk::Path)>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Strip {
        const NAME: &'static str = "LayerPanelTabs";
        type Type = super::PanelTabs;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Strip {
        fn dispose(&self) {
            for button in self.buttons.take() {
                button.unparent();
            }
            for tint in self.tints.take().into_iter().flatten() {
                tint.unparent();
            }
        }
    }
    impl WidgetImpl for Strip {
        fn measure(&self, axis: gtk::Orientation, size: i32) -> (i32, i32, i32, i32) {
            if axis == gtk::Orientation::Horizontal && self.style.get() == TabStyle::Automatic {
                let widths = self.obj().widths();
                return (
                    widths.iter().map(|w| w[1]).sum::<f32>().ceil() as i32,
                    widths.iter().map(|w| w[0]).sum::<f32>().ceil() as i32,
                    -1,
                    -1,
                );
            }
            let mut min = 0;
            let mut natural = 0;
            for button in self.buttons.borrow().iter() {
                let (m, n, _, _) = button.measure(axis, size);
                if axis == gtk::Orientation::Horizontal {
                    min += m;
                    natural += n;
                } else {
                    min = min.max(m);
                    natural = natural.max(n);
                }
            }
            (min, natural, -1, -1)
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let widths = self.obj().widths();
            let automatic = self.style.get() == TabStyle::Automatic;
            let names = TabStyle::automatic_names(width as f32, &widths);
            let mut x = 0.;
            for (i, button) in self.buttons.borrow().iter().enumerate() {
                let w = if automatic {
                    let content = button.child().unwrap();
                    content.first_child().unwrap().set_visible(true);
                    content.last_child().unwrap().set_visible(names[i]);
                    if names[i] {
                        button.remove_css_class("icon-only-tab");
                    } else {
                        button.add_css_class("icon-only-tab");
                    }
                    widths[i][usize::from(!names[i])]
                } else {
                    button.measure(gtk::Orientation::Horizontal, -1).1 as f32
                };
                let end = x + w;
                button.allocate(
                    (end.round() - x.round()) as i32,
                    height,
                    -1,
                    Some(
                        gtk::gsk::Transform::new()
                            .translate(&gtk::graphene::Point::new(x.round(), 0.)),
                    ),
                );
                x = end;
            }
            for tint in self.tints.borrow().iter().flatten() {
                tint.allocate(0, 0, -1, None);
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let buttons = self.buttons.borrow();
            let bounds = gtk::graphene::Rect::new(0., 0., obj.width() as f32, obj.height() as f32);
            let Some([strip, foot]) = self.tints.borrow().clone() else { return };
            let selected = buttons.iter().find(|b| b.has_css_class("selected-tool") && b.opacity() > 0.);
            let active = selected.and_then(|tab| tab.compute_bounds(&*obj));
            let gpu = self.gpu_background.get();
            let hovered = gtk::Snapshot::new();
            let corners = self.hover_bottom.borrow();
            for (i, button) in buttons.iter().enumerate().filter(|(_, button)| Some(*button) != selected) {
                if button.is_sensitive() && button.opacity() > 0. && !button.has_css_class("selected-tool")
                    && button.state_flags().contains(gtk::StateFlags::PRELIGHT)
                    && let Some(tab) = button.compute_bounds(&*obj) {
                        let corner = corners.get(i).copied().unwrap_or_default();
                        let key = [tab.x(), tab.y(), tab.width(), tab.height(), corner[0] as f32, corner[1] as f32].map(f32::to_bits);
                        let mut cached = self.hover_path.borrow_mut();
                        if cached.as_ref().is_none_or(|(previous, _)| *previous != key) {
                            *cached = Some((key, crate::squircle::path(|cr| hover(cr, &tab, corner))));
                        }
                        let mut color = button.color();
                        color.set_alpha((f64::from(color.alpha()) * 0.08 * button.opacity()) as f32);
                        hovered.append_node(gsk::FillNode::new(gsk::ColorNode::new(&color, &bounds),
                            &cached.as_ref().unwrap().1, gsk::FillRule::Winding));
                    }
            }
            let hovered = hovered.to_node();
            let node = if gpu {
                hovered.unwrap_or_else(|| gsk::ColorNode::new(&gdk::RGBA::TRANSPARENT, &bounds).upcast())
            } else {
                let background = gsk::ColorNode::new(&strip.color(), &bounds);
                let foreground = gsk::ColorNode::new(&foot.color(), &bounds);
                tab_background(&bounds, active.as_ref(), background.upcast_ref(), foreground.upcast_ref(), hovered.as_ref(),
                    crate::squircle::PixelGrid::for_widget(obj.upcast_ref()), &mut self.paths.borrow_mut())
            };
            snapshot.append_node(if strip.color().alpha() < 1. || foot.color().alpha() < 1. {
                crate::glass::surface(node)
            } else { node });
            for button in buttons.iter().filter(|button| Some(*button) != selected) {
                obj.snapshot_child(button, snapshot);
            }
            if let Some(button) = selected {
                obj.snapshot_child(button, snapshot);
            }
        }
    }
}

struct TabRegion {
    bounds: graphene::Rect,
    fill: Option<(gsk::Path, gsk::FillRule)>,
}

impl TabRegion {
    fn rectangle(regions: &mut Vec<Self>, [x, y, width, height]: [f32; 4]) {
        if width > 0. && height > 0. {
            regions.push(Self { bounds: graphene::Rect::new(x, y, width, height), fill: None });
        }
    }

    fn append(&self, snapshot: &gtk::Snapshot, source: &gsk::RenderNode) {
        let Some((path, rule)) = &self.fill else {
            if let Some(color) = source.downcast_ref::<gsk::ColorNode>() {
                snapshot.append_color(&color.color(), &self.bounds);
            } else {
                snapshot.push_clip(&self.bounds);
                snapshot.append_node(source);
                snapshot.pop();
            }
            return;
        };
        let local: gsk::RenderNode = match source.downcast_ref::<gsk::ColorNode>() {
            Some(color) => gsk::ColorNode::new(&color.color(), &graphene::Rect::new(0., 0.,
                self.bounds.width(), self.bounds.height())).upcast(),
            None => gsk::TransformNode::new(source, Some(&gsk::Transform::new().translate(
                &graphene::Point::new(-self.bounds.x(), -self.bounds.y())))).upcast(),
        };
        let fill = gsk::FillNode::new(local, path, *rule);
        snapshot.append_node(gsk::TransformNode::new(fill, Some(&gsk::Transform::new().translate(
            &graphene::Point::new(self.bounds.x(), self.bounds.y())))));
    }
}

fn selected_regions(bounds: &graphene::Rect, tab: &graphene::Rect,
    grid: crate::squircle::PixelGrid, paths: &mut CornerPaths) -> (Vec<TabRegion>, Vec<TabRegion>) {
    let square = graphene::Size::new(0., 0.);
    let radius = graphene::Size::new(layer_ui::SURFACE_RADIUS, layer_ui::SURFACE_RADIUS);
    let mut shape = gsk::RoundedRect::new(*tab, radius, radius, square, square);
    shape.normalize();
    let [left_radius, right_radius, _, _] = *shape.corner();
    let [left, top, right, bottom] = [tab.x(), tab.y(), tab.x() + tab.width(), bounds.y() + bounds.height()];
    let top_start = grid.floor(1, top);
    let top_end = grid.ceil(1, top + left_radius.height().max(right_radius.height()));
    let foot_start = grid.floor(1, bottom - 6.);
    let top_left = [grid.floor(0, left), grid.ceil(0, left + left_radius.width())];
    let top_right = [grid.floor(0, right - right_radius.width()), grid.ceil(0, right)];
    let joined_top = top_left[1] > top_right[0];
    let foot_left = [grid.floor(0, left - 6.), grid.ceil(0, left)];
    let foot_right = [grid.floor(0, right), grid.ceil(0, right + 6.)];
    let far = bounds.x() + bounds.width();
    let mut background = Vec::with_capacity(10);
    let mut foreground = Vec::with_capacity(7);
    for rect in [
        [bounds.x(), bounds.y(), bounds.width(), top_start - bounds.y()],
        [bounds.x(), top_start, top_left[0] - bounds.x(), top_end - top_start],
        [top_right[1], top_start, far - top_right[1], top_end - top_start],
        [bounds.x(), top_end, left - bounds.x(), foot_start - top_end],
        [right, top_end, far - right, foot_start - top_end],
        [bounds.x(), foot_start, foot_left[0] - bounds.x(), bottom - foot_start],
        [foot_right[1], foot_start, far - foot_right[1], bottom - foot_start],
        [top_left[1], top_start, top_right[0] - top_left[1], top - top_start],
    ] { TabRegion::rectangle(&mut background, rect); }
    for rect in [
        [top_left[1], top, top_right[0] - top_left[1], top_end - top],
        [left, top_end, right - left, foot_start - top_end],
        [foot_left[1], foot_start, foot_right[0] - foot_left[1], bottom - foot_start],
    ] { TabRegion::rectangle(&mut foreground, rect); }
    for (kind, x, y, width, height, anchor, radius) in [
        (0u32, top_left[0], top_start, top_left[1] - top_left[0], top_end - top_start,
            [left - top_left[0], top - top_start], [left_radius.width(), left_radius.height()]),
        (1, top_right[0], top_start, top_right[1] - top_right[0], top_end - top_start,
            [right - top_right[0], top - top_start], [right_radius.width(), right_radius.height()]),
        (2, foot_left[0], foot_start, foot_left[1] - foot_left[0], bottom - foot_start,
            [left - foot_left[0], bottom - foot_start], [6., 6.]),
        (3, foot_right[0], foot_start, foot_right[1] - foot_right[0], bottom - foot_start,
            [right - foot_right[0], bottom - foot_start], [6., 6.]),
        (4, top_left[0], top_start, top_right[1] - top_left[0], top_end - top_start,
            [left - top_left[0], top - top_start], [left_radius.width(), left_radius.height()]),
    ] {
        if (kind < 2 && joined_top) || (kind == 4 && !joined_top) { continue; }
        if width <= 0. || height <= 0. { continue; }
        let key = [width, height, anchor[0], anchor[1], radius[0], radius[1], kind as f32,
            if kind == 4 { tab.width() } else { 0. }].map(f32::to_bits);
        let (inside, outside) = paths.entry(key).or_insert_with(|| {
            let outline = |cr: &gtk::cairo::Context| match kind {
                0 => {
                    cr.move_to(f64::from(anchor[0]), f64::from(height));
                    cr.line_to(f64::from(anchor[0]), f64::from(anchor[1] + radius[1]));
                    crate::squircle::corner(cr, [f64::from(anchor[0] + radius[0]), f64::from(anchor[1] + radius[1])],
                        [f64::from(-radius[0]), 0.], [0., f64::from(-radius[1])]);
                    cr.line_to(f64::from(width), f64::from(anchor[1]));
                    cr.line_to(f64::from(width), f64::from(height));
                    cr.close_path();
                }
                1 => {
                    cr.move_to(0., f64::from(anchor[1]));
                    cr.line_to(f64::from(anchor[0] - radius[0]), f64::from(anchor[1]));
                    crate::squircle::corner(cr, [f64::from(anchor[0] - radius[0]), f64::from(anchor[1] + radius[1])],
                        [0., f64::from(-radius[1])], [f64::from(radius[0]), 0.]);
                    cr.line_to(f64::from(anchor[0]), f64::from(height));
                    cr.line_to(0., f64::from(height));
                    cr.close_path();
                }
                2 => {
                    cr.rectangle(f64::from(anchor[0]), 0., f64::from(width - anchor[0]), f64::from(height));
                    crate::squircle::concave_foot(cr, f64::from(anchor[0]), f64::from(anchor[1]), 6., -1.);
                }
                3 => {
                    cr.rectangle(0., 0., f64::from(anchor[0]), f64::from(height));
                    crate::squircle::concave_foot(cr, f64::from(anchor[0]), f64::from(anchor[1]), 6., 1.);
                }
                _ => {
                    let radius = graphene::Size::new(radius[0], radius[1]);
                    let shape = gsk::RoundedRect::new(graphene::Rect::new(anchor[0], anchor[1],
                        tab.width(), height - anchor[1]), radius, radius, square, square);
                    crate::squircle::rounded_rect(cr, &shape);
                }
            };
            let inside = crate::squircle::path(outline);
            let outside = crate::squircle::path(|cr| {
                cr.rectangle(0., 0., f64::from(width), f64::from(height));
                outline(cr);
            });
            (inside, outside)
        });
        let rect = graphene::Rect::new(x, y, width, height);
        foreground.push(TabRegion { bounds: rect, fill: Some((inside.clone(), gsk::FillRule::Winding)) });
        background.push(TabRegion { bounds: rect, fill: Some((outside.clone(), gsk::FillRule::EvenOdd)) });
    }
    (background, foreground)
}

fn tab_background(bounds: &graphene::Rect, tab: Option<&graphene::Rect>, background: &gsk::RenderNode,
    foreground: &gsk::RenderNode, hover: Option<&gsk::RenderNode>, grid: crate::squircle::PixelGrid,
    paths: &mut CornerPaths) -> gsk::RenderNode {
    if paths.len() > 64 { paths.clear(); }
    let (outside, inside) = tab.map_or_else(|| (vec![TabRegion { bounds: *bounds, fill: None }], Vec::new()),
        |tab| selected_regions(bounds, tab, grid, paths));
    let snapshot = gtk::Snapshot::new();
    snapshot.push_clip(bounds);
    for region in &outside { region.append(&snapshot, background); }
    if let Some(hover) = hover { for region in &outside { region.append(&snapshot, hover); } }
    for region in &inside { region.append(&snapshot, foreground); }
    snapshot.pop();
    snapshot.to_node().unwrap_or_else(|| gsk::ColorNode::new(&gdk::RGBA::TRANSPARENT, bounds).upcast())
}


mod bar_imp {
    use super::*;
    #[derive(Default)]
    pub struct Bar {
        pub content: RefCell<Option<gtk::Box>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Bar {
        const NAME: &'static str = "CapyPanelTabBar";
        type Type = super::PanelTabBar;
        type ParentType = gtk::Widget;
        fn class_init(klass: &mut Self::Class) { klass.set_layout_manager_type::<gtk::BinLayout>(); }
    }
    impl ObjectImpl for Bar {
        fn dispose(&self) {
            if let Some(content) = self.content.take() { content.unparent(); }
        }
    }
    impl WidgetImpl for Bar {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(content) = self.content.borrow().clone() else { return };
            let obj = self.obj();
            let bounds = gtk::graphene::Rect::new(0., 0., obj.width() as f32, obj.height() as f32);
            let child = gtk::Snapshot::new();
            obj.snapshot_child(&content, &child);
            let Some(node) = child.to_node() else { return };
            snapshot.append_node(super::tab_bar_clip(&node, &bounds));
        }
    }
}
fn tab_bar_clip(node: &gtk::gsk::RenderNode, bounds: &gtk::graphene::Rect) -> gtk::gsk::RenderNode {
    let radius = gtk::graphene::Size::new(TAB_RADIUS, TAB_RADIUS);
    let square = gtk::graphene::Size::new(0., 0.);
    let mut outline = gtk::gsk::RoundedRect::new(*bounds, radius, radius, square, square);
    outline.normalize();
    gtk::gsk::RoundedClipNode::new(node, &outline).upcast()
}
glib::wrapper! {
    pub struct PanelTabBar(ObjectSubclass<bar_imp::Bar>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
pub(crate) fn bar(content: &gtk::Box) -> PanelTabBar {
    let bar: PanelTabBar = glib::Object::new();
    bar.set_widget_name("panel-tab-bar");
    content.set_parent(&bar);
    *bar.imp().content.borrow_mut() = Some(content.clone());
    bar
}
glib::wrapper! {
    pub struct PanelTabs(ObjectSubclass<imp::Strip>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl PanelTabs {
    pub fn set_gpu_background(&self,enabled: bool) {
        if self.imp().gpu_background.replace(enabled)!=enabled {self.queue_draw();}
    }
    pub fn surface_colors(&self) -> [gtk::gdk::RGBA;2] {
        self.imp().tints.borrow().as_ref().map_or([gtk::gdk::RGBA::TRANSPARENT;2],|tints|tints.each_ref().map(|t|t.color()))
    }
    pub fn new(style: TabStyle) -> Self {
        let tabs: Self = glib::Object::new();
        tabs.imp().style.set(style);
        tabs.set_hexpand(true);
        let tints = ["tab-strip-tint", "tab-foot-tint"].map(|class| {
            let tint = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            tint.add_css_class(class);
            tint.set_can_target(false);
            tint.set_parent(&tabs);
            tint
        });
        *tabs.imp().tints.borrow_mut() = Some(tints);
        tabs
    }
    pub fn set_style(&self, style: TabStyle) {
        if self.imp().style.replace(style) != style {
            self.queue_resize();
        }
    }
    pub fn set_hover_bottom(&self, corners: impl Iterator<Item = [i8; 2]>) {
        let corners: Vec<_> = corners.collect();
        if *self.imp().hover_bottom.borrow() != corners {
            *self.imp().hover_bottom.borrow_mut() = corners;
            self.queue_draw();
        }
    }
    pub fn append(&self, button: &gtk::Button) {
        button.set_cursor_from_name(Some("default"));
        button.set_parent(self);
        button.connect_state_flags_changed(glib::clone!(#[weak(rename_to = tabs)] self, move |_, _| tabs.queue_draw()));
        button.connect_opacity_notify(glib::clone!(#[weak(rename_to = tabs)] self, move |_| tabs.queue_draw()));
        self.imp().buttons.borrow_mut().push(button.clone());
        self.queue_resize();
    }
    fn widths(&self) -> Vec<[f32; 2]> {
        self.imp()
            .buttons
            .borrow()
            .iter()
            .map(|button| {
                let content = button.child().unwrap().downcast::<gtk::Box>().unwrap();
                let icon = content.first_child().unwrap();
                let label = content
                    .last_child()
                    .unwrap()
                    .downcast::<gtk::Label>()
                    .unwrap();
                let text = label.layout().pixel_size().0;
                let face = content.measure(gtk::Orientation::Horizontal, -1).0;
                let minimum = if button.has_css_class("icon-only-tab") {
                    20
                } else {
                    0
                };
                let inset =
                    (button.measure(gtk::Orientation::Horizontal, -1).0 - face.max(minimum)).max(0);
                let icon = icon.measure(gtk::Orientation::Horizontal, -1).1.max(14);
                [
                    (inset + icon + content.spacing() + text) as f32,
                    (inset + icon.max(20)) as f32,
                ]
            })
            .collect()
    }
}

pub(crate) fn hover(cr: &gtk::cairo::Context, tab: &gtk::graphene::Rect, corners: [i8; 2]) {
    let [left, right] = corners.map(f32::from);
    let radius = gtk::graphene::Size::new(layer_ui::SURFACE_RADIUS, layer_ui::SURFACE_RADIUS);
    let lower = |r: f32| gtk::graphene::Size::new(r.max(0.), r.max(0.));
    crate::squircle::rounded_rect(cr, &gtk::gsk::RoundedRect::new(*tab, radius, radius, lower(right), lower(left)));
    for (r, x, direction) in [(left, tab.x(), -1.), (right, tab.x() + tab.width(), 1.)] {
        if r < 0. { crate::squircle::concave_foot(cr, x.into(), (tab.y() + tab.height()).into(), f64::from(-r), direction); }
    }
}

pub(crate) fn feet(cr: &gtk::cairo::Context, tab: &gtk::graphene::Rect, bottom: f32) {
    for (x, direction) in [(tab.x(), -1.), (tab.x() + tab.width(), 1.)] {
        crate::squircle::concave_foot(cr, f64::from(x), f64::from(bottom), 6., direction);
    }
}

pub(crate) fn strip_end(child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let end = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    end.add_css_class("tab-strip-end");
    end.set_halign(gtk::Align::End);
    end.append(child);
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_tabs_paint_their_joined_top_once_at_fractional_device_phases() {
        for width in [36., 37.] { for scale in [1., 2.] {
            let bounds = graphene::Rect::new(0., 0., 100., 36.);
            let tab = graphene::Rect::new(12., 0., width, 36.);
            let grid = crate::squircle::PixelGrid { scale: [scale; 2], offset: [0.25, 0.5] };
            let mut paths = CornerPaths::new();
            let (_, inside) = selected_regions(&bounds, &tab, grid, &mut paths);
            let outline = crate::squircle::path(|cr| {
                let radius = graphene::Size::new(layer_ui::SURFACE_RADIUS, layer_ui::SURFACE_RADIUS);
                let square = graphene::Size::new(0., 0.);
                let mut shape = gsk::RoundedRect::new(tab, radius, radius, square, square);
                shape.normalize();
                crate::squircle::rounded_rect(cr, &shape);
                feet(cr, &tab, bounds.height());
            });
            for y in 1..(bounds.height() * scale) as usize { for x in 0..(bounds.width() * scale) as usize {
                let point = graphene::Point::new((x as f32 + 0.5 - grid.offset[0]) / scale,
                    (y as f32 + 0.5 - grid.offset[1]) / scale);
                let count = inside.iter().filter(|region| region.bounds.contains_point(&point)
                    && region.fill.as_ref().is_none_or(|(path, rule)| path.in_fill(
                        &graphene::Point::new(point.x() - region.bounds.x(), point.y() - region.bounds.y()), *rule))).count();
                assert_eq!(count, usize::from(outline.in_fill(&point, gsk::FillRule::Winding)),
                    "width {width}, scale {scale}, point {point:?}");
            } }
            for alpha in [1f32, 0.56] {
                let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32,
                    (bounds.width() * scale + 2.) as i32, (bounds.height() * scale + 2.) as i32).unwrap();
                {
                    let cr = gtk::cairo::Context::new(&surface).unwrap();
                    cr.translate(grid.offset[0].into(), grid.offset[1].into());
                    cr.scale(scale.into(), scale.into());
                    cr.set_source_rgba(0.2, 0.4, 0.6, f64::from(alpha));
                    for region in &inside {
                        cr.save().unwrap();
                        cr.translate(region.bounds.x().into(), region.bounds.y().into());
                        if let Some((path, rule)) = &region.fill {
                            path.to_cairo(&cr);
                            cr.set_fill_rule(if *rule == gsk::FillRule::EvenOdd {
                                gtk::cairo::FillRule::EvenOdd
                            } else { gtk::cairo::FillRule::Winding });
                        } else { cr.rectangle(0., 0., region.bounds.width().into(), region.bounds.height().into()); }
                        cr.fill().unwrap();
                        cr.restore().unwrap();
                    }
                }
                let x = ((tab.x() + tab.width() * 0.5) * scale + grid.offset[0]).floor() as usize;
                let y = (17. * scale + grid.offset[1]).floor() as usize;
                let offset = y * surface.stride() as usize + x * 4;
                let bytes = surface.data().unwrap();
                let observed = (u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap()) >> 24) as u8;
                assert!(observed.abs_diff((alpha * 255.).round() as u8) <= 1,
                    "width {width}, scale {scale}, alpha {alpha}: {observed}");
            }
        } }
    }

    #[test]
    fn tab_bar_corners_clip_content_without_painting_an_unowned_backing() {
        let bounds=gtk::graphene::Rect::new(0.,0.,100.,30.);
        let content=gtk::gsk::ColorNode::new(&gtk::gdk::RGBA::WHITE,&bounds).upcast();
        let node=tab_bar_clip(&content,&bounds);
        let clip=node.downcast_ref::<gtk::gsk::RoundedClipNode>().unwrap();
        assert_eq!(clip.child().as_ptr(),content.as_ptr());
        assert_eq!(clip.clip().corner().map(|r|r.width()),[TAB_RADIUS,TAB_RADIUS,0.,0.]);
        let converted=crate::squircle::converted(&node,1.);
        let mut pixels=gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32,100,30).unwrap();
        let cr=gtk::cairo::Context::new(&pixels).unwrap();
        converted.draw(&cr);
        drop(cr);
        let stride=pixels.stride() as usize;
        let bytes=pixels.data().unwrap();
        let alpha=|x:usize,y:usize|bytes[y*stride+x*4+3];
        assert_eq!([alpha(0,0),alpha(99,0)],[0,0]);
        assert_eq!([alpha(50,0),alpha(0,29),alpha(99,29)],[255,255,255]);
    }
}

#[cfg(test)]
#[path = "native_panel_tab_pixel_tests.rs"]
mod native_rgba_pixels;
