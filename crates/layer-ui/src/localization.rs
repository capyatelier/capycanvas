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

pub const SHIPPED_LANGUAGES: &[UiLanguage] = &UiLanguage::ALL;

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
    preferred_tags.iter().find_map(|tag| matching_language(tag)).unwrap_or(UiLanguage::English)
}

pub fn resolve_launch_language(preference: LanguagePreference, preferred_tags: &[&str]) -> UiLanguage {
    resolve_available_language(preference, preferred_tags, SHIPPED_LANGUAGES)
}

fn resolve_available_language(preference: LanguagePreference, preferred_tags: &[&str], available: &[UiLanguage]) -> UiLanguage {
    if let LanguagePreference::Explicit(language) = preference
        && available.contains(&language)
    { return language; }
    preferred_tags.iter().filter_map(|tag| matching_language(tag)).find(|language| available.contains(language)).unwrap_or(UiLanguage::English)
}

fn matching_language(tag: &str) -> Option<UiLanguage> {
    let id = normalize_language_tag(tag).parse::<LanguageIdentifier>().ok()?;
    let script = id.script.map(|s| s.to_string());
    match id.language.as_str() {
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
    }
}

pub fn launch_localization(saved: &str, preferred_tags: &[&str]) -> Arc<Localizer> {
    Localizer::shared(resolve_launch_language(crate::Settings::language_preference(saved), preferred_tags))
}

#[derive(Clone, Debug, Serialize)]
pub struct BootstrapView {
    pub active_tag: &'static str,
    pub common: crate::CommonCopy,
    pub recovery: crate::RecoveryCopy,
    pub shipped_tags: Vec<&'static str>,
    pub preparing_brush: Arc<str>,
    pub preparing_canvas: Arc<str>,
    pub canvas_init_failed: Arc<str>,
    pub restart_canvas: Arc<str>,
    pub application_start_failed: Arc<str>,
    pub action_failed: Arc<str>,
    pub editor_closed: Arc<str>,
    pub cannot_open_file: Arc<str>,
    pub ok: Arc<str>,
    pub drawing_workspace: Arc<str>,
    pub drawing_canvas: Arc<str>,
    pub drawing_canvas_help: Arc<str>,
    pub canvas_availability: Arc<str>,
    pub starting_canvas: Arc<str>,
    pub application_menus: Arc<str>,
    pub workspace_controls: Arc<str>,
    pub canvas_status: Arc<str>,
    pub starting_webgpu: Arc<str>,
    pub restarting_canvas: Arc<str>,
    pub canvas_recovery_failed: Arc<str>,
    pub canvas_stopped: Arc<str>,
    pub canvas_restart_failed: Arc<str>,
    pub opening_files: Arc<str>,
    pub canvas_ready: Arc<str>,
    pub preparing_document: Arc<str>,
    pub loading_filters: Arc<str>,
    pub painting_unavailable_save: Arc<str>,
}

pub fn bootstrap_view(l: &Localizer) -> BootstrapView {
    BootstrapView {
        active_tag: l.language().tag(),
        common: crate::CommonCopy::new(l),
        recovery: crate::RecoveryCopy::new(l),
        shipped_tags: SHIPPED_LANGUAGES.iter().map(|language| language.tag()).collect(),
        preparing_brush: l.text(MessageId::COMMON_PREPARING_BRUSH),
        preparing_canvas: l.text(MessageId::COMMON_PREPARING_CANVAS),
        canvas_init_failed: l.text(MessageId::COMMON_CANVAS_INIT_FAILED),
        restart_canvas: l.text(MessageId::COMMON_RESTART_CANVAS),
        application_start_failed: l.text(MessageId::COMMON_APPLICATION_START_FAILED),
        action_failed: l.text(MessageId::COMMON_ACTION_FAILED),
        editor_closed: l.text(MessageId::COMMON_EDITOR_CLOSED),
        cannot_open_file: l.text(MessageId::COMMON_CANNOT_OPEN_FILE),
        ok: l.text(MessageId::COMMON_OK),
        drawing_workspace: l.text(MessageId::COMMON_DRAWING_WORKSPACE),
        drawing_canvas: l.text(MessageId::COMMON_DRAWING_CANVAS),
        drawing_canvas_help: l.text(MessageId::COMMON_DRAWING_CANVAS_HELP),
        canvas_availability: l.text(MessageId::COMMON_CANVAS_AVAILABILITY),
        starting_canvas: l.text(MessageId::COMMON_STARTING_CANVAS),
        application_menus: l.text(MessageId::COMMON_APPLICATION_MENUS),
        workspace_controls: l.text(MessageId::COMMON_WORKSPACE_CONTROLS),
        canvas_status: l.text(MessageId::COMMON_CANVAS_STATUS),
        starting_webgpu: l.text(MessageId::COMMON_STARTING_WEBGPU),
        restarting_canvas: l.text(MessageId::COMMON_RESTARTING_CANVAS),
        canvas_recovery_failed: l.text(MessageId::COMMON_CANVAS_RECOVERY_FAILED),
        canvas_stopped: l.text(MessageId::COMMON_CANVAS_STOPPED),
        canvas_restart_failed: l.text(MessageId::COMMON_CANVAS_RESTART_FAILED),
        opening_files: l.text(MessageId::COMMON_OPENING_FILES),
        canvas_ready: l.text(MessageId::COMMON_CANVAS_READY),
        preparing_document: l.text(MessageId::COMMON_PREPARING_DOCUMENT),
        loading_filters: l.text(MessageId::COMMON_LOADING_FILTERS),
        painting_unavailable_save: l.text(MessageId::COMMON_PAINTING_UNAVAILABLE_SAVE),
    }
}

