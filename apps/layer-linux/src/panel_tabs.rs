//! Retained tab buttons adapt to their actual allocation and native font metrics.
//! Shared Rust chooses which complete labels fit; native input targets survive.
use gtk::{glib, prelude::*, subclass::prelude::*};
use layer_ui::TabStyle;
use std::cell::{Cell, RefCell};
pub(crate) const TAB_RADIUS: f32 = layer_ui::SURFACE_RADIUS * crate::squircle::CORNER_FIT;
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Strip {
        pub buttons: RefCell<Vec<gtk::Button>>,
        pub style: Cell<TabStyle>,
        pub hover_bottom: RefCell<Vec<[i8; 2]>>,
        pub tints: RefCell<Option<[gtk::Box; 2]>>,
        pub gpu_background: Cell<bool>,
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
            let background = gtk::Snapshot::new();
            let gpu=self.gpu_background.get();
            if !gpu { background.append_color(&strip.color(), &bounds); }
            let corners = self.hover_bottom.borrow();
            for (i, button) in buttons.iter().enumerate().filter(|(_, button)| Some(*button) != selected) {
                if button.is_sensitive() && button.opacity() > 0. && !button.has_css_class("selected-tool")
                    && button.state_flags().contains(gtk::StateFlags::PRELIGHT)
                    && let Some(tab) = button.compute_bounds(&*obj) {
                        let cr = background.append_cairo(&bounds);
                        let color = button.color();
                        cr.set_source_rgba(color.red().into(), color.green().into(), color.blue().into(), f64::from(color.alpha()) * 0.08 * button.opacity());
                        super::hover(&cr, &tab, corners.get(i).copied().unwrap_or_default());
                        let _ = cr.fill();
                    }
            }
            let mut node = background.to_node().unwrap_or_else(||gtk::gsk::ColorNode::new(&gtk::gdk::RGBA::TRANSPARENT,&bounds).upcast());
            if let Some(tab) = active.filter(|_|!gpu) {
                let mask = gtk::Snapshot::new();
                let cr = mask.append_cairo(&bounds);
                let radius = gtk::graphene::Size::new(layer_ui::SURFACE_RADIUS, layer_ui::SURFACE_RADIUS);
                let square = gtk::graphene::Size::new(0., 0.);
                let mut shape = gtk::gsk::RoundedRect::new(tab, radius, radius, square, square);
                shape.normalize();
                crate::squircle::rounded_rect(&cr, &shape);
                super::feet(&cr, &tab, bounds.height());
                cr.set_source_rgba(1., 1., 1., 1.);
                let _ = cr.fill();
                drop(cr);
                let mask = mask.to_node().unwrap();
                node = super::masked_fill(node, gtk::gsk::ColorNode::new(&foot.color(), &bounds).upcast(), mask);
            }
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

fn masked_fill(background: gtk::gsk::RenderNode, foreground: gtk::gsk::RenderNode, mask: gtk::gsk::RenderNode) -> gtk::gsk::RenderNode {
    let outside = gtk::gsk::MaskNode::new(background, &mask, gtk::gsk::MaskMode::InvertedAlpha);
    let inside = gtk::gsk::MaskNode::new(foreground, mask, gtk::gsk::MaskMode::Alpha);
    let average = gtk::gsk::CrossFadeNode::new(outside, inside, 0.5);
    let restore_alpha = gtk::graphene::Matrix::from_float([
        1., 0., 0., 0.,
        0., 1., 0., 0.,
        0., 0., 1., 0.,
        0., 0., 0., 2.,
    ]);
    gtk::gsk::ColorMatrixNode::new(average, &restore_alpha, &gtk::graphene::Vec4::zero()).upcast()
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
