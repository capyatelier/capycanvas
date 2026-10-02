use std::{collections::BTreeMap, sync::{Arc, OnceLock}};
use fluent_bundle::{concurrent::FluentBundle, FluentResource};
pub use fluent_bundle::FluentArgs;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

include!(concat!(env!("OUT_DIR"), "/localization_catalogs.rs"));

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UiLanguage {
    #[serde(rename = "en")] English,
    #[serde(rename = "ja")] Japanese,
    #[serde(rename = "zh-Hans")] SimplifiedChinese,
    #[serde(rename = "zh-Hant")] TraditionalChinese,
    #[serde(rename = "ko")] Korean,
}

impl UiLanguage {
    pub const ALL: [Self; 5] = [Self::English, Self::Japanese, Self::SimplifiedChinese, Self::TraditionalChinese, Self::Korean];
    pub const fn tag(self) -> &'static str {
        match self { Self::English => "en", Self::Japanese => "ja", Self::SimplifiedChinese => "zh-Hans", Self::TraditionalChinese => "zh-Hant", Self::Korean => "ko" }
    }
    pub const fn native_name(self) -> &'static str {
        match self { Self::English => "English", Self::Japanese => "日本語", Self::SimplifiedChinese => "简体中文", Self::TraditionalChinese => "繁體中文", Self::Korean => "한국어" }
    }
}

pub const SHIPPED_LANGUAGES: &[UiLanguage] = &[UiLanguage::English];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum LanguagePreference {
    #[default] System,
    Explicit(UiLanguage),
}

/// Converts POSIX locale spelling to a language tag at the platform boundary.
pub fn normalize_language_tag(tag: &str) -> String {
    let tag = tag.trim();
    let (base, modifier) = tag.split_once('@').unwrap_or((tag, ""));
    let base = base.split('.').next().unwrap_or("");
    if base.eq_ignore_ascii_case("C") || base.eq_ignore_ascii_case("POSIX") { return "en".to_owned(); }
    let mut normalized = base.replace('_', "-");
    if base.eq_ignore_ascii_case("zh") || base.to_ascii_lowercase().starts_with("zh_") {
        let script = match modifier.to_ascii_lowercase().as_str() { "hans" => Some("Hans"), "hant" => Some("Hant"), _ => None };
        if let Some(script) = script {
            let mut parts = normalized.split('-');
            let language = parts.next().unwrap();
            let tail = parts.map(str::to_owned).collect::<Vec<_>>();
            normalized = format!("{language}-{script}");
            for part in tail { normalized.push('-'); normalized.push_str(&part); }
        }
    }
    normalized
}

pub fn resolve_language(preference: LanguagePreference, preferred_tags: &[&str]) -> UiLanguage {
    if let LanguagePreference::Explicit(language) = preference { return language; }
    for tag in preferred_tags {
        let Ok(id) = normalize_language_tag(tag).parse::<LanguageIdentifier>() else { continue };
        let script = id.script.map(|s| s.to_string());
        let selected = match id.language.as_str() {
            "en" if script.as_deref().is_none_or(|s| s == "Latn") => Some(UiLanguage::English),
            "ja" if script.as_deref().is_none_or(|s| s == "Jpan") => Some(UiLanguage::Japanese),
            "ko" if script.as_deref().is_none_or(|s| s == "Kore") => Some(UiLanguage::Korean),
            "zh" => match script.as_deref() {
                Some("Hans") => Some(UiLanguage::SimplifiedChinese),
                Some("Hant") => Some(UiLanguage::TraditionalChinese),
                Some(_) => None,
                None => Some(if id.region.is_some_and(|r| matches!(r.as_str(), "TW" | "HK" | "MO")) { UiLanguage::TraditionalChinese } else { UiLanguage::SimplifiedChinese }),
            },
            _ => None,
        };
        if let Some(language) = selected { return language; }
    }
    UiLanguage::English
}

/// Immutable launch context. Arguments are literal text without bidi isolation marks.
pub struct Localizer {
    language: UiLanguage,
    active: FluentBundle<FluentResource>,
    english: FluentBundle<FluentResource>,
    labels: BTreeMap<MessageId, Arc<str>>,
}

