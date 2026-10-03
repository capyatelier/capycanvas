use std::sync::Arc;
use layer_ui::Localizer;
#[cfg(target_os = "android")]
use std::sync::Mutex;
#[cfg(any(target_os = "android", test))]
use layer_ui::LanguagePreference;

#[cfg(any(target_os = "android", test))]
#[derive(Default)]
struct ApplicationLanguage {
    context: Option<Arc<Localizer>>,
    preference: Option<LanguagePreference>,
}

#[cfg(any(target_os = "android", test))]
impl ApplicationLanguage {
    fn publish_if_current(&mut self, preference: LanguagePreference, publish: impl FnOnce() -> Option<Arc<Localizer>>) -> Option<Arc<Localizer>> {
        if self.preference != Some(preference) { return None; }
        let context = publish()?;
        self.context = Some(context.clone());
        Some(context)
    }
}

#[cfg(target_os = "android")]
static LOCALIZATION: Mutex<ApplicationLanguage> = Mutex::new(ApplicationLanguage { context: None, preference: None });

#[cfg(target_os = "android")]
pub(crate) fn localization(saved: &str, preferred_tags: &[&str]) -> Arc<Localizer> {
    let mut state = LOCALIZATION.lock().unwrap();
    let preference = *state.preference.get_or_insert_with(|| layer_ui::Settings::language_preference(saved));
    let language = layer_ui::resolve_launch_language(preference, preferred_tags);
    if state.context.as_ref().is_none_or(|context| context.language() != language) {
        state.context = Some(Localizer::shared(language));
    }
    state.context.as_ref().unwrap().clone()
}

#[cfg(target_os = "android")]
pub(crate) fn preference(preference: LanguagePreference) {
    LOCALIZATION.lock().unwrap().preference = Some(preference);
}

#[cfg(target_os = "android")]
pub(crate) fn current_preference() -> Option<LanguagePreference> {
    LOCALIZATION.lock().unwrap().preference
}

#[cfg(target_os = "android")]
pub(crate) fn publish(preference: LanguagePreference, publish: impl FnOnce() -> Option<Arc<Localizer>>) -> Option<Arc<Localizer>> {
    LOCALIZATION.lock().unwrap().publish_if_current(preference, publish)
}

#[cfg(target_os = "android")]
pub(crate) fn active_localization() -> Result<Arc<Localizer>, &'static str> {
    LOCALIZATION.lock().unwrap().context.clone().ok_or("android_launch_not_initialized")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_application_intent_rejects_stale_publication_without_consuming_it() {
        let previous = Localizer::shared(layer_ui::UiLanguage::Japanese);
        let current = Localizer::shared(layer_ui::UiLanguage::Korean);
        let older = LanguagePreference::Explicit(layer_ui::UiLanguage::Japanese);
        let latest = LanguagePreference::Explicit(layer_ui::UiLanguage::Korean);
        let mut state = ApplicationLanguage { context: Some(previous.clone()), preference: Some(latest) };
        assert!(state.publish_if_current(older, || panic!("stale publication consumed a pending context")).is_none());
        assert!(Arc::ptr_eq(state.context.as_ref().unwrap(), &previous));
        assert!(Arc::ptr_eq(&state.publish_if_current(latest, || Some(current.clone())).unwrap(), &current));
        assert!(Arc::ptr_eq(state.context.as_ref().unwrap(), &current));
        assert_eq!(state.preference, Some(latest));
    }

    #[test]
    fn application_context_can_change_without_recreating_the_host() {
        let initial = Localizer::shared(layer_ui::UiLanguage::Japanese);
        let mut host = layer_host::NativeHost::launch_localized(layer_ui::Platform::Android, "", initial.clone()).unwrap();
        let next = Localizer::shared(layer_ui::UiLanguage::Korean);
        assert!(host.set_localization(next.clone()));
        assert!(Arc::ptr_eq(host.session.localization(), &next));
        assert_eq!(host.bootstrap_view().active_tag, "ko");
        assert!(host.session.engine().backend().0.is_none());
        assert!(!host.set_localization(next));
    }
}
