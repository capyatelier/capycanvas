//! Desktop file dialogs with a remembered folder for each kind of file.
use super::*;
use std::{io::Read, path::PathBuf};

#[derive(Clone, Copy)]
pub(super) enum Folder {
    Artwork,
    Profiles,
    Lookup,
    Save,
    Export,
}

fn location(folder: Folder) -> Option<PathBuf> {
    Some(crate::storage::roots()?.file_dialogs().join(match folder {
        Folder::Artwork => "artwork",
        Folder::Profiles => "profiles",
        Folder::Lookup => "lookup",
        Folder::Save => "save",
        Folder::Export => "export",
    }))
}

async fn restore(dialog: &gtk::FileDialog, folder: Folder) {
    let uri = gio::spawn_blocking(move || {
        let mut uri = String::new();
        std::fs::File::open(location(folder)?)
            .ok()?
            .take(16384)
            .read_to_string(&mut uri)
            .ok()?;
        let file = gio::File::for_uri(&uri);
        file.path()?.is_dir().then_some(uri)
    })
    .await
    .ok()
    .flatten();
    if let Some(uri) = uri {
        dialog.set_initial_folder(Some(&gio::File::for_uri(&uri)));
    } else if matches!(folder,Folder::Lookup) {
        dialog.set_initial_folder(Some(&gio::File::for_path(glib::user_special_dir(glib::UserDirectory::Downloads).unwrap_or_else(glib::home_dir))));
    }
}

async fn remember(file: &gio::File, folder: Folder) {
    let Some(parent) = file.parent() else { return };
    let uri = parent.uri().to_string();
    let _ = gio::spawn_blocking(move || -> Result<(), String> {
        let Some(path) = location(folder) else { return Ok(()) };
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        layer_core::atomic_write(&path, |file| {
            use std::io::Write;
            file.write_all(uri.as_bytes()).map_err(|e| e.to_string())
        })
    })
    .await;
}

pub(super) async fn open(
    dialog: &gtk::FileDialog,
    parent: &impl IsA<gtk::Window>,
    folder: Folder,
) -> Result<gio::File, glib::Error> {
    restore(dialog, folder).await;
    let file = dialog.open_future(Some(parent)).await?;
    remember(&file, folder).await;
    Ok(file)
}

pub(super) async fn open_multiple(
    dialog: &gtk::FileDialog,
    parent: &impl IsA<gtk::Window>,
    folder: Folder,
) -> Result<gio::ListModel, glib::Error> {
    restore(dialog, folder).await;
    let files = dialog.open_multiple_future(Some(parent)).await?;
    if let Some(file) = files.item(0).and_downcast::<gio::File>() {
        remember(&file, folder).await;
    }
    Ok(files)
}

#[cfg(test)]
thread_local! {
    static NEXT_SAVE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Native tests answer the next save dialog with this file, without a desktop portal.
#[cfg(test)]
pub(crate) fn choose_next_save(path: PathBuf) {
    NEXT_SAVE.set(Some(path));
}

pub(super) async fn save(
    dialog: &gtk::FileDialog,
    parent: &impl IsA<gtk::Window>,
    folder: Folder,
) -> Result<gio::File, glib::Error> {
    #[cfg(test)]
    if let Some(path) = NEXT_SAVE.take() {
        return Ok(gio::File::for_path(path));
    }
    restore(dialog, folder).await;
    let file = dialog.save_future(Some(parent)).await?;
    remember(&file, folder).await;
    Ok(file)
}
