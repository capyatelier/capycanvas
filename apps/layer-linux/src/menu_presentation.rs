use gtk::prelude::*;
use layer_ui::{MenuAnchor, MenuTarget};
#[cfg(test)]
use layer_ui::MenuPresentation;

const PRESENTATION: &str = "capy-menu-presentation";

pub(crate) fn configure(popover: &impl IsA<gtk::Popover>, target: MenuTarget, anchor: Option<gtk::gdk::Rectangle>) {
    let presentation=target.presentation();
    let popover=popover.as_ref();
    popover.set_has_arrow(presentation.has_arrow);
    let anchor=anchor.map(|rect|match presentation.anchor {
        MenuAnchor::Contact=>gtk::gdk::Rectangle::new(rect.x(),rect.y(),1,1),
        MenuAnchor::Widget=>rect,
    });
    popover.set_pointing_to(anchor.as_ref());
    unsafe {popover.set_data(PRESENTATION,presentation);}
}

#[cfg(test)]
pub(crate) fn declared(popover: &gtk::Popover) -> Option<MenuPresentation> {
    unsafe {popover.data::<MenuPresentation>(PRESENTATION).map(|value|*value.as_ref())}
}

#[cfg(test)]
pub(crate) fn assert_declared(popover: &gtk::Popover) {
    let presentation=declared(popover)
        .expect("the native menu declares its shared presentation");
    assert_eq!(popover.has_arrow(),presentation.has_arrow,"the native menu keeps its declared arrow");
    let (pointing,rect)=popover.pointing_to();
    if presentation.anchor==MenuAnchor::Contact {
        assert!(pointing,"the native context menu retains its contact anchor");
        assert_eq!([rect.width(),rect.height()],[1,1]);
    } else if pointing {
        assert!(rect.width()>1 || rect.height()>1,"the native menu retains its widget anchor");
    }
}
