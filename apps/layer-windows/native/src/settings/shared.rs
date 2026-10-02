//! Preferences shared by the native windows using the same private profile.
//! Rendering owners only take short state locks; disk work has its own lock.
use super::*;
use std::collections::HashMap;
use std::sync::{OnceLock, Weak};

type Wake = Mutex<Option<Box<dyn Fn() + Send>>>;
pub(super) struct Hub {
    state: Mutex<State>,
    file: Mutex<SettingsFile>,
    localization: Arc<layer_ui::Localizer>,
}
struct State {
    settings: Settings,
    bytes: Vec<u8>,
    load_error: Option<String>,
    dirty: bool,
    listeners: Vec<Weak<Wake>>,
}
impl Hub {
    pub(super) fn with_launch(file: SettingsFile, launch: impl FnOnce(Option<&str>) -> Result<NativeHost, String>) -> Result<(NativeHost, Arc<Self>), String> {
        static PROFILES: OnceLock<Mutex<HashMap<PathBuf, Arc<Hub>>>> = OnceLock::new();
        let mut profiles = PROFILES.get_or_init(Default::default).lock().unwrap();
        if let Some(hub) = profiles.get(&file.directory).cloned() {
            let saved = String::from_utf8(hub.state.lock().unwrap().bytes.clone()).map_err(|error| error.to_string())?;
            let native = NativeHost::launch_localized(layer_ui::Platform::Windows, &saved, hub.localization.clone())?;
            return Ok((native, hub));
        }
        let (saved, load_error) = match file.load_saved() {
            Ok(value) => (value, None),
            Err(error) => (None, Some(format!("{error} Defaults are in use; the next change replaces the saved file."))),
        };
        let native = launch(saved.as_deref())?;
        let settings = native.session.state().settings.clone();
        let bytes = encode(&settings)?;
        let key = file.directory.clone();
        let hub = Arc::new(Self {
            localization: native.session.localization().clone(),
            state: Mutex::new(State { settings, bytes, load_error, dirty: false, listeners: Vec::new() }),
            file: Mutex::new(file),
        });
        profiles.insert(key, hub.clone());
        Ok((native, hub))
    }
    #[cfg(test)]
    pub(super) fn open(file: SettingsFile, defaults: Settings) -> Result<Arc<Self>, String> {
        Self::with_launch(file, |saved| {
            let mut native = NativeHost::new_localized(layer_ui::Platform::Windows, layer_ui::Localizer::shared(layer_ui::UiLanguage::English))?;
            let settings = saved.map(|saved| Settings::restore_localized(saved, native.session.localization())).unwrap_or(defaults);
            native.dispatch(UiAction::RestoreSettings { settings })?;
            Ok(native)
        }).map(|(_, hub)| hub)
    }
    pub(super) fn load_error(&self) -> Option<String> {
        self.state.lock().unwrap().load_error.clone()
    }
    pub(super) fn dirty(&self) -> bool {
        self.state.lock().unwrap().dirty
    }
    pub(super) fn write(&self, bytes: &[u8]) -> Result<(), String> {
        // Serialize replacement across all windows. A newer accepted edit will
        // either supersede this job before it starts or replace it afterwards.
        let mut file = self.file.lock().unwrap();
        if self.state.lock().unwrap().bytes != bytes {
            return Ok(());
        }
        file.write(bytes)?;
        let mut state = self.state.lock().unwrap();
        state.load_error = None;
        if state.bytes == bytes {
            state.dirty = false;
        }
        Ok(())
    }
}
pub(super) struct Subscription {
    pub(super) hub: Arc<Hub>,
    wake: Arc<Wake>,
    baseline: Settings,
}
impl Subscription {
    pub(super) fn new(hub: Arc<Hub>, wake: impl Fn() + Send + 'static) -> Self {
        let wake: Arc<Wake> = Arc::new(Mutex::new(Some(Box::new(wake))));
        let baseline = {
            let mut state = hub.state.lock().unwrap();
            state
                .listeners
                .retain(|listener| listener.strong_count() != 0);
            state.listeners.push(Arc::downgrade(&wake));
            state.settings.clone()
        };
        Self {
            hub,
            wake,
            baseline,
        }
    }
    pub(super) fn notifier(&self) -> impl Fn() + Send + 'static {
        let wake = self.wake.clone();
        move || notify(&wake)
    }
    pub(super) fn adopt(&mut self, current: &Settings) -> Option<Settings> {
        let state = self.hub.state.lock().unwrap();
        if self.baseline != state.settings {
            self.baseline = state.settings.clone();
        }
        (current != &self.baseline).then(|| self.baseline.clone())
    }
    pub(super) fn edit(&mut self, desired: &Settings) -> Result<Vec<u8>, String> {
        let (bytes, listeners) = {
            let mut state = self.hub.state.lock().unwrap();
            // Other render owners may have edited preferences since this window
            // last adopted them. Apply only this window's changed fields/keys.
            let mut value = serde_json::to_value(&state.settings).map_err(|e| e.to_string())?;
            merge(
                &serde_json::to_value(&self.baseline).map_err(|e| e.to_string())?,
                &serde_json::to_value(desired).map_err(|e| e.to_string())?,
                &mut value,
            );
            let settings: Settings = serde_json::from_value(value).map_err(|e| e.to_string())?;
            settings.validate_localized(&self.hub.localization)?;
            let bytes = encode(&settings)?;
            let changed = state.settings != settings;
            state.dirty |= changed;
            state.settings = settings;
            state.bytes = bytes.clone();
            self.baseline = state.settings.clone();
            let listeners = if changed {
                state.listeners.iter().filter_map(Weak::upgrade).collect()
            } else {
                Vec::new()
            };
            (bytes, listeners)
        };
        // Never call a host callback while holding shared state. Each notifier's
        // lock also fences disconnection against an already-running callback.
        for listener in listeners {
            notify(&listener);
        }
        Ok(bytes)
    }
    pub(super) fn stop(&self) {
        *self.wake.lock().unwrap() = None;
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.stop();
    }
}
fn notify(wake: &Wake) {
    if let Some(wake) = &*wake.lock().unwrap() {
        wake();
    }
}
fn merge(before: &serde_json::Value, desired: &serde_json::Value, current: &mut serde_json::Value) {
    if before == desired {
        return;
    }
    if let (Some(before), Some(desired), Some(current)) = (
        before.as_object(),
        desired.as_object(),
        current.as_object_mut(),
    ) {
        for key in before.keys() {
            if !desired.contains_key(key) {
                current.remove(key);
            }
        }
        for (key, desired) in desired {
            if before.get(key) == Some(desired) {
                continue;
            }
            if let (Some(before), Some(current)) = (before.get(key), current.get_mut(key)) {
                merge(before, desired, current);
            } else {
                current.insert(key.clone(), desired.clone());
            }
        }
    } else {
        *current = desired.clone();
    }
}
