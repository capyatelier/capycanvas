//! Where an installation keeps its files. Each host supplies its platform's
//! folder for every kind of file; the place of each store within them is named here.
use std::path::{Path, PathBuf};

pub const STORAGE_OVERRIDE: &str = "CAPY_STORAGE_DIR";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageRoots {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
    pub temp: PathBuf,
}

impl StorageRoots {
    pub fn within(directory: &Path) -> Self {
        Self {
            config: directory.join("config"),
            data: directory.join("data"),
            state: directory.join("state"),
            cache: directory.join("cache"),
            temp: directory.join("temp"),
        }
    }

    /// `CAPY_STORAGE_DIR` replaces the platform folders with one private folder;
    /// a relative name is a folder inside the platform's temporary folder, which
    /// tests of sandboxed apps can name without knowing the app's container.
    /// The installation's temporary folder is set from the result.
    pub fn resolve(platform: impl FnOnce() -> Result<Self, String>) -> Result<Self, String> {
        let roots = Self::chosen(std::env::var_os(STORAGE_OVERRIDE).map(PathBuf::from), platform)?;
        if [&roots.config, &roots.data, &roots.state, &roots.cache, &roots.temp]
            .iter()
            .any(|path| !path.is_absolute())
        {
            return Err("App storage folders must be absolute paths".into());
        }
        layer_core::temp_files::set_directory(roots.temp.clone())?;
        Ok(roots)
    }

    fn chosen(directory: Option<PathBuf>, platform: impl FnOnce() -> Result<Self, String>) -> Result<Self, String> {
        Ok(match directory {
            Some(directory) if directory.is_absolute() => Self::within(&directory),
            Some(name) => Self::within(&platform()?.temp.join(name)),
            None => platform()?,
        })
    }

    /// Removes what earlier processes left in the temporary folder. Only the
    /// process that owns the installation calls this, before it creates any.
    pub fn clear_temp(&self) -> Result<(), String> {
        match std::fs::remove_dir_all(&self.temp) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(format!("Cannot clear temporary files: {error}")),
            _ => Ok(()),
        }
    }

    pub fn settings(&self) -> PathBuf {
        self.config.join("settings.json")
    }
    pub fn export_presets(&self) -> PathBuf {
        self.config.join("export-presets")
    }
    pub fn color_profiles(&self) -> PathBuf {
        self.data.join("color-profiles")
    }
    pub fn workspaces(&self) -> PathBuf {
        self.data.join("workspaces")
    }
    pub fn sessions(&self) -> PathBuf {
        self.state.join("sessions")
    }
    pub fn file_dialogs(&self) -> PathBuf {
        self.state.join("file-dialogs")
    }
    pub fn shaders(&self) -> PathBuf {
        self.cache.join("shaders")
    }
    /// The latest copied image other apps read; it outlives the process that copied it.
    pub fn clipboard(&self) -> PathBuf {
        self.cache.join("clipboard")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_private_folder_separates_every_kind_of_file() {
        let root = Path::new("/private/capy");
        let roots = StorageRoots::within(root);
        let stores = [
            roots.settings(),
            roots.export_presets(),
            roots.color_profiles(),
            roots.workspaces(),
            roots.sessions(),
            roots.file_dialogs(),
            roots.shaders(),
            roots.clipboard(),
            roots.temp.clone(),
        ];
        assert!(stores.iter().all(|path| path.starts_with(root)));
        assert_eq!(stores.iter().collect::<std::collections::BTreeSet<_>>().len(), stores.len());
        assert!(roots.sessions().starts_with(&roots.state));
        assert!(roots.workspaces().starts_with(&roots.data));
    }

    #[test]
    fn a_relative_override_is_a_folder_in_the_platform_temporary_folder() {
        let platform = || Ok(StorageRoots::within(Path::new("/app")));
        assert_eq!(
            StorageRoots::chosen(Some("capy-test".into()), platform).unwrap(),
            StorageRoots::within(Path::new("/app/temp/capy-test"))
        );
        assert_eq!(
            StorageRoots::chosen(Some("/private".into()), || Err("unused".into())).unwrap(),
            StorageRoots::within(Path::new("/private"))
        );
    }

    #[test]
    fn relative_platform_folders_are_refused() {
        let relative = StorageRoots { state: PathBuf::from("state"), ..StorageRoots::within(Path::new("/capy")) };
        assert!(StorageRoots::resolve(|| Ok(relative)).is_err());
    }
}
