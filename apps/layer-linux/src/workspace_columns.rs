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
    tint: gtk::Box,
}
impl Strip {
    fn matches(&self, column: &layer_ui::CollapsedColumnPlacement) -> bool {
        self.id==column.id && self.root.parent().is_some() && self.key.len()==column.groups.len()
            && self.key.iter().zip(&column.groups).all(|((id,panels),group)|
                *id==group.group && panels.iter().copied().eq(group.icons.iter().map(|icon|icon.panel)))
    }
}
#[derive(Default)]
pub(super) struct Columns {
    strips: RefCell<Vec<Strip>>,
    pub(super) connection_geometry: RefCell<Vec<(u32, Panel, layer_ui::PanelBridge)>>,
    pub drawers: RefCell<Vec<Rc<drawers::Drawer>>>,
}
impl Columns {
    pub fn present_connections(&self, w: &Workspace, layout: &ResolvedLayout) {
        let mut layout = layout.clone();
        let groups = w.groups.borrow();
        layout.groups.retain_mut(|group| {
            let Some(bounds) = groups.iter().find(|g| g.id == group.id).and_then(|g| g.root.compute_bounds(&w.surface)) else {return false;};
            if bounds.width()<=0. || bounds.height()<=0. {return false;}
            group.bounds = Bounds { x: bounds.x(), y: bounds.y(), width: bounds.width(), height: bounds.height() };
            true
        });
        let strips = self.strips.borrow();
        layout.collapsed.retain_mut(|column| {
            let Some(strip) = strips.iter().find(|s| s.id == column.id) else { return false; };
            let Some(bounds) = strip.root.compute_bounds(&w.surface) else {return false;};
            column.bounds = Bounds { x: bounds.x(), y: bounds.y(), width: bounds.width(), height: bounds.height() };
            let Some(content) = strip.root.first_child().and_then(|scroll| scroll.compute_bounds(&w.surface)) else {return false;};
            column.content = Bounds {x:content.x(),y:content.y(),width:content.width(),height:content.height()};
            for icon in column.groups.iter_mut().flat_map(|g| &mut g.icons) {
                if let Some(bounds) = strip.buttons.iter().find(|(p, _)| *p == icon.panel).and_then(|(_, b)| b.compute_bounds(&w.surface)) {
                    icon.bounds = Bounds { x: bounds.x(), y: bounds.y(), width: bounds.width(), height: bounds.height() };
                } else {icon.bounds=Bounds::default();}
            }
            if let Some(open) = &mut column.open {
                let bounds = layout.groups.iter().filter(|g| column.groups.iter().any(|c| c.group == g.id)).map(|g| g.bounds);
                if let Some(bounds) = bounds.reduce(|a, b| Bounds {
                    x: a.x.min(b.x), y: a.y.min(b.y),
                    width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
                    height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
                }) { open.bounds = bounds; }
            }
            true
        });
        drop(strips);
        drop(groups);
        let layout = layout.with_panel_bridges();
        w.present_column_corners(&layout);
        let bridges=layout.panel_bridges();
        self.present_surface_ownership(w,&bridges);
        *self.connection_geometry.borrow_mut()=bridges;
    }

    fn present_surface_ownership(&self,w: &Workspace,bridges: &[(u32,Panel,layer_ui::PanelBridge)]) {
        let roots: Vec<_>=w.groups.borrow().iter().map(|g|(g.root.clone().upcast::<gtk::Widget>(),Some(g.tab_strip.clone()),
            bridges.iter().any(|(_,_,b)|b.receivers.iter().any(|r|r.group==g.id)))).chain(self.strips.borrow().iter()
            .map(|s|(s.root.clone().upcast(),None,bridges.iter().any(|(id,..)|*id==s.id)))).collect();
        for (root,tabs,active) in roots {
            if let Some(tabs)=tabs {tabs.set_gpu_background(active);}
            if root.has_css_class("gpu-panel-surface")!=active {
                if active { root.add_css_class("gpu-panel-surface"); } else { root.remove_css_class("gpu-panel-surface"); }
            }
        }
    }

