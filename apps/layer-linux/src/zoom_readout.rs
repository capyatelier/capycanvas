//! Canvas view controls.
use crate::{number_control::NumberControl, workspace::Workspace};
use adw::prelude::*;
use gtk::{gio, glib};
use layer_ui::{Camera, NumericControl, UiAction};
use std::{cell::RefCell, rc::Rc};

const FIELD: &str = "zoom-field";
const ROTATION: &str = "rotation-field";
const BUTTONS: &str = "zoom-buttons";

pub struct ZoomReadout {
    pub root: gtk::Button,
    label: gtk::Label,
    pub(crate) menu: gtk::PopoverMenu,
    pub(crate) field: NumberControl,
    pub(crate) rotation: NumberControl,
    controls: crate::navigator::NavigationControls,
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
        let field = NumberControl::inline(NumericControl::zoom(), &title, localization.clone());
        field.set_widget_name(FIELD);
        field.set_size_request(220, -1);
        let rotation = NumberControl::new(NumericControl::rotation(), &localization.text(layer_ui::MessageId::MENU_ROTATION), "", localization);
        rotation.set_widget_name(ROTATION);
        let controls = crate::navigator::NavigationControls::new("zoom");
        Rc::new(Self {
            root,
            label,
            menu,
            field,
            rotation,
            controls,
            previous_focus: RefCell::new(None),
        })
    }

    pub fn set_localization(&self, localization: std::sync::Arc<layer_ui::Localizer>) {
        let title = localization.text(layer_ui::MessageId::MENU_ZOOM);
        self.root.set_tooltip_text(Some(&title));
        self.field.set_caption(&title, "", localization.clone());
        self.rotation.set_caption(&localization.text(layer_ui::MessageId::MENU_ROTATION), "", localization);
    }

    pub fn bind(self: &Rc<Self>, workspace: &Rc<Workspace>) {
        self.controls.bind(workspace);
        let context = gtk::GestureClick::new();
        context.set_button(3);
        context.connect_pressed(glib::clone!(#[weak(rename_to = readout)] self, #[weak] workspace,
            move |gesture, _, _, _| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                readout.open(&workspace);
            }));
        self.root.add_controller(context);
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
        self.rotation.connect_value_changed(glib::clone!(#[weak] workspace,
            move |field| workspace.dispatch(UiAction::SetRotation { rotation: field.value() as f32 })));
    }

    fn open(self: &Rc<Self>, workspace: &Rc<Workspace>) {
        let Some((model, camera)) = workspace
            .gpu
            .borrow()
            .as_ref()
            .map(|g| (g.session.zoom_menu(), g.session.state().camera.clone()))
        else {
            return;
        };
        *self.previous_focus.borrow_mut() = gtk::prelude::RootExt::focus(&workspace.window).map(|f| f.downgrade());
        if self.field.parent().is_some() {
            self.menu.remove_child(&self.field);
            self.menu.remove_child(&self.rotation);
            self.menu.remove_child(&self.controls.root);
        }
        self.controls.refresh(&model.buttons);
        let rotation_section = model.rotation_section as i32;
        let root = workspace.workspace_menu_model(&self.menu, model.menu);
        for (position, name) in [(0, FIELD), (rotation_section + 1, ROTATION), (-1, BUTTONS)] {
            let item = gio::MenuItem::new(None, None);
            item.set_attribute_value("custom", Some(&name.to_variant()));
            let section = gio::Menu::new();
            section.append_item(&item);
            root.insert_section(position, None, &section);
        }
        self.menu.set_menu_model(Some(&root));
        self.menu.add_child(&self.field, FIELD);
        self.menu.add_child(&self.rotation, ROTATION);
        self.menu.add_child(&self.controls.root, BUTTONS);
        self.field.set_value(f64::from(camera.zoom));
        self.rotation.set_value(f64::from(camera.rotation));
        workspace.popup_at(self.menu.upcast_ref(), &self.root, [self.root.width() as f32 / 2., 0.]);
    }

    pub fn refresh(&self, camera: &Camera, commands: &[layer_ui::CommandState]) {
        self.label.set_text(&format!(
            "{:.0}% · {:.0}°",
            camera.zoom * 100.0,
            camera.rotation.to_degrees()
        ));
        if self.menu.is_visible() {
            self.field.set_value(f64::from(camera.zoom));
            self.rotation.set_value(f64::from(camera.rotation));
            self.controls.refresh(commands);
        }
    }
}
