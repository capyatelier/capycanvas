use super::*;
use gtk::subclass::prelude::*;
use layer_ui::{LayerConnection, LayerRelationKind};

#[derive(Default)]
struct State {
    rows: Option<Rc<RefCell<HashMap<usize, Row>>>>,
    order: HashMap<u64, (usize, u32)>,
    connections: Vec<LayerConnection>,
    glyph: Option<gtk::Svg>,
}

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Connections { pub(super) state: RefCell<State> }
    #[glib::object_subclass]
    impl ObjectSubclass for Connections {
        const NAME: &'static str = "CapyLayerConnections";
        type Type = super::Connections;
        type ParentType = gtk::Widget;
        fn class_init(klass: &mut Self::Class) { klass.set_accessible_role(gtk::AccessibleRole::Presentation); }
    }
    impl ObjectImpl for Connections {}
    impl WidgetImpl for Connections {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) { (0, 0, -1, -1) }
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.obj().queue_draw();
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let state = self.state.borrow();
            let Some(rows) = &state.rows else { return; };
            let bounds = gtk::graphene::Rect::new(0., 0., widget.width() as f32, widget.height() as f32);
            let geometry: HashMap<_, _> = rows.borrow().values()
                .filter(|row| row.bound.get() && state.order.contains_key(&row.id.get()) && row.root.is_mapped() && row.root.width() > 0)
                .filter_map(|row| Some((row.id.get(), (row.root.compute_bounds(widget.as_ref())?, row.content.compute_bounds(widget.as_ref())?))))
                .collect();
            let Some((&anchor, (anchor_row, anchor_thumb))) = geometry.iter().min_by_key(|(id, _)| state.order[id].0) else { return; };
            let first = geometry.keys().map(|id| state.order[id].0).min().unwrap();
            let last = geometry.keys().map(|id| state.order[id].0).max().unwrap();
            let swipe_x = rows.borrow().values().find(|row| row.id.get() == anchor)
                .and_then(|row| row.swipe.compute_bounds(widget.as_ref())).map_or(anchor_row.x(), |rect| rect.x());
            let column = anchor_thumb.x() - anchor_row.x() + swipe_x - (state.order[&anchor].1 * 8).min(24) as f32;
            let endpoint = |id: u64, bottom: bool| -> Option<f32> {
                if let Some((_, thumb)) = geometry.get(&id) {
                    Some(thumb.y() + if bottom { thumb.height() } else { 0. })
                } else {
                    let position = state.order.get(&id)?.0;
                    if position < first { Some(0.) } else if position > last { Some(bounds.height()) } else { None }
                }
            };
            let color = widget.color();
            let link_color = widget.parent().map_or(color, |parent| parent.color());
            snapshot.push_clip(&bounds);
            let cr = snapshot.append_cairo(&bounds);
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            let mut glyphs = Vec::new();
            for connection in &state.connections {
                let effect = connection.kind == LayerRelationKind::Effect;
                let (Some(top), Some(bottom)) = (endpoint(connection.from, effect), endpoint(connection.to, !effect)) else { continue; };
                if bottom < 0. || top > bounds.height() || bottom <= top { continue; }
                let x = column + (connection.depth * 8).min(24) as f32;
                let ink = if effect { link_color } else { color };
                cr.set_source_rgba(f64::from(ink.red()), f64::from(ink.green()), f64::from(ink.blue()), f64::from(ink.alpha()));
                if !effect {
                    cr.set_line_width(2.);
                    cr.move_to(f64::from(x - 3.5), f64::from(top));
                    cr.line_to(f64::from(x - 3.5), f64::from(bottom));
                    cr.stroke().unwrap();
                    continue;
                }
                if state.order[&connection.to].0 != state.order[&connection.from].0 + 1 { continue; }
                let center = x + 15.;
                let glyph_y = (top + bottom) * 0.5;
                cr.set_line_width(1.);
                if glyph_y - top > 6. { cr.move_to(f64::from(center), f64::from(top)); cr.line_to(f64::from(center), f64::from(glyph_y - 6.)); }
                if bottom - glyph_y > 6. { cr.move_to(f64::from(center), f64::from(glyph_y + 6.)); cr.line_to(f64::from(center), f64::from(bottom)); }
                cr.stroke().unwrap();
                glyphs.push((center - 6., glyph_y - 6.));
            }
            drop(cr);
            if let Some(glyph) = &state.glyph {
                for (x, y) in glyphs {
                    snapshot.save();
                    snapshot.translate(&gtk::graphene::Point::new(x, y));
                    glyph.snapshot_symbolic(snapshot, 12., 12., &[link_color]);
                    snapshot.restore();
                }
            }
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    pub struct Connections(ObjectSubclass<imp::Connections>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Connections {
    pub(super) fn new(rows: Rc<RefCell<HashMap<usize, Row>>>) -> Self {
        let widget: Self = glib::Object::new();
        widget.set_can_target(false);
        widget.add_css_class("layer-connections");
        *widget.imp().state.borrow_mut() = State { rows: Some(rows), glyph: crate::icons::paintable("layer-effect-link-symbolic", None), ..Default::default() };
        widget
    }
    pub(super) fn refresh(&self, rows: &[LayerState], connections: &[LayerConnection]) {
        let mut state = self.imp().state.borrow_mut();
        if state.connections == connections && state.order.len() == rows.len()
            && rows.iter().enumerate().all(|(index, row)| state.order.get(&row.id) == Some(&(index, row.depth))) { return; }
        state.order = rows.iter().enumerate().map(|(index, row)| (row.id, (index, row.depth))).collect();
        state.connections.clear();
        state.connections.extend_from_slice(connections);
        self.queue_draw();
    }
}