    pub fn snapshot(&self, w: &Workspace, nodes: &[(Slot, Option<gtk::gsk::RenderNode>)]) -> Vec<layer_render_wgpu::PanelSurface> {
        let rect = |b: gtk::graphene::Rect| [b.x(),b.y(),b.width(),b.height()];
        let rgba = |c: gdk::RGBA, opacity: f32| [c.red(),c.green(),c.blue(),c.alpha()*opacity];
        let opacity = |slot| nodes.iter().find(|(candidate,_)| *candidate==slot)
            .and_then(|(_,node)| node.as_ref()).map_or(0., crate::glass::opacity);
        let strips=self.strips.borrow();
        let geometry=self.connection_geometry.borrow();
        let groups=w.groups.borrow();
        let mut surfaces=Vec::new();
        for strip in strips.iter() {
            let Some((_,_,first))=geometry.iter().find(|(id,..)|*id==strip.id) else { continue; };
            let Some(sidebar)=strip.root.compute_bounds(&w.surface) else { continue; };
            let source_opacity=opacity(Slot::Column(strip.id));
            let tint=strip.tint.color();
            let color=rgba(tint,source_opacity);
            let mut panels=Vec::new();
            for receiver in &first.receivers {
                let bounds=receiver.bounds;
                let Some(group)=groups.iter().find(|g|g.id==receiver.group) else { continue; };
                let receiver_opacity=opacity(Slot::Group(group.id));
                if receiver_opacity==0. { continue; }
                let mut bar=group.tab_strip.parent();
                while bar.as_ref().is_some_and(|b|b.widget_name()!="panel-tab-bar") { bar=bar.and_then(|b|b.parent()); }
                let bar=bar.filter(|b|group.tabs_visible && b.is_mapped()).and_then(|b|b.compute_bounds(&w.surface));
                let active=group.tab_strip.active_bounds().and_then(|rect| {
                    let origin=group.tab_strip.compute_point(&w.surface,&gtk::graphene::Point::zero())?;
                    Some(gtk::graphene::Rect::new(rect.x()+origin.x(),rect.y()+origin.y(),rect.width(),rect.height()))
                });
                let mut viewport=group.tab_strip.parent();
                while viewport.as_ref().is_some_and(|b|!b.is::<gtk::ScrolledWindow>()) { viewport=viewport.and_then(|b|b.parent()); }
                let clip=viewport.and_then(|b|b.compute_bounds(&w.surface)).or(bar);
                let colors=group.tab_strip.surface_colors();
                panels.push(layer_render_wgpu::PanelSurfaceGroup {
                    bounds:[bounds.x,bounds.y,bounds.width,bounds.height],radii:receiver.radii,
                    tab_bar:bar.map_or([0.;4],rect),active_tab:active.map_or([0.;4],rect),tab_clip:clip.map_or([0.;4],rect),
                    color:rgba(tint,receiver_opacity),strip_color:rgba(if bar.is_some() {colors[0]} else {tint},receiver_opacity),
                });
            }
            if panels.is_empty() && source_opacity==0. { continue; }
            let anchors=if source_opacity>0. && !panels.is_empty() {
                geometry.iter().filter(|(id,..)|*id==strip.id).map(|(_,_,g)|[g.anchor.x,g.anchor.y,g.anchor.width,g.anchor.height]).collect()
            } else {Vec::new()};
            let radii=w.column_corner_styles.borrow().iter().find(|(name,_)|name==&format!("column-strip-{}",strip.id))
                .map_or([SURFACE_RADIUS;4],|(_,r)|*r);
            surfaces.push(layer_render_wgpu::PanelSurface { sidebar:if source_opacity>0. {rect(sidebar)} else {[0.;4]},sidebar_radii:radii,
                plane:first.plane,bridge_width:first.width(),direction:if first.direction==Edge::Right {1.} else {-1.},scale:1.,
                color,groups:panels,anchors,
            });
        }
        surfaces
    }

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
                    origin.filter(|(p, _)| p == panel).map(|(_, edge)| edge),
                );
            }
        }
    }
    pub fn reconcile(&self, w: &Rc<Workspace>, layout: &DockLayout, resolved: &ResolvedLayout, groups_changed: bool) {
        let projection_changed=groups_changed || {
            let strips=self.strips.borrow();
            strips.len()!=resolved.collapsed.len() || resolved.collapsed.iter().any(|c|!strips.iter().any(|s|s.matches(c)))
        };
        if projection_changed {
            self.connection_geometry.borrow_mut().clear();
            self.present_surface_ownership(w,&[]);
        }
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
            if old.is_none_or(|i| !strips[i].matches(c)) {
                if let Some(i) = old {
                    strips.remove(i);
                }
                w.surface.remove_slots(|slot| slot == Slot::Column(c.id));
                let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
                root.add_css_class("dock-panel");
                root.add_css_class("collapsed-column");
                root.add_css_class(&format!("column-strip-{}", c.id));
                root.set_widget_name(&format!("collapsed-column-{}", c.id));
                root.set_overflow(gtk::Overflow::Hidden);
                w.install_context(&root, ContextTarget::Column { column: c.id });
                let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
                let mut buttons = Vec::new();
                let mut ordinal = 0;
                for (index, group) in c.groups.iter().enumerate() {
                    if index > 0 {
                        content.append(&column_separator());
                    }
                    let mini = gtk::Box::new(gtk::Orientation::Vertical, 2);
                    mini.set_valign(gtk::Align::Start);
                    mini.add_css_class("collapsed-group");
                    for icon in &group.icons {
                        let config = layout.panel(icon.panel).unwrap();
                        let button = w.action_button(
                            &config.title_localized(&w.localization()),
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
                        button.add_css_class(&format!("column-tile-{}-{ordinal}", c.id));
                        ordinal += 1;
                        button.set_size_request(TILE_SIZE as i32, TILE_SIZE as i32);
                        button.set_child(Some(&w.customization.icon(w, config.icon(), 20)));
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
                let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::Never)
                    .vscrollbar_policy(gtk::PolicyType::External)
                    .vexpand(true)
                    .child(&content)
                    .build());
                scroll.set_widget_name(&format!("column-scroll-{}", c.id));
                scroll.set_margin_bottom(WORKSPACE_SPACING as i32);
                // A fresh GtkAdjustment initially has an empty range. Seed it
                // before restoring the offset, otherwise GTK clamps it to zero
                // while this new projection is waiting for its first allocation.
                let offset = c.scroll_offset;
                scroll.vadjustment().configure(
                    f64::from(offset),
                    0.,
                    f64::from(c.content.height + c.scroll_max),
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
                let grip = tiles::grip(&w.localization().text(layer_ui::MessageId::DOCUMENTS_DELIVERY_DRAG_PANEL));
                grip.set_halign(gtk::Align::Center);
                grip.set_valign(gtk::Align::Center);
                grip.set_hexpand(true);
                footer.append(&grip);
                w.install_panel_drag(&footer, DockItem::Column { column: c.id });
                root.append(&footer);
                let tint = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                tint.add_css_class("drawer-connection");
                tint.set_visible(false);
                root.append(&tint);
                w.surface.add(Slot::Column(c.id), &root);
                strips.push(Strip {
                    id: c.id,
                    root,
                    key,
                    buttons,
                    scroll: scroll.vadjustment(),
                    updating_scroll,
                    tint,
                });
            }
            let strip = strips.iter().find(|s| s.id == c.id).unwrap();
            for (panel, button) in &strip.buttons {
                let active = c
                    .open
                    .as_ref()
                    .is_some_and(|_| c.groups.iter().any(|g| g.active == *panel));
                selected(button, active);
                customization::drawer_origin(
                    button,
                    None,
                );
            }
            let offset = c.scroll_offset;
            strip.updating_scroll.set(true);
            strip.scroll.set_value(f64::from(offset));
            strip.updating_scroll.set(false);
            for (panel, button) in &strip.buttons {
                button.set_tooltip_text(Some(&layout.panel(*panel).unwrap().title_localized(&w.localization())));
            }
        }
        drop(strips);
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

fn column_separator() -> gtk::Box {
    // Only tab groups within a member have a separator.
    let slot = crate::tiles::divider_slot(gtk::Orientation::Vertical);
    slot.add_css_class("column-divider");
    slot.set_vexpand(false);
    slot
}
