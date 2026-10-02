use std::sync::{Arc, OnceLock};
use layer_ui::Localizer;

#[cfg(target_os = "android")]
static LOCALIZATION: OnceLock<Arc<Localizer>> = OnceLock::new();

#[cfg(target_os = "android")]
pub(crate) fn localization(saved: &str, preferred_tags: &[&str]) -> Arc<Localizer> {
    cached_localization(&LOCALIZATION, saved, preferred_tags)
}

#[cfg(target_os = "android")]
pub(crate) fn active_localization() -> Result<&'static Arc<Localizer>, &'static str> {
    LOCALIZATION.get().ok_or("android_launch_not_initialized")
}

fn cached_localization(launch: &OnceLock<Arc<Localizer>>, saved: &str, preferred_tags: &[&str]) -> Arc<Localizer> {
    launch.get_or_init(|| layer_ui::launch_localization(saved, preferred_tags)).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recreated_hosts_keep_process_context_after_saved_language_changes() {
        let active = Localizer::shared(layer_ui::UiLanguage::Japanese);
        let launch = OnceLock::from(active.clone());
        for (saved, tags) in [(r#"{"language":{"Explicit":"en"},"theme":"dark"}"#, &["en-US"][..]),
            (r#"{"language":{"Explicit":"ko"}}"#, &["ko-KR", "zh-TW"][..])] {
            let bootstrap = cached_localization(&launch, saved, tags);
            let host = layer_host::NativeHost::launch_localized(layer_ui::Platform::Android, saved, bootstrap.clone()).unwrap();
            assert!(Arc::ptr_eq(&active, &bootstrap));
            assert!(Arc::ptr_eq(&active, host.session.localization()));
            assert_eq!(host.bootstrap_view().active_tag, "ja");
            assert!(host.session.engine().backend().0.is_none());
        }
        let fresh = OnceLock::new();
        let initial = cached_localization(&fresh, "", &["ja-JP", "en-US"]);
        assert!(Arc::ptr_eq(fresh.get().unwrap(), &initial));
        assert_eq!(initial.language(), layer_ui::UiLanguage::Japanese);
    }
}
