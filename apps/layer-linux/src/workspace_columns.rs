//! Native collapsed strips. The shared layout supplies membership, actions,
//! sizing and drawer lifecycle; GTK owns only widgets and scrolling.
use super::*;

struct Strip {
    id: u32,
    root: gtk::Box,
    key: Vec<(u32, Vec<Panel>)>,
    buttons: Vec<(Panel, gtk::Button)>,
    scroll: gtk::Adjustment,
    updating_scroll: Rc<Cell<bool>>,
}
#[derive(Default)]
pub(super) struct Columns {
    strips: RefCell<Vec<Strip>>,
    pub drawers: RefCell<Vec<Rc<drawers::Drawer>>>,
}
impl Columns {
    pub fn button(&self, column: u32, panel: Panel) -> Option<gtk::Button> {
        self.strips
            .borrow()
            .iter()
            .find(|s| s.id == column)?
            .buttons
            .iter()
            .find(|(p, _)| *p == panel)
            .map(|(_, b)| b.clone())
    }
    pub fn reconcile(&self, w: &Rc<Workspace>, layout: &DockLayout, resolved: &ResolvedLayout) {
        let mut strips = self.strips.borrow_mut();
        strips.retain(|s| resolved.collapsed.iter().any(|c| c.id == s.id));
        w.surface.remove_slots(|slot| matches!(slot, Slot::Column(id) if !resolved.collapsed.iter().any(|c| c.id == id)));
        for c in &resolved.collapsed {
            let key: Vec<_> = c
                .groups
                .iter()
                .map(|g| (g.group, g.icons.iter().map(|i| i.panel).collect()))
                .collect();
            let old = strips.iter().position(|s| s.id == c.id);
            if old.is_none_or(|i| strips[i].key != key || strips[i].root.parent().is_none()) {
                if let Some(i) = old {
                    strips.remove(i);
                }
                w.surface.remove_slots(|slot| slot == Slot::Column(c.id));
                let root = gtk::Box::new(gtk::Orientation::Vertical, WORKSPACE_SPACING as i32);
                root.add_css_class("dock-panel");
                root.add_css_class("collapsed-column");
                root.set_widget_name(&format!("collapsed-column-{}", c.id));
                root.set_overflow(gtk::Overflow::Hidden);
                let expand = w.action_button(c.expand_label(), c.expand_action());
                expand.set_widget_name(&format!("expand-column-{}", c.id));
                expand.add_css_class("flat");
                expand.set_child(Some(&gtk::Image::from_icon_name(
                    "layer-column-expand-symbolic",
                )));
                expand.set_height_request(c.expand.height as i32);
                root.append(&expand);
                let content = gtk::Box::new(gtk::Orientation::Vertical, WORKSPACE_SPACING as i32);
                let mut buttons = Vec::new();
                for group in &c.groups {
                    let mini = gtk::Box::new(gtk::Orientation::Vertical, 2);
                    mini.set_valign(gtk::Align::Start);
                    mini.add_css_class("collapsed-group");
                    for icon in &group.icons {
                        let config = layout.panel(icon.panel).unwrap();
                        let button = w.action_button(
                            config.title(),
                            UiAction::Customize {
                                action: CustomizationAction::ToggleColumnDrawer {
                                    group: group.group,
                                    panel: icon.panel,
                                },
                            },
                        );
                        button.set_widget_name(&format!("column-icon-{:?}", icon.panel));
                        button.add_css_class("flat");
                        button.add_css_class("tile-button");
                        button.set_size_request(TILE_SIZE as i32, TILE_SIZE as i32);
                        let image = gtk::Image::from_icon_name(&format!(
                            "layer-{}-symbolic",
                            config.icon()
                        ));
                        image.set_pixel_size(20);
                        button.set_child(Some(&image));
                        w.install_context(&button, ContextTarget::Panel { panel: icon.panel });
                        mini.append(&button);
                        buttons.push((icon.panel, button));
                    }
                    w.install_context(&mini, ContextTarget::Group { group: group.group });
                    content.append(&mini);
                }
                let scroll = gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::Never)
                    .vscrollbar_policy(gtk::PolicyType::External)
                    .vexpand(true)
                    .child(&content)
                    .build();
                scroll.set_widget_name(&format!("column-scroll-{}", c.id));
                // A fresh GtkAdjustment initially has an empty range. Seed it
                // before restoring the offset, otherwise GTK clamps it to zero
                // while this new projection is waiting for its first allocation.
                let offset = layout
                    .column_scroll
                    .iter()
                    .find(|(id, _)| *id == c.id)
                    .map_or(0., |(_, v)| *v);
                scroll.vadjustment().configure(
                    f64::from(offset),
                    0.,
                    f64::from(
                        (content
                            .measure(gtk::Orientation::Vertical, TILE_SIZE as i32)
                            .1 as f32)
                            .max(c.content.height),
                    ),
                    16.,
                    f64::from(TILE_SIZE),
                    f64::from(c.content.height),
                );
                let column = c.id;
                let updating_scroll = Rc::new(Cell::new(false));
                let updating = updating_scroll.clone();
                scroll.vadjustment().connect_value_changed(glib::clone!(
                    #[weak]
                    w,
                    move |a| {
                        if updating.get() {
                            return;
                        }
                        w.dispatch(UiAction::MeasureColumnScroll {
                            column,
                            offset: a.value() as f32,
                        });
                    }
                ));
                root.append(&scroll);
                let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                footer.add_css_class("panel-footer");
                footer.set_height_request(c.grip.height as i32);
                footer.set_widget_name(&format!("column-grip-{}", c.id));
                let grip = tiles::grip();
                grip.set_halign(gtk::Align::Center);
                grip.set_valign(gtk::Align::Center);
                grip.set_hexpand(true);
                footer.append(&grip);
                w.install_panel_drag(&footer, DockItem::Column { column: c.id });
                root.append(&footer);
                w.surface.add(Slot::Column(c.id), &root);
                strips.push(Strip {
                    id: c.id,
                    root,
                    key,
                    buttons,
                    scroll: scroll.vadjustment(),
                    updating_scroll,
                });
            }
            let strip = strips.iter().find(|s| s.id == c.id).unwrap();
            let offset = layout
                .column_scroll
                .iter()
                .find(|(id, _)| *id == c.id)
                .map_or(0., |(_, offset)| *offset);
            strip.updating_scroll.set(true);
            strip.scroll.set_value(f64::from(offset));
            strip.updating_scroll.set(false);
            for (panel, button) in &strip.buttons {
                selected(button, c.groups.iter().any(|g| g.active == *panel));
                button.set_tooltip_text(Some(layout.panel(*panel).unwrap().title()));
            }
        }
    }
    pub fn refresh_drawers(&self, w: &Rc<Workspace>, state: &UiState, regions: u32) {
        {
            let mut views = self.drawers.borrow_mut();
            views.retain(|v| !v.is_closed() || state.customization.column_drawers.iter().any(|d| matches!(d.anchor, DrawerAnchor::Column { column, .. } if column == v.id)));
            for drawer in &state.customization.column_drawers {
                let DrawerAnchor::Column { column, .. } = drawer.anchor else {
                    continue;
                };
                if !views.iter().any(|v| v.id == column) {
                    views.push(drawers::Drawer::new(column));
                }
            }
        }
        // Do not hold this list borrowed while native measurements/animation
        // callbacks ask the workspace for its drawer projections.
        for view in self.drawers.borrow().clone() {
            let next = state.customization.column_drawers.iter().find(
                |d| matches!(d.anchor, DrawerAnchor::Column { column, .. } if column == view.id),
            );
            view.refresh(w, state, regions, next);
        }
    }
}
