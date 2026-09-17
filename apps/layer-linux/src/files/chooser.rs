//! Desktop file dialogs with a remembered folder for each kind of file.
use super::*;
use std::{io::Read, path::PathBuf};

#[derive(Clone, Copy)]
pub(super) enum Folder {
    Artwork,
    Profiles,
    Save,
    Export,
}

fn location(folder: Folder) -> PathBuf {
    let root = std::env::var_os("LAYER_SETTINGS_FILE")
        .map(PathBuf::from)
        .and_then(|p| p.parent().map(|p| p.to_owned()))
        .unwrap_or_else(|| {
            if cfg!(test) {
                std::env::temp_dir().join(format!("capy-file-folders-{}", std::process::id()))
            } else {
                glib::user_config_dir().join("capycanvas")
            }
        });
    root.join("file-dialogs").join(match folder {
        Folder::Artwork => "artwork",
        Folder::Profiles => "profiles",
        Folder::Save => "save",
        Folder::Export => "export",
    })
}

async fn restore(dialog: &gtk::FileDialog, folder: Folder) {
    let uri = gio::spawn_blocking(move || {
        let mut uri = String::new();
        std::fs::File::open(location(folder))
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
    }
}

async fn remember(file: &gio::File, folder: Folder) {
    let Some(parent) = file.parent() else { return };
    let uri = parent.uri().to_string();
    let _ = gio::spawn_blocking(move || -> Result<(), String> {
        let path = location(folder);
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

pub(super) async fn save(
    dialog: &gtk::FileDialog,
    parent: &impl IsA<gtk::Window>,
    folder: Folder,
) -> Result<gio::File, glib::Error> {
    restore(dialog, folder).await;
    let file = dialog.save_future(Some(parent)).await?;
    remember(&file, folder).await;
    Ok(file)
}