fn bundle(language: UiLanguage) -> FluentBundle<FluentResource> {
    let sources = CATALOGS.iter().find(|(tag, _)| *tag == language.tag()).unwrap().1;
    let mut bundle = FluentBundle::new_concurrent(vec![language.tag().parse().unwrap()]);
    bundle.set_use_isolating(false);
    for (domain, source) in sources {
        let resource = FluentResource::try_new((*source).to_owned()).unwrap_or_else(|(_, errors)| panic!("Invalid {} {domain} catalog: {errors:?}", language.tag()));
        bundle.add_resource(resource).unwrap_or_else(|errors| panic!("Duplicate {} {domain} catalog entries: {errors:?}", language.tag()));
    }
    bundle
}

fn render(bundle: &FluentBundle<FluentResource>, id: MessageId, args: Option<&FluentArgs<'_>>) -> Result<String, String> {
    let pattern = bundle.get_message(id.key()).and_then(|message| message.value()).ok_or_else(|| "missing message".to_owned())?;
    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, args, &mut errors);
    if errors.is_empty() { Ok(text.into_owned()) } else { Err(format!("{errors:?}")) }
}

impl std::fmt::Debug for Localizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Localizer").field("language", &self.language).finish_non_exhaustive()
    }
}

impl Localizer {
    pub fn shared(language: UiLanguage) -> Arc<Self> {
        static EN: OnceLock<Arc<Localizer>> = OnceLock::new();
        static JA: OnceLock<Arc<Localizer>> = OnceLock::new();
        static HANS: OnceLock<Arc<Localizer>> = OnceLock::new();
        static HANT: OnceLock<Arc<Localizer>> = OnceLock::new();
        static KO: OnceLock<Arc<Localizer>> = OnceLock::new();
        let slot = match language { UiLanguage::English => &EN, UiLanguage::Japanese => &JA, UiLanguage::SimplifiedChinese => &HANS, UiLanguage::TraditionalChinese => &HANT, UiLanguage::Korean => &KO };
        Arc::clone(slot.get_or_init(|| Arc::new(Self::new(language))))
    }
    pub fn new(language: UiLanguage) -> Self {
        let mut localizer = Self { language, active: bundle(language), english: bundle(UiLanguage::English), labels: BTreeMap::new() };
        for &id in MessageId::ALL {
            match render(&localizer.active, id, None) {
                Ok(text) => { localizer.labels.insert(id, Arc::from(text)); }
                Err(error) => {
                    if let Ok(text) = render(&localizer.english, id, None) {
                        eprintln!("Localization {} {}: {error}", language.tag(), id.key());
                        localizer.labels.insert(id, Arc::from(text));
                    }
                }
            }
        }
        localizer
    }
    pub const fn language(&self) -> UiLanguage { self.language }
    /// Returns a warmed parameterless label; argument-bearing messages use `format`.
    pub fn text(&self, id: MessageId) -> Arc<str> {
        self.labels.get(&id).cloned().unwrap_or_else(|| {
            if cfg!(debug_assertions) { panic!("Message {} requires named arguments", id.key()); }
            Arc::clone(self.labels.get(&MessageId::COMMON_ERROR).expect("Generic failure label must be cached"))
        })
    }
    pub fn format(&self, id: MessageId, args: &FluentArgs<'_>) -> String {
        match render(&self.active, id, Some(args)) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("Localization {} {}: {error}", self.language.tag(), id.key());
                match render(&self.english, id, Some(args)) {
                    Ok(text) => text,
                    Err(error) => {
                        eprintln!("Localization en {}: {error}", id.key());
                        if cfg!(debug_assertions) { panic!("English localization failure for {}: {error}", id.key()); }
                        render(&self.active, MessageId::COMMON_ERROR, None).or_else(|_| render(&self.english, MessageId::COMMON_ERROR, None)).expect("Generic failure message must format")
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_language_preferences_and_explicit_scripts() {
        use UiLanguage::*;
        for (tag, expected) in [("ja-JP", Japanese), ("ko-KR", Korean), ("zh-CN", SimplifiedChinese), ("zh-SG", SimplifiedChinese), ("zh-TW", TraditionalChinese), ("zh-HK", TraditionalChinese), ("zh-Hant-CN", TraditionalChinese), ("zh-Hans-TW", SimplifiedChinese), ("zh", SimplifiedChinese), ("ja_JP.UTF-8", Japanese), ("zh_TW.UTF-8@hans", SimplifiedChinese)] {
            assert_eq!(resolve_language(LanguagePreference::System, &[tag]), expected, "{tag}");
        }
        assert_eq!(resolve_language(LanguagePreference::System, &["fr", "zh-Latn", "ko", "ja"]), Korean);
        assert_eq!(resolve_language(LanguagePreference::System, &["en-Cyrl", "ja"]), Japanese);
        assert_eq!(resolve_language(LanguagePreference::System, &["fr", "invalid!"]), English);
        assert_eq!(resolve_language(LanguagePreference::Explicit(Japanese), &["ko"]), Japanese);
        assert_eq!(resolve_language(LanguagePreference::System, &["en", "ja"]), English);
        for tag in ["C", "POSIX", "C.UTF-8"] {
            assert_eq!(resolve_language(LanguagePreference::System, &[tag, "ja"]), English);
        }
    }

    #[test]
    fn language_tags_round_trip() {
        for language in UiLanguage::ALL {
            let json = serde_json::to_string(&language).unwrap();
            assert_eq!(json, format!("{:?}", language.tag()));
            assert_eq!(serde_json::from_str::<UiLanguage>(&json).unwrap(), language);
        }
        assert_eq!(LanguagePreference::default(), LanguagePreference::System);
    }

    #[test]
    fn labels_and_contexts_are_cached_and_thread_safe() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Localizer>();
        let ja = Localizer::shared(UiLanguage::Japanese);
        assert!(Arc::ptr_eq(&ja, &Localizer::shared(UiLanguage::Japanese)));
        assert!(Arc::ptr_eq(&ja.text(MessageId::COMMON_CANCEL), &ja.text(MessageId::COMMON_CANCEL)));
        let ko = Localizer::shared(UiLanguage::Korean);
        let worker = std::thread::spawn(move || ko.text(MessageId::COMMON_CANCEL));
        assert_eq!(&*ja.text(MessageId::COMMON_CANCEL), "キャンセル");
        assert_eq!(&*worker.join().unwrap(), "취소");
    }

    fn fixture(source: &str) -> FluentBundle<FluentResource> {
        let mut bundle = FluentBundle::new_concurrent(vec!["en".parse().unwrap()]);
        bundle.set_use_isolating(false);
        bundle.add_resource(FluentResource::try_new(source.to_owned()).unwrap()).unwrap();
        bundle
    }

    #[test]
    fn missing_and_broken_translation_retry_english() {
        let mut localizer = Localizer::new(UiLanguage::Japanese);
        localizer.active = fixture("common-error = 問題\n");
        assert_eq!(localizer.format(MessageId::COMMON_SAVE, &FluentArgs::new()), "Save");
        localizer.active = fixture("common-save = { $missing }\ncommon-error = 問題\n");
        assert_eq!(localizer.format(MessageId::COMMON_SAVE, &FluentArgs::new()), "Save");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "requires named arguments")]
    fn argument_bearing_messages_cannot_format_through_text() {
        let mut localizer = Localizer::new(UiLanguage::English);
        localizer.active = fixture("common-save = Save { $name }\n");
        localizer.labels.remove(&MessageId::COMMON_SAVE);
        localizer.text(MessageId::COMMON_SAVE);
    }

    #[test]
    fn named_user_arguments_remain_literal() {
        let bundle = fixture("common-save = Save { $name }\n");
        for name in ["日本語 한글 繁體 😀", "a{b} \"quoted\".capy", "\u{2068}user\u{2069}"] {
            let mut args = FluentArgs::new();
            args.set("name", name);
            assert_eq!(render(&bundle, MessageId::COMMON_SAVE, Some(&args)).unwrap(), format!("Save {name}"));
        }
    }
}