pub fn file_open_failure(l: &Localizer, name: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("name", name);
    l.format(MessageId::COMMON_OPEN_FILE_FAILED, &args)
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
        Self::from_bundles(language, bundle(language), bundle(UiLanguage::English))
    }
    fn from_bundles(language: UiLanguage, active: FluentBundle<FluentResource>, english: FluentBundle<FluentResource>) -> Self {
        let mut localizer = Self { language, active, english, labels: BTreeMap::new() };
        for &id in MessageId::STATIC {
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
    pub fn static_message(&self, key: &str) -> Option<MessageId> {
        self.labels.get_key_value(key).map(|(&id, _)| id)
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
    fn bootstrap_copy_uses_warmed_text_and_literal_file_names() {
        let localization = launch_localization("", &["en-US"]);
        let first = bootstrap_view(&localization);
        let second = bootstrap_view(&localization);
        assert!(Arc::ptr_eq(&first.preparing_canvas, &second.preparing_canvas));
        assert_eq!(first.active_tag, "en");
        assert_eq!(first.shipped_tags, UiLanguage::ALL.map(UiLanguage::tag));
        let name = "作品{draft}\"🎨\u{2068}literal\u{2069}";
        assert_eq!(file_open_failure(&localization, name), format!("Could not open “{name}”."));
    }

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
    fn launch_negotiation_never_selects_an_unshipped_catalog() {
        for language in UiLanguage::ALL {
            let expected = if SHIPPED_LANGUAGES.contains(&language) { language } else { UiLanguage::English };
            assert_eq!(resolve_launch_language(LanguagePreference::Explicit(language), &["ja"]), expected);
            assert_eq!(resolve_launch_language(LanguagePreference::System, &[language.tag()]), expected);
        }
        assert_eq!(resolve_available_language(LanguagePreference::System, &["fr", "ko", "ja"], &[UiLanguage::English, UiLanguage::Japanese]), UiLanguage::Japanese);
        assert_eq!(resolve_available_language(LanguagePreference::Explicit(UiLanguage::Korean), &["fr", "ja"], &[UiLanguage::English, UiLanguage::Japanese]), UiLanguage::Japanese);
        assert_eq!(resolve_available_language(LanguagePreference::System, &["en-Cyrl", "zh-Latn", "zh-Hant"], &[UiLanguage::English, UiLanguage::TraditionalChinese]), UiLanguage::TraditionalChinese);
        assert_eq!(resolve_available_language(LanguagePreference::System, &["C", "ja"], &[UiLanguage::English, UiLanguage::Japanese]), UiLanguage::English);
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
    fn numeric_edits_use_the_active_context_and_keep_refused_state() {
        let english = "tool-control-selection-brush-hardness = Hardness\ntool-control-selection-brush-opacity = Opacity\nnumeric-range = { $label } is out of range.\ntool-control-selection-brush-size = Size\nnumeric-whole-pixels = { $label } needs whole pixels.\ntool-control-gap-closing = Close gaps\n";
        let japanese = "tool-control-selection-brush-hardness = 硬さ\ntool-control-selection-brush-opacity = 不透明度\nnumeric-range = { $label } は範囲外です。\ntool-control-selection-brush-size = サイズ１２\nnumeric-whole-pixels = { $label } は整数のピクセルで指定してください。\ntool-control-gap-closing = 隙間１２\n";
        let active = Localizer::from_bundles(UiLanguage::Japanese, fixture(japanese), fixture(english));
        let canonical = Localizer::from_bundles(UiLanguage::English, fixture(english), fixture(english));
        let mut capture = crate::WorkspaceCapture {
            history: crate::LayoutHistory::new(&crate::DockLayout::default()),
            working: crate::WorkspaceWorkingState::default(),
        };
        capture.working.selection.brush = serde_json::from_value(serde_json::json!({"size": 0.})).unwrap();
        let before_capture = capture.clone();
        let structural = capture.validate_structure().unwrap_err();
        let admission = crate::PreparedWorkspace::new(capture.clone()).err().unwrap();
        assert_eq!(structural, admission);
        assert_eq!(admission.message(&active), "サイズ１２ は範囲外です。");
        assert_eq!(admission.message(&canonical), "Size is out of range.");
        assert_eq!(capture, before_capture);
        let mut brush = crate::SelectionBrushOptions::default();
        let before = brush.clone();
        assert_eq!(brush.edit("selection_brush_size", 0., &active).unwrap_err(), "サイズ１２ は範囲外です。");
        assert_eq!(brush, before);
        assert_eq!(brush.edit("selection_brush_size", 0., &canonical).unwrap_err(), "Size is out of range.");
        brush.edit("selection_brush_size", 48., &active).unwrap();
        assert_eq!(brush.controls(&active).iter().find(|control| control.id == "selection_brush_size").unwrap().value, 48.);
    }

    #[test]
    fn translated_argument_omission_does_not_change_static_resource_eligibility() {
        let english = "common-error = Error\nresources-animated-tooltip = { $name }\n";
        let translated = "common-error = 問題\nresources-animated-tooltip = 固定の説明\n";
        for (language, active) in [(UiLanguage::English, english), (UiLanguage::Japanese, translated)] {
            let localizer = Localizer::from_bundles(language, fixture(active), fixture(english));
            assert!(localizer.static_message(MessageId::RESOURCES_ANIMATED_TOOLTIP.key()).is_none());
            assert_eq!(localizer.static_message(MessageId::COMMON_ERROR.key()), Some(MessageId::COMMON_ERROR));
            let mut args = FluentArgs::new(); args.set("name", "日本語 {literal}");
            assert_eq!(localizer.format(MessageId::RESOURCES_ANIMATED_TOOLTIP, &args), if language == UiLanguage::English { "日本語 {literal}" } else { "固定の説明" });
        }
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
