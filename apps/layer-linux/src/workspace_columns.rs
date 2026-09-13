//! Native collapsed strips. The shared layout supplies membership, actions,
//! sizing and drawer lifecycle; GTK owns only widgets and scrolling.
use super::*;

struct Strip {
    id: u32,
    root: gtk::Box,
    key: Vec<(u32, Vec<Panel>)>,
    buttons: Vec<(Panel, gtk::Button)>,
    open_tiles: RefCell<Vec<(Panel, Edge)>>,
    scroll: gtk::Adjustment,
    updating_scroll: Rc<Cell<bool>>,
}
#[derive(Default)]
pub(super) struct Columns {
    strips: RefCell<Vec<Strip>>,
    pub drawers: RefCell<Vec<Rc<drawers::Drawer>>>,
}
impl Columns {
    pub fn background_at(&self, w: &Workspace, point: [f32; 2]) -> Option<u32> {
        let mut picked = w
            .surface
            .pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT);
        let strips = self.strips.borrow();
        while let Some(widget) = picked {
            // Buttons (including their image/label children) keep their own
            // click actions. All other descendants belong to the strip.
            if widget.is::<gtk::Button>() {
                return None;
            }
            if let Some(strip) = strips
                .iter()
                .find(|s| s.root.upcast_ref::<gtk::Widget>() == &widget)
            {
                return Some(strip.id);
            }
            picked = widget.parent();
        }
        None
    }
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
    pub fn mark_drawer_origin(&self, column: u32, origin: Option<(Panel, Edge)>) {
        let strips = self.strips.borrow();
        if let Some(strip) = strips.iter().find(|s| s.id == column) {
            for (panel, button) in &strip.buttons {
                customization::drawer_origin(
                    button,
                    strip
                        .open_tiles
                        .borrow()
                        .iter()
                        .find(|(p, _)| p == panel)
                        .map(|(_, edge)| *edge)
                        .or_else(|| origin.filter(|(p, _)| p == panel).map(|(_, edge)| edge)),
                );
            }
        }
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
                let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
                root.add_css_class("dock-panel");
                root.add_css_class("collapsed-column");
                root.set_widget_name(&format!("collapsed-column-{}", c.id));
                root.set_overflow(gtk::Overflow::Hidden);
                w.install_context(&root, ContextTarget::Column { column: c.id });
                let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
                let mut buttons = Vec::new();
                for (index, group) in c.groups.iter().enumerate() {
                    content.append(&column_separator(index == 0));
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
                        let image =
                            crate::icons::image(&format!("layer-{}-symbolic", config.icon()));
                        image.set_pixel_size(20);
                        button.set_child(Some(&image));
                        // An icon tile remains a held source even though it moves a panel.
                        button.add_css_class("drag-hold");
                        w.install_panel_drag(&button, DockItem::Panel { panel: icon.panel });
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
                scroll.set_margin_bottom(WORKSPACE_SPACING as i32);
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
                    open_tiles: RefCell::default(),
                    scroll: scroll.vadjustment(),
                    updating_scroll,
                });
            }
            let strip = strips.iter().find(|s| s.id == c.id).unwrap();
            *strip.open_tiles.borrow_mut() = c
                .open
                .as_ref()
                .map(|o| c.groups.iter().map(|g| (g.active, o.direction)).collect())
                .unwrap_or_default();
            for (panel, button) in &strip.buttons {
                let active = c
                    .open
                    .as_ref()
                    .is_some_and(|_| c.groups.iter().any(|g| g.active == *panel));
                selected(button, active);
                customization::drawer_origin(
                    button,
                    active.then(|| c.open.as_ref().unwrap().direction),
                );
            }
            let offset = layout
                .column_scroll
                .iter()
                .find(|(id, _)| *id == c.id)
                .map_or(0., |(_, offset)| *offset);
            strip.updating_scroll.set(true);
            strip.scroll.set_value(f64::from(offset));
            strip.updating_scroll.set(false);
            for (panel, button) in &strip.buttons {
                button.set_tooltip_text(Some(layout.panel(*panel).unwrap().title()));
            }
        }
        drop(strips);
        w.surface.remove_slots(|slot| {
            matches!(slot, Slot::ColumnConnection(id, panel)
            if !resolved.collapsed.iter().any(|c| c.id == id && c.open.as_ref()
                .is_some_and(|o| o.connections.iter().any(|(p, _)| *p == panel))))
        });
        for c in &resolved.collapsed {
            let Some(open) = &c.open else {
                continue;
            };
            for (panel, _) in &open.connections {
                let slot = Slot::ColumnConnection(c.id, *panel);
                if w.surface
                    .imp()
                    .children
                    .borrow()
                    .iter()
                    .any(|(s, _)| *s == slot)
                {
                    continue;
                }
                let area = gtk::DrawingArea::new();
                area.add_css_class("drawer-connection");
                area.set_widget_name(&format!("column-connection-{}-{panel:?}", c.id));
                area.set_can_target(false);
                let (column, panel) = (c.id, *panel);
                area.set_draw_func(glib::clone!(
                    #[weak]
                    w,
                    move |area, cr, _, _| {
                        let resolved = w.resolved();
                        let Some(connection) = resolved
                            .collapsed
                            .iter()
                            .find(|c| c.id == column)
                            .and_then(|c| c.open.as_ref())
                            .and_then(|o| o.connections.iter().find(|(p, _)| *p == panel))
                            .map(|(_, c)| c)
                        else {
                            return;
                        };
                        let color = area.color();
                        cr.set_source_rgba(
                            color.red().into(),
                            color.green().into(),
                            color.blue().into(),
                            color.alpha().into(),
                        );
                        let [xx, yx, xy, yy, x, y] = connection.transform.map(f64::from);
                        cr.transform(gtk::cairo::Matrix::new(xx, yx, xy, yy, x, y));
                        cr.rectangle(0., 0., connection.length.into(), connection.depth.into());
                        concave_foot(
                            cr,
                            0.,
                            connection.depth.into(),
                            connection.radii[0].into(),
                            -1.,
                        );
                        concave_foot(
                            cr,
                            connection.length.into(),
                            connection.depth.into(),
                            connection.radii[1].into(),
                            1.,
                        );
                        let _ = cr.fill();
                    }
                ));
                w.surface.add(slot, &area);
            }
        }
    }
    pub fn refresh_drawers(&self, w: &Rc<Workspace>, state: &UiState, regions: u32) {
        // The remembered active tab is not an open drawer. Only its current
        // opener is selected; idle collapsed strips keep the neutral theme.
        let resolved = w.resolved();
        for strip in self.strips.borrow().iter() {
            for (panel, button) in &strip.buttons {
                selected(
                    button,
                    state.workspace.layout.column_stack(strip.id).open_column == Some(strip.id)
                        && resolved
                            .collapsed
                            .iter()
                            .find(|c| c.id == strip.id)
                            .is_some_and(|c| c.groups.iter().any(|g| g.active == *panel))
                        || state.customization.column_drawers.iter().any(|d| {
                            matches!(d.anchor, DrawerAnchor::Column { column, origin, .. }
                        if column == strip.id && origin == *panel)
                        }),
                );
            }
        }
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

fn column_separator(leading: bool) -> gtk::Box {
    // The leading line sits at the top; later dividers keep toolbar spacing.
    let slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    slot.add_css_class("toolbar-divider");
    slot.add_css_class("column-divider");
    slot.set_height_request(if leading { 4 } else { 8 });
    slot.set_vexpand(false);
    let line = gtk::Separator::new(gtk::Orientation::Horizontal);
    line.set_halign(gtk::Align::Center);
    line.set_valign(if leading {
        gtk::Align::Start
    } else {
        gtk::Align::Center
    });
    line.set_vexpand(true);
    slot.append(&line);
    slot
}
