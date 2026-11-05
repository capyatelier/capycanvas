//! Native transport for portable delivery preferences. Disk/CMM work is off-thread.
use super::*;
use std::{
    io::{Read, Write},
    path::PathBuf,
};
static LOCK: Mutex<()> = Mutex::new(());

fn path() -> Option<PathBuf> {
    crate::storage::roots().map(layer_host::StorageRoots::export_presets)
}
fn read(path: &std::path::Path) -> Result<ExportPresets, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ExportPresets::default()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(ExportPresets::MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(ExportPresets::restore(&bytes))
}
pub(super) async fn load(
    document: layer_core::color::DocumentColor,
) -> Result<ExportPresets, layer_ui::ColorFeatureError> {
    gio::spawn_blocking(move || {
        let library = path().map_or(Ok(ExportPresets::default()), |path| read(&path))?;
        library.validate_profiles(document)?;
        Ok::<_,layer_ui::ColorFeatureError>(library)
    })
    .await
    .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Export preset reader failed".into()))?
}
fn write(
    path: &std::path::Path,
    expected: &ExportPresets,
    next: &ExportPresets,
) -> Result<(), layer_ui::ColorFeatureError> {
    let _lock = LOCK.lock().map_err(|e| e.to_string())?;
    if read(path)? != *expected {
        return Err(layer_ui::ColorFeatureError::PresetChanged);
    }
    let bytes = next.encode()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    layer_core::atomic_write(path, |file| {
        file.write_all(&bytes).map_err(|e| e.to_string())
    }).map_err(layer_ui::ColorFeatureError::from)
}
pub(super) async fn save(expected: ExportPresets, next: ExportPresets) -> Result<(), layer_ui::ColorFeatureError> {
    gio::spawn_blocking(move || path().map_or(Ok(()), |path| write(&path, &expected, &next)))
        .await
        .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Export preset writer failed".into()))?
}

pub(super) struct Presentation {
    group: adw::PreferencesGroup,
    status: adw::ActionRow,
    buttons: Vec<(ExportDialogPresetOperation,adw::ButtonRow)>,
}
impl Presentation {
    pub(super) fn new(body:&gtk::Box,view:&ExportDialogView)->Self {
        let group=adw::PreferencesGroup::new();body.append(&group);
        let status=adw::ActionRow::builder().use_markup(false).visible(false).build();status.set_widget_name("export-presets-status");group.add(&status);
        let buttons=[ExportDialogPresetOperation::Save,ExportDialogPresetOperation::Update,ExportDialogPresetOperation::Remove,ExportDialogPresetOperation::Reset].into_iter().enumerate().map(|(i,operation)|{
            let button=adw::ButtonRow::builder().title(&view.preset_labels[i]).use_markup(false).build();
            button.set_widget_name(["export-preset-save","export-preset-update","export-preset-remove","export-preset-reset"][i]);
            if operation==ExportDialogPresetOperation::Remove {button.add_css_class("destructive-action");}
            group.add(&button);(operation,button)
        }).collect();
        Self {group,status,buttons}
    }
    pub(super) fn render(&self,view:&ExportDialogView) {
        self.group.set_sensitive(!view.preset_busy);
        self.status.set_title(view.preset_status.as_deref().unwrap_or_default());self.status.set_visible(view.preset_status.is_some());
        if view.preset_error {self.status.add_css_class("error");}else{self.status.remove_css_class("error");}
        let copy=&view.copy;
        for (i,(_,button)) in self.buttons.iter().enumerate(){
            button.set_title(&view.preset_labels[i]);button.set_visible([view.preset_save,view.preset_update,view.preset_remove,view.preset_reset][i]);
            button.set_tooltip_text(Some([copy.save_preset_help.as_ref(),copy.update_preset_help.as_ref(),copy.remove_preset_help.as_ref(),copy.reset_preset_help.as_ref()][i]));
        }
    }
    pub(super) fn bind(&self,p:&Rc<super::dialog::Projection>) {
        for (operation,button) in &self.buttons {
            let weak=Rc::downgrade(p);let operation=*operation;
            button.connect_activated(move|_|if let Some(p)=weak.upgrade(){p.preset_operation(operation);});
        }
    }
}

