//! Native file transport for the shared runtime JSON/WGSL loader.
use crate::workspace_async::{AsyncTask, BlockingTask};
use layer_core::{EffectInstallMode, EffectPackage};
use layer_host::NativeHost;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

const MANIFEST_LIMIT: usize = 16 * 1024 * 1024;
const MODULE_LIMIT: usize = 1024 * 1024;
const MODULES_LIMIT: usize = 16 * 1024 * 1024;

fn merge_mode() -> EffectInstallMode {
    EffectInstallMode::Merge
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    /// None reloads the installed/overridden resources. Paths never enter the
    /// optional workspace query queue or a per-frame JSON payload.
    directory: Option<PathBuf>,
    #[serde(default = "merge_mode")]
    mode: EffectInstallMode,
    #[serde(default)]
    library: bool,
}
struct Package {
    manifest: String,
    modules: BTreeMap<Arc<str>, Arc<str>>,
    mode: EffectInstallMode,
    library: bool,
}
fn read_text(path: &Path, limit: usize) -> Result<String, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("Could not open a filter resource ({:?}).", e.kind()))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("Could not inspect a filter resource ({:?}).", e.kind()))?;
    if !metadata.is_file() {
        return Err("Filter resources must be regular files.".into());
    }
    if metadata.len() > limit as u64 {
        return Err("Filter resource exceeds package limits.".into());
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err("Filter resources cannot be file links.".into());
        }
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not read a filter resource ({:?}).", e.kind()))?;
    if bytes.len() > limit {
        return Err("Filter resource exceeds package limits.".into());
    }
    String::from_utf8(bytes).map_err(|_| "Filter resources must contain UTF-8 text.".into())
}
fn read_directory(
    directory: &Path,
    mode: EffectInstallMode,
    library: bool,
) -> Result<Package, String> {
    let root = directory
        .canonicalize()
        .map_err(|e| format!("Could not locate the filter package ({:?}).", e.kind()))?;
    let manifest = read_text(&root.join("manifest.json"), MANIFEST_LIMIT)?;
    let parsed = EffectPackage::parse(&manifest)?;
    let mut modules = BTreeMap::new();
    let mut remaining = MODULES_LIMIT;
    // Only names approved by the shared parser are ever opened.
    for name in parsed.module_names()? {
        let text = read_text(&root.join(name.as_ref()), MODULE_LIMIT.min(remaining))?;
        remaining -= text.len();
        modules.insert(name, Arc::from(text));
    }
    Ok(Package {
        manifest,
        modules,
        mode,
        library,
    })
}
fn resources(request: Request) -> Result<Option<Package>, String> {
    let directory = request
        .directory
        .or_else(|| std::env::var_os("CAPY_FILTERS_DIR").map(PathBuf::from));
    let directory = if let Some(directory) = directory {
        directory
    } else {
        let path = std::env::current_exe()
            .map_err(|_| "Could not locate installed filter resources.")?
            .parent()
            .ok_or("Could not locate installed filter resources.")?
            .join("Assets")
            .join("filters");
        if !path.exists() {
            return Ok(None);
        } // Embedded startup fallback remains usable.
        path
    };
    read_directory(&directory, request.mode, request.library).map(Some)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Status {
    request_id: u64,
    pending: bool,
    phase: &'static str,
    error: Option<String>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            request_id: 0,
            pending: false,
            phase: "idle",
            error: None,
        }
    }
}
pub(crate) struct FilterService {
    task: AsyncTask<Result<Option<Package>, String>>,
    acquired: Option<Package>,
    validating: Option<u64>,
    document_epoch: Option<u64>,
    status: Status,
}
impl FilterService {
    pub(crate) fn new(wake: impl Fn() + Send + 'static) -> Self {
        Self {
            task: AsyncTask::new(wake),
            acquired: None,
            validating: None,
            document_epoch: None,
            status: Status::default(),
        }
    }
    pub(crate) fn status(&self) -> &Status {
        &self.status
    }
    pub(crate) fn startup(&mut self, native: &mut NativeHost) {
        let mode = std::env::var("CAPY_FILTERS_MODE").unwrap_or_else(|_| "merge".into());
        match serde_json::from_value(serde_json::Value::String(mode)) {
            Ok(mode) => {
                if let Err(error) = self.load(
                    native,
                    Request {
                        directory: None,
                        mode,
                        library: true,
                    },
                ) {
                    self.failed(native, error);
                }
            }
            Err(_) => self.failed(
                native,
                "Filter installation mode must be add, replace or merge.".into(),
            ),
        }
    }
    pub(crate) fn load(&mut self, native: &mut NativeHost, request: Request) -> Result<(), String> {
        if self.status.pending || native.session.state().filter_load.pending {
            return Err("A filter package is already being loaded.".into());
        }
        let id = self
            .status
            .request_id
            .checked_add(1)
            .ok_or("Filter request identity exhausted.")?;
        let document_epoch =
            (!request.library).then_some(native.session.state().document_file.epoch);
        self.task
            .start(async move {
                let job = BlockingTask::start(move || resources(request)).map_err(|e| {
                    format!("Could not start filter file transport ({:?}).", e.kind())
                })?;
                job.await?
            })
            .map_err(|_| "Filter file transport is unavailable.")?;
        self.document_epoch = document_epoch;
        self.status = Status {
            request_id: id,
            pending: true,
            phase: "reading",
            error: None,
        };
        native.invalidate_snapshot();
        Ok(())
    }
    fn failed(&mut self, native: &mut NativeHost, error: String) {
        self.status.pending = false;
        self.status.phase = "failed";
        self.status.error = Some(error);
        self.acquired = None;
        self.validating = None;
        native.invalidate_snapshot();
    }
    pub(crate) fn poll(&mut self, native: &mut NativeHost) {
        if let Some(result) = self.task.poll() {
            match result {
                Ok(Some(package)) => {
                    self.acquired = Some(package);
                    self.status.phase = "waiting_for_canvas";
                }
                Ok(None) => {
                    self.status.pending = false;
                    self.status.phase = "embedded_fallback";
                }
                Err(error) => self.failed(native, error),
            }
            native.invalidate_snapshot();
        }
        if self.acquired.is_some()
            && self
                .document_epoch
                .is_some_and(|epoch| epoch != native.session.state().document_file.epoch)
        {
            self.failed(
                native,
                "The document changed while reading filters. Load the package again.".into(),
            );
        }
        if self.acquired.is_some()
            && native.session.renderer_mut().0.is_some()
            && native.session.can_stage_effect_package()
        {
            let package = self.acquired.take().unwrap();
            let read = |name: &str| {
                package
                    .modules
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("Missing filter module: {name}"))
            };
            let result = if package.library {
                native
                    .session
                    .load_effect_library(&package.manifest, read, package.mode)
            } else {
                native
                    .session
                    .load_effect_package(&package.manifest, read, package.mode)
            };
            match result {
                Ok(change) => {
                    native.dirty |= change.canvas_wake;
                    self.validating = Some(native.session.state().filter_load.request_id);
                    self.status.phase = "validating";
                }
                Err(error) => self.failed(native, error),
            }
            native.invalidate_snapshot();
        }
        if let Some(id) = self.validating {
            let load = &native.session.state().filter_load;
            if load.request_id == id && !load.pending {
                if let Some(error) = &load.error {
                    self.failed(native, error.clone());
                } else {
                    self.validating = None;
                    self.status.pending = false;
                    self.status.phase = "ready";
                    native.invalidate_snapshot();
                }
            }
        }
    }
    pub(crate) fn stop(&mut self) {
        self.task.close();
        self.acquired = None;
    }
}
#[cfg(test)]
#[path = "filter_package_tests.rs"]
mod tests;
