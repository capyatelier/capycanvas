//! The canvas zoom and rotation readout. It opens the shared zoom menu with a
//! typed zoom field; the camera supplies the value and Rust applies each
//! change. Closing the menu hands keyboard focus back, normally to the canvas.
use crate::{number_control::NumberControl, workspace::Workspace};
use adw::prelude::*;
use gtk::{gio, glib};
use layer_ui::{Camera, NumericControl, UiAction};
use std::{cell::RefCell, rc::Rc};

const FIELD: &str = "zoom-field";

pub struct ZoomReadout {
    pub root: gtk::Button,
    label: gtk::Label,
    pub(crate) menu: gtk::PopoverMenu,
    pub(crate) field: NumberControl,
    previous_focus: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
}

impl ZoomReadout {
    pub fn new(localization: std::sync::Arc<layer_ui::Localizer>) -> Rc<Self> {
        let label = gtk::Label::new(Some("100% · 0°"));
        let root = gtk::Button::builder().child(&label).build();
        root.set_widget_name("canvas-view-info");
        let title = localization.text(layer_ui::MessageId::MENU_ZOOM);
        root.set_tooltip_text(Some(&title));
        root.update_property(&[gtk::accessible::Property::HasPopup(true)]);
        root.add_css_class("flat");
        root.add_css_class("status-bubble");
        root.set_focus_on_click(false);
        root.set_can_focus(false);
        let menu = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
        menu.set_widget_name("zoom-menu");
        menu.set_position(gtk::PositionType::Top);
        let field = NumberControl::inline(NumericControl::zoom(), &title, localization);
        field.set_widget_name(FIELD);
        field.set_size_request(220, -1);
        Rc::new(Self {
            root,
            label,
            menu,
            field,
            previous_focus: RefCell::new(None),
        })
    }

    pub fn set_localization(&self, localization: std::sync::Arc<layer_ui::Localizer>) {
        let title = localization.text(layer_ui::MessageId::MENU_ZOOM);
        self.root.set_tooltip_text(Some(&title));
        self.field.set_caption(&title, "", localization);
    }

    pub fn bind(self: &Rc<Self>, workspace: &Rc<Workspace>) {
        self.root.connect_clicked(glib::clone!(
            #[weak(rename_to = readout)]
            self,
            #[weak]
            workspace,
            move |_| readout.open(&workspace)
        ));
        self.menu.connect_closed(glib::clone!(
            #[weak(rename_to = readout)]
            self,
            #[weak]
            workspace,
            move |_| {
                let previous = readout.previous_focus.take().and_then(|f| f.upgrade()).filter(|f| f.is_mapped());
                if workspace.window.visible_dialog().is_none() {
                    match previous {
                        Some(focus) => focus.grab_focus(),
                        None => workspace.area.grab_focus(),
                    };
                }
            }
        ));
        self.field.connect_value_changed(glib::clone!(
            #[weak]
            workspace,
            move |field| workspace.dispatch(UiAction::SetZoom { zoom: field.value() as f32 })
        ));
    }

    fn open(self: &Rc<Self>, workspace: &Rc<Workspace>) {
        let Some((model, zoom)) = workspace
            .gpu
            .borrow()
            .as_ref()
            .map(|g| (g.session.zoom_menu(), g.session.state().camera.zoom))
        else {
            return;
        };
        *self.previous_focus.borrow_mut() = gtk::prelude::RootExt::focus(&workspace.window).map(|f| f.downgrade());
        if self.field.parent().is_some() {
            self.menu.remove_child(&self.field);
        }
        let root = workspace.workspace_menu_model(&self.menu, model);
        let item = gio::MenuItem::new(None, None);
        item.set_attribute_value("custom", Some(&FIELD.to_variant()));
        let section = gio::Menu::new();
        section.append_item(&item);
        root.prepend_section(None, &section);
        self.menu.set_menu_model(Some(&root));
        self.menu.add_child(&self.field, FIELD);
        self.field.set_value(f64::from(zoom));
        workspace.popup_at(self.menu.upcast_ref(), &self.root, [self.root.width() as f32 / 2., 0.]);
    }

    /// The field is refreshed only while its menu is open, so pan and zoom
    /// motion updates one label.
    pub fn refresh(&self, camera: &Camera) {
        self.label.set_text(&format!(
            "{:.0}% · {:.0}°",
            camera.zoom * 100.0,
            camera.rotation.to_degrees()
        ));
        if self.menu.is_visible() {
            self.field.set_value(f64::from(camera.zoom));
        }
    }
}