impl super::dialog::Projection {
    fn preset_operation(self:&Rc<Self>,operation:ExportDialogPresetOperation) {
        if self.view().is_none_or(|view|view.preset_busy){return;}
        let Some(w)=self.w.upgrade()else{return;};
        self.send(ExportDialogAction::BeginPreset {operation});
        if self.view().is_none_or(|view|!view.preset_busy){return;}
        let p=self.clone();
        glib::MainContext::default().spawn_local(async move {
            if p.view().is_some_and(|view|view.page==ExportDialogPage::SavePreset) {
                let Some(view)=p.view()else{return;};let copy=&view.copy;
                let entry=adw::EntryRow::builder().title(copy.preset_name.as_ref()).text(&view.name).build();entry.set_widget_name("export-preset-name");
                let group=adw::PreferencesGroup::new();group.add(&entry);
                let dialog=adw::AlertDialog::builder().heading(copy.save_preset_title.as_ref()).extra_child(&group).build();dialog.set_widget_name("export-preset-name-dialog");
                dialog.add_responses(&[("cancel",copy.common.cancel.as_ref()),("save",copy.common.save.as_ref())]);dialog.set_close_response("cancel");dialog.set_default_response(Some("save"));dialog.set_response_appearance("save",adw::ResponseAppearance::Suggested);
                crate::page_dialog::set_response_enabled(&dialog,"save",view.name_valid);
                entry.connect_changed({let weak=Rc::downgrade(&p);let dialog=dialog.downgrade();move|entry|if let Some(p)=weak.upgrade(){
                    p.send(ExportDialogAction::Name {value:entry.text().to_string()});
                    if let Some(dialog)=dialog.upgrade(){crate::page_dialog::set_response_enabled(&dialog,"save",p.view().is_some_and(|view|view.name_valid));}
                }});
                w.on_localization(glib::clone!(#[weak] entry,#[weak] dialog,#[upgrade_or] false,move|localization|{
                    let copy=layer_ui::color_feature_copy::ExportCopy::new(localization);entry.set_title(&copy.preset_name);dialog.set_heading(Some(&copy.save_preset_title));
                    crate::page_dialog::set_response_label(&dialog,"cancel",&copy.common.cancel);crate::page_dialog::set_response_label(&dialog,"save",&copy.common.save);true
                }));
                if crate::alert::choose_intent(dialog,&w.window,copy.preset_dialog,Some("save"),&[]).await!="save" {
                    p.send(ExportDialogAction::PresetCancel);return;
                }
                p.send(ExportDialogAction::Back);
            }
            let write={let gpu=w.gpu.borrow();gpu.as_ref().ok_or(layer_ui::ColorFeatureError::ExportCombination).and_then(|gpu|gpu.session.export_dialog_preset_write(p.id,operation))};
            let result=match write {Ok(write)=>save(write.expected.clone(),write.next.clone()).await.map(|_|write),Err(error)=>Err(error)};
            p.send(ExportDialogAction::PresetResult {result});
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_presets_detect_stale_windows_and_keep_embedded_profile_bytes() {
        let path =
            std::env::temp_dir().join(format!("capy-export-store-{}.json", std::process::id()));
        let empty = ExportPresets::default();
        let mut next = empty.clone();
        let mut recipe = ExportRecipe::web_share();
        recipe.profile.profile = layer_core::color::ColorProfile::Icc(
            layer_color::profile_bytes(&recipe.profile.profile)
                .unwrap()
                .into(),
        );
        next.save("My delivery", recipe.clone()).unwrap();
        write(&path, &empty, &next).unwrap();
        let readback = read(&path).unwrap();
        assert_eq!(readback.recipe(4, Default::default()).unwrap(), recipe);
        assert_eq!(write(&path, &empty, &empty).unwrap_err(), layer_ui::ColorFeatureError::PresetChanged);
        assert_eq!(read(&path).unwrap(), next);
        std::fs::write(&path, b"broken").unwrap();
        assert_eq!(read(&path).unwrap(), empty);
        assert!(write(&path, &next, &empty).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        write(&path, &empty, &next).unwrap();
        assert_eq!(read(&path).unwrap(), next);
        std::fs::remove_file(path).unwrap();
    }
}
