use super::*;

#[test]
#[ignore = "private Wayland display and CAPY_COMPAT_OUT"]
fn native_write_compat_fixtures() {
    let (app,windows)=crate::application("art.capycanvas.CompatWriter");
    let app=NativeTestApp(app);app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let english=layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    crate::open_workspace(&app,&windows,Some((new_drawing(768,512,&english).unwrap(),None)));
    until(||windows.borrow().first().is_some_and(|w|w.window.is_mapped()),"fixture window mapped");
    let w=windows.borrow()[0].clone();new_photo::ready(&w);
    new_photo::invoke(&w,CommandId::AddLayer);
    let directory=std::path::PathBuf::from(std::env::var_os("CAPY_COMPAT_OUT").expect("CAPY_COMPAT_OUT"));
    let path=directory.join("app-preview.capy");
    crate::files::choose_next_save(path);new_photo::invoke(&w,CommandId::SaveDocument);
    until(||state(&w).document_file.location.is_some()&&!state(&w).document_file.busy&&!w.servicing.get(),"fixture saved");
    assert!(!state(&w).document_file.modified,"{:?}",state(&w).host_error);
    w.window.close();until(||!w.window.is_visible(),"fixture window closed");
}
